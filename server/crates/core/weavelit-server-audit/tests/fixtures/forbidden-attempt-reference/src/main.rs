use weavelit_server_audit::AuditAttemptReference;

fn main() {
    let _forged = AuditAttemptReference::new(panic!(), panic!(), panic!());
}