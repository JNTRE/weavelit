use core::fmt::{self, Write as _};

use weavelit_server_database::{
    AccountAuditReference, AuditTerminalObligationIdentifier, GroupAuditReference,
};
use weavelit_server_log::{
    AuditLogBody, AuditLogClassification, AuditPrincipal, AuditTerminalCompleteness, LogResult,
};

use crate::AuditError;

const MAX_SAFE_REFERENCE_BYTES: usize = 96;
const ATTEMPT_DETAIL: &str = "accountable action accepted";

macro_rules! safe_reference {
    ($name:ident, $label:literal) => {
        #[doc = concat!("Bounded, non-secret ", $label, " reference approved for Audit output.")]
        #[derive(Clone, Eq, PartialEq)]
        pub struct $name(Box<str>);

        impl $name {
            /// Validates one stable reference without accepting a database identifier.
            pub fn new(value: impl Into<Box<str>>) -> Result<Self, AuditError> {
                let value = value.into();
                if !is_safe_reference(&value) {
                    return Err(AuditError::InvalidReference);
                }
                Ok(Self(value))
            }

            fn render(&self) -> String {
                concat!($label, ":{}").replace("{}", &self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(concat!(stringify!($name), "(REDACTED)"))
            }
        }
    };
}

safe_reference!(AutomationReference, "automation");
safe_reference!(BackupReference, "backup");
safe_reference!(ComponentReference, "component");
safe_reference!(GrantReference, "grant");
safe_reference!(LogConfigurationReference, "log-configuration");
safe_reference!(LogModuleReference, "log-module");
safe_reference!(LogPolicyReference, "log-policy");
safe_reference!(MfaModuleReference, "mfa-module");
safe_reference!(OperationReference, "operation");
safe_reference!(ServiceConnectionReference, "service-connection");

/// Bounded, non-secret reference to one immutable Audit terminal obligation.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AuditTerminalObligationReference(AuditTerminalObligationIdentifier);

impl AuditTerminalObligationReference {
    /// Creates a reference only from the typed nonzero persistence identity.
    #[must_use]
    pub const fn from_identifier(identifier: AuditTerminalObligationIdentifier) -> Self {
        Self(identifier)
    }

    fn render(self) -> String {
        let mut rendered = String::from("audit-terminal:");
        for byte in self.0.as_bytes() {
            write!(&mut rendered, "{byte:02x}").expect("writing to String cannot fail");
        }
        rendered
    }
}

impl fmt::Debug for AuditTerminalObligationReference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuditTerminalObligationReference(REDACTED)")
    }
}

/// Accountable actor represented only by approved Audit-safe references.
#[derive(Clone, Eq, PartialEq)]
pub enum AuditActor {
    /// Authenticated human account.
    Human(AccountAuditReference),
    /// Automation Identity and its required Responsible Owner account.
    Automation {
        identity: AutomationReference,
        responsible_owner: AccountAuditReference,
    },
}

impl AuditActor {
    fn to_log_principal(&self) -> Result<AuditPrincipal, AuditError> {
        match self {
            Self::Human(account) => AuditPrincipal::human(render_account(account)),
            Self::Automation {
                identity,
                responsible_owner,
            } => AuditPrincipal::automation(identity.render(), render_account(responsible_owner)),
        }
        .map_err(|_| AuditError::InvalidRecord)
    }
}

impl fmt::Debug for AuditActor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuditActor(REDACTED)")
    }
}

/// Closed accountable action set covering the implemented Audit taxonomy.
#[derive(Clone, Eq, PartialEq)]
#[non_exhaustive]
pub enum AuditEvent {
    LifecycleBackupCreated {
        backup: BackupReference,
    },
    AuthenticationUserCreated {
        account: AccountAuditReference,
    },
    AuthenticationUserDisabled {
        account: AccountAuditReference,
    },
    AuthenticationPasswordChanged {
        account: AccountAuditReference,
    },
    AuthenticationPasswordResetStarted {
        account: AccountAuditReference,
    },
    AuthenticationMfaEnrolled {
        account: AccountAuditReference,
    },
    AuthenticationMfaReset {
        account: AccountAuditReference,
    },
    AuthenticationMfaRequirementChanged {
        account: AccountAuditReference,
    },
    AuthenticationMfaModuleEnablementChanged {
        module: MfaModuleReference,
    },
    AuthenticationSessionRevoked {
        account: AccountAuditReference,
    },
    AuthorizationGroupCreated {
        group: GroupAuditReference,
    },
    AuthorizationGroupMembershipChanged {
        group: GroupAuditReference,
        account: AccountAuditReference,
    },
    AuthorizationGroupGrantChanged {
        group: GroupAuditReference,
        grant: GrantReference,
    },
    AuthorizationGroupGrantRemovalDenied {
        group: GroupAuditReference,
        account: AccountAuditReference,
    },
    AuthorizationAutomationScopeChanged {
        automation: AutomationReference,
        operation: OperationReference,
    },
    DependencyAuditTerminalSuperseded {
        obligation: AuditTerminalObligationReference,
    },
    DependencyLogModuleConfigurationChanged {
        module: LogModuleReference,
        configuration: LogConfigurationReference,
    },
    DependencyServiceConnectionChanged {
        connection: ServiceConnectionReference,
    },
    ProviderOperation {
        operation: OperationReference,
        connection: ServiceConnectionReference,
    },
    InternalServerConfigurationChanged {
        component: ComponentReference,
    },
    InternalUserStatusChanged {
        account: AccountAuditReference,
    },
    InternalLogPolicyChanged {
        policy: LogPolicyReference,
    },
}

impl AuditEvent {
    pub(crate) fn body(
        &self,
        actor: &AuditActor,
        detail: impl Into<Box<str>>,
        phase: EventPhase,
    ) -> Result<AuditLogBody, AuditError> {
        AuditLogBody::new(
            self.classification(phase),
            actor.to_log_principal()?,
            self.action(phase),
            self.target(),
            detail,
        )
        .map_err(|_| AuditError::InvalidRecord)
    }

    pub(crate) const fn attempt_detail() -> &'static str {
        ATTEMPT_DETAIL
    }

    pub(crate) const fn classification(&self, phase: EventPhase) -> AuditLogClassification {
        match self {
            Self::LifecycleBackupCreated { .. } => AuditLogClassification::LifecycleBackupCreated,
            Self::AuthenticationUserCreated { .. } => {
                AuditLogClassification::AuthenticationUserCreated
            }
            Self::AuthenticationUserDisabled { .. } => {
                AuditLogClassification::AuthenticationUserDisabled
            }
            Self::AuthenticationPasswordChanged { .. } => {
                AuditLogClassification::AuthenticationPasswordChanged
            }
            Self::AuthenticationPasswordResetStarted { .. } => {
                AuditLogClassification::AuthenticationPasswordResetStarted
            }
            Self::AuthenticationMfaEnrolled { .. } => {
                AuditLogClassification::AuthenticationMfaEnrolled
            }
            Self::AuthenticationMfaReset { .. } => AuditLogClassification::AuthenticationMfaReset,
            Self::AuthenticationMfaRequirementChanged { .. } => {
                AuditLogClassification::AuthenticationMfaRequirementChanged
            }
            Self::AuthenticationMfaModuleEnablementChanged { .. } => {
                AuditLogClassification::AuthenticationMfaModuleEnablementChanged
            }
            Self::AuthenticationSessionRevoked { .. } => {
                AuditLogClassification::AuthenticationSessionRevoked
            }
            Self::AuthorizationGroupCreated { .. } => {
                AuditLogClassification::AuthorizationGroupCreated
            }
            Self::AuthorizationGroupMembershipChanged { .. } => {
                AuditLogClassification::AuthorizationGroupMembershipChanged
            }
            Self::AuthorizationGroupGrantChanged { .. } => {
                AuditLogClassification::AuthorizationGroupGrantChanged
            }
            Self::AuthorizationGroupGrantRemovalDenied { .. } => {
                AuditLogClassification::AuthorizationGroupGrantRemovalDenied
            }
            Self::AuthorizationAutomationScopeChanged { .. } => {
                AuditLogClassification::AuthorizationAutomationScopeChanged
            }
            Self::DependencyAuditTerminalSuperseded { .. } => {
                AuditLogClassification::DependencyAuditTerminalSuperseded
            }
            Self::DependencyLogModuleConfigurationChanged { .. } => {
                AuditLogClassification::DependencyLogModuleConfigurationChanged
            }
            Self::DependencyServiceConnectionChanged { .. } => {
                AuditLogClassification::DependencyServiceConnectionChanged
            }
            Self::ProviderOperation { .. } => match phase {
                EventPhase::Attempt => AuditLogClassification::ProviderOperationStarted,
                EventPhase::Terminal => AuditLogClassification::ProviderOperationCompleted,
            },
            Self::InternalServerConfigurationChanged { .. } => {
                AuditLogClassification::InternalServerConfigurationChanged
            }
            Self::InternalUserStatusChanged { .. } => {
                AuditLogClassification::InternalUserStatusChanged
            }
            Self::InternalLogPolicyChanged { .. } => {
                AuditLogClassification::InternalLogPolicyChanged
            }
        }
    }

    pub(crate) const fn action(&self, phase: EventPhase) -> &'static str {
        match self {
            Self::LifecycleBackupCreated { .. } => "create-backup",
            Self::AuthenticationUserCreated { .. } => "create",
            Self::AuthenticationUserDisabled { .. } => "disable",
            Self::AuthenticationPasswordChanged { .. } => "change-password",
            Self::AuthenticationPasswordResetStarted { .. } => "reset-password",
            Self::AuthenticationMfaEnrolled { .. } => "enroll-mfa",
            Self::AuthenticationMfaReset { .. } => "reset-mfa",
            Self::AuthenticationMfaRequirementChanged { .. } => "change-mfa-requirement",
            Self::AuthenticationMfaModuleEnablementChanged { .. } => "change-mfa-module",
            Self::AuthenticationSessionRevoked { .. } => "revoke-session",
            Self::AuthorizationGroupCreated { .. } => "create-group",
            Self::AuthorizationGroupMembershipChanged { .. } => "change-membership",
            Self::AuthorizationGroupGrantChanged { .. } => "change-grant",
            Self::AuthorizationGroupGrantRemovalDenied { .. } => "remove-grant",
            Self::AuthorizationAutomationScopeChanged { .. } => "change-automation-scope",
            Self::DependencyAuditTerminalSuperseded { .. } => "supersede-terminal-delivery",
            Self::DependencyLogModuleConfigurationChanged { .. } => {
                "change-log-module-configuration"
            }
            Self::DependencyServiceConnectionChanged { .. } => "change-service-connection",
            Self::ProviderOperation { .. } => match phase {
                EventPhase::Attempt => "operation-start",
                EventPhase::Terminal => "operation-complete",
            },
            Self::InternalServerConfigurationChanged { .. } => "change-server-configuration",
            Self::InternalUserStatusChanged { .. } => "change-user-status",
            Self::InternalLogPolicyChanged { .. } => "change-log-policy",
        }
    }

    fn target(&self) -> String {
        match self {
            Self::LifecycleBackupCreated { backup } => backup.render(),
            Self::AuthenticationUserCreated { account }
            | Self::AuthenticationUserDisabled { account }
            | Self::AuthenticationPasswordChanged { account }
            | Self::AuthenticationPasswordResetStarted { account }
            | Self::AuthenticationMfaEnrolled { account }
            | Self::AuthenticationMfaReset { account }
            | Self::AuthenticationMfaRequirementChanged { account }
            | Self::AuthenticationSessionRevoked { account }
            | Self::InternalUserStatusChanged { account } => render_account(account),
            Self::AuthenticationMfaModuleEnablementChanged { module } => module.render(),
            Self::AuthorizationGroupCreated { group } => render_group(group),
            Self::AuthorizationGroupMembershipChanged { group, account }
            | Self::AuthorizationGroupGrantRemovalDenied { group, account } => {
                join_targets(render_group(group), render_account(account))
            }
            Self::AuthorizationGroupGrantChanged { group, grant } => {
                join_targets(render_group(group), grant.render())
            }
            Self::AuthorizationAutomationScopeChanged {
                automation,
                operation,
            } => join_targets(automation.render(), operation.render()),
            Self::DependencyAuditTerminalSuperseded { obligation } => obligation.render(),
            Self::DependencyLogModuleConfigurationChanged {
                module,
                configuration,
            } => join_targets(module.render(), configuration.render()),
            Self::DependencyServiceConnectionChanged { connection } => connection.render(),
            Self::ProviderOperation {
                operation,
                connection,
            } => join_targets(operation.render(), connection.render()),
            Self::InternalServerConfigurationChanged { component } => component.render(),
            Self::InternalLogPolicyChanged { policy } => policy.render(),
        }
    }

    const fn expected_detail_kind(&self) -> DetailKind {
        match self {
            Self::LifecycleBackupCreated { .. } => DetailKind::LifecycleBackupCreated,
            Self::AuthenticationUserCreated { .. } => DetailKind::AuthenticationUserCreated,
            Self::AuthenticationUserDisabled { .. } => DetailKind::AuthenticationUserDisabled,
            Self::AuthenticationPasswordChanged { .. } => DetailKind::AuthenticationPasswordChanged,
            Self::AuthenticationPasswordResetStarted { .. } => {
                DetailKind::AuthenticationPasswordResetStarted
            }
            Self::AuthenticationMfaEnrolled { .. } => DetailKind::AuthenticationMfaEnrolled,
            Self::AuthenticationMfaReset { .. } => DetailKind::AuthenticationMfaReset,
            Self::AuthenticationMfaRequirementChanged { .. } => {
                DetailKind::AuthenticationMfaRequirementChanged
            }
            Self::AuthenticationMfaModuleEnablementChanged { .. } => {
                DetailKind::AuthenticationMfaModuleEnablementChanged
            }
            Self::AuthenticationSessionRevoked { .. } => DetailKind::AuthenticationSessionRevoked,
            Self::AuthorizationGroupCreated { .. } => DetailKind::AuthorizationGroupCreated,
            Self::AuthorizationGroupMembershipChanged { .. } => {
                DetailKind::AuthorizationGroupMembershipChanged
            }
            Self::AuthorizationGroupGrantChanged { .. } => {
                DetailKind::AuthorizationGroupGrantChanged
            }
            Self::AuthorizationGroupGrantRemovalDenied { .. } => {
                DetailKind::AuthorizationGroupGrantRemovalDenied
            }
            Self::AuthorizationAutomationScopeChanged { .. } => {
                DetailKind::AuthorizationAutomationScopeChanged
            }
            Self::DependencyAuditTerminalSuperseded { .. } => {
                DetailKind::DependencyAuditTerminalSuperseded
            }
            Self::DependencyLogModuleConfigurationChanged { .. } => {
                DetailKind::DependencyLogModuleConfigurationChanged
            }
            Self::DependencyServiceConnectionChanged { .. } => {
                DetailKind::DependencyServiceConnectionChanged
            }
            Self::ProviderOperation { .. } => DetailKind::ProviderOperation,
            Self::InternalServerConfigurationChanged { .. } => {
                DetailKind::InternalServerConfigurationChanged
            }
            Self::InternalUserStatusChanged { .. } => DetailKind::InternalUserStatusChanged,
            Self::InternalLogPolicyChanged { .. } => DetailKind::InternalLogPolicyChanged,
        }
    }

    pub(crate) fn terminal_outcome(
        &self,
        detail: AuditOutcomeDetail,
    ) -> Result<TerminalOutcome, AuditError> {
        use AuditOutcomeDetail as Detail;

        if self.expected_detail_kind() != detail.kind() {
            return Err(AuditError::InvalidOutcome);
        }

        match detail {
            Detail::LifecycleBackupCreated(outcome)
            | Detail::AuthenticationUserCreated(outcome)
            | Detail::AuthenticationPasswordChanged(outcome)
            | Detail::AuthenticationPasswordResetStarted(outcome)
            | Detail::AuthenticationMfaEnrolled(outcome)
            | Detail::AuthenticationSessionRevoked(outcome)
            | Detail::AuthorizationGroupCreated(outcome)
            | Detail::AuthorizationGroupMembershipChanged(outcome)
            | Detail::AuthorizationGroupGrantChanged(outcome)
            | Detail::AuthorizationAutomationScopeChanged(outcome)
            | Detail::DependencyLogModuleConfigurationChanged(outcome)
            | Detail::DependencyServiceConnectionChanged(outcome)
            | Detail::ProviderOperation(outcome)
            | Detail::InternalLogPolicyChanged(outcome) => Ok(TerminalOutcome::action(outcome)),
            Detail::DependencyAuditTerminalSuperseded(outcome) => {
                Ok(TerminalOutcome::state_change(outcome))
            }
            Detail::AuthenticationUserDisabled(outcome) => match outcome {
                StateChangeOutcome::Succeeded(AccountStatus::Active) => {
                    Err(AuditError::InvalidOutcome)
                }
                _ => Ok(TerminalOutcome::state_change(outcome)),
            },
            Detail::AuthenticationMfaReset(outcome) => Ok(TerminalOutcome::state_change(outcome)),
            Detail::AuthenticationMfaRequirementChanged(outcome) => {
                Ok(TerminalOutcome::state_change(outcome))
            }
            Detail::AuthenticationMfaModuleEnablementChanged(outcome) => {
                Ok(TerminalOutcome::state_change(outcome))
            }
            Detail::AuthorizationGroupGrantRemovalDenied => {
                Ok(TerminalOutcome::action(ActionOutcome::Denied))
            }
            Detail::InternalServerConfigurationChanged(outcome) => {
                Ok(TerminalOutcome::state_change(outcome))
            }
            Detail::InternalUserStatusChanged(outcome) => {
                Ok(TerminalOutcome::state_change(outcome))
            }
        }
    }
}

impl fmt::Debug for AuditEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuditEvent(REDACTED)")
    }
}

/// Closed result for an action whose success has no additional safe fact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionOutcome {
    Succeeded,
    Denied,
    Failed,
}

/// Closed result for a state mutation whose success requires a committed fact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateChangeOutcome<T> {
    Succeeded(T),
    Denied,
    Failed,
}

/// Safe resulting account state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountStatus {
    Active,
    Disabled,
}

/// Safe resulting MFA policy requirement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MfaRequirement {
    Required,
    Optional,
}

/// Safe resulting component or MFA Module state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComponentState {
    Enabled,
    Disabled,
}

/// Safe resulting MFA reset state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MfaResetState {
    ReenrollmentRequired,
}

/// Committed MFA Module state and the number of affected Human Users.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MfaModuleChange {
    state: ComponentState,
    affected_count: u64,
}

impl MfaModuleChange {
    #[must_use]
    pub const fn new(state: ComponentState, affected_count: u64) -> Self {
        Self {
            state,
            affected_count,
        }
    }

    #[must_use]
    pub const fn state(self) -> ComponentState {
        self.state
    }

    #[must_use]
    pub const fn affected_count(self) -> u64 {
        self.affected_count
    }
}

/// Exhaustive terminal detail paired one-for-one with [`AuditEvent`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AuditOutcomeDetail {
    LifecycleBackupCreated(ActionOutcome),
    AuthenticationUserCreated(ActionOutcome),
    AuthenticationUserDisabled(StateChangeOutcome<AccountStatus>),
    AuthenticationPasswordChanged(ActionOutcome),
    AuthenticationPasswordResetStarted(ActionOutcome),
    AuthenticationMfaEnrolled(ActionOutcome),
    AuthenticationMfaReset(StateChangeOutcome<MfaResetState>),
    AuthenticationMfaRequirementChanged(StateChangeOutcome<MfaRequirement>),
    AuthenticationMfaModuleEnablementChanged(StateChangeOutcome<MfaModuleChange>),
    AuthenticationSessionRevoked(ActionOutcome),
    AuthorizationGroupCreated(ActionOutcome),
    AuthorizationGroupMembershipChanged(ActionOutcome),
    AuthorizationGroupGrantChanged(ActionOutcome),
    AuthorizationGroupGrantRemovalDenied,
    AuthorizationAutomationScopeChanged(ActionOutcome),
    DependencyAuditTerminalSuperseded(StateChangeOutcome<AuditTerminalCompleteness>),
    DependencyLogModuleConfigurationChanged(ActionOutcome),
    DependencyServiceConnectionChanged(ActionOutcome),
    ProviderOperation(ActionOutcome),
    InternalServerConfigurationChanged(StateChangeOutcome<ComponentState>),
    InternalUserStatusChanged(StateChangeOutcome<AccountStatus>),
    InternalLogPolicyChanged(ActionOutcome),
}

impl AuditOutcomeDetail {
    const fn kind(&self) -> DetailKind {
        match self {
            Self::LifecycleBackupCreated(_) => DetailKind::LifecycleBackupCreated,
            Self::AuthenticationUserCreated(_) => DetailKind::AuthenticationUserCreated,
            Self::AuthenticationUserDisabled(_) => DetailKind::AuthenticationUserDisabled,
            Self::AuthenticationPasswordChanged(_) => DetailKind::AuthenticationPasswordChanged,
            Self::AuthenticationPasswordResetStarted(_) => {
                DetailKind::AuthenticationPasswordResetStarted
            }
            Self::AuthenticationMfaEnrolled(_) => DetailKind::AuthenticationMfaEnrolled,
            Self::AuthenticationMfaReset(_) => DetailKind::AuthenticationMfaReset,
            Self::AuthenticationMfaRequirementChanged(_) => {
                DetailKind::AuthenticationMfaRequirementChanged
            }
            Self::AuthenticationMfaModuleEnablementChanged(_) => {
                DetailKind::AuthenticationMfaModuleEnablementChanged
            }
            Self::AuthenticationSessionRevoked(_) => DetailKind::AuthenticationSessionRevoked,
            Self::AuthorizationGroupCreated(_) => DetailKind::AuthorizationGroupCreated,
            Self::AuthorizationGroupMembershipChanged(_) => {
                DetailKind::AuthorizationGroupMembershipChanged
            }
            Self::AuthorizationGroupGrantChanged(_) => DetailKind::AuthorizationGroupGrantChanged,
            Self::AuthorizationGroupGrantRemovalDenied => {
                DetailKind::AuthorizationGroupGrantRemovalDenied
            }
            Self::AuthorizationAutomationScopeChanged(_) => {
                DetailKind::AuthorizationAutomationScopeChanged
            }
            Self::DependencyAuditTerminalSuperseded(_) => {
                DetailKind::DependencyAuditTerminalSuperseded
            }
            Self::DependencyLogModuleConfigurationChanged(_) => {
                DetailKind::DependencyLogModuleConfigurationChanged
            }
            Self::DependencyServiceConnectionChanged(_) => {
                DetailKind::DependencyServiceConnectionChanged
            }
            Self::ProviderOperation(_) => DetailKind::ProviderOperation,
            Self::InternalServerConfigurationChanged(_) => {
                DetailKind::InternalServerConfigurationChanged
            }
            Self::InternalUserStatusChanged(_) => DetailKind::InternalUserStatusChanged,
            Self::InternalLogPolicyChanged(_) => DetailKind::InternalLogPolicyChanged,
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum DetailKind {
    LifecycleBackupCreated,
    AuthenticationUserCreated,
    AuthenticationUserDisabled,
    AuthenticationPasswordChanged,
    AuthenticationPasswordResetStarted,
    AuthenticationMfaEnrolled,
    AuthenticationMfaReset,
    AuthenticationMfaRequirementChanged,
    AuthenticationMfaModuleEnablementChanged,
    AuthenticationSessionRevoked,
    AuthorizationGroupCreated,
    AuthorizationGroupMembershipChanged,
    AuthorizationGroupGrantChanged,
    AuthorizationGroupGrantRemovalDenied,
    AuthorizationAutomationScopeChanged,
    DependencyAuditTerminalSuperseded,
    DependencyLogModuleConfigurationChanged,
    DependencyServiceConnectionChanged,
    ProviderOperation,
    InternalServerConfigurationChanged,
    InternalUserStatusChanged,
    InternalLogPolicyChanged,
}

#[derive(Clone, Copy)]
pub(crate) enum EventPhase {
    Attempt,
    Terminal,
}

#[derive(Clone, Copy)]
enum OutcomeKind {
    Succeeded,
    Denied,
    Failed,
}

pub(crate) struct TerminalOutcome {
    kind: OutcomeKind,
    fact: Option<String>,
}

impl TerminalOutcome {
    fn action(outcome: ActionOutcome) -> Self {
        let kind = match outcome {
            ActionOutcome::Succeeded => OutcomeKind::Succeeded,
            ActionOutcome::Denied => OutcomeKind::Denied,
            ActionOutcome::Failed => OutcomeKind::Failed,
        };
        Self { kind, fact: None }
    }

    fn state_change<T: SafeFact>(outcome: StateChangeOutcome<T>) -> Self {
        match outcome {
            StateChangeOutcome::Succeeded(fact) => Self {
                kind: OutcomeKind::Succeeded,
                fact: Some(fact.render()),
            },
            StateChangeOutcome::Denied => Self {
                kind: OutcomeKind::Denied,
                fact: None,
            },
            StateChangeOutcome::Failed => Self {
                kind: OutcomeKind::Failed,
                fact: None,
            },
        }
    }

    pub(crate) const fn result(&self) -> LogResult {
        match self.kind {
            OutcomeKind::Succeeded => LogResult::Success,
            OutcomeKind::Denied | OutcomeKind::Failed => LogResult::Failure,
        }
    }

    pub(crate) fn completion_detail(&self) -> String {
        self.render(match self.kind {
            OutcomeKind::Succeeded => "accountable action completed successfully",
            OutcomeKind::Denied => "accountable action denied",
            OutcomeKind::Failed => "accountable action failed",
        })
    }

    pub(crate) fn correction_detail(&self) -> String {
        self.render(match self.kind {
            OutcomeKind::Succeeded => "corrected outcome: accountable action succeeded",
            OutcomeKind::Denied => "corrected outcome: accountable action was denied",
            OutcomeKind::Failed => "corrected outcome: accountable action failed",
        })
    }

    fn render(&self, summary: &str) -> String {
        self.fact
            .as_ref()
            .map_or_else(|| summary.to_owned(), |fact| format!("{summary}; {fact}"))
    }
}

trait SafeFact: Copy {
    fn render(self) -> String;
}

impl SafeFact for AccountStatus {
    fn render(self) -> String {
        match self {
            Self::Active => "account status: active".to_owned(),
            Self::Disabled => "account status: disabled".to_owned(),
        }
    }
}

impl SafeFact for MfaRequirement {
    fn render(self) -> String {
        match self {
            Self::Required => "MFA requirement: required".to_owned(),
            Self::Optional => "MFA requirement: optional".to_owned(),
        }
    }
}

impl SafeFact for ComponentState {
    fn render(self) -> String {
        match self {
            Self::Enabled => "component state: enabled".to_owned(),
            Self::Disabled => "component state: disabled".to_owned(),
        }
    }
}

impl SafeFact for MfaResetState {
    fn render(self) -> String {
        match self {
            Self::ReenrollmentRequired => "MFA reset state: re-enrollment required".to_owned(),
        }
    }
}

impl SafeFact for MfaModuleChange {
    fn render(self) -> String {
        let state = match self.state {
            ComponentState::Enabled => "enabled",
            ComponentState::Disabled => "disabled",
        };
        format!(
            "MFA module state: {state}; affected count: {}",
            self.affected_count
        )
    }
}

impl SafeFact for AuditTerminalCompleteness {
    fn render(self) -> String {
        match self {
            Self::Degraded => "Audit completeness: degraded".to_owned(),
        }
    }
}

fn render_account(reference: &AccountAuditReference) -> String {
    format!("account:{}", reference.audit_reference())
}

fn render_group(reference: &GroupAuditReference) -> String {
    format!("group:{}", reference.audit_reference())
}

fn join_targets(first: String, second: String) -> String {
    format!("{first};{second}")
}

fn is_safe_reference(value: &str) -> bool {
    if value.is_empty()
        || value.len() > MAX_SAFE_REFERENCE_BYTES
        || value.starts_with(['-', '.'])
        || value.ends_with(['-', '.'])
        || value
            .as_bytes()
            .windows(2)
            .any(|pair| pair == b"--" || pair == b"..")
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'.')
        })
    {
        return false;
    }

    let compact: String = value
        .chars()
        .filter(|character| *character != '-')
        .collect();
    let hexadecimal_identifier =
        compact.len() == 32 && compact.bytes().all(|byte| byte.is_ascii_hexdigit());
    !hexadecimal_identifier
}

#[cfg(test)]
mod tests {
    use super::AutomationReference;
    use crate::AuditError;

    #[test]
    fn safe_references_reject_free_form_oversized_and_database_identifiers() {
        for forbidden in [
            "",
            "Correct Horse Battery Staple",
            "raw/request",
            "0123456789abcdef0123456789abcdef",
            "01234567-89ab-cdef-0123-456789abcdef",
        ] {
            assert_eq!(
                AutomationReference::new(forbidden).unwrap_err(),
                AuditError::InvalidReference
            );
        }
        assert_eq!(
            AutomationReference::new("a".repeat(97)).unwrap_err(),
            AuditError::InvalidReference
        );
        AutomationReference::new("a".repeat(96)).expect("the boundary value must be accepted");
        AutomationReference::new("abcdefghijklmnopqrstuv")
            .expect("an ordinary 22-character identifier must be accepted");
    }
}
