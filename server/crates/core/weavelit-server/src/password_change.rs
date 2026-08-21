//! Internal forced password replacement from a restricted authenticated session.

#![allow(dead_code)]

use std::fmt;

use weavelit_module_client::SessionEstablished;
use weavelit_server_audit::{ActionOutcome, AuditActor, AuditEvent, AuditOutcomeDetail};
use weavelit_server_authentication::{Argon2Engine, PasswordReplacementInput};
use weavelit_server_database::{
    PasswordChangeAuditTerminalWrites, PasswordChangeOutcome, ValidatedAuditTerminalObligationWrite,
};
use weavelit_server_log::{
    AuditLogClassification, CorrelationId, EventTime, LogRecordPersistenceView, LogRecordType,
};

use crate::{
    authentication::{
        AuthenticationRuntime, PasswordChangePreparationError, ValidatedSession, system_clock,
    },
    operational::OperationalDatabase,
    operational_audit::{
        AuditRecoverySequenceState, OperationalAuditGenerationDestination, OperationalAuditRecovery,
    },
};

/// Postcommit Audit terminal delivery state for a password change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PasswordChangeDelivery {
    Acknowledged,
    Pending,
}

/// Complete internal result of one forced password change.
pub(crate) enum PasswordChangeResult {
    Changed {
        session: SessionEstablished,
        delivery: PasswordChangeDelivery,
    },
    Denied {
        delivery: PasswordChangeDelivery,
    },
}

impl fmt::Debug for PasswordChangeResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PasswordChangeResult(REDACTED)")
    }
}

/// Payload-free refusal before a password-change result commits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PasswordChangeWorkflowError {
    Denied,
    Unavailable,
    AuditLogUnavailable,
}

impl fmt::Display for PasswordChangeWorkflowError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Denied => "password change denied",
            Self::Unavailable => "password change unavailable",
            Self::AuditLogUnavailable => "consequential operation Audit Log unavailable",
        })
    }
}

impl std::error::Error for PasswordChangeWorkflowError {}

/// Route-independent forced password-change orchestration.
pub(crate) struct PasswordChangeWorkflow<'a, E> {
    database: &'a OperationalDatabase,
    authentication: &'a AuthenticationRuntime<E>,
    audit: &'a OperationalAuditRecovery,
}

impl<'a, E> PasswordChangeWorkflow<'a, E>
where
    E: Argon2Engine + Send + Sync + 'static,
{
    pub(crate) const fn new(
        database: &'a OperationalDatabase,
        authentication: &'a AuthenticationRuntime<E>,
        audit: &'a OperationalAuditRecovery,
    ) -> Self {
        Self {
            database,
            authentication,
            audit,
        }
    }

    /// Consumes the exact restricted session and commits one audited outcome.
    pub(crate) fn change(
        &self,
        session: ValidatedSession,
        input: PasswordReplacementInput,
        correlation_id: &str,
    ) -> Result<PasswordChangeResult, PasswordChangeWorkflowError> {
        if self.audit.drain_before_consequential_operation().active()
            != AuditRecoverySequenceState::Ready
        {
            return Err(PasswordChangeWorkflowError::AuditLogUnavailable);
        }
        let admission = self
            .authentication
            .admit_password_change(session, input)
            .map_err(preparation_error)?;
        let account = self
            .database
            .load_account_audit_reference(admission.account())
            .map_err(|_| PasswordChangeWorkflowError::Unavailable)?
            .ok_or(PasswordChangeWorkflowError::Unavailable)?;

        self.audit
            .with_current_destination(|destination| {
                destination
                    .destination()
                    .preflight(LogRecordType::Audit)
                    .map_err(|_| PasswordChangeWorkflowError::AuditLogUnavailable)?;
                let terminals = self.prepare_terminals(account, destination, correlation_id)?;
                let (mutation, session) = self
                    .authentication
                    .prepare_password_change_mutation(admission)
                    .map_err(preparation_error)?;
                let outcome = self
                    .database
                    .change_password(&mutation, &terminals.writes())
                    .map_err(|_| PasswordChangeWorkflowError::AuditLogUnavailable)?;
                Ok(self.result(outcome, session))
            })
            .map_err(|_| PasswordChangeWorkflowError::AuditLogUnavailable)?
    }

    fn prepare_terminals(
        &self,
        account: weavelit_server_database::AccountAuditReference,
        destination: &OperationalAuditGenerationDestination,
        correlation_id: &str,
    ) -> Result<PreparedPasswordChangeTerminals, PasswordChangeWorkflowError> {
        let correlation = CorrelationId::new(correlation_id.to_owned())
            .map_err(|_| PasswordChangeWorkflowError::Unavailable)?;
        let attempt = self
            .audit
            .producer()
            .prepare_attempt(
                event_time()?,
                correlation,
                AuditActor::Human(account),
                AuditEvent::AuthenticationPasswordChanged { account },
            )
            .map_err(|_| PasswordChangeWorkflowError::AuditLogUnavailable)?;
        let LogRecordPersistenceView::Audit(attempt_record) = attempt.record().persistence_view()
        else {
            return Err(PasswordChangeWorkflowError::AuditLogUnavailable);
        };
        let attempt_identifier = *attempt_record.record_id().as_bytes();
        let attempt_event_time = attempt_record.event_time().unix_milliseconds();
        let attempt_correlation = attempt_record.correlation_id().as_str().to_owned();
        let delivered = match attempt.deliver(destination.destination()) {
            Ok(delivered) => delivered,
            Err(error) => {
                self.audit.reject_attempt_delivery(
                    error,
                    attempt_identifier,
                    attempt_event_time,
                    &attempt_correlation,
                    destination.module(),
                    AuditLogClassification::AuthenticationPasswordChanged,
                );
                return Err(PasswordChangeWorkflowError::AuditLogUnavailable);
            }
        };
        let succeeded = self
            .audit
            .producer()
            .prepare_completion(
                &delivered,
                event_time()?,
                AuditOutcomeDetail::AuthenticationPasswordChanged(ActionOutcome::Succeeded),
            )
            .map_err(|_| PasswordChangeWorkflowError::AuditLogUnavailable)?;
        let denied = self
            .audit
            .producer()
            .prepare_completion(
                &delivered,
                event_time()?,
                AuditOutcomeDetail::AuthenticationPasswordChanged(ActionOutcome::Denied),
            )
            .map_err(|_| PasswordChangeWorkflowError::AuditLogUnavailable)?;
        let persistence = self.database.audit_terminal_recovery_persistence();
        Ok(PreparedPasswordChangeTerminals {
            succeeded: succeeded
                .recovery_obligation(persistence, destination.binding())
                .map_err(|_| PasswordChangeWorkflowError::AuditLogUnavailable)?,
            denied: denied
                .recovery_obligation(persistence, destination.binding())
                .map_err(|_| PasswordChangeWorkflowError::AuditLogUnavailable)?,
        })
    }

    fn result(
        &self,
        outcome: PasswordChangeOutcome,
        session: SessionEstablished,
    ) -> PasswordChangeResult {
        let delivery = if self.audit.drain_after_consequential_operation().active()
            == AuditRecoverySequenceState::Ready
        {
            PasswordChangeDelivery::Acknowledged
        } else {
            PasswordChangeDelivery::Pending
        };
        match outcome {
            PasswordChangeOutcome::Changed { .. } => {
                PasswordChangeResult::Changed { session, delivery }
            }
            PasswordChangeOutcome::Denied => {
                drop(session);
                PasswordChangeResult::Denied { delivery }
            }
        }
    }
}

struct PreparedPasswordChangeTerminals {
    succeeded: ValidatedAuditTerminalObligationWrite,
    denied: ValidatedAuditTerminalObligationWrite,
}

impl PreparedPasswordChangeTerminals {
    fn writes(&self) -> PasswordChangeAuditTerminalWrites<'_> {
        PasswordChangeAuditTerminalWrites::new(&self.succeeded, &self.denied)
    }
}

fn preparation_error(error: PasswordChangePreparationError) -> PasswordChangeWorkflowError {
    match error {
        PasswordChangePreparationError::Denied => PasswordChangeWorkflowError::Denied,
        PasswordChangePreparationError::Unavailable => PasswordChangeWorkflowError::Unavailable,
    }
}

fn event_time() -> Result<EventTime, PasswordChangeWorkflowError> {
    let milliseconds = system_clock()().ok_or(PasswordChangeWorkflowError::AuditLogUnavailable)?;
    let milliseconds = u64::try_from(milliseconds)
        .map_err(|_| PasswordChangeWorkflowError::AuditLogUnavailable)?;
    Ok(EventTime::from_unix_milliseconds(milliseconds))
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, fs, os::unix::fs::PermissionsExt, path::PathBuf, sync::Arc};

    use rusqlite::{Connection, params};
    use weavelit_server_authentication::{
        Argon2Engine, CURRENT_ARGON2_PROFILE, PasswordVerifierFactory, RustCryptoArgon2,
        SessionSecrets,
    };

    const CORRELATION: &str = "password-change-test-correlation";
    use weavelit_server_database::{
        Account, AccountPasswordVerifier, CredentialRevision, Name, NewSession, PasswordVerifier,
        SessionCsrfHash, SessionInstant, SessionIssuance, SessionTokenHash,
        TemporaryCredentialExpiration,
    };

    use super::*;
    use crate::{
        APPLICATION_DATABASE_FILE, RestrictedStartup, StartupOutcome,
        administration::tests::{recovery, recovery_with_hook, recovery_with_preflight_failure},
        authentication::{AuthenticationClocks, monotonic_clock},
        classify_restricted_startup,
        tests::{SealedStateParts, seal_deployment_with, sealed_application_state_from},
    };

    const ACCOUNT_BYTES: [u8; 16] = [0x25; 16];
    const CLIENT_MODULE: &str = "web-ui";
    const TEMPORARY_PASSWORD: &[u8] = b"temporary credential";
    const REPLACEMENT_PASSWORD: &[u8] = b"new ordinary password";

    struct Surface {
        _startup: RestrictedStartup,
        _root: tempfile::TempDir,
        path: PathBuf,
        database: OperationalDatabase,
        authentication: Arc<AuthenticationRuntime<RustCryptoArgon2>>,
        now: i64,
    }

    fn identifier() -> weavelit_server_database::StateIdentifier {
        weavelit_server_database::StateIdentifier::from_bytes(ACCOUNT_BYTES).unwrap()
    }

    fn surface(temporary: bool) -> Surface {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let state_root = root.path().canonicalize().unwrap();
        let now = system_clock()().unwrap();
        let verifier = PasswordVerifierFactory::approved()
            .create(TEMPORARY_PASSWORD)
            .unwrap();
        let state = sealed_application_state_from(SealedStateParts {
            accounts: vec![Account {
                identifier: identifier(),
                username: Name::new("temporary-user").unwrap(),
                display_name: None,
                active: true,
                mfa_required: false,
                credential_revision: CredentialRevision::INITIAL,
                must_change_password: temporary,
                temporary_credential_expiration: temporary.then(|| {
                    TemporaryCredentialExpiration::from_unix_milliseconds(now + 60_000).unwrap()
                }),
            }],
            password_verifiers: vec![AccountPasswordVerifier {
                account: identifier(),
                verifier: PasswordVerifier::new(verifier.into_string()).unwrap(),
            }],
            ..SealedStateParts::default()
        });
        seal_deployment_with(&state_root, &state);
        let startup = classify_restricted_startup(&state_root).unwrap();
        assert_eq!(startup.outcome(), StartupOutcome::Initialized);
        let database = startup.application_database().unwrap().clone();
        let clock = Arc::new(move || Some(now));
        let authentication = AuthenticationRuntime::with_engine(
            RustCryptoArgon2::default(),
            database.clone(),
            startup.initialized_state().unwrap(),
            BTreeSet::from([Name::new(CLIENT_MODULE).unwrap()]),
            AuthenticationClocks {
                wall: clock,
                elapsed: monotonic_clock(),
            },
            None,
            startup.protection(),
        )
        .unwrap();
        Surface {
            _startup: startup,
            _root: root,
            path: state_root.join(APPLICATION_DATABASE_FILE),
            database,
            authentication,
            now,
        }
    }

    fn validated_session(surface: &Surface) -> (ValidatedSession, SessionSecrets) {
        let secrets = SessionSecrets::generate().unwrap();
        let (session_digest, csrf_digest) = secrets.digests();
        let stored = NewSession::new(
            SessionTokenHash::from_bytes(*session_digest.as_bytes()).unwrap(),
            SessionCsrfHash::from_bytes(*csrf_digest.as_bytes()).unwrap(),
            identifier(),
            CredentialRevision::INITIAL,
            Name::new(CLIENT_MODULE).unwrap(),
            SessionInstant::from_unix_milliseconds(surface.now - 1).unwrap(),
        );
        let issuance = surface
            .database
            .with(|database| database.sessions().unwrap().create(&stored))
            .unwrap()
            .unwrap();
        assert_eq!(issuance, SessionIssuance::Issued);
        let validated = surface
            .authentication
            .validated_session(secrets.session().as_str(), secrets.csrf().as_str())
            .unwrap();
        (validated, secrets)
    }

    fn input(password: &[u8]) -> PasswordReplacementInput {
        PasswordReplacementInput::new(zeroize::Zeroizing::new(password.to_vec())).unwrap()
    }

    fn credential_state(path: &PathBuf) -> (Vec<u8>, i64, Option<i64>, String) {
        Connection::open(path)
            .unwrap()
            .query_row(
                "SELECT account.credential_revision, account.must_change_password, \
             account.temporary_credential_expires_at_milliseconds, verifier.encoded_verifier \
             FROM weavelit_account AS account JOIN weavelit_password_verifier AS verifier \
             ON verifier.account_id = account.account_id WHERE account.account_id = ?1",
                [identifier().as_bytes().as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap()
    }

    fn session_count(path: &PathBuf) -> i64 {
        Connection::open(path)
            .unwrap()
            .query_row(
                "SELECT count(*) FROM weavelit_session WHERE account_id = ?1",
                [identifier().as_bytes().as_slice()],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn restricted_session_changes_password_and_returns_only_a_fresh_ordinary_session() {
        let surface = surface(true);
        let (restricted, old_secrets) = validated_session(&surface);
        assert!(!restricted.is_ordinary());
        let (audit, records, _) = recovery(surface.database.clone(), None);
        let workflow =
            PasswordChangeWorkflow::new(&surface.database, &surface.authentication, &audit);

        let result = workflow
            .change(restricted, input(REPLACEMENT_PASSWORD), CORRELATION)
            .unwrap();
        let rendered = format!("{result:?}");
        let PasswordChangeResult::Changed { session, delivery } = result else {
            panic!("the exact restricted session must change its password")
        };
        assert_eq!(delivery, PasswordChangeDelivery::Acknowledged);
        assert_eq!(
            credential_state(&surface.path).0,
            2_u64.to_be_bytes().to_vec()
        );
        assert_eq!(
            (
                credential_state(&surface.path).1,
                credential_state(&surface.path).2
            ),
            (0, None)
        );
        assert_eq!(session_count(&surface.path), 1);
        assert!(
            surface
                .authentication
                .validated_session(old_secrets.session().as_str(), old_secrets.csrf().as_str())
                .is_err()
        );
        let fresh = surface
            .authentication
            .validated_session(session.session_token.as_str(), session.csrf_token.as_str())
            .unwrap();
        assert!(fresh.is_ordinary());

        let stored_verifier = credential_state(&surface.path).3;
        let engine = RustCryptoArgon2::default();
        assert!(engine.verify(
            REPLACEMENT_PASSWORD,
            &CURRENT_ARGON2_PROFILE,
            &stored_verifier
        ));
        assert!(!engine.verify(
            TEMPORARY_PASSWORD,
            &CURRENT_ARGON2_PROFILE,
            &stored_verifier
        ));
        let records = records.lock().unwrap();
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|record| {
            record.classification == "authentication.password.changed"
                && record.action == "change-password"
        }));
        for secret in [
            TEMPORARY_PASSWORD,
            REPLACEMENT_PASSWORD,
            old_secrets.session().as_str().as_bytes(),
            old_secrets.csrf().as_str().as_bytes(),
            session.session_token.as_bytes(),
            session.csrf_token.as_bytes(),
            stored_verifier.as_bytes(),
        ] {
            let secret = std::str::from_utf8(secret).unwrap();
            assert!(!rendered.contains(secret));
            assert!(records.iter().all(|record| {
                !format!(
                    "{} {} {} {}",
                    record.classification, record.action, record.target, record.detail
                )
                .contains(secret)
            }));
        }
    }

    #[test]
    fn same_password_and_ordinary_session_are_denied_before_an_audit_attempt() {
        let temporary = surface(true);
        let (restricted, _) = validated_session(&temporary);
        let (audit, records, _) = recovery(temporary.database.clone(), None);
        let workflow =
            PasswordChangeWorkflow::new(&temporary.database, &temporary.authentication, &audit);
        assert_eq!(
            workflow
                .change(restricted, input(TEMPORARY_PASSWORD), CORRELATION)
                .unwrap_err(),
            PasswordChangeWorkflowError::Denied
        );
        assert!(records.lock().unwrap().is_empty());
        assert_eq!(credential_state(&temporary.path).1, 1);

        let ordinary = surface(false);
        let (session, _) = validated_session(&ordinary);
        assert!(session.is_ordinary());
        let (audit, records, _) = recovery(ordinary.database.clone(), None);
        let workflow =
            PasswordChangeWorkflow::new(&ordinary.database, &ordinary.authentication, &audit);
        assert_eq!(
            workflow
                .change(session, input(REPLACEMENT_PASSWORD), CORRELATION)
                .unwrap_err(),
            PasswordChangeWorkflowError::Denied
        );
        assert!(records.lock().unwrap().is_empty());
    }

    #[test]
    fn expiry_after_acknowledged_attempt_commits_only_the_denied_terminal() {
        let surface = surface(true);
        let (restricted, _) = validated_session(&surface);
        let path = surface.path.clone();
        let now = surface.now;
        let hook = Arc::new(move |delivery: usize| {
            if delivery == 1 {
                Connection::open(&path).unwrap().execute(
                    "UPDATE weavelit_account SET temporary_credential_expires_at_milliseconds = ?2 \
                     WHERE account_id = ?1",
                    params![identifier().as_bytes().as_slice(), now],
                ).unwrap();
            }
        });
        let (audit, records, _) = recovery_with_hook(surface.database.clone(), None, Some(hook));
        let workflow =
            PasswordChangeWorkflow::new(&surface.database, &surface.authentication, &audit);

        let result = workflow
            .change(restricted, input(REPLACEMENT_PASSWORD), CORRELATION)
            .unwrap();

        assert!(matches!(
            result,
            PasswordChangeResult::Denied {
                delivery: PasswordChangeDelivery::Acknowledged
            }
        ));
        assert_eq!(credential_state(&surface.path).1, 1);
        assert_eq!(
            credential_state(&surface.path).3.as_str(),
            credential_state(&surface.path).3
        );
        assert_eq!(session_count(&surface.path), 1);
        let records = records.lock().unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[1].detail, "accountable action denied");
    }

    #[test]
    fn stale_revision_after_acknowledged_attempt_commits_only_the_denied_terminal() {
        let surface = surface(true);
        let (restricted, _) = validated_session(&surface);
        let path = surface.path.clone();
        let hook = Arc::new(move |delivery: usize| {
            if delivery == 1 {
                Connection::open(&path)
                    .unwrap()
                    .execute(
                        "UPDATE weavelit_account SET credential_revision = ?2 \
                         WHERE account_id = ?1",
                        params![
                            identifier().as_bytes().as_slice(),
                            2_u64.to_be_bytes().as_slice()
                        ],
                    )
                    .unwrap();
            }
        });
        let (audit, records, _) = recovery_with_hook(surface.database.clone(), None, Some(hook));
        let workflow =
            PasswordChangeWorkflow::new(&surface.database, &surface.authentication, &audit);

        let result = workflow
            .change(restricted, input(REPLACEMENT_PASSWORD), CORRELATION)
            .unwrap();

        assert!(matches!(
            result,
            PasswordChangeResult::Denied {
                delivery: PasswordChangeDelivery::Acknowledged
            }
        ));
        let credential = credential_state(&surface.path);
        assert_eq!(credential.0, 2_u64.to_be_bytes().to_vec());
        assert_eq!(
            (credential.1, credential.2),
            (1, Some(surface.now + 60_000))
        );
        assert!(RustCryptoArgon2::default().verify(
            TEMPORARY_PASSWORD,
            &CURRENT_ARGON2_PROFILE,
            &credential.3
        ));
        assert_eq!(session_count(&surface.path), 1);
        let records = records.lock().unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[1].detail, "accountable action denied");
    }

    #[test]
    fn audit_attempt_failure_changes_nothing_and_postcommit_failure_preserves_success() {
        let preflight = surface(true);
        let (restricted, _) = validated_session(&preflight);
        let before = credential_state(&preflight.path);
        let (audit, records, attempts) =
            recovery_with_preflight_failure(preflight.database.clone());
        let workflow =
            PasswordChangeWorkflow::new(&preflight.database, &preflight.authentication, &audit);
        assert_eq!(
            workflow
                .change(restricted, input(REPLACEMENT_PASSWORD), CORRELATION)
                .unwrap_err(),
            PasswordChangeWorkflowError::AuditLogUnavailable
        );
        assert_eq!(credential_state(&preflight.path), before);
        assert_eq!(session_count(&preflight.path), 1);
        assert!(records.lock().unwrap().is_empty());
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 0);

        let failed = surface(true);
        let (restricted, _) = validated_session(&failed);
        let before = credential_state(&failed.path);
        let (audit, records, _) = recovery(failed.database.clone(), Some(1));
        let workflow =
            PasswordChangeWorkflow::new(&failed.database, &failed.authentication, &audit);
        assert_eq!(
            workflow
                .change(restricted, input(REPLACEMENT_PASSWORD), CORRELATION)
                .unwrap_err(),
            PasswordChangeWorkflowError::AuditLogUnavailable
        );
        assert_eq!(credential_state(&failed.path), before);
        assert_eq!(session_count(&failed.path), 1);
        assert!(records.lock().unwrap().is_empty());

        let pending = surface(true);
        let (restricted, _) = validated_session(&pending);
        let (audit, records, _) = recovery(pending.database.clone(), Some(2));
        let workflow =
            PasswordChangeWorkflow::new(&pending.database, &pending.authentication, &audit);
        let result = workflow
            .change(restricted, input(REPLACEMENT_PASSWORD), CORRELATION)
            .unwrap();
        let PasswordChangeResult::Changed { session, delivery } = result else {
            panic!("the mutation must remain committed")
        };
        assert_eq!(delivery, PasswordChangeDelivery::Pending);
        assert_eq!(credential_state(&pending.path).1, 0);
        assert!(
            pending
                .authentication
                .validated_session(session.session_token.as_str(), session.csrf_token.as_str())
                .is_ok()
        );
        assert_eq!(records.lock().unwrap().len(), 1);

        let (restarted, recovered, _) = recovery(pending.database.clone(), None);
        assert_eq!(
            restarted.drain_for_activation().active(),
            AuditRecoverySequenceState::Ready
        );
        assert_eq!(recovered.lock().unwrap().len(), 1);
    }
}
