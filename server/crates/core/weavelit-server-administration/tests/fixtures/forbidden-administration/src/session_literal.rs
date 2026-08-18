use weavelit_server_administration::CurrentAdministrationSession;
use weavelit_server_database::{
    SESSION_DIGEST_LENGTH, STATE_IDENTIFIER_LENGTH, SessionTokenHash, StateIdentifier,
};

fn main() {
    let actor = StateIdentifier::from_bytes([1; STATE_IDENTIFIER_LENGTH]).unwrap();
    let session = SessionTokenHash::from_bytes([2; SESSION_DIGEST_LENGTH]).unwrap();
    let _current = CurrentAdministrationSession { actor, session };
}