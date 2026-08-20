use weavelit_server_lifecycle::SelectedDatabase;

fn main() {
    let _selected = SelectedDatabase {
        database: panic!("the private field must be unreachable"),
        persistence: panic!("the private field must be unreachable"),
        audit_terminal_recovery_persistence: panic!("the private field must be unreachable"),
        log_configuration_generation_persistence: panic!("the private field must be unreachable"),
    };
}
