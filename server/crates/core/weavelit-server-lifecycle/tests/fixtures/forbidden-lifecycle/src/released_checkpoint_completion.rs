use weavelit_server_database::ApplicationState;
use weavelit_server_lifecycle::ReleasedInitCheckpoint;

fn main() {}

/// A released Init checkpoint must not be able to replace application state
/// without first being reauthorized under a fresh exclusive permit.
fn complete_without_reauthorization(released: ReleasedInitCheckpoint, state: &ApplicationState) {
    let _replaced = released.complete_checkpoint(state);
}
