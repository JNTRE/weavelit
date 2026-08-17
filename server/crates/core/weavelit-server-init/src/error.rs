//! Stable payload-free Init failure categories.
//!
//! Every failure inside this crate collapses to one of the Init Design's fixed
//! categories or to a shared lifecycle category. A raw Rust, dependency,
//! cryptographic, SQL, filesystem, or operating-system failure is converted at
//! the point it occurs, so none of them has a path to a client or a log.

use std::{error::Error as StdError, fmt};

use weavelit_server_lifecycle::{LifecycleError, WorkflowError};
use weavelit_server_recovery_key::RecoveryKeyPreparationError;

/// Stable payload-free Init failure category.
///
/// A malformed request, an unusable password, an unavailable Log Module, a
/// failed seal, and an inconsistent assembled state all collapse to
/// [`InitError::InitializationFailed`] with no distinguishing data, so a client
/// learns that Init did not complete and nothing about the value that stopped
/// it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum InitError {
    /// Finalization carried no recovery-key proof of possession.
    RecoveryKeyConfirmationRequired,
    /// The submitted proof did not match the checkpoint's expected proof.
    RecoveryKeyConfirmationInvalid,
    /// Request validation, secret protection, or state assembly stopped Init.
    InitializationFailed,
    /// The deployment is already sealed, so no Init operation may run.
    AlreadyInitialized,
    /// A composed shared lifecycle category rejected the request.
    Lifecycle(LifecycleError),
}

impl InitError {
    /// Returns the stable category/reason pair for centralized error presentation.
    #[must_use]
    pub const fn category_reason(&self) -> (&'static str, &'static str) {
        match self {
            Self::RecoveryKeyConfirmationRequired => (
                "recovery_key_confirmation_required",
                "recovery_key_confirmation_required",
            ),
            Self::RecoveryKeyConfirmationInvalid => (
                "recovery_key_confirmation_invalid",
                "recovery_key_confirmation_invalid",
            ),
            Self::InitializationFailed => ("initialization_failed", "initialization_failed"),
            Self::AlreadyInitialized => ("deployment_state_invalid", "already_initialized"),
            Self::Lifecycle(error) => lifecycle_category_reason(*error),
        }
    }
}

impl fmt::Display for InitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.category_reason().0)
    }
}

impl StdError for InitError {}

impl From<LifecycleError> for InitError {
    fn from(error: LifecycleError) -> Self {
        Self::Lifecycle(error)
    }
}

/// The lifecycle authority's sealed answer keeps its identity.
///
/// Init must report `AlreadyInitialized` for a direct in-process call against a
/// sealed deployment, so that one workflow result is carried through instead of
/// being folded into the generic invalid-state category with every other
/// refusal.
impl From<WorkflowError> for InitError {
    fn from(error: WorkflowError) -> Self {
        match error {
            WorkflowError::AlreadyInitialized => Self::AlreadyInitialized,
            WorkflowError::Lifecycle(error) => Self::Lifecycle(error),
            _ => Self::Lifecycle(LifecycleError::InvalidState),
        }
    }
}

/// Recovery-key preparation failures carry no cryptographic detail into Init.
impl From<RecoveryKeyPreparationError> for InitError {
    fn from(_: RecoveryKeyPreparationError) -> Self {
        Self::InitializationFailed
    }
}

const fn lifecycle_category_reason(error: LifecycleError) -> (&'static str, &'static str) {
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

/// Invalid submitted Init request rejected before any state is created.
///
/// Variants exist for validation-order attribution inside the workspace. Their
/// display representation is uniform, and every variant reaches a client only
/// as [`InitError::InitializationFailed`], so no variant tells a caller which
/// part of a submitted request was wrong.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestError {
    /// A submitted secret was empty.
    SecretEmpty,
    /// A submitted secret exceeded its accepted bound.
    SecretTooLong,
    /// A collection was empty or exceeded its accepted entry bound.
    CollectionOutOfBounds,
    /// Two entries shared an identity that must be unique.
    DuplicateEntry,
    /// A named Log Module is not compiled into this build.
    ComponentUnavailable,
    /// A configuration carried a setting its Log Module does not declare.
    SettingUnsupported,
    /// A log-type assignment named no submitted configuration.
    UnresolvedAssignment,
    /// A log-type assignment named a disabled configuration.
    DisabledAssignment,
}

impl fmt::Display for RequestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("initialization request is invalid")
    }
}

impl StdError for RequestError {}

impl From<RequestError> for InitError {
    fn from(_: RequestError) -> Self {
        Self::InitializationFailed
    }
}

/// Invalid stored Init checkpoint metadata rejected before proof comparison.
///
/// Variants exist for validation-order attribution inside the workspace. Their
/// display representation is uniform, and every variant reaches a client only
/// as [`InitError::InitializationFailed`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckpointError {
    /// The metadata did not carry the supported checkpoint format version.
    UnsupportedFormatVersion,
    /// The metadata was truncated, overlong, or otherwise not the fixed layout.
    Malformed,
    /// The encoded recipient was not a canonical recovery public key.
    RecipientInvalid,
}

impl fmt::Display for CheckpointError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("initialization checkpoint is invalid")
    }
}

impl StdError for CheckpointError {}

impl From<CheckpointError> for InitError {
    fn from(_: CheckpointError) -> Self {
        Self::InitializationFailed
    }
}
