use weavelit_server_database::{
    AccountAdministrationProjection, AccountPublicIdentifier, Name,
};

fn construct(public_identifier: AccountPublicIdentifier) {
    let _projection = AccountAdministrationProjection {
        public_identifier,
        username: Name::new("administrator").unwrap(),
        display_name: None,
        active: true,
        mfa_required: false,
    };
}

fn main() {}