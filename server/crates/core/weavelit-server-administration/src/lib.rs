//! Typed, transport-independent Administration Plane authorization contract.
//!
//! A caller can reach the single [`AdministrationPlane::authorize`] entry only
//! with the compound admission produced by the Server authorization runtime.
//! The contract additionally enforces current-session MFA step-up for the two
//! designated action families, reads persisted component enablement for every
//! component Operation, and admits enablement changes only for compiled-in targets.

#![forbid(unsafe_code)]

use std::{error::Error as StdError, fmt, time::Duration};

use weavelit_server_administration_authority::ServerAdministrationAuthority;
use weavelit_server_authorization::{AuthorizationDenied, AuthorizedAdministration};
use weavelit_server_components::AvailableComponents;
use weavelit_server_database::{
    AccountPublicIdentifier, AccountStatus, ComponentEnablement, ComponentKind, GroupGrant,
    LogModuleSetting, LogType, Name, SessionTokenHash, StateIdentifier,
};

/// Lifetime of one current-session MFA step-up proof.
pub const MFA_STEP_UP_LIFETIME: Duration = Duration::from_secs(5 * 60);

/// Injected monotonic clock used for step-up issuance and validation.
pub trait AdministrationClock {
    /// Returns elapsed time from one stable process-local origin.
    fn now(&self) -> Duration;
}

/// Live source of the persisted component-enablement projection.
pub trait ComponentEnablementSource {
    /// Loads enablement for exactly one authorization check.
    ///
    /// An unavailable read is mapped to the same reason-free denial as a
    /// disabled component before it reaches this boundary.
    fn load_component_enablement(&mut self) -> Result<ComponentEnablement, AuthorizationDenied>;
}

/// Stable payload-free rejection raised before an administration request exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdministrationInputRejected;

impl fmt::Display for AdministrationInputRejected {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("administration input rejected")
    }
}

impl StdError for AdministrationInputRejected {}

/// One bounded component or named Operation whose enabled state is required.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentOperation {
    kind: ComponentKind,
    name: Name,
}

impl ComponentOperation {
    /// Validates a component target against the shared persisted-name bound.
    pub fn new(
        kind: ComponentKind,
        name: impl Into<Box<str>>,
    ) -> Result<Self, AdministrationInputRejected> {
        Ok(Self {
            kind,
            name: Name::new(name).map_err(|_| AdministrationInputRejected)?,
        })
    }

    /// Returns the persisted component kind.
    #[must_use]
    pub const fn kind(&self) -> ComponentKind {
        self.kind
    }

    /// Returns the bounded component name.
    #[must_use]
    pub const fn name(&self) -> &Name {
        &self.name
    }

    fn is_available(&self, components: &AvailableComponents) -> bool {
        match self.kind {
            ComponentKind::ClientModule => components.has_client_module(&self.name),
            ComponentKind::ServiceModule => components.has_service_module(&self.name),
            ComponentKind::Operation => components.has_operation(&self.name),
            ComponentKind::MfaModule => components.has_mfa_module(&self.name),
        }
    }
}

/// One bounded compiled-in component and its desired enablement state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentEnablementChange {
    kind: ComponentKind,
    name: Name,
    enabled: bool,
}

impl ComponentEnablementChange {
    /// Validates a component target against the shared persisted-name bound.
    pub fn new(
        kind: ComponentKind,
        name: impl Into<Box<str>>,
        enabled: bool,
    ) -> Result<Self, AdministrationInputRejected> {
        Ok(Self {
            kind,
            name: Name::new(name).map_err(|_| AdministrationInputRejected)?,
            enabled,
        })
    }

    /// Returns the persisted component kind.
    #[must_use]
    pub const fn kind(&self) -> ComponentKind {
        self.kind
    }

    /// Returns the bounded component name.
    #[must_use]
    pub const fn name(&self) -> &Name {
        &self.name
    }

    /// Returns the desired enablement state.
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    fn is_available(&self, components: &AvailableComponents) -> bool {
        match self.kind {
            ComponentKind::ClientModule => components.has_client_module(&self.name),
            ComponentKind::ServiceModule => components.has_service_module(&self.name),
            ComponentKind::Operation => components.has_operation(&self.name),
            ComponentKind::MfaModule => components.has_mfa_module(&self.name),
        }
    }
}

/// Desired destination for one Log Type after a configuration change.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct LogAssignmentChange {
    log_type: LogType,
    configuration: StateIdentifier,
}

impl LogAssignmentChange {
    /// Binds one Log Type to its desired existing configuration.
    #[must_use]
    pub const fn new(log_type: LogType, configuration: StateIdentifier) -> Self {
        Self {
            log_type,
            configuration,
        }
    }

    /// Returns the Log Type whose assignment is being selected.
    #[must_use]
    pub const fn log_type(&self) -> LogType {
        self.log_type
    }

    /// Returns the desired destination configuration.
    #[must_use]
    pub const fn configuration(&self) -> StateIdentifier {
        self.configuration
    }
}

/// One bounded, assignment-aware Log Module configuration change.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogConfigurationChange {
    primary: StateIdentifier,
    enabled: Option<bool>,
    settings: Option<Box<[LogModuleSetting]>>,
    assignments: Box<[LogAssignmentChange]>,
}

impl LogConfigurationChange {
    /// Validates and canonicalizes one complete requested change.
    pub fn new(
        primary: StateIdentifier,
        enabled: Option<bool>,
        mut settings: Option<Vec<LogModuleSetting>>,
        mut assignments: Vec<LogAssignmentChange>,
    ) -> Result<Self, AdministrationInputRejected> {
        if enabled.is_none() && settings.is_none() && assignments.is_empty() {
            return Err(AdministrationInputRejected);
        }
        if let Some(settings) = settings.as_mut() {
            settings.sort_by(|left, right| left.key.cmp(&right.key));
            if settings.windows(2).any(|pair| pair[0].key == pair[1].key) {
                return Err(AdministrationInputRejected);
            }
        }
        assignments.sort_unstable();
        if assignments
            .windows(2)
            .any(|pair| pair[0].log_type == pair[1].log_type)
        {
            return Err(AdministrationInputRejected);
        }

        Ok(Self {
            primary,
            enabled,
            settings: settings.map(Vec::into_boxed_slice),
            assignments: assignments.into_boxed_slice(),
        })
    }

    /// Returns the existing logical configuration that anchors this change.
    #[must_use]
    pub const fn primary(&self) -> StateIdentifier {
        self.primary
    }

    /// Returns the requested enabled state, when it is part of the change.
    #[must_use]
    pub const fn enabled(&self) -> Option<bool> {
        self.enabled
    }

    /// Returns the complete desired settings, when settings are part of the change.
    #[must_use]
    pub fn settings(&self) -> Option<&[LogModuleSetting]> {
        self.settings.as_deref()
    }

    /// Returns the canonically ordered desired Log Type assignments.
    #[must_use]
    pub const fn assignments(&self) -> &[LogAssignmentChange] {
        &self.assignments
    }
}

/// One bounded account administration read admitted by the action gate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountAdministrationRead {
    /// List every bounded account projection in the store's canonical order.
    List,
    /// View the account with one exact typed public identifier.
    View(AccountPublicIdentifier),
}

/// One bounded local Human User account creation request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountCreate {
    username: Name,
    display_name: Option<Name>,
}

impl AccountCreate {
    /// Validates the username and display name against persisted-name bounds.
    pub fn new(
        username: impl Into<Box<str>>,
        display_name: Option<impl Into<Box<str>>>,
    ) -> Result<Self, AdministrationInputRejected> {
        Ok(Self {
            username: Name::new(username).map_err(|_| AdministrationInputRejected)?,
            display_name: display_name
                .map(|display_name| {
                    Name::new(display_name).map_err(|_| AdministrationInputRejected)
                })
                .transpose()?,
        })
    }

    /// Returns the requested unique username.
    #[must_use]
    pub const fn username(&self) -> &Name {
        &self.username
    }

    /// Returns the requested display name.
    #[must_use]
    pub const fn display_name(&self) -> Option<&Name> {
        self.display_name.as_ref()
    }
}

/// One exact local Human User password-reset target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccountPasswordReset {
    target: AccountPublicIdentifier,
}

impl AccountPasswordReset {
    /// Targets one exact typed account public identifier.
    #[must_use]
    pub const fn new(target: AccountPublicIdentifier) -> Self {
        Self { target }
    }

    /// Returns the exact target account public identifier.
    #[must_use]
    pub const fn target(&self) -> AccountPublicIdentifier {
        self.target
    }
}

/// One exact local Human User account status change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccountStatusChange {
    target: AccountPublicIdentifier,
    desired: AccountStatus,
}

impl AccountStatusChange {
    /// Targets one exact typed account public identifier and desired status.
    #[must_use]
    pub const fn new(target: AccountPublicIdentifier, desired: AccountStatus) -> Self {
        Self { target, desired }
    }

    /// Returns the exact target account public identifier.
    #[must_use]
    pub const fn target(&self) -> AccountPublicIdentifier {
        self.target
    }

    /// Returns the desired account status.
    #[must_use]
    pub const fn desired(&self) -> AccountStatus {
        self.desired
    }
}

/// Closed account administration actions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccountAdministrationAction {
    /// Read the bounded account administration projection.
    Read(AccountAdministrationRead),
    /// Create one local Human User account with a temporary credential.
    Create(AccountCreate),
    /// Replace one local Human User account's credential with a temporary credential.
    PasswordReset(AccountPasswordReset),
    /// Change one local Human User account's active status.
    StatusChange(AccountStatusChange),
}

/// One exact existing-Group membership change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GroupMembershipMutation {
    group: StateIdentifier,
    account: AccountPublicIdentifier,
    desired: bool,
}

impl GroupMembershipMutation {
    /// Targets one existing Group and one account public identifier.
    #[must_use]
    pub const fn new(
        group: StateIdentifier,
        account: AccountPublicIdentifier,
        desired: bool,
    ) -> Self {
        Self {
            group,
            account,
            desired,
        }
    }

    /// Returns the existing Group target.
    #[must_use]
    pub const fn group(&self) -> StateIdentifier {
        self.group
    }

    /// Returns the exact account public identifier target.
    #[must_use]
    pub const fn account(&self) -> AccountPublicIdentifier {
        self.account
    }

    /// Returns whether the membership must be present after the change.
    #[must_use]
    pub const fn desired(&self) -> bool {
        self.desired
    }
}

/// One exact existing-Group direct grant change.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupGrantMutation {
    group: StateIdentifier,
    grant: GroupGrant,
    desired: bool,
}

impl GroupGrantMutation {
    /// Targets one existing Group and one canonical grant.
    #[must_use]
    pub const fn new(group: StateIdentifier, grant: GroupGrant, desired: bool) -> Self {
        Self {
            group,
            grant,
            desired,
        }
    }

    /// Returns the existing Group target.
    #[must_use]
    pub const fn group(&self) -> StateIdentifier {
        self.group
    }

    /// Returns the canonical direct grant target.
    #[must_use]
    pub const fn grant(&self) -> &GroupGrant {
        &self.grant
    }

    /// Returns whether the grant must be present after the change.
    #[must_use]
    pub const fn desired(&self) -> bool {
        self.desired
    }

    fn is_available(&self, components: &AvailableComponents) -> bool {
        match &self.grant {
            GroupGrant::ClientModule(name) => components.has_client_module(name),
            GroupGrant::ServiceModule(name) => components.has_service_module(name),
            GroupGrant::Operation(name) => components.has_operation(name),
            GroupGrant::ServerAdministration => true,
        }
    }
}

/// Closed existing-Group mutation descriptors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GroupMutation {
    /// Add or remove one account membership.
    Membership(GroupMembershipMutation),
    /// Add or remove one direct Group grant.
    Grant(GroupGrantMutation),
}

/// Closed Administration Plane action families owned by this foundation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdministrationAction {
    /// Account administration; credential issuance has its own exact-session gate.
    Account(AccountAdministrationAction),
    /// MFA requirement or enrollment-reset administration.
    MfaPolicy,
    /// Group membership or grant mutation.
    GrantMutation(GroupMutation),
    /// An action whose target component or named Operation must be enabled.
    ComponentOperation(ComponentOperation),
    /// A requested enablement change for one known compiled-in component.
    ComponentEnablementChange(ComponentEnablementChange),
    /// A requested internal Log Module configuration and assignment change.
    LogConfigurationChange(LogConfigurationChange),
}

impl AdministrationAction {
    fn step_up_family(&self) -> Option<StepUpActionFamily> {
        match self {
            Self::Account(_)
            | Self::ComponentOperation(_)
            | Self::ComponentEnablementChange(_)
            | Self::LogConfigurationChange(_) => None,
            Self::MfaPolicy => Some(StepUpActionFamily::MfaPolicy),
            Self::GrantMutation(_) => Some(StepUpActionFamily::GrantMutation),
        }
    }
}

/// The action families for which current-session MFA step-up can be proven.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StepUpActionFamily {
    /// MFA policy and enrollment-reset actions.
    MfaPolicy,
    /// Group membership and grant mutations.
    GrantMutation,
}

/// One Administration Plane authorization bound to its exact validated session.
///
/// Trusted Server composition consumes the lower-level authorization proof and
/// binds the authenticated actor and stored session digest in the same step.
/// The fields are private, the value is not clonable, and diagnostics expose no
/// proof, actor, Client Module, or session value.
pub struct AuthorizedAdministrationAdmission {
    authorization: AuthorizedAdministration,
    actor: StateIdentifier,
    session: SessionTokenHash,
}

impl AuthorizedAdministrationAdmission {
    /// Binds a successful authorization to the validated session that produced it.
    #[must_use]
    pub const fn from_server_authority(
        _authority: &ServerAdministrationAuthority,
        authorization: AuthorizedAdministration,
        actor: StateIdentifier,
        session: SessionTokenHash,
    ) -> Self {
        Self {
            authorization,
            actor,
            session,
        }
    }

    /// Consumes live Administration authorization into credential assurance.
    ///
    /// The proof route does not select an account action. A later create or
    /// reset request must independently authorize its exact action before it
    /// can claim the ticket minted from this handoff.
    #[must_use]
    pub fn into_credential_issuance(self) -> AuthorizedCredentialIssuance {
        AuthorizedCredentialIssuance {
            actor: self.actor,
            session: self.session,
            client_module: self.authorization.client_module().clone(),
        }
    }
}

impl fmt::Debug for AuthorizedAdministrationAdmission {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuthorizedAdministrationAdmission(REDACTED)")
    }
}

/// Live Administration authorization for one credential-assurance attempt.
///
/// This value carries no create or reset target. It is not clonable and can be
/// obtained only by consuming the Server-bound Administration admission.
pub struct AuthorizedCredentialIssuance {
    actor: StateIdentifier,
    session: SessionTokenHash,
    client_module: Name,
}

impl AuthorizedCredentialIssuance {
    /// Returns the authenticated Administrator account.
    #[must_use]
    pub const fn actor(&self) -> StateIdentifier {
        self.actor
    }

    /// Returns the exact validated session digest.
    #[must_use]
    pub const fn session(&self) -> SessionTokenHash {
        self.session
    }

    /// Returns the Client Module through which assurance was authorized.
    #[must_use]
    pub const fn client_module(&self) -> &Name {
        &self.client_module
    }
}

impl fmt::Debug for AuthorizedCredentialIssuance {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuthorizedCredentialIssuance(REDACTED)")
    }
}

/// One session validated by Server authentication for step-up issuance.
///
/// Construction requires the Server-only authority dependency. The actor and
/// session digest are retained privately so proof issuance uses the exact
/// session whose MFA verification succeeded. This value is never supplied to
/// the administration action gate.
pub struct CurrentAdministrationSession {
    actor: StateIdentifier,
    session: SessionTokenHash,
    factor: StateIdentifier,
}

impl CurrentAdministrationSession {
    /// Binds the validated account and session digest in trusted composition.
    #[must_use]
    pub const fn from_server_authority(
        _authority: &ServerAdministrationAuthority,
        actor: StateIdentifier,
        session: SessionTokenHash,
        factor: StateIdentifier,
    ) -> Self {
        Self {
            actor,
            session,
            factor,
        }
    }

    /// Returns the authenticated account for future accountable workflows.
    #[must_use]
    pub const fn actor(&self) -> StateIdentifier {
        self.actor
    }
}

impl fmt::Debug for CurrentAdministrationSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CurrentAdministrationSession(REDACTED)")
    }
}

/// Non-forgeable proof of current-session MFA for one action family.
///
/// Fields and construction remain private to this crate. The proof is borrowed
/// by requests, so the same proof can authorize matching actions until its
/// fixed expiry without becoming clonable or caller-editable.
pub struct MfaStepUpProof {
    actor: StateIdentifier,
    session: SessionTokenHash,
    factor: StateIdentifier,
    family: StepUpActionFamily,
    issued_at: Duration,
    expires_at: Duration,
}

impl MfaStepUpProof {
    fn permits(
        &self,
        admission: &AuthorizedAdministrationAdmission,
        family: StepUpActionFamily,
        now: Duration,
    ) -> bool {
        self.actor == admission.actor
            && self.session.matches(&admission.session)
            && self.family == family
            && now >= self.issued_at
            && now < self.expires_at
    }
}

impl fmt::Debug for MfaStepUpProof {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MfaStepUpProof(REDACTED)")
    }
}

/// One typed administration authorization request.
#[derive(Debug)]
pub struct AdministrationRequest<'proof> {
    action: AdministrationAction,
    step_up: Option<&'proof MfaStepUpProof>,
}

impl<'proof> AdministrationRequest<'proof> {
    /// Creates a request without a step-up proof.
    #[must_use]
    pub const fn new(action: AdministrationAction) -> Self {
        Self {
            action,
            step_up: None,
        }
    }

    /// Supplies a reusable proof for a policy-protected action.
    #[must_use]
    pub const fn with_step_up(mut self, proof: &'proof MfaStepUpProof) -> Self {
        self.step_up = Some(proof);
        self
    }
}

/// Non-forgeable result authorizing one typed administration action.
///
/// The compound admission is retained by value, so a future workflow cannot
/// obtain this result without spending the exact authorization/session binding
/// here.
pub struct AuthorizedAdministrationAction {
    admission: AuthorizedAdministrationAdmission,
    action: AdministrationAction,
    step_up_factor: Option<StateIdentifier>,
}

impl AuthorizedAdministrationAction {
    /// Returns the Client Module through which administration was authorized.
    #[must_use]
    pub const fn client_module(&self) -> &Name {
        self.admission.authorization.client_module()
    }

    /// Returns the authenticated actor for a future accountable workflow.
    #[must_use]
    pub const fn actor(&self) -> StateIdentifier {
        self.admission.actor
    }

    /// Returns the exact authorized action descriptor.
    #[must_use]
    pub const fn action(&self) -> &AdministrationAction {
        &self.action
    }

    /// Consumes an authorized action into the exact account-workflow proof.
    ///
    /// A non-account action is consumed and denied rather than returned for
    /// reuse against another workflow.
    pub fn into_account(
        self,
    ) -> Result<AuthorizedAccountAdministrationAction, AuthorizationDenied> {
        let Self {
            admission, action, ..
        } = self;
        let AdministrationAction::Account(action) = action else {
            return Err(AuthorizationDenied);
        };
        Ok(AuthorizedAccountAdministrationAction {
            actor: admission.actor,
            session: admission.session,
            client_module: admission.authorization.client_module().clone(),
            action,
        })
    }

    /// Consumes an authorized action into the exact Group-mutation proof.
    ///
    /// A non-Group action is consumed and denied rather than returned for
    /// reuse against another workflow.
    pub fn into_group_mutation(self) -> Result<AuthorizedGroupMutation, AuthorizationDenied> {
        let Self {
            admission, action, ..
        } = self;
        let AdministrationAction::GrantMutation(mutation) = action else {
            return Err(AuthorizationDenied);
        };
        Ok(AuthorizedGroupMutation {
            actor: admission.actor,
            session: admission.session,
            client_module: admission.authorization.client_module().clone(),
            mutation,
        })
    }

    /// Consumes an authorized action into the exact MFA-policy proof.
    ///
    /// A non-policy action is consumed and denied rather than returned for
    /// reuse against another workflow.
    pub fn into_mfa_policy(self) -> Result<AuthorizedMfaPolicy, AuthorizationDenied> {
        let Self {
            admission,
            action,
            step_up_factor,
        } = self;
        let AdministrationAction::MfaPolicy = action else {
            return Err(AuthorizationDenied);
        };
        Ok(AuthorizedMfaPolicy {
            actor: admission.actor,
            session: admission.session,
            client_module: admission.authorization.client_module().clone(),
            factor: step_up_factor.ok_or(AuthorizationDenied)?,
        })
    }
}

impl fmt::Debug for AuthorizedAdministrationAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuthorizedAdministrationAction(REDACTED)")
    }
}

/// One authorized MFA policy action bound to its exact step-up factor.
///
/// The value is not clonable and can be obtained only by consuming a matching
/// [`AuthorizedAdministrationAction`].
pub struct AuthorizedMfaPolicy {
    actor: StateIdentifier,
    session: SessionTokenHash,
    client_module: Name,
    factor: StateIdentifier,
}

impl AuthorizedMfaPolicy {
    /// Returns the authenticated actor.
    #[must_use]
    pub const fn actor(&self) -> StateIdentifier {
        self.actor
    }

    /// Returns the exact validated session digest.
    #[must_use]
    pub const fn session(&self) -> SessionTokenHash {
        self.session
    }

    /// Returns the Client Module through which the policy action was authorized.
    #[must_use]
    pub const fn client_module(&self) -> &Name {
        &self.client_module
    }

    /// Returns the exact factor that established current-session step-up.
    #[must_use]
    pub const fn factor(&self) -> StateIdentifier {
        self.factor
    }
}

impl fmt::Debug for AuthorizedMfaPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuthorizedMfaPolicy(REDACTED)")
    }
}

/// One authorized account action bound to its exact validated session.
///
/// The value proves only Administration Plane authorization for the retained
/// action. It does not prove target state or fresh credential reauthentication.
/// It is not clonable and can be obtained only by consuming an
/// [`AuthorizedAdministrationAction`].
pub struct AuthorizedAccountAdministrationAction {
    actor: StateIdentifier,
    session: SessionTokenHash,
    client_module: Name,
    action: AccountAdministrationAction,
}

impl AuthorizedAccountAdministrationAction {
    /// Returns the authenticated actor.
    #[must_use]
    pub const fn actor(&self) -> StateIdentifier {
        self.actor
    }

    /// Returns the exact validated session digest.
    #[must_use]
    pub const fn session(&self) -> SessionTokenHash {
        self.session
    }

    /// Returns the Client Module through which the action was authorized.
    #[must_use]
    pub const fn client_module(&self) -> &Name {
        &self.client_module
    }

    /// Returns the exact authorized account action.
    #[must_use]
    pub const fn action(&self) -> &AccountAdministrationAction {
        &self.action
    }

    /// Consumes the proof and returns the exact authorized account action.
    #[must_use]
    pub fn into_action(self) -> AccountAdministrationAction {
        self.action
    }
}

impl fmt::Debug for AuthorizedAccountAdministrationAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuthorizedAccountAdministrationAction(REDACTED)")
    }
}

/// One authorized Group mutation bound to its exact validated session.
///
/// The value is not clonable and can be obtained only by consuming an
/// [`AuthorizedAdministrationAction`].
pub struct AuthorizedGroupMutation {
    actor: StateIdentifier,
    session: SessionTokenHash,
    client_module: Name,
    mutation: GroupMutation,
}

impl AuthorizedGroupMutation {
    /// Returns the authenticated actor.
    #[must_use]
    pub const fn actor(&self) -> StateIdentifier {
        self.actor
    }

    /// Returns the exact validated session digest.
    #[must_use]
    pub const fn session(&self) -> SessionTokenHash {
        self.session
    }

    /// Returns the Client Module through which the action was authorized.
    #[must_use]
    pub const fn client_module(&self) -> &Name {
        &self.client_module
    }

    /// Returns the exact authorized Group mutation.
    #[must_use]
    pub const fn mutation(&self) -> &GroupMutation {
        &self.mutation
    }

    /// Consumes the proof and returns the exact authorized Group mutation.
    #[must_use]
    pub fn into_mutation(self) -> GroupMutation {
        self.mutation
    }
}

impl fmt::Debug for AuthorizedGroupMutation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuthorizedGroupMutation(REDACTED)")
    }
}

/// Transport-independent gate for Administration Plane actions.
pub struct AdministrationPlane<C, E> {
    clock: C,
    enablement: E,
    components: AvailableComponents,
}

impl<C, E> AdministrationPlane<C, E>
where
    C: AdministrationClock,
    E: ComponentEnablementSource,
{
    /// Composes the gate from a clock, live projection source, and build inventory.
    #[must_use]
    pub const fn new(clock: C, enablement: E, components: AvailableComponents) -> Self {
        Self {
            clock,
            enablement,
            components,
        }
    }

    /// Mints a proof after trusted Server composition verifies current-session MFA.
    ///
    /// The expiry is always derived here from the injected clock; no caller can
    /// supply an issuance time, expiry, boolean, actor, session, or action.
    pub fn issue_step_up(
        &self,
        _authority: &ServerAdministrationAuthority,
        session: &CurrentAdministrationSession,
        family: StepUpActionFamily,
    ) -> Result<MfaStepUpProof, AuthorizationDenied> {
        let issued_at = self.clock.now();
        let expires_at = issued_at
            .checked_add(MFA_STEP_UP_LIFETIME)
            .ok_or(AuthorizationDenied)?;

        Ok(MfaStepUpProof {
            actor: session.actor,
            session: session.session,
            factor: session.factor,
            family,
            issued_at,
            expires_at,
        })
    }

    /// Authorizes one typed Administration Plane action.
    ///
    /// Every denial returns the existing reason-free [`AuthorizationDenied`]:
    /// a missing, mismatched, rolled-back, or expired step-up proof; an
    /// unavailable enablement read; and a disabled target are indistinguishable.
    /// Enablement changes validate inventory membership without reading current
    /// enablement. Account and policy-only actions also perform no such read.
    pub fn authorize(
        &mut self,
        admission: AuthorizedAdministrationAdmission,
        request: AdministrationRequest<'_>,
    ) -> Result<AuthorizedAdministrationAction, AuthorizationDenied> {
        let step_up_factor = match request.action.step_up_family() {
            Some(family) => {
                let proof = request.step_up.ok_or(AuthorizationDenied)?;
                if !proof.permits(&admission, family, self.clock.now()) {
                    return Err(AuthorizationDenied);
                }
                Some(proof.factor)
            }
            None => None,
        };

        if let AdministrationAction::ComponentOperation(target) = &request.action {
            if !target.is_available(&self.components) {
                return Err(AuthorizationDenied);
            }
            let enablement = self.enablement.load_component_enablement()?;
            if !enablement.is_enabled(target.kind(), target.name()) {
                return Err(AuthorizationDenied);
            }
        }

        if let AdministrationAction::ComponentEnablementChange(target) = &request.action
            && !target.is_available(&self.components)
        {
            return Err(AuthorizationDenied);
        }

        if let AdministrationAction::GrantMutation(GroupMutation::Grant(target)) = &request.action
            && !target.is_available(&self.components)
        {
            return Err(AuthorizationDenied);
        }

        Ok(AuthorizedAdministrationAction {
            admission,
            action: request.action,
            step_up_factor,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use weavelit_server_authorization::{
        AdministrationRequest as AuthorizationRequest, AuthorizationCatalog,
        ClientModuleDeclaration, Plane, authorize_administration,
    };
    use weavelit_server_components::MfaFactorFormat;
    use weavelit_server_database::{
        GroupGrant, HumanAuthorizationSnapshot, MAX_NAME_LENGTH, SESSION_DIGEST_LENGTH,
        STATE_IDENTIFIER_LENGTH,
    };

    use super::*;

    const CLIENT_MODULE: &str = "web-ui";

    #[derive(Clone)]
    struct TestClock(Rc<Cell<Duration>>);

    impl TestClock {
        fn new(now: Duration) -> Self {
            Self(Rc::new(Cell::new(now)))
        }

        fn set(&self, now: Duration) {
            self.0.set(now);
        }
    }

    impl AdministrationClock for TestClock {
        fn now(&self) -> Duration {
            self.0.get()
        }
    }

    #[derive(Clone)]
    struct TestEnablement {
        value: Rc<std::cell::RefCell<ComponentEnablement>>,
        reads: Rc<Cell<usize>>,
        unavailable: Rc<Cell<bool>>,
    }

    impl TestEnablement {
        fn new(value: ComponentEnablement) -> Self {
            Self {
                value: Rc::new(std::cell::RefCell::new(value)),
                reads: Rc::new(Cell::new(0)),
                unavailable: Rc::new(Cell::new(false)),
            }
        }

        fn set(&self, value: ComponentEnablement) {
            *self.value.borrow_mut() = value;
        }

        fn reads(&self) -> usize {
            self.reads.get()
        }

        fn fail(&self) {
            self.unavailable.set(true);
        }
    }

    impl ComponentEnablementSource for TestEnablement {
        fn load_component_enablement(
            &mut self,
        ) -> Result<ComponentEnablement, AuthorizationDenied> {
            self.reads.set(self.reads.get() + 1);
            if self.unavailable.get() {
                return Err(AuthorizationDenied);
            }
            Ok(self.value.borrow().clone())
        }
    }

    fn identifier(byte: u8) -> StateIdentifier {
        StateIdentifier::from_bytes([byte; STATE_IDENTIFIER_LENGTH]).unwrap()
    }

    fn setting(key: &str, value: &str) -> LogModuleSetting {
        LogModuleSetting {
            key: weavelit_server_database::ConfigurationKey::new(key).unwrap(),
            value: weavelit_server_database::ConfigurationValue::new(value).unwrap(),
        }
    }

    fn session(byte: u8) -> SessionTokenHash {
        SessionTokenHash::from_bytes([byte; SESSION_DIGEST_LENGTH]).unwrap()
    }

    fn current_session(
        authority: &ServerAdministrationAuthority,
        actor: u8,
        session_byte: u8,
    ) -> CurrentAdministrationSession {
        CurrentAdministrationSession::from_server_authority(
            authority,
            identifier(actor),
            session(session_byte),
            identifier(actor.wrapping_add(20)),
        )
    }

    fn administration_admission(
        authority: &ServerAdministrationAuthority,
        actor: u8,
        session_byte: u8,
    ) -> AuthorizedAdministrationAdmission {
        AuthorizedAdministrationAdmission::from_server_authority(
            authority,
            administration_authorization(),
            identifier(actor),
            session(session_byte),
        )
    }

    fn administration_authorization() -> AuthorizedAdministration {
        let client_module = Name::new(CLIENT_MODULE).unwrap();
        let account = HumanAuthorizationSnapshot::new(
            true,
            vec![
                GroupGrant::ClientModule(client_module.clone()),
                GroupGrant::ServerAdministration,
            ],
        );
        let catalog = AuthorizationCatalog::new(
            vec![ClientModuleDeclaration::new(
                client_module.clone(),
                true,
                &[Plane::Administration],
            )],
            vec![],
            vec![],
        )
        .unwrap();

        authorize_administration(
            &account,
            &catalog,
            AuthorizationRequest {
                client_module: &client_module,
            },
        )
        .unwrap()
    }

    fn plane(
        clock: TestClock,
        enablement: TestEnablement,
    ) -> AdministrationPlane<TestClock, TestEnablement> {
        AdministrationPlane::new(
            clock,
            enablement,
            AvailableComponents {
                client_modules: [Name::new(CLIENT_MODULE).unwrap()].into_iter().collect(),
                mfa_modules: [(
                    Name::new("totp").unwrap(),
                    MfaFactorFormat {
                        factor_data_bytes: 20,
                    },
                )]
                .into_iter()
                .collect(),
                service_modules: [Name::new("zendesk").unwrap()].into_iter().collect(),
                operations: [Name::new("ticket.read").unwrap()].into_iter().collect(),
                ..AvailableComponents::default()
            },
        )
    }

    #[test]
    fn account_actions_need_administration_but_not_step_up() {
        let authority = ServerAdministrationAuthority::new();
        let enablement = TestEnablement::new(ComponentEnablement::default());
        let mut plane = plane(TestClock::new(Duration::ZERO), enablement.clone());

        for account_action in [
            AccountAdministrationAction::Read(AccountAdministrationRead::List),
            AccountAdministrationAction::Read(AccountAdministrationRead::View(
                AccountPublicIdentifier::generate().unwrap(),
            )),
            AccountAdministrationAction::Create(
                AccountCreate::new("new-user", Some("New User")).unwrap(),
            ),
            AccountAdministrationAction::PasswordReset(AccountPasswordReset::new(
                AccountPublicIdentifier::generate().unwrap(),
            )),
            AccountAdministrationAction::StatusChange(AccountStatusChange::new(
                AccountPublicIdentifier::generate().unwrap(),
                AccountStatus::Disabled,
            )),
        ] {
            let action = AdministrationAction::Account(account_action);
            let authorized = plane
                .authorize(
                    administration_admission(&authority, 1, 11),
                    AdministrationRequest::new(action.clone()),
                )
                .unwrap();

            assert_eq!(authorized.actor(), identifier(1));
            assert_eq!(authorized.client_module().as_str(), CLIENT_MODULE);
            assert_eq!(authorized.action(), &action);

            let authorized = authorized.into_account().unwrap();
            assert_eq!(authorized.actor(), identifier(1));
            assert!(authorized.session().matches(&session(11)));
            assert_eq!(authorized.client_module().as_str(), CLIENT_MODULE);
            assert_eq!(
                authorized.action(),
                match &action {
                    AdministrationAction::Account(action) => action,
                    AdministrationAction::MfaPolicy
                    | AdministrationAction::GrantMutation(_)
                    | AdministrationAction::ComponentOperation(_)
                    | AdministrationAction::ComponentEnablementChange(_)
                    | AdministrationAction::LogConfigurationChange(_) => unreachable!(),
                }
            );
            assert_eq!(
                format!("{authorized:?}"),
                "AuthorizedAccountAdministrationAction(REDACTED)"
            );
        }
        assert_eq!(enablement.reads(), 0);
    }

    #[test]
    fn non_account_authorization_is_consumed_without_producing_an_account_proof() {
        let authority = ServerAdministrationAuthority::new();
        let mut plane = plane(
            TestClock::new(Duration::ZERO),
            TestEnablement::new(ComponentEnablement::default()),
        );
        let authorized = plane
            .authorize(
                administration_admission(&authority, 1, 11),
                AdministrationRequest::new(AdministrationAction::LogConfigurationChange(
                    LogConfigurationChange::new(identifier(9), Some(true), None, Vec::new())
                        .unwrap(),
                )),
            )
            .unwrap();

        assert_eq!(authorized.into_account().unwrap_err(), AuthorizationDenied);
    }

    #[test]
    fn account_write_inputs_are_bounded_and_rejection_is_payload_free() {
        let create = AccountCreate::new("new-user", Some("New User")).unwrap();
        assert_eq!(create.username().as_str(), "new-user");
        assert_eq!(create.display_name().unwrap().as_str(), "New User");
        assert!(
            AccountCreate::new("without-display", None::<&str>)
                .unwrap()
                .display_name()
                .is_none()
        );

        let target = AccountPublicIdentifier::generate().unwrap();
        assert_eq!(AccountPasswordReset::new(target).target(), target);
        let status = AccountStatusChange::new(target, AccountStatus::Active);
        assert_eq!(status.target(), target);
        assert_eq!(status.desired(), AccountStatus::Active);

        let oversized = "sensitive".repeat(MAX_NAME_LENGTH + 1);
        for rejected in [
            AccountCreate::new(oversized.clone(), Some("display")).unwrap_err(),
            AccountCreate::new("username", Some(oversized)).unwrap_err(),
        ] {
            assert_eq!(rejected.to_string(), "administration input rejected");
            assert_eq!(format!("{rejected:?}"), "AdministrationInputRejected");
        }
    }

    #[test]
    fn disabled_totp_reenable_needs_no_enablement_read_or_step_up() {
        let authority = ServerAdministrationAuthority::new();
        let enablement = TestEnablement::new(ComponentEnablement::new([(
            ComponentKind::MfaModule,
            Name::new("totp").unwrap(),
        )]));
        enablement.fail();
        let mut plane = plane(TestClock::new(Duration::ZERO), enablement.clone());

        let authorized = plane
            .authorize(
                administration_admission(&authority, 1, 11),
                AdministrationRequest::new(AdministrationAction::ComponentEnablementChange(
                    ComponentEnablementChange::new(ComponentKind::MfaModule, "totp", true).unwrap(),
                )),
            )
            .unwrap();

        let AdministrationAction::ComponentEnablementChange(change) = authorized.action() else {
            panic!("the authorized action must retain the enablement change");
        };
        assert_eq!(change.kind(), ComponentKind::MfaModule);
        assert_eq!(change.name().as_str(), "totp");
        assert!(change.enabled());
        assert_eq!(enablement.reads(), 0);

        assert_eq!(
            plane
                .authorize(
                    administration_admission(&authority, 1, 11),
                    AdministrationRequest::new(AdministrationAction::ComponentOperation(
                        ComponentOperation::new(ComponentKind::MfaModule, "totp").unwrap(),
                    )),
                )
                .unwrap_err(),
            AuthorizationDenied
        );
        assert_eq!(
            enablement.reads(),
            1,
            "ComponentOperation must keep reading enablement and fail closed"
        );
    }

    #[test]
    fn component_enablement_change_retains_each_requested_state() {
        let authority = ServerAdministrationAuthority::new();
        let enablement = TestEnablement::new(ComponentEnablement::default());
        let mut plane = plane(TestClock::new(Duration::ZERO), enablement.clone());

        for enabled in [true, false] {
            let authorized = plane
                .authorize(
                    administration_admission(&authority, 1, 11),
                    AdministrationRequest::new(AdministrationAction::ComponentEnablementChange(
                        ComponentEnablementChange::new(
                            ComponentKind::ServiceModule,
                            "zendesk",
                            enabled,
                        )
                        .unwrap(),
                    )),
                )
                .unwrap();

            let AdministrationAction::ComponentEnablementChange(change) = authorized.action()
            else {
                panic!("the authorized action must retain the enablement change");
            };
            assert_eq!(change.kind(), ComponentKind::ServiceModule);
            assert_eq!(change.name().as_str(), "zendesk");
            assert_eq!(change.enabled(), enabled);
        }

        assert_eq!(enablement.reads(), 0);
    }

    #[test]
    fn log_configuration_change_is_bounded_canonical_and_admitted_without_a_read() {
        assert_eq!(
            LogConfigurationChange::new(identifier(9), None, None, Vec::new()),
            Err(AdministrationInputRejected)
        );
        assert_eq!(
            LogConfigurationChange::new(
                identifier(9),
                None,
                Some(vec![setting("path", "one"), setting("path", "two")]),
                Vec::new(),
            ),
            Err(AdministrationInputRejected)
        );
        assert_eq!(
            LogConfigurationChange::new(
                identifier(9),
                None,
                None,
                vec![
                    LogAssignmentChange::new(LogType::Audit, identifier(9)),
                    LogAssignmentChange::new(LogType::Audit, identifier(8)),
                ],
            ),
            Err(AdministrationInputRejected)
        );

        let change = LogConfigurationChange::new(
            identifier(9),
            Some(false),
            Some(vec![setting("z", "last"), setting("a", "first")]),
            vec![
                LogAssignmentChange::new(LogType::Audit, identifier(8)),
                LogAssignmentChange::new(LogType::System, identifier(9)),
            ],
        )
        .unwrap();
        let authority = ServerAdministrationAuthority::new();
        let enablement = TestEnablement::new(ComponentEnablement::default());
        enablement.fail();
        let mut plane = plane(TestClock::new(Duration::ZERO), enablement.clone());
        let authorized = plane
            .authorize(
                administration_admission(&authority, 1, 11),
                AdministrationRequest::new(AdministrationAction::LogConfigurationChange(change)),
            )
            .unwrap();

        let AdministrationAction::LogConfigurationChange(change) = authorized.action() else {
            panic!("the authorized action must retain the exact Log configuration change");
        };
        assert_eq!(change.primary(), identifier(9));
        assert_eq!(change.enabled(), Some(false));
        assert_eq!(
            change
                .settings()
                .unwrap()
                .iter()
                .map(|setting| setting.key.as_str())
                .collect::<Vec<_>>(),
            ["a", "z"]
        );
        assert_eq!(
            change
                .assignments()
                .iter()
                .map(LogAssignmentChange::log_type)
                .collect::<Vec<_>>(),
            [LogType::System, LogType::Audit]
        );
        assert_eq!(enablement.reads(), 0);
    }

    #[test]
    fn component_enablement_change_denies_unknown_or_wrong_kind_without_a_read() {
        let authority = ServerAdministrationAuthority::new();
        let enablement = TestEnablement::new(ComponentEnablement::default());
        let mut plane = plane(TestClock::new(Duration::ZERO), enablement.clone());

        for (kind, name) in [
            (ComponentKind::MfaModule, "webauthn"),
            (ComponentKind::ServiceModule, "totp"),
        ] {
            assert_eq!(
                plane
                    .authorize(
                        administration_admission(&authority, 1, 11),
                        AdministrationRequest::new(
                            AdministrationAction::ComponentEnablementChange(
                                ComponentEnablementChange::new(kind, name, true).unwrap(),
                            ),
                        ),
                    )
                    .unwrap_err(),
                AuthorizationDenied
            );
        }

        assert_eq!(enablement.reads(), 0);
    }

    #[test]
    fn grant_mutation_requires_matching_step_up_and_retains_exact_intent() {
        let authority = ServerAdministrationAuthority::new();
        let current = current_session(&authority, 1, 11);
        let clock = TestClock::new(Duration::from_secs(10));
        let mut plane = plane(clock, TestEnablement::new(ComponentEnablement::default()));
        let mfa_proof = plane
            .issue_step_up(&authority, &current, StepUpActionFamily::MfaPolicy)
            .unwrap();
        let grant_proof = plane
            .issue_step_up(&authority, &current, StepUpActionFamily::GrantMutation)
            .unwrap();

        assert_eq!(
            plane
                .authorize(
                    administration_admission(&authority, 1, 11),
                    AdministrationRequest::new(AdministrationAction::MfaPolicy),
                )
                .unwrap_err(),
            AuthorizationDenied
        );
        assert_eq!(
            plane
                .authorize(
                    administration_admission(&authority, 1, 11),
                    AdministrationRequest::new(AdministrationAction::GrantMutation(
                        GroupMutation::Grant(GroupGrantMutation::new(
                            identifier(5),
                            GroupGrant::ServerAdministration,
                            false,
                        )),
                    ))
                    .with_step_up(&mfa_proof),
                )
                .unwrap_err(),
            AuthorizationDenied
        );
        assert!(
            plane
                .authorize(
                    administration_admission(&authority, 1, 11),
                    AdministrationRequest::new(AdministrationAction::MfaPolicy)
                        .with_step_up(&mfa_proof),
                )
                .is_ok()
        );
        assert!(
            plane
                .authorize(
                    administration_admission(&authority, 1, 11),
                    AdministrationRequest::new(AdministrationAction::MfaPolicy)
                        .with_step_up(&mfa_proof),
                )
                .is_ok(),
            "one proof remains reusable for its matching family"
        );
        let authorized = plane
            .authorize(
                administration_admission(&authority, 1, 11),
                AdministrationRequest::new(AdministrationAction::GrantMutation(
                    GroupMutation::Membership(GroupMembershipMutation::new(
                        identifier(5),
                        AccountPublicIdentifier::generate().unwrap(),
                        true,
                    )),
                ))
                .with_step_up(&grant_proof),
            )
            .unwrap()
            .into_group_mutation()
            .unwrap();
        assert_eq!(authorized.actor(), identifier(1));
        assert!(authorized.session().matches(&session(11)));
        assert_eq!(authorized.client_module().as_str(), CLIENT_MODULE);
        assert!(matches!(
            authorized.mutation(),
            GroupMutation::Membership(mutation)
                if mutation.group() == identifier(5) && mutation.desired()
        ));
        assert_eq!(
            format!("{authorized:?}"),
            "AuthorizedGroupMutation(REDACTED)"
        );
        assert_eq!(
            plane
                .authorize(
                    administration_admission(&authority, 1, 11),
                    AdministrationRequest::new(AdministrationAction::GrantMutation(
                        GroupMutation::Grant(GroupGrantMutation::new(
                            identifier(5),
                            GroupGrant::Operation(Name::new("missing.operation").unwrap()),
                            true,
                        )),
                    ))
                    .with_step_up(&grant_proof),
                )
                .unwrap_err(),
            AuthorizationDenied
        );
    }

    #[test]
    fn step_up_is_bound_to_actor_session_action_and_fixed_expiry() {
        let authority = ServerAdministrationAuthority::new();
        let current = current_session(&authority, 1, 11);
        let clock = TestClock::new(Duration::from_secs(20));
        let mut plane = plane(
            clock.clone(),
            TestEnablement::new(ComponentEnablement::default()),
        );
        let proof = plane
            .issue_step_up(&authority, &current, StepUpActionFamily::MfaPolicy)
            .unwrap();

        for (actor, session_byte) in [(2, 11), (1, 12)] {
            assert!(
                plane
                    .authorize(
                        administration_admission(&authority, actor, session_byte),
                        AdministrationRequest::new(AdministrationAction::MfaPolicy)
                            .with_step_up(&proof),
                    )
                    .is_err()
            );
        }

        clock.set(Duration::from_secs(19));
        assert!(
            plane
                .authorize(
                    administration_admission(&authority, 1, 11),
                    AdministrationRequest::new(AdministrationAction::MfaPolicy)
                        .with_step_up(&proof),
                )
                .is_err()
        );

        clock.set(Duration::from_secs(20) + MFA_STEP_UP_LIFETIME - Duration::from_nanos(1));
        assert!(
            plane
                .authorize(
                    administration_admission(&authority, 1, 11),
                    AdministrationRequest::new(AdministrationAction::MfaPolicy)
                        .with_step_up(&proof),
                )
                .is_ok(),
            "the proof remains valid until its exclusive expiry"
        );

        clock.set(Duration::from_secs(20) + MFA_STEP_UP_LIFETIME);
        assert!(
            plane
                .authorize(
                    administration_admission(&authority, 1, 11),
                    AdministrationRequest::new(AdministrationAction::MfaPolicy)
                        .with_step_up(&proof),
                )
                .is_err()
        );
    }

    #[test]
    fn component_enablement_is_read_live_for_every_check() {
        let authority = ServerAdministrationAuthority::new();
        let enablement = TestEnablement::new(ComponentEnablement::default());
        let mut plane = plane(TestClock::new(Duration::ZERO), enablement.clone());
        let target = ComponentOperation::new(ComponentKind::Operation, "ticket.read").unwrap();

        let authorized = plane
            .authorize(
                administration_admission(&authority, 1, 11),
                AdministrationRequest::new(AdministrationAction::ComponentOperation(
                    target.clone(),
                )),
            )
            .unwrap();
        assert_eq!(authorized.actor(), identifier(1));
        assert_eq!(authorized.client_module().as_str(), CLIENT_MODULE);

        enablement.set(ComponentEnablement::new([(
            ComponentKind::Operation,
            target.name().clone(),
        )]));
        assert_eq!(
            plane
                .authorize(
                    administration_admission(&authority, 1, 11),
                    AdministrationRequest::new(AdministrationAction::ComponentOperation(target)),
                )
                .unwrap_err(),
            AuthorizationDenied
        );

        let unknown = ComponentOperation::new(ComponentKind::Operation, "ticket.unknown").unwrap();
        assert_eq!(
            plane
                .authorize(
                    administration_admission(&authority, 1, 11),
                    AdministrationRequest::new(AdministrationAction::ComponentOperation(unknown)),
                )
                .unwrap_err(),
            AuthorizationDenied
        );
        assert_eq!(enablement.reads(), 2);
    }

    #[test]
    fn component_inventory_and_enablement_are_scoped_by_kind() {
        let authority = ServerAdministrationAuthority::new();
        let enablement = TestEnablement::new(ComponentEnablement::new([(
            ComponentKind::ClientModule,
            Name::new(CLIENT_MODULE).unwrap(),
        )]));
        let mut plane = AdministrationPlane::new(
            TestClock::new(Duration::ZERO),
            enablement.clone(),
            AvailableComponents {
                client_modules: [Name::new(CLIENT_MODULE).unwrap()].into_iter().collect(),
                service_modules: [Name::new(CLIENT_MODULE).unwrap()].into_iter().collect(),
                ..AvailableComponents::default()
            },
        );

        assert_eq!(
            plane
                .authorize(
                    administration_admission(&authority, 1, 11),
                    AdministrationRequest::new(AdministrationAction::ComponentOperation(
                        ComponentOperation::new(ComponentKind::MfaModule, CLIENT_MODULE).unwrap(),
                    )),
                )
                .unwrap_err(),
            AuthorizationDenied
        );
        assert_eq!(
            enablement.reads(),
            0,
            "an unknown kind denies before the read"
        );

        assert_eq!(
            plane
                .authorize(
                    administration_admission(&authority, 1, 11),
                    AdministrationRequest::new(AdministrationAction::ComponentOperation(
                        ComponentOperation::new(ComponentKind::ClientModule, CLIENT_MODULE)
                            .unwrap(),
                    )),
                )
                .unwrap_err(),
            AuthorizationDenied
        );

        assert!(
            plane
                .authorize(
                    administration_admission(&authority, 1, 11),
                    AdministrationRequest::new(AdministrationAction::ComponentOperation(
                        ComponentOperation::new(ComponentKind::ServiceModule, CLIENT_MODULE)
                            .unwrap(),
                    )),
                )
                .is_ok(),
            "disabling the Client Module must not disable the same Service Module name"
        );
        assert_eq!(enablement.reads(), 2);
    }

    #[test]
    fn mfa_policy_actions_do_not_read_component_enablement() {
        let authority = ServerAdministrationAuthority::new();
        let current = current_session(&authority, 1, 11);
        let enablement = TestEnablement::new(ComponentEnablement::default());
        let mut plane = plane(TestClock::new(Duration::ZERO), enablement.clone());
        let proof = plane
            .issue_step_up(&authority, &current, StepUpActionFamily::MfaPolicy)
            .unwrap();

        assert!(
            plane
                .authorize(
                    administration_admission(&authority, 1, 11),
                    AdministrationRequest::new(AdministrationAction::MfaPolicy)
                        .with_step_up(&proof),
                )
                .is_ok()
        );
        assert_eq!(enablement.reads(), 0);
    }

    #[test]
    fn component_input_is_bounded_and_rejection_is_payload_free() {
        let oversized_name = "sensitive".repeat(MAX_NAME_LENGTH + 1);
        let rejected =
            ComponentOperation::new(ComponentKind::Operation, oversized_name.clone()).unwrap_err();
        let change_rejected =
            ComponentEnablementChange::new(ComponentKind::MfaModule, oversized_name, true)
                .unwrap_err();

        assert_eq!(rejected.to_string(), "administration input rejected");
        assert_eq!(format!("{rejected:?}"), "AdministrationInputRejected");
        assert_eq!(change_rejected, rejected);
    }

    #[test]
    fn denial_rendering_discloses_no_policy_or_enablement_cause() {
        let authority = ServerAdministrationAuthority::new();
        let target = ComponentOperation::new(ComponentKind::MfaModule, "totp").unwrap();
        let mut protected_plane = plane(
            TestClock::new(Duration::ZERO),
            TestEnablement::new(ComponentEnablement::default()),
        );
        let mut disabled_plane = plane(
            TestClock::new(Duration::ZERO),
            TestEnablement::new(ComponentEnablement::new([(
                target.kind(),
                target.name().clone(),
            )])),
        );
        let unavailable_enablement = TestEnablement::new(ComponentEnablement::default());
        unavailable_enablement.fail();
        let mut unavailable_plane = plane(TestClock::new(Duration::ZERO), unavailable_enablement);

        let missing_step_up = protected_plane
            .authorize(
                administration_admission(&authority, 1, 11),
                AdministrationRequest::new(AdministrationAction::MfaPolicy),
            )
            .unwrap_err();
        let disabled = disabled_plane
            .authorize(
                administration_admission(&authority, 1, 11),
                AdministrationRequest::new(AdministrationAction::ComponentOperation(target)),
            )
            .unwrap_err();
        let unavailable = unavailable_plane
            .authorize(
                administration_admission(&authority, 1, 11),
                AdministrationRequest::new(AdministrationAction::ComponentOperation(
                    ComponentOperation::new(ComponentKind::MfaModule, "totp").unwrap(),
                )),
            )
            .unwrap_err();

        assert_eq!(missing_step_up, disabled);
        assert_eq!(disabled, unavailable);
        assert_eq!(missing_step_up.to_string(), "request authorization denied");
        assert_eq!(format!("{missing_step_up:?}"), "AuthorizationDenied");
    }
}
