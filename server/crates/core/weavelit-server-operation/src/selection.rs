//! Step six and step seven of the operational request path.
//!
//! Authorization ends with an [`AuthorizedOperation`] proof. This crate owns
//! what happens next: selecting the Service Connection the authorized
//! Operation will run against, and then executing the provider.
//!
//! Both steps consume the value they receive. Selection takes the proof by
//! value, so the proof is gone once a connection has been selected and cannot
//! be reused to select a second connection or to justify a second Operation.
//! Execution takes the selection by value, so a provider runs at most once per
//! authorization. Because the proof cannot be constructed outside
//! `weavelit-server-authorization`, reaching either step without an
//! authorization decision does not compile, and reaching execution without a
//! selection does not compile either.
//!
//! The ordering is therefore carried by the types rather than by a comment: a
//! request path that skipped authorization would have no proof to move, and a
//! request path that skipped selection would have nothing to execute.

use weavelit_server_authorization::AuthorizedOperation;
use weavelit_server_database::{Name, ServiceConnection, StateIdentifier};

/// A Service Connection an authorized Operation may run against.
///
/// The value can only be produced by moving an [`AuthorizedOperation`] into
/// [`SelectedServiceConnection::select`]. Its fields are private and it has no
/// public constructor, so it cannot be forged and cannot be assembled from a
/// connection identifier alone.
///
/// It carries no provider credential. The selection names the connection; the
/// credential stays in the Application Database until the provider asks for it,
/// so an authorization result never holds a secret.
#[derive(Debug, Eq, PartialEq)]
pub struct SelectedServiceConnection {
    /// The proof this selection was made under, kept so the executing step can
    /// see exactly which Operation was authorized.
    operation: AuthorizedOperation,
    /// The selected connection.
    connection: StateIdentifier,
    /// The selected connection's name within its Service Module.
    connection_name: Name,
}

impl SelectedServiceConnection {
    /// Selects the named Service Connection the authorized Operation runs on.
    ///
    /// The proof is taken by value, so selection is the point at which an
    /// authorization is spent. Selection is not a second authorization
    /// decision: it only refuses a connection the authorized Operation's own
    /// Service Module does not own, because running an Operation against
    /// another Service Module's connection would use a credential the
    /// authorization never covered.
    ///
    /// Returns `None` when no connection of that name belongs to the proof's
    /// Service Module. The proof is consumed either way, so a failed selection
    /// cannot be retried against a different Service Module.
    #[must_use]
    pub fn select(
        operation: AuthorizedOperation,
        connections: &[ServiceConnection],
        connection: &Name,
    ) -> Option<Self> {
        let selected = connections.iter().find(|candidate| {
            &candidate.service_module == operation.service_module() && &candidate.name == connection
        })?;

        Some(Self {
            operation,
            connection: selected.identifier,
            connection_name: selected.name.clone(),
        })
    }

    /// Returns the authorization this selection was made under.
    #[must_use]
    pub fn operation(&self) -> &AuthorizedOperation {
        &self.operation
    }

    /// Returns the selected connection's opaque identifier.
    #[must_use]
    pub fn connection(&self) -> &StateIdentifier {
        &self.connection
    }

    /// Returns the selected connection's name within its Service Module.
    #[must_use]
    pub fn connection_name(&self) -> &Name {
        &self.connection_name
    }

    /// Runs the provider for the selected connection.
    ///
    /// This is the only entry into provider execution, and it consumes the
    /// selection, which owns the proof. A caller therefore cannot execute
    /// twice under one authorization, and cannot execute at all without having
    /// passed through both the authorization decision and the selection.
    ///
    /// No provider execution path exists yet, so this crate supplies the shape
    /// rather than a provider: the Service Module boundary that will implement
    /// execution receives the selection and nothing else.
    pub fn execute<R>(self, provider: impl FnOnce(&Self) -> R) -> R {
        provider(&self)
    }
}

#[cfg(test)]
mod tests {
    use weavelit_server_authorization::{
        AuthorizationCatalog, ClientModuleDeclaration, OperationDeclaration, Plane,
        ServiceModuleDeclaration, UserOperationRequest, authorize_user_operation,
    };
    use weavelit_server_database::{
        GroupGrant, HumanAuthorizationSnapshot, ProtectedValue, STATE_IDENTIFIER_LENGTH,
    };

    use super::*;

    fn name(value: &str) -> Name {
        Name::new(value).expect("the test name must be accepted")
    }

    fn identifier(byte: u8) -> StateIdentifier {
        StateIdentifier::from_bytes([byte; STATE_IDENTIFIER_LENGTH])
            .expect("the test identifier must be accepted")
    }

    fn connection(byte: u8, service_module: &str, connection_name: &str) -> ServiceConnection {
        ServiceConnection {
            identifier: identifier(byte),
            service_module: name(service_module),
            name: name(connection_name),
            protected_credential: ProtectedValue::new(
                b"protected-provider-credential".to_vec().into_boxed_slice(),
            )
            .expect("the test credential must be accepted"),
        }
    }

    /// Produces a real proof through the evaluator, because no other source of
    /// one exists.
    fn proof() -> AuthorizedOperation {
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
            vec![
                ServiceModuleDeclaration::new(name("zendesk"), true),
                ServiceModuleDeclaration::new(name("other"), true),
            ],
            vec![OperationDeclaration::new(
                name("zendesk.ticket.create"),
                name("zendesk"),
                true,
            )],
        )
        .expect("the test catalog must be accepted");

        authorize_user_operation(
            &account,
            &catalog,
            UserOperationRequest {
                client_module: &name("web-ui"),
                service_module: &name("zendesk"),
                operation: &name("zendesk.ticket.create"),
            },
        )
        .expect("the fully granted request must be authorized")
    }

    #[test]
    fn selection_names_the_connection_the_authorized_service_module_owns() {
        let connections = [
            connection(0x11, "other", "primary"),
            connection(0x22, "zendesk", "primary"),
            connection(0x33, "zendesk", "secondary"),
        ];

        let selected = SelectedServiceConnection::select(proof(), &connections, &name("primary"))
            .expect("the owned connection must be selectable");

        assert_eq!(selected.connection(), &identifier(0x22));
        assert_eq!(selected.connection_name(), &name("primary"));
        assert_eq!(
            selected.operation().operation(),
            &name("zendesk.ticket.create")
        );
    }

    #[test]
    fn selection_refuses_a_connection_the_authorized_service_module_does_not_own() {
        let connections = [connection(0x11, "other", "primary")];

        assert_eq!(
            SelectedServiceConnection::select(proof(), &connections, &name("primary")),
            None
        );
        assert_eq!(
            SelectedServiceConnection::select(proof(), &connections, &name("absent")),
            None
        );
        assert_eq!(
            SelectedServiceConnection::select(proof(), &[], &name("primary")),
            None
        );
    }

    #[test]
    fn execution_receives_the_selection_that_owns_the_proof() {
        let connections = [connection(0x22, "zendesk", "primary")];
        let selected = SelectedServiceConnection::select(proof(), &connections, &name("primary"))
            .expect("the owned connection must be selectable");

        let executed = selected.execute(|selection| {
            (
                *selection.connection(),
                selection.operation().service_module().clone(),
            )
        });

        assert_eq!(executed, (identifier(0x22), name("zendesk")));
    }

    #[test]
    fn a_selection_carries_no_provider_credential() {
        let connections = [connection(0x22, "zendesk", "primary")];
        let selected = SelectedServiceConnection::select(proof(), &connections, &name("primary"))
            .expect("the owned connection must be selectable");

        let rendered = format!("{selected:?}");

        assert!(!rendered.contains("protected-provider-credential"));
    }
}
