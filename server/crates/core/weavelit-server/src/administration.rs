//! Server-owned, transport-independent administration workflows.

#![allow(dead_code)]

use std::{error::Error as StdError, fmt};

use weavelit_server_administration::{
    AccountAdministrationAction, AccountAdministrationRead, AccountCreate, AccountPasswordReset,
    AccountStatusChange, AdministrationAction, AuthorizedAdministrationAction, GroupMutation,
    LogConfigurationChange,
};
use weavelit_server_audit::{
    AccountStatus as AuditAccountStatus, ActionOutcome, AuditActor, AuditEvent, AuditOutcomeDetail,
    ComponentState, GrantReference, GroupMutationOutcome as AuditGroupMutationOutcome,
    LogConfigurationAuditReferences, MfaModuleChange, MfaModuleReference, MfaRequirement,
    MfaResetState, StateChangeOutcome,
};
use weavelit_server_authentication::{
    Argon2Engine, PasswordVerifierFactory, PreparedTemporaryPassword, TEMPORARY_PASSWORD_LIFETIME,
    TemporaryPasswordDisclosure,
};
use weavelit_server_database::{
    Account, AccountAdministrationProjection, AccountAuditReference, AccountCreateMutation,
    AccountCreateOutcome, AccountCredentialAuditTerminalWrites, AccountPasswordResetMutation,
    AccountPasswordResetOutcome, AccountPasswordVerifier, AccountPublicIdentifier,
    AccountPublicIdentity, AccountStatus, AccountStatusAuditTerminalWrites, AccountStatusMutation,
    AccountStatusMutationError, AccountStatusMutationOutcome, AccountStatusRecheck,
    AuditReferenceIdentifier, ComponentKind, CredentialRevision, DatabaseError, GroupGrant,
    GroupMutationAuditTerminalWrites, GroupMutationError, GroupMutationOutcome,
    GroupMutationRecheck, GroupMutationTarget, LogAssignment, LogConfigurationAuditTerminalWrites,
    LogConfigurationMutationOutcome, LogConfigurationMutationRequest, LogConfigurationPreparation,
    LogType, MfaEnablementAuditTerminalWrites, MfaEnablementOutcome, MfaModuleTarget,
    MfaPolicyAction, MfaPolicyAuditTerminalWrites, MfaPolicyMutation, MfaPolicyMutationError,
    MfaPolicyMutationOutcome, MfaPolicyRecheck, Name, PasswordVerifier, PreparedGroupMutation,
    SessionInstant, StateIdentifier, TemporaryCredentialExpiration,
    ValidatedAuditTerminalObligationWrite,
};
use weavelit_server_log::{
    AuditLogClassification, CorrelationId, EventTime, LogRecordPersistenceView, LogRecordType,
};

use crate::{
    authentication::{
        AccountCredentialIssuanceAdmission, AccountCredentialIssuanceError, AuthenticationRuntime,
        correlation_identifier, random_bytes, system_clock,
    },
    operational::OperationalDatabase,
    operational_audit::{
        AuditRecoverySequenceState, OperationalAuditGenerationDestination, OperationalAuditRecovery,
    },
    operational_logging::ConsequentialOperationError,
};

const TOTP_MODULE: &str = weavelit_module_mfa_totp::MODULE_IDENTIFIER;

/// Complete internal result of one authorized account administration read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AccountAdministrationReadResult {
    /// Every account in deterministic store order.
    List(Vec<AccountAdministrationProjection>),
    /// The exact account, or safe absence when the public identifier is unknown.
    View(Option<AccountAdministrationProjection>),
}

/// Payload-free refusal of an account administration read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AccountAdministrationReadError {
    /// The consumed action was not an account read.
    ActionNotSupported,
    /// The selected Application Database could not safely serve the read.
    Unavailable,
}

impl fmt::Display for AccountAdministrationReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ActionNotSupported => formatter.write_str("administration action not supported"),
            Self::Unavailable => formatter.write_str("account administration is unavailable"),
        }
    }
}

impl StdError for AccountAdministrationReadError {}

/// Read-only workflow for one exact authorized account administration action.
pub(crate) struct AccountAdministrationReadWorkflow<'a> {
    database: &'a OperationalDatabase,
}

/// Postcommit Audit terminal delivery state for an account credential writer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AccountCredentialIssuanceDelivery {
    /// Exact destination delivery and database acknowledgement completed.
    Acknowledged,
    /// The committed obligation remains available for bounded restart recovery.
    Pending,
}

/// Complete internal result of one account credential writer.
pub(crate) enum AccountCredentialIssuanceResult {
    /// Account state committed and the temporary password may be disclosed once.
    Created {
        account: AccountPublicIdentifier,
        temporary_password: TemporaryPasswordDisclosure,
        delivery: AccountCredentialIssuanceDelivery,
    },
    /// Password reset committed and the new temporary password may be disclosed once.
    PasswordReset {
        account: AccountPublicIdentifier,
        temporary_password: TemporaryPasswordDisclosure,
        delivery: AccountCredentialIssuanceDelivery,
    },
    /// A username or generated identity collided; no account or disclosure committed.
    Conflict {
        delivery: AccountCredentialIssuanceDelivery,
    },
    /// The prepared reset target revision changed; no credential or disclosure committed.
    Stale {
        delivery: AccountCredentialIssuanceDelivery,
    },
    /// Final issuer state denied the mutation; no business state or disclosure committed.
    Denied {
        delivery: AccountCredentialIssuanceDelivery,
    },
}

impl fmt::Debug for AccountCredentialIssuanceResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AccountCredentialIssuanceResult(REDACTED)")
    }
}

/// Payload-free refusal before or during an account credential writer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AccountCredentialIssuanceWorkflowError {
    /// The consumed authorization was not an account credential writer action.
    ActionNotSupported,
    /// Fresh issuer credentials were not accepted.
    CredentialDenied,
    /// The exact password-reset target did not exist at preparation.
    TargetNotFound,
    /// Credential, identifier, clock, or selected database preparation failed.
    Unavailable,
    /// Required Audit recovery, destination, Attempt, terminal, or commit failed.
    AuditLogUnavailable,
}

impl fmt::Display for AccountCredentialIssuanceWorkflowError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ActionNotSupported => "administration action not supported",
            Self::CredentialDenied => "account credential issuance denied",
            Self::TargetNotFound => "account credential target not found",
            Self::Unavailable => "account credential issuance unavailable",
            Self::AuditLogUnavailable => "consequential operation Audit Log unavailable",
        })
    }
}

impl StdError for AccountCredentialIssuanceWorkflowError {}

/// Transport-independent Administrator account creation and password reset workflow.
pub(crate) struct AccountCredentialIssuanceWorkflow<'a, E> {
    database: &'a OperationalDatabase,
    authentication: &'a AuthenticationRuntime<E>,
    audit: &'a OperationalAuditRecovery,
}

impl<'a, E> AccountCredentialIssuanceWorkflow<'a, E>
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

    /// Consumes authorization and one assurance ticket, then commits one audited outcome.
    pub(crate) fn issue(
        &self,
        authorization: AuthorizedAdministrationAction,
        credential_issuance_ticket: &str,
    ) -> Result<AccountCredentialIssuanceResult, AccountCredentialIssuanceWorkflowError> {
        let authorization = authorization
            .into_account()
            .map_err(|_| AccountCredentialIssuanceWorkflowError::ActionNotSupported)?;
        let admission = self
            .authentication
            .claim_credential_issuance_ticket(authorization, credential_issuance_ticket)
            .map_err(issuance_admission_error)?;
        if self.audit.drain_before_consequential_operation().active()
            != AuditRecoverySequenceState::Ready
        {
            return Err(AccountCredentialIssuanceWorkflowError::AuditLogUnavailable);
        }

        let action = admission.action().clone();
        match action {
            AccountAdministrationAction::Create(change) => self.create(admission, change),
            AccountAdministrationAction::PasswordReset(change) => self.reset(admission, change),
            AccountAdministrationAction::Read(_) | AccountAdministrationAction::StatusChange(_) => {
                Err(AccountCredentialIssuanceWorkflowError::ActionNotSupported)
            }
        }
    }

    fn create(
        &self,
        admission: AccountCredentialIssuanceAdmission,
        change: AccountCreate,
    ) -> Result<AccountCredentialIssuanceResult, AccountCredentialIssuanceWorkflowError> {
        let prepared = PreparedTemporaryPassword::generate(&PasswordVerifierFactory::approved())
            .map_err(|_| AccountCredentialIssuanceWorkflowError::Unavailable)?;
        let account = issue_state_identifier()?;
        let public_identifier = AccountPublicIdentifier::generate()
            .map_err(|_| AccountCredentialIssuanceWorkflowError::Unavailable)?;
        let target = AccountAuditReference::new(
            account,
            AuditReferenceIdentifier::generate()
                .map_err(|_| AccountCredentialIssuanceWorkflowError::Unavailable)?,
        );
        let actor = self.actor_reference(&admission)?;
        let event = AuditEvent::AuthenticationUserCreated { account: target };

        self.audit
            .with_current_destination(|destination| {
                destination
                    .destination()
                    .preflight(LogRecordType::Audit)
                    .map_err(|_| AccountCredentialIssuanceWorkflowError::AuditLogUnavailable)?;
                let terminals = self.prepare_account_terminals(
                    actor,
                    event,
                    AuditLogClassification::AuthenticationUserCreated,
                    destination,
                )?;
                let (action, recheck) = self
                    .authentication
                    .prepare_account_credential_issuance_recheck(admission)
                    .map_err(issuance_admission_error)?;
                if action != AccountAdministrationAction::Create(change.clone()) {
                    return Err(AccountCredentialIssuanceWorkflowError::ActionNotSupported);
                }
                let expiration = temporary_expiration(recheck.now())?;
                let (verifier, disclosure) = prepared.into_parts();
                let mutation = AccountCreateMutation::new(
                    recheck,
                    Account {
                        identifier: account,
                        username: change.username().clone(),
                        display_name: change.display_name().cloned(),
                        active: true,
                        mfa_required: false,
                        credential_revision: CredentialRevision::INITIAL,
                        must_change_password: true,
                        temporary_credential_expiration: Some(expiration),
                    },
                    AccountPublicIdentity::new(account, public_identifier),
                    target,
                    AccountPasswordVerifier {
                        account,
                        verifier: PasswordVerifier::new(verifier.into_string())
                            .map_err(|_| AccountCredentialIssuanceWorkflowError::Unavailable)?,
                    },
                )
                .map_err(|_| AccountCredentialIssuanceWorkflowError::Unavailable)?;
                let outcome = self
                    .database
                    .create_account(&mutation, &terminals.writes())
                    .map_err(|_| AccountCredentialIssuanceWorkflowError::AuditLogUnavailable)?;
                Ok(self.create_result(outcome, public_identifier, disclosure))
            })
            .map_err(|_| AccountCredentialIssuanceWorkflowError::AuditLogUnavailable)?
    }

    fn reset(
        &self,
        admission: AccountCredentialIssuanceAdmission,
        change: AccountPasswordReset,
    ) -> Result<AccountCredentialIssuanceResult, AccountCredentialIssuanceWorkflowError> {
        let target = self
            .database
            .prepare_account_password_reset_target(change.target())
            .map_err(|_| AccountCredentialIssuanceWorkflowError::Unavailable)?
            .ok_or(AccountCredentialIssuanceWorkflowError::TargetNotFound)?;
        let target_account = target.account();
        let prepared = PreparedTemporaryPassword::generate(&PasswordVerifierFactory::approved())
            .map_err(|_| AccountCredentialIssuanceWorkflowError::Unavailable)?;
        let actor = self.actor_reference(&admission)?;
        let event = AuditEvent::AuthenticationPasswordResetStarted {
            account: target.audit_reference(),
        };

        self.audit
            .with_current_destination(|destination| {
                destination
                    .destination()
                    .preflight(LogRecordType::Audit)
                    .map_err(|_| AccountCredentialIssuanceWorkflowError::AuditLogUnavailable)?;
                let terminals = self.prepare_account_terminals(
                    actor,
                    event,
                    AuditLogClassification::AuthenticationPasswordResetStarted,
                    destination,
                )?;
                let (action, recheck) = self
                    .authentication
                    .prepare_account_credential_issuance_recheck(admission)
                    .map_err(issuance_admission_error)?;
                if action != AccountAdministrationAction::PasswordReset(change) {
                    return Err(AccountCredentialIssuanceWorkflowError::ActionNotSupported);
                }
                let expiration = temporary_expiration(recheck.now())?;
                let (verifier, disclosure) = prepared.into_parts();
                let mutation = AccountPasswordResetMutation::new(
                    recheck,
                    target,
                    expiration,
                    AccountPasswordVerifier {
                        account: target_account,
                        verifier: PasswordVerifier::new(verifier.into_string())
                            .map_err(|_| AccountCredentialIssuanceWorkflowError::Unavailable)?,
                    },
                )
                .map_err(|_| AccountCredentialIssuanceWorkflowError::Unavailable)?;
                let outcome = self
                    .database
                    .reset_account_password(&mutation, &terminals.writes())
                    .map_err(|_| AccountCredentialIssuanceWorkflowError::AuditLogUnavailable)?;
                Ok(self.reset_result(outcome, change.target(), disclosure))
            })
            .map_err(|_| AccountCredentialIssuanceWorkflowError::AuditLogUnavailable)?
    }

    fn actor_reference(
        &self,
        admission: &AccountCredentialIssuanceAdmission,
    ) -> Result<AccountAuditReference, AccountCredentialIssuanceWorkflowError> {
        self.database
            .load_account_audit_reference(admission.actor())
            .map_err(|_| AccountCredentialIssuanceWorkflowError::Unavailable)?
            .ok_or(AccountCredentialIssuanceWorkflowError::Unavailable)
    }

    fn prepare_account_terminals(
        &self,
        actor: AccountAuditReference,
        event: AuditEvent,
        classification: AuditLogClassification,
        destination: &OperationalAuditGenerationDestination,
    ) -> Result<PreparedAccountCredentialTerminals, AccountCredentialIssuanceWorkflowError> {
        let correlation = CorrelationId::new(
            correlation_identifier().ok_or(AccountCredentialIssuanceWorkflowError::Unavailable)?,
        )
        .map_err(|_| AccountCredentialIssuanceWorkflowError::Unavailable)?;
        let attempt = self
            .audit
            .producer()
            .prepare_attempt(
                account_event_time()?,
                correlation,
                AuditActor::Human(actor),
                event,
            )
            .map_err(|_| AccountCredentialIssuanceWorkflowError::AuditLogUnavailable)?;
        let LogRecordPersistenceView::Audit(attempt_record) = attempt.record().persistence_view()
        else {
            return Err(AccountCredentialIssuanceWorkflowError::AuditLogUnavailable);
        };
        let attempt_identifier = *attempt_record.record_id().as_bytes();
        let attempt_event_time = attempt_record.event_time().unix_milliseconds();
        let attempt_correlation = attempt_record.correlation_id().as_str().to_owned();
        let delivered = match attempt.deliver(destination.destination()) {
            Ok(attempt) => attempt,
            Err(error) => {
                self.audit.reject_attempt_delivery(
                    error,
                    attempt_identifier,
                    attempt_event_time,
                    &attempt_correlation,
                    destination.module(),
                    classification,
                );
                return Err(AccountCredentialIssuanceWorkflowError::AuditLogUnavailable);
            }
        };
        let succeeded = self
            .audit
            .producer()
            .prepare_completion(
                &delivered,
                account_event_time()?,
                account_audit_detail(classification, ActionOutcome::Succeeded),
            )
            .map_err(|_| AccountCredentialIssuanceWorkflowError::AuditLogUnavailable)?;
        let conflict = self
            .audit
            .producer()
            .prepare_completion(
                &delivered,
                account_event_time()?,
                account_audit_detail(classification, ActionOutcome::Denied),
            )
            .map_err(|_| AccountCredentialIssuanceWorkflowError::AuditLogUnavailable)?;
        let denied = self
            .audit
            .producer()
            .prepare_completion(
                &delivered,
                account_event_time()?,
                account_audit_detail(classification, ActionOutcome::Denied),
            )
            .map_err(|_| AccountCredentialIssuanceWorkflowError::AuditLogUnavailable)?;
        let persistence = self.database.audit_terminal_recovery_persistence();
        Ok(PreparedAccountCredentialTerminals {
            succeeded: succeeded
                .recovery_obligation(persistence, destination.binding())
                .map_err(|_| AccountCredentialIssuanceWorkflowError::AuditLogUnavailable)?,
            conflict: conflict
                .recovery_obligation(persistence, destination.binding())
                .map_err(|_| AccountCredentialIssuanceWorkflowError::AuditLogUnavailable)?,
            denied: denied
                .recovery_obligation(persistence, destination.binding())
                .map_err(|_| AccountCredentialIssuanceWorkflowError::AuditLogUnavailable)?,
        })
    }

    fn create_result(
        &self,
        outcome: AccountCreateOutcome,
        account: AccountPublicIdentifier,
        disclosure: TemporaryPasswordDisclosure,
    ) -> AccountCredentialIssuanceResult {
        let delivery = self.postcommit_delivery();
        match outcome {
            AccountCreateOutcome::Created => AccountCredentialIssuanceResult::Created {
                account,
                temporary_password: disclosure,
                delivery,
            },
            AccountCreateOutcome::Conflict => {
                drop(disclosure);
                AccountCredentialIssuanceResult::Conflict { delivery }
            }
            AccountCreateOutcome::Denied => {
                drop(disclosure);
                AccountCredentialIssuanceResult::Denied { delivery }
            }
        }
    }

    fn reset_result(
        &self,
        outcome: AccountPasswordResetOutcome,
        account: AccountPublicIdentifier,
        disclosure: TemporaryPasswordDisclosure,
    ) -> AccountCredentialIssuanceResult {
        let delivery = self.postcommit_delivery();
        match outcome {
            AccountPasswordResetOutcome::Reset { .. } => {
                AccountCredentialIssuanceResult::PasswordReset {
                    account,
                    temporary_password: disclosure,
                    delivery,
                }
            }
            AccountPasswordResetOutcome::Stale => {
                drop(disclosure);
                AccountCredentialIssuanceResult::Stale { delivery }
            }
            AccountPasswordResetOutcome::Denied => {
                drop(disclosure);
                AccountCredentialIssuanceResult::Denied { delivery }
            }
        }
    }

    fn postcommit_delivery(&self) -> AccountCredentialIssuanceDelivery {
        if self.audit.drain_after_consequential_operation().active()
            == AuditRecoverySequenceState::Ready
        {
            AccountCredentialIssuanceDelivery::Acknowledged
        } else {
            AccountCredentialIssuanceDelivery::Pending
        }
    }
}

struct PreparedAccountCredentialTerminals {
    succeeded: ValidatedAuditTerminalObligationWrite,
    conflict: ValidatedAuditTerminalObligationWrite,
    denied: ValidatedAuditTerminalObligationWrite,
}

impl PreparedAccountCredentialTerminals {
    fn writes(&self) -> AccountCredentialAuditTerminalWrites<'_> {
        AccountCredentialAuditTerminalWrites::new(&self.succeeded, &self.conflict, &self.denied)
    }
}

fn account_audit_detail(
    classification: AuditLogClassification,
    outcome: ActionOutcome,
) -> AuditOutcomeDetail {
    match classification {
        AuditLogClassification::AuthenticationUserCreated => {
            AuditOutcomeDetail::AuthenticationUserCreated(outcome)
        }
        AuditLogClassification::AuthenticationPasswordResetStarted => {
            AuditOutcomeDetail::AuthenticationPasswordResetStarted(outcome)
        }
        AuditLogClassification::LifecycleBackupCreated
        | AuditLogClassification::AuthenticationUserDisabled
        | AuditLogClassification::AuthenticationPasswordChanged
        | AuditLogClassification::AuthenticationMfaEnrolled
        | AuditLogClassification::AuthenticationMfaReset
        | AuditLogClassification::AuthenticationMfaRequirementChanged
        | AuditLogClassification::AuthenticationMfaModuleEnablementChanged
        | AuditLogClassification::AuthenticationSessionRevoked
        | AuditLogClassification::AuthorizationGroupCreated
        | AuditLogClassification::AuthorizationGroupMembershipChanged
        | AuditLogClassification::AuthorizationGroupGrantChanged
        | AuditLogClassification::AuthorizationGroupGrantRemovalDenied
        | AuditLogClassification::AuthorizationAutomationScopeChanged
        | AuditLogClassification::DependencyAuditTerminalSuperseded
        | AuditLogClassification::DependencyLogModuleConfigurationChanged
        | AuditLogClassification::DependencyServiceConnectionChanged
        | AuditLogClassification::ProviderOperationStarted
        | AuditLogClassification::ProviderOperationCompleted
        | AuditLogClassification::InternalServerConfigurationChanged
        | AuditLogClassification::InternalUserStatusChanged
        | AuditLogClassification::InternalLogPolicyChanged => {
            unreachable!("account writers pass only their two classifications")
        }
    }
}

fn temporary_expiration(
    now: weavelit_server_database::SessionInstant,
) -> Result<TemporaryCredentialExpiration, AccountCredentialIssuanceWorkflowError> {
    let lifetime = i64::try_from(TEMPORARY_PASSWORD_LIFETIME.as_millis())
        .map_err(|_| AccountCredentialIssuanceWorkflowError::Unavailable)?;
    let expiration = now
        .as_unix_milliseconds()
        .checked_add(lifetime)
        .ok_or(AccountCredentialIssuanceWorkflowError::Unavailable)?;
    TemporaryCredentialExpiration::from_unix_milliseconds(expiration)
        .map_err(|_| AccountCredentialIssuanceWorkflowError::Unavailable)
}

fn issue_state_identifier() -> Result<StateIdentifier, AccountCredentialIssuanceWorkflowError> {
    for _ in 0..8 {
        let bytes =
            random_bytes::<16>().ok_or(AccountCredentialIssuanceWorkflowError::Unavailable)?;
        if let Ok(identifier) = StateIdentifier::from_bytes(bytes) {
            return Ok(identifier);
        }
    }
    Err(AccountCredentialIssuanceWorkflowError::Unavailable)
}

fn account_event_time() -> Result<EventTime, AccountCredentialIssuanceWorkflowError> {
    let milliseconds =
        system_clock()().ok_or(AccountCredentialIssuanceWorkflowError::AuditLogUnavailable)?;
    let milliseconds = u64::try_from(milliseconds)
        .map_err(|_| AccountCredentialIssuanceWorkflowError::AuditLogUnavailable)?;
    Ok(EventTime::from_unix_milliseconds(milliseconds))
}

fn issuance_admission_error(
    error: AccountCredentialIssuanceError,
) -> AccountCredentialIssuanceWorkflowError {
    match error {
        AccountCredentialIssuanceError::Denied => {
            AccountCredentialIssuanceWorkflowError::CredentialDenied
        }
        AccountCredentialIssuanceError::Unavailable => {
            AccountCredentialIssuanceWorkflowError::Unavailable
        }
    }
}

/// Postcommit Audit terminal delivery state for an account status writer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AccountStatusChangeDelivery {
    /// Exact destination delivery and database acknowledgement completed.
    Acknowledged,
    /// The committed obligation remains available for bounded restart recovery.
    Pending,
}

/// Complete internal result of one account status writer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AccountStatusChangeResult {
    /// The target already had the requested status; no Audit record was produced.
    Unchanged,
    /// The status change committed with its selected terminal.
    Changed {
        status: AccountStatus,
        revoked_sessions: usize,
        delivery: AccountStatusChangeDelivery,
    },
    /// The prepared target changed; only the denied terminal committed.
    Stale {
        delivery: AccountStatusChangeDelivery,
    },
    /// Final issuer state denied the mutation; only the denied terminal committed.
    Denied {
        delivery: AccountStatusChangeDelivery,
    },
}

/// Payload-free refusal before or during an account status writer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AccountStatusChangeError {
    /// The consumed authorization was not an account status action.
    ActionNotSupported,
    /// The exact target did not exist at preparation.
    TargetNotFound,
    /// Disablement could not advance the maximal credential revision.
    CredentialRevisionExhausted,
    /// Clock or selected database preparation failed.
    Unavailable,
    /// Required Audit recovery, destination, Attempt, terminal, or commit failed.
    AuditLogUnavailable,
}

impl fmt::Display for AccountStatusChangeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ActionNotSupported => "administration action not supported",
            Self::TargetNotFound => "account status target not found",
            Self::CredentialRevisionExhausted => "account status change rejected",
            Self::Unavailable => "account status change unavailable",
            Self::AuditLogUnavailable => "consequential operation Audit Log unavailable",
        })
    }
}

impl StdError for AccountStatusChangeError {}

/// Transport-independent Administrator account disable and re-enable workflow.
pub(crate) struct AccountStatusChangeWorkflow<'a> {
    database: &'a OperationalDatabase,
    audit: &'a OperationalAuditRecovery,
}

impl<'a> AccountStatusChangeWorkflow<'a> {
    pub(crate) const fn new(
        database: &'a OperationalDatabase,
        audit: &'a OperationalAuditRecovery,
    ) -> Self {
        Self { database, audit }
    }

    /// Consumes ordinary account authorization and commits one audited status outcome.
    pub(crate) fn apply(
        &self,
        authorization: AuthorizedAdministrationAction,
    ) -> Result<AccountStatusChangeResult, AccountStatusChangeError> {
        let authorization = authorization
            .into_account()
            .map_err(|_| AccountStatusChangeError::ActionNotSupported)?;
        let AccountAdministrationAction::StatusChange(change) = authorization.action() else {
            return Err(AccountStatusChangeError::ActionNotSupported);
        };
        let change = *change;
        let target = self
            .database
            .prepare_account_status_target(change.target())
            .map_err(|_| AccountStatusChangeError::Unavailable)?
            .ok_or(AccountStatusChangeError::TargetNotFound)?;
        if target.status() == change.desired() {
            return Ok(AccountStatusChangeResult::Unchanged);
        }

        let now = system_clock()().ok_or(AccountStatusChangeError::Unavailable)?;
        let recheck = AccountStatusRecheck::new(
            authorization.actor(),
            authorization.session(),
            authorization.client_module().clone(),
            SessionInstant::from_unix_milliseconds(now)
                .map_err(|_| AccountStatusChangeError::Unavailable)?,
        );
        let mutation =
            AccountStatusMutation::new(recheck, target, change.desired()).map_err(|error| {
                match error {
                    AccountStatusMutationError::CredentialRevisionExhausted => {
                        AccountStatusChangeError::CredentialRevisionExhausted
                    }
                    AccountStatusMutationError::Unchanged
                    | AccountStatusMutationError::InvalidTarget => {
                        AccountStatusChangeError::Unavailable
                    }
                }
            })?;
        if self.audit.drain_before_consequential_operation().active()
            != AuditRecoverySequenceState::Ready
        {
            return Err(AccountStatusChangeError::AuditLogUnavailable);
        }

        let actor = self
            .database
            .load_account_audit_reference(authorization.actor())
            .map_err(|_| AccountStatusChangeError::Unavailable)?
            .ok_or(AccountStatusChangeError::Unavailable)?;
        let event = account_status_event(change, mutation.target().audit_reference());
        self.audit
            .with_current_destination(|destination| {
                self.apply_with_destination(mutation, actor, event, destination)
            })
            .map_err(|_| AccountStatusChangeError::AuditLogUnavailable)?
    }

    fn apply_with_destination(
        &self,
        mutation: AccountStatusMutation,
        actor: AccountAuditReference,
        event: AuditEvent,
        destination: &OperationalAuditGenerationDestination,
    ) -> Result<AccountStatusChangeResult, AccountStatusChangeError> {
        destination
            .destination()
            .preflight(LogRecordType::Audit)
            .map_err(|_| AccountStatusChangeError::AuditLogUnavailable)?;
        let correlation = CorrelationId::new(
            correlation_identifier().ok_or(AccountStatusChangeError::Unavailable)?,
        )
        .map_err(|_| AccountStatusChangeError::Unavailable)?;
        let classification = account_status_classification(mutation.desired());
        let attempt = self
            .audit
            .producer()
            .prepare_attempt(
                account_status_event_time()?,
                correlation,
                AuditActor::Human(actor),
                event,
            )
            .map_err(|_| AccountStatusChangeError::AuditLogUnavailable)?;
        let LogRecordPersistenceView::Audit(attempt_record) = attempt.record().persistence_view()
        else {
            return Err(AccountStatusChangeError::AuditLogUnavailable);
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
                    classification,
                );
                return Err(AccountStatusChangeError::AuditLogUnavailable);
            }
        };
        let succeeded_terminal = self
            .audit
            .producer()
            .prepare_completion(
                &delivered_attempt,
                account_status_event_time()?,
                account_status_audit_detail(mutation.desired(), true),
            )
            .map_err(|_| AccountStatusChangeError::AuditLogUnavailable)?;
        let denied_terminal = self
            .audit
            .producer()
            .prepare_completion(
                &delivered_attempt,
                account_status_event_time()?,
                account_status_audit_detail(mutation.desired(), false),
            )
            .map_err(|_| AccountStatusChangeError::AuditLogUnavailable)?;
        let persistence = self.database.audit_terminal_recovery_persistence();
        let succeeded_write = succeeded_terminal
            .recovery_obligation(persistence, destination.binding())
            .map_err(|_| AccountStatusChangeError::AuditLogUnavailable)?;
        let denied_write = denied_terminal
            .recovery_obligation(persistence, destination.binding())
            .map_err(|_| AccountStatusChangeError::AuditLogUnavailable)?;
        let outcome = self
            .database
            .change_account_status(
                &mutation,
                &AccountStatusAuditTerminalWrites::new(&succeeded_write, &denied_write),
            )
            .map_err(|_| AccountStatusChangeError::AuditLogUnavailable)?;
        let delivery = if self.audit.drain_after_consequential_operation().active()
            == AuditRecoverySequenceState::Ready
        {
            AccountStatusChangeDelivery::Acknowledged
        } else {
            AccountStatusChangeDelivery::Pending
        };

        Ok(match outcome {
            AccountStatusMutationOutcome::Changed { revoked_sessions } => {
                AccountStatusChangeResult::Changed {
                    status: mutation.desired(),
                    revoked_sessions,
                    delivery,
                }
            }
            AccountStatusMutationOutcome::Stale => AccountStatusChangeResult::Stale { delivery },
            AccountStatusMutationOutcome::Denied => AccountStatusChangeResult::Denied { delivery },
        })
    }
}

fn account_status_event(change: AccountStatusChange, account: AccountAuditReference) -> AuditEvent {
    match change.desired() {
        AccountStatus::Disabled => AuditEvent::AuthenticationUserDisabled { account },
        AccountStatus::Active => AuditEvent::InternalUserStatusChanged { account },
    }
}

fn account_status_audit_detail(status: AccountStatus, succeeded: bool) -> AuditOutcomeDetail {
    let outcome = if succeeded {
        StateChangeOutcome::Succeeded(match status {
            AccountStatus::Active => AuditAccountStatus::Active,
            AccountStatus::Disabled => AuditAccountStatus::Disabled,
        })
    } else {
        StateChangeOutcome::Denied
    };
    match status {
        AccountStatus::Disabled => AuditOutcomeDetail::AuthenticationUserDisabled(outcome),
        AccountStatus::Active => AuditOutcomeDetail::InternalUserStatusChanged(outcome),
    }
}

const fn account_status_classification(status: AccountStatus) -> AuditLogClassification {
    match status {
        AccountStatus::Disabled => AuditLogClassification::AuthenticationUserDisabled,
        AccountStatus::Active => AuditLogClassification::InternalUserStatusChanged,
    }
}

fn account_status_event_time() -> Result<EventTime, AccountStatusChangeError> {
    let milliseconds = system_clock()().ok_or(AccountStatusChangeError::AuditLogUnavailable)?;
    let milliseconds =
        u64::try_from(milliseconds).map_err(|_| AccountStatusChangeError::AuditLogUnavailable)?;
    Ok(EventTime::from_unix_milliseconds(milliseconds))
}

/// Postcommit Audit terminal delivery state for one MFA policy writer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MfaPolicyChangeDelivery {
    Acknowledged,
    Pending,
}

/// Complete internal result of one MFA requirement or enrollment-reset writer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MfaPolicyChangeResult {
    /// The requested requirement already held or no enrollment existed to reset.
    Unchanged,
    /// The requested policy state committed with its selected terminal.
    Changed {
        revoked_sessions: usize,
        delivery: MfaPolicyChangeDelivery,
    },
    /// The prepared target state changed; only the denied terminal committed.
    Stale { delivery: MfaPolicyChangeDelivery },
    /// Final issuer state denied the mutation; only the denied terminal committed.
    Denied { delivery: MfaPolicyChangeDelivery },
}

/// Payload-free refusal before or during an MFA policy writer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MfaPolicyChangeError {
    ActionNotSupported,
    TargetNotFound,
    Unavailable,
    AuditLogUnavailable,
}

impl fmt::Display for MfaPolicyChangeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ActionNotSupported => "administration action not supported",
            Self::TargetNotFound => "MFA policy target not found",
            Self::Unavailable => "MFA policy change unavailable",
            Self::AuditLogUnavailable => "consequential operation Audit Log unavailable",
        })
    }
}

impl StdError for MfaPolicyChangeError {}

/// Transport-independent Administrator MFA requirement and enrollment-reset workflow.
pub(crate) struct MfaPolicyChangeWorkflow<'a> {
    database: &'a OperationalDatabase,
    audit: &'a OperationalAuditRecovery,
}

impl<'a> MfaPolicyChangeWorkflow<'a> {
    pub(crate) const fn new(
        database: &'a OperationalDatabase,
        audit: &'a OperationalAuditRecovery,
    ) -> Self {
        Self { database, audit }
    }

    pub(crate) fn apply(
        &self,
        authorization: AuthorizedAdministrationAction,
        target: AccountPublicIdentifier,
        action: MfaPolicyAction,
    ) -> Result<MfaPolicyChangeResult, MfaPolicyChangeError> {
        let authorization = authorization
            .into_mfa_policy()
            .map_err(|_| MfaPolicyChangeError::ActionNotSupported)?;
        let module = Name::new(TOTP_MODULE).map_err(|_| MfaPolicyChangeError::Unavailable)?;
        let target = self
            .database
            .prepare_mfa_policy_target(target, &module)
            .map_err(|_| MfaPolicyChangeError::Unavailable)?
            .ok_or(MfaPolicyChangeError::TargetNotFound)?;
        let is_unchanged = match action {
            MfaPolicyAction::Requirement { required } => target.required() == required,
            MfaPolicyAction::EnrollmentReset => target.factor().is_none(),
        };
        if is_unchanged {
            return Ok(MfaPolicyChangeResult::Unchanged);
        }
        let now = system_clock()().ok_or(MfaPolicyChangeError::Unavailable)?;
        let recheck = MfaPolicyRecheck::new(
            authorization.actor(),
            authorization.session(),
            authorization.client_module().clone(),
            MfaModuleTarget {
                module: module.clone(),
                component: module,
            },
            authorization.factor(),
            SessionInstant::from_unix_milliseconds(now)
                .map_err(|_| MfaPolicyChangeError::Unavailable)?,
        );
        let mutation =
            MfaPolicyMutation::new(recheck, target, action).map_err(|error| match error {
                MfaPolicyMutationError::Unchanged | MfaPolicyMutationError::InvalidTarget => {
                    MfaPolicyChangeError::Unavailable
                }
            })?;
        if self.audit.drain_before_consequential_operation().active()
            != AuditRecoverySequenceState::Ready
        {
            return Err(MfaPolicyChangeError::AuditLogUnavailable);
        }
        let actor = self
            .database
            .load_account_audit_reference(authorization.actor())
            .map_err(|_| MfaPolicyChangeError::Unavailable)?
            .ok_or(MfaPolicyChangeError::Unavailable)?;
        let event = mfa_policy_event(action, mutation.target().audit_reference());
        self.audit
            .with_current_destination(|destination| {
                self.apply_with_destination(mutation, actor, event, destination)
            })
            .map_err(|_| MfaPolicyChangeError::AuditLogUnavailable)?
    }

    fn apply_with_destination(
        &self,
        mutation: MfaPolicyMutation,
        actor: AccountAuditReference,
        event: AuditEvent,
        destination: &OperationalAuditGenerationDestination,
    ) -> Result<MfaPolicyChangeResult, MfaPolicyChangeError> {
        destination
            .destination()
            .preflight(LogRecordType::Audit)
            .map_err(|_| MfaPolicyChangeError::AuditLogUnavailable)?;
        let correlation =
            CorrelationId::new(correlation_identifier().ok_or(MfaPolicyChangeError::Unavailable)?)
                .map_err(|_| MfaPolicyChangeError::Unavailable)?;
        let classification = mfa_policy_classification(mutation.action());
        let attempt = self
            .audit
            .producer()
            .prepare_attempt(
                mfa_policy_event_time()?,
                correlation,
                AuditActor::Human(actor),
                event,
            )
            .map_err(|_| MfaPolicyChangeError::AuditLogUnavailable)?;
        let LogRecordPersistenceView::Audit(attempt_record) = attempt.record().persistence_view()
        else {
            return Err(MfaPolicyChangeError::AuditLogUnavailable);
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
                    classification,
                );
                return Err(MfaPolicyChangeError::AuditLogUnavailable);
            }
        };
        let succeeded_terminal = self
            .audit
            .producer()
            .prepare_completion(
                &delivered_attempt,
                mfa_policy_event_time()?,
                mfa_policy_audit_detail(mutation.action(), true),
            )
            .map_err(|_| MfaPolicyChangeError::AuditLogUnavailable)?;
        let denied_terminal = self
            .audit
            .producer()
            .prepare_completion(
                &delivered_attempt,
                mfa_policy_event_time()?,
                mfa_policy_audit_detail(mutation.action(), false),
            )
            .map_err(|_| MfaPolicyChangeError::AuditLogUnavailable)?;
        let persistence = self.database.audit_terminal_recovery_persistence();
        let succeeded_write = succeeded_terminal
            .recovery_obligation(persistence, destination.binding())
            .map_err(|_| MfaPolicyChangeError::AuditLogUnavailable)?;
        let denied_write = denied_terminal
            .recovery_obligation(persistence, destination.binding())
            .map_err(|_| MfaPolicyChangeError::AuditLogUnavailable)?;
        let outcome = self
            .database
            .change_mfa_policy(
                &mutation,
                &MfaPolicyAuditTerminalWrites::new(&succeeded_write, &denied_write),
            )
            .map_err(|_| MfaPolicyChangeError::AuditLogUnavailable)?;
        let delivery = if self.audit.drain_after_consequential_operation().active()
            == AuditRecoverySequenceState::Ready
        {
            MfaPolicyChangeDelivery::Acknowledged
        } else {
            MfaPolicyChangeDelivery::Pending
        };
        Ok(match outcome {
            MfaPolicyMutationOutcome::Changed { revoked_sessions } => {
                MfaPolicyChangeResult::Changed {
                    revoked_sessions,
                    delivery,
                }
            }
            MfaPolicyMutationOutcome::Stale => MfaPolicyChangeResult::Stale { delivery },
            MfaPolicyMutationOutcome::Denied => MfaPolicyChangeResult::Denied { delivery },
        })
    }
}

fn mfa_policy_event(action: MfaPolicyAction, account: AccountAuditReference) -> AuditEvent {
    match action {
        MfaPolicyAction::Requirement { .. } => {
            AuditEvent::AuthenticationMfaRequirementChanged { account }
        }
        MfaPolicyAction::EnrollmentReset => AuditEvent::AuthenticationMfaReset { account },
    }
}

fn mfa_policy_audit_detail(action: MfaPolicyAction, succeeded: bool) -> AuditOutcomeDetail {
    match action {
        MfaPolicyAction::Requirement { required } => {
            AuditOutcomeDetail::AuthenticationMfaRequirementChanged(if succeeded {
                StateChangeOutcome::Succeeded(if required {
                    MfaRequirement::Required
                } else {
                    MfaRequirement::Optional
                })
            } else {
                StateChangeOutcome::Denied
            })
        }
        MfaPolicyAction::EnrollmentReset => {
            AuditOutcomeDetail::AuthenticationMfaReset(if succeeded {
                StateChangeOutcome::Succeeded(MfaResetState::ReenrollmentRequired)
            } else {
                StateChangeOutcome::Denied
            })
        }
    }
}

const fn mfa_policy_classification(action: MfaPolicyAction) -> AuditLogClassification {
    match action {
        MfaPolicyAction::Requirement { .. } => {
            AuditLogClassification::AuthenticationMfaRequirementChanged
        }
        MfaPolicyAction::EnrollmentReset => AuditLogClassification::AuthenticationMfaReset,
    }
}

fn mfa_policy_event_time() -> Result<EventTime, MfaPolicyChangeError> {
    let milliseconds = system_clock()().ok_or(MfaPolicyChangeError::AuditLogUnavailable)?;
    let milliseconds =
        u64::try_from(milliseconds).map_err(|_| MfaPolicyChangeError::AuditLogUnavailable)?;
    Ok(EventTime::from_unix_milliseconds(milliseconds))
}

/// Postcommit Audit terminal delivery state for one Group mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GroupMutationDelivery {
    /// Exact destination delivery and database acknowledgement completed.
    Acknowledged,
    /// The committed obligation remains available for bounded restart recovery.
    Pending,
}

/// Complete internal result of one existing-Group membership or grant mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GroupMutationResult {
    /// The desired association state already existed and no Attempt was produced.
    Unchanged,
    /// Exactly one membership or direct-grant row changed.
    Changed { delivery: GroupMutationDelivery },
    /// Target state drifted after the Attempt; only the generic denied terminal committed.
    Stale { delivery: GroupMutationDelivery },
    /// Final issuer state denied the mutation; only the generic denied terminal committed.
    Denied { delivery: GroupMutationDelivery },
}

/// Payload-free pre-commit refusal or the fixed committed last-administrator denial.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GroupMutationWorkflowError {
    /// The consumed authorization was not a supported Group mutation.
    ActionNotSupported,
    /// The existing Group or account target could not be resolved.
    TargetNotFound,
    /// Required Audit recovery, destination, Attempt, terminal, or commit failed.
    AuditLogUnavailable,
    /// The selected removal would leave no active effective Administrator.
    CannotRemoveLastAdministrator,
}

impl fmt::Display for GroupMutationWorkflowError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ActionNotSupported => "administration action not supported",
            Self::TargetNotFound => "Group mutation target not found",
            Self::AuditLogUnavailable => "consequential operation Audit Log unavailable",
            Self::CannotRemoveLastAdministrator => {
                "Cannot remove the last Server Administration Permission grant."
            }
        })
    }
}

impl StdError for GroupMutationWorkflowError {}

/// Transport-independent workflow for existing-Group membership and grant writes.
pub(crate) struct GroupMutationWorkflow<'a> {
    database: &'a OperationalDatabase,
    audit: &'a OperationalAuditRecovery,
}

impl<'a> GroupMutationWorkflow<'a> {
    pub(crate) const fn new(
        database: &'a OperationalDatabase,
        audit: &'a OperationalAuditRecovery,
    ) -> Self {
        Self { database, audit }
    }

    /// Consumes exact action authority and commits one audited Group outcome.
    pub(crate) fn apply(
        &self,
        action: AuthorizedAdministrationAction,
    ) -> Result<GroupMutationResult, GroupMutationWorkflowError> {
        let authorization = action
            .into_group_mutation()
            .map_err(|_| GroupMutationWorkflowError::ActionNotSupported)?;
        if self.audit.drain_before_consequential_operation().active()
            != AuditRecoverySequenceState::Ready
        {
            return Err(GroupMutationWorkflowError::AuditLogUnavailable);
        }
        self.audit
            .with_current_destination(|destination| {
                destination
                    .destination()
                    .preflight(LogRecordType::Audit)
                    .map_err(|_| GroupMutationWorkflowError::AuditLogUnavailable)?;
                self.apply_with_destination(authorization, destination)
            })
            .map_err(|_| GroupMutationWorkflowError::AuditLogUnavailable)?
    }

    fn apply_with_destination(
        &self,
        authorization: weavelit_server_administration::AuthorizedGroupMutation,
        destination: &OperationalAuditGenerationDestination,
    ) -> Result<GroupMutationResult, GroupMutationWorkflowError> {
        let descriptor = authorization.mutation().clone();
        let target = match descriptor.clone() {
            GroupMutation::Membership(change) => self
                .database
                .prepare_group_membership_target(change.group(), change.account())
                .map_err(|_| GroupMutationWorkflowError::TargetNotFound)?
                .map(GroupMutationTarget::Membership)
                .ok_or(GroupMutationWorkflowError::TargetNotFound)?,
            GroupMutation::Grant(change) => self
                .database
                .prepare_group_grant_target(change.group(), change.grant().clone())
                .map_err(|_| GroupMutationWorkflowError::TargetNotFound)?
                .map(GroupMutationTarget::Grant)
                .ok_or(GroupMutationWorkflowError::TargetNotFound)?,
        };
        let desired = match &descriptor {
            GroupMutation::Membership(change) => change.desired(),
            GroupMutation::Grant(change) => change.desired(),
        };
        let recheck = GroupMutationRecheck::new(
            authorization.actor(),
            authorization.session(),
            authorization.client_module().clone(),
            current_session_instant()?,
        );
        let mutation = match PreparedGroupMutation::new(recheck, target, desired) {
            Ok(mutation) => mutation,
            Err(GroupMutationError::Unchanged) => return Ok(GroupMutationResult::Unchanged),
            Err(GroupMutationError::InvalidTarget) => {
                return Err(GroupMutationWorkflowError::TargetNotFound);
            }
        };
        let actor = self
            .database
            .load_account_audit_reference(authorization.actor())
            .map_err(|_| GroupMutationWorkflowError::AuditLogUnavailable)?
            .ok_or(GroupMutationWorkflowError::AuditLogUnavailable)?;
        let event = group_mutation_event(mutation.target())?;
        let classification = match event {
            AuditEvent::AuthorizationGroupMembershipChanged { .. } => {
                AuditLogClassification::AuthorizationGroupMembershipChanged
            }
            AuditEvent::AuthorizationGroupGrantChanged { .. } => {
                AuditLogClassification::AuthorizationGroupGrantChanged
            }
            _ => unreachable!("Group mutation workflow constructs only Group events"),
        };
        let correlation = CorrelationId::new(
            correlation_identifier().ok_or(GroupMutationWorkflowError::AuditLogUnavailable)?,
        )
        .map_err(|_| GroupMutationWorkflowError::AuditLogUnavailable)?;
        let attempt = self
            .audit
            .producer()
            .prepare_attempt(
                group_event_time().map_err(|_| GroupMutationWorkflowError::AuditLogUnavailable)?,
                correlation,
                AuditActor::Human(actor),
                event,
            )
            .map_err(|_| GroupMutationWorkflowError::AuditLogUnavailable)?;
        let LogRecordPersistenceView::Audit(attempt_record) = attempt.record().persistence_view()
        else {
            return Err(GroupMutationWorkflowError::AuditLogUnavailable);
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
                    classification,
                );
                return Err(GroupMutationWorkflowError::AuditLogUnavailable);
            }
        };
        let terminal = |outcome| {
            self.audit.producer().prepare_completion(
                &delivered_attempt,
                group_event_time()?,
                group_mutation_audit_detail(&descriptor, outcome),
            )
        };
        let succeeded = terminal(AuditGroupMutationOutcome::Succeeded)
            .map_err(|_| GroupMutationWorkflowError::AuditLogUnavailable)?;
        let denied = terminal(AuditGroupMutationOutcome::Denied)
            .map_err(|_| GroupMutationWorkflowError::AuditLogUnavailable)?;
        let last_administrator_denied =
            terminal(AuditGroupMutationOutcome::LastAdministratorDenied)
                .map_err(|_| GroupMutationWorkflowError::AuditLogUnavailable)?;
        let persistence = self.database.audit_terminal_recovery_persistence();
        let succeeded = succeeded
            .recovery_obligation(persistence, destination.binding())
            .map_err(|_| GroupMutationWorkflowError::AuditLogUnavailable)?;
        let denied = denied
            .recovery_obligation(persistence, destination.binding())
            .map_err(|_| GroupMutationWorkflowError::AuditLogUnavailable)?;
        let last_administrator_denied = last_administrator_denied
            .recovery_obligation(persistence, destination.binding())
            .map_err(|_| GroupMutationWorkflowError::AuditLogUnavailable)?;
        let outcome = self
            .database
            .commit_group_mutation(
                &mutation,
                &GroupMutationAuditTerminalWrites::new(
                    &succeeded,
                    &denied,
                    &last_administrator_denied,
                ),
            )
            .map_err(|_| GroupMutationWorkflowError::AuditLogUnavailable)?;
        let delivery = if self.audit.drain_after_consequential_operation().active()
            == AuditRecoverySequenceState::Ready
        {
            GroupMutationDelivery::Acknowledged
        } else {
            GroupMutationDelivery::Pending
        };
        match outcome {
            GroupMutationOutcome::Changed => Ok(GroupMutationResult::Changed { delivery }),
            GroupMutationOutcome::Stale => Ok(GroupMutationResult::Stale { delivery }),
            GroupMutationOutcome::Denied => Ok(GroupMutationResult::Denied { delivery }),
            GroupMutationOutcome::LastAdministratorDenied => {
                Err(GroupMutationWorkflowError::CannotRemoveLastAdministrator)
            }
        }
    }
}

fn group_mutation_event(
    target: &GroupMutationTarget,
) -> Result<AuditEvent, GroupMutationWorkflowError> {
    match target {
        GroupMutationTarget::Membership(target) => {
            Ok(AuditEvent::AuthorizationGroupMembershipChanged {
                group: target.group(),
                account: target.account(),
            })
        }
        GroupMutationTarget::Grant(target) => Ok(AuditEvent::AuthorizationGroupGrantChanged {
            group: target.group(),
            grant: GrantReference::new(canonical_grant_reference(target.grant()))
                .map_err(|_| GroupMutationWorkflowError::ActionNotSupported)?,
        }),
    }
}

fn canonical_grant_reference(grant: &GroupGrant) -> String {
    match grant {
        GroupGrant::ClientModule(name) => format!("client-module.{}", name.as_str()),
        GroupGrant::ServiceModule(name) => format!("service-module.{}", name.as_str()),
        GroupGrant::Operation(name) => format!("operation.{}", name.as_str()),
        GroupGrant::ServerAdministration => "server-administration".to_owned(),
    }
}

fn group_mutation_audit_detail(
    mutation: &GroupMutation,
    outcome: AuditGroupMutationOutcome,
) -> AuditOutcomeDetail {
    match mutation {
        GroupMutation::Membership(_) => {
            AuditOutcomeDetail::AuthorizationGroupMembershipChanged(outcome)
        }
        GroupMutation::Grant(_) => AuditOutcomeDetail::AuthorizationGroupGrantChanged(outcome),
    }
}

fn current_session_instant() -> Result<SessionInstant, GroupMutationWorkflowError> {
    let now = system_clock()().ok_or(GroupMutationWorkflowError::AuditLogUnavailable)?;
    SessionInstant::from_unix_milliseconds(now)
        .map_err(|_| GroupMutationWorkflowError::AuditLogUnavailable)
}

fn group_event_time() -> Result<EventTime, weavelit_server_audit::AuditError> {
    let milliseconds = system_clock()().ok_or(weavelit_server_audit::AuditError::InvalidRecord)?;
    let milliseconds = u64::try_from(milliseconds)
        .map_err(|_| weavelit_server_audit::AuditError::InvalidRecord)?;
    Ok(EventTime::from_unix_milliseconds(milliseconds))
}

impl<'a> AccountAdministrationReadWorkflow<'a> {
    pub(crate) const fn new(database: &'a OperationalDatabase) -> Self {
        Self { database }
    }

    /// Consumes one exact authorization and returns only the bounded projection.
    pub(crate) fn read(
        &self,
        action: AuthorizedAdministrationAction,
    ) -> Result<AccountAdministrationReadResult, AccountAdministrationReadError> {
        let AdministrationAction::Account(AccountAdministrationAction::Read(read)) =
            action.action()
        else {
            return Err(AccountAdministrationReadError::ActionNotSupported);
        };
        let read = *read;

        self.database
            .with_account_administration(|persistence, store| match read {
                AccountAdministrationRead::List => store
                    .list_account_administration_projections(persistence)
                    .map(AccountAdministrationReadResult::List),
                AccountAdministrationRead::View(public_identifier) => store
                    .load_account_administration_projection(persistence, public_identifier)
                    .map(AccountAdministrationReadResult::View),
            })
            .map_err(|_| AccountAdministrationReadError::Unavailable)
    }
}

/// Delivery state of one committed Log configuration terminal obligation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LogConfigurationChangeDelivery {
    /// Exact destination delivery and database acknowledgement completed.
    Acknowledged,
    /// The durable obligation remains available for bounded restart recovery.
    Pending,
}

/// Complete internal result of one authorized Log configuration workflow.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LogConfigurationChangeResult {
    /// The complete desired state already matched; nothing was recorded or written.
    Unchanged,
    /// The state and immutable generations committed.
    Applied {
        generation_count: usize,
        delivery: LogConfigurationChangeDelivery,
    },
    /// The prepared state changed after the Attempt; only the stale terminal committed.
    Stale {
        delivery: LogConfigurationChangeDelivery,
    },
}

/// Payload-free pre-commit refusal of a Log configuration workflow.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LogConfigurationChangeError {
    /// The consumed action was not a Log configuration change.
    ActionNotSupported,
    /// The requested or persisted topology could not safely produce a candidate.
    ChangeRejected,
    /// Audit preparation, recovery, delivery, or required persistence was unavailable.
    AuditLogUnavailable,
}

impl fmt::Display for LogConfigurationChangeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ActionNotSupported => formatter.write_str("administration action not supported"),
            Self::ChangeRejected => formatter.write_str("Log Module configuration change rejected"),
            Self::AuditLogUnavailable => {
                ConsequentialOperationError::AuditLogUnavailable.fmt(formatter)
            }
        }
    }
}

impl StdError for LogConfigurationChangeError {}

/// Synchronous internal workflow for one authorized Log configuration change.
pub(crate) struct LogConfigurationChangeWorkflow<'a> {
    database: &'a OperationalDatabase,
    audit: &'a OperationalAuditRecovery,
}

impl<'a> LogConfigurationChangeWorkflow<'a> {
    pub(crate) const fn new(
        database: &'a OperationalDatabase,
        audit: &'a OperationalAuditRecovery,
    ) -> Self {
        Self { database, audit }
    }

    /// Consumes one exact authorization and applies its configuration change.
    pub(crate) fn apply(
        &self,
        action: AuthorizedAdministrationAction,
    ) -> Result<LogConfigurationChangeResult, LogConfigurationChangeError> {
        let request = exact_log_configuration_change(&action)?;
        if self.audit.drain_before_consequential_operation().active()
            != AuditRecoverySequenceState::Ready
        {
            return Err(LogConfigurationChangeError::AuditLogUnavailable);
        }
        let prepared = match self
            .database
            .prepare_log_configuration_mutation(&request)
            .map_err(log_configuration_unavailable)?
        {
            LogConfigurationPreparation::Unchanged => {
                return Ok(LogConfigurationChangeResult::Unchanged);
            }
            LogConfigurationPreparation::Prepared(prepared) => prepared,
            LogConfigurationPreparation::VersionExhausted
            | LogConfigurationPreparation::Invalid => {
                return Err(LogConfigurationChangeError::ChangeRejected);
            }
        };

        self.audit
            .preflight_log_configuration_mutation(&prepared)
            .map_err(|_| LogConfigurationChangeError::ChangeRejected)?;
        let expected_audit_configuration = prepared
            .expected_assignments()
            .iter()
            .find(|assignment| assignment.log_type == LogType::Audit)
            .ok_or(LogConfigurationChangeError::ChangeRejected)?
            .configuration;
        let expected_audit_generation = prepared
            .expected_generation(expected_audit_configuration)
            .ok_or(LogConfigurationChangeError::ChangeRejected)?
            .key();
        let actor = self
            .database
            .load_account_audit_reference(action.actor())
            .map_err(log_configuration_unavailable)?
            .ok_or(LogConfigurationChangeError::AuditLogUnavailable)?;
        let references = prepared
            .entries()
            .iter()
            .map(|entry| {
                self.database
                    .load_log_configuration_audit_reference(entry.expected().key().configuration())
                    .map_err(log_configuration_unavailable)?
                    .ok_or(LogConfigurationChangeError::AuditLogUnavailable)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let event = AuditEvent::DependencyLogModuleConfigurationChanged {
            configurations: LogConfigurationAuditReferences::new(references)
                .map_err(|_| LogConfigurationChangeError::AuditLogUnavailable)?,
        };

        self.audit
            .with_expected_current_destination(expected_audit_generation, |destination| {
                self.apply_with_destination(action, prepared, actor, event, destination)
            })
            .map_err(log_configuration_unavailable)?
    }

    fn apply_with_destination(
        &self,
        _action: AuthorizedAdministrationAction,
        prepared: weavelit_server_database::PreparedLogConfigurationMutation,
        actor: weavelit_server_database::AccountAuditReference,
        event: AuditEvent,
        destination: &OperationalAuditGenerationDestination,
    ) -> Result<LogConfigurationChangeResult, LogConfigurationChangeError> {
        let correlation = CorrelationId::new(
            correlation_identifier().ok_or(LogConfigurationChangeError::AuditLogUnavailable)?,
        )
        .map_err(|_| LogConfigurationChangeError::AuditLogUnavailable)?;
        let attempt = self
            .audit
            .producer()
            .prepare_attempt(
                event_time().map_err(|_| LogConfigurationChangeError::AuditLogUnavailable)?,
                correlation,
                AuditActor::Human(actor),
                event,
            )
            .map_err(|_| LogConfigurationChangeError::AuditLogUnavailable)?;
        let LogRecordPersistenceView::Audit(attempt_record) = attempt.record().persistence_view()
        else {
            return Err(LogConfigurationChangeError::AuditLogUnavailable);
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
                    AuditLogClassification::DependencyLogModuleConfigurationChanged,
                );
                return Err(LogConfigurationChangeError::AuditLogUnavailable);
            }
        };
        let applied_terminal = self
            .audit
            .producer()
            .prepare_completion(
                &delivered_attempt,
                event_time().map_err(|_| LogConfigurationChangeError::AuditLogUnavailable)?,
                AuditOutcomeDetail::DependencyLogModuleConfigurationChanged(
                    ActionOutcome::Succeeded,
                ),
            )
            .map_err(|_| LogConfigurationChangeError::AuditLogUnavailable)?;
        let stale_terminal = self
            .audit
            .producer()
            .prepare_completion(
                &delivered_attempt,
                event_time().map_err(|_| LogConfigurationChangeError::AuditLogUnavailable)?,
                AuditOutcomeDetail::DependencyLogModuleConfigurationChanged(ActionOutcome::Denied),
            )
            .map_err(|_| LogConfigurationChangeError::AuditLogUnavailable)?;
        let persistence = self.database.audit_terminal_recovery_persistence();
        let applied_write = applied_terminal
            .recovery_obligation(persistence, destination.binding())
            .map_err(|_| LogConfigurationChangeError::AuditLogUnavailable)?;
        let stale_write = stale_terminal
            .recovery_obligation(persistence, destination.binding())
            .map_err(|_| LogConfigurationChangeError::AuditLogUnavailable)?;
        let committed = self
            .database
            .commit_log_configuration_mutation(
                &prepared,
                &LogConfigurationAuditTerminalWrites::new(&applied_write, &stale_write),
            )
            .map_err(log_configuration_unavailable)?;
        let delivery = if self.audit.drain_after_consequential_operation().active()
            == AuditRecoverySequenceState::Ready
        {
            LogConfigurationChangeDelivery::Acknowledged
        } else {
            LogConfigurationChangeDelivery::Pending
        };

        Ok(match committed {
            LogConfigurationMutationOutcome::Applied { generation_count } => {
                LogConfigurationChangeResult::Applied {
                    generation_count,
                    delivery,
                }
            }
            LogConfigurationMutationOutcome::Stale => {
                LogConfigurationChangeResult::Stale { delivery }
            }
        })
    }
}

fn exact_log_configuration_change(
    action: &AuthorizedAdministrationAction,
) -> Result<LogConfigurationMutationRequest, LogConfigurationChangeError> {
    let AdministrationAction::LogConfigurationChange(change) = action.action() else {
        return Err(LogConfigurationChangeError::ActionNotSupported);
    };
    log_configuration_request(change)
}

fn log_configuration_request(
    change: &LogConfigurationChange,
) -> Result<LogConfigurationMutationRequest, LogConfigurationChangeError> {
    LogConfigurationMutationRequest::new(
        change.primary(),
        change.enabled(),
        change.settings().map(<[_]>::to_vec),
        change
            .assignments()
            .iter()
            .map(|assignment| LogAssignment {
                log_type: assignment.log_type(),
                configuration: assignment.configuration(),
            })
            .collect(),
    )
    .map_err(|_| LogConfigurationChangeError::ActionNotSupported)
}

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
                    AuditLogClassification::AuthenticationMfaModuleEnablementChanged,
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

fn log_configuration_unavailable(_: DatabaseError) -> LogConfigurationChangeError {
    LogConfigurationChangeError::AuditLogUnavailable
}

#[cfg(test)]
pub(crate) mod tests {
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
        AccountAdministrationRead, AccountStatusChange, AdministrationClock, AdministrationPlane,
        AdministrationRequest, AuthorizedAdministrationAdmission, ComponentEnablementChange,
        ComponentEnablementSource, LogAssignmentChange, LogConfigurationChange,
    };
    use weavelit_server_administration_authority::ServerAdministrationAuthority;
    use weavelit_server_authorization::{
        AdministrationRequest as AuthorizationRequest, AuthorizationCatalog, AuthorizationDenied,
        ClientModuleDeclaration, Plane, authorize_administration,
    };
    use weavelit_server_components::{AvailableComponents, MfaFactorFormat};
    use weavelit_server_database::{
        AccountPublicIdentifier, AccountPublicIdentifierPersistence, ComponentEnablement,
        ConfigurationKey, ConfigurationValue, GroupGrant, HumanAuthorizationSnapshot,
        LogModuleSetting, SESSION_DIGEST_LENGTH, SessionTokenHash, StateIdentifier,
    };
    use weavelit_server_database_authority::ServerDatabaseAuthority;
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
    pub(crate) struct ObservedAuditRecord {
        pub(crate) classification: String,
        pub(crate) action: String,
        pub(crate) target: String,
        pub(crate) detail: String,
    }

    struct RecordingDestination {
        records: Arc<Mutex<Vec<ObservedAuditRecord>>>,
        attempts: Arc<AtomicUsize>,
        fail_preflight: bool,
        fail_on_attempt: Option<usize>,
        after_delivery: Option<Arc<dyn Fn(usize) + Send + Sync>>,
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
            if let Some(after_delivery) = self.after_delivery.as_ref() {
                after_delivery(attempt);
            }
            Ok(acknowledgement)
        }

        fn preflight(&self, _record_type: LogRecordType) -> Result<(), LogDestinationError> {
            if self.fail_preflight {
                Err(LogDestinationError::Unavailable)
            } else {
                Ok(())
            }
        }
    }

    struct RecordingFactory {
        records: Arc<Mutex<Vec<ObservedAuditRecord>>>,
        attempts: Arc<AtomicUsize>,
        fail_preflight: bool,
        fail_on_attempt: Option<usize>,
        after_delivery: Option<Arc<dyn Fn(usize) + Send + Sync>>,
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
                fail_preflight: self.fail_preflight,
                fail_on_attempt: self.fail_on_attempt,
                after_delivery: self.after_delivery.clone(),
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

    fn account_public_identifier(byte: u8) -> AccountPublicIdentifier {
        AccountPublicIdentifierPersistence::from_server_authority(&ServerDatabaseAuthority::new())
            .decode([byte; 16])
            .unwrap()
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
                "INSERT INTO weavelit_account_public_identity (account_id, public_identifier) \
                 VALUES (?1, ?2)",
                params![
                    identifier(1).as_bytes().as_slice(),
                    [0x91_u8; 16].as_slice()
                ],
            )
            .unwrap();
        for (byte, configuration_name, configuration_reference, enabled) in [
            (0x68, "primary", "ar-68686868686868686868686868686868", true),
            (
                0x69,
                "secondary",
                "ar-69696969696969696969696969696969",
                false,
            ),
        ] {
            connection
                .execute(
                    "INSERT INTO weavelit_log_module_configuration \
                     (configuration_id, module, name, enabled) \
                     VALUES (?1, 'recording', ?2, ?3)",
                    params![
                        identifier(byte).as_bytes().as_slice(),
                        configuration_name,
                        i64::from(enabled),
                    ],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO weavelit_log_configuration_audit_reference \
                     (configuration_id, audit_reference) VALUES (?1, ?2)",
                    params![
                        identifier(byte).as_bytes().as_slice(),
                        configuration_reference
                    ],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO weavelit_log_configuration_generation \
                     (configuration_id, generation_version, module, name, enabled) \
                     VALUES (?1, ?2, 'recording', ?3, ?4)",
                    params![
                        identifier(byte).as_bytes().as_slice(),
                        1_u64.to_be_bytes().as_slice(),
                        configuration_name,
                        i64::from(enabled),
                    ],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO weavelit_log_configuration_current_generation \
                     (configuration_id, generation_version) VALUES (?1, ?2)",
                    params![
                        identifier(byte).as_bytes().as_slice(),
                        1_u64.to_be_bytes().as_slice(),
                    ],
                )
                .unwrap();
        }
        for log_type in ["system", "audit"] {
            connection
                .execute(
                    "INSERT INTO weavelit_log_assignment (log_type, configuration_id) \
                     VALUES (?1, ?2)",
                    params![log_type, identifier(0x68).as_bytes().as_slice()],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO weavelit_log_configuration_generation_log_type \
                     (configuration_id, generation_version, log_type) VALUES (?1, ?2, ?3)",
                    params![
                        identifier(0x68).as_bytes().as_slice(),
                        1_u64.to_be_bytes().as_slice(),
                        log_type,
                    ],
                )
                .unwrap();
        }
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

    fn insert_status_account(path: &Path, active: bool, revision: u64) {
        let connection = Connection::open(path).unwrap();
        connection
            .execute(
                "INSERT INTO weavelit_account \
                 (account_id, username, display_name, active, mfa_required, credential_revision, \
                  must_change_password, temporary_credential_expires_at_milliseconds) \
                 VALUES (?1, 'target', 'Target Account', ?2, 1, ?3, 0, NULL)",
                params![
                    identifier(2).as_bytes().as_slice(),
                    i64::from(active),
                    revision.to_be_bytes().as_slice(),
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO weavelit_password_verifier (account_id, encoded_verifier) \
                 VALUES (?1, '$target-verifier')",
                [identifier(2).as_bytes().as_slice()],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO weavelit_account_audit_reference (account_id, audit_reference) \
                 VALUES (?1, 'ar-22222222222222222222222222222222')",
                [identifier(2).as_bytes().as_slice()],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO weavelit_account_public_identity (account_id, public_identifier) \
                 VALUES (?1, ?2)",
                params![
                    identifier(2).as_bytes().as_slice(),
                    [0x92_u8; 16].as_slice(),
                ],
            )
            .unwrap();
    }

    fn insert_live_status_session(path: &Path, token: u8, account: u8) {
        let now = system_clock()().unwrap();
        let issued_at = now - 1;
        Connection::open(path)
            .unwrap()
            .execute(
                "INSERT INTO weavelit_session \
                 (token_hash, csrf_hash, account_id, client_module, issued_at_milliseconds, \
                  last_seen_at_milliseconds, absolute_expires_at_milliseconds) \
                 VALUES (?1, ?2, ?3, 'web-ui', ?4, ?4, ?5)",
                params![
                    [token; SESSION_DIGEST_LENGTH].as_slice(),
                    [token.wrapping_add(1); SESSION_DIGEST_LENGTH].as_slice(),
                    identifier(account).as_bytes().as_slice(),
                    issued_at,
                    issued_at + weavelit_server_database::SESSION_ABSOLUTE_LIFETIME_MILLISECONDS,
                ],
            )
            .unwrap();
    }

    fn insert_target_session(path: &Path, token: u8) {
        insert_live_status_session(path, token, 2);
    }

    fn stored_account_status(path: &Path, account: u8) -> (bool, u64) {
        let (active, revision): (i64, Vec<u8>) = Connection::open(path)
            .unwrap()
            .query_row(
                "SELECT active, credential_revision FROM weavelit_account WHERE account_id = ?1",
                [identifier(account).as_bytes().as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        (
            active != 0,
            u64::from_be_bytes(revision.try_into().unwrap()),
        )
    }

    pub(crate) fn recovery(
        database: OperationalDatabase,
        fail_on_attempt: Option<usize>,
    ) -> (
        OperationalAuditRecovery,
        Arc<Mutex<Vec<ObservedAuditRecord>>>,
        Arc<AtomicUsize>,
    ) {
        recovery_with_hook(database, fail_on_attempt, None)
    }

    pub(crate) fn recovery_with_hook(
        database: OperationalDatabase,
        fail_on_attempt: Option<usize>,
        after_delivery: Option<Arc<dyn Fn(usize) + Send + Sync>>,
    ) -> (
        OperationalAuditRecovery,
        Arc<Mutex<Vec<ObservedAuditRecord>>>,
        Arc<AtomicUsize>,
    ) {
        recovery_config(database, false, fail_on_attempt, after_delivery)
    }

    pub(crate) fn recovery_with_preflight_failure(
        database: OperationalDatabase,
    ) -> (
        OperationalAuditRecovery,
        Arc<Mutex<Vec<ObservedAuditRecord>>>,
        Arc<AtomicUsize>,
    ) {
        recovery_config(database, true, None, None)
    }

    fn recovery_config(
        database: OperationalDatabase,
        fail_preflight: bool,
        fail_on_attempt: Option<usize>,
        after_delivery: Option<Arc<dyn Fn(usize) + Send + Sync>>,
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
                LogCapabilities::new(vec![LogRecordType::System, LogRecordType::Audit]).unwrap(),
                Box::new(RecordingFactory {
                    records: Arc::clone(&records),
                    attempts: Arc::clone(&attempts),
                    fail_preflight,
                    fail_on_attempt,
                    after_delivery,
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

    pub(crate) fn authorized_change(enabled: bool) -> AuthorizedAdministrationAction {
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

    fn authorized_account_read(read: AccountAdministrationRead) -> AuthorizedAdministrationAction {
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
        AdministrationPlane::new(FixedClock, NoEnablementRead, AvailableComponents::default())
            .authorize(
                admission,
                AdministrationRequest::new(AdministrationAction::Account(
                    AccountAdministrationAction::Read(read),
                )),
            )
            .unwrap()
    }

    fn authorized_account_status(
        target: AccountPublicIdentifier,
        desired: AccountStatus,
    ) -> AuthorizedAdministrationAction {
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
            SessionTokenHash::from_bytes([0x31; SESSION_DIGEST_LENGTH]).unwrap(),
        );
        AdministrationPlane::new(FixedClock, NoEnablementRead, AvailableComponents::default())
            .authorize(
                admission,
                AdministrationRequest::new(AdministrationAction::Account(
                    AccountAdministrationAction::StatusChange(AccountStatusChange::new(
                        target, desired,
                    )),
                )),
            )
            .unwrap()
    }

    fn authorized_log_change(change: LogConfigurationChange) -> AuthorizedAdministrationAction {
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
        AdministrationPlane::new(FixedClock, NoEnablementRead, AvailableComponents::default())
            .authorize(
                admission,
                AdministrationRequest::new(AdministrationAction::LogConfigurationChange(change)),
            )
            .unwrap()
    }

    fn log_setting(key: &str, value: &str) -> LogModuleSetting {
        LogModuleSetting {
            key: ConfigurationKey::new(key).unwrap(),
            value: ConfigurationValue::new(value).unwrap(),
        }
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
    fn authorized_account_reads_return_only_bounded_data_without_side_effects() {
        let surface = surface(false, false, true);
        let connection = Connection::open(&surface.path).unwrap();
        connection
            .execute(
                "INSERT INTO weavelit_account \
                 (account_id, username, display_name, active, mfa_required) \
                 VALUES (?1, 'zeta-operator', 'Zeta Operator', 0, 1)",
                [identifier(2).as_bytes().as_slice()],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO weavelit_account_public_identity (account_id, public_identifier) \
                 VALUES (?1, ?2)",
                params![
                    identifier(2).as_bytes().as_slice(),
                    [0x92_u8; 16].as_slice()
                ],
            )
            .unwrap();
        drop(connection);
        let sessions_before = count(&surface.path, "weavelit_session", "");
        let audit_before = count(&surface.path, "weavelit_audit_terminal_obligation", "");
        let workflow = AccountAdministrationReadWorkflow::new(&surface.database);

        let AccountAdministrationReadResult::List(accounts) = workflow
            .read(authorized_account_read(AccountAdministrationRead::List))
            .unwrap()
        else {
            panic!("the list action must return a list result");
        };
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[0].username().as_str(), "administrator");
        assert_eq!(accounts[0].display_name(), None);
        assert!(accounts[0].active());
        assert!(!accounts[0].mfa_required());
        assert_eq!(
            accounts[0].public_identifier(),
            account_public_identifier(0x91)
        );
        assert_eq!(accounts[1].username().as_str(), "zeta-operator");
        assert_eq!(
            accounts[1].display_name().map(Name::as_str),
            Some("Zeta Operator")
        );
        assert!(!accounts[1].active());
        assert!(accounts[1].mfa_required());

        let AccountAdministrationReadResult::View(exact) = workflow
            .read(authorized_account_read(AccountAdministrationRead::View(
                account_public_identifier(0x92),
            )))
            .unwrap()
        else {
            panic!("the view action must return a view result");
        };
        assert_eq!(exact, Some(accounts[1].clone()));

        let AccountAdministrationReadResult::View(unknown) = workflow
            .read(authorized_account_read(AccountAdministrationRead::View(
                account_public_identifier(0x99),
            )))
            .unwrap()
        else {
            panic!("the view action must return a view result");
        };
        assert_eq!(unknown, None);
        assert_eq!(
            count(&surface.path, "weavelit_session", ""),
            sessions_before
        );
        assert_eq!(
            count(&surface.path, "weavelit_audit_terminal_obligation", ""),
            audit_before
        );
    }

    #[test]
    fn account_read_rejects_another_authorized_action_without_database_access() {
        let surface = surface(false, false, false);
        surface.database.close_for_test().unwrap();
        let workflow = AccountAdministrationReadWorkflow::new(&surface.database);
        let error = workflow.read(authorized_change(true)).unwrap_err();

        assert_eq!(error, AccountAdministrationReadError::ActionNotSupported);
        assert_eq!(error.to_string(), "administration action not supported");
    }

    #[test]
    fn status_noop_missing_and_revision_exhaustion_return_before_audit() {
        let surface = surface(false, false, false);
        insert_status_account(&surface.path, true, 7);
        insert_live_status_session(&surface.path, 0x31, 1);
        let (audit, records, attempts) = recovery(surface.database.clone(), None);
        let workflow = AccountStatusChangeWorkflow::new(&surface.database, &audit);

        assert_eq!(
            workflow.apply(authorized_account_status(
                account_public_identifier(0x92),
                AccountStatus::Active,
            )),
            Ok(AccountStatusChangeResult::Unchanged)
        );
        assert_eq!(
            workflow.apply(authorized_account_status(
                account_public_identifier(0x99),
                AccountStatus::Disabled,
            )),
            Err(AccountStatusChangeError::TargetNotFound)
        );
        Connection::open(&surface.path)
            .unwrap()
            .execute(
                "UPDATE weavelit_account SET credential_revision = ?2 WHERE account_id = ?1",
                params![
                    identifier(2).as_bytes().as_slice(),
                    u64::MAX.to_be_bytes().as_slice(),
                ],
            )
            .unwrap();
        assert_eq!(
            workflow.apply(authorized_account_status(
                account_public_identifier(0x92),
                AccountStatus::Disabled,
            )),
            Err(AccountStatusChangeError::CredentialRevisionExhausted)
        );
        assert!(records.lock().unwrap().is_empty());
        assert_eq!(attempts.load(Ordering::SeqCst), 0);
        assert_eq!(
            count(&surface.path, "weavelit_audit_terminal_obligation", ""),
            0
        );
    }

    #[test]
    fn status_change_requires_ready_active_recovery_before_attempt_or_mutation() {
        let surface = surface(false, false, false);
        insert_status_account(&surface.path, true, 7);
        insert_live_status_session(&surface.path, 0x31, 1);
        let (audit, records, attempts) = recovery(surface.database.clone(), None);
        Connection::open(&surface.path)
            .unwrap()
            .execute_batch(
                "DROP TABLE weavelit_audit_terminal_supersession; \
                 DROP TABLE weavelit_audit_terminal_obligation;",
            )
            .unwrap();
        let workflow = AccountStatusChangeWorkflow::new(&surface.database, &audit);

        assert_eq!(
            workflow.apply(authorized_account_status(
                account_public_identifier(0x92),
                AccountStatus::Disabled,
            )),
            Err(AccountStatusChangeError::AuditLogUnavailable)
        );
        assert_eq!(stored_account_status(&surface.path, 2), (true, 7));
        assert!(records.lock().unwrap().is_empty());
        assert_eq!(attempts.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn disable_and_reenable_emit_exact_typed_audit_and_restore_no_session() {
        let surface = surface(false, false, false);
        insert_status_account(&surface.path, true, 7);
        insert_live_status_session(&surface.path, 0x31, 1);
        insert_target_session(&surface.path, 0x41);
        insert_target_session(&surface.path, 0x42);
        let (audit, records, _) = recovery(surface.database.clone(), None);
        let workflow = AccountStatusChangeWorkflow::new(&surface.database, &audit);

        assert_eq!(
            workflow.apply(authorized_account_status(
                account_public_identifier(0x92),
                AccountStatus::Disabled,
            )),
            Ok(AccountStatusChangeResult::Changed {
                status: AccountStatus::Disabled,
                revoked_sessions: 2,
                delivery: AccountStatusChangeDelivery::Acknowledged,
            })
        );
        assert_eq!(stored_account_status(&surface.path, 2), (false, 8));
        assert_eq!(
            count(
                &surface.path,
                "weavelit_session",
                "WHERE account_id = X'02020202020202020202020202020202'",
            ),
            0
        );

        assert_eq!(
            workflow.apply(authorized_account_status(
                account_public_identifier(0x92),
                AccountStatus::Active,
            )),
            Ok(AccountStatusChangeResult::Changed {
                status: AccountStatus::Active,
                revoked_sessions: 0,
                delivery: AccountStatusChangeDelivery::Acknowledged,
            })
        );
        assert_eq!(stored_account_status(&surface.path, 2), (true, 8));
        assert_eq!(
            count(
                &surface.path,
                "weavelit_session",
                "WHERE account_id = X'02020202020202020202020202020202'",
            ),
            0
        );

        let records = records.lock().unwrap();
        assert_eq!(records.len(), 4);
        for record in &records[0..2] {
            assert_eq!(record.classification, "authentication.user.disabled");
            assert_eq!(record.action, "disable");
            assert_eq!(record.target, "account:ar-22222222222222222222222222222222");
        }
        assert_eq!(
            records[1].detail,
            "accountable action completed successfully; account status: disabled"
        );
        for record in &records[2..4] {
            assert_eq!(record.classification, "internal.user-status.changed");
            assert_eq!(record.action, "change-user-status");
            assert_eq!(record.target, "account:ar-22222222222222222222222222222222");
        }
        assert_eq!(
            records[3].detail,
            "accountable action completed successfully; account status: active"
        );
        let rendered = format!("{records:?}");
        assert!(!rendered.contains("$target-verifier"));
        assert!(!rendered.contains("Target Account"));
    }

    #[test]
    fn self_disable_authorizes_before_revoking_its_exact_session() {
        let surface = surface(false, false, false);
        insert_live_status_session(&surface.path, 0x31, 1);
        let (audit, _, _) = recovery(surface.database.clone(), None);
        let workflow = AccountStatusChangeWorkflow::new(&surface.database, &audit);

        assert_eq!(
            workflow.apply(authorized_account_status(
                account_public_identifier(0x91),
                AccountStatus::Disabled,
            )),
            Ok(AccountStatusChangeResult::Changed {
                status: AccountStatus::Disabled,
                revoked_sessions: 1,
                delivery: AccountStatusChangeDelivery::Acknowledged,
            })
        );
        assert_eq!(stored_account_status(&surface.path, 1), (false, 2));
        assert_eq!(count(&surface.path, "weavelit_session", ""), 0);
    }

    #[test]
    fn post_attempt_target_staleness_and_issuer_revocation_commit_denied_terminals() {
        let stale = surface(false, false, false);
        insert_status_account(&stale.path, true, 7);
        insert_live_status_session(&stale.path, 0x31, 1);
        insert_target_session(&stale.path, 0x41);
        let stale_path = stale.path.clone();
        let after_delivery: Arc<dyn Fn(usize) + Send + Sync> = Arc::new(move |attempt| {
            if attempt == 1 {
                Connection::open(&stale_path)
                    .unwrap()
                    .execute(
                        "UPDATE weavelit_account SET credential_revision = ?2 WHERE account_id = ?1",
                        params![
                            identifier(2).as_bytes().as_slice(),
                            9_u64.to_be_bytes().as_slice(),
                        ],
                    )
                    .unwrap();
            }
        });
        let (audit, records, _) =
            recovery_with_hook(stale.database.clone(), None, Some(after_delivery));
        let workflow = AccountStatusChangeWorkflow::new(&stale.database, &audit);
        assert_eq!(
            workflow.apply(authorized_account_status(
                account_public_identifier(0x92),
                AccountStatus::Disabled,
            )),
            Ok(AccountStatusChangeResult::Stale {
                delivery: AccountStatusChangeDelivery::Acknowledged,
            })
        );
        assert_eq!(stored_account_status(&stale.path, 2), (true, 9));
        assert_eq!(count(&stale.path, "weavelit_session", ""), 2);
        assert_eq!(
            records.lock().unwrap()[1].detail,
            "accountable action denied"
        );

        let denied = surface(false, false, false);
        insert_status_account(&denied.path, true, 7);
        insert_live_status_session(&denied.path, 0x31, 1);
        let denied_path = denied.path.clone();
        let after_delivery: Arc<dyn Fn(usize) + Send + Sync> = Arc::new(move |attempt| {
            if attempt == 1 {
                Connection::open(&denied_path)
                    .unwrap()
                    .execute("DELETE FROM weavelit_session", [])
                    .unwrap();
            }
        });
        let (audit, records, _) =
            recovery_with_hook(denied.database.clone(), None, Some(after_delivery));
        let workflow = AccountStatusChangeWorkflow::new(&denied.database, &audit);
        assert_eq!(
            workflow.apply(authorized_account_status(
                account_public_identifier(0x92),
                AccountStatus::Disabled,
            )),
            Ok(AccountStatusChangeResult::Denied {
                delivery: AccountStatusChangeDelivery::Acknowledged,
            })
        );
        assert_eq!(stored_account_status(&denied.path, 2), (true, 7));
        assert_eq!(
            records.lock().unwrap()[1].detail,
            "accountable action denied"
        );
    }

    #[test]
    fn committed_status_remains_changed_when_delivery_is_pending_and_restart_recovers() {
        let surface = surface(false, false, false);
        insert_status_account(&surface.path, true, 7);
        insert_live_status_session(&surface.path, 0x31, 1);
        let (audit, records, attempts) = recovery(surface.database.clone(), Some(2));
        let workflow = AccountStatusChangeWorkflow::new(&surface.database, &audit);

        assert_eq!(
            workflow.apply(authorized_account_status(
                account_public_identifier(0x92),
                AccountStatus::Disabled,
            )),
            Ok(AccountStatusChangeResult::Changed {
                status: AccountStatus::Disabled,
                revoked_sessions: 0,
                delivery: AccountStatusChangeDelivery::Pending,
            })
        );
        assert_eq!(stored_account_status(&surface.path, 2), (false, 8));
        assert_eq!(records.lock().unwrap().len(), 1);
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert_eq!(
            count(
                &surface.path,
                "weavelit_audit_terminal_obligation",
                "WHERE acknowledged = 0",
            ),
            1
        );

        drop(audit);
        let (restarted, recovered_records, _) = recovery(surface.database.clone(), None);
        assert_eq!(
            restarted.drain_for_activation().active(),
            AuditRecoverySequenceState::Ready
        );
        assert_eq!(recovered_records.lock().unwrap().len(), 1);
        assert_eq!(
            count(
                &surface.path,
                "weavelit_audit_terminal_obligation",
                "WHERE acknowledged = 0",
            ),
            0
        );
    }

    #[test]
    fn exact_log_configuration_no_op_emits_no_audit_or_generation() {
        let surface = surface(false, false, false);
        let action = authorized_log_change(
            LogConfigurationChange::new(
                identifier(0x68),
                Some(true),
                Some(Vec::new()),
                vec![LogAssignmentChange::new(LogType::Audit, identifier(0x68))],
            )
            .unwrap(),
        );
        let (audit, records, attempts) = recovery(surface.database.clone(), None);
        let workflow = LogConfigurationChangeWorkflow::new(&surface.database, &audit);

        assert_eq!(
            workflow.apply(action),
            Ok(LogConfigurationChangeResult::Unchanged)
        );
        assert!(records.lock().unwrap().is_empty());
        assert_eq!(attempts.load(Ordering::SeqCst), 0);
        assert_eq!(
            count(&surface.path, "weavelit_log_configuration_generation", ""),
            2
        );
        assert_eq!(
            count(&surface.path, "weavelit_audit_terminal_obligation", ""),
            0
        );
    }

    #[test]
    fn invalid_settings_are_rejected_before_factory_delivery_or_audit_attempt() {
        let surface = surface(false, false, false);
        let action = authorized_log_change(
            LogConfigurationChange::new(
                identifier(0x68),
                None,
                Some(vec![log_setting("secret-path", "/sensitive")]),
                Vec::new(),
            )
            .unwrap(),
        );
        let (audit, records, attempts) = recovery(surface.database.clone(), None);
        let workflow = LogConfigurationChangeWorkflow::new(&surface.database, &audit);

        assert_eq!(
            workflow.apply(action),
            Err(LogConfigurationChangeError::ChangeRejected)
        );
        assert!(records.lock().unwrap().is_empty());
        assert_eq!(attempts.load(Ordering::SeqCst), 0);
        assert!(
            !format!("{:?}", LogConfigurationChangeError::ChangeRejected).contains("/sensitive")
        );
    }

    #[test]
    fn audit_assignment_move_versions_both_endpoints_and_uses_only_typed_targets() {
        let surface = surface(false, false, false);
        let action = authorized_log_change(
            LogConfigurationChange::new(
                identifier(0x69),
                Some(true),
                None,
                vec![LogAssignmentChange::new(LogType::Audit, identifier(0x69))],
            )
            .unwrap(),
        );
        let (audit, records, _) = recovery(surface.database.clone(), None);
        let workflow = LogConfigurationChangeWorkflow::new(&surface.database, &audit);

        assert_eq!(
            workflow.apply(action),
            Ok(LogConfigurationChangeResult::Applied {
                generation_count: 2,
                delivery: LogConfigurationChangeDelivery::Acknowledged,
            })
        );
        let records = records.lock().unwrap();
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|record| {
            record.classification == "dependency.log-module-configuration.changed"
                && record.action == "change-log-module-configuration"
                && record.target
                    == "log-configuration:ar-68686868686868686868686868686868;log-configuration:ar-69696969696969696969696969696969"
                && !record.target.contains("primary")
                && !record.target.contains("secondary")
                && !record.target.contains("recording")
        }));
        assert_eq!(
            Connection::open(&surface.path)
                .unwrap()
                .query_row(
                    "SELECT hex(generation_version) FROM \
                     weavelit_log_configuration_current_generation \
                     WHERE configuration_id = ?1",
                    [identifier(0x69).as_bytes().as_slice()],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "0000000000000002"
        );
    }

    #[test]
    fn committed_log_configuration_result_remains_success_when_terminal_delivery_is_pending() {
        let surface = surface(false, false, false);
        let action = authorized_log_change(
            LogConfigurationChange::new(
                identifier(0x69),
                Some(true),
                None,
                vec![LogAssignmentChange::new(LogType::Audit, identifier(0x69))],
            )
            .unwrap(),
        );
        let (audit, records, attempts) = recovery(surface.database.clone(), Some(2));
        let workflow = LogConfigurationChangeWorkflow::new(&surface.database, &audit);

        assert_eq!(
            workflow.apply(action),
            Ok(LogConfigurationChangeResult::Applied {
                generation_count: 2,
                delivery: LogConfigurationChangeDelivery::Pending,
            })
        );
        assert_eq!(records.lock().unwrap().len(), 1);
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert_eq!(
            count(
                &surface.path,
                "weavelit_audit_terminal_obligation",
                "WHERE acknowledged = 0"
            ),
            1
        );

        drop(audit);
        let (restarted, recovered_records, _) = recovery(surface.database.clone(), None);
        assert_eq!(
            restarted.drain_for_activation().active(),
            AuditRecoverySequenceState::Ready
        );
        assert_eq!(recovered_records.lock().unwrap().len(), 1);
        assert_eq!(
            count(
                &surface.path,
                "weavelit_audit_terminal_obligation",
                "WHERE acknowledged = 0"
            ),
            0
        );
    }

    #[test]
    fn post_attempt_staleness_commits_only_denied_terminal_and_no_planned_state() {
        let surface = surface(false, false, false);
        let action = authorized_log_change(
            LogConfigurationChange::new(
                identifier(0x69),
                Some(true),
                None,
                vec![LogAssignmentChange::new(LogType::Audit, identifier(0x69))],
            )
            .unwrap(),
        );
        let path = surface.path.clone();
        let after_delivery: Arc<dyn Fn(usize) + Send + Sync> = Arc::new(move |attempt| {
            if attempt == 1 {
                Connection::open(&path)
                    .unwrap()
                    .execute(
                        "UPDATE weavelit_log_module_configuration SET enabled = 1 \
                         WHERE configuration_id = ?1",
                        [identifier(0x69).as_bytes().as_slice()],
                    )
                    .unwrap();
            }
        });
        let (audit, records, _) =
            recovery_with_hook(surface.database.clone(), None, Some(after_delivery));
        let workflow = LogConfigurationChangeWorkflow::new(&surface.database, &audit);

        assert_eq!(
            workflow.apply(action),
            Ok(LogConfigurationChangeResult::Stale {
                delivery: LogConfigurationChangeDelivery::Acknowledged,
            })
        );
        let records = records.lock().unwrap();
        assert_eq!(records.len(), 2);
        assert!(records[1].detail.contains("accountable action denied"));
        assert_eq!(
            count(&surface.path, "weavelit_log_configuration_generation", ""),
            2
        );
        assert_eq!(
            Connection::open(&surface.path)
                .unwrap()
                .query_row(
                    "SELECT configuration_id FROM weavelit_log_assignment \
                     WHERE log_type = 'audit'",
                    [],
                    |row| row.get::<_, Vec<u8>>(0),
                )
                .unwrap(),
            identifier(0x68).as_bytes()
        );
    }

    #[test]
    fn non_administrator_is_denied_before_a_workflow_or_audit_side_effect_exists() {
        let surface = surface(false, false, false);
        insert_status_account(&surface.path, true, 7);
        let (_audit, records, attempts) = recovery(surface.database.clone(), None);
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
        assert_eq!(stored_account_status(&surface.path, 2), (true, 7));
        assert!(records.lock().unwrap().is_empty());
        assert_eq!(attempts.load(Ordering::SeqCst), 0);
        assert_eq!(
            count(&surface.path, "weavelit_audit_terminal_obligation", ""),
            0
        );
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
