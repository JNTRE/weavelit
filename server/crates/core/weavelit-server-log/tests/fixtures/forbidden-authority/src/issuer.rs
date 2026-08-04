use weavelit_server_log::TrustedRecordIssuer;

fn main() {
    let issuer = TrustedRecordIssuer::new();
    let _record_id = issuer.issue([1; 16]);
}