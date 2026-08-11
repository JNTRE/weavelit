use weavelit_server_database::{CheckpointMetadata, DeploymentIdentifier, WorkflowCheckpoint};
use weavelit_server_lifecycle::ReleasedInitCheckpoint;

fn main() {
    let deployment = DeploymentIdentifier::from_bytes([7; 16]).expect("valid deployment");
    let _forged = ReleasedInitCheckpoint {
        checkpoint: WorkflowCheckpoint::new(
            deployment,
            weavelit_server_database::WorkflowKind::Init,
            CheckpointMetadata::from_bytes(&b"forged-metadata"[..]).expect("valid metadata"),
        ),
    };
}
