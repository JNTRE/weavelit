use std::{collections::BTreeSet, fmt};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use weavelit_server_components::AvailableComponents;
use weavelit_server_database::{
    Account, AccountPasswordVerifier, ConfigurationEntry, ConfigurationKey, Group, GroupGrant,
    GroupGrantRecord, GroupMembership, LogAssignment, LogModuleConfiguration, LogModuleSetting,
    LogType, MAX_PROTECTED_VALUE_LENGTH, Name, PasswordVerifier, RecoveryPublicKey,
    STATE_IDENTIFIER_LENGTH, StateIdentifier,
};
use weavelit_server_lifecycle::{BackendIdentifier, MAX_PROTECTED_PLAINTEXT_BYTES};
use zeroize::Zeroizing;

use crate::ContentError;

/// The only inner backup content format version this Server accepts.
pub const BACKUP_CONTENT_FORMAT_VERSION: u32 = 1;

/// Maximum entries accepted in one backup collection.
pub const MAX_COLLECTION_ENTRIES: usize = 100_000;

/// Maximum non-secret settings accepted for one Log Module configuration.
pub const MAX_LOG_MODULE_SETTINGS: usize = 256;

/// Maximum bytes accepted in one decrypted protected value.
///
/// This is the plaintext this Server can seal, not the larger bound on a stored
/// protected value. Authenticated encryption and envelope encoding expand a
/// sealed value, so accepting a plaintext up to the stored bound would admit
/// secrets that could never be written back and would fail mid-restore instead
/// of during validation. A backup produced by any Weavelit Server is subject to
/// the same bound at its source.
pub const MAX_SENSITIVE_VALUE_BYTES: usize = MAX_PROTECTED_PLAINTEXT_BYTES;

const _: () = assert!(MAX_SENSITIVE_VALUE_BYTES < MAX_PROTECTED_VALUE_LENGTH);

/// Decrypted application secret held only in bounded transient memory.
///
/// The value is cleared on drop and never enters a display, debug, log, or
/// client-visible representation.
#[derive(Clone, Eq, PartialEq)]
pub struct SensitiveBytes(Zeroizing<Vec<u8>>);

impl SensitiveBytes {
    /// Creates a bounded decrypted secret.
    pub fn new(bytes: Vec<u8>) -> Result<Self, ContentError> {
        if bytes.is_empty() || bytes.len() > MAX_SENSITIVE_VALUE_BYTES {
            return Err(ContentError::DomainInvalid);
        }
        Ok(Self(Zeroizing::new(bytes)))
    }

    /// Returns the decrypted secret for re-encryption under the replacement key.
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for SensitiveBytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SensitiveBytes(REDACTED)")
    }
}

impl Ord for SensitiveBytes {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.as_slice().cmp(other.0.as_slice())
    }
}

impl PartialOrd for SensitiveBytes {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Component secret whose value is decrypted but not yet re-encrypted.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct BackupProtectedSecret {
    /// Component that owns the secret.
    pub component: Name,
    /// Secret key unique within the component.
    pub key: ConfigurationKey,
    /// Decrypted secret payload.
    pub value: SensitiveBytes,
}

/// Enrolled MFA factor whose module-owned data is decrypted.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct BackupMfaFactor {
    /// Opaque factor identifier.
    pub identifier: StateIdentifier,
    /// Account that owns the factor.
    pub account: StateIdentifier,
    /// MFA Module that owns the factor encoding.
    pub module: Name,
    /// Decrypted factor data.
    pub factor_data: SensitiveBytes,
}

/// Service Connection whose provider credential is decrypted.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct BackupServiceConnection {
    /// Opaque connection identifier.
    pub identifier: StateIdentifier,
    /// Service Module that owns the connection type.
    pub service_module: Name,
    /// Connection name unique within the Service Module.
    pub name: Name,
    /// Decrypted provider credential.
    pub credential: SensitiveBytes,
}

/// Validated, normalized, deployment-neutral representation of a backup.
///
/// Its protected values are decrypted and still awaiting re-encryption under
/// the replacement Server's at-rest key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NormalizedBackup {
    source_backend: BackendIdentifier,
    configuration: Vec<ConfigurationEntry>,
    protected_secrets: Vec<BackupProtectedSecret>,
    accounts: Vec<Account>,
    password_verifiers: Vec<AccountPasswordVerifier>,
    groups: Vec<Group>,
    group_memberships: Vec<GroupMembership>,
    group_grants: Vec<GroupGrantRecord>,
    mfa_factors: Vec<BackupMfaFactor>,
    service_connections: Vec<BackupServiceConnection>,
    recovery_public_key: RecoveryPublicKey,
    log_module_configurations: Vec<LogModuleConfiguration>,
    log_assignments: Vec<LogAssignment>,
}

impl NormalizedBackup {
    /// Returns the backup's source Application Database backend.
    pub const fn source_backend(&self) -> &BackendIdentifier {
        &self.source_backend
    }

    /// Returns the non-secret component configuration.
    pub fn configuration(&self) -> &[ConfigurationEntry] {
        &self.configuration
    }

    /// Returns the decrypted component secrets.
    pub fn protected_secrets(&self) -> &[BackupProtectedSecret] {
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
    pub fn mfa_factors(&self) -> &[BackupMfaFactor] {
        &self.mfa_factors
    }

    /// Returns the Service Connections.
    pub fn service_connections(&self) -> &[BackupServiceConnection] {
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
}

// ---------------------------------------------------------------------------
// Version 1 wire model
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BackupDocumentV1 {
    format_version: u32,
    source_backend: String,
    recovery_public_key: String,
    configuration: Vec<ConfigurationEntryV1>,
    protected_secrets: Vec<ProtectedSecretV1>,
    accounts: Vec<AccountV1>,
    password_verifiers: Vec<PasswordVerifierV1>,
    groups: Vec<GroupV1>,
    group_memberships: Vec<GroupMembershipV1>,
    group_grants: Vec<GroupGrantV1>,
    mfa_factors: Vec<MfaFactorV1>,
    service_connections: Vec<ServiceConnectionV1>,
    log_module_configurations: Vec<LogModuleConfigurationV1>,
    log_assignments: Vec<LogAssignmentV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigurationEntryV1 {
    component: String,
    key: String,
    value: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProtectedSecretV1 {
    component: String,
    key: String,
    value: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountV1 {
    identifier: String,
    username: String,
    display_name: Option<String>,
    active: bool,
    /// Absent in a document written before the requirement existed.
    ///
    /// A missing flag restores an account that is not required to use a second
    /// factor, which is what such a document actually described.
    #[serde(default)]
    mfa_required: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PasswordVerifierV1 {
    account: String,
    verifier: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupV1 {
    identifier: String,
    name: String,
    description: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupMembershipV1 {
    group: String,
    account: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupGrantV1 {
    group: String,
    grant: GrantV1,
}

#[derive(Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
enum GrantV1 {
    ClientModule(String),
    ServiceModule(String),
    Operation(String),
    ServerAdministration,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MfaFactorV1 {
    identifier: String,
    account: String,
    module: String,
    factor_data: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ServiceConnectionV1 {
    identifier: String,
    service_module: String,
    name: String,
    credential: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LogModuleConfigurationV1 {
    identifier: String,
    module: String,
    name: String,
    enabled: bool,
    settings: Vec<LogModuleSettingV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LogModuleSettingV1 {
    key: String,
    value: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LogAssignmentV1 {
    log_type: LogTypeV1,
    configuration: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum LogTypeV1 {
    System,
    Audit,
}

// ---------------------------------------------------------------------------
// Normalization
// ---------------------------------------------------------------------------

/// Parses, compatibility-checks, and normalizes authenticated backup plaintext.
///
/// The plaintext is bounded, strict, versioned UTF-8 JSON. Compatibility is an
/// exact match for Milestone 1: the inner format version must equal
/// [`BACKUP_CONTENT_FORMAT_VERSION`] and the source backend must equal the
/// selected backend.
pub fn normalize(
    plaintext: &[u8],
    selected_backend: &BackendIdentifier,
    components: &AvailableComponents,
) -> Result<NormalizedBackup, ContentError> {
    let document: BackupDocumentV1 =
        serde_json::from_slice(plaintext).map_err(|_| ContentError::Malformed)?;

    if document.format_version != BACKUP_CONTENT_FORMAT_VERSION {
        return Err(ContentError::UnsupportedFormatVersion);
    }
    let source_backend = BackendIdentifier::new(document.source_backend.clone())
        .map_err(|_| ContentError::DomainInvalid)?;
    if source_backend.as_str() != selected_backend.as_str() {
        return Err(ContentError::BackendMismatch);
    }

    let recovery_public_key = RecoveryPublicKey::new(document.recovery_public_key)
        .map_err(|_| ContentError::DomainInvalid)?;

    let configuration = map_collection(document.configuration, |entry| {
        Ok(ConfigurationEntry {
            component: name(entry.component)?,
            key: bounded(entry.key)?,
            value: bounded(entry.value)?,
        })
    })?;
    let protected_secrets = map_collection(document.protected_secrets, |entry| {
        Ok(BackupProtectedSecret {
            component: name(entry.component)?,
            key: bounded(entry.key)?,
            value: SensitiveBytes::new(decode_bytes(&entry.value)?)?,
        })
    })?;
    let accounts = map_collection(document.accounts, |entry| {
        Ok(Account {
            identifier: identifier(&entry.identifier)?,
            username: name(entry.username)?,
            display_name: entry.display_name.map(name).transpose()?,
            active: entry.active,
            mfa_required: entry.mfa_required,
        })
    })?;
    let password_verifiers = map_collection(document.password_verifiers, |entry| {
        Ok(AccountPasswordVerifier {
            account: identifier(&entry.account)?,
            verifier: PasswordVerifier::new(entry.verifier)
                .map_err(|_| ContentError::DomainInvalid)?,
        })
    })?;
    let groups = map_collection(document.groups, |entry| {
        Ok(Group {
            identifier: identifier(&entry.identifier)?,
            name: name(entry.name)?,
            description: entry.description.map(bounded).transpose()?,
        })
    })?;
    let group_memberships = map_collection(document.group_memberships, |entry| {
        Ok(GroupMembership {
            group: identifier(&entry.group)?,
            account: identifier(&entry.account)?,
        })
    })?;
    let group_grants = map_collection(document.group_grants, |entry| {
        Ok(GroupGrantRecord {
            group: identifier(&entry.group)?,
            grant: match entry.grant {
                GrantV1::ClientModule(value) => GroupGrant::ClientModule(name(value)?),
                GrantV1::ServiceModule(value) => GroupGrant::ServiceModule(name(value)?),
                GrantV1::Operation(value) => GroupGrant::Operation(name(value)?),
                GrantV1::ServerAdministration => GroupGrant::ServerAdministration,
            },
        })
    })?;
    let mfa_factors = map_collection(document.mfa_factors, |entry| {
        Ok(BackupMfaFactor {
            identifier: identifier(&entry.identifier)?,
            account: identifier(&entry.account)?,
            module: name(entry.module)?,
            factor_data: SensitiveBytes::new(decode_bytes(&entry.factor_data)?)?,
        })
    })?;
    let service_connections = map_collection(document.service_connections, |entry| {
        Ok(BackupServiceConnection {
            identifier: identifier(&entry.identifier)?,
            service_module: name(entry.service_module)?,
            name: name(entry.name)?,
            credential: SensitiveBytes::new(decode_bytes(&entry.credential)?)?,
        })
    })?;
    let log_module_configurations = map_collection(document.log_module_configurations, |entry| {
        if entry.settings.len() > MAX_LOG_MODULE_SETTINGS {
            return Err(ContentError::CollectionTooLarge);
        }
        Ok(LogModuleConfiguration {
            identifier: identifier(&entry.identifier)?,
            module: name(entry.module)?,
            name: name(entry.name)?,
            enabled: entry.enabled,
            settings: entry
                .settings
                .into_iter()
                .map(|setting| {
                    Ok(LogModuleSetting {
                        key: bounded(setting.key)?,
                        value: bounded(setting.value)?,
                    })
                })
                .collect::<Result<Vec<_>, ContentError>>()?,
        })
    })?;
    let log_assignments = map_collection(document.log_assignments, |entry| {
        Ok(LogAssignment {
            log_type: match entry.log_type {
                LogTypeV1::System => LogType::System,
                LogTypeV1::Audit => LogType::Audit,
            },
            configuration: identifier(&entry.configuration)?,
        })
    })?;

    let mut backup = NormalizedBackup {
        source_backend,
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
    };

    order(&mut backup);
    reject_duplicates(&backup)?;
    reject_unresolved_references(&backup)?;
    reject_invalid_assignments(&backup)?;
    reject_unavailable_components(&backup, components)?;

    Ok(backup)
}

fn order(backup: &mut NormalizedBackup) {
    backup.configuration.sort();
    backup.protected_secrets.sort();
    backup.accounts.sort();
    backup.password_verifiers.sort();
    backup.groups.sort();
    backup.group_memberships.sort();
    backup.group_grants.sort();
    backup.mfa_factors.sort();
    backup.service_connections.sort();
    for configuration in &mut backup.log_module_configurations {
        configuration.settings.sort();
    }
    backup.log_module_configurations.sort();
    backup.log_assignments.sort();
}

fn reject_duplicates(backup: &NormalizedBackup) -> Result<(), ContentError> {
    reject_adjacent(&backup.configuration, |left, right| {
        left.component == right.component && left.key == right.key
    })?;
    reject_adjacent(&backup.protected_secrets, |left, right| {
        left.component == right.component && left.key == right.key
    })?;
    reject_adjacent(&backup.accounts, |left, right| {
        left.identifier == right.identifier || left.username == right.username
    })?;
    reject_adjacent(&backup.password_verifiers, |left, right| {
        left.account == right.account
    })?;
    reject_adjacent(&backup.groups, |left, right| {
        left.identifier == right.identifier
    })?;
    reject_duplicate_keys(&backup.groups, |group| group.name.clone())?;
    reject_adjacent(&backup.group_memberships, |left, right| left == right)?;
    reject_adjacent(&backup.group_grants, |left, right| left == right)?;
    reject_adjacent(&backup.mfa_factors, |left, right| {
        left.identifier == right.identifier
    })?;
    reject_duplicate_keys(&backup.mfa_factors, |factor| {
        (factor.account, factor.module.clone())
    })?;
    reject_adjacent(&backup.service_connections, |left, right| {
        left.identifier == right.identifier
    })?;
    reject_duplicate_keys(&backup.service_connections, |connection| {
        (connection.service_module.clone(), connection.name.clone())
    })?;
    reject_adjacent(&backup.log_module_configurations, |left, right| {
        left.identifier == right.identifier
    })?;
    reject_duplicate_keys(&backup.log_module_configurations, |entry| {
        entry.name.clone()
    })?;
    for configuration in &backup.log_module_configurations {
        reject_adjacent(&configuration.settings, |left, right| left.key == right.key)?;
    }

    Ok(())
}

fn reject_unresolved_references(backup: &NormalizedBackup) -> Result<(), ContentError> {
    let accounts = identifiers(&backup.accounts, |account| account.identifier);
    let groups = identifiers(&backup.groups, |group| group.identifier);

    for verifier in &backup.password_verifiers {
        require(&accounts, verifier.account)?;
    }
    for membership in &backup.group_memberships {
        require(&groups, membership.group)?;
        require(&accounts, membership.account)?;
    }
    for grant in &backup.group_grants {
        require(&groups, grant.group)?;
    }
    for factor in &backup.mfa_factors {
        require(&accounts, factor.account)?;
    }

    Ok(())
}

fn reject_invalid_assignments(backup: &NormalizedBackup) -> Result<(), ContentError> {
    for log_type in LogType::ALL {
        let assigned = backup
            .log_assignments
            .iter()
            .filter(|assignment| assignment.log_type == log_type)
            .collect::<Vec<_>>();
        let [assignment] = assigned.as_slice() else {
            return Err(ContentError::AssignmentInvalid);
        };
        let enabled = backup
            .log_module_configurations
            .iter()
            .any(|entry| entry.identifier == assignment.configuration && entry.enabled);
        if !enabled {
            return Err(ContentError::AssignmentInvalid);
        }
    }

    Ok(())
}

fn reject_unavailable_components(
    backup: &NormalizedBackup,
    components: &AvailableComponents,
) -> Result<(), ContentError> {
    for configuration in &backup.log_module_configurations {
        require_component(components.has_log_module(&configuration.module))?;
    }
    for factor in &backup.mfa_factors {
        require_component(components.has_mfa_module(&factor.module))?;
    }
    for connection in &backup.service_connections {
        require_component(components.has_service_module(&connection.service_module))?;
    }
    for record in &backup.group_grants {
        match &record.grant {
            GroupGrant::ClientModule(module) => {
                require_component(components.has_client_module(module))?
            }
            GroupGrant::ServiceModule(module) => {
                require_component(components.has_service_module(module))?;
            }
            GroupGrant::Operation(operation) => {
                require_component(components.has_operation(operation))?;
            }
            GroupGrant::ServerAdministration => {}
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Field helpers
// ---------------------------------------------------------------------------

fn map_collection<Wire, Value>(
    entries: Vec<Wire>,
    convert: impl Fn(Wire) -> Result<Value, ContentError>,
) -> Result<Vec<Value>, ContentError> {
    if entries.len() > MAX_COLLECTION_ENTRIES {
        return Err(ContentError::CollectionTooLarge);
    }
    entries.into_iter().map(convert).collect()
}

fn name(value: String) -> Result<Name, ContentError> {
    bounded(value)
}

fn bounded<const MAX: usize>(
    value: String,
) -> Result<weavelit_server_database::BoundedText<MAX>, ContentError> {
    weavelit_server_database::BoundedText::new(value).map_err(|_| ContentError::DomainInvalid)
}

fn identifier(value: &str) -> Result<StateIdentifier, ContentError> {
    let bytes: [u8; STATE_IDENTIFIER_LENGTH] = decode_bytes(value)?
        .try_into()
        .map_err(|_| ContentError::EncodingInvalid)?;
    StateIdentifier::from_bytes(bytes).map_err(|_| ContentError::DomainInvalid)
}

/// Decodes canonical unpadded URL-safe Base64 and rejects non-canonical text.
fn decode_bytes(value: &str) -> Result<Vec<u8>, ContentError> {
    let decoded = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| ContentError::EncodingInvalid)?;
    if URL_SAFE_NO_PAD.encode(&decoded) != value {
        return Err(ContentError::EncodingInvalid);
    }
    Ok(decoded)
}

fn reject_adjacent<Value>(
    values: &[Value],
    duplicate: impl Fn(&Value, &Value) -> bool,
) -> Result<(), ContentError> {
    if values.windows(2).any(|pair| duplicate(&pair[0], &pair[1])) {
        return Err(ContentError::DuplicateEntry);
    }
    Ok(())
}

fn reject_duplicate_keys<Value, Key: Ord>(
    values: &[Value],
    key: impl Fn(&Value) -> Key,
) -> Result<(), ContentError> {
    let mut keys = values.iter().map(key).collect::<Vec<_>>();
    keys.sort();
    let total = keys.len();
    keys.dedup();
    if keys.len() != total {
        return Err(ContentError::DuplicateEntry);
    }
    Ok(())
}

fn identifiers<Value>(
    values: &[Value],
    key: impl Fn(&Value) -> StateIdentifier,
) -> BTreeSet<StateIdentifier> {
    values.iter().map(key).collect()
}

fn require(known: &BTreeSet<StateIdentifier>, value: StateIdentifier) -> Result<(), ContentError> {
    if known.contains(&value) {
        return Ok(());
    }
    Err(ContentError::UnresolvedReference)
}

fn require_component(available: bool) -> Result<(), ContentError> {
    if available {
        return Ok(());
    }
    Err(ContentError::ComponentUnavailable)
}
