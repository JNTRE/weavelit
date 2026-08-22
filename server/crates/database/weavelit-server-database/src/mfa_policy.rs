//! Backend-neutral MFA requirement and enrollment-reset mutation contract.

use std::fmt;

use crate::{
    AccountAuditReference, AccountPublicIdentifier, AccountPublicIdentifierPersistence,
    AuditReferencePersistence, DatabaseError, MfaModuleTarget, Name, SessionInstant,
    SessionTokenHash, StateIdentifier, ValidatedAuditTerminalObligationWrite,
};

/// One MFA policy change admitted for an exact account target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MfaPolicyAction {
    /// Change whether MFA is required at the account's next usable sign-in.
    Requirement { required: bool },
    /// Remove the account's current TOTP enrollment.
    EnrollmentReset,
}

/// Exact live issuer state an MFA policy writer must recheck.
pub struct MfaPolicyRecheck {
    actor: StateIdentifier,
    session: SessionTokenHash,
    client_module: Name,
    target: MfaModuleTarget,
    factor: StateIdentifier,
    now: SessionInstant,
}

impl MfaPolicyRecheck {
    /// Binds one policy step-up to its exact live session and TOTP factor.
    #[must_use]
    pub const fn new(
        actor: StateIdentifier,
        session: SessionTokenHash,
        client_module: Name,
        target: MfaModuleTarget,
        factor: StateIdentifier,
        now: SessionInstant,
    ) -> Self {
        Self {
            actor,
            session,
            client_module,
            target,
            factor,
            now,
        }
    }

    /// Returns the authenticated issuer account.
    pub const fn actor(&self) -> StateIdentifier {
        self.actor
    }

    /// Returns the exact validated session digest.
    pub const fn session(&self) -> &SessionTokenHash {
        &self.session
    }

    /// Returns the issuing Client Module.
    pub const fn client_module(&self) -> &Name {
        &self.client_module
    }

    /// Returns the exact MFA Module and enablement component.
    pub const fn target(&self) -> &MfaModuleTarget {
        &self.target
    }

    /// Returns the exact factor that proved the step-up.
    pub const fn factor(&self) -> StateIdentifier {
        self.factor
    }

    /// Returns the instant at which final session liveness is judged.
    pub const fn now(&self) -> SessionInstant {
        self.now
    }
}

impl fmt::Debug for MfaPolicyRecheck {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MfaPolicyRecheck(REDACTED)")
    }
}

/// Immutable MFA policy target read from one consistent backend snapshot.
pub struct MfaPolicyTarget {
    public_identifier: AccountPublicIdentifier,
    account: StateIdentifier,
    audit_reference: AccountAuditReference,
    required: bool,
    factor: Option<StateIdentifier>,
}

impl MfaPolicyTarget {
    /// Rebuilds a prepared target from authority-decoded persistence values.
    pub fn from_persistence(
        _public_identifier_persistence: &AccountPublicIdentifierPersistence,
        _audit_reference_persistence: &AuditReferencePersistence,
        public_identifier: AccountPublicIdentifier,
        account: StateIdentifier,
        audit_reference: AccountAuditReference,
        required: bool,
        factor: Option<StateIdentifier>,
    ) -> Result<Self, MfaPolicyMutationError> {
        if audit_reference.account() != account {
            return Err(MfaPolicyMutationError::InvalidTarget);
        }
        Ok(Self {
            public_identifier,
            account,
            audit_reference,
            required,
            factor,
        })
    }

    /// Returns the exact public identifier used for lookup and final recheck.
    pub const fn public_identifier(&self) -> AccountPublicIdentifier {
        self.public_identifier
    }

    /// Returns the internal target account identity.
    pub const fn account(&self) -> StateIdentifier {
        self.account
    }

    /// Returns the target's typed Audit Reference.
    pub const fn audit_reference(&self) -> AccountAuditReference {
        self.audit_reference
    }

    /// Returns the requirement observed during preparation.
    pub const fn required(&self) -> bool {
        self.required
    }

    /// Returns the factor observed during preparation, when enrolled.
    pub const fn factor(&self) -> Option<StateIdentifier> {
        self.factor
    }
}

impl fmt::Debug for MfaPolicyTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MfaPolicyTarget(REDACTED)")
    }
}

/// Validated MFA policy mutation prepared before its transaction begins.
pub struct MfaPolicyMutation {
    recheck: MfaPolicyRecheck,
    target: MfaPolicyTarget,
    action: MfaPolicyAction,
}

impl MfaPolicyMutation {
    /// Rejects an unchanged requirement or reset without an enrollment.
    pub fn new(
        recheck: MfaPolicyRecheck,
        target: MfaPolicyTarget,
        action: MfaPolicyAction,
    ) -> Result<Self, MfaPolicyMutationError> {
        let unchanged = match action {
            MfaPolicyAction::Requirement { required } => target.required == required,
            MfaPolicyAction::EnrollmentReset => target.factor.is_none(),
        };
        if unchanged {
            return Err(MfaPolicyMutationError::Unchanged);
        }
        Ok(Self {
            recheck,
            target,
            action,
        })
    }

    /// Returns the exact issuer and factor recheck.
    pub const fn recheck(&self) -> &MfaPolicyRecheck {
        &self.recheck
    }

    /// Returns the prepared exact target.
    pub const fn target(&self) -> &MfaPolicyTarget {
        &self.target
    }

    /// Returns the policy action to commit.
    pub const fn action(&self) -> MfaPolicyAction {
        self.action
    }
}

impl fmt::Debug for MfaPolicyMutation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MfaPolicyMutation(REDACTED)")
    }
}

/// Payload-free invalid MFA policy mutation construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MfaPolicyMutationError {
    /// The target already has the requested state.
    Unchanged,
    /// Persisted target associations were not exact.
    InvalidTarget,
}

impl fmt::Display for MfaPolicyMutationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MFA policy mutation is invalid")
    }
}

impl std::error::Error for MfaPolicyMutationError {}

/// The terminal records prepared for one MFA policy mutation.
pub struct MfaPolicyAuditTerminalWrites<'a> {
    succeeded: &'a ValidatedAuditTerminalObligationWrite,
    denied: &'a ValidatedAuditTerminalObligationWrite,
}

impl<'a> MfaPolicyAuditTerminalWrites<'a> {
    /// Binds both terminal alternatives before mutation begins.
    #[must_use]
    pub const fn new(
        succeeded: &'a ValidatedAuditTerminalObligationWrite,
        denied: &'a ValidatedAuditTerminalObligationWrite,
    ) -> Self {
        Self { succeeded, denied }
    }

    /// Returns the successful business-outcome terminal.
    pub const fn succeeded(&self) -> &ValidatedAuditTerminalObligationWrite {
        self.succeeded
    }

    /// Returns the stale-target or final issuer-denial terminal.
    pub const fn denied(&self) -> &ValidatedAuditTerminalObligationWrite {
        self.denied
    }
}

impl fmt::Debug for MfaPolicyAuditTerminalWrites<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MfaPolicyAuditTerminalWrites(REDACTED)")
    }
}

/// Authoritative MFA policy transaction result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MfaPolicyMutationOutcome {
    /// The policy state and selected success terminal committed.
    Changed {
        /// Sessions removed to make the changed policy effective immediately.
        revoked_sessions: usize,
    },
    /// The prepared target requirement or factor changed.
    Stale,
    /// Final issuer session, factor, or Module state was not exact.
    Denied,
}

/// Backend-neutral preparation and atomic commit of MFA policy writers.
pub trait MfaPolicyWriterStore {
    /// Resolves one exact public-ID target and current TOTP enrollment.
    fn prepare_mfa_policy_target(
        &mut self,
        public_identifier_persistence: &AccountPublicIdentifierPersistence,
        audit_reference_persistence: &AuditReferencePersistence,
        module: &Name,
        target: AccountPublicIdentifier,
    ) -> Result<Option<MfaPolicyTarget>, DatabaseError>;

    /// Commits one policy mutation or one denied terminal atomically.
    fn change_mfa_policy(
        &mut self,
        public_identifier_persistence: &AccountPublicIdentifierPersistence,
        mutation: &MfaPolicyMutation,
        audit_terminals: &MfaPolicyAuditTerminalWrites<'_>,
    ) -> Result<MfaPolicyMutationOutcome, DatabaseError>;
}

#[cfg(test)]
mod tests {
    use weavelit_server_database_authority::ServerDatabaseAuthority;

    use super::*;
    use crate::{AuditReferenceIdentifier, SESSION_DIGEST_LENGTH, STATE_IDENTIFIER_LENGTH};

    fn identifier(byte: u8) -> StateIdentifier {
        StateIdentifier::from_bytes([byte; STATE_IDENTIFIER_LENGTH]).unwrap()
    }

    fn persistence() -> (
        AccountPublicIdentifierPersistence,
        AuditReferencePersistence,
    ) {
        let authority = ServerDatabaseAuthority::new();
        (
            AccountPublicIdentifierPersistence::from_server_authority(&authority),
            AuditReferencePersistence::from_server_authority(&authority),
        )
    }

    fn recheck() -> MfaPolicyRecheck {
        MfaPolicyRecheck::new(
            identifier(1),
            SessionTokenHash::from_bytes([2; SESSION_DIGEST_LENGTH]).unwrap(),
            Name::new("web-ui").unwrap(),
            MfaModuleTarget {
                module: Name::new("totp").unwrap(),
                component: Name::new("totp").unwrap(),
            },
            identifier(3),
            SessionInstant::from_unix_milliseconds(1_000).unwrap(),
        )
    }

    fn target(required: bool, factor: Option<StateIdentifier>) -> MfaPolicyTarget {
        let (public_persistence, audit_persistence) = persistence();
        let account = identifier(4);
        MfaPolicyTarget::from_persistence(
            &public_persistence,
            &audit_persistence,
            AccountPublicIdentifier::generate().unwrap(),
            account,
            AccountAuditReference::new(account, AuditReferenceIdentifier::generate().unwrap()),
            required,
            factor,
        )
        .unwrap()
    }

    #[test]
    fn requirement_and_enrollment_reset_retain_exact_intent() {
        let requirement = MfaPolicyMutation::new(
            recheck(),
            target(false, None),
            MfaPolicyAction::Requirement { required: true },
        )
        .unwrap();
        assert_eq!(
            requirement.action(),
            MfaPolicyAction::Requirement { required: true }
        );

        let factor = identifier(5);
        let reset = MfaPolicyMutation::new(
            recheck(),
            target(true, Some(factor)),
            MfaPolicyAction::EnrollmentReset,
        )
        .unwrap();
        assert_eq!(reset.action(), MfaPolicyAction::EnrollmentReset);
        assert_eq!(reset.target().factor(), Some(factor));
    }

    #[test]
    fn unchanged_requirement_and_absent_enrollment_are_rejected() {
        assert_eq!(
            MfaPolicyMutation::new(
                recheck(),
                target(true, None),
                MfaPolicyAction::Requirement { required: true },
            )
            .unwrap_err(),
            MfaPolicyMutationError::Unchanged
        );
        assert_eq!(
            MfaPolicyMutation::new(
                recheck(),
                target(false, None),
                MfaPolicyAction::EnrollmentReset,
            )
            .unwrap_err(),
            MfaPolicyMutationError::Unchanged
        );
    }

    #[test]
    fn associations_and_diagnostics_are_payload_free() {
        let (public_persistence, audit_persistence) = persistence();
        let error = MfaPolicyTarget::from_persistence(
            &public_persistence,
            &audit_persistence,
            AccountPublicIdentifier::generate().unwrap(),
            identifier(4),
            AccountAuditReference::new(
                identifier(5),
                AuditReferenceIdentifier::generate().unwrap(),
            ),
            false,
            None,
        )
        .unwrap_err();

        assert_eq!(error, MfaPolicyMutationError::InvalidTarget);
        assert_eq!(error.to_string(), "MFA policy mutation is invalid");
        assert_eq!(format!("{error:?}"), "InvalidTarget");
        assert_eq!(
            format!("{:?} {:?}", recheck(), target(false, None)),
            "MfaPolicyRecheck(REDACTED) MfaPolicyTarget(REDACTED)"
        );
    }
}
