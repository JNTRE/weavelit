#![forbid(unsafe_code)]

//! Backend-neutral persistence contract for the Weavelit Application Database.

mod session;
mod state;

pub use session::{
    MAX_SESSION_INSTANT_MILLISECONDS, NewSession, SESSION_ABSOLUTE_LIFETIME_MILLISECONDS,
    SESSION_DIGEST_LENGTH, SESSION_IDLE_TIMEOUT_MILLISECONDS, SessionCsrfHash, SessionInstant,
    SessionRejection, SessionStore, SessionTokenHash, SessionValidation, StoredSession,
};
pub use state::{
    Account, AccountPasswordVerifier, ApplicationState, ApplicationStateInput, BoundedText,
    COMPONENT_ENABLED_VALUE, CompletionObligation, ComponentEnablement, ComponentKind,
    ConfigurationEntry, ConfigurationKey, ConfigurationValue, CorrelationIdentifier, Description,
    Group, GroupGrant, GroupGrantRecord, GroupMembership, HumanAuthorizationSnapshot,
    InitializedState, LogAssignment, LogClassification, LogDetail, LogModuleConfiguration,
    LogModuleSetting, LogType, MAX_CONFIGURATION_KEY_LENGTH, MAX_CONFIGURATION_VALUE_LENGTH,
    MAX_DESCRIPTION_LENGTH, MAX_LOG_CLASSIFICATION_LENGTH, MAX_LOG_CORRELATION_IDENTIFIER_LENGTH,
    MAX_LOG_DETAIL_LENGTH, MAX_NAME_LENGTH, MAX_PASSWORD_VERIFIER_LENGTH,
    MAX_PROTECTED_VALUE_LENGTH, MAX_RECOVERY_PUBLIC_KEY_LENGTH, MfaFactor, Name, PasswordVerifier,
    ProtectedSecret, ProtectedValue, RecoveryPublicKey, STATE_IDENTIFIER_LENGTH, ServiceConnection,
    StateIdentifier,
};

use std::{error::Error as StdError, fmt};

/// Number of bytes in a deployment identifier.
pub const DEPLOYMENT_IDENTIFIER_LENGTH: usize = 16;

/// Maximum encoded checkpoint metadata accepted by the persistence contract.
pub const MAX_CHECKPOINT_METADATA_LENGTH: usize = 4 * 1024;

/// Opaque identifier binding durable application state to one Server deployment.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct DeploymentIdentifier([u8; DEPLOYMENT_IDENTIFIER_LENGTH]);

impl DeploymentIdentifier {
    /// Creates an identifier from trusted binary representation.
    pub fn from_bytes(
        bytes: [u8; DEPLOYMENT_IDENTIFIER_LENGTH],
    ) -> Result<Self, ContractInputError> {
        if bytes == [0; DEPLOYMENT_IDENTIFIER_LENGTH] {
            return Err(ContractInputError::InvalidDeploymentIdentifier);
        }

        Ok(Self(bytes))
    }

    /// Returns the identifier's binary representation.
    pub const fn as_bytes(&self) -> &[u8; DEPLOYMENT_IDENTIFIER_LENGTH] {
        &self.0
    }
}

impl TryFrom<[u8; DEPLOYMENT_IDENTIFIER_LENGTH]> for DeploymentIdentifier {
    type Error = ContractInputError;

    fn try_from(bytes: [u8; DEPLOYMENT_IDENTIFIER_LENGTH]) -> Result<Self, Self::Error> {
        Self::from_bytes(bytes)
    }
}

impl fmt::Debug for DeploymentIdentifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DeploymentIdentifier(REDACTED)")
    }
}

/// Init or Restore workflow that owns a pending checkpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkflowKind {
    /// New application-state initialization.
    Init,
    /// Restoration from a validated backup.
    Restore,
}

/// Immutable workflow-owned checkpoint metadata stored without interpretation.
#[derive(Clone, Eq, PartialEq)]
pub struct CheckpointMetadata(Box<[u8]>);

impl CheckpointMetadata {
    /// Creates bounded metadata from its workflow-defined encoding.
    pub fn from_bytes(bytes: impl Into<Box<[u8]>>) -> Result<Self, ContractInputError> {
        let bytes = bytes.into();
        if bytes.len() > MAX_CHECKPOINT_METADATA_LENGTH {
            return Err(ContractInputError::CheckpointMetadataTooLarge);
        }

        Ok(Self(bytes))
    }

    /// Returns the opaque encoded metadata.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for CheckpointMetadata {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CheckpointMetadata")
            .field("length", &self.0.len())
            .finish_non_exhaustive()
    }
}

/// Complete expected identity of a pending non-operational workflow checkpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkflowCheckpoint {
    deployment_identifier: DeploymentIdentifier,
    workflow: WorkflowKind,
    metadata: CheckpointMetadata,
}

impl WorkflowCheckpoint {
    /// Creates a checkpoint value after the owning workflow validates its metadata.
    pub const fn new(
        deployment_identifier: DeploymentIdentifier,
        workflow: WorkflowKind,
        metadata: CheckpointMetadata,
    ) -> Self {
        Self {
            deployment_identifier,
            workflow,
            metadata,
        }
    }

    /// Returns the deployment identifier bound to the checkpoint.
    pub const fn deployment_identifier(&self) -> DeploymentIdentifier {
        self.deployment_identifier
    }

    /// Returns the workflow that owns the checkpoint.
    pub const fn workflow(&self) -> WorkflowKind {
        self.workflow
    }

    /// Returns the workflow-defined metadata without interpreting it.
    pub const fn metadata(&self) -> &CheckpointMetadata {
        &self.metadata
    }
}

/// Durable state observed through the Application Database contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DatabaseInspection {
    /// No workflow or initialized application state is present.
    Uninitialized,
    /// A non-operational Init or Restore checkpoint is present.
    Pending(WorkflowCheckpoint),
    /// Complete application state is bound to the deployment identifier.
    Initialized {
        /// Deployment identifier bound to the initialized state.
        deployment_identifier: DeploymentIdentifier,
    },
}

/// Backend-neutral Application Database operations available before operation.
pub trait ApplicationDatabase: Send {
    /// Inspects durable state and rejects state bound to another deployment.
    fn inspect(
        &mut self,
        expected_deployment_identifier: DeploymentIdentifier,
    ) -> Result<DatabaseInspection, DatabaseError>;

    /// Atomically creates a checkpoint in an eligible uninitialized database.
    fn create_checkpoint(&mut self, checkpoint: &WorkflowCheckpoint) -> Result<(), DatabaseError>;

    /// Atomically replaces the exact pending checkpoint with complete state once.
    fn complete_checkpoint(
        &mut self,
        checkpoint: &WorkflowCheckpoint,
        state: &ApplicationState,
    ) -> Result<(), DatabaseError>;

    /// Loads complete initialized state bound to the expected deployment.
    fn load_initialized_state(
        &mut self,
        expected_deployment_identifier: DeploymentIdentifier,
    ) -> Result<InitializedState, DatabaseError>;

    /// Marks the persisted completion obligation acknowledged exactly once.
    fn acknowledge_completion(
        &mut self,
        expected_deployment_identifier: DeploymentIdentifier,
        record_identifier: StateIdentifier,
    ) -> Result<(), DatabaseError>;

    /// Loads only what authorizing one account's request requires.
    ///
    /// This runs on every authorized request, so it is a deliberately narrow
    /// read rather than a projection of loaded application state: it returns
    /// the account's active flag and the Group grants joined from that
    /// account's memberships, and nothing else. An account the database does
    /// not hold returns `None` rather than an empty grant set, so an unknown
    /// account and a granted-nothing account are not the same value.
    fn load_human_authorization(
        &mut self,
        account: StateIdentifier,
    ) -> Result<Option<HumanAuthorizationSnapshot>, DatabaseError>;

    /// Loads only which components an administrator has disabled.
    ///
    /// This runs on every authorized request beside the account's grants, so
    /// it is a deliberately narrow read: it returns the disabled components
    /// and nothing else. It is never captured at startup or carried in a
    /// session, so disabling a component takes effect on the next request.
    fn load_component_enablement(&mut self) -> Result<ComponentEnablement, DatabaseError>;

    /// Returns this database's live session store, when it owns one.
    ///
    /// Live sessions are a separate contract from restorable application
    /// state, so a backend answers for them separately. The method is required
    /// rather than defaulted: a backend must state that it serves no session
    /// store, and a caller that receives `None` refuses the request instead of
    /// silently authenticating without durable sessions.
    fn sessions(&mut self) -> Option<&mut dyn SessionStore>;

    /// Closes the database and releases its storage cleanly.
    ///
    /// Taking the box consumes the only handle to the backend, so an operation
    /// against a closed database is not a value a caller can construct. The
    /// method is required rather than defaulted so a backend states how it
    /// releases storage instead of inheriting a silent success it may not be
    /// able to support, and it returns the failure rather than reporting a
    /// clean stop it did not achieve.
    fn close(self: Box<Self>) -> Result<(), DatabaseError>;
}

/// Invalid caller-provided value rejected before persistence access.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContractInputError {
    /// The deployment identifier is the reserved all-zero value.
    InvalidDeploymentIdentifier,
    /// The encoded checkpoint metadata exceeds the contract limit.
    CheckpointMetadataTooLarge,
    /// The entity identifier is the reserved all-zero value.
    InvalidStateIdentifier,
    /// A required text value is empty.
    TextEmpty,
    /// A text value exceeds its contract limit.
    TextTooLong,
    /// A text value contains control characters.
    TextNotPrintable,
    /// A protected value is empty.
    ProtectedValueEmpty,
    /// A protected value exceeds the contract limit.
    ProtectedValueTooLarge,
    /// The encoded password verifier is not a bounded PHC string.
    InvalidPasswordVerifier,
    /// The encoded recovery public key is not canonical.
    InvalidRecoveryPublicKey,
    /// The session or CSRF digest is the reserved all-zero value.
    InvalidSessionDigest,
    /// The session instant is negative or outside the accepted range.
    InvalidSessionInstant,
    /// The completion-record event time is negative.
    InvalidEventTime,
    /// Two state entries share an identifier or unique key.
    DuplicateEntry,
    /// A state entry references an absent entity.
    UnknownReference,
    /// A required log type has no assignment.
    MissingAssignment,
    /// An assignment references a disabled Log Module configuration.
    DisabledAssignment,
}

impl fmt::Display for ContractInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidDeploymentIdentifier => "deployment identifier is invalid",
            Self::CheckpointMetadataTooLarge => "checkpoint metadata is too large",
            Self::InvalidStateIdentifier => "state identifier is invalid",
            Self::TextEmpty => "text value is empty",
            Self::TextTooLong => "text value is too long",
            Self::TextNotPrintable => "text value is not printable",
            Self::ProtectedValueEmpty => "protected value is empty",
            Self::ProtectedValueTooLarge => "protected value is too large",
            Self::InvalidPasswordVerifier => "password verifier is invalid",
            Self::InvalidRecoveryPublicKey => "recovery public key is invalid",
            Self::InvalidSessionDigest => "session digest is invalid",
            Self::InvalidSessionInstant => "session instant is invalid",
            Self::InvalidEventTime => "completion event time is invalid",
            Self::DuplicateEntry => "application state contains a duplicate entry",
            Self::UnknownReference => "application state contains an unknown reference",
            Self::MissingAssignment => "application state is missing a log assignment",
            Self::DisabledAssignment => "application state assigns a disabled log module",
        };
        formatter.write_str(message)
    }
}

impl StdError for ContractInputError {}

/// Stable storage-neutral failure categories returned by every backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DatabaseError {
    /// Complete application state already exists.
    AlreadyInitialized,
    /// An operation requires initialized application state that is absent.
    NotInitialized,
    /// Persisted state or the requested transition is not valid.
    InvalidState,
    /// Durable state is bound to another Server deployment.
    DeploymentMismatch,
    /// Backend configuration is missing, unsafe, or invalid.
    ConfigurationInvalid,
    /// Valid storage cannot currently be opened, locked, queried, or changed.
    Unavailable,
    /// Persisted state, schema, or migration integrity cannot be trusted.
    IntegrityFailure,
}

impl fmt::Display for DatabaseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::AlreadyInitialized => "application database is already initialized",
            Self::NotInitialized => "application database is not initialized",
            Self::InvalidState => "application database state is invalid",
            Self::DeploymentMismatch => "application database deployment does not match",
            Self::ConfigurationInvalid => "application database configuration is invalid",
            Self::Unavailable => "application database is unavailable",
            Self::IntegrityFailure => "application database integrity validation failed",
        };
        formatter.write_str(message)
    }
}

impl StdError for DatabaseError {}

#[cfg(test)]
mod tests {
    use super::*;

    const SENSITIVE_VALUE: &str = "/private/database.sqlite?token=secret";

    fn deployment_identifier() -> DeploymentIdentifier {
        DeploymentIdentifier::from_bytes([1; DEPLOYMENT_IDENTIFIER_LENGTH]).unwrap()
    }

    #[test]
    fn deployment_identifier_rejects_reserved_zero_value() {
        let error = DeploymentIdentifier::from_bytes([0; DEPLOYMENT_IDENTIFIER_LENGTH])
            .expect_err("the all-zero identifier must be rejected");

        assert_eq!(error, ContractInputError::InvalidDeploymentIdentifier);
        assert!(!error.to_string().contains(SENSITIVE_VALUE));
    }

    #[test]
    fn checkpoint_metadata_accepts_empty_and_maximum_values() {
        let empty = CheckpointMetadata::from_bytes([]).unwrap();
        let maximum =
            CheckpointMetadata::from_bytes(vec![7; MAX_CHECKPOINT_METADATA_LENGTH]).unwrap();

        assert!(empty.as_bytes().is_empty());
        assert_eq!(maximum.as_bytes().len(), MAX_CHECKPOINT_METADATA_LENGTH);
    }

    #[test]
    fn checkpoint_metadata_rejects_oversized_values_without_exposing_them() {
        let mut bytes = SENSITIVE_VALUE.as_bytes().to_vec();
        bytes.resize(MAX_CHECKPOINT_METADATA_LENGTH + 1, b'x');

        let error = CheckpointMetadata::from_bytes(bytes)
            .expect_err("oversized checkpoint metadata must be rejected");

        assert_eq!(error, ContractInputError::CheckpointMetadataTooLarge);
        assert!(!error.to_string().contains(SENSITIVE_VALUE));
    }

    #[test]
    fn inspection_represents_every_preoperational_state() {
        let identifier = deployment_identifier();
        let metadata = CheckpointMetadata::from_bytes(b"workflow-owned".as_slice()).unwrap();

        let states = [
            DatabaseInspection::Uninitialized,
            DatabaseInspection::Pending(WorkflowCheckpoint::new(
                identifier,
                WorkflowKind::Init,
                metadata.clone(),
            )),
            DatabaseInspection::Pending(WorkflowCheckpoint::new(
                identifier,
                WorkflowKind::Restore,
                metadata,
            )),
            DatabaseInspection::Initialized {
                deployment_identifier: identifier,
            },
        ];

        assert!(matches!(states[0], DatabaseInspection::Uninitialized));
        assert!(matches!(
            &states[1],
            DatabaseInspection::Pending(checkpoint) if checkpoint.workflow() == WorkflowKind::Init
        ));
        assert!(matches!(
            &states[2],
            DatabaseInspection::Pending(checkpoint) if checkpoint.workflow() == WorkflowKind::Restore
        ));
        assert!(matches!(states[3], DatabaseInspection::Initialized { .. }));
    }

    #[test]
    fn opaque_values_are_redacted_from_debug_output() {
        let identifier = DeploymentIdentifier::from_bytes(*b"sensitive-value!").unwrap();
        let metadata = CheckpointMetadata::from_bytes(SENSITIVE_VALUE.as_bytes()).unwrap();
        let checkpoint = WorkflowCheckpoint::new(identifier, WorkflowKind::Init, metadata);
        let output = format!("{checkpoint:?}");

        assert!(!output.contains("sensitive-value"));
        assert!(!output.contains(SENSITIVE_VALUE));
    }

    #[test]
    fn database_errors_have_stable_redacted_messages() {
        let errors = [
            DatabaseError::AlreadyInitialized,
            DatabaseError::NotInitialized,
            DatabaseError::InvalidState,
            DatabaseError::DeploymentMismatch,
            DatabaseError::ConfigurationInvalid,
            DatabaseError::Unavailable,
            DatabaseError::IntegrityFailure,
        ];

        for error in errors {
            let message = error.to_string();
            assert!(message.starts_with("application database"));
            assert!(!message.contains(SENSITIVE_VALUE));
            assert!(!message.contains("sqlite"));
            assert!(!message.contains("SELECT"));
        }
    }
}
