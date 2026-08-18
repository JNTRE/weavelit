use weavelit_server_administration::AuthorizedAdministrationAdmission;
use weavelit_server_authorization::AuthorizedAdministration;
use weavelit_server_database::{SessionTokenHash, StateIdentifier};

fn forge(
    authorization: AuthorizedAdministration,
    actor: StateIdentifier,
    session: SessionTokenHash,
) {
    let _admission = AuthorizedAdministrationAdmission {
        authorization,
        actor,
        session,
    };
}

fn main() {}