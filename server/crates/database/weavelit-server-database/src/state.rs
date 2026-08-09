//! Bounded typed application state persisted by an Application Database backend.
//!
//! Active sessions, Log Module destination data, and Log Module authentication
//! or connection credentials are deliberately absent from every type here and
//! therefore cannot enter persisted application state.

use std::fmt;

use crate::{ContractInputError, DeploymentIdentifier, WorkflowKind};

/// Number of bytes in an application-state entity identifier.
pub const STATE_IDENTIFIER_LENGTH: usize = 16;

/// Maximum UTF-8 bytes in a name, username, module, or component value.
pub const MAX_NAME_LENGTH: usize = 256;

/// Maximum UTF-8 bytes in a free-form description.
pub const MAX_DESCRIPTION_LENGTH: usize = 1024;

/// Maximum UTF-8 bytes in a configuration key.
pub const MAX_CONFIGURATION_KEY_LENGTH: usize = 256;

/// Maximum UTF-8 bytes in a configuration value.
pub const MAX_CONFIGURATION_VALUE_LENGTH: usize = 4 * 1024;

/// Maximum bytes in an already-protected opaque payload.
pub const MAX_PROTECTED_VALUE_LENGTH: usize = 64 * 1024;

/// Maximum UTF-8 bytes in an encoded password verifier.
pub const MAX_PASSWORD_VERIFIER_LENGTH: usize = 512;

/// Maximum UTF-8 bytes in an encoded backup recovery public key.
pub const MAX_RECOVERY_PUBLIC_KEY_LENGTH: usize = 128;

/// Maximum UTF-8 bytes in a completion-record classification.
pub const MAX_LOG_CLASSIFICATION_LENGTH: usize = 128;

/// Maximum UTF-8 bytes in a completion-record correlation identifier.
pub const MAX_LOG_CORRELATION_IDENTIFIER_LENGTH: usize = 64;

/// Maximum UTF-8 bytes in a completion-record detail value.
pub const MAX_LOG_DETAIL_LENGTH: usize = 4 * 1024;

/// Opaque identifier of one application-state entity.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StateIdentifier([u8; STATE_IDENTIFIER_LENGTH]);

impl StateIdentifier {
    /// Creates an entity identifier and rejects the reserved all-zero value.
    pub fn from_bytes(bytes: [u8; STATE_IDENTIFIER_LENGTH]) -> Result<Self, ContractInputError> {
        if bytes == [0; STATE_IDENTIFIER_LENGTH] {
            return Err(ContractInputError::InvalidStateIdentifier);
        }

        Ok(Self(bytes))
    }

    /// Returns the identifier's binary representation.
    pub const fn as_bytes(&self) -> &[u8; STATE_IDENTIFIER_LENGTH] {
        &self.0
    }
}

impl fmt::Debug for StateIdentifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("StateIdentifier(REDACTED)")
    }
}

/// Non-empty printable UTF-8 text bounded to `MAX_BYTES` encoded bytes.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BoundedText<const MAX_BYTES: usize>(Box<str>);

impl<const MAX_BYTES: usize> BoundedText<MAX_BYTES> {
    /// Creates bounded text and rejects empty, oversized, or control-character values.
    pub fn new(value: impl Into<Box<str>>) -> Result<Self, ContractInputError> {
        let value = value.into();
        if value.is_empty() {
            return Err(ContractInputError::TextEmpty);
        }
        if value.len() > MAX_BYTES {
            return Err(ContractInputError::TextTooLong);
        }
        if value.chars().any(char::is_control) {
            return Err(ContractInputError::TextNotPrintable);
        }

        Ok(Self(value))
    }

    /// Returns the validated text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<const MAX_BYTES: usize> fmt::Debug for BoundedText<MAX_BYTES> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BoundedText")
            .field("length", &self.0.len())
            .finish_non_exhaustive()
    }
}

/// Bounded name, username, module, or component value.
pub type Name = BoundedText<MAX_NAME_LENGTH>;

/// Bounded free-form description.
pub type Description = BoundedText<MAX_DESCRIPTION_LENGTH>;

/// Bounded configuration key.
pub type ConfigurationKey = BoundedText<MAX_CONFIGURATION_KEY_LENGTH>;

/// Bounded configuration value that never carries a secret.
pub type ConfigurationValue = BoundedText<MAX_CONFIGURATION_VALUE_LENGTH>;

/// Bounded completion-record classification.
pub type LogClassification = BoundedText<MAX_LOG_CLASSIFICATION_LENGTH>;

/// Bounded completion-record correlation identifier.
pub type CorrelationIdentifier = BoundedText<MAX_LOG_CORRELATION_IDENTIFIER_LENGTH>;

/// Bounded completion-record detail value.
pub type LogDetail = BoundedText<MAX_LOG_DETAIL_LENGTH>;

/// Opaque payload already protected by Server-owned at-rest encryption.
///
/// The Application Database stores and returns these bytes exactly. It never
/// derives keys, encrypts, decrypts, interprets, or discloses them.
#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProtectedValue(Box<[u8]>);

impl ProtectedValue {
    /// Creates a bounded protected payload from an already-protected value.
    pub fn new(bytes: impl Into<Box<[u8]>>) -> Result<Self, ContractInputError> {
        let bytes = bytes.into();
        if bytes.is_empty() {
            return Err(ContractInputError::ProtectedValueEmpty);
        }
        if bytes.len() > MAX_PROTECTED_VALUE_LENGTH {
            return Err(ContractInputError::ProtectedValueTooLarge);
        }

        Ok(Self(bytes))
    }

    /// Returns the opaque protected payload.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for ProtectedValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ProtectedValue(REDACTED)")
    }
}

/// Encoded password verifier in the authentication design's PHC string format.
#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct PasswordVerifier(Box<str>);

impl PasswordVerifier {
    /// Creates a verifier and rejects any value outside the encoded PHC shape.
    pub fn new(value: impl Into<Box<str>>) -> Result<Self, ContractInputError> {
        let value = value.into();
        let valid = value.len() <= MAX_PASSWORD_VERIFIER_LENGTH
            && value.starts_with('$')
            && value.is_ascii()
            && !value.chars().any(|character| character.is_ascii_control());
        if !valid {
            return Err(ContractInputError::InvalidPasswordVerifier);
        }

        Ok(Self(value))
    }

    /// Returns the encoded verifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for PasswordVerifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PasswordVerifier(REDACTED)")
    }
}

/// Canonical encoded backup recovery public key retained for future backups.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RecoveryPublicKey(Box<str>);

impl RecoveryPublicKey {
    /// Creates a recovery public key and rejects any non-canonical encoding.
    pub fn new(value: impl Into<Box<str>>) -> Result<Self, ContractInputError> {
        const PREFIX: &str = "age1";

        let value = value.into();
        let valid = value.len() > PREFIX.len()
            && value.len() <= MAX_RECOVERY_PUBLIC_KEY_LENGTH
            && value.starts_with(PREFIX)
            && value
                .chars()
                .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit());
        if !valid {
            return Err(ContractInputError::InvalidRecoveryPublicKey);
        }

        Ok(Self(value))
    }

    /// Returns the canonical encoded public key.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Non-secret configuration value owned by one Server component.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ConfigurationEntry {
    /// Component that owns the setting.
    pub component: Name,
    /// Setting key unique within the component.
    pub key: ConfigurationKey,
    /// Non-secret setting value.
    pub value: ConfigurationValue,
}

/// Already-protected secret value owned by one Server component.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProtectedSecret {
    /// Component that owns the secret.
    pub component: Name,
    /// Secret key unique within the component.
    pub key: ConfigurationKey,
    /// Opaque already-protected payload.
    pub value: ProtectedValue,
}

/// Local Human User account record.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Account {
    /// Opaque account identifier.
    pub identifier: StateIdentifier,
    /// Unique local username.
    pub username: Name,
    /// Optional display name.
    pub display_name: Option<Name>,
    /// Whether the account may authenticate.
    pub active: bool,
}

/// Password verifier bound to exactly one account.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct AccountPasswordVerifier {
    /// Account that owns the verifier.
    pub account: StateIdentifier,
    /// Encoded verifier.
    pub verifier: PasswordVerifier,
}

/// Group record that carries every Human User grant.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Group {
    /// Opaque Group identifier.
    pub identifier: StateIdentifier,
    /// Unique Group name.
    pub name: Name,
    /// Optional Group description.
    pub description: Option<Description>,
}

/// Membership of one account in one Group.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct GroupMembership {
    /// Group that grants membership.
    pub group: StateIdentifier,
    /// Member account.
    pub account: StateIdentifier,
}

/// Grant that a Group confers on its members.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum GroupGrant {
    /// Access to one Client Module.
    ClientModule(Name),
    /// Access to one Service Module.
    ServiceModule(Name),
    /// Access to one named Operation.
    Operation(Name),
    /// The Server Administration Permission.
    ServerAdministration,
}

/// Grant bound to one Group.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct GroupGrantRecord {
    /// Group that confers the grant.
    pub group: StateIdentifier,
    /// Conferred grant.
    pub grant: GroupGrant,
}

/// Enrolled MFA factor whose module-owned data is already protected.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MfaFactor {
    /// Opaque factor identifier.
    pub identifier: StateIdentifier,
    /// Account that owns the factor.
    pub account: StateIdentifier,
    /// MFA Module that owns the factor encoding.
    pub module: Name,
    /// Opaque already-protected factor data.
    pub protected_factor_data: ProtectedValue,
}

/// Service Connection whose provider credential is already protected.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ServiceConnection {
    /// Opaque connection identifier.
    pub identifier: StateIdentifier,
    /// Service Module that owns the connection type.
    pub service_module: Name,
    /// Connection name unique within the Service Module.
    pub name: Name,
    /// Opaque already-protected provider credential.
    pub protected_credential: ProtectedValue,
}

/// Non-secret Log Module setting.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct LogModuleSetting {
    /// Setting key unique within the configuration.
    pub key: ConfigurationKey,
    /// Non-secret setting value.
    pub value: ConfigurationValue,
}

/// Configured Log Module with only its non-secret settings and enabled state.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct LogModuleConfiguration {
    /// Opaque configuration identifier.
    pub identifier: StateIdentifier,
    /// Log Module that owns the configuration.
    pub module: Name,
    /// Unique configuration name.
    pub name: Name,
    /// Whether the configuration is enabled.
    pub enabled: bool,
    /// Non-secret settings; authentication and connection credentials are excluded.
    pub settings: Vec<LogModuleSetting>,
}

/// Log type that a configured Log Module receives.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum LogType {
    /// System Logs.
    System,
    /// Audit Logs.
    Audit,
}

impl LogType {
    /// Every log type that requires exactly one assignment.
    pub const ALL: [Self; 2] = [Self::System, Self::Audit];
}

/// Assignment of one configured Log Module to one log type.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct LogAssignment {
    /// Assigned log type.
    pub log_type: LogType,
    /// Configured Log Module that receives the log type.
    pub configuration: StateIdentifier,
}

/// Post-commit System Log completion obligation carried by committed state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionObligation {
    record_identifier: StateIdentifier,
    workflow: WorkflowKind,
    classification: LogClassification,
    correlation_identifier: CorrelationIdentifier,
    event_time_milliseconds: i64,
    detail: LogDetail,
}

impl CompletionObligation {
    /// Creates an obligation from non-secret completion-record fields.
    pub fn new(
        record_identifier: StateIdentifier,
        workflow: WorkflowKind,
        classification: LogClassification,
        correlation_identifier: CorrelationIdentifier,
        event_time_milliseconds: i64,
        detail: LogDetail,
    ) -> Result<Self, ContractInputError> {
        if event_time_milliseconds < 0 {
            return Err(ContractInputError::InvalidEventTime);
        }

        Ok(Self {
            record_identifier,
            workflow,
            classification,
            correlation_identifier,
            event_time_milliseconds,
            detail,
        })
    }

    /// Returns the opaque completion-record identifier.
    pub const fn record_identifier(&self) -> StateIdentifier {
        self.record_identifier
    }

    /// Returns the workflow that owns the obligation.
    pub const fn workflow(&self) -> WorkflowKind {
        self.workflow
    }

    /// Returns the completion-record classification.
    pub const fn classification(&self) -> &LogClassification {
        &self.classification
    }

    /// Returns the completion-record correlation identifier.
    pub const fn correlation_identifier(&self) -> &CorrelationIdentifier {
        &self.correlation_identifier
    }

    /// Returns the completion-record event time in UTC Unix milliseconds.
    pub const fn event_time_milliseconds(&self) -> i64 {
        self.event_time_milliseconds
    }

    /// Returns the completion-record detail.
    pub const fn detail(&self) -> &LogDetail {
        &self.detail
    }
}

/// Caller-supplied collections validated into an [`ApplicationState`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationStateInput {
    /// Non-secret component configuration.
    pub configuration: Vec<ConfigurationEntry>,
    /// Already-protected component secrets.
    pub protected_secrets: Vec<ProtectedSecret>,
    /// Local accounts.
    pub accounts: Vec<Account>,
    /// Password verifiers.
    pub password_verifiers: Vec<AccountPasswordVerifier>,
    /// Groups.
    pub groups: Vec<Group>,
    /// Group memberships.
    pub group_memberships: Vec<GroupMembership>,
    /// Group grants.
    pub group_grants: Vec<GroupGrantRecord>,
    /// Enrolled MFA factors.
    pub mfa_factors: Vec<MfaFactor>,
    /// Service Connections.
    pub service_connections: Vec<ServiceConnection>,
    /// Retained backup recovery public key.
    pub recovery_public_key: RecoveryPublicKey,
    /// Configured Log Modules.
    pub log_module_configurations: Vec<LogModuleConfiguration>,
    /// Log type assignments.
    pub log_assignments: Vec<LogAssignment>,
    /// Post-commit completion obligation.
    pub completion_obligation: CompletionObligation,
}

/// Complete deployment-bound application state written or read as one unit.
///
/// This aggregate deliberately has no session, Log Module record, or Log Module
/// credential member, so restored or initialized state cannot carry them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationState {
    configuration: Vec<ConfigurationEntry>,
    protected_secrets: Vec<ProtectedSecret>,
    accounts: Vec<Account>,
    password_verifiers: Vec<AccountPasswordVerifier>,
    groups: Vec<Group>,
    group_memberships: Vec<GroupMembership>,
    group_grants: Vec<GroupGrantRecord>,
    mfa_factors: Vec<MfaFactor>,
    service_connections: Vec<ServiceConnection>,
    recovery_public_key: RecoveryPublicKey,
    log_module_configurations: Vec<LogModuleConfiguration>,
    log_assignments: Vec<LogAssignment>,
    completion_obligation: CompletionObligation,
}

impl ApplicationState {
    /// Validates uniqueness, references, and assignments, then orders the state.
    pub fn new(input: ApplicationStateInput) -> Result<Self, ContractInputError> {
        let ApplicationStateInput {
            mut configuration,
            mut protected_secrets,
            mut accounts,
            mut password_verifiers,
            mut groups,
            mut group_memberships,
            mut group_grants,
            mut mfa_factors,
            mut service_connections,
            recovery_public_key,
            mut log_module_configurations,
            mut log_assignments,
            completion_obligation,
        } = input;

        configuration.sort();
        protected_secrets.sort();
        accounts.sort();
        password_verifiers.sort();
        groups.sort();
        group_memberships.sort();
        group_grants.sort();
        mfa_factors.sort();
        service_connections.sort();
        for log_module_configuration in &mut log_module_configurations {
            log_module_configuration.settings.sort();
        }
        log_module_configurations.sort();
        log_assignments.sort();

        reject_adjacent_duplicates(&configuration, |left, right| {
            left.component == right.component && left.key == right.key
        })?;
        reject_adjacent_duplicates(&protected_secrets, |left, right| {
            left.component == right.component && left.key == right.key
        })?;
        reject_adjacent_duplicates(&accounts, |left, right| left.identifier == right.identifier)?;
        reject_duplicate_keys(&accounts, |account| account.username.clone())?;
        reject_adjacent_duplicates(&password_verifiers, |left, right| {
            left.account == right.account
        })?;
        reject_adjacent_duplicates(&groups, |left, right| left.identifier == right.identifier)?;
        reject_duplicate_keys(&groups, |group| group.name.clone())?;
        reject_adjacent_duplicates(&group_memberships, |left, right| left == right)?;
        reject_adjacent_duplicates(&group_grants, |left, right| left == right)?;
        reject_adjacent_duplicates(&mfa_factors, |left, right| {
            left.identifier == right.identifier
        })?;
        reject_duplicate_keys(&mfa_factors, |factor| {
            (factor.account, factor.module.clone())
        })?;
        reject_adjacent_duplicates(&service_connections, |left, right| {
            left.identifier == right.identifier
        })?;
        reject_duplicate_keys(&service_connections, |connection| {
            (connection.service_module.clone(), connection.name.clone())
        })?;
        reject_adjacent_duplicates(&log_module_configurations, |left, right| {
            left.identifier == right.identifier
        })?;
        reject_duplicate_keys(&log_module_configurations, |entry| entry.name.clone())?;
        for log_module_configuration in &log_module_configurations {
            reject_adjacent_duplicates(&log_module_configuration.settings, |left, right| {
                left.key == right.key
            })?;
        }

        let account_identifiers = sorted_identifiers(&accounts, |account| account.identifier);
        let group_identifiers = sorted_identifiers(&groups, |group| group.identifier);
        let log_module_identifiers =
            sorted_identifiers(&log_module_configurations, |entry| entry.identifier);

        for verifier in &password_verifiers {
            require_reference(&account_identifiers, verifier.account)?;
        }
        for membership in &group_memberships {
            require_reference(&group_identifiers, membership.group)?;
            require_reference(&account_identifiers, membership.account)?;
        }
        for grant in &group_grants {
            require_reference(&group_identifiers, grant.group)?;
        }
        for factor in &mfa_factors {
            require_reference(&account_identifiers, factor.account)?;
        }

        for log_type in LogType::ALL {
            let assignments = log_assignments
                .iter()
                .filter(|assignment| assignment.log_type == log_type)
                .collect::<Vec<_>>();
            let [assignment] = assignments.as_slice() else {
                return Err(if assignments.is_empty() {
                    ContractInputError::MissingAssignment
                } else {
                    ContractInputError::DuplicateEntry
                });
            };
            require_reference(&log_module_identifiers, assignment.configuration)?;
            let enabled = log_module_configurations
                .iter()
                .any(|entry| entry.identifier == assignment.configuration && entry.enabled);
            if !enabled {
                return Err(ContractInputError::DisabledAssignment);
            }
        }

        Ok(Self {
            configuration,
            protected_secrets,
            accounts,
            password_verifiers,
            groups,
            group_memberships,
            group_grants,
            mfa_factors,
            service_connections,
            recovery_public_key,
            log_module_configurations,
            log_assignments,
            completion_obligation,
        })
    }

    /// Returns the non-secret component configuration.
    pub fn configuration(&self) -> &[ConfigurationEntry] {
        &self.configuration
    }

    /// Returns the already-protected component secrets.
    pub fn protected_secrets(&self) -> &[ProtectedSecret] {
        &self.protected_secrets
    }

    /// Returns the local accounts.
    pub fn accounts(&self) -> &[Account] {
        &self.accounts
    }

    /// Returns the password verifiers.
    pub fn password_verifiers(&self) -> &[AccountPasswordVerifier] {
        &self.password_verifiers
    }

    /// Returns the Groups.
    pub fn groups(&self) -> &[Group] {
        &self.groups
    }

    /// Returns the Group memberships.
    pub fn group_memberships(&self) -> &[GroupMembership] {
        &self.group_memberships
    }

    /// Returns the Group grants.
    pub fn group_grants(&self) -> &[GroupGrantRecord] {
        &self.group_grants
    }

    /// Returns the enrolled MFA factors.
    pub fn mfa_factors(&self) -> &[MfaFactor] {
        &self.mfa_factors
    }

    /// Returns the Service Connections.
    pub fn service_connections(&self) -> &[ServiceConnection] {
        &self.service_connections
    }

    /// Returns the retained backup recovery public key.
    pub const fn recovery_public_key(&self) -> &RecoveryPublicKey {
        &self.recovery_public_key
    }

    /// Returns the configured Log Modules.
    pub fn log_module_configurations(&self) -> &[LogModuleConfiguration] {
        &self.log_module_configurations
    }

    /// Returns the log type assignments.
    pub fn log_assignments(&self) -> &[LogAssignment] {
        &self.log_assignments
    }

    /// Returns the post-commit completion obligation.
    pub const fn completion_obligation(&self) -> &CompletionObligation {
        &self.completion_obligation
    }
}

/// Initialized application state loaded for one Server deployment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InitializedState {
    deployment_identifier: DeploymentIdentifier,
    state: ApplicationState,
    completion_acknowledged: bool,
}

impl InitializedState {
    /// Creates a loaded initialized-state result.
    pub const fn new(
        deployment_identifier: DeploymentIdentifier,
        state: ApplicationState,
        completion_acknowledged: bool,
    ) -> Self {
        Self {
            deployment_identifier,
            state,
            completion_acknowledged,
        }
    }

    /// Returns the deployment identifier bound to the loaded state.
    pub const fn deployment_identifier(&self) -> DeploymentIdentifier {
        self.deployment_identifier
    }

    /// Returns the loaded application state.
    pub const fn state(&self) -> &ApplicationState {
        &self.state
    }

    /// Returns whether the completion obligation is already acknowledged.
    pub const fn completion_acknowledged(&self) -> bool {
        self.completion_acknowledged
    }
}

fn reject_adjacent_duplicates<T>(
    items: &[T],
    conflicts: impl Fn(&T, &T) -> bool,
) -> Result<(), ContractInputError> {
    for pair in items.windows(2) {
        if conflicts(&pair[0], &pair[1]) {
            return Err(ContractInputError::DuplicateEntry);
        }
    }

    Ok(())
}

fn reject_duplicate_keys<T, K: Ord>(
    items: &[T],
    key: impl Fn(&T) -> K,
) -> Result<(), ContractInputError> {
    let mut keys = items.iter().map(key).collect::<Vec<_>>();
    keys.sort_unstable();
    reject_adjacent_duplicates(&keys, |left, right| left == right)
}

fn sorted_identifiers<T>(
    items: &[T],
    identifier: impl Fn(&T) -> StateIdentifier,
) -> Vec<StateIdentifier> {
    let mut identifiers = items.iter().map(identifier).collect::<Vec<_>>();
    identifiers.sort_unstable();
    identifiers
}

fn require_reference(
    identifiers: &[StateIdentifier],
    identifier: StateIdentifier,
) -> Result<(), ContractInputError> {
    identifiers
        .binary_search(&identifier)
        .map(|_| ())
        .map_err(|_| ContractInputError::UnknownReference)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROTECTED_BYTES: &[u8] = b"protected-payload-bytes";
    const VERIFIER: &str = "$argon2id$v=19$m=65536,t=3,p=1$c2FsdHNhbHRzYWx0$dmVyaWZpZXI";
    const RECOVERY_KEY: &str = "age1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqsm5xurc";

    fn identifier(byte: u8) -> StateIdentifier {
        StateIdentifier::from_bytes([byte; STATE_IDENTIFIER_LENGTH]).unwrap()
    }

    fn name(value: &str) -> Name {
        Name::new(value).unwrap()
    }

    fn protected() -> ProtectedValue {
        ProtectedValue::new(PROTECTED_BYTES).unwrap()
    }

    fn obligation() -> CompletionObligation {
        CompletionObligation::new(
            identifier(0xF0),
            WorkflowKind::Restore,
            LogClassification::new("lifecycle.restore").unwrap(),
            CorrelationIdentifier::new("correlation").unwrap(),
            1,
            LogDetail::new("restore completed").unwrap(),
        )
        .unwrap()
    }

    fn valid_input() -> ApplicationStateInput {
        ApplicationStateInput {
            configuration: vec![ConfigurationEntry {
                component: name("mfa.totp"),
                key: ConfigurationKey::new("enabled").unwrap(),
                value: ConfigurationValue::new("false").unwrap(),
            }],
            protected_secrets: vec![ProtectedSecret {
                component: name("mfa.totp"),
                key: ConfigurationKey::new("module-secret").unwrap(),
                value: protected(),
            }],
            accounts: vec![Account {
                identifier: identifier(1),
                username: name("first-admin"),
                display_name: Some(name("First Admin")),
                active: true,
            }],
            password_verifiers: vec![AccountPasswordVerifier {
                account: identifier(1),
                verifier: PasswordVerifier::new(VERIFIER).unwrap(),
            }],
            groups: vec![Group {
                identifier: identifier(2),
                name: name("Administrators"),
                description: None,
            }],
            group_memberships: vec![GroupMembership {
                group: identifier(2),
                account: identifier(1),
            }],
            group_grants: vec![
                GroupGrantRecord {
                    group: identifier(2),
                    grant: GroupGrant::ServerAdministration,
                },
                GroupGrantRecord {
                    group: identifier(2),
                    grant: GroupGrant::ClientModule(name("web-ui")),
                },
            ],
            mfa_factors: vec![MfaFactor {
                identifier: identifier(3),
                account: identifier(1),
                module: name("totp"),
                protected_factor_data: protected(),
            }],
            service_connections: vec![ServiceConnection {
                identifier: identifier(4),
                service_module: name("zendesk"),
                name: name("primary"),
                protected_credential: protected(),
            }],
            recovery_public_key: RecoveryPublicKey::new(RECOVERY_KEY).unwrap(),
            log_module_configurations: vec![LogModuleConfiguration {
                identifier: identifier(5),
                module: name("log-sqlite"),
                name: name("local"),
                enabled: true,
                settings: vec![LogModuleSetting {
                    key: ConfigurationKey::new("retention").unwrap(),
                    value: ConfigurationValue::new("unsupported").unwrap(),
                }],
            }],
            log_assignments: vec![
                LogAssignment {
                    log_type: LogType::System,
                    configuration: identifier(5),
                },
                LogAssignment {
                    log_type: LogType::Audit,
                    configuration: identifier(5),
                },
            ],
            completion_obligation: obligation(),
        }
    }

    fn reject(input: ApplicationStateInput) -> ContractInputError {
        ApplicationState::new(input).expect_err("invalid application state must be rejected")
    }

    #[test]
    fn state_identifier_rejects_reserved_zero_value() {
        assert_eq!(
            StateIdentifier::from_bytes([0; STATE_IDENTIFIER_LENGTH]),
            Err(ContractInputError::InvalidStateIdentifier)
        );
    }

    #[test]
    fn bounded_text_enforces_emptiness_length_and_printability() {
        assert_eq!(Name::new(""), Err(ContractInputError::TextEmpty));
        assert_eq!(
            Name::new("a".repeat(MAX_NAME_LENGTH + 1)),
            Err(ContractInputError::TextTooLong)
        );
        assert_eq!(
            Name::new("line\nbreak"),
            Err(ContractInputError::TextNotPrintable)
        );
        assert_eq!(
            Name::new("a".repeat(MAX_NAME_LENGTH))
                .unwrap()
                .as_str()
                .len(),
            MAX_NAME_LENGTH
        );
    }

    #[test]
    fn protected_value_enforces_bounds_and_preserves_bytes() {
        assert_eq!(
            ProtectedValue::new([]),
            Err(ContractInputError::ProtectedValueEmpty)
        );
        assert_eq!(
            ProtectedValue::new(vec![0; MAX_PROTECTED_VALUE_LENGTH + 1]),
            Err(ContractInputError::ProtectedValueTooLarge)
        );
        assert_eq!(protected().as_bytes(), PROTECTED_BYTES);
    }

    #[test]
    fn password_verifier_requires_bounded_phc_encoding() {
        for candidate in ["", "argon2id$missing-prefix", "$verifier\u{7f}"] {
            assert_eq!(
                PasswordVerifier::new(candidate),
                Err(ContractInputError::InvalidPasswordVerifier)
            );
        }
        assert_eq!(
            PasswordVerifier::new(format!("${}", "a".repeat(MAX_PASSWORD_VERIFIER_LENGTH))),
            Err(ContractInputError::InvalidPasswordVerifier)
        );
        assert_eq!(PasswordVerifier::new(VERIFIER).unwrap().as_str(), VERIFIER);
    }

    #[test]
    fn recovery_public_key_requires_canonical_encoding() {
        for candidate in ["", "age1", "AGE1QQQQ", "age1 qqqq", "age1\u{00e9}"] {
            assert_eq!(
                RecoveryPublicKey::new(candidate),
                Err(ContractInputError::InvalidRecoveryPublicKey)
            );
        }
        assert_eq!(
            RecoveryPublicKey::new(RECOVERY_KEY).unwrap().as_str(),
            RECOVERY_KEY
        );
    }

    #[test]
    fn completion_obligation_rejects_negative_event_time() {
        let error = CompletionObligation::new(
            identifier(6),
            WorkflowKind::Init,
            LogClassification::new("lifecycle.init").unwrap(),
            CorrelationIdentifier::new("correlation").unwrap(),
            -1,
            LogDetail::new("detail").unwrap(),
        )
        .expect_err("a negative event time must be rejected");

        assert_eq!(error, ContractInputError::InvalidEventTime);
    }

    #[test]
    fn valid_state_is_accepted_and_ordered_independently_of_input_order() {
        let ordered = ApplicationState::new(valid_input()).unwrap();
        let mut reversed = valid_input();
        reversed.group_grants.reverse();
        reversed.log_assignments.reverse();
        reversed.accounts.reverse();

        assert_eq!(ApplicationState::new(reversed).unwrap(), ordered);
        assert_eq!(ordered.accounts().len(), 1);
        assert_eq!(ordered.group_grants().len(), 2);
        assert_eq!(
            ordered.group_grants()[0].grant,
            GroupGrant::ClientModule(name("web-ui"))
        );
        assert_eq!(ordered.recovery_public_key().as_str(), RECOVERY_KEY);
        assert_eq!(
            ordered.completion_obligation().workflow(),
            WorkflowKind::Restore
        );
    }

    #[test]
    fn duplicate_identifiers_and_unique_keys_are_rejected() {
        let mut duplicate_account_identifier = valid_input();
        duplicate_account_identifier.accounts.push(Account {
            identifier: identifier(1),
            username: name("second-admin"),
            display_name: None,
            active: true,
        });

        let mut duplicate_username = valid_input();
        duplicate_username.accounts.push(Account {
            identifier: identifier(7),
            username: name("first-admin"),
            display_name: None,
            active: true,
        });

        let mut duplicate_group_name = valid_input();
        duplicate_group_name.groups.push(Group {
            identifier: identifier(8),
            name: name("Administrators"),
            description: None,
        });

        let mut duplicate_configuration = valid_input();
        duplicate_configuration
            .configuration
            .push(ConfigurationEntry {
                component: name("mfa.totp"),
                key: ConfigurationKey::new("enabled").unwrap(),
                value: ConfigurationValue::new("true").unwrap(),
            });

        let mut duplicate_factor = valid_input();
        duplicate_factor.mfa_factors.push(MfaFactor {
            identifier: identifier(9),
            account: identifier(1),
            module: name("totp"),
            protected_factor_data: protected(),
        });

        let mut duplicate_connection = valid_input();
        duplicate_connection
            .service_connections
            .push(ServiceConnection {
                identifier: identifier(10),
                service_module: name("zendesk"),
                name: name("primary"),
                protected_credential: protected(),
            });

        let mut duplicate_setting = valid_input();
        duplicate_setting.log_module_configurations[0]
            .settings
            .push(LogModuleSetting {
                key: ConfigurationKey::new("retention").unwrap(),
                value: ConfigurationValue::new("other").unwrap(),
            });

        let mut duplicate_verifier = valid_input();
        duplicate_verifier
            .password_verifiers
            .push(AccountPasswordVerifier {
                account: identifier(1),
                verifier: PasswordVerifier::new(VERIFIER).unwrap(),
            });

        for input in [
            duplicate_account_identifier,
            duplicate_username,
            duplicate_group_name,
            duplicate_configuration,
            duplicate_factor,
            duplicate_connection,
            duplicate_setting,
            duplicate_verifier,
        ] {
            assert_eq!(reject(input), ContractInputError::DuplicateEntry);
        }
    }

    #[test]
    fn unknown_references_are_rejected() {
        let mut unknown_verifier_account = valid_input();
        unknown_verifier_account.password_verifiers[0].account = identifier(0x20);

        let mut unknown_membership_group = valid_input();
        unknown_membership_group.group_memberships[0].group = identifier(0x21);

        let mut unknown_membership_account = valid_input();
        unknown_membership_account.group_memberships[0].account = identifier(0x22);

        let mut unknown_grant_group = valid_input();
        unknown_grant_group.group_grants[0].group = identifier(0x23);

        let mut unknown_factor_account = valid_input();
        unknown_factor_account.mfa_factors[0].account = identifier(0x24);

        let mut unknown_assignment = valid_input();
        unknown_assignment.log_assignments[0].configuration = identifier(0x25);

        for input in [
            unknown_verifier_account,
            unknown_membership_group,
            unknown_membership_account,
            unknown_grant_group,
            unknown_factor_account,
            unknown_assignment,
        ] {
            assert_eq!(reject(input), ContractInputError::UnknownReference);
        }
    }

    #[test]
    fn log_assignments_require_exactly_one_enabled_module_per_log_type() {
        let mut missing = valid_input();
        missing
            .log_assignments
            .retain(|assignment| assignment.log_type == LogType::Audit);

        let mut duplicated = valid_input();
        duplicated
            .log_module_configurations
            .push(LogModuleConfiguration {
                identifier: identifier(0x30),
                module: name("log-sqlite"),
                name: name("secondary"),
                enabled: true,
                settings: Vec::new(),
            });
        duplicated.log_assignments.push(LogAssignment {
            log_type: LogType::System,
            configuration: identifier(0x30),
        });

        let mut disabled = valid_input();
        disabled.log_module_configurations[0].enabled = false;

        assert_eq!(reject(missing), ContractInputError::MissingAssignment);
        assert_eq!(reject(duplicated), ContractInputError::DuplicateEntry);
        assert_eq!(reject(disabled), ContractInputError::DisabledAssignment);
    }

    #[test]
    fn state_debug_output_redacts_protected_values_and_identities() {
        let state = ApplicationState::new(valid_input()).unwrap();
        let output = format!("{state:?}");

        assert!(!output.contains("protected-payload-bytes"));
        assert!(!output.contains(VERIFIER));
        assert!(!output.contains("first-admin"));
        assert!(!output.contains("Administrators"));
        assert!(!output.contains("session"));
    }
}
