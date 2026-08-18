use std::{fmt::Write as _, path::PathBuf};

use weavelit_server_audit::{
    AccountStatus, ActionOutcome, AuditActor, AuditError, AuditEvent, AuditOutcomeDetail,
    AutomationReference, BackupReference, ComponentReference, ComponentState, GrantReference,
    LogConfigurationReference, LogModuleReference, LogPolicyReference, MfaModuleChange,
    MfaModuleReference, MfaRequirement, MfaResetState, OperationReference, ServerAudit,
    ServiceConnectionReference, StateChangeOutcome,
};
use weavelit_server_database::{
    Account, AccountAuditReference, AuditReferenceIdentifier, Group, GroupAuditReference, Name,
    StateIdentifier,
};
use weavelit_server_log::{
    AuditRecordPhase, CompleteLogRecord, ConfiguredLogDestination, CorrelationId,
    DurableAcknowledgement, EventTime, LogCapabilities, LogDeliveryError, LogDestination,
    LogDestinationError, LogDestinationFactory, LogModuleCatalog, LogModuleFactoryContext,
    LogModuleIdentifier, LogModuleRegistration, LogRecordPersistenceView, LogRecordType, LogResult,
    LogSettingsContract, TrustedLogModuleContext, TrustedRecordIssuer,
};
use weavelit_server_log_authority::ServerLogAuthority;

fn reference<T>(constructor: impl FnOnce(String) -> Result<T, AuditError>, value: &str) -> T {
    constructor(value.to_owned()).expect("the fixture reference must be valid")
}

fn state_identifier(value: u8) -> StateIdentifier {
    StateIdentifier::from_bytes([value; 16]).expect("the fixture state identifier must be valid")
}

fn account(value: u8) -> AccountAuditReference {
    AccountAuditReference::new(
        state_identifier(value),
        AuditReferenceIdentifier::generate().expect("the fixture audit reference must generate"),
    )
}

fn group(value: u8) -> GroupAuditReference {
    GroupAuditReference::new(
        state_identifier(value),
        AuditReferenceIdentifier::generate().expect("the fixture audit reference must generate"),
    )
}

fn account_target(account: AccountAuditReference) -> String {
    format!("account:{}", account.audit_reference())
}

fn group_target(group: GroupAuditReference) -> String {
    format!("group:{}", group.audit_reference())
}

fn state_identifier_hex(identifier: StateIdentifier) -> String {
    let mut encoded = String::new();
    for byte in identifier.as_bytes() {
        write!(&mut encoded, "{byte:02x}").unwrap();
    }
    encoded
}

fn producer() -> ServerAudit {
    ServerAudit::new(TrustedRecordIssuer::from_server_authority(
        &ServerLogAuthority::new(),
    ))
}

fn human() -> AuditActor {
    AuditActor::Human(account(0xA1))
}

fn correlation() -> CorrelationId {
    CorrelationId::new("workflow-correlation-01").expect("the correlation must be valid")
}

fn assert_audit(record: &CompleteLogRecord) -> weavelit_server_log::AuditLogPersistenceView<'_> {
    let LogRecordPersistenceView::Audit(view) = record.persistence_view() else {
        panic!("Server Audit must produce only Audit records");
    };
    view
}

fn acknowledging_destination() -> ConfiguredLogDestination {
    let authority = ServerLogAuthority::new();
    let catalog = LogModuleCatalog::new(vec![LogModuleRegistration::new(
        "acknowledging",
        LogCapabilities::new(vec![LogRecordType::Audit]).unwrap(),
        Box::new(AcknowledgingFactory),
    )])
    .unwrap();
    catalog
        .create_destination(
            &LogModuleIdentifier::new("acknowledging").unwrap(),
            &TrustedLogModuleContext::from_server_authority(
                &authority,
                PathBuf::from("/unused"),
                [8; 16],
            ),
        )
        .unwrap()
}

#[test]
fn every_event_has_the_fixed_registered_taxonomy_action_and_safe_target() {
    let target_account = account(0x11);
    let target_account_value = account_target(target_account);
    let last_admin = account(0x12);
    let last_admin_value = account_target(last_admin);
    let operators = group(0x21);
    let operators_value = group_target(operators);
    let administrators = group(0x22);
    let administrators_value = group_target(administrators);
    let events = vec![
        (
            AuditEvent::LifecycleBackupCreated {
                backup: reference(BackupReference::new, "nightly"),
            },
            AuditOutcomeDetail::LifecycleBackupCreated(ActionOutcome::Succeeded),
            "lifecycle.backup.created",
            "create-backup",
            "backup:nightly".to_owned(),
        ),
        (
            AuditEvent::AuthenticationUserCreated {
                account: target_account,
            },
            AuditOutcomeDetail::AuthenticationUserCreated(ActionOutcome::Succeeded),
            "authentication.user.created",
            "create",
            target_account_value.clone(),
        ),
        (
            AuditEvent::AuthenticationUserDisabled {
                account: target_account,
            },
            AuditOutcomeDetail::AuthenticationUserDisabled(StateChangeOutcome::Succeeded(
                AccountStatus::Disabled,
            )),
            "authentication.user.disabled",
            "disable",
            target_account_value.clone(),
        ),
        (
            AuditEvent::AuthenticationPasswordChanged {
                account: target_account,
            },
            AuditOutcomeDetail::AuthenticationPasswordChanged(ActionOutcome::Succeeded),
            "authentication.password.changed",
            "change-password",
            target_account_value.clone(),
        ),
        (
            AuditEvent::AuthenticationPasswordResetStarted {
                account: target_account,
            },
            AuditOutcomeDetail::AuthenticationPasswordResetStarted(ActionOutcome::Succeeded),
            "authentication.password-reset.started",
            "reset-password",
            target_account_value.clone(),
        ),
        (
            AuditEvent::AuthenticationMfaEnrolled {
                account: target_account,
            },
            AuditOutcomeDetail::AuthenticationMfaEnrolled(ActionOutcome::Succeeded),
            "authentication.mfa.enrolled",
            "enroll-mfa",
            target_account_value.clone(),
        ),
        (
            AuditEvent::AuthenticationMfaReset {
                account: target_account,
            },
            AuditOutcomeDetail::AuthenticationMfaReset(StateChangeOutcome::Succeeded(
                MfaResetState::ReenrollmentRequired,
            )),
            "authentication.mfa.reset",
            "reset-mfa",
            target_account_value.clone(),
        ),
        (
            AuditEvent::AuthenticationMfaRequirementChanged {
                account: target_account,
            },
            AuditOutcomeDetail::AuthenticationMfaRequirementChanged(StateChangeOutcome::Succeeded(
                MfaRequirement::Required,
            )),
            "authentication.mfa-requirement.changed",
            "change-mfa-requirement",
            target_account_value.clone(),
        ),
        (
            AuditEvent::AuthenticationMfaModuleEnablementChanged {
                module: reference(MfaModuleReference::new, "totp"),
            },
            AuditOutcomeDetail::AuthenticationMfaModuleEnablementChanged(
                StateChangeOutcome::Succeeded(MfaModuleChange::new(ComponentState::Enabled, 3)),
            ),
            "authentication.mfa-module-enablement.changed",
            "change-mfa-module",
            "mfa-module:totp".to_owned(),
        ),
        (
            AuditEvent::AuthenticationSessionRevoked {
                account: target_account,
            },
            AuditOutcomeDetail::AuthenticationSessionRevoked(ActionOutcome::Succeeded),
            "authentication.session.revoked",
            "revoke-session",
            target_account_value.clone(),
        ),
        (
            AuditEvent::AuthorizationGroupCreated { group: operators },
            AuditOutcomeDetail::AuthorizationGroupCreated(ActionOutcome::Succeeded),
            "authorization.group.created",
            "create-group",
            operators_value.clone(),
        ),
        (
            AuditEvent::AuthorizationGroupMembershipChanged {
                group: operators,
                account: target_account,
            },
            AuditOutcomeDetail::AuthorizationGroupMembershipChanged(ActionOutcome::Succeeded),
            "authorization.group-membership.changed",
            "change-membership",
            format!("{operators_value};{target_account_value}"),
        ),
        (
            AuditEvent::AuthorizationGroupGrantChanged {
                group: operators,
                grant: reference(GrantReference::new, "server-admin"),
            },
            AuditOutcomeDetail::AuthorizationGroupGrantChanged(ActionOutcome::Succeeded),
            "authorization.group-grant.changed",
            "change-grant",
            format!("{operators_value};grant:server-admin"),
        ),
        (
            AuditEvent::AuthorizationGroupGrantRemovalDenied {
                group: administrators,
                account: last_admin,
            },
            AuditOutcomeDetail::AuthorizationGroupGrantRemovalDenied,
            "authorization.group-grant.removal-denied",
            "remove-grant",
            format!("{administrators_value};{last_admin_value}"),
        ),
        (
            AuditEvent::AuthorizationAutomationScopeChanged {
                automation: reference(AutomationReference::new, "nightly-sync"),
                operation: reference(OperationReference::new, "zendesk.ticket.read"),
            },
            AuditOutcomeDetail::AuthorizationAutomationScopeChanged(ActionOutcome::Succeeded),
            "authorization.automation-scope.changed",
            "change-automation-scope",
            "automation:nightly-sync;operation:zendesk.ticket.read".to_owned(),
        ),
        (
            AuditEvent::DependencyLogModuleConfigurationChanged {
                module: reference(LogModuleReference::new, "sqlite"),
                configuration: reference(LogConfigurationReference::new, "primary-audit"),
            },
            AuditOutcomeDetail::DependencyLogModuleConfigurationChanged(ActionOutcome::Succeeded),
            "dependency.log-module-configuration.changed",
            "change-log-module-configuration",
            "log-module:sqlite;log-configuration:primary-audit".to_owned(),
        ),
        (
            AuditEvent::DependencyServiceConnectionChanged {
                connection: reference(ServiceConnectionReference::new, "zendesk-primary"),
            },
            AuditOutcomeDetail::DependencyServiceConnectionChanged(ActionOutcome::Succeeded),
            "dependency.service-connection.changed",
            "change-service-connection",
            "service-connection:zendesk-primary".to_owned(),
        ),
        (
            AuditEvent::ProviderOperation {
                operation: reference(OperationReference::new, "zendesk.ticket.update"),
                connection: reference(ServiceConnectionReference::new, "zendesk-primary"),
            },
            AuditOutcomeDetail::ProviderOperation(ActionOutcome::Succeeded),
            "provider.operation.started",
            "operation-start",
            "operation:zendesk.ticket.update;service-connection:zendesk-primary".to_owned(),
        ),
        (
            AuditEvent::InternalServerConfigurationChanged {
                component: reference(ComponentReference::new, "web-ui"),
            },
            AuditOutcomeDetail::InternalServerConfigurationChanged(StateChangeOutcome::Succeeded(
                ComponentState::Enabled,
            )),
            "internal.server-configuration.changed",
            "change-server-configuration",
            "component:web-ui".to_owned(),
        ),
        (
            AuditEvent::InternalUserStatusChanged {
                account: target_account,
            },
            AuditOutcomeDetail::InternalUserStatusChanged(StateChangeOutcome::Succeeded(
                AccountStatus::Active,
            )),
            "internal.user-status.changed",
            "change-user-status",
            target_account_value,
        ),
        (
            AuditEvent::InternalLogPolicyChanged {
                policy: reference(LogPolicyReference::new, "audit-retention"),
            },
            AuditOutcomeDetail::InternalLogPolicyChanged(ActionOutcome::Succeeded),
            "internal.log-policy.changed",
            "change-log-policy",
            "log-policy:audit-retention".to_owned(),
        ),
    ];

    let destination = acknowledging_destination();
    for (index, (event, detail, classification, action, target)) in events.into_iter().enumerate() {
        let producer = producer();
        let prepared = producer
            .prepare_attempt(
                EventTime::from_unix_milliseconds(index as u64 + 1),
                correlation(),
                human(),
                event,
            )
            .expect("every registered event must prepare");
        let view = assert_audit(prepared.record());
        assert_eq!(view.body().classification(), classification);
        assert_eq!(view.body().action(), action);
        assert_eq!(view.body().target(), target);
        assert_eq!(view.body().detail(), "accountable action accepted");
        assert_eq!(view.phase(), &AuditRecordPhase::Attempt);
        let attempt = prepared.deliver(&destination).unwrap();
        producer
            .prepare_completion(
                &attempt,
                EventTime::from_unix_milliseconds(index as u64 + 101),
                detail,
            )
            .expect("the matching outcome detail must prepare");
    }
}

#[test]
fn attempt_completion_and_correction_have_fresh_ids_one_correlation_and_exact_links() {
    let producer = producer();
    let destination = acknowledging_destination();
    let responsible_owner = account(0xA2);
    let attempt = producer
        .prepare_attempt(
            EventTime::from_unix_milliseconds(100),
            correlation(),
            AuditActor::Automation {
                identity: reference(AutomationReference::new, "nightly-sync"),
                responsible_owner,
            },
            AuditEvent::ProviderOperation {
                operation: reference(OperationReference::new, "zendesk.ticket.update"),
                connection: reference(ServiceConnectionReference::new, "zendesk-primary"),
            },
        )
        .unwrap();
    let attempt_view = assert_audit(attempt.record());
    let attempt_id = *attempt_view.record_id().as_bytes();
    assert_eq!(
        attempt_view.body().classification(),
        "provider.operation.started"
    );
    assert_eq!(attempt_view.body().action(), "operation-start");
    assert_eq!(attempt_view.body().principal(), "automation:nightly-sync");
    assert_eq!(
        attempt_view.body().responsible_owner(),
        Some(account_target(responsible_owner).as_str())
    );
    assert_eq!(attempt_view.phase().result(), None);
    assert_eq!(attempt_view.phase().attempt_record_id(), None);
    let attempt = attempt.deliver(&destination).unwrap();

    let completion = producer
        .prepare_completion(
            &attempt,
            EventTime::from_unix_milliseconds(101),
            AuditOutcomeDetail::ProviderOperation(ActionOutcome::Succeeded),
        )
        .unwrap();
    let correction = producer
        .prepare_correction(
            &attempt,
            EventTime::from_unix_milliseconds(102),
            AuditOutcomeDetail::ProviderOperation(ActionOutcome::Failed),
        )
        .unwrap();
    let completion_view = assert_audit(completion.record());
    let correction_view = assert_audit(correction.record());

    assert_ne!(completion_view.record_id().as_bytes(), &attempt_id);
    assert_ne!(correction_view.record_id().as_bytes(), &attempt_id);
    assert_ne!(completion_view.record_id(), correction_view.record_id());
    assert_eq!(
        completion_view.correlation_id().as_str(),
        "workflow-correlation-01"
    );
    assert_eq!(
        correction_view.correlation_id().as_str(),
        "workflow-correlation-01"
    );
    assert_eq!(completion_view.phase().result(), Some(LogResult::Success));
    assert_eq!(correction_view.phase().result(), Some(LogResult::Failure));
    assert_eq!(
        completion_view
            .phase()
            .attempt_record_id()
            .unwrap()
            .as_bytes(),
        &attempt_id
    );
    assert_eq!(
        correction_view
            .phase()
            .attempt_record_id()
            .unwrap()
            .as_bytes(),
        &attempt_id
    );
    assert_eq!(completion_view.phase().as_str(), "completion");
    assert_eq!(correction_view.phase().as_str(), "correction");
    assert_eq!(
        completion_view.body().classification(),
        "provider.operation.completed"
    );
    assert_eq!(completion_view.body().action(), "operation-complete");
    assert_eq!(
        correction_view.body().classification(),
        "provider.operation.completed"
    );
    assert_eq!(correction_view.body().action(), "operation-complete");
}

#[test]
fn typed_success_facts_and_corrected_facts_render_with_derived_results() {
    let cases = vec![
        (
            AuditEvent::AuthenticationUserDisabled {
                account: account(0x61),
            },
            AuditOutcomeDetail::AuthenticationUserDisabled(StateChangeOutcome::Succeeded(
                AccountStatus::Disabled,
            )),
            "account status: disabled",
        ),
        (
            AuditEvent::AuthenticationMfaReset {
                account: account(0x62),
            },
            AuditOutcomeDetail::AuthenticationMfaReset(StateChangeOutcome::Succeeded(
                MfaResetState::ReenrollmentRequired,
            )),
            "MFA reset state: re-enrollment required",
        ),
        (
            AuditEvent::AuthenticationMfaRequirementChanged {
                account: account(0x63),
            },
            AuditOutcomeDetail::AuthenticationMfaRequirementChanged(StateChangeOutcome::Succeeded(
                MfaRequirement::Optional,
            )),
            "MFA requirement: optional",
        ),
        (
            AuditEvent::AuthenticationMfaModuleEnablementChanged {
                module: reference(MfaModuleReference::new, "totp"),
            },
            AuditOutcomeDetail::AuthenticationMfaModuleEnablementChanged(
                StateChangeOutcome::Succeeded(MfaModuleChange::new(ComponentState::Disabled, 42)),
            ),
            "MFA module state: disabled; affected count: 42",
        ),
        (
            AuditEvent::InternalServerConfigurationChanged {
                component: reference(ComponentReference::new, "web-ui"),
            },
            AuditOutcomeDetail::InternalServerConfigurationChanged(StateChangeOutcome::Succeeded(
                ComponentState::Enabled,
            )),
            "component state: enabled",
        ),
        (
            AuditEvent::InternalUserStatusChanged {
                account: account(0x64),
            },
            AuditOutcomeDetail::InternalUserStatusChanged(StateChangeOutcome::Succeeded(
                AccountStatus::Active,
            )),
            "account status: active",
        ),
    ];
    let destination = acknowledging_destination();

    for (index, (event, detail, fact)) in cases.into_iter().enumerate() {
        let producer = producer();
        let attempt = producer
            .prepare_attempt(
                EventTime::from_unix_milliseconds(index as u64 + 1),
                correlation(),
                human(),
                event,
            )
            .unwrap()
            .deliver(&destination)
            .unwrap();
        let completion = producer
            .prepare_completion(
                &attempt,
                EventTime::from_unix_milliseconds(index as u64 + 101),
                detail,
            )
            .unwrap();
        let correction = producer
            .prepare_correction(
                &attempt,
                EventTime::from_unix_milliseconds(index as u64 + 201),
                detail,
            )
            .unwrap();
        let completion_view = assert_audit(completion.record());
        let correction_view = assert_audit(correction.record());

        assert_eq!(completion_view.phase().result(), Some(LogResult::Success));
        assert_eq!(
            completion_view.body().detail(),
            format!("accountable action completed successfully; {fact}")
        );
        assert_eq!(correction_view.phase().result(), Some(LogResult::Success));
        assert_eq!(
            correction_view.body().detail(),
            format!("corrected outcome: accountable action succeeded; {fact}")
        );
    }
}

#[test]
fn denied_and_failed_state_changes_carry_no_committed_fact() {
    let destination = acknowledging_destination();
    for (index, detail) in [
        AuditOutcomeDetail::InternalUserStatusChanged(StateChangeOutcome::Denied),
        AuditOutcomeDetail::InternalUserStatusChanged(StateChangeOutcome::Failed),
    ]
    .into_iter()
    .enumerate()
    {
        let producer = producer();
        let attempt = producer
            .prepare_attempt(
                EventTime::from_unix_milliseconds(index as u64 + 1),
                correlation(),
                human(),
                AuditEvent::InternalUserStatusChanged {
                    account: account(0x65),
                },
            )
            .unwrap()
            .deliver(&destination)
            .unwrap();
        let terminal = producer
            .prepare_completion(
                &attempt,
                EventTime::from_unix_milliseconds(index as u64 + 101),
                detail,
            )
            .unwrap();
        let view = assert_audit(terminal.record());

        assert_eq!(view.phase().result(), Some(LogResult::Failure));
        assert!(!view.body().detail().contains("account status"));
    }
}

#[test]
fn account_and_group_targets_use_only_persisted_audit_projections() {
    let source_account = Account {
        identifier: state_identifier(0x71),
        username: Name::new("administrateur-élève-東京").unwrap(),
        display_name: Some(Name::new("Équipe Opérations 東京").unwrap()),
        active: true,
        mfa_required: true,
    };
    let source_group = Group {
        identifier: state_identifier(0x72),
        name: Name::new("équipe-administration-東京").unwrap(),
        description: None,
    };
    let account_projection = AccountAuditReference::new(
        source_account.identifier,
        AuditReferenceIdentifier::generate().unwrap(),
    );
    let group_projection = GroupAuditReference::new(
        source_group.identifier,
        AuditReferenceIdentifier::generate().unwrap(),
    );
    let prepared = producer()
        .prepare_attempt(
            EventTime::from_unix_milliseconds(1),
            correlation(),
            AuditActor::Human(account_projection),
            AuditEvent::AuthorizationGroupMembershipChanged {
                group: group_projection,
                account: account_projection,
            },
        )
        .unwrap();
    let view = assert_audit(prepared.record());

    assert_eq!(view.body().principal(), account_target(account_projection));
    assert_eq!(view.body().responsible_owner(), None);
    assert_eq!(
        view.body().target(),
        format!(
            "{};{}",
            group_target(group_projection),
            account_target(account_projection)
        )
    );
    for forbidden in [
        "administrateur-élève-東京".to_owned(),
        "Équipe Opérations 東京".to_owned(),
        "équipe-administration-東京".to_owned(),
        state_identifier_hex(source_account.identifier),
        state_identifier_hex(source_group.identifier),
    ] {
        assert!(!view.body().principal().contains(&forbidden));
        assert!(!view.body().target().contains(&forbidden));
    }
}

#[test]
fn terminal_time_and_denial_only_event_invariants_are_refused_payload_free() {
    let producer = producer();
    let destination = acknowledging_destination();
    let administrators = group(0x31);
    let last_admin = account(0x32);
    let attempt = producer
        .prepare_attempt(
            EventTime::from_unix_milliseconds(10),
            correlation(),
            human(),
            AuditEvent::AuthorizationGroupGrantRemovalDenied {
                group: administrators,
                account: last_admin,
            },
        )
        .unwrap()
        .deliver(&destination)
        .unwrap();

    assert_eq!(
        producer
            .prepare_completion(
                &attempt,
                EventTime::from_unix_milliseconds(11),
                AuditOutcomeDetail::AuthorizationGroupCreated(ActionOutcome::Succeeded),
            )
            .unwrap_err(),
        AuditError::InvalidOutcome
    );
    assert_eq!(
        producer
            .prepare_completion(
                &attempt,
                EventTime::from_unix_milliseconds(9),
                AuditOutcomeDetail::AuthorizationGroupGrantRemovalDenied,
            )
            .unwrap_err(),
        AuditError::InvalidRecord
    );
    assert_eq!(format!("{:?}", attempt), "AuditAttemptReference(REDACTED)");
    assert_eq!(
        AuditError::InvalidRecord.to_string(),
        "Audit record is invalid"
    );
    assert_eq!(
        format!("{:?}", AuditError::InvalidOutcome),
        "InvalidOutcome"
    );

    let impossible = producer
        .prepare_attempt(
            EventTime::from_unix_milliseconds(20),
            correlation(),
            human(),
            AuditEvent::AuthenticationUserDisabled {
                account: account(0x33),
            },
        )
        .unwrap()
        .deliver(&destination)
        .unwrap();
    assert_eq!(
        producer
            .prepare_completion(
                &impossible,
                EventTime::from_unix_milliseconds(21),
                AuditOutcomeDetail::AuthenticationUserDisabled(StateChangeOutcome::Succeeded(
                    AccountStatus::Active
                ),),
            )
            .unwrap_err(),
        AuditError::InvalidOutcome
    );
}

#[test]
fn debug_surfaces_redact_all_accepted_reference_values() {
    fn redacted_debug(value: &impl std::fmt::Debug, expected: &str) -> String {
        let debug = format!("{value:?}");
        assert_eq!(debug, expected);
        debug
    }

    let human_account = account(0x41);
    let responsible_owner = account(0x42);
    let event_account = account(0x43);
    let automation_value = "automation-debug-reference";
    let automation = reference(AutomationReference::new, automation_value);
    let human_actor = AuditActor::Human(human_account);
    let automation_actor = AuditActor::Automation {
        identity: automation.clone(),
        responsible_owner,
    };
    let event = AuditEvent::AuthenticationPasswordResetStarted {
        account: event_account,
    };
    let prepared = producer()
        .prepare_attempt(
            EventTime::from_unix_milliseconds(1),
            correlation(),
            human_actor.clone(),
            event.clone(),
        )
        .unwrap();

    let mut debug_outputs = vec![
        redacted_debug(
            &human_account,
            "AccountAuditReference { account: StateIdentifier(REDACTED), audit_reference: AuditReferenceIdentifier(REDACTED) }",
        ),
        redacted_debug(&automation, "AutomationReference(REDACTED)"),
        redacted_debug(&human_actor, "AuditActor(REDACTED)"),
        redacted_debug(&automation_actor, "AuditActor(REDACTED)"),
        redacted_debug(&event, "AuditEvent(REDACTED)"),
        redacted_debug(&prepared, "PreparedAuditAttempt(REDACTED)"),
        redacted_debug(prepared.record(), "CompleteLogRecord(REDACTED)"),
    ];

    let destination = acknowledging_destination();
    let attempt = prepared.deliver(&destination).unwrap();
    let terminal = producer()
        .prepare_completion(
            &attempt,
            EventTime::from_unix_milliseconds(2),
            AuditOutcomeDetail::AuthenticationPasswordResetStarted(ActionOutcome::Succeeded),
        )
        .unwrap();
    debug_outputs.push(redacted_debug(&terminal, "PreparedAuditTerminal(REDACTED)"));

    let human_reference = human_account.audit_reference().to_string();
    let owner_reference = responsible_owner.audit_reference().to_string();
    let event_reference = event_account.audit_reference().to_string();
    let human_state = state_identifier_hex(human_account.account());
    let owner_state = state_identifier_hex(responsible_owner.account());
    let event_state = state_identifier_hex(event_account.account());
    for accepted_value in [
        automation_value,
        &human_reference,
        &owner_reference,
        &event_reference,
        &human_state,
        &owner_state,
        &event_state,
    ] {
        assert!(!accepted_value.is_empty());
        for debug in &debug_outputs {
            assert!(!debug.contains(accepted_value));
        }
    }
}

struct UnavailableFactory;

struct AcknowledgingFactory;

impl LogDestinationFactory for AcknowledgingFactory {
    fn accepted_settings(&self) -> LogSettingsContract {
        LogSettingsContract::none()
    }

    fn create(
        &self,
        _context: &LogModuleFactoryContext<'_>,
    ) -> Result<Box<dyn LogDestination>, LogDestinationError> {
        Ok(Box::new(AcknowledgingDestination))
    }
}

struct AcknowledgingDestination;

impl LogDestination for AcknowledgingDestination {
    fn deliver(
        &self,
        _record: &CompleteLogRecord,
        acknowledgement: DurableAcknowledgement,
    ) -> Result<DurableAcknowledgement, LogDestinationError> {
        Ok(acknowledgement)
    }

    fn preflight(&self, _record_type: LogRecordType) -> Result<(), LogDestinationError> {
        Ok(())
    }
}

impl LogDestinationFactory for UnavailableFactory {
    fn accepted_settings(&self) -> LogSettingsContract {
        LogSettingsContract::none()
    }

    fn create(
        &self,
        _context: &LogModuleFactoryContext<'_>,
    ) -> Result<Box<dyn LogDestination>, LogDestinationError> {
        Ok(Box::new(UnavailableDestination))
    }
}

struct UnavailableDestination;

impl LogDestination for UnavailableDestination {
    fn deliver(
        &self,
        _record: &CompleteLogRecord,
        _acknowledgement: DurableAcknowledgement,
    ) -> Result<DurableAcknowledgement, LogDestinationError> {
        Err(LogDestinationError::Unavailable)
    }

    fn preflight(&self, _record_type: LogRecordType) -> Result<(), LogDestinationError> {
        Err(LogDestinationError::Unavailable)
    }
}

#[test]
fn delivery_returns_the_existing_destination_error_without_mapping() {
    let authority = ServerLogAuthority::new();
    let catalog = LogModuleCatalog::new(vec![LogModuleRegistration::new(
        "unavailable",
        LogCapabilities::new(vec![LogRecordType::Audit]).unwrap(),
        Box::new(UnavailableFactory),
    )])
    .unwrap();
    let destination = catalog
        .create_destination(
            &LogModuleIdentifier::new("unavailable").unwrap(),
            &TrustedLogModuleContext::from_server_authority(
                &authority,
                PathBuf::from("/unused"),
                [7; 16],
            ),
        )
        .unwrap();
    let prepared = producer()
        .prepare_attempt(
            EventTime::from_unix_milliseconds(1),
            correlation(),
            human(),
            AuditEvent::AuthenticationUserDisabled {
                account: account(0x51),
            },
        )
        .unwrap();

    assert_eq!(
        prepared.deliver(&destination).unwrap_err(),
        LogDeliveryError::Destination(LogDestinationError::Unavailable)
    );
}

#[test]
fn system_only_destination_refuses_audit_without_calling_the_module() {
    let authority = ServerLogAuthority::new();
    let catalog = LogModuleCatalog::new(vec![LogModuleRegistration::new(
        "system-only",
        LogCapabilities::new(vec![LogRecordType::System]).unwrap(),
        Box::new(AcknowledgingFactory),
    )])
    .unwrap();
    let destination = catalog
        .create_destination(
            &LogModuleIdentifier::new("system-only").unwrap(),
            &TrustedLogModuleContext::from_server_authority(
                &authority,
                PathBuf::from("/unused"),
                [9; 16],
            ),
        )
        .unwrap();
    let prepared = producer()
        .prepare_attempt(
            EventTime::from_unix_milliseconds(1),
            correlation(),
            human(),
            AuditEvent::AuthenticationUserCreated {
                account: account(0x81),
            },
        )
        .unwrap();

    assert_eq!(
        prepared.deliver(&destination).unwrap_err(),
        LogDeliveryError::CapabilityUnavailable
    );
}
