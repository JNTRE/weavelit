//! Catalogued components an authorization decision is evaluated against.
//!
//! The catalog answers only "is this component present and enabled, and what
//! does it declare". It holds no grant, so a catalog change can never widen a
//! caller's access; it can only narrow it.

use std::collections::BTreeMap;
use std::{error::Error as StdError, fmt};

use weavelit_server_database::Name;

/// A plane a Client Module may declare on its authenticated surface.
///
/// Every match on this enum in this crate is exhaustive with no wildcard arm,
/// so adding a plane fails to compile until each decision states how it treats
/// the new plane.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Plane {
    /// The User Plane, which carries non-administrative functions.
    User,
    /// The Administration Plane, which carries server-administration functions.
    Administration,
}

/// One catalogued Client Module, its enablement, and the planes it declares.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientModuleDeclaration {
    name: Name,
    user_plane: bool,
    administration_plane: bool,
    enabled: bool,
}

impl ClientModuleDeclaration {
    /// Declares a Client Module, its enablement, and the planes it exposes.
    #[must_use]
    pub fn new(name: Name, enabled: bool, planes: &[Plane]) -> Self {
        Self {
            name,
            user_plane: planes.contains(&Plane::User),
            administration_plane: planes.contains(&Plane::Administration),
            enabled,
        }
    }

    /// Returns the declared Client Module name.
    #[must_use]
    pub const fn name(&self) -> &Name {
        &self.name
    }

    /// Returns whether the Client Module is enabled.
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    /// Returns whether the Client Module declares the plane.
    #[must_use]
    pub const fn declares(&self, plane: Plane) -> bool {
        match plane {
            Plane::User => self.user_plane,
            Plane::Administration => self.administration_plane,
        }
    }
}

/// One catalogued Service Module and its enablement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceModuleDeclaration {
    name: Name,
    enabled: bool,
}

impl ServiceModuleDeclaration {
    /// Declares a Service Module and its enablement.
    #[must_use]
    pub const fn new(name: Name, enabled: bool) -> Self {
        Self { name, enabled }
    }

    /// Returns the declared Service Module name.
    #[must_use]
    pub const fn name(&self) -> &Name {
        &self.name
    }

    /// Returns whether the Service Module is enabled.
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }
}

/// One catalogued named Operation, its enablement, and its owning Service Module.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationDeclaration {
    name: Name,
    service_module: Name,
    enabled: bool,
}

impl OperationDeclaration {
    /// Declares a named Operation, its owner, and its enablement.
    #[must_use]
    pub const fn new(name: Name, service_module: Name, enabled: bool) -> Self {
        Self {
            name,
            service_module,
            enabled,
        }
    }

    /// Returns the declared Operation name.
    #[must_use]
    pub const fn name(&self) -> &Name {
        &self.name
    }

    /// Returns the Service Module that implements the Operation.
    #[must_use]
    pub const fn service_module(&self) -> &Name {
        &self.service_module
    }

    /// Returns whether the Operation is enabled.
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }
}

/// Invalid catalog construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogError {
    /// Two declarations of the same kind used the same name.
    DuplicateDeclaration,
}

impl fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("authorization catalog declaration is duplicated")
    }
}

impl StdError for CatalogError {}

/// The catalogued Client Modules, Service Modules, and named Operations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizationCatalog {
    client_modules: BTreeMap<Name, ClientModuleDeclaration>,
    service_modules: BTreeMap<Name, ServiceModuleDeclaration>,
    operations: BTreeMap<Name, OperationDeclaration>,
}

impl AuthorizationCatalog {
    /// Builds the catalog and rejects a repeated name of any one kind.
    pub fn new(
        client_modules: Vec<ClientModuleDeclaration>,
        service_modules: Vec<ServiceModuleDeclaration>,
        operations: Vec<OperationDeclaration>,
    ) -> Result<Self, CatalogError> {
        Ok(Self {
            client_modules: indexed(client_modules, |declaration| declaration.name.clone())?,
            service_modules: indexed(service_modules, |declaration| declaration.name.clone())?,
            operations: indexed(operations, |declaration| declaration.name.clone())?,
        })
    }

    /// Returns the Client Module declaration, or `None` when it is not catalogued.
    ///
    /// An absent entry is not an error. Every evaluator treats it exactly as it
    /// treats a disabled entry, so an unknown component denies by default.
    #[must_use]
    pub fn client_module(&self, name: &Name) -> Option<&ClientModuleDeclaration> {
        self.client_modules.get(name)
    }

    /// Returns the Service Module declaration, or `None` when it is not catalogued.
    #[must_use]
    pub fn service_module(&self, name: &Name) -> Option<&ServiceModuleDeclaration> {
        self.service_modules.get(name)
    }

    /// Returns the Operation declaration, or `None` when it is not catalogued.
    #[must_use]
    pub fn operation(&self, name: &Name) -> Option<&OperationDeclaration> {
        self.operations.get(name)
    }
}

fn indexed<D>(
    declarations: Vec<D>,
    key: impl Fn(&D) -> Name,
) -> Result<BTreeMap<Name, D>, CatalogError> {
    let mut indexed = BTreeMap::new();
    for declaration in declarations {
        if indexed.insert(key(&declaration), declaration).is_some() {
            return Err(CatalogError::DuplicateDeclaration);
        }
    }

    Ok(indexed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(value: &str) -> Name {
        Name::new(value).expect("a bounded printable name is valid")
    }

    #[test]
    fn a_client_module_declares_only_the_planes_it_was_given() {
        let user_only = ClientModuleDeclaration::new(name("cli"), true, &[Plane::User]);
        let both = ClientModuleDeclaration::new(
            name("web-ui"),
            true,
            &[Plane::User, Plane::Administration],
        );
        let neither = ClientModuleDeclaration::new(name("mcp"), true, &[]);

        assert!(user_only.declares(Plane::User));
        assert!(!user_only.declares(Plane::Administration));
        assert!(both.declares(Plane::User));
        assert!(both.declares(Plane::Administration));
        assert!(!neither.declares(Plane::User));
        assert!(!neither.declares(Plane::Administration));
    }

    #[test]
    fn an_uncatalogued_name_is_absent_rather_than_an_error() {
        let catalog = AuthorizationCatalog::new(
            vec![ClientModuleDeclaration::new(
                name("web-ui"),
                true,
                &[Plane::User],
            )],
            vec![ServiceModuleDeclaration::new(name("zendesk"), true)],
            vec![OperationDeclaration::new(
                name("zendesk.ticket.create"),
                name("zendesk"),
                true,
            )],
        )
        .expect("distinct names build a catalog");

        assert!(catalog.client_module(&name("web-ui")).is_some());
        assert!(catalog.client_module(&name("unknown")).is_none());
        assert!(catalog.service_module(&name("unknown")).is_none());
        assert!(catalog.operation(&name("unknown")).is_none());
    }

    #[test]
    fn a_repeated_declaration_name_is_rejected() {
        let error = AuthorizationCatalog::new(
            vec![
                ClientModuleDeclaration::new(name("web-ui"), true, &[Plane::User]),
                ClientModuleDeclaration::new(name("web-ui"), false, &[]),
            ],
            Vec::new(),
            Vec::new(),
        )
        .expect_err("a repeated Client Module name must be rejected");

        assert_eq!(error, CatalogError::DuplicateDeclaration);
    }
}
