use weavelit_server_authorization::AuthorizedAdministration;
use weavelit_server_database::Name;

fn main() {
    let client_module = Name::new("web-ui").expect("valid name");
    let _proof = AuthorizedAdministration { client_module };
}
