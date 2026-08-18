use weavelit_server_lifecycle_forbidden_database_authority_fixture::ExternalDatabase;

fn main() {
    let _persistence = ExternalDatabase.audit_reference_persistence();
}
