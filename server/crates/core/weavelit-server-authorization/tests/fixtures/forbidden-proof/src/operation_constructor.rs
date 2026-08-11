use weavelit_server_authorization::AuthorizedOperation;
use weavelit_server_database::Name;

fn main() {
    let name = Name::new("web-ui").expect("valid name");
    let _proof = AuthorizedOperation::granted(name.clone(), name.clone(), name);
}
