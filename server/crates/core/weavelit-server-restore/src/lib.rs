#![forbid(unsafe_code)]

//! Server-owned validation of an encrypted Weavelit backup.
//!
//! This crate owns backup-specific request normalization, envelope and
//! cryptographic validation, compatibility checks, and restored-state
//! transformation. It does not own startup classification, the deployment
//! record, the database locator, Application Database selection, lifecycle
//! sealing, or client presentation; it receives the validated lifecycle
//! authority and selected-database binding as inbound values.

mod bounds;
mod content;
mod crypto;
mod envelope;
mod error;
mod state;
mod ticket;
#[cfg(test)]
mod vectors;

pub use bounds::{
    MAX_AUTHENTICATED_PLAINTEXT_BYTES, MAX_CONCURRENT_RESTORE_OPERATIONS,
    MAX_ENCRYPTED_ARTIFACT_BYTES, RequestBudget, RequestDeadline, RestoreConcurrency, RestoreSlot,
    TOTAL_REQUEST_DEADLINE, TransferBounds, UPLOAD_DEADLINE, check_total_elapsed,
    check_upload_elapsed,
};
pub use content::{
    BACKUP_CONTENT_FORMAT_VERSION, BackupMfaFactor, BackupProtectedSecret, BackupServiceConnection,
    MAX_COLLECTION_ENTRIES, MAX_LOG_MODULE_SETTINGS, MAX_SENSITIVE_VALUE_BYTES, NormalizedBackup,
    SensitiveBytes, normalize,
};
pub use envelope::{BACKUP_FORMAT_VERSION, BACKUP_MAGIC, Envelope, HEADER_LENGTH};
pub use error::{ContentError, EnvelopeError, RestoreError};
pub use state::build_application_state;
pub use ticket::{
    RESTORE_TICKET_ENTROPY_BYTES, RESTORE_TICKET_TEXT_BYTES, RestoreTicket, RestoreTicketDigest,
};
pub use weavelit_server_components::{AvailableComponents, LogSettingsFormat, MfaFactorFormat};
pub use weavelit_server_database::{
    Account, AccountPasswordVerifier, ConfigurationEntry, ConfigurationKey, DeploymentIdentifier,
    Group, GroupGrant, GroupGrantRecord, GroupMembership, LogAssignment, LogModuleConfiguration,
    LogModuleSetting, LogType, Name, PasswordVerifier, RecoveryPublicKey, StateIdentifier,
};
pub use weavelit_server_lifecycle::{BackendIdentifier, LifecycleError};
pub use weavelit_server_recovery_key::{
    IDENTITY_PREFIX, MAX_RECOVERY_KEY_LENGTH, RECIPIENT_PREFIX, RecoveryIdentity, RecoveryKey,
    RecoveryKeyError, RecoveryRecipient,
};

/// Trusted lifecycle eligibility and selected-database binding for one Restore.
///
/// The runtime implements this over the lifecycle authority. Every mutating
/// Restore operation calls it before reading a private recovery key or backup
/// content, so an initialized deployment, an existing checkpoint, or an
/// unavailable or mismatched database is rejected before sensitive input
/// processing.
pub trait RestoreAuthority {
    /// Rechecks Restore eligibility under the lifecycle mutation permit.
    fn authorize(&self) -> Result<RestoreTarget, RestoreError>;
}

/// Trusted replacement deployment and selected Application Database backend.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestoreTarget {
    deployment_identifier: DeploymentIdentifier,
    selected_backend: BackendIdentifier,
}

impl RestoreTarget {
    /// Creates the trusted binding a lifecycle authority hands to Restore.
    pub const fn new(
        deployment_identifier: DeploymentIdentifier,
        selected_backend: BackendIdentifier,
    ) -> Self {
        Self {
            deployment_identifier,
            selected_backend,
        }
    }

    /// Returns the replacement deployment identifier.
    pub const fn deployment_identifier(&self) -> DeploymentIdentifier {
        self.deployment_identifier
    }

    /// Returns the selected Application Database backend.
    pub const fn selected_backend(&self) -> &BackendIdentifier {
        &self.selected_backend
    }
}

/// Normalized Restore request held entirely in bounded transient memory.
pub struct RestoreRequest<'request> {
    /// Encrypted backup artifact exactly as submitted.
    pub artifact: &'request [u8],
    /// Submitted private recovery key line.
    pub recovery_key: &'request str,
}

impl std::fmt::Debug for RestoreRequest<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RestoreRequest")
            .field("artifact_length", &self.artifact.len())
            .finish_non_exhaustive()
    }
}

/// Validated backup bound to the replacement deployment identifier.
///
/// The private recovery key and unwrapped data key have been cleared; only the
/// recovery public key is retained for future backups.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedBackup {
    deployment_identifier: DeploymentIdentifier,
    backup: NormalizedBackup,
}

impl ValidatedBackup {
    /// Returns the replacement deployment identifier this backup is bound to.
    pub const fn deployment_identifier(&self) -> DeploymentIdentifier {
        self.deployment_identifier
    }

    /// Returns the normalized backup contents.
    pub const fn backup(&self) -> &NormalizedBackup {
        &self.backup
    }

    /// Consumes the result and returns the normalized backup contents.
    pub fn into_backup(self) -> NormalizedBackup {
        self.backup
    }
}

/// Restore validation front half, configured with its approved bounds.
#[derive(Debug)]
pub struct RestoreValidator {
    components: AvailableComponents,
    bounds: TransferBounds,
    concurrency: RestoreConcurrency,
}

impl RestoreValidator {
    /// Creates a validator with the Security Model's approved transfer bounds.
    pub fn new(components: AvailableComponents) -> Self {
        Self::with_bounds(components, TransferBounds::APPROVED)
    }

    /// Creates a validator with explicit transfer bounds.
    pub fn with_bounds(components: AvailableComponents, bounds: TransferBounds) -> Self {
        Self {
            components,
            bounds,
            concurrency: RestoreConcurrency::new(),
        }
    }

    /// Runs the Restore Design's fixed validation order through normalization.
    ///
    /// Steps 1 through 9 run in order and mutate no state. Step 10 clears the
    /// private recovery key, the unwrapped data key, and the decrypted
    /// plaintext, retaining only the recovery public key inside the returned
    /// normalized backup. Checkpoint creation and atomic replacement are not
    /// part of this boundary.
    ///
    /// The budget is rechecked once more before success is reported, so work
    /// that crossed the total deadline is answered as a failure rather than
    /// returned as a result the caller would then commit.
    pub fn validate(
        &self,
        authority: &dyn RestoreAuthority,
        budget: &dyn RequestDeadline,
        request: RestoreRequest<'_>,
    ) -> Result<ValidatedBackup, RestoreError> {
        // 1. Exclusive Restore permit, then lifecycle eligibility.
        let _slot = self.concurrency.try_acquire()?;
        let target = authority.authorize()?;

        // 2. Configured transfer bounds.
        budget.check()?;
        self.bounds.check_artifact(request.artifact.len())?;

        // 3. Fixed outer header and its exact declared length.
        let envelope = Envelope::parse(request.artifact)?;

        // 4. Canonical recovery-key syntax.
        let identity = RecoveryKey::parse(request.recovery_key)?.into_identity()?;

        // 5-7. Age parameter policy, authenticated decryption, plaintext bounds.
        budget.check()?;
        let plaintext = crypto::decrypt_payload(envelope.payload(), &identity, self.bounds)?;

        // 8-9. Compatibility, references, components, domain semantics, and the
        // recipient binding below.
        budget.check()?;
        let backup = normalize(&plaintext, target.selected_backend(), &self.components)?;

        // The retained recovery public key must be the submitted identity's own
        // recipient. Without this the backup could declare any syntactically
        // valid recipient, and every future backup would be encrypted to a
        // private key the operator may not hold. A declared key that is not a
        // canonical recipient and one that belongs to another identity are both
        // reported as an invalid backup, indistinguishable from a wrong key.
        let declared = match RecoveryKey::parse(backup.recovery_public_key().as_str()) {
            Ok(RecoveryKey::Recipient(recipient)) => recipient,
            Ok(RecoveryKey::Identity(_)) | Err(_) => return Err(RestoreError::BackupInvalid),
        };
        if declared != identity.recipient() {
            return Err(RestoreError::BackupInvalid);
        }

        // 10. Clear the private recovery key, unwrapped data key, and plaintext.
        drop(identity);
        drop(plaintext);

        // Normalization and the recipient binding are the last work the budget
        // guards, and nothing above rechecks it once they have run. The caller
        // commits whatever this returns from an uncancellable chain, so a
        // result handed back after the deadline passed would let the deployment
        // be replaced after the request was already answered as timed out.
        budget.check()?;

        Ok(ValidatedBackup {
            deployment_identifier: target.deployment_identifier(),
            backup,
        })
    }
}
