//! Server Audit: construction and synchronous delivery of pre-redacted Audit Log records.

#![forbid(unsafe_code)]

mod model;

pub use model::{
    AccountStatus, ActionOutcome, AuditActor, AuditEvent, AuditOutcomeDetail,
    AuditTerminalObligationReference, AutomationReference, BackupReference, ComponentReference,
    ComponentState, GrantReference, LogConfigurationAuditReferences, LogPolicyReference,
    MfaModuleChange, MfaModuleReference, MfaRequirement, MfaResetState, OperationReference,
    ServiceConnectionReference, StateChangeOutcome,
};

use core::fmt;

use weavelit_server_database::{
    AuditTerminalAcknowledgementProof, AuditTerminalObligation, AuditTerminalRecoveryPersistence,
    AuditTerminalRecoveryStore, AuditTerminalSupersession, DatabaseError,
    StoredAuditDestinationBinding, ValidatedAuditTerminalObligationWrite,
};
use weavelit_server_log::{
    AuditDestinationBinding, AuditDestinationBindingTransition, AuditTerminalRecoveryProjection,
    AuditTerminalReplayError, AuditTerminalSupersessionAuthorization,
    AuditTerminalSupersessionConfirmation, AuditTerminalSupersessionDisposition, CompleteLogRecord,
    ConfiguredLogDestination, CorrelationId, EventTime, LogDeliveryError, LogRecordPersistenceView,
    PreflightedAuditDestination, RecordId, RecoveredAuditTerminal as RecoveredLogAuditTerminal,
    ResolvedAuditDestination, TrustedRecordIssuer,
};
use weavelit_server_log_authority::ServerLogAuthority;

/// Maximum number of attempts to generate a nonzero record identifier before returning exhaustion.
const RECORD_ID_GENERATION_ATTEMPTS: usize = 8;

/// Payload-free failure to validate or construct an Audit record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditError {
    /// A safe reference was empty, malformed, oversized, or a database identifier.
    InvalidReference,
    /// The operating system could not supply record-identifier entropy.
    RandomnessUnavailable,
    /// The closed event and terminal outcome are not a valid pairing.
    InvalidOutcome,
    /// The shared complete-record contract rejected the prepared fields.
    InvalidRecord,
}

impl fmt::Display for AuditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => "Audit reference is invalid",
            Self::RandomnessUnavailable => "Audit record identity is unavailable",
            Self::InvalidOutcome => "Audit outcome is invalid for this event",
            Self::InvalidRecord => "Audit record is invalid",
        })
    }
}

impl core::error::Error for AuditError {}

/// Server-owned producer of complete, pre-redacted Audit Log records.
pub struct ServerAudit {
    record_issuer: TrustedRecordIssuer,
}

impl ServerAudit {
    /// Creates the producer from the Server-owned record issuer.
    #[must_use]
    pub const fn new(record_issuer: TrustedRecordIssuer) -> Self {
        Self { record_issuer }
    }

    /// Constructs one bounded result-less Audit Attempt.
    pub fn prepare_attempt(
        &self,
        event_time: EventTime,
        correlation_id: CorrelationId,
        actor: AuditActor,
        event: AuditEvent,
    ) -> Result<PreparedAuditAttempt, AuditError> {
        let body = event.body(
            &actor,
            AuditEvent::attempt_detail(),
            model::EventPhase::Attempt,
        )?;
        let record = CompleteLogRecord::audit_attempt(
            self.issue_record_id()?,
            event_time,
            correlation_id,
            body,
        )
        .map_err(|_| AuditError::InvalidRecord)?;
        Ok(PreparedAuditAttempt {
            record,
            actor,
            event,
        })
    }

    /// Constructs one authoritative Completion linked to a genuine Attempt.
    pub fn prepare_completion(
        &self,
        attempt: &AuditAttemptReference,
        event_time: EventTime,
        detail: AuditOutcomeDetail,
    ) -> Result<PreparedAuditTerminal, AuditError> {
        self.prepare_terminal(attempt, event_time, detail, TerminalPhase::Completion)
    }

    /// Constructs one authoritative Correction linked to a genuine Attempt.
    pub fn prepare_correction(
        &self,
        attempt: &AuditAttemptReference,
        event_time: EventTime,
        detail: AuditOutcomeDetail,
    ) -> Result<PreparedAuditTerminal, AuditError> {
        self.prepare_terminal(attempt, event_time, detail, TerminalPhase::Correction)
    }

    /// Imports and revalidates one opaque live terminal recovery obligation.
    pub fn restore_terminal_recovery(
        &self,
        persistence: &AuditTerminalRecoveryPersistence,
        obligation: &AuditTerminalObligation,
    ) -> Result<PendingAuditTerminalRecovery, AuditTerminalObligationError> {
        let projection = AuditTerminalRecoveryProjection::from_persisted(
            persistence.projection_bytes(obligation).to_vec(),
        )
        .map_err(|_| AuditTerminalObligationError::InvalidObligation)?;
        let terminal = projection
            .restore(&self.record_issuer)
            .map_err(|_| AuditTerminalObligationError::InvalidObligation)?;
        if terminal.record().record_id().as_bytes() != obligation.identifier().as_bytes()
            || terminal.binding().identifier() != obligation.binding().identifier()
            || terminal.binding().version() != obligation.binding().version()
        {
            return Err(AuditTerminalObligationError::InvalidObligation);
        }
        Ok(PendingAuditTerminalRecovery {
            obligation: obligation.clone(),
            terminal,
        })
    }

    fn prepare_terminal(
        &self,
        attempt: &AuditAttemptReference,
        event_time: EventTime,
        detail: AuditOutcomeDetail,
        phase: TerminalPhase,
    ) -> Result<PreparedAuditTerminal, AuditError> {
        let outcome = attempt.event.terminal_outcome(detail)?;
        let detail = match phase {
            TerminalPhase::Completion => outcome.completion_detail(),
            TerminalPhase::Correction => outcome.correction_detail(),
        };
        let body = attempt
            .event
            .body(&attempt.actor, detail, model::EventPhase::Terminal)?;
        let attempt_record_id = attempt
            .record
            .attempt_record_id()
            .ok_or(AuditError::InvalidRecord)?;
        let LogRecordPersistenceView::Audit(attempt_view) = attempt.record.persistence_view()
        else {
            return Err(AuditError::InvalidRecord);
        };
        let record_id = self.issue_record_id()?;
        let correlation_id = attempt_view.correlation_id().clone();
        let record = match phase {
            TerminalPhase::Completion => CompleteLogRecord::audit_completion(
                record_id,
                event_time,
                attempt_record_id,
                outcome.result(),
                correlation_id,
                body,
            ),
            TerminalPhase::Correction => CompleteLogRecord::audit_correction(
                record_id,
                event_time,
                attempt_record_id,
                outcome.result(),
                correlation_id,
                body,
            ),
        }
        .map_err(|_| AuditError::InvalidRecord)?;
        Ok(PreparedAuditTerminal { record })
    }

    fn issue_record_id(&self) -> Result<RecordId, AuditError> {
        self.issue_record_id_with(|entropy| {
            getrandom::fill(entropy).map_err(|_| AuditError::RandomnessUnavailable)
        })
    }

    fn issue_record_id_with(
        &self,
        mut fill: impl FnMut(&mut [u8; 16]) -> Result<(), AuditError>,
    ) -> Result<RecordId, AuditError> {
        for _ in 0..RECORD_ID_GENERATION_ATTEMPTS {
            let mut entropy = [0; 16];
            fill(&mut entropy)?;
            if entropy != [0; 16] {
                return self
                    .record_issuer
                    .issue(entropy)
                    .map_err(|_| AuditError::InvalidRecord);
            }
        }
        Err(AuditError::RandomnessUnavailable)
    }
}

#[derive(Clone, Copy)]
enum TerminalPhase {
    Completion,
    Correction,
}

/// Prepared Audit Attempt that can be delivered and retained as an opaque reference.
pub struct PreparedAuditAttempt {
    record: CompleteLogRecord,
    actor: AuditActor,
    event: AuditEvent,
}

impl PreparedAuditAttempt {
    /// Returns the complete immutable record for typed inspection.
    pub const fn record(&self) -> &CompleteLogRecord {
        &self.record
    }

    /// Delivers synchronously and returns the retained Attempt only after acknowledgement.
    pub fn deliver(
        self,
        destination: &ConfiguredLogDestination,
    ) -> Result<AuditAttemptReference, LogDeliveryError> {
        destination.deliver(&self.record)?;
        Ok(AuditAttemptReference::new(
            self.record,
            self.actor,
            self.event,
        ))
    }
}

impl fmt::Debug for PreparedAuditAttempt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PreparedAuditAttempt(REDACTED)")
    }
}

/// Non-forgeable retained source for linked Audit terminal records.
pub struct AuditAttemptReference {
    record: CompleteLogRecord,
    actor: AuditActor,
    event: AuditEvent,
}

impl AuditAttemptReference {
    fn new(record: CompleteLogRecord, actor: AuditActor, event: AuditEvent) -> Self {
        Self {
            record,
            actor,
            event,
        }
    }
}

impl fmt::Debug for AuditAttemptReference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuditAttemptReference(REDACTED)")
    }
}

/// Prepared Audit Completion or Correction.
pub struct PreparedAuditTerminal {
    record: CompleteLogRecord,
}

impl PreparedAuditTerminal {
    /// Returns the complete immutable record for typed inspection.
    pub const fn record(&self) -> &CompleteLogRecord {
        &self.record
    }

    /// Delivers the same immutable record idempotently through a borrowed destination.
    ///
    /// Repeated calls deliberately replay the exact record identifier and content. This
    /// producer does not schedule retries, loop, or construct a replacement record.
    pub fn deliver(&self, destination: &ConfiguredLogDestination) -> Result<(), LogDeliveryError> {
        destination.deliver(&self.record)
    }

    /// Captures this exact terminal record and destination binding before mutation commit.
    pub fn recovery_obligation(
        &self,
        persistence: &AuditTerminalRecoveryPersistence,
        binding: &AuditDestinationBinding,
    ) -> Result<ValidatedAuditTerminalObligationWrite, AuditTerminalObligationError> {
        let projection = AuditTerminalRecoveryProjection::capture(&self.record, binding)
            .map_err(|_| AuditTerminalObligationError::InvalidObligation)?;
        let binding = StoredAuditDestinationBinding::from_persisted(
            persistence,
            *binding.identifier(),
            binding.version(),
        )
        .map_err(|_| AuditTerminalObligationError::InvalidObligation)?;
        ValidatedAuditTerminalObligationWrite::from_server_audit(
            persistence,
            *self.record.record_id().as_bytes(),
            projection.as_bytes().to_vec(),
            binding,
        )
        .map_err(|_| AuditTerminalObligationError::InvalidObligation)
    }
}

impl fmt::Debug for PreparedAuditTerminal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PreparedAuditTerminal(REDACTED)")
    }
}

/// Revalidated pending terminal obligation retained after its mutation committed.
pub struct PendingAuditTerminalRecovery {
    obligation: AuditTerminalObligation,
    terminal: RecoveredLogAuditTerminal,
}

impl PendingAuditTerminalRecovery {
    /// Returns the retained binding that current assignment resolution must match.
    pub const fn binding(&self) -> &AuditDestinationBinding {
        self.terminal.binding()
    }

    /// Records completed exact-session reauthentication for this exact obligation.
    pub fn record_supersession_authorization(
        &self,
        authority: &ServerLogAuthority,
    ) -> AuditTerminalSupersessionAuthorization {
        AuditTerminalSupersessionAuthorization::from_server_authority(authority, &self.terminal)
    }

    /// Records explicit confirmation of this exact original and replacement binding.
    pub fn record_supersession_confirmation(
        &self,
        authority: &ServerLogAuthority,
        transition: &AuditDestinationBindingTransition,
        authorization: &AuditTerminalSupersessionAuthorization,
    ) -> Result<AuditTerminalSupersessionConfirmation, AuditTerminalObligationError> {
        AuditTerminalSupersessionConfirmation::from_server_authority(
            authority,
            &self.terminal,
            transition,
            authorization,
        )
        .map_err(|_| AuditTerminalObligationError::InvalidObligation)
    }

    /// Prepares the append-only database request after all exact evidence matches.
    pub fn prepare_supersession(
        &self,
        persistence: &AuditTerminalRecoveryPersistence,
        transition: &AuditDestinationBindingTransition,
        authorization: &AuditTerminalSupersessionAuthorization,
        confirmation: &AuditTerminalSupersessionConfirmation,
        replacement: &PreflightedAuditDestination<'_>,
        replacement_obligation: ValidatedAuditTerminalObligationWrite,
    ) -> Result<AuditTerminalSupersession, AuditTerminalObligationError> {
        let disposition = AuditTerminalSupersessionDisposition::capture(
            &self.terminal,
            transition,
            authorization,
            confirmation,
            replacement,
        )
        .map_err(|_| AuditTerminalObligationError::InvalidObligation)?;
        let original_binding = StoredAuditDestinationBinding::from_persisted(
            persistence,
            *transition.retained().identifier(),
            transition.retained().version(),
        )
        .map_err(|_| AuditTerminalObligationError::InvalidObligation)?;
        let replacement_binding = StoredAuditDestinationBinding::from_persisted(
            persistence,
            *transition.replacement().identifier(),
            transition.replacement().version(),
        )
        .map_err(|_| AuditTerminalObligationError::InvalidObligation)?;
        AuditTerminalSupersession::from_server_audit(
            persistence,
            &self.obligation,
            disposition.as_bytes().to_vec(),
            original_binding,
            replacement_binding,
            replacement_obligation,
        )
        .map_err(|_| AuditTerminalObligationError::InvalidObligation)
    }

    /// Performs one exact replay attempt and yields acknowledgement authority on success.
    ///
    /// Callers may invoke this repeatedly while the obligation remains pending. Each
    /// successful destination acknowledgement produces a fresh database proof.
    pub fn deliver(
        &self,
        persistence: &AuditTerminalRecoveryPersistence,
        destination: &ResolvedAuditDestination<'_>,
    ) -> Result<AcknowledgedAuditTerminalRecovery, AuditTerminalReplayError> {
        let acknowledgement = self.terminal.deliver(destination)?;
        if !acknowledgement.matches(
            self.obligation.identifier().as_bytes(),
            self.terminal.binding(),
        ) {
            return Err(AuditTerminalReplayError::DeliveryPending(
                LogDeliveryError::IntegrityFailure,
            ));
        }
        let proof = AuditTerminalAcknowledgementProof::from_server_audit(
            persistence,
            *acknowledgement.record_id(),
            self.obligation.binding().clone(),
        )
        .map_err(|_| {
            AuditTerminalReplayError::DeliveryPending(LogDeliveryError::IntegrityFailure)
        })?;
        Ok(AcknowledgedAuditTerminalRecovery { proof })
    }
}

impl fmt::Debug for PendingAuditTerminalRecovery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PendingAuditTerminalRecovery(REDACTED)")
    }
}

/// Capability proving exact bound destination acknowledgement for one pending obligation.
pub struct AcknowledgedAuditTerminalRecovery {
    proof: AuditTerminalAcknowledgementProof,
}

impl AcknowledgedAuditTerminalRecovery {
    /// Removes the exact oldest pending obligation after destination acknowledgement.
    pub fn acknowledge(
        self,
        store: &mut dyn AuditTerminalRecoveryStore,
    ) -> Result<(), DatabaseError> {
        store.acknowledge_audit_terminal_obligation(self.proof)
    }
}

impl fmt::Debug for AcknowledgedAuditTerminalRecovery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AcknowledgedAuditTerminalRecovery(REDACTED)")
    }
}

/// Payload-free invalid producer export or persisted import category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditTerminalObligationError {
    /// The terminal projection, identity, or retained binding is invalid.
    InvalidObligation,
}

impl fmt::Display for AuditTerminalObligationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Audit terminal recovery obligation is invalid")
    }
}

impl core::error::Error for AuditTerminalObligationError {}

#[cfg(test)]
mod tests {
    use super::*;
    use weavelit_server_log::TrustedRecordIssuer;
    use weavelit_server_log_authority::ServerLogAuthority;

    fn test_producer() -> ServerAudit {
        ServerAudit::new(TrustedRecordIssuer::from_server_authority(
            &ServerLogAuthority::new(),
        ))
    }

    #[test]
    fn bounded_zero_entropy_exhaustion_returns_randomness_unavailable() {
        let producer = test_producer();
        let mut attempt_count = 0;

        let fill_zeros = |_entropy: &mut [u8; 16]| {
            attempt_count += 1;
            Ok::<(), AuditError>(())
        };

        let result = producer.issue_record_id_with(fill_zeros);

        assert_eq!(result, Err(AuditError::RandomnessUnavailable));
        assert_eq!(attempt_count, RECORD_ID_GENERATION_ATTEMPTS);
    }

    #[test]
    fn generation_error_returns_randomness_unavailable() {
        let producer = test_producer();
        let fill_error =
            |_entropy: &mut [u8; 16]| Err::<(), AuditError>(AuditError::RandomnessUnavailable);

        let result = producer.issue_record_id_with(fill_error);

        assert_eq!(result, Err(AuditError::RandomnessUnavailable));
    }

    #[test]
    fn fresh_nonzero_entropy_generates_successfully() {
        let producer = test_producer();
        let nonzero_entropy = [42; 16];

        let fill_nonzero = |entropy: &mut [u8; 16]| {
            *entropy = nonzero_entropy;
            Ok::<(), AuditError>(())
        };

        let result = producer.issue_record_id_with(fill_nonzero);

        assert!(
            result.is_ok(),
            "nonzero entropy should generate a valid record ID"
        );
    }
}
