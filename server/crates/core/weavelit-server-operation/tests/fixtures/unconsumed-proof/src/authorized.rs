//! A shared authorized proof for the compile-fail fixtures.
//!
//! Every fixture needs a real proof, and the evaluator is the only source of
//! one. Nothing here is ever run: the fixtures are compiled, not executed.

use weavelit_server_authorization::{
    AuthorizationCatalog, AuthorizedOperation, ClientModuleDeclaration, OperationDeclaration,
    Plane, ServiceModuleDeclaration, UserOperationRequest, authorize_user_operation,
};
use weavelit_server_database::{
    GroupGrant, HumanAuthorizationSnapshot, Name, ProtectedValue, STATE_IDENTIFIER_LENGTH,
    ServiceConnection, StateIdentifier,
};

pub fn name(value: &str) -> Name {
    Name::new(value).expect("valid name")
}

pub fn proof() -> AuthorizedOperation {
    let account = HumanAuthorizationSnapshot::new(
        true,
        vec![
            GroupGrant::ClientModule(name("web-ui")),
            GroupGrant::ServiceModule(name("zendesk")),
            GroupGrant::Operation(name("zendesk.ticket.create")),
        ],
    );
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
    .expect("valid catalog");

    authorize_user_operation(
        &account,
        &catalog,
        UserOperationRequest {
            client_module: &name("web-ui"),
            service_module: &name("zendesk"),
            operation: &name("zendesk.ticket.create"),
        },
    )
    .expect("authorized")
}

pub fn connections() -> Vec<ServiceConnection> {
    vec![ServiceConnection {
        identifier: StateIdentifier::from_bytes([0x22; STATE_IDENTIFIER_LENGTH])
            .expect("valid identifier"),
        service_module: name("zendesk"),
        name: name("primary"),
        protected_credential: ProtectedValue::new(b"credential".to_vec().into_boxed_slice())
            .expect("valid credential"),
    }]
}
