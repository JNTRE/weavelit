use weavelit_server_database::AuditReferencePersistence;

fn main() {
    let persistence = AuditReferencePersistence { _private: () };
    let _decoded = persistence.decode("ar-0123456789abcdef0123456789abcdef");
}
