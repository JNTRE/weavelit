//! Selection must take the proof by value, not by reference.

#[path = "authorized.rs"]
mod authorized;

use weavelit_server_operation::SelectedServiceConnection;

fn main() {
    let proof = authorized::proof();
    let connections = authorized::connections();
    let _selected =
        SelectedServiceConnection::select(&proof, &connections, &authorized::name("primary"));
}
