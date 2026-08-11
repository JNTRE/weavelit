//! A Human User's effective grants, folded additively across every Group.

use std::collections::BTreeSet;

use weavelit_server_database::{GroupGrant, HumanAuthorizationSnapshot, Name};

/// The effective grants that name a component the caller may reach.
///
/// This type deliberately has no Server Administration Permission member. The
/// User Plane evaluator receives only this value, so it cannot read the
/// permission even by mistake, and holding the permission cannot stand in for
/// a Client Module, Service Module, or Operation grant.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OperationalGrants {
    client_modules: BTreeSet<Name>,
    service_modules: BTreeSet<Name>,
    operations: BTreeSet<Name>,
}

impl OperationalGrants {
    /// Returns every granted Client Module name in canonical order.
    pub fn client_modules(&self) -> impl ExactSizeIterator<Item = &Name> {
        self.client_modules.iter()
    }

    /// Returns every granted Service Module name in canonical order.
    pub fn service_modules(&self) -> impl ExactSizeIterator<Item = &Name> {
        self.service_modules.iter()
    }

    /// Returns every granted Operation name in canonical order.
    pub fn operations(&self) -> impl ExactSizeIterator<Item = &Name> {
        self.operations.iter()
    }

    /// Returns whether the exact Client Module name is granted.
    #[must_use]
    pub fn grants_client_module(&self, name: &Name) -> bool {
        self.client_modules.contains(name)
    }

    /// Returns whether the exact Service Module name is granted.
    #[must_use]
    pub fn grants_service_module(&self, name: &Name) -> bool {
        self.service_modules.contains(name)
    }

    /// Returns whether the exact Operation name is granted.
    ///
    /// The comparison is by whole name. There is no wildcard, prefix, or
    /// Service-Module-wide Operation grant, so a newly registered Operation is
    /// unreachable until a Group grants that exact name.
    #[must_use]
    pub fn grants_operation(&self, name: &Name) -> bool {
        self.operations.contains(name)
    }
}

/// Whether the effective grants include the Server Administration Permission.
///
/// This is a two-state enum rather than a boolean so the administration
/// decision must match it exhaustively, and so a future permission state
/// fails to compile until that decision states how it treats it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ServerAdministrationPermission {
    /// No membership Group confers the permission.
    #[default]
    Absent,
    /// At least one membership Group confers the permission.
    Granted,
}

/// A Human User's complete effective grants.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EffectiveHumanGrants {
    operational: OperationalGrants,
    administration: ServerAdministrationPermission,
}

impl EffectiveHumanGrants {
    /// Folds the grants of every membership Group into one additive union.
    ///
    /// The fold is purely additive: a grant is present when any Group confers
    /// it, no Group can remove another Group's grant, and repeating a grant
    /// across Groups changes nothing. The match over the grant kinds is
    /// exhaustive, so a new grant kind fails to compile until this fold states
    /// where it belongs.
    #[must_use]
    pub fn from_snapshot(snapshot: &HumanAuthorizationSnapshot) -> Self {
        let mut folded = Self::default();
        for grant in snapshot.grants() {
            match grant {
                GroupGrant::ClientModule(module) => {
                    folded.operational.client_modules.insert(module.clone());
                }
                GroupGrant::ServiceModule(module) => {
                    folded.operational.service_modules.insert(module.clone());
                }
                GroupGrant::Operation(operation) => {
                    folded.operational.operations.insert(operation.clone());
                }
                GroupGrant::ServerAdministration => {
                    folded.administration = ServerAdministrationPermission::Granted;
                }
            }
        }

        folded
    }

    /// Returns the grants that name a reachable component.
    #[must_use]
    pub const fn operational(&self) -> &OperationalGrants {
        &self.operational
    }

    /// Returns whether the Server Administration Permission is effective.
    #[must_use]
    pub const fn administration(&self) -> ServerAdministrationPermission {
        self.administration
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(value: &str) -> Name {
        Name::new(value).expect("a bounded printable name is valid")
    }

    fn names<'a>(grants: impl ExactSizeIterator<Item = &'a Name>) -> Vec<String> {
        grants.map(|value| value.as_str().to_owned()).collect()
    }

    #[test]
    fn two_groups_union_their_overlapping_grants_exactly_once() {
        // The projection is the join across both Groups, so an overlapping
        // grant arrives once per conferring Group.
        let snapshot = HumanAuthorizationSnapshot::new(
            true,
            vec![
                // First Group.
                GroupGrant::ClientModule(name("web-ui")),
                GroupGrant::ServiceModule(name("zendesk")),
                GroupGrant::Operation(name("zendesk.ticket.create")),
                // Second Group, overlapping on the Client Module, the Service
                // Module, and one Operation.
                GroupGrant::ClientModule(name("web-ui")),
                GroupGrant::ClientModule(name("cli")),
                GroupGrant::ServiceModule(name("zendesk")),
                GroupGrant::Operation(name("zendesk.ticket.create")),
                GroupGrant::Operation(name("zendesk.ticket.comment")),
            ],
        );

        let grants = EffectiveHumanGrants::from_snapshot(&snapshot);

        assert_eq!(
            names(grants.operational().client_modules()),
            ["cli", "web-ui"]
        );
        assert_eq!(names(grants.operational().service_modules()), ["zendesk"]);
        assert_eq!(
            names(grants.operational().operations()),
            ["zendesk.ticket.comment", "zendesk.ticket.create"]
        );
        assert_eq!(
            grants.administration(),
            ServerAdministrationPermission::Absent
        );
    }

    #[test]
    fn one_group_conferring_administration_makes_the_permission_effective() {
        let snapshot = HumanAuthorizationSnapshot::new(
            true,
            vec![
                GroupGrant::ClientModule(name("web-ui")),
                GroupGrant::ServerAdministration,
            ],
        );

        let grants = EffectiveHumanGrants::from_snapshot(&snapshot);

        assert_eq!(
            grants.administration(),
            ServerAdministrationPermission::Granted
        );
        // The permission adds no operational grant of any kind.
        assert_eq!(names(grants.operational().client_modules()), ["web-ui"]);
        assert_eq!(grants.operational().service_modules().len(), 0);
        assert_eq!(grants.operational().operations().len(), 0);
    }

    #[test]
    fn no_membership_leaves_every_grant_absent() {
        let grants =
            EffectiveHumanGrants::from_snapshot(&HumanAuthorizationSnapshot::new(true, Vec::new()));

        assert_eq!(grants, EffectiveHumanGrants::default());
        assert!(!grants.operational().grants_client_module(&name("web-ui")));
        assert!(!grants.operational().grants_service_module(&name("zendesk")));
        assert!(
            !grants
                .operational()
                .grants_operation(&name("zendesk.ticket.create"))
        );
    }

    #[test]
    fn an_operation_grant_matches_only_its_exact_name() {
        let snapshot = HumanAuthorizationSnapshot::new(
            true,
            vec![GroupGrant::Operation(name("zendesk.ticket.create"))],
        );

        let grants = EffectiveHumanGrants::from_snapshot(&snapshot);
        let operational = grants.operational();

        assert!(operational.grants_operation(&name("zendesk.ticket.create")));
        for near_miss in [
            "zendesk.ticket",
            "zendesk.ticket.",
            "zendesk.ticket.create.extra",
            "zendesk.*",
            "*",
        ] {
            assert!(
                !operational.grants_operation(&name(near_miss)),
                "{near_miss} must not match an exact Operation grant"
            );
        }
    }
}
