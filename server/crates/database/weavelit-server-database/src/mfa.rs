//! Live MFA replay watermarks held outside restorable application state.
//!
//! An enrolled factor is restorable application state. The last time step that
//! factor accepted is not: it is live operational data, like a session, so it
//! is not part of [`crate::ApplicationState`] and cannot enter a checkpoint, a
//! normalized backup, or restored state. It is cleared inside the one atomic
//! state replacement a Restore performs, so restored state never carries a
//! watermark that could accept or refuse a code on evidence from another
//! deployment's history.

use crate::{ContractInputError, DatabaseError, StateIdentifier};

/// Largest accepted time step.
///
/// A step is bounded so it is representable in the signed integer a backend
/// column stores, which makes [`MfaTimeStep::as_stored`] total.
pub const MAX_MFA_TIME_STEP: u64 = i64::MAX as u64;

/// One RFC 6238 time step recorded against an enrolled factor.
///
/// The type is ordered because the watermark decision is an ordering: a step
/// is accepted only when it is strictly greater than the stored one.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MfaTimeStep(u64);

impl MfaTimeStep {
    /// Creates a step and rejects a value no backend column can hold.
    pub fn from_step(step: u64) -> Result<Self, ContractInputError> {
        if step > MAX_MFA_TIME_STEP {
            return Err(ContractInputError::InvalidMfaTimeStep);
        }

        Ok(Self(step))
    }

    /// Returns the step number counted from `T0 = 0`.
    #[must_use]
    pub const fn as_step(self) -> u64 {
        self.0
    }

    /// Returns the step as the signed integer a backend column stores.
    #[must_use]
    pub const fn as_stored(self) -> i64 {
        self.0 as i64
    }
}

/// The result of presenting a verified code's time step to the store.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MfaAcceptance {
    /// The step advanced the factor's watermark and was recorded.
    Accepted,
    /// The step did not advance the watermark, so the code is a replay.
    Replayed,
}

/// Durable live MFA replay watermarks available during normal operation.
///
/// The store owns the replay decision rather than reporting the stored value
/// for a caller to compare, because a caller that read the watermark, decided,
/// and then wrote it back would leave a window in which a concurrent
/// presentation of the same code observes the same earlier watermark and is
/// also accepted.
pub trait MfaStore {
    /// Returns the last step one factor accepted, when it has accepted one.
    fn accepted_step(
        &mut self,
        factor: StateIdentifier,
    ) -> Result<Option<MfaTimeStep>, DatabaseError>;

    /// Accepts `step` for one factor only when it advances that factor's watermark.
    ///
    /// The comparison and the write are one atomic operation, so two
    /// concurrent presentations of the same code cannot both be accepted.
    fn accept_step(
        &mut self,
        factor: StateIdentifier,
        step: MfaTimeStep,
    ) -> Result<MfaAcceptance, DatabaseError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_accepts_the_representable_range_and_rejects_beyond_it() {
        assert_eq!(MfaTimeStep::from_step(0).unwrap().as_step(), 0);
        assert_eq!(
            MfaTimeStep::from_step(MAX_MFA_TIME_STEP)
                .unwrap()
                .as_stored(),
            i64::MAX
        );
        assert_eq!(
            MfaTimeStep::from_step(MAX_MFA_TIME_STEP + 1),
            Err(ContractInputError::InvalidMfaTimeStep)
        );
    }

    #[test]
    fn a_later_step_orders_above_an_earlier_one() {
        let earlier = MfaTimeStep::from_step(41_152_263).unwrap();
        let later = MfaTimeStep::from_step(41_152_264).unwrap();

        assert!(later > earlier);
        assert!(!(earlier > earlier));
    }
}
