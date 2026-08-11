use weavelit_server_lifecycle::ReleasedInitCheckpoint;

fn main() {}

/// A released Init checkpoint must not be able to seal the deployment without
/// first being reauthorized under a fresh exclusive permit.
fn seal_without_reauthorization(released: ReleasedInitCheckpoint) {
    let _sealed = released.seal();
}
