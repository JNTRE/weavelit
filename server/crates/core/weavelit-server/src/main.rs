use std::{future::Future, process::ExitCode, time::Duration};

use tokio::signal::unix::{SignalKind, signal};
use weavelit_server::{
    ShutdownSignal, StartupError, classify_restricted_startup, read_state_root,
    read_trusted_https_listener, run_restricted_https_listener,
};

/// Runs the Server and reports its outcome as a process exit status.
///
/// The status is returned rather than set with an immediate process exit, so an
/// orderly shutdown still unwinds normally and every retained value, including
/// the process-lifetime state-root lock, is released by its own destructor.
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            present_error(error);
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), StartupError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|_| StartupError::HttpsListenerUnavailable)?;

    // Registered before the listening socket exists, and so before any host can
    // observe this process as started. Registering it after the bind would open
    // a window in which a supervisor's `SIGTERM` could already reach a
    // connectable Server that still carried the default disposition, killing it
    // rather than draining it.
    let signalled = {
        let _runtime_context = runtime.enter();
        termination_signal().map_err(|_| StartupError::HttpsListenerUnavailable)?
    };

    let listener = read_trusted_https_listener()?;
    let state_root = read_state_root()?;
    let startup = classify_restricted_startup(&state_root)?;

    let result = runtime.block_on(run_restricted_https_listener(
        listener,
        startup,
        ShutdownSignal::new(signalled),
    ));
    // The listener does not return while a lifecycle transition admitted before
    // the stop still holds its gate: it waits through activation or release
    // before attempting the database close. Nothing lifecycle-owned is left
    // behind here; only blocking work that already exceeded its own close
    // budget can remain after the listener reported an incomplete shutdown.
    runtime.shutdown_timeout(Duration::ZERO);

    result
}

/// Completes when the host asks this process to stop.
///
/// Deciding which signals stop the Server is process policy, so it is settled
/// here rather than inside the listener. `SIGTERM` is what a service supervisor
/// sends and `SIGINT` is what an interactive operator sends; both mean stop, so
/// both begin the same orderly shutdown.
fn termination_signal() -> std::io::Result<impl Future<Output = ()> + Send + 'static> {
    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;

    Ok(async move {
        tokio::select! {
            _ = terminate.recv() => {}
            _ = interrupt.recv() => {}
        }
    })
}

fn present_error(error: StartupError) {
    let (category, reason) = error.category_reason();
    eprintln!("{{\"category\":\"{category}\",\"reason\":\"{reason}\"}}");
}
