//! Bounded Group projections and atomic Group create, update, and empty-delete contracts.

use std::fmt;

use crate::{
    AuditReferencePersistence, DatabaseError, Description, Group, GroupAuditReference,
    GroupMutationRecheck, GroupPublicIdentifier, GroupPublicIdentifierPersistence,
    GroupPublicIdentity, Name, ValidatedAuditTerminalObligationWrite,
};

/// The complete Group data available to an administration read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupAdministrationProjection {
    public_identifier: GroupPublicIdentifier,
    name: Name,
    description: Option<Description>,
}

impl GroupAdministrationProjection {
    /// Builds one complete projection from trusted persistence values.
    #[must_use]
    pub fn from_persistence(
        _persistence: &GroupPublicIdentifierPersistence,
        public_identifier: GroupPublicIdentifier,
        name: Name,
        description: Option<Description>,
    ) -> Self {
        Self {
            public_identifier,
            name,
            description,
        }
    }

    /// Returns the Group's stable public identifier.
    pub const fn public_identifier(&self) -> GroupPublicIdentifier {
        self.public_identifier
    }

    /// Returns the unique Group name.
    pub const fn name(&self) -> &Name {
        &self.name
    }

    /// Returns the optional Group description.
    pub const fn description(&self) -> Option<&Description> {
        self.description.as_ref()
    }
}

/// Exact persisted Group target prepared for update or deletion.
pub struct GroupAdministrationTarget {
    group: GroupAuditReference,
    projection: GroupAdministrationProjection,
}

impl GroupAdministrationTarget {
    /// Rebuilds one target from authority-decoded persistence values.
    #[must_use]
    pub fn from_persistence(
        _public_identity_persistence: &GroupPublicIdentifierPersistence,
        _audit_reference_persistence: &AuditReferencePersistence,
        group: GroupAuditReference,
        projection: GroupAdministrationProjection,
    ) -> Self {
        Self { group, projection }
    }

    /// Returns the internal Group and Audit Reference used only by trusted workflows.
    pub const fn group(&self) -> GroupAuditReference {
        self.group
    }

    /// Returns the bounded public projection.
    pub const fn projection(&self) -> &GroupAdministrationProjection {
        &self.projection
    }
}

impl fmt::Debug for GroupAdministrationTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GroupAdministrationTarget(REDACTED)")
    }
}

/// Prepared creation of one independently identified empty Group.
pub struct GroupCreateMutation {
    recheck: GroupMutationRecheck,
    group: Group,
    public_identity: GroupPublicIdentity,
    audit_reference: GroupAuditReference,
}

impl GroupCreateMutation {
    /// Requires all three identities to refer to the same new Group.
    pub fn new(
        recheck: GroupMutationRecheck,
        group: Group,
        public_identity: GroupPublicIdentity,
        audit_reference: GroupAuditReference,
    ) -> Result<Self, GroupAdministrationMutationError> {
        if group.identifier != public_identity.group()
            || group.identifier != audit_reference.group()
        {
            return Err(GroupAdministrationMutationError::InvalidTarget);
        }
        Ok(Self {
            recheck,
            group,
            public_identity,
            audit_reference,
        })
    }

    pub const fn recheck(&self) -> &GroupMutationRecheck {
        &self.recheck
    }

    pub const fn group(&self) -> &Group {
        &self.group
    }

    pub const fn public_identity(&self) -> GroupPublicIdentity {
        self.public_identity
    }

    pub const fn audit_reference(&self) -> GroupAuditReference {
        self.audit_reference
    }
}

impl fmt::Debug for GroupCreateMutation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GroupCreateMutation(REDACTED)")
    }
}

/// Prepared exact-target Group name and description replacement.
pub struct GroupUpdateMutation {
    recheck: GroupMutationRecheck,
    target: GroupAdministrationTarget,
    name: Name,
    description: Option<Description>,
}

impl GroupUpdateMutation {
    /// Rejects an exact no-op before an Audit Attempt can be constructed.
    pub fn new(
        recheck: GroupMutationRecheck,
        target: GroupAdministrationTarget,
        name: Name,
        description: Option<Description>,
    ) -> Result<Self, GroupAdministrationMutationError> {
        if target.projection().name() == &name
            && target.projection().description() == description.as_ref()
        {
            return Err(GroupAdministrationMutationError::Unchanged);
        }
        Ok(Self {
            recheck,
            target,
            name,
            description,
        })
    }

    pub const fn recheck(&self) -> &GroupMutationRecheck {
        &self.recheck
    }

    pub const fn target(&self) -> &GroupAdministrationTarget {
        &self.target
    }

    pub const fn name(&self) -> &Name {
        &self.name
    }

    pub const fn description(&self) -> Option<&Description> {
        self.description.as_ref()
    }
}

impl fmt::Debug for GroupUpdateMutation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GroupUpdateMutation(REDACTED)")
    }
}

/// Prepared exact-target deletion of one Group.
pub struct GroupDeleteMutation {
    recheck: GroupMutationRecheck,
    target: GroupAdministrationTarget,
}

impl GroupDeleteMutation {
    #[must_use]
    pub const fn new(recheck: GroupMutationRecheck, target: GroupAdministrationTarget) -> Self {
        Self { recheck, target }
    }

    pub const fn recheck(&self) -> &GroupMutationRecheck {
        &self.recheck
    }

    pub const fn target(&self) -> &GroupAdministrationTarget {
        &self.target
    }
}

impl fmt::Debug for GroupDeleteMutation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GroupDeleteMutation(REDACTED)")
    }
}

/// Payload-free invalid Group mutation construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GroupAdministrationMutationError {
    Unchanged,
    InvalidTarget,
}

impl fmt::Display for GroupAdministrationMutationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Group administration mutation is invalid")
    }
}

impl std::error::Error for GroupAdministrationMutationError {}

/// Prevalidated terminal obligations for Group CRUD outcomes.
pub struct GroupAdministrationAuditTerminalWrites<'a> {
    succeeded: &'a ValidatedAuditTerminalObligationWrite,
    conflict: &'a ValidatedAuditTerminalObligationWrite,
    denied: &'a ValidatedAuditTerminalObligationWrite,
}

impl<'a> GroupAdministrationAuditTerminalWrites<'a> {
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

    pub const fn succeeded(&self) -> &ValidatedAuditTerminalObligationWrite {
        self.succeeded
    }

    pub const fn conflict(&self) -> &ValidatedAuditTerminalObligationWrite {
        self.conflict
    }

    pub const fn denied(&self) -> &ValidatedAuditTerminalObligationWrite {
        self.denied
    }
}

/// Authoritative transaction result for Group creation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GroupCreateOutcome {
    Created,
    Conflict,
    Denied,
}

/// Authoritative transaction result for Group update.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GroupUpdateOutcome {
    Changed,
    Conflict,
    Stale,
    Denied,
}

/// Authoritative transaction result for Group deletion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GroupDeleteOutcome {
    Deleted,
    Nonempty,
    Stale,
    Denied,
}

/// Backend-neutral Group administration reads and writers.
pub trait GroupAdministrationStore {
    fn list_group_administration_projections(
        &mut self,
        public_identity_persistence: &GroupPublicIdentifierPersistence,
    ) -> Result<Vec<GroupAdministrationProjection>, DatabaseError>;

    fn load_group_administration_projection(
        &mut self,
        public_identity_persistence: &GroupPublicIdentifierPersistence,
        public_identifier: GroupPublicIdentifier,
    ) -> Result<Option<GroupAdministrationProjection>, DatabaseError>;

    fn prepare_group_administration_target(
        &mut self,
        public_identity_persistence: &GroupPublicIdentifierPersistence,
        audit_reference_persistence: &AuditReferencePersistence,
        public_identifier: GroupPublicIdentifier,
    ) -> Result<Option<GroupAdministrationTarget>, DatabaseError>;

    fn create_group(
        &mut self,
        public_identity_persistence: &GroupPublicIdentifierPersistence,
        mutation: &GroupCreateMutation,
        audit_terminals: &GroupAdministrationAuditTerminalWrites<'_>,
    ) -> Result<GroupCreateOutcome, DatabaseError>;

    fn update_group(
        &mut self,
        public_identity_persistence: &GroupPublicIdentifierPersistence,
        audit_reference_persistence: &AuditReferencePersistence,
        mutation: &GroupUpdateMutation,
        audit_terminals: &GroupAdministrationAuditTerminalWrites<'_>,
    ) -> Result<GroupUpdateOutcome, DatabaseError>;

    fn delete_group(
        &mut self,
        public_identity_persistence: &GroupPublicIdentifierPersistence,
        audit_reference_persistence: &AuditReferencePersistence,
        mutation: &GroupDeleteMutation,
        audit_terminals: &GroupAdministrationAuditTerminalWrites<'_>,
    ) -> Result<GroupDeleteOutcome, DatabaseError>;
}
