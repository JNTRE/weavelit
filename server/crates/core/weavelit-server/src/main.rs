use weavelit_server::{StartupError, classify_restricted_startup, read_state_root};

fn main() {
    let state_root = match read_state_root() {
        Ok(path) => path,
        Err(error) => {
            present_error(error);
            std::process::exit(1);
        }
    };

    match classify_restricted_startup(&state_root) {
        Ok(_outcome) => {}
        Err(error) => {
            present_error(error);
            std::process::exit(1);
        }
    }
}

fn present_error(error: StartupError) {
    let (category, reason) = error.category_reason();
    eprintln!("{{\"category\":\"{category}\",\"reason\":\"{reason}\"}}");
}
