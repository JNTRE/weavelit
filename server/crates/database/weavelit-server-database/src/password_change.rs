//! Atomic replacement of a temporary credential from its restricted session.

use std::fmt;

use crate::{
    AccountPasswordVerifier, CredentialRevision, DatabaseError, Name, NewSession, PasswordVerifier,
    SessionInstant, SessionTokenHash, StateIdentifier, ValidatedAuditTerminalObligationWrite,
};

/// Exact restricted-session and credential state a password change must recheck.
pub struct PasswordChangeRecheck {
    account: StateIdentifier,
    session: SessionTokenHash,
    client_module: Name,
    expected_revision: CredentialRevision,
    expected_verifier: PasswordVerifier,
    now: SessionInstant,
}

impl PasswordChangeRecheck {
    /// Binds a prepared replacement to the restricted session and credential it observed.
    #[must_use]
    pub const fn new(
        account: StateIdentifier,
        session: SessionTokenHash,
        client_module: Name,
        expected_revision: CredentialRevision,
        expected_verifier: PasswordVerifier,
        now: SessionInstant,
    ) -> Self {
        Self {
            account,
            session,
            client_module,
            expected_revision,
            expected_verifier,
            now,
        }
    }

    pub const fn account(&self) -> StateIdentifier {
        self.account
    }

    pub const fn session(&self) -> &SessionTokenHash {
        &self.session
    }

    pub const fn client_module(&self) -> &Name {
        &self.client_module
    }

    pub const fn expected_revision(&self) -> CredentialRevision {
        self.expected_revision
    }

    pub const fn expected_verifier(&self) -> &PasswordVerifier {
        &self.expected_verifier
    }

    pub const fn now(&self) -> SessionInstant {
        self.now
    }
}

impl fmt::Debug for PasswordChangeRecheck {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PasswordChangeRecheck(REDACTED)")
    }
}

/// Complete password replacement and fresh ordinary session prepared before mutation.
pub struct PasswordChangeMutation {
    recheck: PasswordChangeRecheck,
    next_revision: CredentialRevision,
    replacement: AccountPasswordVerifier,
    fresh_session: NewSession,
}

impl PasswordChangeMutation {
    /// Validates that the replacement and fresh session belong to the exact successor state.
    pub fn new(
        recheck: PasswordChangeRecheck,
        replacement: AccountPasswordVerifier,
        fresh_session: NewSession,
    ) -> Result<Self, PasswordChangeMutationError> {
        let next_revision = recheck
            .expected_revision
            .checked_next()
            .ok_or(PasswordChangeMutationError)?;
        if replacement.account != recheck.account
            || fresh_session.account() != recheck.account
            || fresh_session.client_module() != &recheck.client_module
            || fresh_session.expected_credential_revision() != next_revision
            || fresh_session.issued_at() != recheck.now
        {
            return Err(PasswordChangeMutationError);
        }
        Ok(Self {
            recheck,
            next_revision,
            replacement,
            fresh_session,
        })
    }

    pub const fn recheck(&self) -> &PasswordChangeRecheck {
        &self.recheck
    }

    pub const fn next_revision(&self) -> CredentialRevision {
        self.next_revision
    }

    pub const fn replacement(&self) -> &AccountPasswordVerifier {
        &self.replacement
    }

    pub const fn fresh_session(&self) -> &NewSession {
        &self.fresh_session
    }
}

impl fmt::Debug for PasswordChangeMutation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PasswordChangeMutation(REDACTED)")
    }
}

/// Payload-free invalid password-change mutation construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PasswordChangeMutationError;

impl fmt::Display for PasswordChangeMutationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("password change mutation is invalid")
    }
}

impl std::error::Error for PasswordChangeMutationError {}

/// Every terminal alternative prepared before a password-change transaction.
pub struct PasswordChangeAuditTerminalWrites<'a> {
    succeeded: &'a ValidatedAuditTerminalObligationWrite,
    denied: &'a ValidatedAuditTerminalObligationWrite,
}

impl<'a> PasswordChangeAuditTerminalWrites<'a> {
    #[must_use]
    pub const fn new(
        succeeded: &'a ValidatedAuditTerminalObligationWrite,
        denied: &'a ValidatedAuditTerminalObligationWrite,
    ) -> Self {
        Self { succeeded, denied }
    }

    pub const fn succeeded(&self) -> &ValidatedAuditTerminalObligationWrite {
        self.succeeded
    }

    pub const fn denied(&self) -> &ValidatedAuditTerminalObligationWrite {
        self.denied
    }
}

impl fmt::Debug for PasswordChangeAuditTerminalWrites<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PasswordChangeAuditTerminalWrites(REDACTED)")
    }
}

/// Authoritative result of the final atomic password-change decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PasswordChangeOutcome {
    /// The credential was replaced and a fresh ordinary session was inserted.
    Changed { revoked_sessions: usize },
    /// Session, account, revision, expiry, or current verifier was no longer exact.
    Denied,
}

/// Backend-neutral atomic password-change writer.
pub trait PasswordChangeWriterStore {
    fn change_password(
        &mut self,
        mutation: &PasswordChangeMutation,
        audit_terminals: &PasswordChangeAuditTerminalWrites<'_>,
    ) -> Result<PasswordChangeOutcome, DatabaseError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AccountPasswordVerifier, PasswordVerifier, SESSION_DIGEST_LENGTH, SessionCsrfHash,
    };

    fn account(byte: u8) -> StateIdentifier {
        StateIdentifier::from_bytes([byte; 16]).unwrap()
    }

    fn mutation(
        fresh_account: u8,
        fresh_revision: u64,
    ) -> Result<PasswordChangeMutation, PasswordChangeMutationError> {
        let actor = account(1);
        let now = SessionInstant::from_unix_milliseconds(1_000).unwrap();
        PasswordChangeMutation::new(
            PasswordChangeRecheck::new(
                actor,
                SessionTokenHash::from_bytes([2; SESSION_DIGEST_LENGTH]).unwrap(),
                Name::new("web-ui").unwrap(),
                CredentialRevision::INITIAL,
                PasswordVerifier::new("$current").unwrap(),
                now,
            ),
            AccountPasswordVerifier {
                account: actor,
                verifier: PasswordVerifier::new("$replacement").unwrap(),
            },
            NewSession::new(
                SessionTokenHash::from_bytes([3; SESSION_DIGEST_LENGTH]).unwrap(),
                SessionCsrfHash::from_bytes([4; SESSION_DIGEST_LENGTH]).unwrap(),
                account(fresh_account),
                CredentialRevision::from_value(fresh_revision).unwrap(),
                Name::new("web-ui").unwrap(),
                now,
            ),
        )
    }

    #[test]
    fn mutation_binds_the_fresh_session_to_the_successor_credential_state() {
        let valid = mutation(1, 2).unwrap();
        assert_eq!(
            valid.next_revision(),
            CredentialRevision::from_value(2).unwrap()
        );
        assert_eq!(valid.fresh_session().account(), account(1));
        assert_eq!(
            format!("{valid:?} {:?}", valid.recheck()),
            "PasswordChangeMutation(REDACTED) PasswordChangeRecheck(REDACTED)"
        );

        assert_eq!(mutation(2, 2).unwrap_err(), PasswordChangeMutationError);
        assert_eq!(mutation(1, 1).unwrap_err(), PasswordChangeMutationError);
    }
}
