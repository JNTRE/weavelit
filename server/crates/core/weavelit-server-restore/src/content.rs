use std::{collections::BTreeSet, fmt, marker::PhantomData};

use argon2::password_hash::PasswordHash;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{
    Deserialize, Deserializer,
    de::{Error as DeserializeError, IgnoredAny, SeqAccess, Visitor},
};
use weavelit_server_authentication::PasswordPolicy;
use weavelit_server_components::AvailableComponents;
use weavelit_server_database::{
    Account, AccountPasswordVerifier, ConfigurationEntry, ConfigurationKey, Group, GroupGrant,
    GroupGrantRecord, GroupMembership, LogAssignment, LogModuleConfiguration, LogModuleSetting,
    LogType, MAX_CONFIGURATION_KEY_LENGTH, MAX_CONFIGURATION_VALUE_LENGTH, MAX_DESCRIPTION_LENGTH,
    MAX_NAME_LENGTH, MAX_PASSWORD_VERIFIER_LENGTH, MAX_PROTECTED_VALUE_LENGTH,
    MAX_RECOVERY_PUBLIC_KEY_LENGTH, Name, PasswordVerifier, RecoveryPublicKey,
    STATE_IDENTIFIER_LENGTH, StateIdentifier,
};
use weavelit_server_lifecycle::{
    BackendIdentifier, MAX_IDENTIFIER_LENGTH, MAX_PROTECTED_PLAINTEXT_BYTES,
};
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
// Bounded wire primitives
// ---------------------------------------------------------------------------

/// Returns the length of the canonical unpadded Base64 encoding of `bytes`.
const fn encoded_length(bytes: usize) -> usize {
    bytes / 3 * 4
        + match bytes % 3 {
            0 => 0,
            1 => 2,
            _ => 3,
        }
}

/// Encoded length of one opaque state identifier.
const WIRE_IDENTIFIER_LENGTH: usize = encoded_length(STATE_IDENTIFIER_LENGTH);

const _: () = assert!(WIRE_IDENTIFIER_LENGTH == 22);

/// Encoded ceiling of one decrypted protected value.
const WIRE_SENSITIVE_VALUE_LENGTH: usize = encoded_length(MAX_SENSITIVE_VALUE_BYTES);

/// Wire sequence that deserializes at most `MAX` typed elements.
///
/// Every element past the limit is consumed as [`IgnoredAny`] and is never
/// deserialized as `Element`, so a document that declares more entries than the
/// Server accepts cannot allocate the surplus records or their strings before
/// the bound is applied. Overflow is recorded rather than rejected here, so
/// [`ContentError::CollectionTooLarge`] stays attributed to `map_collection`.
struct BoundedCollection<Element, const MAX: usize> {
    entries: Vec<Element>,
    overflow: bool,
}

struct BoundedCollectionVisitor<Element, const MAX: usize>(PhantomData<fn() -> Element>);

impl<'de, Element: Deserialize<'de>, const MAX: usize> Visitor<'de>
    for BoundedCollectionVisitor<Element, MAX>
{
    type Value = BoundedCollection<Element, MAX>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "a sequence of at most {MAX} entries")
    }

    fn visit_seq<Access: SeqAccess<'de>>(
        self,
        mut sequence: Access,
    ) -> Result<Self::Value, Access::Error> {
        let mut entries = Vec::new();
        while entries.len() < MAX {
            let Some(entry) = sequence.next_element::<Element>()? else {
                return Ok(BoundedCollection {
                    entries,
                    overflow: false,
                });
            };
            entries.push(entry);
        }

        let mut overflow = false;
        while sequence.next_element::<IgnoredAny>()?.is_some() {
            overflow = true;
        }
        Ok(BoundedCollection { entries, overflow })
    }
}

impl<'de, Element: Deserialize<'de>, const MAX: usize> Deserialize<'de>
    for BoundedCollection<Element, MAX>
{
    fn deserialize<Source: Deserializer<'de>>(source: Source) -> Result<Self, Source::Error> {
        source.deserialize_seq(BoundedCollectionVisitor::<Element, MAX>(PhantomData))
    }
}

/// Wire text rejected before it is owned when it exceeds `MAX` encoded bytes.
///
/// The domain constructor applies the same bound again, together with the rest
/// of its contract; this wrapper only stops an over-long value from being
/// allocated in the first place.
struct WireText<const MAX: usize>(String);

impl<const MAX: usize> WireText<MAX> {
    fn into_inner(self) -> String {
        self.0
    }
}

impl<const MAX: usize> std::ops::Deref for WireText<MAX> {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

struct WireTextVisitor<const MAX: usize>;

impl<const MAX: usize> Visitor<'_> for WireTextVisitor<MAX> {
    type Value = WireText<MAX>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "a string of at most {MAX} bytes")
    }

    fn visit_str<Failure: DeserializeError>(self, value: &str) -> Result<Self::Value, Failure> {
        if value.len() > MAX {
            return Err(Failure::invalid_length(value.len(), &self));
        }
        Ok(WireText(value.to_owned()))
    }

    fn visit_string<Failure: DeserializeError>(
        self,
        value: String,
    ) -> Result<Self::Value, Failure> {
        if value.len() > MAX {
            return Err(Failure::invalid_length(value.len(), &self));
        }
        Ok(WireText(value))
    }
}

impl<'de, const MAX: usize> Deserialize<'de> for WireText<MAX> {
    fn deserialize<Source: Deserializer<'de>>(source: Source) -> Result<Self, Source::Error> {
        source.deserialize_str(WireTextVisitor::<MAX>)
    }
}

/// Encoded wire secret bounded the same way, whose buffer is wiped when dropped.
struct WireSecret<const MAX: usize>(Zeroizing<String>);

impl<const MAX: usize> std::ops::Deref for WireSecret<MAX> {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

struct WireSecretVisitor<const MAX: usize>;

impl<const MAX: usize> Visitor<'_> for WireSecretVisitor<MAX> {
    type Value = WireSecret<MAX>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "an encoded secret of at most {MAX} bytes")
    }

    fn visit_str<Failure: DeserializeError>(self, value: &str) -> Result<Self::Value, Failure> {
        if value.len() > MAX {
            return Err(Failure::invalid_length(value.len(), &self));
        }
        Ok(WireSecret(Zeroizing::new(value.to_owned())))
    }

    fn visit_string<Failure: DeserializeError>(
        self,
        value: String,
    ) -> Result<Self::Value, Failure> {
        // The buffer is wiped on both the accepting and the rejecting path.
        let value = Zeroizing::new(value);
        if value.len() > MAX {
            return Err(Failure::invalid_length(value.len(), &self));
        }
        Ok(WireSecret(value))
    }
}

impl<'de, const MAX: usize> Deserialize<'de> for WireSecret<MAX> {
    fn deserialize<Source: Deserializer<'de>>(source: Source) -> Result<Self, Source::Error> {
        source.deserialize_str(WireSecretVisitor::<MAX>)
    }
}

type WireBackendIdentifier = WireText<MAX_IDENTIFIER_LENGTH>;
type WireConfigurationKey = WireText<MAX_CONFIGURATION_KEY_LENGTH>;
type WireConfigurationValue = WireText<MAX_CONFIGURATION_VALUE_LENGTH>;
type WireDescription = WireText<MAX_DESCRIPTION_LENGTH>;
type WireIdentifier = WireText<WIRE_IDENTIFIER_LENGTH>;
type WireName = WireText<MAX_NAME_LENGTH>;
type WirePasswordVerifier = WireText<MAX_PASSWORD_VERIFIER_LENGTH>;
type WireRecoveryPublicKey = WireText<MAX_RECOVERY_PUBLIC_KEY_LENGTH>;
type WireSensitiveValue = WireSecret<WIRE_SENSITIVE_VALUE_LENGTH>;

// ---------------------------------------------------------------------------
// Version 1 wire model
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BackupDocumentV1 {
    format_version: u32,
    source_backend: WireBackendIdentifier,
    recovery_public_key: WireRecoveryPublicKey,
    configuration: BoundedCollection<ConfigurationEntryV1, MAX_COLLECTION_ENTRIES>,
    protected_secrets: BoundedCollection<ProtectedSecretV1, MAX_COLLECTION_ENTRIES>,
    accounts: BoundedCollection<AccountV1, MAX_COLLECTION_ENTRIES>,
    password_verifiers: BoundedCollection<PasswordVerifierV1, MAX_COLLECTION_ENTRIES>,
    groups: BoundedCollection<GroupV1, MAX_COLLECTION_ENTRIES>,
    group_memberships: BoundedCollection<GroupMembershipV1, MAX_COLLECTION_ENTRIES>,
    group_grants: BoundedCollection<GroupGrantV1, MAX_COLLECTION_ENTRIES>,
    mfa_factors: BoundedCollection<MfaFactorV1, MAX_COLLECTION_ENTRIES>,
    service_connections: BoundedCollection<ServiceConnectionV1, MAX_COLLECTION_ENTRIES>,
    log_module_configurations: BoundedCollection<LogModuleConfigurationV1, MAX_COLLECTION_ENTRIES>,
    log_assignments: BoundedCollection<LogAssignmentV1, MAX_COLLECTION_ENTRIES>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigurationEntryV1 {
    component: WireName,
    key: WireConfigurationKey,
    value: WireConfigurationValue,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProtectedSecretV1 {
    component: WireName,
    key: WireConfigurationKey,
    /// Encoded secret; the owned buffer is wiped when the entry is dropped.
    value: WireSensitiveValue,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountV1 {
    identifier: WireIdentifier,
    username: WireName,
    display_name: Option<WireName>,
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
    account: WireIdentifier,
    verifier: WirePasswordVerifier,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupV1 {
    identifier: WireIdentifier,
    name: WireName,
    description: Option<WireDescription>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupMembershipV1 {
    group: WireIdentifier,
    account: WireIdentifier,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupGrantV1 {
    group: WireIdentifier,
    grant: GrantV1,
}

#[derive(Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
enum GrantV1 {
    ClientModule(WireName),
    ServiceModule(WireName),
    Operation(WireName),
    ServerAdministration,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MfaFactorV1 {
    identifier: WireIdentifier,
    account: WireIdentifier,
    module: WireName,
    /// Encoded secret; the owned buffer is wiped when the entry is dropped.
    factor_data: WireSensitiveValue,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ServiceConnectionV1 {
    identifier: WireIdentifier,
    service_module: WireName,
    name: WireName,
    /// Encoded secret; the owned buffer is wiped when the entry is dropped.
    credential: WireSensitiveValue,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LogModuleConfigurationV1 {
    identifier: WireIdentifier,
    module: WireName,
    name: WireName,
    enabled: bool,
    settings: BoundedCollection<LogModuleSettingV1, MAX_LOG_MODULE_SETTINGS>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LogModuleSettingV1 {
    key: WireConfigurationKey,
    value: WireConfigurationValue,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LogAssignmentV1 {
    log_type: LogTypeV1,
    configuration: WireIdentifier,
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
    let source_backend = BackendIdentifier::new(document.source_backend.into_inner())
        .map_err(|_| ContentError::DomainInvalid)?;
    if source_backend.as_str() != selected_backend.as_str() {
        return Err(ContentError::BackendMismatch);
    }

    let recovery_public_key = RecoveryPublicKey::new(document.recovery_public_key.into_inner())
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
            verifier: PasswordVerifier::new(entry.verifier.into_inner())
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
        Ok(LogModuleConfiguration {
            identifier: identifier(&entry.identifier)?,
            module: name(entry.module)?,
            name: name(entry.name)?,
            enabled: entry.enabled,
            settings: map_collection(entry.settings, |setting| {
                Ok(LogModuleSetting {
                    key: bounded(setting.key)?,
                    value: bounded(setting.value)?,
                })
            })?,
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
    reject_unreadable_module_data(&backup, components)?;
    reject_unusable_password_verifiers(&backup)?;

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
        left.identifier == right.identifier
    })?;
    reject_duplicate_keys(&backup.accounts, |account| account.username.clone())?;
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

/// Rejects state a compiled-in module could not read or be served with.
///
/// A factor or configuration naming a module this build does not compile in was
/// already refused as unavailable, so everything reaching here names a module
/// whose own declarations the inventory carries.
///
/// Checking an MFA factor's format here means a factor the named module could
/// never open is refused as an invalid backup rather than sealed, activated, and
/// then discovered at the account's next sign-in, which for the only required
/// Administrator would leave the deployment unreachable. Checking a Log Module
/// configuration's settings here means a configuration the named module would
/// refuse to open is caught before any checkpoint exists, rather than sealed and
/// then silently served without the settings it declared. That check compares
/// declared keys only: it opens no destination and creates no local Log Module
/// storage, which a pre-checkpoint failure has promised not to leave behind.
///
/// Both declarations are the module crates' own, carried on the inventory, so a
/// second MFA or Log Module is covered by supplying its declaration there rather
/// than by extending this check.
fn reject_unreadable_module_data(
    backup: &NormalizedBackup,
    components: &AvailableComponents,
) -> Result<(), ContentError> {
    for factor in &backup.mfa_factors {
        let readable = components
            .mfa_factor_format(&factor.module)
            .is_some_and(|format| format.accepts(factor.factor_data.expose()));
        if !readable {
            return Err(ContentError::FactorDataInvalid);
        }
    }
    for configuration in &backup.log_module_configurations {
        let servable = components
            .log_settings_format(&configuration.module)
            .is_some_and(|format| {
                configuration
                    .settings
                    .iter()
                    .all(|setting| format.accepts(setting.key.as_str()))
            });
        if !servable {
            return Err(ContentError::SettingUnsupported);
        }
    }

    Ok(())
}

/// Rejects a password verifier the Server would never attempt.
///
/// A stored verifier's encoded shape is not enough. The Application Database
/// contract accepts any bounded ASCII PHC-shaped string, but the password
/// decision attempts a stored verifier only when its algorithm, version, cost
/// parameters, salt length, and output length match the closed allowlist the
/// authentication crate owns; a verifier outside that allowlist is verified
/// against a decoy and always denied. A backup can carry whatever parameters
/// its author chose, so without this check a backup whose only active
/// Administrator carries an off-profile verifier would restore, seal, and then
/// deny every password, leaving a deployment no one can sign in to and a
/// Restore that cannot be retried.
///
/// Every off-profile verifier is refused, not only the last Administrator's, so
/// the check needs no reasoning about account topology and stays a field
/// rejection like its neighbours.
///
/// It covers supplied entries only. An account with no verifier is a modeled
/// credential state, so no verifier is required to exist here and no
/// administrator-topology check belongs here: the specification accepts a
/// deployment no Administrator can authenticate to and forbids Restore from
/// claiming to guarantee renewed administrative access.
///
/// The allowlist is resolved through [`PasswordPolicy::approved`] against the
/// same PHC reader the authentication decision parses with, so a backup is
/// accepted here exactly when the Server would later attempt what it carries.
fn reject_unusable_password_verifiers(backup: &NormalizedBackup) -> Result<(), ContentError> {
    let policy = PasswordPolicy::approved();
    for entry in &backup.password_verifiers {
        let usable = PasswordHash::new(entry.verifier.as_str())
            .is_ok_and(|parsed| policy.resolve(&parsed).is_some());
        if !usable {
            return Err(ContentError::DomainInvalid);
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Field helpers
// ---------------------------------------------------------------------------

fn map_collection<Wire, Value, const MAX: usize>(
    entries: BoundedCollection<Wire, MAX>,
    convert: impl Fn(Wire) -> Result<Value, ContentError>,
) -> Result<Vec<Value>, ContentError> {
    if entries.overflow {
        return Err(ContentError::CollectionTooLarge);
    }
    entries.entries.into_iter().map(convert).collect()
}

fn name(value: WireName) -> Result<Name, ContentError> {
    bounded(value)
}

fn bounded<const MAX: usize>(
    value: WireText<MAX>,
) -> Result<weavelit_server_database::BoundedText<MAX>, ContentError> {
    weavelit_server_database::BoundedText::new(value.into_inner())
        .map_err(|_| ContentError::DomainInvalid)
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
    // The canonical re-encoding reproduces the caller's encoded secret, so it is
    // wiped on both the accepting and the rejecting path.
    let canonical = Zeroizing::new(URL_SAFE_NO_PAD.encode(&decoded));
    if canonical.as_str() != value {
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

// ---------------------------------------------------------------------------
// Wire model type pins
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use serde::{Deserialize, Deserializer};

    use super::{
        BoundedCollection, MfaFactorV1, ProtectedSecretV1, ServiceConnectionV1,
        WIRE_SENSITIVE_VALUE_LENGTH, WireSecret, Zeroizing,
    };

    /// Pins the wire model's secret fields to the wiping string type.
    ///
    /// Each binding is an explicit type annotation, so returning any of these
    /// fields to a plain `String` fails to compile rather than silently
    /// reintroducing an encoded secret that is dropped without being wiped.
    #[test]
    fn the_wire_secret_fields_are_zeroizing_strings() {
        let secret: ProtectedSecretV1 = serde_json::from_str(
            r#"{"component":"weavelit-server","key":"provider","value":"cHJvdmlkZXItdG9rZW4"}"#,
        )
        .expect("the protected secret entry parses");
        let value: WireSecret<WIRE_SENSITIVE_VALUE_LENGTH> = secret.value;
        let value: Zeroizing<String> = value.0;
        assert_eq!(value.as_str(), "cHJvdmlkZXItdG9rZW4");

        let factor: MfaFactorV1 = serde_json::from_str(
            r#"{"identifier":"AgMEBQYHCAkKCwwNDg8QEQ","account":"AgMEBQYHCAkKCwwNDg8QEQ","module":"totp","factor_data":"dG90cC1zZWVk"}"#,
        )
        .expect("the MFA factor entry parses");
        let factor_data: WireSecret<WIRE_SENSITIVE_VALUE_LENGTH> = factor.factor_data;
        let factor_data: Zeroizing<String> = factor_data.0;
        assert_eq!(factor_data.as_str(), "dG90cC1zZWVk");

        let connection: ServiceConnectionV1 = serde_json::from_str(
            r#"{"identifier":"AgMEBQYHCAkKCwwNDg8QEQ","service_module":"zendesk","name":"primary","credential":"cHJvdmlkZXItdG9rZW4"}"#,
        )
        .expect("the Service Connection entry parses");
        let credential: WireSecret<WIRE_SENSITIVE_VALUE_LENGTH> = connection.credential;
        let credential: Zeroizing<String> = credential.0;
        assert_eq!(credential.as_str(), "cHJvdmlkZXItdG9rZW4");
    }

    thread_local! {
        /// Number of times [`Recorded`] was asked to deserialize an element.
        static ELEMENT_DESERIALIZATIONS: Cell<usize> = const { Cell::new(0) };
    }

    /// Element that records every attempt to deserialize it.
    struct Recorded;

    impl<'de> Deserialize<'de> for Recorded {
        fn deserialize<Source: Deserializer<'de>>(source: Source) -> Result<Self, Source::Error> {
            ELEMENT_DESERIALIZATIONS.with(|count| count.set(count.get() + 1));
            u32::deserialize(source)?;
            Ok(Self)
        }
    }

    #[test]
    fn a_bounded_collection_never_deserializes_an_element_past_its_limit() {
        ELEMENT_DESERIALIZATIONS.with(|count| count.set(0));

        // The fourth entry could not deserialize as `Recorded`, so accepting the
        // document proves the surplus entries were consumed as `IgnoredAny`.
        let collection: BoundedCollection<Recorded, 3> =
            serde_json::from_str(r#"[1,2,3,"not a number",{"nested":[4,5]}]"#)
                .expect("entries past the limit are ignored rather than deserialized");

        assert_eq!(collection.entries.len(), 3);
        assert!(collection.overflow);
        assert_eq!(ELEMENT_DESERIALIZATIONS.with(Cell::get), 3);
    }

    #[test]
    fn a_bounded_collection_at_its_limit_does_not_report_overflow() {
        ELEMENT_DESERIALIZATIONS.with(|count| count.set(0));

        let collection: BoundedCollection<Recorded, 3> =
            serde_json::from_str("[1,2,3]").expect("a collection at its limit is accepted");

        assert_eq!(collection.entries.len(), 3);
        assert!(!collection.overflow);
        assert_eq!(ELEMENT_DESERIALIZATIONS.with(Cell::get), 3);
    }
}
