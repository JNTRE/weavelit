//! Server-owned, transport-independent administration workflows.

#![allow(dead_code)]

use std::{error::Error as StdError, fmt};

use weavelit_server_administration::{AdministrationAction, AuthorizedAdministrationAction};
use weavelit_server_audit::{
    AuditActor, AuditEvent, AuditOutcomeDetail, ComponentState, MfaModuleChange,
    MfaModuleReference, StateChangeOutcome,
};
use weavelit_server_database::{
    ComponentKind, DatabaseError, MfaEnablementAuditTerminalWrites, MfaEnablementOutcome,
    MfaModuleTarget, Name,
};
use weavelit_server_log::{CorrelationId, EventTime, LogRecordPersistenceView};

use crate::{
    authentication::{correlation_identifier, system_clock},
    operational::OperationalDatabase,
    operational_audit::{
        AuditRecoverySequenceState, OperationalAuditGenerationDestination, OperationalAuditRecovery,
    },
    operational_logging::ConsequentialOperationError,
};

const TOTP_MODULE: &str = weavelit_module_mfa_totp::MODULE_IDENTIFIER;

/// Target-bound preview of Human Users affected by one TOTP enablement change.
pub(crate) struct MfaModuleEnablementPreview {
    target: MfaModuleTarget,
    desired_state: bool,
    affected_users: usize,
}

impl MfaModuleEnablementPreview {
    /// Returns the number of distinct enrolled Human Users observed for the preview.
    pub(crate) const fn affected_users(&self) -> usize {
        self.affected_users
    }
}

impl fmt::Debug for MfaModuleEnablementPreview {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MfaModuleEnablementPreview")
            .field("affected_users", &self.affected_users)
            .finish_non_exhaustive()
    }
}

/// Authoritative result of the transactional enablement decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MfaModuleEnablementOutcome {
    /// The desired state and any required session revocation committed.
    Applied {
        desired_state: bool,
        affected_users: usize,
    },
    /// The preview was stale, so only the conflict terminal committed.
    EnrolledCountChanged { current_affected_users: usize },
}

/// Delivery state after the committed result entered bounded recovery draining.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MfaModuleEnablementDelivery {
    /// Exact destination delivery and database acknowledgement completed.
    Acknowledged,
    /// The durable obligation remains available for bounded restart recovery.
    Pending,
}

/// Complete post-commit result of one authorized TOTP enablement workflow.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MfaModuleEnablementResult {
    pub(crate) outcome: MfaModuleEnablementOutcome,
    pub(crate) delivery: MfaModuleEnablementDelivery,
}

/// Payload-free pre-commit refusal of an internal enablement workflow.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MfaModuleEnablementError {
    /// The consumed action or preview was not the exact supported TOTP change.
    ActionNotSupported,
    /// Audit preparation, recovery, delivery, or required persistence was unavailable.
    AuditLogUnavailable,
}

impl fmt::Display for MfaModuleEnablementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ActionNotSupported => formatter.write_str("administration action not supported"),
            Self::AuditLogUnavailable => {
                ConsequentialOperationError::AuditLogUnavailable.fmt(formatter)
            }
        }
    }
}

impl StdError for MfaModuleEnablementError {}

/// Synchronous internal workflow for Administrator-controlled TOTP enablement.
pub(crate) struct MfaModuleEnablementWorkflow<'a> {
    database: &'a OperationalDatabase,
    audit: &'a OperationalAuditRecovery,
}

impl<'a> MfaModuleEnablementWorkflow<'a> {
    pub(crate) const fn new(
        database: &'a OperationalDatabase,
        audit: &'a OperationalAuditRecovery,
    ) -> Self {
        Self { database, audit }
    }

    /// Reads a distinct enrolled-Human-User count for the exact authorized target.
    pub(crate) fn preview(
        &self,
        action: &AuthorizedAdministrationAction,
    ) -> Result<MfaModuleEnablementPreview, MfaModuleEnablementError> {
        let desired_state = exact_totp_change(action)?;
        let target = totp_target()?;
        let affected_users = self
            .database
            .with_mfa(|store| store.enrolled_accounts(&target))
            .map_err(audit_unavailable)?;
        Ok(MfaModuleEnablementPreview {
            target,
            desired_state,
            affected_users,
        })
    }

    /// Consumes one exact authorization and its target-bound preview.
    pub(crate) fn apply(
        &self,
        action: AuthorizedAdministrationAction,
        preview: MfaModuleEnablementPreview,
    ) -> Result<MfaModuleEnablementResult, MfaModuleEnablementError> {
        let desired_state = exact_totp_change(&action)?;
        if desired_state != preview.desired_state || preview.target != totp_target()? {
            return Err(MfaModuleEnablementError::ActionNotSupported);
        }
        if self.audit.drain_before_consequential_operation().active()
            != AuditRecoverySequenceState::Ready
        {
            return Err(MfaModuleEnablementError::AuditLogUnavailable);
        }

        self.audit
            .with_current_destination(|destination| {
                self.apply_with_destination(action, preview, destination)
            })
            .map_err(audit_unavailable)?
    }

    fn apply_with_destination(
        &self,
        action: AuthorizedAdministrationAction,
        preview: MfaModuleEnablementPreview,
        destination: &OperationalAuditGenerationDestination,
    ) -> Result<MfaModuleEnablementResult, MfaModuleEnablementError> {
        let actor = self
            .database
            .load_account_audit_reference(action.actor())
            .map_err(audit_unavailable)?
            .ok_or(MfaModuleEnablementError::AuditLogUnavailable)?;
        let correlation = CorrelationId::new(
            correlation_identifier().ok_or(MfaModuleEnablementError::AuditLogUnavailable)?,
        )
        .map_err(|_| MfaModuleEnablementError::AuditLogUnavailable)?;
        let event = AuditEvent::AuthenticationMfaModuleEnablementChanged {
            module: MfaModuleReference::new(TOTP_MODULE)
                .map_err(|_| MfaModuleEnablementError::AuditLogUnavailable)?,
        };
        let attempt = self
            .audit
            .producer()
            .prepare_attempt(event_time()?, correlation, AuditActor::Human(actor), event)
            .map_err(|_| MfaModuleEnablementError::AuditLogUnavailable)?;
        let LogRecordPersistenceView::Audit(attempt_record) = attempt.record().persistence_view()
        else {
            return Err(MfaModuleEnablementError::AuditLogUnavailable);
        };
        let attempt_identifier = *attempt_record.record_id().as_bytes();
        let attempt_event_time = attempt_record.event_time().unix_milliseconds();
        let attempt_correlation = attempt_record.correlation_id().as_str().to_owned();
        let delivered_attempt = match attempt.deliver(destination.destination()) {
            Ok(attempt) => attempt,
            Err(error) => {
                self.audit.reject_attempt_delivery(
                    error,
                    attempt_identifier,
                    attempt_event_time,
                    &attempt_correlation,
                    destination.module(),
                );
                return Err(MfaModuleEnablementError::AuditLogUnavailable);
            }
        };

        let state = if preview.desired_state {
            ComponentState::Enabled
        } else {
            ComponentState::Disabled
        };
        let affected_users = u64::try_from(preview.affected_users)
            .map_err(|_| MfaModuleEnablementError::AuditLogUnavailable)?;
        let applied_terminal = self
            .audit
            .producer()
            .prepare_completion(
                &delivered_attempt,
                event_time()?,
                AuditOutcomeDetail::AuthenticationMfaModuleEnablementChanged(
                    StateChangeOutcome::Succeeded(MfaModuleChange::new(state, affected_users)),
                ),
            )
            .map_err(|_| MfaModuleEnablementError::AuditLogUnavailable)?;
        let conflict_terminal = self
            .audit
            .producer()
            .prepare_completion(
                &delivered_attempt,
                event_time()?,
                AuditOutcomeDetail::AuthenticationMfaModuleEnablementChanged(
                    StateChangeOutcome::Denied,
                ),
            )
            .map_err(|_| MfaModuleEnablementError::AuditLogUnavailable)?;
        let persistence = self.database.audit_terminal_recovery_persistence();
        let applied_write = applied_terminal
            .recovery_obligation(persistence, destination.binding())
            .map_err(|_| MfaModuleEnablementError::AuditLogUnavailable)?;
        let conflict_write = conflict_terminal
            .recovery_obligation(persistence, destination.binding())
            .map_err(|_| MfaModuleEnablementError::AuditLogUnavailable)?;
        let audit_terminals =
            MfaEnablementAuditTerminalWrites::new(&applied_write, &conflict_write);

        let committed = self
            .database
            .with_mfa(|store| {
                store.set_module_enabled(
                    &preview.target,
                    preview.desired_state,
                    preview.affected_users,
                    &audit_terminals,
                )
            })
            .map_err(audit_unavailable)?;
        let outcome = match committed {
            MfaEnablementOutcome::Applied { .. } => MfaModuleEnablementOutcome::Applied {
                desired_state: preview.desired_state,
                affected_users: preview.affected_users,
            },
            MfaEnablementOutcome::EnrolledCountChanged {
                current_affected_users,
            } => MfaModuleEnablementOutcome::EnrolledCountChanged {
                current_affected_users,
            },
        };
        let delivery = if self.audit.drain_after_consequential_operation().active()
            == AuditRecoverySequenceState::Ready
        {
            MfaModuleEnablementDelivery::Acknowledged
        } else {
            MfaModuleEnablementDelivery::Pending
        };

        Ok(MfaModuleEnablementResult { outcome, delivery })
    }
}

fn exact_totp_change(
    action: &AuthorizedAdministrationAction,
) -> Result<bool, MfaModuleEnablementError> {
    let AdministrationAction::ComponentEnablementChange(change) = action.action() else {
        return Err(MfaModuleEnablementError::ActionNotSupported);
    };
    if change.kind() != ComponentKind::MfaModule || change.name().as_str() != TOTP_MODULE {
        return Err(MfaModuleEnablementError::ActionNotSupported);
    }
    Ok(change.enabled())
}

fn totp_target() -> Result<MfaModuleTarget, MfaModuleEnablementError> {
    let name = Name::new(TOTP_MODULE).map_err(|_| MfaModuleEnablementError::ActionNotSupported)?;
    Ok(MfaModuleTarget {
        module: name.clone(),
        component: name,
    })
}

fn event_time() -> Result<EventTime, MfaModuleEnablementError> {
    let milliseconds = system_clock()().ok_or(MfaModuleEnablementError::AuditLogUnavailable)?;
    let milliseconds =
        u64::try_from(milliseconds).map_err(|_| MfaModuleEnablementError::AuditLogUnavailable)?;
    Ok(EventTime::from_unix_milliseconds(milliseconds))
}

fn audit_unavailable(_: DatabaseError) -> MfaModuleEnablementError {
    MfaModuleEnablementError::AuditLogUnavailable
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use rusqlite::{Connection, params};
    use tempfile::TempDir;
    use weavelit_server_administration::{
        AdministrationClock, AdministrationPlane, AdministrationRequest,
        AuthorizedAdministrationAdmission, ComponentEnablementChange, ComponentEnablementSource,
    };
    use weavelit_server_administration_authority::ServerAdministrationAuthority;
    use weavelit_server_authorization::{
        AdministrationRequest as AuthorizationRequest, AuthorizationCatalog, AuthorizationDenied,
        ClientModuleDeclaration, Plane, authorize_administration,
    };
    use weavelit_server_components::{AvailableComponents, MfaFactorFormat};
    use weavelit_server_database::{
        ComponentEnablement, GroupGrant, HumanAuthorizationSnapshot, SESSION_DIGEST_LENGTH,
        SessionTokenHash, StateIdentifier,
    };
    use weavelit_server_database_sqlite::SqliteDatabase;
    use weavelit_server_log::{
        CompleteLogRecord, ConfiguredLogDestination, DurableAcknowledgement, LogCapabilities,
        LogDestination, LogDestinationError, LogDestinationFactory, LogModuleCatalog,
        LogModuleFactoryContext, LogModuleIdentifier, LogModuleRegistration,
        LogRecordPersistenceView, LogRecordType, LogSettingsContract, TrustedLogModuleContext,
    };
    use weavelit_server_log_authority::ServerLogAuthority;

    use super::*;
    use crate::{
        operational::OperationalDatabase,
        operational_audit::{AuditRecoverySequenceState, OperationalAuditRecovery},
    };

    const CLIENT_MODULE: &str = "web-ui";

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct ObservedAuditRecord {
        classification: String,
        action: String,
        target: String,
        detail: String,
    }

    struct RecordingDestination {
        records: Arc<Mutex<Vec<ObservedAuditRecord>>>,
        attempts: Arc<AtomicUsize>,
        fail_on_attempt: Option<usize>,
    }

    impl LogDestination for RecordingDestination {
        fn deliver(
            &self,
            record: &CompleteLogRecord,
            acknowledgement: DurableAcknowledgement,
        ) -> Result<DurableAcknowledgement, LogDestinationError> {
            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst) + 1;
            if self.fail_on_attempt == Some(attempt) {
                return Err(LogDestinationError::Unavailable);
            }
            let LogRecordPersistenceView::Audit(view) = record.persistence_view() else {
                return Err(LogDestinationError::IntegrityFailure);
            };
            self.records.lock().unwrap().push(ObservedAuditRecord {
                classification: view.body().classification().to_owned(),
                action: view.body().action().to_owned(),
                target: view.body().target().to_owned(),
                detail: view.body().detail().to_owned(),
            });
            Ok(acknowledgement)
        }

        fn preflight(&self, _record_type: LogRecordType) -> Result<(), LogDestinationError> {
            Ok(())
        }
    }

    struct RecordingFactory {
        records: Arc<Mutex<Vec<ObservedAuditRecord>>>,
        attempts: Arc<AtomicUsize>,
        fail_on_attempt: Option<usize>,
    }

    impl LogDestinationFactory for RecordingFactory {
        fn accepted_settings(&self) -> LogSettingsContract {
            LogSettingsContract::none()
        }

        fn create(
            &self,
            _context: &LogModuleFactoryContext<'_>,
        ) -> Result<Box<dyn LogDestination>, LogDestinationError> {
            Ok(Box::new(RecordingDestination {
                records: Arc::clone(&self.records),
                attempts: Arc::clone(&self.attempts),
                fail_on_attempt: self.fail_on_attempt,
            }))
        }
    }

    struct FixedClock;

    impl AdministrationClock for FixedClock {
        fn now(&self) -> Duration {
            Duration::ZERO
        }
    }

    struct NoEnablementRead;

    impl ComponentEnablementSource for NoEnablementRead {
        fn load_component_enablement(
            &mut self,
        ) -> Result<ComponentEnablement, AuthorizationDenied> {
            panic!("enablement changes must not read current enablement during admission")
        }
    }

    struct Surface {
        _directory: TempDir,
        path: PathBuf,
        database: OperationalDatabase,
    }

    fn identifier(byte: u8) -> StateIdentifier {
        StateIdentifier::from_bytes([byte; 16]).unwrap()
    }

    fn name(value: &str) -> Name {
        Name::new(value).unwrap()
    }

    fn surface(enabled: bool, enrolled: bool, session: bool) -> Surface {
        let directory = tempfile::tempdir().unwrap();
        let path = directory
            .path()
            .canonicalize()
            .unwrap()
            .join("application.db");
        drop(SqliteDatabase::open(&path).unwrap());
        let connection = Connection::open(&path).unwrap();
        connection
            .execute(
                "INSERT INTO weavelit_account \
                 (account_id, username, display_name, active, mfa_required) \
                 VALUES (?1, 'administrator', NULL, 1, 0)",
                [identifier(1).as_bytes().as_slice()],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO weavelit_account_audit_reference (account_id, audit_reference) \
                 VALUES (?1, 'ar-11111111111111111111111111111111')",
                [identifier(1).as_bytes().as_slice()],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO weavelit_configuration (component, setting_key, setting_value) \
                 VALUES ('totp', 'mfa-module.enabled', ?1)",
                [if enabled { "true" } else { "false" }],
            )
            .unwrap();
        if enrolled {
            insert_enrollment(&connection, 1);
        }
        if session {
            connection
                .execute(
                    "INSERT INTO weavelit_session \
                     (token_hash, csrf_hash, account_id, client_module, issued_at_milliseconds, \
                      last_seen_at_milliseconds, absolute_expires_at_milliseconds) \
                     VALUES (?1, ?2, ?3, 'web-ui', 1, 1, 43200001)",
                    params![
                        [0x31_u8; 32].as_slice(),
                        [0x32_u8; 32].as_slice(),
                        identifier(1).as_bytes().as_slice()
                    ],
                )
                .unwrap();
        }
        drop(connection);
        let database =
            OperationalDatabase::from_open(Box::new(SqliteDatabase::open(&path).unwrap()));
        Surface {
            _directory: directory,
            path,
            database,
        }
    }

    fn insert_enrollment(connection: &Connection, account_byte: u8) {
        connection
            .execute(
                "INSERT INTO weavelit_mfa_factor \
                 (factor_id, account_id, module, protected_factor_data) \
                 VALUES (?1, ?2, 'totp', ?3)",
                params![
                    identifier(account_byte + 0x40).as_bytes().as_slice(),
                    identifier(account_byte).as_bytes().as_slice(),
                    [0x55_u8; 20].as_slice()
                ],
            )
            .unwrap();
    }

    fn recovery(
        database: OperationalDatabase,
        fail_on_attempt: Option<usize>,
    ) -> (
        OperationalAuditRecovery,
        Arc<Mutex<Vec<ObservedAuditRecord>>>,
        Arc<AtomicUsize>,
    ) {
        let records = Arc::new(Mutex::new(Vec::new()));
        let attempts = Arc::new(AtomicUsize::new(0));
        let module = LogModuleIdentifier::new("recording").unwrap();
        let catalog = Arc::new(
            LogModuleCatalog::new(vec![LogModuleRegistration::new(
                "recording",
                LogCapabilities::new(vec![LogRecordType::Audit]).unwrap(),
                Box::new(RecordingFactory {
                    records: Arc::clone(&records),
                    attempts: Arc::clone(&attempts),
                    fail_on_attempt,
                }),
            )])
            .unwrap(),
        );
        let destination: ConfiguredLogDestination = catalog
            .create_destination(
                &module,
                &TrustedLogModuleContext::from_server_authority(
                    &ServerLogAuthority::new(),
                    PathBuf::from("/unused"),
                    [0x42; 16],
                ),
            )
            .unwrap();
        (
            OperationalAuditRecovery::for_test(database, catalog, module, destination),
            records,
            attempts,
        )
    }

    fn authorized_change(enabled: bool) -> AuthorizedAdministrationAction {
        let client_module = name(CLIENT_MODULE);
        let authorization = authorize_administration(
            &HumanAuthorizationSnapshot::new(
                true,
                vec![
                    GroupGrant::ClientModule(client_module.clone()),
                    GroupGrant::ServerAdministration,
                ],
            ),
            &AuthorizationCatalog::new(
                vec![ClientModuleDeclaration::new(
                    client_module,
                    true,
                    &[Plane::Administration],
                )],
                vec![],
                vec![],
            )
            .unwrap(),
            AuthorizationRequest {
                client_module: &name(CLIENT_MODULE),
            },
        )
        .unwrap();
        let authority = ServerAdministrationAuthority::new();
        let admission = AuthorizedAdministrationAdmission::from_server_authority(
            &authority,
            authorization,
            identifier(1),
            SessionTokenHash::from_bytes([0x21; SESSION_DIGEST_LENGTH]).unwrap(),
        );
        AdministrationPlane::new(
            FixedClock,
            NoEnablementRead,
            AvailableComponents {
                client_modules: [name(CLIENT_MODULE)].into_iter().collect(),
                mfa_modules: [(
                    name(TOTP_MODULE),
                    MfaFactorFormat {
                        factor_data_bytes: 20,
                    },
                )]
                .into_iter()
                .collect(),
                ..AvailableComponents::default()
            },
        )
        .authorize(
            admission,
            AdministrationRequest::new(AdministrationAction::ComponentEnablementChange(
                ComponentEnablementChange::new(ComponentKind::MfaModule, TOTP_MODULE, enabled)
                    .unwrap(),
            )),
        )
        .unwrap()
    }

    fn enablement(path: &Path) -> String {
        Connection::open(path)
            .unwrap()
            .query_row(
                "SELECT setting_value FROM weavelit_configuration \
                 WHERE component = 'totp' AND setting_key = 'mfa-module.enabled'",
                [],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn count(path: &Path, table: &str, predicate: &str) -> i64 {
        Connection::open(path)
            .unwrap()
            .query_row(
                &format!("SELECT count(*) FROM {table} {predicate}"),
                [],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn non_administrator_is_denied_before_a_workflow_or_audit_side_effect_exists() {
        let client_module = name(CLIENT_MODULE);
        let denied = authorize_administration(
            &HumanAuthorizationSnapshot::new(
                true,
                vec![GroupGrant::ClientModule(client_module.clone())],
            ),
            &AuthorizationCatalog::new(
                vec![ClientModuleDeclaration::new(
                    client_module.clone(),
                    true,
                    &[Plane::Administration],
                )],
                vec![],
                vec![],
            )
            .unwrap(),
            AuthorizationRequest {
                client_module: &client_module,
            },
        );

        assert_eq!(denied.unwrap_err(), AuthorizationDenied);
    }

    #[test]
    fn active_recovery_not_ready_rejects_before_attempt_or_mutation() {
        let surface = surface(false, false, false);
        let action = authorized_change(true);
        let (audit, _, attempts) = recovery(surface.database.clone(), None);
        let workflow = MfaModuleEnablementWorkflow::new(&surface.database, &audit);
        let preview = workflow.preview(&action).unwrap();
        Connection::open(&surface.path)
            .unwrap()
            .execute_batch(
                "DROP TABLE weavelit_audit_terminal_supersession; \
                 DROP TABLE weavelit_audit_terminal_obligation;",
            )
            .unwrap();

        assert_eq!(
            workflow.apply(action, preview),
            Err(MfaModuleEnablementError::AuditLogUnavailable)
        );
        assert_eq!(enablement(&surface.path), "false");
        assert_eq!(attempts.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn attempt_delivery_failure_is_redacted_and_mutates_nothing() {
        let surface = surface(false, false, false);
        let action = authorized_change(true);
        let (audit, records, attempts) = recovery(surface.database.clone(), Some(1));
        let workflow = MfaModuleEnablementWorkflow::new(&surface.database, &audit);
        let preview = workflow.preview(&action).unwrap();

        let error = workflow.apply(action, preview).unwrap_err();

        assert_eq!(error, MfaModuleEnablementError::AuditLogUnavailable);
        assert_eq!(
            error.to_string(),
            "Audit Log unavailable; operation rejected."
        );
        assert!(!format!("{error:?}").contains("temporary-password"));
        assert_eq!(enablement(&surface.path), "false");
        assert_eq!(
            count(&surface.path, "weavelit_audit_terminal_obligation", ""),
            0
        );
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert!(records.lock().unwrap().is_empty());
    }

    #[test]
    fn applied_disablement_commits_one_success_terminal_and_is_acknowledged() {
        let surface = surface(true, true, true);
        let action = authorized_change(false);
        let (audit, records, _) = recovery(surface.database.clone(), None);
        let workflow = MfaModuleEnablementWorkflow::new(&surface.database, &audit);
        let preview = workflow.preview(&action).unwrap();
        assert_eq!(preview.affected_users(), 1);

        let result = workflow.apply(action, preview).unwrap();

        assert_eq!(
            result,
            MfaModuleEnablementResult {
                outcome: MfaModuleEnablementOutcome::Applied {
                    desired_state: false,
                    affected_users: 1,
                },
                delivery: MfaModuleEnablementDelivery::Acknowledged,
            }
        );
        assert_eq!(enablement(&surface.path), "false");
        assert_eq!(count(&surface.path, "weavelit_session", ""), 0);
        assert_eq!(
            count(&surface.path, "weavelit_audit_terminal_obligation", ""),
            1
        );
        assert_eq!(
            count(
                &surface.path,
                "weavelit_audit_terminal_obligation",
                "WHERE acknowledged = 1"
            ),
            1
        );
        let records = records.lock().unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(
            records[0].classification,
            "authentication.mfa-module-enablement.changed"
        );
        assert_eq!(records[0].action, "change-mfa-module");
        assert_eq!(records[0].target, "mfa-module:totp");
        assert!(records[1].detail.contains("MFA module state: disabled"));
        assert!(records[1].detail.contains("affected count: 1"));
    }

    #[test]
    fn same_state_disablement_still_revokes_sessions_and_records_one_terminal() {
        let surface = surface(false, true, true);
        let action = authorized_change(false);
        let (audit, records, _) = recovery(surface.database.clone(), None);
        let workflow = MfaModuleEnablementWorkflow::new(&surface.database, &audit);
        let preview = workflow.preview(&action).unwrap();

        let result = workflow.apply(action, preview).unwrap();

        assert_eq!(
            result,
            MfaModuleEnablementResult {
                outcome: MfaModuleEnablementOutcome::Applied {
                    desired_state: false,
                    affected_users: 1,
                },
                delivery: MfaModuleEnablementDelivery::Acknowledged,
            }
        );
        assert_eq!(enablement(&surface.path), "false");
        assert_eq!(count(&surface.path, "weavelit_session", ""), 0);
        assert_eq!(
            count(&surface.path, "weavelit_audit_terminal_obligation", ""),
            1
        );
        assert_eq!(records.lock().unwrap().len(), 2);
    }

    #[test]
    fn stale_preview_commits_only_the_payload_free_conflict_terminal() {
        let surface = surface(false, false, false);
        let action = authorized_change(true);
        let (audit, records, _) = recovery(surface.database.clone(), None);
        let workflow = MfaModuleEnablementWorkflow::new(&surface.database, &audit);
        let preview = workflow.preview(&action).unwrap();
        assert_eq!(preview.affected_users(), 0);
        insert_enrollment(&Connection::open(&surface.path).unwrap(), 1);

        let result = workflow.apply(action, preview).unwrap();

        assert_eq!(
            result,
            MfaModuleEnablementResult {
                outcome: MfaModuleEnablementOutcome::EnrolledCountChanged {
                    current_affected_users: 1,
                },
                delivery: MfaModuleEnablementDelivery::Acknowledged,
            }
        );
        assert_eq!(enablement(&surface.path), "false");
        assert_eq!(
            count(&surface.path, "weavelit_audit_terminal_obligation", ""),
            1
        );
        let records = records.lock().unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[1].detail, "accountable action denied");
        assert!(!records[1].detail.contains('1'));
    }

    #[test]
    fn postcommit_failure_is_pending_and_a_restart_drain_recovers_it() {
        let surface = surface(false, false, false);
        let action = authorized_change(true);
        let (audit, records, attempts) = recovery(surface.database.clone(), Some(2));
        let workflow = MfaModuleEnablementWorkflow::new(&surface.database, &audit);
        let preview = workflow.preview(&action).unwrap();

        let result = workflow.apply(action, preview).unwrap();

        assert_eq!(result.delivery, MfaModuleEnablementDelivery::Pending);
        assert_eq!(enablement(&surface.path), "true");
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert_eq!(records.lock().unwrap().len(), 1);
        assert_eq!(
            count(
                &surface.path,
                "weavelit_audit_terminal_obligation",
                "WHERE acknowledged = 0"
            ),
            1
        );

        let (restarted, restarted_records, _) = recovery(surface.database.clone(), None);
        let recovered = restarted.drain_for_activation();

        assert_eq!(recovered.active(), AuditRecoverySequenceState::Ready);
        assert_eq!(restarted_records.lock().unwrap().len(), 1);
        assert_eq!(
            count(
                &surface.path,
                "weavelit_audit_terminal_obligation",
                "WHERE acknowledged = 0"
            ),
            0
        );
    }
}
