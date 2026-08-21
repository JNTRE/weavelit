use std::time::Duration;

use weavelit_server_administration::{MfaStepUpProof, StepUpActionFamily};
use weavelit_server_database::{
    SessionTokenHash, StateIdentifier, SESSION_DIGEST_LENGTH, STATE_IDENTIFIER_LENGTH,
};

fn main() {
    let actor = StateIdentifier::from_bytes([1; STATE_IDENTIFIER_LENGTH]).unwrap();
    let session = SessionTokenHash::from_bytes([2; SESSION_DIGEST_LENGTH]).unwrap();
    let _proof = MfaStepUpProof {
        actor,
        session,
        factor: actor,
        family: StepUpActionFamily::MfaPolicy,
        issued_at: Duration::ZERO,
        expires_at: Duration::from_secs(300),
    };
}
