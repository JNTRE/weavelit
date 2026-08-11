//! A selection cannot be assembled without passing through authorization.

#[path = "authorized.rs"]
mod authorized;

use weavelit_server_database::{STATE_IDENTIFIER_LENGTH, StateIdentifier};
use weavelit_server_operation::SelectedServiceConnection;

fn main() {
    // Every field is private, so naming even one of them in a struct literal
    // is refused. A caller holding a connection identifier therefore cannot
    // manufacture a selection and skip the authorization decision that would
    // otherwise have had to produce the proof this type carries.
    let _forged = SelectedServiceConnection {
        connection: StateIdentifier::from_bytes([0x22; STATE_IDENTIFIER_LENGTH])
            .expect("valid identifier"),
        connection_name: authorized::name("primary"),
    };
}
