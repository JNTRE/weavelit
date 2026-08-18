use weavelit_server_lifecycle::SelectedDatabase;

fn main() {
    let _selected = SelectedDatabase {
        database: panic!("the private field must be unreachable"),
        persistence: panic!("the private field must be unreachable"),
    };
}
