//! One selection cannot execute a provider twice.

#[path = "authorized.rs"]
mod authorized;

use weavelit_server_operation::SelectedServiceConnection;

fn main() {
    let proof = authorized::proof();
    let connections = authorized::connections();
    let selected =
        SelectedServiceConnection::select(proof, &connections, &authorized::name("primary"))
            .expect("selected");
    let _first = selected.execute(|_| ());
    let _second = selected.execute(|_| ());
}
