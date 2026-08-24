//! Bounded account projections for transport-independent administration reads.

use crate::{AccountPublicIdentifier, AccountPublicIdentifierPersistence, DatabaseError, Name};

/// The complete account data available to an administration read.
///
/// This projection deliberately has no state identifier, Audit Reference,
/// password verifier, MFA factor, session, temporary credential, or extension
/// field. Its private fields keep consumers on this fixed accessor surface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountAdministrationProjection {
    public_identifier: AccountPublicIdentifier,
    username: Name,
    display_name: Option<Name>,
    active: bool,
    mfa_required: bool,
}

impl AccountAdministrationProjection {
    /// Builds one complete projection from trusted persistence values.
    #[must_use]
    pub fn from_persistence(
        _persistence: &AccountPublicIdentifierPersistence,
        public_identifier: AccountPublicIdentifier,
        username: Name,
        display_name: Option<Name>,
        active: bool,
        mfa_required: bool,
    ) -> Self {
        Self {
            public_identifier,
            username,
            display_name,
            active,
            mfa_required,
        }
    }

    /// Returns the account's stable public identifier.
    #[must_use]
    pub const fn public_identifier(&self) -> AccountPublicIdentifier {
        self.public_identifier
    }

    /// Returns the account's unique local username.
    #[must_use]
    pub const fn username(&self) -> &Name {
        &self.username
    }

    /// Returns the account's optional display name.
    #[must_use]
    pub const fn display_name(&self) -> Option<&Name> {
        self.display_name.as_ref()
    }

    /// Returns whether the account may authenticate.
    #[must_use]
    pub const fn active(&self) -> bool {
        self.active
    }

    /// Returns whether the account requires a second factor to authenticate.
    #[must_use]
    pub const fn mfa_required(&self) -> bool {
        self.mfa_required
    }
}

/// Backend-neutral store for bounded account administration reads.
pub trait AccountAdministrationStore {
    /// Lists every account in deterministic ascending username order.
    fn list_account_administration_projections(
        &mut self,
        persistence: &AccountPublicIdentifierPersistence,
    ) -> Result<Vec<AccountAdministrationProjection>, DatabaseError>;

    /// Loads one exact account by its typed public identifier.
    fn load_account_administration_projection(
        &mut self,
        persistence: &AccountPublicIdentifierPersistence,
        public_identifier: AccountPublicIdentifier,
    ) -> Result<Option<AccountAdministrationProjection>, DatabaseError>;
}

#[cfg(test)]
mod tests {
    use weavelit_server_database_authority::ServerDatabaseAuthority;

    use super::*;

    #[test]
    fn projection_exposes_exactly_the_bounded_account_read_values() {
        let persistence = AccountPublicIdentifierPersistence::from_server_authority(
            &ServerDatabaseAuthority::new(),
        );
        let public_identifier = persistence.decode([0x41; 16]).unwrap();
        let projection = AccountAdministrationProjection::from_persistence(
            &persistence,
            public_identifier,
            Name::new("administrator").unwrap(),
            Some(Name::new("Primary Administrator").unwrap()),
            true,
            false,
        );

        assert_eq!(projection.public_identifier(), public_identifier);
        assert_eq!(projection.username().as_str(), "administrator");
        assert_eq!(
            projection.display_name().map(Name::as_str),
            Some("Primary Administrator")
        );
        assert!(projection.active());
        assert!(!projection.mfa_required());
    }
}
