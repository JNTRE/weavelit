use std::{collections::VecDeque, fmt::Write as _, path::PathBuf};

use weavelit_server_audit::{
    AccountStatus, ActionOutcome, AuditActor, AuditError, AuditEvent, AuditOutcomeDetail,
    AuditTerminalObligationReference, AutomationReference, BackupReference, ComponentReference,
    ComponentState, GrantReference, LogConfigurationAuditReferences, LogPolicyReference,
    MfaModuleChange, MfaModuleReference, MfaRequirement, MfaResetState, OperationReference,
    ServerAudit, ServiceConnectionReference, StateChangeOutcome,
};
use weavelit_server_database::{
    Account, AccountAuditReference, AuditReferenceIdentifier, AuditTerminalAcknowledgementProof,
    AuditTerminalObligation, AuditTerminalRecoveryPersistence, AuditTerminalRecoveryStore,
    AuditTerminalRecoveryTransaction, AuditTerminalReplayBatchSize, AuditTerminalSupersession,
    DatabaseError, Group, GroupAuditReference, LogConfigurationAuditReference, Name,
    StateIdentifier, StoredAuditDestinationBinding, ValidatedAuditTerminalObligationWrite,
};
use weavelit_server_database_authority::ServerDatabaseAuthority;
use weavelit_server_log::{
    AuditDestinationBinding, AuditDestinationBindingTransition, AuditRecordPhase,
    AuditTerminalCompleteness, AuditTerminalReplayError, CompleteLogRecord,
    ConfiguredLogDestination, CorrelationId, DurableAcknowledgement, EventTime, LogCapabilities,
    LogDeliveryError, LogDestination, LogDestinationError, LogDestinationFactory, LogModuleCatalog,
    LogModuleFactoryContext, LogModuleIdentifier, LogModuleRegistration, LogRecordPersistenceView,
    LogRecordType, LogResult, LogSettingsContract, ResolvedAuditDestination,
    TrustedLogModuleContext, TrustedRecordIssuer,
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

fn log_configuration(value: u8) -> LogConfigurationAuditReference {
    LogConfigurationAuditReference::new(
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

fn recovery_persistence() -> AuditTerminalRecoveryPersistence {
    AuditTerminalRecoveryPersistence::from_server_authority(&ServerDatabaseAuthority::new())
}

fn stored_binding(
    persistence: &AuditTerminalRecoveryPersistence,
    binding: &AuditDestinationBinding,
) -> StoredAuditDestinationBinding {
    StoredAuditDestinationBinding::from_persisted(
        persistence,
        *binding.identifier(),
        binding.version(),
    )
    .unwrap()
}

fn persisted_obligation(
    persistence: &AuditTerminalRecoveryPersistence,
    write: &ValidatedAuditTerminalObligationWrite,
) -> AuditTerminalObligation {
    AuditTerminalObligation::from_persisted(
        persistence,
        *write.identifier().as_bytes(),
        write.projection_bytes().to_vec(),
        write.binding().clone(),
    )
    .unwrap()
}

fn terminal_obligation(value: u8) -> AuditTerminalObligationReference {
    AuditTerminalObligationReference::from_identifier(
        weavelit_server_database::AuditTerminalObligationIdentifier::from_persisted(
            &recovery_persistence(),
            [value; 16],
        )
        .unwrap(),
    )
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
    let log_configuration = log_configuration(0x41);
    let log_configuration_value =
        format!("log-configuration:{}", log_configuration.audit_reference());
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
            AuditEvent::DependencyAuditTerminalSuperseded {
                obligation: terminal_obligation(0x31),
            },
            AuditOutcomeDetail::DependencyAuditTerminalSuperseded(StateChangeOutcome::Succeeded(
                AuditTerminalCompleteness::Degraded,
            )),
            "dependency.audit-terminal.superseded",
            "supersede-terminal-delivery",
            "audit-terminal:31313131313131313131313131313131".to_owned(),
        ),
        (
            AuditEvent::DependencyLogModuleConfigurationChanged {
                configurations: LogConfigurationAuditReferences::new(vec![log_configuration])
                    .unwrap(),
            },
            AuditOutcomeDetail::DependencyLogModuleConfigurationChanged(ActionOutcome::Succeeded),
            "dependency.log-module-configuration.changed",
            "change-log-module-configuration",
            log_configuration_value,
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
fn supersession_terminal_records_only_the_degraded_integrity_exception() {
    let producer = producer();
    let destination = acknowledging_destination();
    let attempt = producer
        .prepare_attempt(
            EventTime::from_unix_milliseconds(1),
            correlation(),
            human(),
            AuditEvent::DependencyAuditTerminalSuperseded {
                obligation: terminal_obligation(0x41),
            },
        )
        .unwrap()
        .deliver(&destination)
        .unwrap();
    let terminal = producer
        .prepare_completion(
            &attempt,
            EventTime::from_unix_milliseconds(2),
            AuditOutcomeDetail::DependencyAuditTerminalSuperseded(StateChangeOutcome::Succeeded(
                AuditTerminalCompleteness::Degraded,
            )),
        )
        .unwrap();
    let view = assert_audit(terminal.record());

    assert_eq!(
        view.body().classification(),
        "dependency.audit-terminal.superseded"
    );
    assert_eq!(view.body().action(), "supersede-terminal-delivery");
    assert_eq!(
        view.body().target(),
        "audit-terminal:41414141414141414141414141414141"
    );
    assert_eq!(
        view.body().detail(),
        "accountable action completed successfully; Audit completeness: degraded"
    );
    assert_eq!(view.phase().as_str(), "completion");
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
        credential_revision: weavelit_server_database::CredentialRevision::INITIAL,
        must_change_password: false,
        temporary_credential_expiration: None,
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

#[test]
fn terminal_recovery_exports_exactly_replays_and_only_then_acknowledges() {
    let authority = ServerLogAuthority::new();
    let retained_binding =
        AuditDestinationBinding::from_server_authority(&authority, [0x81; 16], 4).unwrap();
    let changed_binding =
        AuditDestinationBinding::from_server_authority(&authority, [0x81; 16], 5).unwrap();
    let persistence = recovery_persistence();
    let producer = producer();
    let attempt = producer
        .prepare_attempt(
            EventTime::from_unix_milliseconds(100),
            correlation(),
            human(),
            AuditEvent::AuthenticationUserDisabled {
                account: account(0x82),
            },
        )
        .unwrap()
        .deliver(&acknowledging_destination())
        .unwrap();
    let terminal = producer
        .prepare_completion(
            &attempt,
            EventTime::from_unix_milliseconds(101),
            AuditOutcomeDetail::AuthenticationUserDisabled(StateChangeOutcome::Succeeded(
                AccountStatus::Disabled,
            )),
        )
        .unwrap();
    let obligation = terminal
        .recovery_obligation(&persistence, &retained_binding)
        .unwrap();
    assert_eq!(
        obligation.identifier().as_bytes(),
        terminal.record().record_id().as_bytes()
    );

    let mut store = MemoryAuditTerminalStore::default();
    store
        .persist_audit_terminal_obligation(&obligation)
        .unwrap();
    let pending = store
        .list_pending_audit_terminal_obligations(
            &persistence,
            AuditTerminalReplayBatchSize::new(1).unwrap(),
        )
        .unwrap();
    let recovered = producer
        .restore_terminal_recovery(&persistence, &pending[0])
        .unwrap();
    assert_eq!(recovered.binding(), &retained_binding);
    let changed_destination = acknowledging_destination();
    let changed_destination = ResolvedAuditDestination::from_server_authority(
        &authority,
        &changed_binding,
        &changed_destination,
    );
    assert_eq!(
        recovered
            .deliver(&persistence, &changed_destination)
            .unwrap_err(),
        AuditTerminalReplayError::DestinationBindingChanged
    );
    assert_eq!(store.pending.len(), 1);

    let destination = acknowledging_destination();
    let destination = ResolvedAuditDestination::from_server_authority(
        &authority,
        &retained_binding,
        &destination,
    );
    let acknowledgement = recovered.deliver(&persistence, &destination).unwrap();
    acknowledgement.acknowledge(&mut store).unwrap();
    assert!(store.pending.is_empty());
    assert_eq!(
        AuditTerminalReplayError::DestinationBindingChanged.to_string(),
        "committed Audit terminal recovery remains pending"
    );
}

#[test]
fn terminal_recovery_rejects_mismatched_identity_without_payload_disclosure() {
    let authority = ServerLogAuthority::new();
    let binding =
        AuditDestinationBinding::from_server_authority(&authority, [0x91; 16], 1).unwrap();
    let persistence = recovery_persistence();
    let producer = producer();
    let attempt = producer
        .prepare_attempt(
            EventTime::from_unix_milliseconds(200),
            correlation(),
            human(),
            AuditEvent::AuthenticationUserDisabled {
                account: account(0x92),
            },
        )
        .unwrap()
        .deliver(&acknowledging_destination())
        .unwrap();
    let terminal = producer
        .prepare_completion(
            &attempt,
            EventTime::from_unix_milliseconds(201),
            AuditOutcomeDetail::AuthenticationUserDisabled(StateChangeOutcome::Succeeded(
                AccountStatus::Disabled,
            )),
        )
        .unwrap();
    let obligation = terminal
        .recovery_obligation(&persistence, &binding)
        .unwrap();
    let mismatched = AuditTerminalObligation::from_persisted(
        &persistence,
        [0x93; 16],
        obligation.projection_bytes().to_vec(),
        obligation.binding().clone(),
    )
    .unwrap();

    let error = producer
        .restore_terminal_recovery(&persistence, &mismatched)
        .unwrap_err();
    assert_eq!(
        error,
        weavelit_server_audit::AuditTerminalObligationError::InvalidObligation
    );
    assert_eq!(
        error.to_string(),
        "Audit terminal recovery obligation is invalid"
    );
    assert!(!format!("{obligation:?}").contains("accountable action"));

    let mismatched_binding = AuditTerminalObligation::from_persisted(
        &persistence,
        *obligation.identifier().as_bytes(),
        obligation.projection_bytes().to_vec(),
        StoredAuditDestinationBinding::from_persisted(&persistence, [0x94; 16], 1).unwrap(),
    )
    .unwrap();
    assert_eq!(
        producer
            .restore_terminal_recovery(&persistence, &mismatched_binding)
            .unwrap_err(),
        weavelit_server_audit::AuditTerminalObligationError::InvalidObligation
    );

    let malformed = AuditTerminalObligation::from_persisted(
        &persistence,
        [0x95; 16],
        b"bounded-but-not-a-terminal-projection".to_vec(),
        obligation.binding().clone(),
    )
    .unwrap();
    assert_eq!(
        producer
            .restore_terminal_recovery(&persistence, &malformed)
            .unwrap_err(),
        weavelit_server_audit::AuditTerminalObligationError::InvalidObligation
    );
}

#[test]
fn terminal_supersession_preserves_late_exact_delivery_and_advances_active_recovery() {
    let authority = ServerLogAuthority::new();
    let original_binding =
        AuditDestinationBinding::from_server_authority(&authority, [0xA1; 16], 3).unwrap();
    let replacement_binding =
        AuditDestinationBinding::from_server_authority(&authority, [0xA2; 16], 1).unwrap();
    let transition = AuditDestinationBindingTransition::from_server_authority(
        &authority,
        &original_binding,
        &replacement_binding,
    )
    .unwrap();
    let persistence = recovery_persistence();
    let producer = producer();
    let original_attempt = producer
        .prepare_attempt(
            EventTime::from_unix_milliseconds(300),
            correlation(),
            human(),
            AuditEvent::AuthenticationUserDisabled {
                account: account(0xA3),
            },
        )
        .unwrap()
        .deliver(&acknowledging_destination())
        .unwrap();
    let original_terminal = producer
        .prepare_completion(
            &original_attempt,
            EventTime::from_unix_milliseconds(301),
            AuditOutcomeDetail::AuthenticationUserDisabled(StateChangeOutcome::Succeeded(
                AccountStatus::Disabled,
            )),
        )
        .unwrap();
    let original_obligation = original_terminal
        .recovery_obligation(&persistence, &original_binding)
        .unwrap();
    let mut store = MemoryAuditTerminalStore::default();
    let malformed_oldest = AuditTerminalObligation::from_persisted(
        &persistence,
        [0xAF; 16],
        b"malformed-oldest-terminal".to_vec(),
        stored_binding(&persistence, &original_binding),
    )
    .unwrap();
    store.pending.push_back(malformed_oldest.clone());
    store
        .persist_audit_terminal_obligation(&original_obligation)
        .unwrap();
    let persisted_original = persisted_obligation(&persistence, &original_obligation);
    let original = producer
        .restore_terminal_recovery(&persistence, &persisted_original)
        .unwrap();

    let authorization = original.record_supersession_authorization(&authority);
    let confirmation = original
        .record_supersession_confirmation(&authority, &transition, &authorization)
        .unwrap();
    let replacement_destination = acknowledging_destination();
    let replacement_destination = ResolvedAuditDestination::from_server_authority(
        &authority,
        &replacement_binding,
        &replacement_destination,
    );
    let preflighted_replacement = replacement_destination
        .preflight_for_terminal_supersession()
        .unwrap();
    let supersession_attempt = producer
        .prepare_attempt(
            EventTime::from_unix_milliseconds(302),
            CorrelationId::new("supersession-correlation").unwrap(),
            human(),
            AuditEvent::DependencyAuditTerminalSuperseded {
                obligation: AuditTerminalObligationReference::from_identifier(
                    original_obligation.identifier(),
                ),
            },
        )
        .unwrap()
        .deliver(preflighted_replacement.destination())
        .unwrap();
    let supersession_terminal = producer
        .prepare_completion(
            &supersession_attempt,
            EventTime::from_unix_milliseconds(303),
            AuditOutcomeDetail::DependencyAuditTerminalSuperseded(StateChangeOutcome::Succeeded(
                AuditTerminalCompleteness::Degraded,
            )),
        )
        .unwrap();
    let replacement_obligation = supersession_terminal
        .recovery_obligation(&persistence, &replacement_binding)
        .unwrap();
    let replacement_identifier = replacement_obligation.identifier();
    let supersession = original
        .prepare_supersession(
            &persistence,
            &transition,
            &authorization,
            &confirmation,
            &preflighted_replacement,
            replacement_obligation,
        )
        .unwrap();
    let mismatched_projection = original_terminal
        .recovery_obligation(&persistence, &replacement_binding)
        .unwrap();
    assert_eq!(
        mismatched_projection.identifier(),
        original_obligation.identifier()
    );
    assert_ne!(
        mismatched_projection.projection_bytes(),
        original_obligation.projection_bytes()
    );
    let mut mismatched_store = MemoryAuditTerminalStore::default();
    mismatched_store
        .persist_audit_terminal_obligation(&mismatched_projection)
        .unwrap();
    assert_eq!(
        mismatched_store
            .append_audit_terminal_supersession(&supersession)
            .unwrap_err(),
        DatabaseError::InvalidState
    );
    assert_eq!(
        mismatched_store.pending.front(),
        Some(&persisted_obligation(&persistence, &mismatched_projection))
    );
    assert!(mismatched_store.late_delivery.is_empty());

    assert_eq!(
        store
            .append_audit_terminal_supersession(&supersession)
            .unwrap_err(),
        DatabaseError::InvalidState
    );
    assert_eq!(store.pending.len(), 2);
    assert!(store.late_delivery.is_empty());
    assert_eq!(store.pending.pop_front(), Some(malformed_oldest));
    store
        .append_audit_terminal_supersession(&supersession)
        .unwrap();

    let active = store
        .list_pending_audit_terminal_obligations(
            &persistence,
            AuditTerminalReplayBatchSize::new(1).unwrap(),
        )
        .unwrap();
    let late = store
        .list_late_delivery_audit_terminal_obligations(
            &persistence,
            AuditTerminalReplayBatchSize::new(1).unwrap(),
        )
        .unwrap();
    assert_eq!(active[0].identifier(), replacement_identifier);
    assert_eq!(late[0], persisted_original);
    assert_eq!(
        original
            .deliver(&persistence, &replacement_destination)
            .unwrap_err(),
        AuditTerminalReplayError::DestinationBindingChanged
    );

    let retained_destination = acknowledging_destination();
    let retained_destination = ResolvedAuditDestination::from_server_authority(
        &authority,
        &original_binding,
        &retained_destination,
    );
    original
        .deliver(&persistence, &retained_destination)
        .unwrap()
        .acknowledge(&mut store)
        .unwrap();
    assert!(
        store
            .list_late_delivery_audit_terminal_obligations(
                &persistence,
                AuditTerminalReplayBatchSize::new(1).unwrap(),
            )
            .unwrap()
            .is_empty()
    );

    let replacement = store
        .list_pending_audit_terminal_obligations(
            &persistence,
            AuditTerminalReplayBatchSize::new(1).unwrap(),
        )
        .unwrap()
        .remove(0);
    producer
        .restore_terminal_recovery(&persistence, &replacement)
        .unwrap()
        .deliver(&persistence, &replacement_destination)
        .unwrap()
        .acknowledge(&mut store)
        .unwrap();
    assert!(store.pending.is_empty());
}

struct MemoryAuditTerminalStore {
    persistence: AuditTerminalRecoveryPersistence,
    pending: VecDeque<AuditTerminalObligation>,
    late_delivery: VecDeque<AuditTerminalObligation>,
    supersessions: Vec<(
        weavelit_server_database::AuditTerminalObligationIdentifier,
        Vec<u8>,
    )>,
}

impl Default for MemoryAuditTerminalStore {
    fn default() -> Self {
        Self {
            persistence: recovery_persistence(),
            pending: VecDeque::new(),
            late_delivery: VecDeque::new(),
            supersessions: Vec::new(),
        }
    }
}

impl AuditTerminalRecoveryTransaction for MemoryAuditTerminalStore {
    fn persist_audit_terminal_obligation(
        &mut self,
        obligation: &ValidatedAuditTerminalObligationWrite,
    ) -> Result<(), DatabaseError> {
        let obligation = persisted_obligation(&self.persistence, obligation);
        if let Some(existing) = self
            .pending
            .iter()
            .chain(&self.late_delivery)
            .find(|pending| pending.identifier() == obligation.identifier())
        {
            return if existing == &obligation {
                Ok(())
            } else {
                Err(DatabaseError::InvalidState)
            };
        }
        self.pending.push_back(obligation);
        Ok(())
    }

    fn append_audit_terminal_supersession(
        &mut self,
        supersession: &AuditTerminalSupersession,
    ) -> Result<(), DatabaseError> {
        let replacement =
            persisted_obligation(&self.persistence, supersession.replacement_obligation());
        if let Some((_, disposition)) = self
            .supersessions
            .iter()
            .find(|(identifier, _)| *identifier == supersession.original_obligation().identifier())
        {
            let exact_original = self
                .late_delivery
                .iter()
                .any(|obligation| obligation == supersession.original_obligation());
            let exact_replacement = self
                .pending
                .iter()
                .any(|obligation| obligation == &replacement);
            return if exact_original
                && exact_replacement
                && disposition.as_slice() == supersession.disposition_bytes()
            {
                Ok(())
            } else {
                Err(DatabaseError::InvalidState)
            };
        }
        if self.pending.front() != Some(supersession.original_obligation())
            || self
                .pending
                .iter()
                .chain(&self.late_delivery)
                .any(|obligation| obligation.identifier() == replacement.identifier())
        {
            return Err(DatabaseError::InvalidState);
        }
        let original = self
            .pending
            .pop_front()
            .ok_or(DatabaseError::InvalidState)?;
        self.late_delivery.push_back(original);
        self.pending.push_back(replacement);
        self.supersessions.push((
            supersession.original_obligation().identifier(),
            supersession.disposition_bytes().to_vec(),
        ));
        Ok(())
    }
}

impl AuditTerminalRecoveryStore for MemoryAuditTerminalStore {
    fn list_pending_audit_terminal_obligations(
        &mut self,
        _persistence: &AuditTerminalRecoveryPersistence,
        batch_size: AuditTerminalReplayBatchSize,
    ) -> Result<Vec<AuditTerminalObligation>, DatabaseError> {
        Ok(self
            .pending
            .iter()
            .take(batch_size.get())
            .cloned()
            .collect())
    }

    fn list_late_delivery_audit_terminal_obligations(
        &mut self,
        _persistence: &AuditTerminalRecoveryPersistence,
        batch_size: AuditTerminalReplayBatchSize,
    ) -> Result<Vec<AuditTerminalObligation>, DatabaseError> {
        Ok(self
            .late_delivery
            .iter()
            .take(batch_size.get())
            .cloned()
            .collect())
    }

    fn acknowledge_audit_terminal_obligation(
        &mut self,
        acknowledgement: AuditTerminalAcknowledgementProof,
    ) -> Result<(), DatabaseError> {
        if self.pending.front().is_some_and(|obligation| {
            obligation.identifier() == acknowledgement.identifier()
                && obligation.binding() == acknowledgement.binding()
        }) {
            self.pending.pop_front();
            return Ok(());
        }
        if self.late_delivery.front().is_some_and(|obligation| {
            obligation.identifier() == acknowledgement.identifier()
                && obligation.binding() == acknowledgement.binding()
        }) {
            self.late_delivery.pop_front();
            return Ok(());
        }
        Err(DatabaseError::InvalidState)
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
