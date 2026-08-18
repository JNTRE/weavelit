use weavelit_server_administration::{
    AdministrationAction, AuthorizedAdministrationAction, AuthorizedAdministrationAdmission,
};

fn forge(admission: AuthorizedAdministrationAdmission, action: AdministrationAction) {
    let _authorized = AuthorizedAdministrationAction { admission, action };
}

fn main() {}