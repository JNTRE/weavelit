//! Backend-neutral account creation and temporary-password reset mutations.

use std::fmt;

use crate::{
    Account, AccountAuditReference, AccountPasswordVerifier, AccountPublicIdentifier,
    AccountPublicIdentifierPersistence, AccountPublicIdentity, AuditReferencePersistence,
    CredentialRevision, DatabaseError, MfaModuleTarget, MfaTimeStep, Name, SessionInstant,
    SessionTokenHash, StateIdentifier, TemporaryCredentialExpiration,
    ValidatedAuditTerminalObligationWrite,
};

/// The MFA state observed while fresh issuer credentials were verified.
pub enum AccountCredentialIssuanceFactor {
    /// No factor for the supported MFA Module existed in the verified snapshot.
    NoneObserved {
        /// The MFA Module checked for an enrollment race.
        target: MfaModuleTarget,
    },
    /// One exact factor verified one exact TOTP time step.
    Totp {
        /// The MFA Module and its enablement component.
        target: MfaModuleTarget,
        /// The exact factor whose protected value verified.
        factor: StateIdentifier,
        /// The verified time step that must advance the replay watermark.
        verified_step: MfaTimeStep,
    },
}

impl fmt::Debug for AccountCredentialIssuanceFactor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AccountCredentialIssuanceFactor(REDACTED)")
    }
}

/// Exact live state that a credential writer must recheck before mutation.
///
/// The value is not clonable. It contains no current password, TOTP code,
/// verifier, temporary password, or response state.
pub struct AccountCredentialIssuanceRecheck {
    actor: StateIdentifier,
    session: SessionTokenHash,
    client_module: Name,
    expected_actor_revision: CredentialRevision,
    now: SessionInstant,
    factor: AccountCredentialIssuanceFactor,
}

impl AccountCredentialIssuanceRecheck {
    /// Binds one fresh credential decision to its exact live-state expectations.
    #[must_use]
    pub const fn new(
        actor: StateIdentifier,
        session: SessionTokenHash,
        client_module: Name,
        expected_actor_revision: CredentialRevision,
        now: SessionInstant,
        factor: AccountCredentialIssuanceFactor,
    ) -> Self {
        Self {
            actor,
            session,
            client_module,
            expected_actor_revision,
            now,
            factor,
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

    /// Returns the actor credential revision verified before mutation.
    pub const fn expected_actor_revision(&self) -> CredentialRevision {
        self.expected_actor_revision
    }

    /// Returns the instant at which final session liveness is judged.
    pub const fn now(&self) -> SessionInstant {
        self.now
    }

    /// Returns the exact MFA state verified before mutation.
    pub const fn factor(&self) -> &AccountCredentialIssuanceFactor {
        &self.factor
    }
}

impl fmt::Debug for AccountCredentialIssuanceRecheck {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AccountCredentialIssuanceRecheck(REDACTED)")
    }
}

/// Exact password-reset target read from one consistent backend snapshot.
pub struct AccountPasswordResetTarget {
    public_identifier: AccountPublicIdentifier,
    account: StateIdentifier,
    audit_reference: AccountAuditReference,
    expected_revision: CredentialRevision,
}

impl AccountPasswordResetTarget {
    /// Constructs a target from a backend-decoded public identity and Audit Reference.
    pub fn from_persistence(
        _public_identifier_persistence: &AccountPublicIdentifierPersistence,
        _audit_reference_persistence: &AuditReferencePersistence,
        public_identifier: AccountPublicIdentifier,
        account: StateIdentifier,
        audit_reference: AccountAuditReference,
        expected_revision: CredentialRevision,
    ) -> Result<Self, AccountCredentialMutationError> {
        if audit_reference.account() != account {
            return Err(AccountCredentialMutationError);
        }
        Ok(Self {
            public_identifier,
            account,
            audit_reference,
            expected_revision,
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

    /// Returns the target credential revision observed during preparation.
    pub const fn expected_revision(&self) -> CredentialRevision {
        self.expected_revision
    }
}

impl fmt::Debug for AccountPasswordResetTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AccountPasswordResetTarget(REDACTED)")
    }
}

/// Validated account-create mutation prepared before its transaction begins.
pub struct AccountCreateMutation {
    recheck: AccountCredentialIssuanceRecheck,
    account: Account,
    public_identity: AccountPublicIdentity,
    audit_reference: AccountAuditReference,
    password_verifier: AccountPasswordVerifier,
}

impl AccountCreateMutation {
    /// Validates the fixed initial state and exact identity associations.
    pub fn new(
        recheck: AccountCredentialIssuanceRecheck,
        account: Account,
        public_identity: AccountPublicIdentity,
        audit_reference: AccountAuditReference,
        password_verifier: AccountPasswordVerifier,
    ) -> Result<Self, AccountCredentialMutationError> {
        if !account.active
            || account.display_name.is_none()
            || account.mfa_required
            || account.credential_revision != CredentialRevision::INITIAL
            || !account.must_change_password
            || account.temporary_credential_expiration.is_none()
            || public_identity.account() != account.identifier
            || audit_reference.account() != account.identifier
            || password_verifier.account != account.identifier
        {
            return Err(AccountCredentialMutationError);
        }
        Ok(Self {
            recheck,
            account,
            public_identity,
            audit_reference,
            password_verifier,
        })
    }

    /// Returns the exact issuer recheck.
    pub const fn recheck(&self) -> &AccountCredentialIssuanceRecheck {
        &self.recheck
    }

    /// Returns the complete fixed initial account state.
    pub const fn account(&self) -> &Account {
        &self.account
    }

    /// Returns the independently generated public identity association.
    pub const fn public_identity(&self) -> &AccountPublicIdentity {
        &self.public_identity
    }

    /// Returns the independently generated typed Audit Reference.
    pub const fn audit_reference(&self) -> AccountAuditReference {
        self.audit_reference
    }

    /// Returns the approved verifier eligible for persistence.
    pub const fn password_verifier(&self) -> &AccountPasswordVerifier {
        &self.password_verifier
    }
}

impl fmt::Debug for AccountCreateMutation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AccountCreateMutation(REDACTED)")
    }
}

/// Validated temporary-password reset mutation prepared before its transaction.
pub struct AccountPasswordResetMutation {
    recheck: AccountCredentialIssuanceRecheck,
    target: AccountPasswordResetTarget,
    next_revision: CredentialRevision,
    expiration: TemporaryCredentialExpiration,
    password_verifier: AccountPasswordVerifier,
}

impl AccountPasswordResetMutation {
    /// Validates target ownership and computes the checked successor revision.
    pub fn new(
        recheck: AccountCredentialIssuanceRecheck,
        target: AccountPasswordResetTarget,
        expiration: TemporaryCredentialExpiration,
        password_verifier: AccountPasswordVerifier,
    ) -> Result<Self, AccountCredentialMutationError> {
        if password_verifier.account != target.account {
            return Err(AccountCredentialMutationError);
        }
        let next_revision = target
            .expected_revision
            .checked_next()
            .ok_or(AccountCredentialMutationError)?;
        Ok(Self {
            recheck,
            target,
            next_revision,
            expiration,
            password_verifier,
        })
    }

    /// Returns the exact issuer recheck.
    pub const fn recheck(&self) -> &AccountCredentialIssuanceRecheck {
        &self.recheck
    }

    /// Returns the prepared exact target.
    pub const fn target(&self) -> &AccountPasswordResetTarget {
        &self.target
    }

    /// Returns the checked successor credential revision.
    pub const fn next_revision(&self) -> CredentialRevision {
        self.next_revision
    }

    /// Returns the fixed temporary-credential expiration.
    pub const fn expiration(&self) -> TemporaryCredentialExpiration {
        self.expiration
    }

    /// Returns the approved replacement verifier.
    pub const fn password_verifier(&self) -> &AccountPasswordVerifier {
        &self.password_verifier
    }
}

impl fmt::Debug for AccountPasswordResetMutation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AccountPasswordResetMutation(REDACTED)")
    }
}

/// Payload-free invalid account credential mutation construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccountCredentialMutationError;

impl fmt::Display for AccountCredentialMutationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("account credential mutation is invalid")
    }
}

impl std::error::Error for AccountCredentialMutationError {}

/// The three terminal records prepared for one account credential mutation.
pub struct AccountCredentialAuditTerminalWrites<'a> {
    succeeded: &'a ValidatedAuditTerminalObligationWrite,
    conflict: &'a ValidatedAuditTerminalObligationWrite,
    denied: &'a ValidatedAuditTerminalObligationWrite,
}

impl<'a> AccountCredentialAuditTerminalWrites<'a> {
    /// Binds every terminal alternative before mutation begins.
    #[must_use]
    pub const fn new(
        succeeded: &'a ValidatedAuditTerminalObligationWrite,
        conflict: &'a ValidatedAuditTerminalObligationWrite,
        denied: &'a ValidatedAuditTerminalObligationWrite,
    ) -> Self {
        Self {
            succeeded,
            conflict,
            denied,
        }
    }

    /// Returns the successful business-outcome terminal.
    pub const fn succeeded(&self) -> &ValidatedAuditTerminalObligationWrite {
        self.succeeded
    }

    /// Returns the duplicate-create or stale-reset terminal.
    pub const fn conflict(&self) -> &ValidatedAuditTerminalObligationWrite {
        self.conflict
    }

    /// Returns the final exact-session or credential-state denial terminal.
    pub const fn denied(&self) -> &ValidatedAuditTerminalObligationWrite {
        self.denied
    }
}

impl fmt::Debug for AccountCredentialAuditTerminalWrites<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AccountCredentialAuditTerminalWrites(REDACTED)")
    }
}

/// Authoritative account-create transaction result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountCreateOutcome {
    /// The account, verifier, identities, and success terminal committed.
    Created,
    /// A username or generated identity collided; only the conflict terminal committed.
    Conflict,
    /// Final issuer state was not exact; only the denial terminal committed.
    Denied,
}

/// Authoritative password-reset transaction result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountPasswordResetOutcome {
    /// Credential state and session revocation committed with the success terminal.
    Reset {
        /// Number of target sessions removed by the transaction.
        revoked_sessions: usize,
    },
    /// The prepared target revision changed; only the conflict terminal committed.
    Stale,
    /// Final issuer state was not exact; only the denial terminal committed.
    Denied,
}

/// Backend-neutral preparation and atomic commit of account credential writers.
pub trait AccountCredentialWriterStore {
    /// Resolves one exact public-ID target and its Audit Reference atomically.
    fn prepare_password_reset_target(
        &mut self,
        public_identifier_persistence: &AccountPublicIdentifierPersistence,
        audit_reference_persistence: &AuditReferencePersistence,
        target: AccountPublicIdentifier,
    ) -> Result<Option<AccountPasswordResetTarget>, DatabaseError>;

    /// Commits a new account or one non-success terminal outcome atomically.
    fn create_account(
        &mut self,
        public_identifier_persistence: &AccountPublicIdentifierPersistence,
        mutation: &AccountCreateMutation,
        audit_terminals: &AccountCredentialAuditTerminalWrites<'_>,
    ) -> Result<AccountCreateOutcome, DatabaseError>;

    /// Commits a credential reset and session revocation or one non-success terminal atomically.
    fn reset_account_password(
        &mut self,
        public_identifier_persistence: &AccountPublicIdentifierPersistence,
        mutation: &AccountPasswordResetMutation,
        audit_terminals: &AccountCredentialAuditTerminalWrites<'_>,
    ) -> Result<AccountPasswordResetOutcome, DatabaseError>;
}

#[cfg(test)]
mod tests {
    use weavelit_server_database_authority::ServerDatabaseAuthority;

    use super::*;
    use crate::{
        AccountPasswordVerifier, AccountPublicIdentifierPersistence, AuditReferenceIdentifier,
        AuditReferencePersistence, PasswordVerifier, SESSION_DIGEST_LENGTH,
        STATE_IDENTIFIER_LENGTH,
    };

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

    fn recheck() -> AccountCredentialIssuanceRecheck {
        AccountCredentialIssuanceRecheck::new(
            identifier(1),
            SessionTokenHash::from_bytes([2; SESSION_DIGEST_LENGTH]).unwrap(),
            Name::new("web-ui").unwrap(),
            CredentialRevision::INITIAL,
            SessionInstant::from_unix_milliseconds(1_000).unwrap(),
            AccountCredentialIssuanceFactor::NoneObserved {
                target: MfaModuleTarget {
                    module: Name::new("totp").unwrap(),
                    component: Name::new("totp").unwrap(),
                },
            },
        )
    }

    fn verifier(account: StateIdentifier) -> AccountPasswordVerifier {
        AccountPasswordVerifier {
            account,
            verifier: PasswordVerifier::new("$argon2id$test").unwrap(),
        }
    }

    #[test]
    fn create_requires_the_fixed_initial_temporary_credential_state() {
        let (public_persistence, _) = persistence();
        let account = identifier(3);
        let public_identity =
            AccountPublicIdentity::new(account, AccountPublicIdentifier::generate().unwrap());
        let audit_reference =
            AccountAuditReference::new(account, AuditReferenceIdentifier::generate().unwrap());
        let valid = Account {
            identifier: account,
            username: Name::new("new-user").unwrap(),
            display_name: Some(Name::new("New User").unwrap()),
            active: true,
            mfa_required: false,
            credential_revision: CredentialRevision::INITIAL,
            must_change_password: true,
            temporary_credential_expiration: Some(
                TemporaryCredentialExpiration::from_unix_milliseconds(86_401_000).unwrap(),
            ),
        };

        let mutation = AccountCreateMutation::new(
            recheck(),
            valid.clone(),
            public_identity,
            audit_reference,
            verifier(account),
        )
        .unwrap();
        assert_eq!(mutation.account(), &valid);
        assert_eq!(mutation.public_identity().account(), account);
        assert_eq!(mutation.audit_reference().account(), account);
        assert_eq!(
            public_persistence.encode(&mutation.public_identity().public_identifier()),
            public_persistence.encode(&public_identity.public_identifier())
        );
        assert_eq!(format!("{mutation:?}"), "AccountCreateMutation(REDACTED)");
    }

    #[test]
    fn reset_target_and_mutation_require_exact_associations_and_revision_successor() {
        let (public_persistence, audit_persistence) = persistence();
        let account = identifier(4);
        let public_identifier = AccountPublicIdentifier::generate().unwrap();
        let audit_reference =
            AccountAuditReference::new(account, AuditReferenceIdentifier::generate().unwrap());
        let target = AccountPasswordResetTarget::from_persistence(
            &public_persistence,
            &audit_persistence,
            public_identifier,
            account,
            audit_reference,
            CredentialRevision::INITIAL,
        )
        .unwrap();
        let expiration = TemporaryCredentialExpiration::from_unix_milliseconds(86_401_000).unwrap();
        let mutation =
            AccountPasswordResetMutation::new(recheck(), target, expiration, verifier(account))
                .unwrap();

        assert_eq!(mutation.target().public_identifier(), public_identifier);
        assert_eq!(mutation.target().account(), account);
        assert_eq!(
            mutation.next_revision(),
            CredentialRevision::from_value(2).unwrap()
        );
        assert_eq!(mutation.expiration(), expiration);
        assert_eq!(
            format!("{mutation:?} {:?}", mutation.recheck()),
            "AccountPasswordResetMutation(REDACTED) AccountCredentialIssuanceRecheck(REDACTED)"
        );
    }

    #[test]
    fn target_rejects_an_audit_reference_for_another_account_without_payload() {
        let (public_persistence, audit_persistence) = persistence();
        let error = AccountPasswordResetTarget::from_persistence(
            &public_persistence,
            &audit_persistence,
            AccountPublicIdentifier::generate().unwrap(),
            identifier(4),
            AccountAuditReference::new(
                identifier(5),
                AuditReferenceIdentifier::generate().unwrap(),
            ),
            CredentialRevision::INITIAL,
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "account credential mutation is invalid");
        assert_eq!(format!("{error:?}"), "AccountCredentialMutationError");
    }
}
