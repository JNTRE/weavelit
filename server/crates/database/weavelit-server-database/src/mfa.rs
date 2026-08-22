//! Live MFA replay watermarks held outside restorable application state.
//!
//! An enrolled factor is restorable application state. The last time step that
//! factor accepted is not: it is live operational data, like a session, so it
//! is not part of [`crate::ApplicationState`] and cannot enter a checkpoint, a
//! normalized backup, or restored state. It is cleared inside the one atomic
//! state replacement a Restore performs, so restored state never carries a
//! watermark that could accept or refuse a code on evidence from another
//! deployment's history.

use crate::{
    AccountCredentialIssuanceRecheck, ContractInputError, DatabaseError, MfaFactor, Name,
    NewSession, SessionInstant, SessionTokenHash, StateIdentifier,
    ValidatedAuditTerminalObligationWrite,
};

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
    /// The step advanced the factor's watermark and the session was issued.
    Accepted,
    /// Account state no longer permits the verified credential to issue a session.
    Rejected,
    /// The step did not advance the watermark, so the code is a replay.
    Replayed,
    /// The MFA Module was disabled when the code was presented.
    ModuleDisabled,
}

/// Exact live session state an administration step-up must recheck.
pub struct MfaAdministrationStepUpRecheck {
    actor: StateIdentifier,
    session: SessionTokenHash,
    client_module: Name,
    now: SessionInstant,
}

impl MfaAdministrationStepUpRecheck {
    /// Binds one verified code to the exact current administration session.
    #[must_use]
    pub const fn new(
        actor: StateIdentifier,
        session: SessionTokenHash,
        client_module: Name,
        now: SessionInstant,
    ) -> Self {
        Self {
            actor,
            session,
            client_module,
            now,
        }
    }

    /// Returns the authenticated account.
    pub const fn actor(&self) -> StateIdentifier {
        self.actor
    }

    /// Returns the exact current session digest.
    pub const fn session(&self) -> &SessionTokenHash {
        &self.session
    }

    /// Returns the issuing Client Module.
    pub const fn client_module(&self) -> &Name {
        &self.client_module
    }

    /// Returns the instant at which session liveness is judged.
    pub const fn now(&self) -> SessionInstant {
        self.now
    }
}

impl std::fmt::Debug for MfaAdministrationStepUpRecheck {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("MfaAdministrationStepUpRecheck(REDACTED)")
    }
}

/// Authoritative result of consuming a verified TOTP step for administration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MfaAdministrationStepUpAcceptance {
    /// The exact current session and factor were accepted and the watermark advanced.
    Accepted,
    /// Session, actor, account, or factor ownership was no longer exact.
    Rejected,
    /// The verified step did not advance the factor watermark.
    Replayed,
    /// The factor's MFA Module was disabled.
    ModuleDisabled,
}

/// Authoritative result of accepting credential-issuance assurance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialIssuanceStepUpAcceptance {
    /// Exact live state was accepted and any TOTP watermark was advanced.
    Accepted,
    /// Session, actor, credential, factor, or Module state was no longer exact.
    Rejected,
    /// The verified TOTP step did not advance its factor watermark.
    Replayed,
}

/// The result of issuing a session no second factor was found to gate.
///
/// Every variant other than [`Self::Issued`] is one row of the admission truth
/// table the caller already decides, re-decided against the state the session
/// would have been written into. Nothing new is reported: a caller answers each
/// of them exactly as it answers that row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MfaDirectSession {
    /// The account is not required to hold a factor and no enabled Module holds
    /// one for it, so the session was written.
    Issued,
    /// The Module was enabled and the account holds a factor for it, so the
    /// login must present that factor. Nothing was written.
    SecondFactorRequired,
    /// The Module was enabled and the account is required to hold a factor it
    /// does not hold, so the login must enroll one. Nothing was written.
    EnrollmentRequired,
    /// The account is required to present a second factor the deployment cannot
    /// currently verify, so the login is admitted to nothing. Nothing was
    /// written.
    ///
    /// An account that has ceased to exist is reported the same way, because a
    /// login can be admitted to nothing on behalf of an account this store no
    /// longer holds either.
    Denied,
}

/// The result of persisting one newly confirmed enrollment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MfaEnrollment {
    /// The factor, its opening watermark, and the session were all written.
    Enrolled,
    /// The account already holds a factor for that MFA Module.
    AlreadyEnrolled,
    /// The MFA Module was disabled when the enrollment was presented.
    ModuleDisabled,
    /// Account state no longer permits the verified credential to issue a session.
    Rejected,
}

/// The two names one MFA Module is addressed by.
///
/// A factor records the module that owns its encoding, while the module's
/// enabled state is a setting owned by the module's own configuration
/// component. The two are different strings, so an operation that spans both
/// is given both rather than deriving one from the other.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MfaModuleTarget {
    /// The module name an enrolled factor records.
    pub module: Name,
    /// The configuration component that owns the module's enabled setting.
    pub component: Name,
}

/// Current state shown before one MFA Module enablement decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MfaEnablementPreviewState {
    current_enabled: bool,
    affected_users: usize,
}

impl MfaEnablementPreviewState {
    /// Constructs one snapshot from values read in a single database transaction.
    #[must_use]
    pub const fn new(current_enabled: bool, affected_users: usize) -> Self {
        Self {
            current_enabled,
            affected_users,
        }
    }

    /// Returns whether the Module is currently enabled.
    #[must_use]
    pub const fn current_enabled(self) -> bool {
        self.current_enabled
    }

    /// Returns the number of distinct enrolled Human Users.
    #[must_use]
    pub const fn affected_users(self) -> usize {
        self.affected_users
    }
}

/// The result of changing one MFA Module's enabled state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MfaEnablementOutcome {
    /// The setting was written, revoking `revoked_sessions` live sessions.
    Applied {
        /// Sessions removed because their account holds a factor for the module.
        ///
        /// Always zero when the module was enabled rather than disabled.
        revoked_sessions: usize,
    },
    /// The caller's enrolled-user preview did not match the stored count.
    ///
    /// The reported count is the number of accounts currently holding a factor
    /// for the module, which the caller already asked for by previewing it.
    /// Nothing was written.
    EnrolledCountChanged {
        /// The affected-Human-User count observed inside the rejecting transaction.
        current_affected_users: usize,
    },
}

/// Prevalidated terminal obligations for either authoritative enablement outcome.
pub struct MfaEnablementAuditTerminalWrites<'a> {
    applied: &'a ValidatedAuditTerminalObligationWrite,
    enrolled_count_changed: &'a ValidatedAuditTerminalObligationWrite,
}

impl<'a> MfaEnablementAuditTerminalWrites<'a> {
    /// Binds the success and stale-preview terminal writes before mutation begins.
    #[must_use]
    pub const fn new(
        applied: &'a ValidatedAuditTerminalObligationWrite,
        enrolled_count_changed: &'a ValidatedAuditTerminalObligationWrite,
    ) -> Self {
        Self {
            applied,
            enrolled_count_changed,
        }
    }

    /// Returns the terminal obligation for an applied state change.
    #[must_use]
    pub const fn applied(&self) -> &ValidatedAuditTerminalObligationWrite {
        self.applied
    }

    /// Returns the terminal obligation for a stale enrolled-user preview.
    #[must_use]
    pub const fn enrolled_count_changed(&self) -> &ValidatedAuditTerminalObligationWrite {
        self.enrolled_count_changed
    }
}

impl std::fmt::Debug for MfaEnablementAuditTerminalWrites<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("MfaEnablementAuditTerminalWrites(REDACTED)")
    }
}

/// Durable live MFA replay watermarks available during normal operation.
///
/// The store owns the replay decision rather than reporting the stored value
/// for a caller to compare, because a caller that read the watermark, decided,
/// and then wrote it back would leave a window in which a concurrent
/// presentation of the same code observes the same earlier watermark and is
/// also accepted.
pub trait MfaStore {
    /// Counts distinct Human Users holding a factor for the target Module.
    fn enrolled_accounts(&mut self, target: &MfaModuleTarget) -> Result<usize, DatabaseError>;

    /// Reads current enablement and affected Human Users in one snapshot.
    fn enablement_preview(
        &mut self,
        target: &MfaModuleTarget,
    ) -> Result<MfaEnablementPreviewState, DatabaseError>;

    /// Returns the last step one factor accepted, when it has accepted one.
    fn accepted_step(
        &mut self,
        factor: StateIdentifier,
    ) -> Result<Option<MfaTimeStep>, DatabaseError>;

    /// Accepts `step` for one factor only when it advances that factor's watermark.
    ///
    /// The comparison and the write are one atomic operation, so two
    /// concurrent presentations of the same code cannot both be accepted.
    ///
    /// The module's enabled state is read, and `session` is written, inside
    /// that same operation. All three are one decision because none of them is
    /// safe on its own: a caller that decided enablement on state it loaded
    /// earlier, or that issued the session in a second transaction, would sign
    /// an account in behind a module the deployment stopped verifying while the
    /// code was in flight, and the disablement's own session revocation cannot
    /// reach a session that does not exist yet. Nothing is written when the
    /// module is not enabled or the step is a replay. `target` names the
    /// configuration component that owns the enabled setting for the module the
    /// factor records.
    fn accept_step(
        &mut self,
        target: &MfaModuleTarget,
        factor: StateIdentifier,
        step: MfaTimeStep,
        session: &NewSession,
    ) -> Result<MfaAcceptance, DatabaseError>;

    /// Consumes a verified step for one exact current administration session.
    ///
    /// Session ownership and liveness, actor activity, factor ownership,
    /// Module enablement, and replay state are rechecked in the transaction
    /// that advances the watermark. No session is issued, touched, or rotated.
    fn accept_administration_step_up(
        &mut self,
        target: &MfaModuleTarget,
        factor: StateIdentifier,
        step: MfaTimeStep,
        recheck: &MfaAdministrationStepUpRecheck,
    ) -> Result<MfaAdministrationStepUpAcceptance, DatabaseError>;

    /// Atomically accepts one credential-issuance assurance snapshot.
    ///
    /// The transaction rechecks the exact live session, active ordinary actor
    /// credential revision, factor identity or absence, and Module state. A
    /// present factor's verified step must advance its replay watermark. No
    /// account business state, Audit record, or session is written.
    fn accept_credential_issuance_step_up(
        &mut self,
        recheck: &AccountCredentialIssuanceRecheck,
    ) -> Result<CredentialIssuanceStepUpAcceptance, DatabaseError>;

    /// Writes one session for a login no second factor was found to gate.
    ///
    /// The enabled-state read, the account's enrollment read, the account's
    /// requirement read, and the session write are one atomic operation, for
    /// the same reason accepting a step is. A caller that decided those three
    /// inputs from separately loaded state would write a session behind a
    /// Module enabled, or a requirement imposed, while the login was in flight,
    /// and neither change can revoke a session that does not exist yet.
    ///
    /// The three inputs are decided here exactly as the caller's own admission
    /// table decides them, and every row that is not the issuing one is
    /// reported rather than written. `target` names the module a factor records
    /// and the configuration component that owns the enabled setting for it.
    fn issue_direct_session(
        &mut self,
        target: &MfaModuleTarget,
        session: &NewSession,
    ) -> Result<MfaDirectSession, DatabaseError>;

    /// Persists one confirmed factor, its opening watermark, and its session.
    ///
    /// All three writes are one atomic operation. A factor that existed without
    /// its watermark would accept the very code that confirmed it a second
    /// time, and a watermark without its factor would refuse a later code on
    /// evidence of an enrollment this deployment never completed.
    ///
    /// The module's enabled state is read inside that same operation and the
    /// enrollment is refused when it is not enabled. An enrollment is opened
    /// before it is confirmed, so a caller that decided enablement on state it
    /// loaded earlier would persist a factor, and issue the session behind it,
    /// against a module the deployment stopped verifying in between. `target`
    /// names the configuration component that owns the enabled setting for the
    /// module the factor records.
    fn enroll(
        &mut self,
        target: &MfaModuleTarget,
        factor: &MfaFactor,
        accepted_step: MfaTimeStep,
        session: &NewSession,
    ) -> Result<MfaEnrollment, DatabaseError>;

    /// Sets one MFA Module's enabled state against a previewed enrolled count.
    ///
    /// The count, the setting write, and the session revocation are one atomic
    /// operation. The count is checked inside it so an administrator cannot be
    /// shown one number, decide against it, and have a concurrent enrollment
    /// change what the decision actually turns off.
    ///
    /// Disabling removes every live session of every account holding a factor
    /// for the module, because those sessions were established behind a factor
    /// the deployment is no longer willing to verify.
    fn set_module_enabled(
        &mut self,
        target: &MfaModuleTarget,
        enabled: bool,
        expected_enrolled: usize,
        audit_terminals: &MfaEnablementAuditTerminalWrites<'_>,
    ) -> Result<MfaEnablementOutcome, DatabaseError>;
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
