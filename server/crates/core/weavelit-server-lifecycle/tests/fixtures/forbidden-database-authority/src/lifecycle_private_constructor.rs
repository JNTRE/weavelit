use weavelit_server_lifecycle::SelectedDatabase;

fn inaccessible<T>() -> T {
    panic!("private constructor arguments must be unreachable")
}

fn main() {
    let _selected = SelectedDatabase::from_server_authority(inaccessible(), inaccessible());
}
