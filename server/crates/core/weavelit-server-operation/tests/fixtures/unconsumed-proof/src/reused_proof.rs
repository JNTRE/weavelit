//! One authorization cannot select two Service Connections.

#[path = "authorized.rs"]
mod authorized;

use weavelit_server_operation::SelectedServiceConnection;

fn main() {
    let proof = authorized::proof();
    let connections = authorized::connections();
    let _first =
        SelectedServiceConnection::select(proof, &connections, &authorized::name("primary"));
    let _second =
        SelectedServiceConnection::select(proof, &connections, &authorized::name("primary"));
}
