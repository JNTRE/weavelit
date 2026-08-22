//! Backend-neutral account status preparation and atomic mutation contract.

use std::fmt;

use crate::{
    AccountAuditReference, AccountPublicIdentifier, AccountPublicIdentifierPersistence,
    AuditReferencePersistence, CredentialRevision, DatabaseError, Name, SessionInstant,
    SessionTokenHash, StateIdentifier, ValidatedAuditTerminalObligationWrite,
};

/// Desired and committed local Human User account status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountStatus {
    /// The account may authenticate and use existing authorization grants.
    Active,
    /// The account may not authenticate or use existing sessions.
    Disabled,
}

impl AccountStatus {
    /// Rebuilds the typed status from the persisted active flag.
    #[must_use]
    pub const fn from_active(active: bool) -> Self {
        if active { Self::Active } else { Self::Disabled }
    }

    /// Returns the persisted active flag.
    #[must_use]
    pub const fn active(self) -> bool {
        matches!(self, Self::Active)
    }
}

/// Exact live issuer state an account status writer must recheck.
pub struct AccountStatusRecheck {
    actor: StateIdentifier,
    session: SessionTokenHash,
    client_module: Name,
    now: SessionInstant,
}

impl AccountStatusRecheck {
    /// Binds one ordinary administration decision to its exact live session.
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

    /// Returns the instant at which final session liveness is judged.
    pub const fn now(&self) -> SessionInstant {
        self.now
    }
}

impl fmt::Debug for AccountStatusRecheck {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AccountStatusRecheck(REDACTED)")
    }
}

/// Immutable account status target read from one consistent backend snapshot.
pub struct AccountStatusTarget {
    public_identifier: AccountPublicIdentifier,
    account: StateIdentifier,
    audit_reference: AccountAuditReference,
    status: AccountStatus,
    credential_revision: CredentialRevision,
}

impl AccountStatusTarget {
    /// Rebuilds a prepared target from authority-decoded persistence values.
    pub fn from_persistence(
        _public_identifier_persistence: &AccountPublicIdentifierPersistence,
        _audit_reference_persistence: &AuditReferencePersistence,
        public_identifier: AccountPublicIdentifier,
        account: StateIdentifier,
        audit_reference: AccountAuditReference,
        active: bool,
        credential_revision: CredentialRevision,
    ) -> Result<Self, AccountStatusMutationError> {
        if audit_reference.account() != account {
            return Err(AccountStatusMutationError::InvalidTarget);
        }
        Ok(Self {
            public_identifier,
            account,
            audit_reference,
            status: AccountStatus::from_active(active),
            credential_revision,
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

    /// Returns the status observed during preparation.
    pub const fn status(&self) -> AccountStatus {
        self.status
    }

    /// Returns the credential revision observed during preparation.
    pub const fn credential_revision(&self) -> CredentialRevision {
        self.credential_revision
    }
}

impl fmt::Debug for AccountStatusTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AccountStatusTarget(REDACTED)")
    }
}

/// Validated status mutation prepared before its transaction begins.
pub struct AccountStatusMutation {
    recheck: AccountStatusRecheck,
    target: AccountStatusTarget,
    desired: AccountStatus,
    resulting_revision: CredentialRevision,
}

impl AccountStatusMutation {
    /// Rejects a no-op and computes the checked revision used by the final CAS.
    pub fn new(
        recheck: AccountStatusRecheck,
        target: AccountStatusTarget,
        desired: AccountStatus,
    ) -> Result<Self, AccountStatusMutationError> {
        if target.status == desired {
            return Err(AccountStatusMutationError::Unchanged);
        }
        let resulting_revision = match desired {
            AccountStatus::Active => target.credential_revision,
            AccountStatus::Disabled => target
                .credential_revision
                .checked_next()
                .ok_or(AccountStatusMutationError::CredentialRevisionExhausted)?,
        };
        Ok(Self {
            recheck,
            target,
            desired,
            resulting_revision,
        })
    }

    /// Returns the exact issuer recheck.
    pub const fn recheck(&self) -> &AccountStatusRecheck {
        &self.recheck
    }

    /// Returns the prepared exact target.
    pub const fn target(&self) -> &AccountStatusTarget {
        &self.target
    }

    /// Returns the status to commit.
    pub const fn desired(&self) -> AccountStatus {
        self.desired
    }

    /// Returns the revision to commit with the status.
    pub const fn resulting_revision(&self) -> CredentialRevision {
        self.resulting_revision
    }
}

impl fmt::Debug for AccountStatusMutation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AccountStatusMutation(REDACTED)")
    }
}

/// Payload-free invalid account status mutation construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountStatusMutationError {
    /// The target already has the requested status.
    Unchanged,
    /// Disablement cannot advance the target's maximal credential revision.
    CredentialRevisionExhausted,
    /// Persisted target associations were not exact.
    InvalidTarget,
}

impl fmt::Display for AccountStatusMutationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("account status mutation is invalid")
    }
}

impl std::error::Error for AccountStatusMutationError {}

/// The terminal records prepared for one account status mutation.
pub struct AccountStatusAuditTerminalWrites<'a> {
    succeeded: &'a ValidatedAuditTerminalObligationWrite,
    denied: &'a ValidatedAuditTerminalObligationWrite,
}

impl<'a> AccountStatusAuditTerminalWrites<'a> {
    /// Binds every terminal alternative before mutation begins.
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

impl fmt::Debug for AccountStatusAuditTerminalWrites<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AccountStatusAuditTerminalWrites(REDACTED)")
    }
}

/// Authoritative account status transaction result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountStatusMutationOutcome {
    /// The desired status and selected success terminal committed.
    Changed {
        /// Number of target sessions revoked by disablement.
        revoked_sessions: usize,
    },
    /// The prepared target identity, status, or revision changed.
    Stale,
    /// Final issuer session state was not exact.
    Denied,
}

/// Backend-neutral preparation and atomic commit of account status writers.
pub trait AccountStatusWriterStore {
    /// Resolves one exact public-ID target and immutable status preparation.
    fn prepare_account_status_target(
        &mut self,
        public_identifier_persistence: &AccountPublicIdentifierPersistence,
        audit_reference_persistence: &AuditReferencePersistence,
        target: AccountPublicIdentifier,
    ) -> Result<Option<AccountStatusTarget>, DatabaseError>;

    /// Commits one status mutation or one denied terminal atomically.
    fn change_account_status(
        &mut self,
        public_identifier_persistence: &AccountPublicIdentifierPersistence,
        mutation: &AccountStatusMutation,
        audit_terminals: &AccountStatusAuditTerminalWrites<'_>,
    ) -> Result<AccountStatusMutationOutcome, DatabaseError>;
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

    fn recheck() -> AccountStatusRecheck {
        AccountStatusRecheck::new(
            identifier(1),
            SessionTokenHash::from_bytes([2; SESSION_DIGEST_LENGTH]).unwrap(),
            Name::new("web-ui").unwrap(),
            SessionInstant::from_unix_milliseconds(1_000).unwrap(),
        )
    }

    fn target(active: bool, revision: u64) -> AccountStatusTarget {
        let (public_persistence, audit_persistence) = persistence();
        let account = identifier(3);
        AccountStatusTarget::from_persistence(
            &public_persistence,
            &audit_persistence,
            AccountPublicIdentifier::generate().unwrap(),
            account,
            AccountAuditReference::new(account, AuditReferenceIdentifier::generate().unwrap()),
            active,
            CredentialRevision::from_value(revision).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn disable_advances_revision_and_reenable_preserves_it() {
        let disable =
            AccountStatusMutation::new(recheck(), target(true, 7), AccountStatus::Disabled)
                .unwrap();
        assert_eq!(disable.desired(), AccountStatus::Disabled);
        assert_eq!(
            disable.resulting_revision(),
            CredentialRevision::from_value(8).unwrap()
        );

        let reenable =
            AccountStatusMutation::new(recheck(), target(false, 7), AccountStatus::Active).unwrap();
        assert_eq!(reenable.desired(), AccountStatus::Active);
        assert_eq!(
            reenable.resulting_revision(),
            CredentialRevision::from_value(7).unwrap()
        );
    }

    #[test]
    fn unchanged_and_exhausted_disable_are_rejected_before_a_store_call() {
        assert_eq!(
            AccountStatusMutation::new(recheck(), target(true, 7), AccountStatus::Active)
                .unwrap_err(),
            AccountStatusMutationError::Unchanged
        );
        assert_eq!(
            AccountStatusMutation::new(recheck(), target(true, u64::MAX), AccountStatus::Disabled,)
                .unwrap_err(),
            AccountStatusMutationError::CredentialRevisionExhausted
        );
    }

    #[test]
    fn target_associations_and_diagnostics_are_payload_free() {
        let (public_persistence, audit_persistence) = persistence();
        let error = AccountStatusTarget::from_persistence(
            &public_persistence,
            &audit_persistence,
            AccountPublicIdentifier::generate().unwrap(),
            identifier(3),
            AccountAuditReference::new(
                identifier(4),
                AuditReferenceIdentifier::generate().unwrap(),
            ),
            true,
            CredentialRevision::INITIAL,
        )
        .unwrap_err();

        assert_eq!(error, AccountStatusMutationError::InvalidTarget);
        assert_eq!(error.to_string(), "account status mutation is invalid");
        assert_eq!(format!("{error:?}"), "InvalidTarget");
        assert_eq!(
            format!("{:?} {:?}", recheck(), target(true, 1)),
            "AccountStatusRecheck(REDACTED) AccountStatusTarget(REDACTED)"
        );
    }
}
