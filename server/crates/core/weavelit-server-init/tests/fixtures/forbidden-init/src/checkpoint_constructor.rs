use weavelit_server_database::RecoveryPublicKey;
use weavelit_server_init::InitCheckpoint;
use weavelit_server_recovery_key::{DeliveryNonce, RecoveryProof};

fn main() {
    let recipient = RecoveryPublicKey::new("age1qqqqqqqqqq").expect("valid recipient");
    let _forged = InitCheckpoint::new(
        recipient,
        DeliveryNonce::from_bytes([1; 32]),
        RecoveryProof::from_bytes([2; 32]),
    );
}
