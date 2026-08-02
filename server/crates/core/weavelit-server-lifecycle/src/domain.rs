use std::{fmt, hash::Hash, mem};

use weavelit_server_database::{DeploymentIdentifier, WorkflowKind};
use zeroize::Zeroize;

use crate::{DomainError, IdentifierError};

/// Current deployment-record and database-locator domain format version.
pub const LIFECYCLE_FORMAT_VERSION: u32 = 1;
/// Number of bytes in a locator generation.
pub const LOCATOR_GENERATION_LENGTH: usize = 16;
/// Maximum number of connection settings in one locator or request.
pub const MAX_CONNECTION_FIELDS: usize = 64;
/// Maximum UTF-8 or byte length of one connection value.
pub const MAX_CONNECTION_VALUE_LENGTH: usize = 16 * 1024;
/// Maximum ASCII length of a backend or connection-field identifier.
pub const MAX_IDENTIFIER_LENGTH: usize = 64;

/// Durable lifecycle state stored in the Server-owned deployment record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleState {
    /// The deployment has no committed application state.
    Uninitialized,
    /// Init or Restore owns a pending non-operational workflow.
    InitializationPending,
    /// Application state is committed and the deployment is irreversibly sealed.
    Initialized,
}

impl LifecycleState {
    /// Returns the canonical persisted state name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Uninitialized => "uninitialized",
            Self::InitializationPending => "initialization_pending",
            Self::Initialized => "initialized",
        }
    }
}

/// Opaque generation binding a deployment record to one immutable locator.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct LocatorGeneration([u8; LOCATOR_GENERATION_LENGTH]);

impl LocatorGeneration {
    /// Creates a locator generation from trusted binary representation.
    pub fn from_bytes(bytes: [u8; LOCATOR_GENERATION_LENGTH]) -> Result<Self, DomainError> {
        if bytes == [0; LOCATOR_GENERATION_LENGTH] {
            return Err(DomainError::InvalidLocatorGeneration);
        }
        Ok(Self(bytes))
    }

    /// Returns the generation's binary representation.
    pub const fn as_bytes(&self) -> &[u8; LOCATOR_GENERATION_LENGTH] {
        &self.0
    }
}

impl fmt::Debug for LocatorGeneration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LocatorGeneration(REDACTED)")
    }
}

/// Validated stable identifier for one compiled-in database backend.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BackendIdentifier(Box<str>);

impl BackendIdentifier {
    /// Validates and creates a backend identifier.
    pub fn new(identifier: impl Into<Box<str>>) -> Result<Self, IdentifierError> {
        let identifier = identifier.into();
        if !is_valid_identifier(&identifier) {
            return Err(IdentifierError::Invalid);
        }
        Ok(Self(identifier))
    }

    /// Returns the canonical identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for BackendIdentifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("BackendIdentifier(REDACTED)")
    }
}

/// Validated stable identifier for one declared connection field.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ConnectionFieldIdentifier(Box<str>);

impl ConnectionFieldIdentifier {
    /// Validates and creates a field identifier.
    pub fn new(identifier: impl Into<Box<str>>) -> Result<Self, IdentifierError> {
        let identifier = identifier.into();
        if !is_valid_identifier(&identifier) {
            return Err(IdentifierError::Invalid);
        }
        Ok(Self(identifier))
    }

    /// Returns the canonical identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ConnectionFieldIdentifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ConnectionFieldIdentifier(REDACTED)")
    }
}

/// Whether a declared connection field must be submitted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionFieldRequirement {
    /// The field must be present exactly once.
    Required,
    /// The field may be omitted but cannot be duplicated.
    Optional,
}

/// Trusted secret classification declared by a backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecretClassification {
    /// The value may be persisted within the encrypted locator as non-secret data.
    NonSecret,
    /// The value is sensitive and must never be exposed through diagnostics.
    Secret,
}

/// Scalar kind accepted by the backend-neutral locator contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionValueKind {
    /// Bounded UTF-8 text.
    String,
    /// Signed 64-bit integer.
    Integer,
    /// Boolean value.
    Boolean,
    /// Bounded opaque bytes.
    Bytes,
}

/// Submitted or validated scalar connection value.
#[derive(Clone, Eq, PartialEq)]
pub enum ConnectionValue {
    /// Bounded UTF-8 text after validation.
    String(Box<str>),
    /// Signed 64-bit integer.
    Integer(i64),
    /// Boolean value.
    Boolean(bool),
    /// Bounded opaque bytes after validation.
    Bytes(Box<[u8]>),
}

impl ConnectionValue {
    /// Creates a text value for later declaration validation.
    pub fn string(value: impl Into<Box<str>>) -> Self {
        Self::String(value.into())
    }

    /// Creates an integer value.
    pub const fn integer(value: i64) -> Self {
        Self::Integer(value)
    }

    /// Creates a boolean value.
    pub const fn boolean(value: bool) -> Self {
        Self::Boolean(value)
    }

    /// Creates a byte value for later declaration validation.
    pub fn bytes(value: impl Into<Box<[u8]>>) -> Self {
        Self::Bytes(value.into())
    }

    /// Returns this value's scalar kind.
    pub const fn kind(&self) -> ConnectionValueKind {
        match self {
            Self::String(_) => ConnectionValueKind::String,
            Self::Integer(_) => ConnectionValueKind::Integer,
            Self::Boolean(_) => ConnectionValueKind::Boolean,
            Self::Bytes(_) => ConnectionValueKind::Bytes,
        }
    }

    /// Returns the contained text when this is a string value.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the contained integer when this is an integer value.
    pub const fn as_integer(&self) -> Option<i64> {
        match self {
            Self::Integer(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns the contained boolean when this is a boolean value.
    pub const fn as_boolean(&self) -> Option<bool> {
        match self {
            Self::Boolean(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns the contained bytes when this is a byte value.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Bytes(value) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn exceeds_bound(&self) -> bool {
        match self {
            Self::String(value) => value.len() > MAX_CONNECTION_VALUE_LENGTH,
            Self::Bytes(value) => value.len() > MAX_CONNECTION_VALUE_LENGTH,
            Self::Integer(_) | Self::Boolean(_) => false,
        }
    }
}

impl fmt::Debug for ConnectionValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectionValue")
            .field("kind", &self.kind())
            .finish_non_exhaustive()
    }
}

impl Zeroize for ConnectionValue {
    fn zeroize(&mut self) {
        match self {
            Self::String(value) => {
                let mut s: String = mem::replace(value, "".into()).into();
                s.zeroize();
            }
            Self::Bytes(value) => value.zeroize(),
            Self::Integer(value) => value.zeroize(),
            Self::Boolean(value) => *value = false,
        }
    }
}

impl Drop for ConnectionValue {
    fn drop(&mut self) {
        self.zeroize();
    }
}

/// One validated connection field passed to a trusted backend factory.
#[derive(Clone, Eq, PartialEq)]
pub struct ValidatedConnectionField {
    identifier: ConnectionFieldIdentifier,
    classification: SecretClassification,
    value: ConnectionValue,
}

impl ValidatedConnectionField {
    pub(crate) const fn new(
        identifier: ConnectionFieldIdentifier,
        classification: SecretClassification,
        value: ConnectionValue,
    ) -> Self {
        Self {
            identifier,
            classification,
            value,
        }
    }

    /// Returns the trusted field identifier.
    pub const fn identifier(&self) -> &ConnectionFieldIdentifier {
        &self.identifier
    }

    /// Returns the backend-declared secret classification.
    pub const fn classification(&self) -> SecretClassification {
        self.classification
    }

    /// Returns the validated value.
    pub const fn value(&self) -> &ConnectionValue {
        &self.value
    }
}

impl fmt::Debug for ValidatedConnectionField {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ValidatedConnectionField")
            .field("classification", &self.classification)
            .field("kind", &self.value.kind())
            .finish_non_exhaustive()
    }
}

/// Canonically ordered connection settings passed to a trusted backend factory.
#[derive(Clone, Eq, PartialEq)]
pub struct ValidatedConnectionSettings {
    backend_identifier: BackendIdentifier,
    fields: Box<[ValidatedConnectionField]>,
}

impl ValidatedConnectionSettings {
    pub(crate) fn new(
        backend_identifier: BackendIdentifier,
        fields: Vec<ValidatedConnectionField>,
    ) -> Self {
        Self {
            backend_identifier,
            fields: fields.into_boxed_slice(),
        }
    }

    /// Returns the backend declaration that validated these settings.
    pub const fn backend_identifier(&self) -> &BackendIdentifier {
        &self.backend_identifier
    }

    /// Returns the number of validated fields.
    pub const fn len(&self) -> usize {
        self.fields.len()
    }

    /// Returns whether no fields were supplied.
    pub const fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// Iterates over validated fields in canonical identifier order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &ValidatedConnectionField> {
        self.fields.iter()
    }

    /// Returns one validated field by identifier.
    pub fn get(&self, identifier: &ConnectionFieldIdentifier) -> Option<&ValidatedConnectionField> {
        self.fields
            .binary_search_by(|field| field.identifier.cmp(identifier))
            .ok()
            .map(|position| &self.fields[position])
    }
}

impl fmt::Debug for ValidatedConnectionSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ValidatedConnectionSettings")
            .field("field_count", &self.fields.len())
            .finish_non_exhaustive()
    }
}

impl ValidatedConnectionSettings {
    /// Removes transient classifications for encrypted locator persistence.
    pub fn into_locator_settings(self) -> LocatorConnectionSettings {
        let fields = self
            .fields
            .into_vec()
            .into_iter()
            .map(|field| LocatorConnectionField::new(field.identifier, field.value))
            .collect();
        LocatorConnectionSettings::new(self.backend_identifier, fields)
    }
}

/// One typed field/value pair stored inside the encrypted locator.
#[derive(Clone, Eq, PartialEq)]
pub struct LocatorConnectionField {
    identifier: ConnectionFieldIdentifier,
    value: ConnectionValue,
}

impl LocatorConnectionField {
    pub(crate) const fn new(identifier: ConnectionFieldIdentifier, value: ConnectionValue) -> Self {
        Self { identifier, value }
    }

    /// Returns the persisted field identifier.
    pub const fn identifier(&self) -> &ConnectionFieldIdentifier {
        &self.identifier
    }

    /// Returns the persisted typed value.
    pub const fn value(&self) -> &ConnectionValue {
        &self.value
    }
}

impl fmt::Debug for LocatorConnectionField {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocatorConnectionField")
            .field("kind", &self.value.kind())
            .finish_non_exhaustive()
    }
}

/// Canonically ordered typed settings stored inside one encrypted locator.
#[derive(Clone, Eq, PartialEq)]
pub struct LocatorConnectionSettings {
    backend_identifier: BackendIdentifier,
    fields: Box<[LocatorConnectionField]>,
}

impl LocatorConnectionSettings {
    pub(crate) fn new(
        backend_identifier: BackendIdentifier,
        mut fields: Vec<LocatorConnectionField>,
    ) -> Self {
        fields.sort_by(|left, right| left.identifier.cmp(&right.identifier));
        Self {
            backend_identifier,
            fields: fields.into_boxed_slice(),
        }
    }

    /// Returns the backend that owns these settings.
    pub const fn backend_identifier(&self) -> &BackendIdentifier {
        &self.backend_identifier
    }

    /// Returns the number of persisted fields.
    pub const fn len(&self) -> usize {
        self.fields.len()
    }

    /// Returns whether no fields are persisted.
    pub const fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// Iterates over persisted fields in canonical identifier order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &LocatorConnectionField> {
        self.fields.iter()
    }
}

impl fmt::Debug for LocatorConnectionSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocatorConnectionSettings")
            .field("field_count", &self.fields.len())
            .finish_non_exhaustive()
    }
}

/// Validated deployment record domain value without persistence concerns.
#[derive(Clone, Eq, PartialEq)]
pub struct DeploymentRecord {
    deployment_identifier: DeploymentIdentifier,
    state: LifecycleState,
    locator_generation: Option<LocatorGeneration>,
}

impl DeploymentRecord {
    /// Creates a record after validating state-to-locator invariants.
    pub fn new(
        deployment_identifier: DeploymentIdentifier,
        state: LifecycleState,
        locator_generation: Option<LocatorGeneration>,
    ) -> Result<Self, DomainError> {
        if state != LifecycleState::Uninitialized && locator_generation.is_none() {
            return Err(DomainError::InvalidDeploymentRecord);
        }
        Ok(Self {
            deployment_identifier,
            state,
            locator_generation,
        })
    }

    /// Returns the fixed domain format version.
    pub const fn format_version(&self) -> u32 {
        LIFECYCLE_FORMAT_VERSION
    }

    /// Returns the bound deployment identifier.
    pub const fn deployment_identifier(&self) -> DeploymentIdentifier {
        self.deployment_identifier
    }

    /// Returns the durable lifecycle state.
    pub const fn state(&self) -> LifecycleState {
        self.state
    }

    /// Returns the selected locator generation, when one exists.
    pub const fn locator_generation(&self) -> Option<LocatorGeneration> {
        self.locator_generation
    }
}

impl fmt::Debug for DeploymentRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DeploymentRecord")
            .field("state", &self.state)
            .field("has_locator", &self.locator_generation.is_some())
            .finish_non_exhaustive()
    }
}

/// Validated backend-neutral database locator content.
#[derive(Clone, Eq, PartialEq)]
pub struct DatabaseLocator {
    deployment_identifier: DeploymentIdentifier,
    generation: LocatorGeneration,
    settings: LocatorConnectionSettings,
}

impl DatabaseLocator {
    /// Creates locator content from validated catalog values.
    pub fn from_validated(
        deployment_identifier: DeploymentIdentifier,
        generation: LocatorGeneration,
        settings: ValidatedConnectionSettings,
    ) -> Self {
        Self::from_persisted(
            deployment_identifier,
            generation,
            settings.into_locator_settings(),
        )
    }

    pub(crate) const fn from_persisted(
        deployment_identifier: DeploymentIdentifier,
        generation: LocatorGeneration,
        settings: LocatorConnectionSettings,
    ) -> Self {
        Self {
            deployment_identifier,
            generation,
            settings,
        }
    }

    /// Returns the fixed domain format version.
    pub const fn format_version(&self) -> u32 {
        LIFECYCLE_FORMAT_VERSION
    }

    /// Returns the bound deployment identifier.
    pub const fn deployment_identifier(&self) -> DeploymentIdentifier {
        self.deployment_identifier
    }

    /// Returns the immutable locator generation.
    pub const fn generation(&self) -> LocatorGeneration {
        self.generation
    }

    /// Returns the selected backend identifier.
    pub const fn backend_identifier(&self) -> &BackendIdentifier {
        self.settings.backend_identifier()
    }

    /// Returns the canonically ordered validated settings.
    pub const fn settings(&self) -> &LocatorConnectionSettings {
        &self.settings
    }
}

impl fmt::Debug for DatabaseLocator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DatabaseLocator")
            .field("setting_count", &self.settings.len())
            .finish_non_exhaustive()
    }
}

/// Capability classification returned by startup orchestration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleClassification {
    /// No database has been selected.
    UninitializedWithoutDatabase,
    /// An eligible uninitialized database has been selected.
    UninitializedWithDatabase,
    /// One workflow owns the non-operational pending state.
    InitializationPending(WorkflowKind),
    /// The Init or Restore workflow committed the database but the deployment record
    /// has not yet been sealed; workflow-specific post-commit reconciliation is required.
    PostCommitReconciliationRequired,
    /// The deployment is sealed for normal operation.
    Initialized,
}

fn is_valid_identifier(identifier: &str) -> bool {
    let bytes = identifier.as_bytes();
    if bytes.is_empty() || bytes.len() > MAX_IDENTIFIER_LENGTH || !bytes[0].is_ascii_lowercase() {
        return false;
    }

    let mut previous_was_hyphen = false;
    for byte in bytes {
        if *byte == b'-' {
            if previous_was_hyphen {
                return false;
            }
            previous_was_hyphen = true;
        } else if byte.is_ascii_lowercase() || byte.is_ascii_digit() {
            previous_was_hyphen = false;
        } else {
            return false;
        }
    }
    !previous_was_hyphen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_states_use_canonical_names() {
        assert_eq!(LifecycleState::Uninitialized.as_str(), "uninitialized");
        assert_eq!(
            LifecycleState::InitializationPending.as_str(),
            "initialization_pending"
        );
        assert_eq!(LifecycleState::Initialized.as_str(), "initialized");
    }

    #[test]
    fn identifiers_enforce_the_canonical_grammar() {
        for valid in ["sqlite", "remote-postgres-2"] {
            assert!(BackendIdentifier::new(valid).is_ok());
        }
        for invalid in [
            "",
            "SQLite",
            "-sqlite",
            "sqlite-",
            "sqlite--local",
            "sql_ite",
        ] {
            assert_eq!(
                BackendIdentifier::new(invalid).unwrap_err(),
                IdentifierError::Invalid
            );
        }
        assert_eq!(
            BackendIdentifier::new("x".repeat(MAX_IDENTIFIER_LENGTH + 1)).unwrap_err(),
            IdentifierError::Invalid
        );
    }

    #[test]
    fn pending_and_initialized_records_require_a_locator() {
        let deployment_identifier = DeploymentIdentifier::from_bytes([1; 16]).unwrap();
        for state in [
            LifecycleState::InitializationPending,
            LifecycleState::Initialized,
        ] {
            assert_eq!(
                DeploymentRecord::new(deployment_identifier, state, None).unwrap_err(),
                DomainError::InvalidDeploymentRecord
            );
        }
        let record =
            DeploymentRecord::new(deployment_identifier, LifecycleState::Uninitialized, None)
                .unwrap();
        assert_eq!(record.format_version(), LIFECYCLE_FORMAT_VERSION);
    }

    #[test]
    fn sensitive_domain_debug_output_is_redacted() {
        let deployment_identifier = DeploymentIdentifier::from_bytes(*b"sensitive-value!").unwrap();
        let generation = LocatorGeneration::from_bytes([7; 16]).unwrap();
        let backend_identifier = BackendIdentifier::new("sensitive-backend").unwrap();
        let settings = ValidatedConnectionSettings::new(
            backend_identifier,
            vec![ValidatedConnectionField::new(
                ConnectionFieldIdentifier::new("sensitive-field").unwrap(),
                SecretClassification::Secret,
                ConnectionValue::string("sensitive-secret"),
            )],
        );
        let locator = DatabaseLocator::from_validated(deployment_identifier, generation, settings);
        let output = format!("{locator:?}");

        assert!(!output.contains("sensitive"));
        assert!(!output.contains("backend"));
    }
}
