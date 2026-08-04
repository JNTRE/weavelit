#![forbid(unsafe_code)]

//! Backend-neutral persistence contract for the Weavelit Application Database.

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
}

/// Invalid caller-provided value rejected before persistence access.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContractInputError {
    /// The deployment identifier is the reserved all-zero value.
    InvalidDeploymentIdentifier,
    /// The encoded checkpoint metadata exceeds the contract limit.
    CheckpointMetadataTooLarge,
}

impl fmt::Display for ContractInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidDeploymentIdentifier => "deployment identifier is invalid",
            Self::CheckpointMetadataTooLarge => "checkpoint metadata is too large",
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
