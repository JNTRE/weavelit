//! Live, non-restorable persistence contract for normal-operation Audit terminal recovery.

use std::fmt;

use weavelit_server_database_authority::ServerDatabaseAuthority;
use weavelit_server_log::{
    AuditTerminalDeliveryAcknowledgement, AuditTerminalSupersessionDisposition,
};

use crate::DatabaseError;

/// Number of bytes in one pending terminal obligation identity.
pub const AUDIT_TERMINAL_OBLIGATION_IDENTIFIER_LENGTH: usize = 16;

/// Maximum opaque bytes retained for one exact terminal record and destination binding.
pub const MAX_AUDIT_TERMINAL_OBLIGATION_BYTES: usize =
    weavelit_server_log::MAX_AUDIT_TERMINAL_RECOVERY_BYTES;

/// Maximum opaque bytes retained for one append-only supersession disposition.
pub const MAX_AUDIT_TERMINAL_SUPERSESSION_DISPOSITION_BYTES: usize =
    weavelit_server_log::MAX_AUDIT_TERMINAL_SUPERSESSION_BYTES;

/// Largest ordered pending-obligation batch one read may return.
pub const MAX_AUDIT_TERMINAL_REPLAY_BATCH_SIZE: usize = 64;

/// Capability gating trusted terminal-obligation construction and persisted decoding.
pub struct AuditTerminalRecoveryPersistence {
    _private: (),
}

impl AuditTerminalRecoveryPersistence {
    /// Creates the capability for Server-owned database selection authority.
    #[must_use]
    pub const fn from_server_authority(_authority: &ServerDatabaseAuthority) -> Self {
        Self { _private: () }
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

/// Immutable opaque terminal record projection retained outside restorable application state.
#[derive(Clone, Eq, PartialEq)]
pub struct AuditTerminalObligation {
    identifier: AuditTerminalObligationIdentifier,
    projection: Box<[u8]>,
}

impl AuditTerminalObligation {
    /// Decodes one bounded obligation under Server-owned database authority.
    pub fn from_persisted(
        persistence: &AuditTerminalRecoveryPersistence,
        identifier: [u8; AUDIT_TERMINAL_OBLIGATION_IDENTIFIER_LENGTH],
        projection: impl Into<Box<[u8]>>,
    ) -> Result<Self, AuditTerminalRecoveryContractError> {
        let identifier =
            AuditTerminalObligationIdentifier::from_persisted(persistence, identifier)?;
        let projection = projection.into();
        if projection.is_empty() {
            return Err(AuditTerminalRecoveryContractError::EmptyProjection);
        }
        if projection.len() > MAX_AUDIT_TERMINAL_OBLIGATION_BYTES {
            return Err(AuditTerminalRecoveryContractError::ProjectionTooLarge);
        }
        Ok(Self {
            identifier,
            projection,
        })
    }

    /// Returns the exact terminal record identity used for acknowledgement.
    pub const fn identifier(&self) -> AuditTerminalObligationIdentifier {
        self.identifier
    }

    /// Returns the opaque projection without interpreting its Audit fields.
    pub fn projection(&self) -> &[u8] {
        &self.projection
    }
}

impl fmt::Debug for AuditTerminalObligation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuditTerminalObligation(REDACTED)")
    }
}

/// Validated transaction input for one constrained terminal supersession.
///
/// This value retains the exact validated original identity and opaque projection.
/// The original remains immutable and pending for late exact delivery. The disposition
/// and replacement recovery obligation are appended in the same transaction in which
/// the owning configuration workflow applies the replacement.
pub struct AuditTerminalSupersession {
    original_obligation: AuditTerminalObligation,
    disposition: AuditTerminalSupersessionDisposition,
    replacement_obligation: AuditTerminalObligation,
}

impl AuditTerminalSupersession {
    /// Retains the exact original with one validated disposition and distinct replacement.
    pub fn new(
        original: &AuditTerminalObligation,
        disposition: AuditTerminalSupersessionDisposition,
        replacement_obligation: AuditTerminalObligation,
    ) -> Result<Self, AuditTerminalRecoveryContractError> {
        if disposition.original_record_id() != original.identifier.as_bytes()
            || replacement_obligation.identifier == original.identifier
        {
            return Err(AuditTerminalRecoveryContractError::MismatchedSupersession);
        }
        Ok(Self {
            original_obligation: original.clone(),
            disposition,
            replacement_obligation,
        })
    }

    /// Returns the exact validated original obligation that must match persisted bytes.
    pub const fn original_obligation(&self) -> &AuditTerminalObligation {
        &self.original_obligation
    }

    /// Returns the append-only degraded-completeness disposition.
    pub const fn disposition(&self) -> &AuditTerminalSupersessionDisposition {
        &self.disposition
    }

    /// Returns the new Audit action's terminal recovery obligation.
    pub const fn replacement_obligation(&self) -> &AuditTerminalObligation {
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
///
/// A backend exposes this capability only inside the same serialized transaction
/// that applies the authoritative mutation. The transaction commits both the
/// mutation and obligation or neither; there is no standalone enqueue operation.
pub trait AuditTerminalRecoveryTransaction {
    /// Persists one previously absent immutable obligation in the current transaction.
    fn persist_audit_terminal_obligation(
        &mut self,
        obligation: &AuditTerminalObligation,
    ) -> Result<(), DatabaseError>;

    /// Appends one constrained supersession and its replacement Audit terminal obligation.
    ///
    /// The exact original must be the oldest active pending obligation, must import as a
    /// valid immutable terminal, and must have no prior disposition. Before mutation, the
    /// backend compares its stored identity and opaque projection bytes with
    /// `supersession.original_obligation()` and compares the stored retained binding with
    /// `supersession.disposition().original_binding()`. The caller reaches this boundary
    /// only after retained-binding repair is permanently unavailable, exact-session
    /// authorization, explicit confirmation, and replacement preflight. The backend
    /// appends the disposition and replacement obligation atomically with the replacement
    /// assignment applied by the owning configuration transaction. It never rewrites,
    /// removes, or acknowledges the original, which remains late-delivery eligible. Any
    /// identity, projection, or binding mismatch, malformed state, repeated disposition,
    /// or non-oldest request returns [`DatabaseError::InvalidState`] without mutation.
    fn append_audit_terminal_supersession(
        &mut self,
        supersession: &AuditTerminalSupersession,
    ) -> Result<(), DatabaseError>;
}

/// Runtime storage boundary for ordered replay and acknowledgement after commit.
pub trait AuditTerminalRecoveryStore {
    /// Lists active pending obligations in insertion order, oldest first.
    ///
    /// An original with an appended supersession disposition is excluded from this active
    /// sequence but remains available through the late-delivery sequence below.
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

    /// Acknowledges exactly the oldest eligible obligation using destination acknowledgement.
    ///
    /// The capability is constructible only after exact bound destination
    /// acknowledgement. An absent, already acknowledged, or non-oldest identity within
    /// its active or late-delivery sequence returns [`DatabaseError::InvalidState`]. A
    /// supersession disposition is never acknowledgement and cannot reach this method.
    fn acknowledge_audit_terminal_obligation(
        &mut self,
        acknowledgement: AuditTerminalDeliveryAcknowledgement,
    ) -> Result<(), DatabaseError>;
}

/// Payload-free rejection of an invalid obligation contract value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditTerminalRecoveryContractError {
    /// The obligation identity is the reserved all-zero value.
    InvalidIdentifier,
    /// The opaque terminal projection is empty.
    EmptyProjection,
    /// The opaque terminal projection exceeds its fixed bound.
    ProjectionTooLarge,
    /// The requested replay batch is zero or exceeds its fixed bound.
    InvalidBatchSize,
    /// A disposition or replacement obligation did not match its exact original.
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
    use weavelit_server_database_authority::ServerDatabaseAuthority;

    const SENSITIVE_VALUE: &str = "temporary-password=do-not-log";

    fn persistence() -> AuditTerminalRecoveryPersistence {
        AuditTerminalRecoveryPersistence::from_server_authority(&ServerDatabaseAuthority::new())
    }

    fn identifier() -> AuditTerminalObligationIdentifier {
        AuditTerminalObligationIdentifier::from_persisted(&persistence(), [1; 16]).unwrap()
    }

    #[test]
    fn identifiers_and_batch_sizes_enforce_nonzero_bounds() {
        assert_eq!(
            AuditTerminalObligationIdentifier::from_persisted(&persistence(), [0; 16]).unwrap_err(),
            AuditTerminalRecoveryContractError::InvalidIdentifier
        );
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
        assert_eq!(
            AuditTerminalReplayBatchSize::new(MAX_AUDIT_TERMINAL_REPLAY_BATCH_SIZE + 1)
                .unwrap_err(),
            AuditTerminalRecoveryContractError::InvalidBatchSize
        );
    }

    #[test]
    fn obligations_enforce_projection_bounds_and_redact_debug_and_errors() {
        let obligation = AuditTerminalObligation::from_persisted(
            &persistence(),
            *identifier().as_bytes(),
            SENSITIVE_VALUE.as_bytes().to_vec(),
        )
        .unwrap();
        assert_eq!(obligation.projection(), SENSITIVE_VALUE.as_bytes());
        assert_eq!(
            format!("{obligation:?}"),
            "AuditTerminalObligation(REDACTED)"
        );
        assert!(!format!("{:?}", obligation.identifier()).contains(SENSITIVE_VALUE));

        assert_eq!(
            AuditTerminalObligation::from_persisted(
                &persistence(),
                *identifier().as_bytes(),
                Vec::new(),
            )
            .unwrap_err(),
            AuditTerminalRecoveryContractError::EmptyProjection
        );
        let error = AuditTerminalObligation::from_persisted(
            &persistence(),
            *identifier().as_bytes(),
            vec![b'x'; MAX_AUDIT_TERMINAL_OBLIGATION_BYTES + 1],
        )
        .unwrap_err();
        assert_eq!(
            error,
            AuditTerminalRecoveryContractError::ProjectionTooLarge
        );
        assert!(!error.to_string().contains(SENSITIVE_VALUE));
    }

    #[test]
    fn supersession_requires_the_exact_original_and_a_distinct_replacement() {
        let persistence = persistence();
        let original = AuditTerminalObligation::from_persisted(
            &persistence,
            [1; 16],
            b"original-terminal".to_vec(),
        )
        .unwrap();
        let replacement = AuditTerminalObligation::from_persisted(
            &persistence,
            [2; 16],
            b"replacement-terminal".to_vec(),
        )
        .unwrap();
        let supersession =
            AuditTerminalSupersession::new(&original, disposition([1; 16]), replacement).unwrap();

        assert_eq!(supersession.original_obligation(), &original);
        assert_eq!(
            supersession.original_obligation().projection(),
            b"original-terminal"
        );
        assert_eq!(
            supersession.disposition().original_record_id(),
            original.identifier().as_bytes()
        );
        assert_eq!(
            supersession.disposition().completeness(),
            weavelit_server_log::AuditTerminalCompleteness::Degraded
        );
        assert_ne!(
            supersession.replacement_obligation().identifier(),
            original.identifier()
        );
        assert_eq!(
            format!("{supersession:?}"),
            "AuditTerminalSupersession(REDACTED)"
        );

        let mismatched = AuditTerminalSupersession::new(
            &original,
            disposition([3; 16]),
            AuditTerminalObligation::from_persisted(
                &persistence,
                [4; 16],
                b"replacement-terminal".to_vec(),
            )
            .unwrap(),
        )
        .unwrap_err();
        assert_eq!(
            mismatched,
            AuditTerminalRecoveryContractError::MismatchedSupersession
        );

        let same_identifier =
            AuditTerminalSupersession::new(&original, disposition([1; 16]), original.clone())
                .unwrap_err();
        assert_eq!(
            same_identifier,
            AuditTerminalRecoveryContractError::MismatchedSupersession
        );
        assert!(!same_identifier.to_string().contains(SENSITIVE_VALUE));
    }

    fn disposition(
        original_identifier: [u8; AUDIT_TERMINAL_OBLIGATION_IDENTIFIER_LENGTH],
    ) -> AuditTerminalSupersessionDisposition {
        let document = serde_json::json!({
            "version": 1,
            "original_obligation_identifier": original_identifier,
            "original_binding": { "identifier": vec![7; 16], "version": 3 },
            "reason": "destination_permanently_unavailable",
            "replacement_binding": { "identifier": vec![8; 16], "version": 1 },
            "completeness": "degraded",
            "original_state": "retained_pending_late_delivery"
        });
        AuditTerminalSupersessionDisposition::from_persisted(serde_json::to_vec(&document).unwrap())
            .unwrap()
    }
}
