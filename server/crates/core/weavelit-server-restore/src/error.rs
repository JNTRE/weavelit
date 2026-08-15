use std::{error::Error as StdError, fmt};

use weavelit_server_lifecycle::LifecycleError;
use weavelit_server_recovery_key::RecoveryKeyError;

/// Stable payload-free Restore failure category.
///
/// Every variant collapses to one of the Restore Design's fixed categories.
/// A wrong recovery key, an altered artifact, and any other authentication
/// failure all produce [`RestoreError::BackupInvalid`] with no distinguishing
/// data, so they remain indistinguishable to a caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RestoreError {
    /// The submitted recovery key was not exactly one canonical age line.
    RecoveryKeyInvalid,
    /// The artifact was malformed, unauthentic, altered, or semantically invalid.
    BackupInvalid,
    /// The artifact's format version, source backend, or components are unsupported.
    BackupIncompatible,
    /// Another Restore operation holds the exclusive Restore permit.
    RestorePending,
    /// A deadline, storage failure, or other internal failure stopped Restore.
    RestoreFailed,
    /// A composed shared lifecycle category rejected the request.
    Lifecycle(LifecycleError),
}

impl RestoreError {
    /// Returns the stable category/reason pair for centralized error presentation.
    pub fn category_reason(&self) -> (&'static str, &'static str) {
        match self {
            Self::RecoveryKeyInvalid => ("recovery_key_invalid", "recovery_key_invalid"),
            Self::BackupInvalid => ("backup_invalid", "backup_invalid"),
            Self::BackupIncompatible => ("backup_incompatible", "backup_incompatible"),
            Self::RestorePending => ("restore_pending", "restore_pending"),
            Self::RestoreFailed => ("restore_failed", "restore_failed"),
            Self::Lifecycle(error) => lifecycle_category_reason(*error),
        }
    }
}

impl fmt::Display for RestoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.category_reason().0)
    }
}

impl StdError for RestoreError {}

impl From<LifecycleError> for RestoreError {
    fn from(error: LifecycleError) -> Self {
        Self::Lifecycle(error)
    }
}

/// Every shared recovery-key rejection collapses to the single Restore category.
impl From<RecoveryKeyError> for RestoreError {
    fn from(_: RecoveryKeyError) -> Self {
        Self::RecoveryKeyInvalid
    }
}

fn lifecycle_category_reason(error: LifecycleError) -> (&'static str, &'static str) {
    match error {
        LifecycleError::Persistence => ("storage_unavailable", "storage_operation_failed"),
        LifecycleError::DependencyUnavailable => ("storage_unavailable", "database_unavailable"),
        LifecycleError::ConfigurationInvalid => {
            ("configuration_invalid", "lifecycle_configuration_invalid")
        }
        LifecycleError::LockContended => ("preoperational_unavailable", "state_root_in_use"),
        LifecycleError::IntegrityFailure => ("storage_integrity_failure", "anchor_set_invalid"),
        LifecycleError::DeploymentMismatch => {
            ("storage_integrity_failure", "anchor_binding_invalid")
        }
        LifecycleError::UnsupportedVersion => {
            ("storage_integrity_failure", "anchor_version_unsupported")
        }
        _ => ("deployment_state_invalid", "state_combination_invalid"),
    }
}

/// Invalid outer backup envelope rejected before any cryptographic work.
///
/// Variants exist for validation-order attribution inside the workspace. Their
/// display representation is uniform, and every variant maps to one of the two
/// public categories through [`RestoreError`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnvelopeError {
    /// The artifact was shorter than the fixed outer header.
    TooShort,
    /// The fixed 8-byte magic did not match.
    MagicMismatch,
    /// The outer format-version field was not the supported version.
    UnsupportedFormatVersion,
    /// A reserved flag byte was not zero.
    FlagsNotZero,
    /// The declared encrypted-payload length did not match the remaining stream.
    DeclaredLengthMismatch,
}

impl fmt::Display for EnvelopeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("backup envelope is invalid")
    }
}

impl StdError for EnvelopeError {}

impl From<EnvelopeError> for RestoreError {
    fn from(error: EnvelopeError) -> Self {
        match error {
            EnvelopeError::UnsupportedFormatVersion => Self::BackupIncompatible,
            _ => Self::BackupInvalid,
        }
    }
}

/// Invalid authenticated backup plaintext rejected before application-state mutation.
///
/// Variants exist for validation-order attribution inside the workspace. Their
/// display representation is uniform, and every variant maps to one of the two
/// public categories through [`RestoreError`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentError {
    /// The authenticated plaintext exceeded its configured bound.
    PlaintextTooLarge,
    /// The plaintext was not bounded, strict, versioned UTF-8 JSON.
    Malformed,
    /// The repeated inner format version was not the supported version.
    UnsupportedFormatVersion,
    /// The backup's source Application Database backend was not the selected backend.
    BackendMismatch,
    /// A required compiled-in component named by the backup was unavailable.
    ComponentUnavailable,
    /// A collection exceeded its configured entry bound.
    CollectionTooLarge,
    /// A value fell outside a Server domain constraint.
    DomainInvalid,
    /// A binary field was not canonical unpadded URL-safe Base64 of the exact length.
    EncodingInvalid,
    /// Two entries shared an identity that must be unique.
    DuplicateEntry,
    /// An internal reference did not resolve.
    UnresolvedReference,
    /// A required log-type assignment was absent or pointed at a disabled configuration.
    AssignmentInvalid,
    /// A factor named a known MFA Module but carried data that module cannot open.
    FactorDataInvalid,
}

impl fmt::Display for ContentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("backup content is invalid")
    }
}

impl StdError for ContentError {}

impl From<ContentError> for RestoreError {
    fn from(error: ContentError) -> Self {
        match error {
            ContentError::UnsupportedFormatVersion
            | ContentError::BackendMismatch
            | ContentError::ComponentUnavailable => Self::BackupIncompatible,
            _ => Self::BackupInvalid,
        }
    }
}
