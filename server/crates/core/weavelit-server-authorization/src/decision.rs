//! The two authorization decisions and the proofs they produce.
//!
//! Each decision runs its checks left to right and denies at the first failure:
//! an inactive account, then a disabled or uncatalogued component, then a
//! missing grant. Only the final branch of each decision constructs a proof,
//! and the proof constructors are private to this crate, so a proof value can
//! come from nowhere else.

use std::{error::Error as StdError, fmt};

use weavelit_server_database::{HumanAuthorizationSnapshot, Name};

use crate::catalog::{AuthorizationCatalog, Plane};
use crate::grants::{EffectiveHumanGrants, OperationalGrants, ServerAdministrationPermission};

/// The single denial every unsuccessful authorization returns.
///
/// There is exactly one denial value, so no branch can report which check
/// failed. An inactive account, a disabled or uncatalogued Client Module,
/// Service Module, or Operation, an Operation owned by a different Service
/// Module, and every missing grant are indistinguishable to the caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthorizationDenied;

impl fmt::Display for AuthorizationDenied {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("request authorization denied")
    }
}

impl StdError for AuthorizationDenied {}

/// One requested User Plane Operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UserOperationRequest<'a> {
    /// Client Module the authenticated session was established for.
    pub client_module: &'a Name,
    /// Service Module expected to implement the Operation.
    pub service_module: &'a Name,
    /// Named Operation being requested.
    pub operation: &'a Name,
}

/// One requested Administration Plane function.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdministrationRequest<'a> {
    /// Client Module the authenticated session was established for.
    pub client_module: &'a Name,
}

/// Proof that one named Operation was authorized for a Human User.
///
/// The fields and the constructor are private to this crate, so this value
/// exists only where [`authorize_user_operation`] allowed the request. It is
/// deliberately not `Default` and not `Clone`.
#[derive(Debug, Eq, PartialEq)]
pub struct AuthorizedOperation {
    client_module: Name,
    service_module: Name,
    operation: Name,
}

impl AuthorizedOperation {
    fn granted(client_module: Name, service_module: Name, operation: Name) -> Self {
        Self {
            client_module,
            service_module,
            operation,
        }
    }

    /// Returns the Client Module the Operation was authorized through.
    #[must_use]
    pub const fn client_module(&self) -> &Name {
        &self.client_module
    }

    /// Returns the Service Module that implements the authorized Operation.
    #[must_use]
    pub const fn service_module(&self) -> &Name {
        &self.service_module
    }

    /// Returns the authorized Operation name.
    #[must_use]
    pub const fn operation(&self) -> &Name {
        &self.operation
    }
}

/// Proof that one Administration Plane function was authorized.
///
/// The field and the constructor are private to this crate, so this value
/// exists only where [`authorize_administration`] allowed the request. It is
/// deliberately not `Default` and not `Clone`.
#[derive(Debug, Eq, PartialEq)]
pub struct AuthorizedAdministration {
    client_module: Name,
}

impl AuthorizedAdministration {
    fn granted(client_module: Name) -> Self {
        Self { client_module }
    }

    /// Returns the Client Module the Administration Plane was authorized through.
    #[must_use]
    pub const fn client_module(&self) -> &Name {
        &self.client_module
    }
}

/// Authorizes one User Plane Operation for a Human User.
///
/// The account must be active; the Client Module must be catalogued, enabled,
/// and declare the User Plane; the Service Module must be catalogued and
/// enabled; the Operation must be catalogued, enabled, and owned by that
/// Service Module; and the effective grants must name that Client Module, that
/// Service Module, and that exact Operation.
pub fn authorize_user_operation(
    account: &HumanAuthorizationSnapshot,
    catalog: &AuthorizationCatalog,
    request: UserOperationRequest<'_>,
) -> Result<AuthorizedOperation, AuthorizationDenied> {
    if !account.active() {
        return Err(AuthorizationDenied);
    }

    let grants = EffectiveHumanGrants::from_snapshot(account);
    // Only the operational grants are handed on, so the Server Administration
    // Permission is not a value this decision can consult.
    evaluate_user_operation(grants.operational(), catalog, request)
}

/// Authorizes one Administration Plane function for a Human User.
///
/// The account must be active; the Client Module must be catalogued, enabled,
/// and declare the Administration Plane; the effective grants must name that
/// Client Module; and the Server Administration Permission must be effective.
/// No Service Module or Operation grant participates.
pub fn authorize_administration(
    account: &HumanAuthorizationSnapshot,
    catalog: &AuthorizationCatalog,
    request: AdministrationRequest<'_>,
) -> Result<AuthorizedAdministration, AuthorizationDenied> {
    if !account.active() {
        return Err(AuthorizationDenied);
    }

    let grants = EffectiveHumanGrants::from_snapshot(account);
    let client_module = catalog
        .client_module(request.client_module)
        .ok_or(AuthorizationDenied)?;
    if !client_module.enabled() || !client_module.declares(Plane::Administration) {
        return Err(AuthorizationDenied);
    }
    if !grants
        .operational()
        .grants_client_module(request.client_module)
    {
        return Err(AuthorizationDenied);
    }

    match grants.administration() {
        ServerAdministrationPermission::Absent => Err(AuthorizationDenied),
        ServerAdministrationPermission::Granted => Ok(AuthorizedAdministration::granted(
            client_module.name().clone(),
        )),
    }
}

fn evaluate_user_operation(
    grants: &OperationalGrants,
    catalog: &AuthorizationCatalog,
    request: UserOperationRequest<'_>,
) -> Result<AuthorizedOperation, AuthorizationDenied> {
    let client_module = catalog
        .client_module(request.client_module)
        .ok_or(AuthorizationDenied)?;
    if !client_module.enabled() || !client_module.declares(Plane::User) {
        return Err(AuthorizationDenied);
    }

    let service_module = catalog
        .service_module(request.service_module)
        .ok_or(AuthorizationDenied)?;
    if !service_module.enabled() {
        return Err(AuthorizationDenied);
    }

    let operation = catalog
        .operation(request.operation)
        .ok_or(AuthorizationDenied)?;
    if !operation.enabled() || operation.service_module() != request.service_module {
        return Err(AuthorizationDenied);
    }

    if !grants.grants_client_module(request.client_module)
        || !grants.grants_service_module(request.service_module)
        || !grants.grants_operation(request.operation)
    {
        return Err(AuthorizationDenied);
    }

    Ok(AuthorizedOperation::granted(
        client_module.name().clone(),
        service_module.name().clone(),
        operation.name().clone(),
    ))
}

#[cfg(test)]
mod tests {
    use weavelit_server_database::GroupGrant;

    use super::*;
    use crate::catalog::{ClientModuleDeclaration, OperationDeclaration, ServiceModuleDeclaration};

    const CLIENT_MODULE: &str = "web-ui";
    const SERVICE_MODULE: &str = "zendesk";
    const OPERATION: &str = "zendesk.ticket.create";

    fn name(value: &str) -> Name {
        Name::new(value).expect("a bounded printable name is valid")
    }

    /// The catalog every row of the precedence table starts from.
    struct CatalogState {
        client_enabled: bool,
        client_planes: Vec<Plane>,
        service_enabled: bool,
        operation_enabled: bool,
        operation_owner: &'static str,
    }

    impl Default for CatalogState {
        fn default() -> Self {
            Self {
                client_enabled: true,
                client_planes: vec![Plane::User, Plane::Administration],
                service_enabled: true,
                operation_enabled: true,
                operation_owner: SERVICE_MODULE,
            }
        }
    }

    impl CatalogState {
        fn build(self) -> AuthorizationCatalog {
            AuthorizationCatalog::new(
                vec![ClientModuleDeclaration::new(
                    name(CLIENT_MODULE),
                    self.client_enabled,
                    &self.client_planes,
                )],
                vec![ServiceModuleDeclaration::new(
                    name(SERVICE_MODULE),
                    self.service_enabled,
                )],
                vec![OperationDeclaration::new(
                    name(OPERATION),
                    name(self.operation_owner),
                    self.operation_enabled,
                )],
            )
            .expect("the fixture catalog has distinct names")
        }
    }

    /// The grants every row of the precedence table starts from.
    struct GrantState {
        client_module: bool,
        service_module: bool,
        operation: bool,
        administration: bool,
    }

    impl Default for GrantState {
        fn default() -> Self {
            Self {
                client_module: true,
                service_module: true,
                operation: true,
                administration: false,
            }
        }
    }

    impl GrantState {
        fn snapshot(self, active: bool) -> HumanAuthorizationSnapshot {
            let mut grants = Vec::new();
            if self.client_module {
                grants.push(GroupGrant::ClientModule(name(CLIENT_MODULE)));
            }
            if self.service_module {
                grants.push(GroupGrant::ServiceModule(name(SERVICE_MODULE)));
            }
            if self.operation {
                grants.push(GroupGrant::Operation(name(OPERATION)));
            }
            if self.administration {
                grants.push(GroupGrant::ServerAdministration);
            }

            HumanAuthorizationSnapshot::new(active, grants)
        }
    }

    fn operation_request<'a>(
        client_module: &'a Name,
        service_module: &'a Name,
        operation: &'a Name,
    ) -> UserOperationRequest<'a> {
        UserOperationRequest {
            client_module,
            service_module,
            operation,
        }
    }

    fn decide(
        account: &HumanAuthorizationSnapshot,
        catalog: &AuthorizationCatalog,
    ) -> Result<AuthorizedOperation, AuthorizationDenied> {
        authorize_user_operation(
            account,
            catalog,
            operation_request(
                &name(CLIENT_MODULE),
                &name(SERVICE_MODULE),
                &name(OPERATION),
            ),
        )
    }

    #[test]
    fn the_only_fully_satisfied_row_of_the_precedence_table_allows() {
        let account = GrantState::default().snapshot(true);
        let catalog = CatalogState::default().build();

        let authorized = decide(&account, &catalog).expect("every requirement is satisfied");

        assert_eq!(authorized.client_module().as_str(), CLIENT_MODULE);
        assert_eq!(authorized.service_module().as_str(), SERVICE_MODULE);
        assert_eq!(authorized.operation().as_str(), OPERATION);
    }

    #[test]
    fn an_inactive_account_is_denied_before_anything_else_is_consulted() {
        let account = GrantState::default().snapshot(false);
        let catalog = CatalogState::default().build();

        assert_eq!(decide(&account, &catalog), Err(AuthorizationDenied));
    }

    #[test]
    fn a_disabled_client_module_is_denied() {
        let account = GrantState::default().snapshot(true);
        let catalog = CatalogState {
            client_enabled: false,
            ..CatalogState::default()
        }
        .build();

        assert_eq!(decide(&account, &catalog), Err(AuthorizationDenied));
    }

    #[test]
    fn a_client_module_that_does_not_declare_the_user_plane_is_denied() {
        let account = GrantState::default().snapshot(true);
        let catalog = CatalogState {
            client_planes: vec![Plane::Administration],
            ..CatalogState::default()
        }
        .build();

        assert_eq!(decide(&account, &catalog), Err(AuthorizationDenied));
    }

    #[test]
    fn a_disabled_service_module_is_denied() {
        let account = GrantState::default().snapshot(true);
        let catalog = CatalogState {
            service_enabled: false,
            ..CatalogState::default()
        }
        .build();

        assert_eq!(decide(&account, &catalog), Err(AuthorizationDenied));
    }

    #[test]
    fn a_disabled_operation_is_denied() {
        let account = GrantState::default().snapshot(true);
        let catalog = CatalogState {
            operation_enabled: false,
            ..CatalogState::default()
        }
        .build();

        assert_eq!(decide(&account, &catalog), Err(AuthorizationDenied));
    }

    #[test]
    fn an_operation_owned_by_another_service_module_is_denied() {
        let account = GrantState::default().snapshot(true);
        let catalog = CatalogState {
            operation_owner: "other-service",
            ..CatalogState::default()
        }
        .build();

        assert_eq!(decide(&account, &catalog), Err(AuthorizationDenied));
    }

    #[test]
    fn a_missing_client_module_grant_is_denied() {
        let account = GrantState {
            client_module: false,
            ..GrantState::default()
        }
        .snapshot(true);
        let catalog = CatalogState::default().build();

        assert_eq!(decide(&account, &catalog), Err(AuthorizationDenied));
    }

    #[test]
    fn a_missing_service_module_grant_is_denied() {
        let account = GrantState {
            service_module: false,
            ..GrantState::default()
        }
        .snapshot(true);
        let catalog = CatalogState::default().build();

        assert_eq!(decide(&account, &catalog), Err(AuthorizationDenied));
    }

    #[test]
    fn a_missing_operation_grant_is_denied() {
        let account = GrantState {
            operation: false,
            ..GrantState::default()
        }
        .snapshot(true);
        let catalog = CatalogState::default().build();

        assert_eq!(decide(&account, &catalog), Err(AuthorizationDenied));
    }

    #[test]
    fn an_uncatalogued_operation_is_denied_by_default() {
        let unknown = name("zendesk.ticket.delete");
        let account = HumanAuthorizationSnapshot::new(
            true,
            vec![
                GroupGrant::ClientModule(name(CLIENT_MODULE)),
                GroupGrant::ServiceModule(name(SERVICE_MODULE)),
                // The Group grants the name; the catalog does not declare it.
                GroupGrant::Operation(unknown.clone()),
            ],
        );
        let catalog = CatalogState::default().build();

        let denial = authorize_user_operation(
            &account,
            &catalog,
            operation_request(&name(CLIENT_MODULE), &name(SERVICE_MODULE), &unknown),
        );

        assert_eq!(denial, Err(AuthorizationDenied));
    }

    #[test]
    fn a_newly_registered_operation_is_denied_until_a_group_grants_it() {
        let registered = name("zendesk.ticket.comment");
        let catalog = AuthorizationCatalog::new(
            vec![ClientModuleDeclaration::new(
                name(CLIENT_MODULE),
                true,
                &[Plane::User],
            )],
            vec![ServiceModuleDeclaration::new(name(SERVICE_MODULE), true)],
            vec![
                OperationDeclaration::new(name(OPERATION), name(SERVICE_MODULE), true),
                OperationDeclaration::new(registered.clone(), name(SERVICE_MODULE), true),
            ],
        )
        .expect("the fixture catalog has distinct names");
        // The account already reaches the Client Module, the Service Module,
        // and one Operation of that Service Module.
        let account = GrantState::default().snapshot(true);

        let denial = authorize_user_operation(
            &account,
            &catalog,
            operation_request(&name(CLIENT_MODULE), &name(SERVICE_MODULE), &registered),
        );

        assert_eq!(denial, Err(AuthorizationDenied));
        assert!(decide(&account, &catalog).is_ok());
    }

    #[test]
    fn an_administrator_without_operational_grants_is_denied_a_user_operation() {
        // An Administrator who reaches the Web UI and holds the Server
        // Administration Permission holds no Service Module or Operation
        // grant, exactly like the Administrators Group created during Init.
        let account = HumanAuthorizationSnapshot::new(
            true,
            vec![
                GroupGrant::ClientModule(name(CLIENT_MODULE)),
                GroupGrant::ServerAdministration,
            ],
        );
        let catalog = CatalogState::default().build();

        assert_eq!(decide(&account, &catalog), Err(AuthorizationDenied));
        // The same account is nonetheless allowed on the Administration Plane.
        assert!(
            authorize_administration(
                &account,
                &catalog,
                AdministrationRequest {
                    client_module: &name(CLIENT_MODULE),
                },
            )
            .is_ok()
        );
    }

    #[test]
    fn administration_requires_an_active_account_a_declared_plane_and_both_grants() {
        let catalog = CatalogState::default().build();
        let request = AdministrationRequest {
            client_module: &name(CLIENT_MODULE),
        };
        let administrator = HumanAuthorizationSnapshot::new(
            true,
            vec![
                GroupGrant::ClientModule(name(CLIENT_MODULE)),
                GroupGrant::ServerAdministration,
            ],
        );
        let inactive = HumanAuthorizationSnapshot::new(
            false,
            vec![
                GroupGrant::ClientModule(name(CLIENT_MODULE)),
                GroupGrant::ServerAdministration,
            ],
        );
        let without_permission = HumanAuthorizationSnapshot::new(
            true,
            vec![GroupGrant::ClientModule(name(CLIENT_MODULE))],
        );
        let without_client_module =
            HumanAuthorizationSnapshot::new(true, vec![GroupGrant::ServerAdministration]);
        let user_plane_only = CatalogState {
            client_planes: vec![Plane::User],
            ..CatalogState::default()
        }
        .build();
        let disabled = CatalogState {
            client_enabled: false,
            ..CatalogState::default()
        }
        .build();

        assert_eq!(
            authorize_administration(&administrator, &catalog, request)
                .expect("every administration requirement is satisfied")
                .client_module()
                .as_str(),
            CLIENT_MODULE
        );
        assert_eq!(
            authorize_administration(&inactive, &catalog, request),
            Err(AuthorizationDenied)
        );
        assert_eq!(
            authorize_administration(&administrator, &disabled, request),
            Err(AuthorizationDenied)
        );
        assert_eq!(
            authorize_administration(&administrator, &user_plane_only, request),
            Err(AuthorizationDenied)
        );
        assert_eq!(
            authorize_administration(&without_client_module, &catalog, request),
            Err(AuthorizationDenied)
        );
        assert_eq!(
            authorize_administration(&without_permission, &catalog, request),
            Err(AuthorizationDenied)
        );
    }

    #[test]
    fn an_uncatalogued_client_module_or_service_module_is_denied_by_default() {
        let account = HumanAuthorizationSnapshot::new(
            true,
            vec![
                GroupGrant::ClientModule(name("mcp")),
                GroupGrant::ServiceModule(name("other-service")),
                GroupGrant::Operation(name(OPERATION)),
            ],
        );
        let catalog = CatalogState::default().build();

        assert_eq!(
            authorize_user_operation(
                &account,
                &catalog,
                operation_request(&name("mcp"), &name(SERVICE_MODULE), &name(OPERATION)),
            ),
            Err(AuthorizationDenied)
        );
        assert_eq!(
            authorize_user_operation(
                &account,
                &catalog,
                operation_request(
                    &name(CLIENT_MODULE),
                    &name("other-service"),
                    &name(OPERATION)
                ),
            ),
            Err(AuthorizationDenied)
        );
        assert_eq!(
            authorize_administration(
                &account,
                &catalog,
                AdministrationRequest {
                    client_module: &name("mcp"),
                },
            ),
            Err(AuthorizationDenied)
        );
    }

    #[test]
    fn a_denial_reports_no_reason_in_any_rendering() {
        let denial = AuthorizationDenied;

        assert_eq!(denial.to_string(), "request authorization denied");
        for identifying in [
            CLIENT_MODULE,
            SERVICE_MODULE,
            OPERATION,
            "inactive",
            "disabled",
            "grant",
        ] {
            assert!(!denial.to_string().contains(identifying));
            assert!(!format!("{denial:?}").contains(identifying));
        }
    }
}
