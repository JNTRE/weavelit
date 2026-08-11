use weavelit_server_database::DeploymentIdentifier;
use weavelit_server_init::AuthorizedInit;

fn main() {
    let deployment = DeploymentIdentifier::from_bytes([7; 16]).expect("valid deployment");
    let _forged = AuthorizedInit::new(deployment);
}
