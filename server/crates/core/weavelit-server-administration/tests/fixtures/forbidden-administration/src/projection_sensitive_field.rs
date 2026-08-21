use weavelit_server_database::AccountAdministrationProjection;

fn expose(projection: AccountAdministrationProjection) {
    let _verifier = projection.password_verifier;
}

fn main() {}