use weavelit_server::{
    StartupError, classify_restricted_startup, read_state_root, read_trusted_https_listener,
    run_restricted_https_listener,
};

fn main() {
    let listener = match read_trusted_https_listener() {
        Ok(listener) => listener,
        Err(error) => {
            present_error(error);
            std::process::exit(1);
        }
    };

    let state_root = match read_state_root() {
        Ok(path) => path,
        Err(error) => {
            present_error(error);
            std::process::exit(1);
        }
    };

    let startup = match classify_restricted_startup(&state_root) {
        Ok(startup) => startup,
        Err(error) => {
            present_error(error);
            std::process::exit(1);
        }
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            present_error(StartupError::HttpsListenerUnavailable);
            std::process::exit(1);
        }
    };
    if let Err(error) = runtime.block_on(run_restricted_https_listener(listener, startup)) {
        present_error(error);
        std::process::exit(1);
    }
}

fn present_error(error: StartupError) {
    let (category, reason) = error.category_reason();
    eprintln!("{{\"category\":\"{category}\",\"reason\":\"{reason}\"}}");
}
