//! Live, non-restorable persistence contract for normal-operation Audit terminal recovery.

use std::fmt;

use weavelit_server_database_authority::ServerDatabaseAuthority;

use crate::DatabaseError;

/// Number of bytes in one pending terminal obligation identity.
pub const AUDIT_TERMINAL_OBLIGATION_IDENTIFIER_LENGTH: usize = 16;

/// Maximum opaque bytes retained for one exact terminal record and destination binding.
pub const MAX_AUDIT_TERMINAL_OBLIGATION_BYTES: usize = 50_176;

/// Maximum opaque bytes retained for one append-only supersession disposition.
pub const MAX_AUDIT_TERMINAL_SUPERSESSION_DISPOSITION_BYTES: usize = 1_024;

/// Largest ordered pending-obligation batch one read may return.
pub const MAX_AUDIT_TERMINAL_REPLAY_BATCH_SIZE: usize = 64;

/// Capability gating trusted recovery writes, acknowledgements, and persisted decoding.
pub struct AuditTerminalRecoveryPersistence {
    _private: (),
}

impl AuditTerminalRecoveryPersistence {
    /// Creates the capability for Server-owned database selection authority.
    #[must_use]
    pub const fn from_server_authority(_authority: &ServerDatabaseAuthority) -> Self {
        Self { _private: () }
    }

    /// Returns opaque projection bytes to the trusted Server Audit importer.
    pub fn projection_bytes<'a>(&self, obligation: &'a AuditTerminalObligation) -> &'a [u8] {
        &obligation.projection.0
    }
}

impl fmt::Debug for AuditTerminalRecoveryPersistence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuditTerminalRecoveryPersistence(REDACTED)")
    }
}

/// Opaque identity of one terminal obligation, equal to its immutable Audit record identity.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AuditTerminalObligationIdentifier([u8; AUDIT_TERMINAL_OBLIGATION_IDENTIFIER_LENGTH]);

impl AuditTerminalObligationIdentifier {
    /// Accepts a nonzero persisted identity under Server-owned database authority.
    pub fn from_persisted(
        _persistence: &AuditTerminalRecoveryPersistence,
        bytes: [u8; AUDIT_TERMINAL_OBLIGATION_IDENTIFIER_LENGTH],
    ) -> Result<Self, AuditTerminalRecoveryContractError> {
        if bytes == [0; AUDIT_TERMINAL_OBLIGATION_IDENTIFIER_LENGTH] {
            return Err(AuditTerminalRecoveryContractError::InvalidIdentifier);
        }
        Ok(Self(bytes))
    }

    /// Returns the opaque bytes used for exact persistence and acknowledgement matching.
    pub const fn as_bytes(&self) -> &[u8; AUDIT_TERMINAL_OBLIGATION_IDENTIFIER_LENGTH] {
        &self.0
    }
}

impl fmt::Debug for AuditTerminalObligationIdentifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuditTerminalObligationIdentifier(REDACTED)")
    }
}

/// Opaque identity and version of one retained Audit destination binding.
#[derive(Clone, Eq, PartialEq)]
pub struct StoredAuditDestinationBinding {
    identifier: [u8; AUDIT_TERMINAL_OBLIGATION_IDENTIFIER_LENGTH],
    version: u64,
}

impl StoredAuditDestinationBinding {
    /// Accepts a nonzero persisted binding under Server-owned database authority.
    pub fn from_persisted(
        _persistence: &AuditTerminalRecoveryPersistence,
        identifier: [u8; AUDIT_TERMINAL_OBLIGATION_IDENTIFIER_LENGTH],
        version: u64,
    ) -> Result<Self, AuditTerminalRecoveryContractError> {
        if identifier == [0; AUDIT_TERMINAL_OBLIGATION_IDENTIFIER_LENGTH] || version == 0 {
            return Err(AuditTerminalRecoveryContractError::InvalidBinding);
        }
        Ok(Self {
            identifier,
            version,
        })
    }

    /// Returns the opaque persisted binding identity.
    pub const fn identifier(&self) -> &[u8; AUDIT_TERMINAL_OBLIGATION_IDENTIFIER_LENGTH] {
        &self.identifier
    }

    /// Returns the nonzero persisted binding version.
    pub const fn version(&self) -> u64 {
        self.version
    }
}

impl fmt::Debug for StoredAuditDestinationBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("StoredAuditDestinationBinding(REDACTED)")
    }
}

/// Bounded opaque terminal projection stored without field access or interpretation.
#[derive(Clone, Eq, PartialEq)]
pub struct OpaqueAuditTerminalProjection(Box<[u8]>);

impl OpaqueAuditTerminalProjection {
    fn from_persisted(
        bytes: impl Into<Box<[u8]>>,
    ) -> Result<Self, AuditTerminalRecoveryContractError> {
        let bytes = bytes.into();
        if bytes.is_empty() {
            return Err(AuditTerminalRecoveryContractError::EmptyProjection);
        }
        if bytes.len() > MAX_AUDIT_TERMINAL_OBLIGATION_BYTES {
            return Err(AuditTerminalRecoveryContractError::ProjectionTooLarge);
        }
        Ok(Self(bytes))
    }
}

impl fmt::Debug for OpaqueAuditTerminalProjection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OpaqueAuditTerminalProjection(REDACTED)")
    }
}

/// Bounded opaque supersession disposition stored without field interpretation.
pub struct OpaqueAuditTerminalDisposition(Box<[u8]>);

impl OpaqueAuditTerminalDisposition {
    fn from_validated(
        bytes: impl Into<Box<[u8]>>,
    ) -> Result<Self, AuditTerminalRecoveryContractError> {
        let bytes = bytes.into();
        if bytes.is_empty() {
            return Err(AuditTerminalRecoveryContractError::EmptyDisposition);
        }
        if bytes.len() > MAX_AUDIT_TERMINAL_SUPERSESSION_DISPOSITION_BYTES {
            return Err(AuditTerminalRecoveryContractError::DispositionTooLarge);
        }
        Ok(Self(bytes))
    }
}

impl fmt::Debug for OpaqueAuditTerminalDisposition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OpaqueAuditTerminalDisposition(REDACTED)")
    }
}

/// Immutable opaque terminal obligation loaded from private backend storage.
#[derive(Clone, Eq, PartialEq)]
pub struct AuditTerminalObligation {
    identifier: AuditTerminalObligationIdentifier,
    projection: OpaqueAuditTerminalProjection,
    binding: StoredAuditDestinationBinding,
}

impl AuditTerminalObligation {
    /// Decodes one bounded opaque stored row under Server-owned database authority.
    pub fn from_persisted(
        persistence: &AuditTerminalRecoveryPersistence,
        identifier: [u8; AUDIT_TERMINAL_OBLIGATION_IDENTIFIER_LENGTH],
        projection: impl Into<Box<[u8]>>,
        binding: StoredAuditDestinationBinding,
    ) -> Result<Self, AuditTerminalRecoveryContractError> {
        Ok(Self {
            identifier: AuditTerminalObligationIdentifier::from_persisted(persistence, identifier)?,
            projection: OpaqueAuditTerminalProjection::from_persisted(projection)?,
            binding,
        })
    }

    /// Returns the exact terminal record identity used for acknowledgement.
    pub const fn identifier(&self) -> AuditTerminalObligationIdentifier {
        self.identifier
    }

    /// Returns the separately stored destination binding.
    pub const fn binding(&self) -> &StoredAuditDestinationBinding {
        &self.binding
    }
}

impl fmt::Debug for AuditTerminalObligation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuditTerminalObligation(REDACTED)")
    }
}

/// Capability-gated input for an opaque obligation written with an owning mutation.
pub struct ValidatedAuditTerminalObligationWrite {
    obligation: AuditTerminalObligation,
}

impl ValidatedAuditTerminalObligationWrite {
    /// Captures bytes that Server Audit has semantically validated before persistence.
    pub fn from_server_audit(
        persistence: &AuditTerminalRecoveryPersistence,
        identifier: [u8; AUDIT_TERMINAL_OBLIGATION_IDENTIFIER_LENGTH],
        projection: impl Into<Box<[u8]>>,
        binding: StoredAuditDestinationBinding,
    ) -> Result<Self, AuditTerminalRecoveryContractError> {
        Ok(Self {
            obligation: AuditTerminalObligation::from_persisted(
                persistence,
                identifier,
                projection,
                binding,
            )?,
        })
    }

    /// Returns the opaque identifier for backend persistence.
    pub const fn identifier(&self) -> AuditTerminalObligationIdentifier {
        self.obligation.identifier
    }

    /// Returns the separately stored binding for backend persistence.
    pub const fn binding(&self) -> &StoredAuditDestinationBinding {
        &self.obligation.binding
    }

    /// Returns the exact opaque projection bytes for backend persistence.
    pub fn projection_bytes(&self) -> &[u8] {
        &self.obligation.projection.0
    }
}

impl fmt::Debug for ValidatedAuditTerminalObligationWrite {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ValidatedAuditTerminalObligationWrite(REDACTED)")
    }
}

/// Capability-gated proof of exact destination acknowledgement for one obligation.
pub struct AuditTerminalAcknowledgementProof {
    identifier: AuditTerminalObligationIdentifier,
    binding: StoredAuditDestinationBinding,
}

impl AuditTerminalAcknowledgementProof {
    /// Converts Server Audit's validated destination acknowledgement into database proof.
    pub fn from_server_audit(
        persistence: &AuditTerminalRecoveryPersistence,
        identifier: [u8; AUDIT_TERMINAL_OBLIGATION_IDENTIFIER_LENGTH],
        binding: StoredAuditDestinationBinding,
    ) -> Result<Self, AuditTerminalRecoveryContractError> {
        Ok(Self {
            identifier: AuditTerminalObligationIdentifier::from_persisted(persistence, identifier)?,
            binding,
        })
    }

    /// Returns the exact acknowledged obligation identity.
    pub const fn identifier(&self) -> AuditTerminalObligationIdentifier {
        self.identifier
    }

    /// Returns the exact acknowledged destination binding.
    pub const fn binding(&self) -> &StoredAuditDestinationBinding {
        &self.binding
    }
}

impl fmt::Debug for AuditTerminalAcknowledgementProof {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuditTerminalAcknowledgementProof(REDACTED)")
    }
}

/// Validated transaction input for one constrained terminal supersession.
pub struct AuditTerminalSupersession {
    original_obligation: AuditTerminalObligation,
    disposition: OpaqueAuditTerminalDisposition,
    original_binding: StoredAuditDestinationBinding,
    replacement_binding: StoredAuditDestinationBinding,
    replacement_obligation: ValidatedAuditTerminalObligationWrite,
}

impl AuditTerminalSupersession {
    /// Captures Server Audit's validated opaque disposition and exact bound obligations.
    pub fn from_server_audit(
        _persistence: &AuditTerminalRecoveryPersistence,
        original: &AuditTerminalObligation,
        disposition: impl Into<Box<[u8]>>,
        original_binding: StoredAuditDestinationBinding,
        replacement_binding: StoredAuditDestinationBinding,
        replacement_obligation: ValidatedAuditTerminalObligationWrite,
    ) -> Result<Self, AuditTerminalRecoveryContractError> {
        if original.binding != original_binding
            || replacement_obligation.binding() != &replacement_binding
            || original_binding == replacement_binding
            || replacement_obligation.identifier() == original.identifier
        {
            return Err(AuditTerminalRecoveryContractError::MismatchedSupersession);
        }
        Ok(Self {
            original_obligation: original.clone(),
            disposition: OpaqueAuditTerminalDisposition::from_validated(disposition)?,
            original_binding,
            replacement_binding,
            replacement_obligation,
        })
    }

    /// Returns the exact validated original obligation.
    pub const fn original_obligation(&self) -> &AuditTerminalObligation {
        &self.original_obligation
    }

    /// Returns the exact original projection bytes for backend comparison.
    pub fn original_projection_bytes(&self) -> &[u8] {
        &self.original_obligation.projection.0
    }

    /// Returns the opaque disposition bytes for append-only persistence.
    pub fn disposition_bytes(&self) -> &[u8] {
        &self.disposition.0
    }

    /// Returns the separately validated original binding.
    pub const fn original_binding(&self) -> &StoredAuditDestinationBinding {
        &self.original_binding
    }

    /// Returns the separately validated replacement binding.
    pub const fn replacement_binding(&self) -> &StoredAuditDestinationBinding {
        &self.replacement_binding
    }

    /// Returns the new Audit action's terminal recovery write.
    pub const fn replacement_obligation(&self) -> &ValidatedAuditTerminalObligationWrite {
        &self.replacement_obligation
    }
}

impl fmt::Debug for AuditTerminalSupersession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuditTerminalSupersession(REDACTED)")
    }
}

/// Validated bound for one oldest-first pending-obligation read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuditTerminalReplayBatchSize(u8);

impl AuditTerminalReplayBatchSize {
    /// Accepts a nonzero batch size no larger than the contract maximum.
    pub fn new(value: usize) -> Result<Self, AuditTerminalRecoveryContractError> {
        if value == 0 || value > MAX_AUDIT_TERMINAL_REPLAY_BATCH_SIZE {
            return Err(AuditTerminalRecoveryContractError::InvalidBatchSize);
        }
        Ok(Self(value as u8))
    }

    /// Returns the validated batch size.
    pub const fn get(self) -> usize {
        self.0 as usize
    }
}

/// Transaction-only insertion boundary for a consequential application-state mutation.
pub trait AuditTerminalRecoveryTransaction {
    /// Persists one validated immutable obligation in the current transaction.
    ///
    /// An exact retry is idempotent. The same identifier with different projection or
    /// binding bytes returns [`DatabaseError::InvalidState`] without mutation. Malformed
    /// persisted rows from the backend return [`DatabaseError::IntegrityFailure`].
    fn persist_audit_terminal_obligation(
        &mut self,
        obligation: &ValidatedAuditTerminalObligationWrite,
    ) -> Result<(), DatabaseError>;

    /// Atomically appends one constrained supersession and replacement obligation.
    ///
    /// An exact retry is idempotent. Every mismatch, partial prior write, or non-oldest
    /// original returns [`DatabaseError::InvalidState`] without mutation. Malformed rows
    /// from the backend return [`DatabaseError::IntegrityFailure`].
    fn append_audit_terminal_supersession(
        &mut self,
        supersession: &AuditTerminalSupersession,
    ) -> Result<(), DatabaseError>;
}

/// Runtime storage boundary for ordered replay and acknowledgement after commit.
pub trait AuditTerminalRecoveryStore {
    /// Lists active pending obligations in insertion order, oldest first.
    fn list_pending_audit_terminal_obligations(
        &mut self,
        persistence: &AuditTerminalRecoveryPersistence,
        batch_size: AuditTerminalReplayBatchSize,
    ) -> Result<Vec<AuditTerminalObligation>, DatabaseError>;

    /// Lists superseded originals still awaiting exact late delivery, oldest first.
    fn list_late_delivery_audit_terminal_obligations(
        &mut self,
        persistence: &AuditTerminalRecoveryPersistence,
        batch_size: AuditTerminalReplayBatchSize,
    ) -> Result<Vec<AuditTerminalObligation>, DatabaseError>;

    /// Acknowledges exactly the oldest eligible obligation using opaque database proof.
    fn acknowledge_audit_terminal_obligation(
        &mut self,
        acknowledgement: AuditTerminalAcknowledgementProof,
    ) -> Result<(), DatabaseError>;
}

/// Payload-free rejection of an invalid opaque recovery contract value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditTerminalRecoveryContractError {
    /// The obligation identity is the reserved all-zero value.
    InvalidIdentifier,
    /// A separately stored binding identity or version is zero.
    InvalidBinding,
    /// The opaque terminal projection is empty.
    EmptyProjection,
    /// The opaque terminal projection exceeds its fixed bound.
    ProjectionTooLarge,
    /// The opaque disposition is empty.
    EmptyDisposition,
    /// The opaque disposition exceeds its fixed bound.
    DispositionTooLarge,
    /// The requested replay batch is zero or exceeds its fixed bound.
    InvalidBatchSize,
    /// A disposition or replacement obligation did not match its exact bindings.
    MismatchedSupersession,
}

impl fmt::Display for AuditTerminalRecoveryContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Audit terminal recovery contract value is invalid")
    }
}

impl std::error::Error for AuditTerminalRecoveryContractError {}

#[cfg(test)]
mod tests {
    use super::*;

    const SENSITIVE_VALUE: &str = "temporary-password=do-not-log";

    fn persistence() -> AuditTerminalRecoveryPersistence {
        AuditTerminalRecoveryPersistence::from_server_authority(&ServerDatabaseAuthority::new())
    }

    fn binding(
        persistence: &AuditTerminalRecoveryPersistence,
        byte: u8,
    ) -> StoredAuditDestinationBinding {
        StoredAuditDestinationBinding::from_persisted(persistence, [byte; 16], 1).unwrap()
    }

    fn write(
        persistence: &AuditTerminalRecoveryPersistence,
        identifier: u8,
        projection: &[u8],
        binding_byte: u8,
    ) -> ValidatedAuditTerminalObligationWrite {
        ValidatedAuditTerminalObligationWrite::from_server_audit(
            persistence,
            [identifier; 16],
            projection.to_vec(),
            binding(persistence, binding_byte),
        )
        .unwrap()
    }

    #[test]
    fn opaque_values_enforce_bounds_and_redact_diagnostics() {
        let persistence = persistence();
        assert_eq!(
            AuditTerminalObligationIdentifier::from_persisted(&persistence, [0; 16]).unwrap_err(),
            AuditTerminalRecoveryContractError::InvalidIdentifier
        );
        assert_eq!(
            StoredAuditDestinationBinding::from_persisted(&persistence, [0; 16], 1).unwrap_err(),
            AuditTerminalRecoveryContractError::InvalidBinding
        );
        assert_eq!(
            StoredAuditDestinationBinding::from_persisted(&persistence, [1; 16], 0).unwrap_err(),
            AuditTerminalRecoveryContractError::InvalidBinding
        );
        assert_eq!(
            ValidatedAuditTerminalObligationWrite::from_server_audit(
                &persistence,
                [1; 16],
                Vec::new(),
                binding(&persistence, 2),
            )
            .unwrap_err(),
            AuditTerminalRecoveryContractError::EmptyProjection
        );
        assert_eq!(
            ValidatedAuditTerminalObligationWrite::from_server_audit(
                &persistence,
                [1; 16],
                vec![b'x'; MAX_AUDIT_TERMINAL_OBLIGATION_BYTES + 1],
                binding(&persistence, 2),
            )
            .unwrap_err(),
            AuditTerminalRecoveryContractError::ProjectionTooLarge
        );

        let write = write(&persistence, 1, SENSITIVE_VALUE.as_bytes(), 2);
        assert_eq!(write.projection_bytes(), SENSITIVE_VALUE.as_bytes());
        assert_eq!(
            format!("{write:?}"),
            "ValidatedAuditTerminalObligationWrite(REDACTED)"
        );
        assert!(!format!("{:?}", write.binding()).contains(SENSITIVE_VALUE));
    }

    #[test]
    fn supersession_requires_exact_distinct_bindings_and_bounded_disposition() {
        let persistence = persistence();
        let original = AuditTerminalObligation::from_persisted(
            &persistence,
            [1; 16],
            b"original".to_vec(),
            binding(&persistence, 7),
        )
        .unwrap();
        let replacement = write(&persistence, 2, b"replacement", 8);
        let supersession = AuditTerminalSupersession::from_server_audit(
            &persistence,
            &original,
            b"opaque-disposition".to_vec(),
            binding(&persistence, 7),
            binding(&persistence, 8),
            replacement,
        )
        .unwrap();
        assert_eq!(supersession.original_projection_bytes(), b"original");
        assert_eq!(supersession.disposition_bytes(), b"opaque-disposition");
        assert_eq!(
            format!("{supersession:?}"),
            "AuditTerminalSupersession(REDACTED)"
        );

        let error = AuditTerminalSupersession::from_server_audit(
            &persistence,
            &original,
            vec![b'x'; MAX_AUDIT_TERMINAL_SUPERSESSION_DISPOSITION_BYTES + 1],
            binding(&persistence, 7),
            binding(&persistence, 8),
            write(&persistence, 3, b"replacement", 8),
        )
        .unwrap_err();
        assert_eq!(
            error,
            AuditTerminalRecoveryContractError::DispositionTooLarge
        );
        assert!(!error.to_string().contains(SENSITIVE_VALUE));
    }

    #[test]
    fn batch_sizes_enforce_nonzero_bounds() {
        assert_eq!(AuditTerminalReplayBatchSize::new(1).unwrap().get(), 1);
        assert_eq!(
            AuditTerminalReplayBatchSize::new(MAX_AUDIT_TERMINAL_REPLAY_BATCH_SIZE)
                .unwrap()
                .get(),
            MAX_AUDIT_TERMINAL_REPLAY_BATCH_SIZE
        );
        assert_eq!(
            AuditTerminalReplayBatchSize::new(0).unwrap_err(),
            AuditTerminalRecoveryContractError::InvalidBatchSize
        );
    }

    #[test]
    fn supersession_mismatches_return_correct_error_for_all_binding_permutations() {
        let persistence = persistence();
        let original = AuditTerminalObligation::from_persisted(
            &persistence,
            [1; 16],
            b"original".to_vec(),
            binding(&persistence, 7),
        )
        .unwrap();

        // Mismatched replacement identifier: replacement obligation has same ID as original
        let error = AuditTerminalSupersession::from_server_audit(
            &persistence,
            &original,
            b"disposition".to_vec(),
            binding(&persistence, 7),
            binding(&persistence, 8),
            write(&persistence, 1, b"different-id-projection", 8),
        )
        .unwrap_err();
        assert_eq!(
            error,
            AuditTerminalRecoveryContractError::MismatchedSupersession
        );

        // Mismatched original binding: original.binding() != original_binding_arg
        let error = AuditTerminalSupersession::from_server_audit(
            &persistence,
            &original,
            b"disposition".to_vec(),
            binding(&persistence, 99), // Wrong original binding
            binding(&persistence, 8),
            write(&persistence, 2, b"replacement", 8),
        )
        .unwrap_err();
        assert_eq!(
            error,
            AuditTerminalRecoveryContractError::MismatchedSupersession
        );

        // Mismatched replacement binding: replacement_obligation.binding() != replacement_binding_arg
        let error = AuditTerminalSupersession::from_server_audit(
            &persistence,
            &original,
            b"disposition".to_vec(),
            binding(&persistence, 7),
            binding(&persistence, 99), // Wrong replacement binding
            write(&persistence, 2, b"replacement", 8), // write() binds to byte 8
        )
        .unwrap_err();
        assert_eq!(
            error,
            AuditTerminalRecoveryContractError::MismatchedSupersession
        );

        // Identical bindings: original_binding == replacement_binding
        let error = AuditTerminalSupersession::from_server_audit(
            &persistence,
            &original,
            b"disposition".to_vec(),
            binding(&persistence, 7),
            binding(&persistence, 7), // Same as original
            write(&persistence, 2, b"replacement", 7),
        )
        .unwrap_err();
        assert_eq!(
            error,
            AuditTerminalRecoveryContractError::MismatchedSupersession
        );
    }

    #[test]
    fn database_contract_has_no_log_crate_dependency() {
        assert!(!include_str!("../Cargo.toml").contains(concat!("weavelit-server-", "log")));
    }
}
