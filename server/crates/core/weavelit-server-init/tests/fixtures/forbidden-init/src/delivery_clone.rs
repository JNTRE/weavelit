use weavelit_server_init::PreparedInitDelivery;

fn main() {
    let prepared = PreparedInitDelivery::prepare().expect("preparation succeeds");
    let _duplicate = prepared.clone();
}
