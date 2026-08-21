//! Backend-neutral preparation and atomic persistence contract for existing-Group mutations.

use std::fmt;

use crate::{
    AccountAuditReference, AccountPublicIdentifier, AccountPublicIdentifierPersistence,
    AuditReferencePersistence, DatabaseError, GroupAuditReference, GroupGrant, Name,
    SessionInstant, SessionTokenHash, StateIdentifier, ValidatedAuditTerminalObligationWrite,
};

/// Exact live issuer state a Group mutation writer must recheck.
pub struct GroupMutationRecheck {
    actor: StateIdentifier,
    session: SessionTokenHash,
    client_module: Name,
    now: SessionInstant,
}

impl GroupMutationRecheck {
    /// Binds one authorized Group mutation to its exact live session.
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

impl fmt::Debug for GroupMutationRecheck {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GroupMutationRecheck(REDACTED)")
    }
}

/// Immutable membership target resolved from one consistent backend snapshot.
pub struct GroupMembershipMutationTarget {
    group: GroupAuditReference,
    account_public_identifier: AccountPublicIdentifier,
    account: AccountAuditReference,
    present: bool,
}

impl GroupMembershipMutationTarget {
    /// Rebuilds one exact target from authority-decoded persistence values.
    pub fn from_persistence(
        _public_identifier_persistence: &AccountPublicIdentifierPersistence,
        _audit_reference_persistence: &AuditReferencePersistence,
        group: GroupAuditReference,
        account_public_identifier: AccountPublicIdentifier,
        account: AccountAuditReference,
        present: bool,
    ) -> Result<Self, GroupMutationError> {
        if group.group() == account.account() {
            return Err(GroupMutationError::InvalidTarget);
        }
        Ok(Self {
            group,
            account_public_identifier,
            account,
            present,
        })
    }

    /// Returns the exact existing Group and its Audit Reference.
    pub const fn group(&self) -> GroupAuditReference {
        self.group
    }

    /// Returns the exact account public identifier used for lookup and recheck.
    pub const fn account_public_identifier(&self) -> AccountPublicIdentifier {
        self.account_public_identifier
    }

    /// Returns the exact account and its Audit Reference.
    pub const fn account(&self) -> AccountAuditReference {
        self.account
    }

    /// Returns whether the membership existed during preparation.
    pub const fn present(&self) -> bool {
        self.present
    }
}

impl fmt::Debug for GroupMembershipMutationTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GroupMembershipMutationTarget(REDACTED)")
    }
}

/// Immutable direct-grant target resolved from one consistent backend snapshot.
pub struct GroupGrantMutationTarget {
    group: GroupAuditReference,
    grant: GroupGrant,
    present: bool,
}

impl GroupGrantMutationTarget {
    /// Rebuilds one exact direct-grant target from decoded persistence values.
    pub fn from_persistence(
        _audit_reference_persistence: &AuditReferencePersistence,
        group: GroupAuditReference,
        grant: GroupGrant,
        present: bool,
    ) -> Self {
        Self {
            group,
            grant,
            present,
        }
    }

    /// Returns the exact existing Group and its Audit Reference.
    pub const fn group(&self) -> GroupAuditReference {
        self.group
    }

    /// Returns the canonical direct grant.
    pub const fn grant(&self) -> &GroupGrant {
        &self.grant
    }

    /// Returns whether the direct grant existed during preparation.
    pub const fn present(&self) -> bool {
        self.present
    }
}

impl fmt::Debug for GroupGrantMutationTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GroupGrantMutationTarget(REDACTED)")
    }
}

/// Closed prepared targets for existing-Group mutations.
pub enum GroupMutationTarget {
    /// One account's membership in one Group.
    Membership(GroupMembershipMutationTarget),
    /// One direct grant on one Group.
    Grant(GroupGrantMutationTarget),
}

impl GroupMutationTarget {
    fn present(&self) -> bool {
        match self {
            Self::Membership(target) => target.present(),
            Self::Grant(target) => target.present(),
        }
    }
}

impl fmt::Debug for GroupMutationTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GroupMutationTarget(REDACTED)")
    }
}

/// Validated Group mutation prepared before its transaction begins.
pub struct PreparedGroupMutation {
    recheck: GroupMutationRecheck,
    target: GroupMutationTarget,
    desired: bool,
}

impl PreparedGroupMutation {
    /// Rejects an exact no-op before an Audit Attempt can be constructed.
    pub fn new(
        recheck: GroupMutationRecheck,
        target: GroupMutationTarget,
        desired: bool,
    ) -> Result<Self, GroupMutationError> {
        if target.present() == desired {
            return Err(GroupMutationError::Unchanged);
        }
        Ok(Self {
            recheck,
            target,
            desired,
        })
    }

    /// Returns the exact issuer recheck.
    pub const fn recheck(&self) -> &GroupMutationRecheck {
        &self.recheck
    }

    /// Returns the exact prepared target.
    pub const fn target(&self) -> &GroupMutationTarget {
        &self.target
    }

    /// Returns whether the association must be present after commit.
    pub const fn desired(&self) -> bool {
        self.desired
    }
}

impl fmt::Debug for PreparedGroupMutation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PreparedGroupMutation(REDACTED)")
    }
}

/// Payload-free invalid Group mutation construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GroupMutationError {
    /// The target already has the requested association state.
    Unchanged,
    /// Persisted target associations were not exact.
    InvalidTarget,
}

impl fmt::Display for GroupMutationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Group mutation is invalid")
    }
}

impl std::error::Error for GroupMutationError {}

/// Prevalidated terminal obligations for every authoritative mutation outcome.
pub struct GroupMutationAuditTerminalWrites<'a> {
    succeeded: &'a ValidatedAuditTerminalObligationWrite,
    denied: &'a ValidatedAuditTerminalObligationWrite,
    last_administrator_denied: &'a ValidatedAuditTerminalObligationWrite,
}

impl<'a> GroupMutationAuditTerminalWrites<'a> {
    /// Binds every terminal alternative before mutation begins.
    #[must_use]
    pub const fn new(
        succeeded: &'a ValidatedAuditTerminalObligationWrite,
        denied: &'a ValidatedAuditTerminalObligationWrite,
        last_administrator_denied: &'a ValidatedAuditTerminalObligationWrite,
    ) -> Self {
        Self {
            succeeded,
            denied,
            last_administrator_denied,
        }
    }

    /// Returns the successful business-outcome terminal.
    pub const fn succeeded(&self) -> &ValidatedAuditTerminalObligationWrite {
        self.succeeded
    }

    /// Returns the stale-target or final issuer-denial terminal.
    pub const fn denied(&self) -> &ValidatedAuditTerminalObligationWrite {
        self.denied
    }

    /// Returns the effective-last-active-Administrator denial terminal.
    pub const fn last_administrator_denied(&self) -> &ValidatedAuditTerminalObligationWrite {
        self.last_administrator_denied
    }
}

impl fmt::Debug for GroupMutationAuditTerminalWrites<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GroupMutationAuditTerminalWrites(REDACTED)")
    }
}

/// Authoritative existing-Group mutation transaction result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GroupMutationOutcome {
    /// Exactly one membership or grant row changed with the success terminal.
    Changed,
    /// Current target identity or association state differed from preparation.
    Stale,
    /// Final issuer session or actor state denied the mutation.
    Denied,
    /// Removal would leave no active effective Administrator.
    LastAdministratorDenied,
}

/// Backend-neutral preparation and atomic commit of existing-Group mutations.
pub trait GroupMutationStore {
    /// Resolves one existing Group and account public identifier in one snapshot.
    fn prepare_group_membership_target(
        &mut self,
        public_identifier_persistence: &AccountPublicIdentifierPersistence,
        audit_reference_persistence: &AuditReferencePersistence,
        group: StateIdentifier,
        account: AccountPublicIdentifier,
    ) -> Result<Option<GroupMembershipMutationTarget>, DatabaseError>;

    /// Resolves one existing Group and canonical direct grant in one snapshot.
    fn prepare_group_grant_target(
        &mut self,
        audit_reference_persistence: &AuditReferencePersistence,
        group: StateIdentifier,
        grant: GroupGrant,
    ) -> Result<Option<GroupGrantMutationTarget>, DatabaseError>;

    /// Commits one association change or exactly one denied terminal atomically.
    fn commit_group_mutation(
        &mut self,
        public_identifier_persistence: &AccountPublicIdentifierPersistence,
        audit_reference_persistence: &AuditReferencePersistence,
        mutation: &PreparedGroupMutation,
        audit_terminals: &GroupMutationAuditTerminalWrites<'_>,
    ) -> Result<GroupMutationOutcome, DatabaseError>;
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

    fn recheck() -> GroupMutationRecheck {
        GroupMutationRecheck::new(
            identifier(1),
            SessionTokenHash::from_bytes([2; SESSION_DIGEST_LENGTH]).unwrap(),
            Name::new("web-ui").unwrap(),
            SessionInstant::from_unix_milliseconds(1_000).unwrap(),
        )
    }

    fn membership(present: bool) -> GroupMutationTarget {
        let (public_persistence, audit_persistence) = persistence();
        GroupMutationTarget::Membership(
            GroupMembershipMutationTarget::from_persistence(
                &public_persistence,
                &audit_persistence,
                GroupAuditReference::new(
                    identifier(3),
                    AuditReferenceIdentifier::generate().unwrap(),
                ),
                AccountPublicIdentifier::generate().unwrap(),
                AccountAuditReference::new(
                    identifier(4),
                    AuditReferenceIdentifier::generate().unwrap(),
                ),
                present,
            )
            .unwrap(),
        )
    }

    #[test]
    fn group_mutation_rejects_noop_and_retains_exact_intent() {
        assert_eq!(
            PreparedGroupMutation::new(recheck(), membership(true), true).unwrap_err(),
            GroupMutationError::Unchanged
        );
        let prepared = PreparedGroupMutation::new(recheck(), membership(false), true).unwrap();
        assert!(prepared.desired());
        assert!(matches!(
            prepared.target(),
            GroupMutationTarget::Membership(target) if !target.present()
        ));
        assert_eq!(
            format!("{prepared:?} {:?}", prepared.recheck()),
            "PreparedGroupMutation(REDACTED) GroupMutationRecheck(REDACTED)"
        );
    }

    #[test]
    fn group_mutation_rejects_cross_kind_target_association() {
        let (public_persistence, audit_persistence) = persistence();
        let shared = identifier(3);
        assert_eq!(
            GroupMembershipMutationTarget::from_persistence(
                &public_persistence,
                &audit_persistence,
                GroupAuditReference::new(shared, AuditReferenceIdentifier::generate().unwrap(),),
                AccountPublicIdentifier::generate().unwrap(),
                AccountAuditReference::new(shared, AuditReferenceIdentifier::generate().unwrap(),),
                false,
            )
            .unwrap_err(),
            GroupMutationError::InvalidTarget
        );
    }
}
