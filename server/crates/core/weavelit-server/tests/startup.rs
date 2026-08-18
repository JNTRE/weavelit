use std::{
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    os::unix::{
        fs::{MetadataExt, PermissionsExt, symlink},
        process::ExitStatusExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use rusqlite::{Connection, OpenFlags, params};
use weavelit_server::{
    StartupError, StartupOutcome, classify_restricted_startup, sqlite_catalog,
    validate_trusted_https_listener,
};
use weavelit_server_database::{
    ApplicationStateInput, CompletionObligation, CorrelationIdentifier, LogAssignment,
    LogClassification, LogDetail, LogModuleConfiguration, LogType, Name, RecoveryPublicKey,
};
use weavelit_server_lifecycle::{
    ApplicationState, BackendIdentifier, CheckpointMetadata, DeploymentIdentifier, LifecycleStore,
    StateIdentifier, TrustedBackendContext, WorkflowArbiter, WorkflowKind,
};

type RootEntrySnapshot = (PathBuf, u32, u32, u32, u64, u64, i64, i64, i64, i64, u64);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn state_root() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let canonical = directory.path().canonicalize().unwrap();
    (directory, canonical)
}

fn root_snapshot(path: &Path) -> Vec<RootEntrySnapshot> {
    let mut entries = fs::read_dir(path)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            let metadata = entry.metadata().unwrap();
            let contents = fs::read(entry.path()).unwrap();
            let mut hasher = DefaultHasher::new();
            contents.hash(&mut hasher);
            (
                PathBuf::from(entry.file_name()),
                metadata.permissions().mode(),
                metadata.uid(),
                metadata.gid(),
                metadata.nlink(),
                metadata.size(),
                metadata.mtime(),
                metadata.mtime_nsec(),
                metadata.ctime(),
                metadata.ctime_nsec(),
                hasher.finish(),
            )
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    entries
}

fn assert_interrupted_startup(state_root: &Path, expected_reason: &str) {
    let before = root_snapshot(state_root);
    assert_startup_failure(state_root, "lifecycle_interrupted", expected_reason);
    assert_eq!(root_snapshot(state_root), before);
}

/// Runs the real Server binary and asserts it exits closed on the exact pair
/// without ever accepting a connection on its configured listener address.
fn assert_startup_failure(state_root: &Path, expected_category: &str, expected_reason: &str) {
    let (_tls_directory, certificate_path, private_key_path) = tls_material();
    let address = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_weavelit-server"))
        .env("WEAVELIT_HTTPS_LISTENER_ADDRESS", address.to_string())
        .env("WEAVELIT_TLS_CERTIFICATE_PATH", certificate_path)
        .env("WEAVELIT_TLS_PRIVATE_KEY_PATH", private_key_path)
        .env("WEAVELIT_STATE_ROOT", state_root)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        output.stderr,
        format!("{{\"category\":\"{expected_category}\",\"reason\":\"{expected_reason}\"}}\n")
            .as_bytes()
    );
    assert!(TcpStream::connect(address).is_err());
}

fn context(root: &std::path::Path) -> TrustedBackendContext {
    TrustedBackendContext::new(root.join("application.sqlite3"))
}

fn checkpoint_and_close_wal(path: &Path) {
    let connection = Connection::open(path.join("application.sqlite3")).unwrap();
    connection
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;")
        .unwrap();
    drop(connection);
    assert!(!path.join("application.sqlite3-wal").exists());
    assert!(!path.join("application.sqlite3-shm").exists());
}

fn sqlite() -> BackendIdentifier {
    BackendIdentifier::new("sqlite").unwrap()
}

fn completion_record_identifier() -> StateIdentifier {
    StateIdentifier::from_bytes([0x5A; 16]).unwrap()
}

/// Builds the smallest state the Application Database contract accepts.
fn sealed_application_state() -> ApplicationState {
    let configuration_identifier = StateIdentifier::from_bytes([0x11; 16]).unwrap();
    ApplicationState::new(ApplicationStateInput {
        configuration: vec![],
        protected_secrets: vec![],
        accounts: vec![],
        account_audit_references: vec![],
        password_verifiers: vec![],
        groups: vec![],
        group_audit_references: vec![],
        group_memberships: vec![],
        group_grants: vec![],
        mfa_factors: vec![],
        service_connections: vec![],
        recovery_public_key: RecoveryPublicKey::new("age1recoverypublickeyvalue").unwrap(),
        log_module_configurations: vec![LogModuleConfiguration {
            identifier: configuration_identifier,
            module: Name::new("log-sqlite").unwrap(),
            name: Name::new("local").unwrap(),
            enabled: true,
            settings: vec![],
        }],
        log_assignments: LogType::ALL
            .into_iter()
            .map(|log_type| LogAssignment {
                log_type,
                configuration: configuration_identifier,
            })
            .collect(),
        completion_obligation: CompletionObligation::new(
            completion_record_identifier(),
            WorkflowKind::Restore,
            LogClassification::new("lifecycle.restore").unwrap(),
            CorrelationIdentifier::new("correlation-identifier").unwrap(),
            1_700_000_000_000,
            LogDetail::new("restore completed").unwrap(),
        )
        .unwrap(),
    })
    .unwrap()
}

/// Seals a real deployment by driving the lifecycle typestate chain to `Initialized`.
///
/// Everything it opened is dropped before returning, so the state-root lock and
/// the Application Database are both released for the startup path under test.
fn seal_deployment(state_root: &Path) -> DeploymentIdentifier {
    let mut store = LifecycleStore::open_or_create(state_root).unwrap();
    let catalog = sqlite_catalog();
    let context = context(state_root);
    store
        .select_database(&catalog, &context, &sqlite(), vec![])
        .unwrap();

    let arbiter = WorkflowArbiter::new(store);
    let permit = arbiter.authorize_workflow(&catalog, &context).unwrap();
    let deployment_identifier = permit.deployment_identifier();
    permit
        .create_checkpoint(
            WorkflowKind::Restore,
            CheckpointMetadata::from_bytes(b"restore-checkpoint-metadata".as_slice()).unwrap(),
        )
        .unwrap()
        .complete_checkpoint(
            &sealed_application_state(),
            &weavelit_server_database::ReconciliationDigest::from_bytes([0xA0; 32]),
        )
        .unwrap()
        .acknowledge_completion(completion_record_identifier())
        .unwrap()
        .seal()
        .unwrap();
    drop(arbiter);
    deployment_identifier
}

/// Names the Application Database the write-ahead log writer child must open.
const WAL_WRITER_DATABASE_ENVIRONMENT: &str = "WEAVELIT_TEST_WAL_WRITER_DATABASE";
/// The exact readiness line the writer child emits once its write is durable.
const WAL_WRITER_READY_MARKER: &str = "weavelit-test-wal-writer-ready";
/// Value the writer child commits into the write-ahead log and never checkpoints.
const WAL_PROBE_VALUE: i64 = 424_242;

/// Counts probe rows visible through an `immutable=1` read, which ignores the WAL.
fn probe_rows_ignoring_wal(database_path: &Path) -> i64 {
    let uri = format!("file:{}?immutable=1", database_path.display());
    let connection = Connection::open_with_flags(
        Path::new(&uri),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .unwrap();
    connection
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name = 'wal_recovery_probe'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

/// Child-process half of the sealed write-ahead log recovery test.
///
/// It commits a probe write with checkpointing disabled, reports readiness on
/// stdout, and then blocks until the parent kills it, so the committed write
/// exists only in the write-ahead log of an abruptly terminated process. It is
/// ignored by default and does nothing at all unless its parent names a
/// database, so an ordinary or `--include-ignored` run is unaffected.
#[test]
#[ignore = "spawned as a child process by the sealed write-ahead log recovery test"]
fn write_ahead_log_writer_child() {
    let Ok(database_path) = std::env::var(WAL_WRITER_DATABASE_ENVIRONMENT) else {
        return;
    };

    let connection = Connection::open(&database_path).unwrap();
    let journal_mode: String = connection
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .unwrap();
    assert_eq!(journal_mode, "wal");
    connection
        .execute_batch(&format!(
            "PRAGMA wal_autocheckpoint = 0;\
             CREATE TABLE wal_recovery_probe (value INTEGER NOT NULL);\
             INSERT INTO wal_recovery_probe (value) VALUES ({WAL_PROBE_VALUE});"
        ))
        .unwrap();

    let mut stdout = std::io::stdout();
    writeln!(stdout, "{WAL_WRITER_READY_MARKER}").unwrap();
    stdout.flush().unwrap();

    // Blocks until the parent kills this process; the connection is never closed.
    let mut discarded = Vec::new();
    std::io::stdin().read_to_end(&mut discarded).unwrap();
}

/// Leaves a real, unrecovered write-ahead log beside the sealed database.
///
/// The child is synchronized entirely by a blocking read of its readiness line,
/// then killed, so nothing here depends on elapsed time.
fn abandon_write_ahead_log(state_root: &Path) {
    let database_path = state_root.join("application.sqlite3");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "write_ahead_log_writer_child",
            "--ignored",
            "--nocapture",
        ])
        .env(WAL_WRITER_DATABASE_ENVIRONMENT, &database_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    let ready = loop {
        line.clear();
        if reader.read_line(&mut line).unwrap() == 0 {
            break false;
        }
        if line.trim_end() == WAL_WRITER_READY_MARKER {
            break true;
        }
    };
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(
        ready,
        "the write-ahead log writer child must report readiness"
    );

    assert!(
        fs::metadata(state_root.join("application.sqlite3-wal"))
            .unwrap()
            .len()
            > 0,
        "the killed child must leave a non-empty write-ahead log"
    );
    assert_eq!(
        probe_rows_ignoring_wal(&database_path),
        0,
        "the committed write must live only in the write-ahead log"
    );
}

/// Blocks until the spawned Server is really accepting on `address`.
///
/// The wait ends on an accepted connection rather than on elapsed time; the
/// deadline only turns a Server that never binds into a failure instead of a
/// hang.
fn await_listener(address: SocketAddr) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while TcpStream::connect(address).is_err() {
        assert!(
            Instant::now() < deadline,
            "the Server did not bind its listener"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Waits until this Server holds the lock in its own state root.
///
/// Unlike the listener address, the state root belongs to one test, so nothing
/// else can satisfy this.
fn await_state_root_lock(state_root: &Path, server: &mut Child) {
    let lock = state_root.join("lifecycle.lock");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !lock.exists() {
        if let Some(status) = server.try_wait().unwrap() {
            panic!("the Server exited during startup with {status}");
        }
        assert!(
            Instant::now() < deadline,
            "the Server did not take its state root lock"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Asks a running Server to stop exactly as a service supervisor does.
fn terminate(server: &Child) {
    let pid = rustix::process::Pid::from_raw(i32::try_from(server.id()).unwrap())
        .expect("a spawned child has a valid process identifier");
    rustix::process::kill_process(pid, rustix::process::Signal::TERM).unwrap();
}

fn tls_material() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let certificate = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
    let certificate_path = directory.path().join("certificate.pem");
    let private_key_path = directory.path().join("private-key.pem");
    fs::write(&certificate_path, certificate.cert.pem()).unwrap();
    fs::write(&private_key_path, certificate.signing_key.serialize_pem()).unwrap();
    fs::set_permissions(&certificate_path, fs::Permissions::from_mode(0o644)).unwrap();
    fs::set_permissions(&private_key_path, fs::Permissions::from_mode(0o600)).unwrap();
    (directory, certificate_path, private_key_path)
}

fn assert_tls_validation_error(
    result: Result<weavelit_server::TrustedHttpsListener, StartupError>,
) {
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("invalid TLS material must be rejected"),
    };
    assert_eq!(error, StartupError::TlsMaterialInvalid);
}

// ---------------------------------------------------------------------------
// Tests: trusted HTTPS listener configuration
// ---------------------------------------------------------------------------

#[test]
fn valid_trusted_https_listener_configuration_is_accepted() {
    let (_directory, certificate_path, private_key_path) = tls_material();
    let listener =
        validate_trusted_https_listener("127.0.0.1:8443", &certificate_path, &private_key_path)
            .expect("valid host TLS material must be accepted");

    assert_eq!(listener.address().to_string(), "127.0.0.1:8443");
    assert!(listener.tls_config().alpn_protocols.is_empty());
}

#[test]
fn valid_certificate_chain_and_private_key_are_accepted() {
    let (_directory, certificate_path, private_key_path) = tls_material();
    let certificate = fs::read(&certificate_path).unwrap();
    fs::write(
        &certificate_path,
        [certificate.as_slice(), &certificate].concat(),
    )
    .unwrap();

    validate_trusted_https_listener("127.0.0.1:8443", &certificate_path, &private_key_path)
        .expect("a certificate chain and one private key must be accepted");
}

#[test]
fn tls_paths_with_interior_dot_components_are_rejected() {
    for (role, component) in [
        ("certificate", "."),
        ("certificate", ".."),
        ("private key", "."),
        ("private key", ".."),
    ] {
        let (_directory, certificate_path, private_key_path) = tls_material();
        let material_path = match role {
            "certificate" => &certificate_path,
            "private key" => &private_key_path,
            _ => unreachable!(),
        };
        let directory = material_path.parent().unwrap();
        let invalid_path = match component {
            "." => directory.join(".").join(material_path.file_name().unwrap()),
            ".." => directory
                .join("unused")
                .join("..")
                .join(material_path.file_name().unwrap()),
            _ => unreachable!(),
        };
        let (certificate_path, private_key_path) = match role {
            "certificate" => (invalid_path, private_key_path),
            "private key" => (certificate_path, invalid_path),
            _ => unreachable!(),
        };

        assert_tls_validation_error(validate_trusted_https_listener(
            "127.0.0.1:8443",
            &certificate_path,
            &private_key_path,
        ));
    }
}

#[test]
fn certificate_pem_with_prefix_or_suffix_plaintext_is_rejected() {
    for (prefix, suffix) in [("prefix\n", ""), ("", "\nsuffix\n")] {
        let (_directory, certificate_path, private_key_path) = tls_material();
        let certificate = fs::read(&certificate_path).unwrap();
        fs::write(
            &certificate_path,
            [prefix.as_bytes(), certificate.as_slice(), suffix.as_bytes()].concat(),
        )
        .unwrap();

        assert_tls_validation_error(validate_trusted_https_listener(
            "127.0.0.1:8443",
            &certificate_path,
            &private_key_path,
        ));
    }
}

#[test]
fn private_key_pem_with_prefix_or_suffix_plaintext_is_rejected() {
    for (prefix, suffix) in [("prefix\n", ""), ("", "\nsuffix\n")] {
        let (_directory, certificate_path, private_key_path) = tls_material();
        let private_key = fs::read(&private_key_path).unwrap();
        fs::write(
            &private_key_path,
            [prefix.as_bytes(), private_key.as_slice(), suffix.as_bytes()].concat(),
        )
        .unwrap();

        assert_tls_validation_error(validate_trusted_https_listener(
            "127.0.0.1:8443",
            &certificate_path,
            &private_key_path,
        ));
    }
}

#[test]
fn invalid_listener_address_is_rejected() {
    let (_directory, certificate_path, private_key_path) = tls_material();
    let error = match validate_trusted_https_listener(
        "localhost:8443",
        &certificate_path,
        &private_key_path,
    ) {
        Err(error) => error,
        Ok(_) => panic!("an invalid listener address must be rejected"),
    };
    assert_eq!(error, StartupError::ListenerAddressInvalid);
}

#[test]
fn missing_unsafe_malformed_and_mismatched_tls_material_are_rejected() {
    let (directory, certificate_path, private_key_path) = tls_material();
    let missing_path = directory.path().join("missing.pem");
    assert_tls_validation_error(validate_trusted_https_listener(
        "127.0.0.1:8443",
        &missing_path,
        &private_key_path,
    ));

    let unreadable_path = directory.path().join("unreadable.pem");
    fs::copy(&private_key_path, &unreadable_path).unwrap();
    fs::set_permissions(&unreadable_path, fs::Permissions::from_mode(0o000)).unwrap();
    assert_tls_validation_error(validate_trusted_https_listener(
        "127.0.0.1:8443",
        &certificate_path,
        &unreadable_path,
    ));

    let symlink_path = directory.path().join("certificate-link.pem");
    symlink(&certificate_path, &symlink_path).unwrap();
    assert_tls_validation_error(validate_trusted_https_listener(
        "127.0.0.1:8443",
        &symlink_path,
        &private_key_path,
    ));

    let hard_link_path = directory.path().join("certificate-hard-link.pem");
    fs::hard_link(&certificate_path, &hard_link_path).unwrap();
    assert_tls_validation_error(validate_trusted_https_listener(
        "127.0.0.1:8443",
        &certificate_path,
        &private_key_path,
    ));

    let malformed_path = directory.path().join("malformed.pem");
    fs::write(&malformed_path, "not PEM").unwrap();
    fs::set_permissions(&malformed_path, fs::Permissions::from_mode(0o600)).unwrap();
    assert_tls_validation_error(validate_trusted_https_listener(
        "127.0.0.1:8443",
        &malformed_path,
        &private_key_path,
    ));

    let (_other_directory, other_certificate_path, other_private_key_path) = tls_material();
    let _ = other_certificate_path;
    assert_tls_validation_error(validate_trusted_https_listener(
        "127.0.0.1:8443",
        &certificate_path,
        &other_private_key_path,
    ));
}

#[test]
fn invalid_tls_configuration_exits_before_lifecycle_state_is_created() {
    let directory = tempfile::tempdir().unwrap();
    let state_root = directory.path().join("state-root");
    let missing_material = directory.path().join("missing.pem");
    let output = Command::new(env!("CARGO_BIN_EXE_weavelit-server"))
        .env("WEAVELIT_HTTPS_LISTENER_ADDRESS", "127.0.0.1:8443")
        .env("WEAVELIT_TLS_CERTIFICATE_PATH", &missing_material)
        .env("WEAVELIT_TLS_PRIVATE_KEY_PATH", &missing_material)
        .env("WEAVELIT_STATE_ROOT", &state_root)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        output.stderr,
        b"{\"category\":\"configuration_invalid\",\"reason\":\"tls_material_invalid\"}\n"
    );
    assert!(!Path::new(&state_root).exists());
}

#[test]
fn occupied_listener_port_exits_with_the_preoperational_unavailable_pair() {
    let (_state_directory, state_root) = state_root();
    let (_tls_directory, certificate_path, private_key_path) = tls_material();
    let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = occupied.local_addr().unwrap().to_string();

    let output = Command::new(env!("CARGO_BIN_EXE_weavelit-server"))
        .env("WEAVELIT_HTTPS_LISTENER_ADDRESS", address)
        .env("WEAVELIT_TLS_CERTIFICATE_PATH", certificate_path)
        .env("WEAVELIT_TLS_PRIVATE_KEY_PATH", private_key_path)
        .env("WEAVELIT_STATE_ROOT", state_root)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        output.stderr,
        b"{\"category\":\"preoperational_unavailable\",\"reason\":\"https_listener_unavailable\"}\n"
    );
}

#[test]
fn serving_process_retains_state_root_lock() {
    let (_state_directory, state_root) = state_root();
    let (_tls_directory, certificate_path, private_key_path) = tls_material();
    let first_address = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let second_address = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();

    let mut first = Command::new(env!("CARGO_BIN_EXE_weavelit-server"))
        .env("WEAVELIT_HTTPS_LISTENER_ADDRESS", first_address.to_string())
        .env("WEAVELIT_TLS_CERTIFICATE_PATH", &certificate_path)
        .env("WEAVELIT_TLS_PRIVATE_KEY_PATH", &private_key_path)
        .env("WEAVELIT_STATE_ROOT", &state_root)
        .spawn()
        .unwrap();
    await_listener(first_address);

    let mut second = Command::new(env!("CARGO_BIN_EXE_weavelit-server"))
        .env(
            "WEAVELIT_HTTPS_LISTENER_ADDRESS",
            second_address.to_string(),
        )
        .env("WEAVELIT_TLS_CERTIFICATE_PATH", certificate_path)
        .env("WEAVELIT_TLS_PRIVATE_KEY_PATH", private_key_path)
        .env("WEAVELIT_STATE_ROOT", state_root)
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let status = loop {
        if let Some(status) = second.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = second.kill();
            let _ = second.wait();
            panic!("second server retained the state-root lock incorrectly");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut stderr = Vec::new();
    second
        .stderr
        .take()
        .unwrap()
        .read_to_end(&mut stderr)
        .unwrap();
    let _ = first.kill();
    let _ = first.wait();

    assert_eq!(status.code(), Some(1));
    assert_eq!(
        stderr,
        b"{\"category\":\"preoperational_unavailable\",\"reason\":\"state_root_in_use\"}\n"
    );
}

// ---------------------------------------------------------------------------
// Tests: fresh startup and restart
// ---------------------------------------------------------------------------

/// A real supervisor stop must end as a clean exit, not as a killed process,
/// and must leave nothing of the stopped generation holding the host.
#[test]
fn a_terminating_signal_stops_the_server_cleanly_and_frees_what_it_held() {
    let (_state_directory, state_root) = state_root();
    let (_tls_directory, certificate_path, private_key_path) = tls_material();
    let address = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();

    let mut server = Command::new(env!("CARGO_BIN_EXE_weavelit-server"))
        .env("WEAVELIT_HTTPS_LISTENER_ADDRESS", address.to_string())
        .env("WEAVELIT_TLS_CERTIFICATE_PATH", certificate_path)
        .env("WEAVELIT_TLS_PRIVATE_KEY_PATH", private_key_path)
        .env("WEAVELIT_STATE_ROOT", &state_root)
        .spawn()
        .unwrap();
    await_listener(address);
    // An ephemeral port is chosen by binding and releasing it, so a concurrent
    // test can win the same port and answer the probe above. The lock file
    // appears in this test's own state root, and only after this child has
    // registered its termination handler and bound its listener, so waiting for
    // it proves the signal below reaches a Server that can already catch it.
    await_state_root_lock(&state_root, &mut server);

    terminate(&server);
    let status = server.wait().unwrap();

    assert_eq!(status.code(), Some(0));
    assert_eq!(
        status.signal(),
        None,
        "a signalled shutdown must not end as a terminated process"
    );
    // The next Server generation can take back both the listener address and
    // the state root the stopped one held.
    drop(TcpListener::bind(address).unwrap());
    drop(LifecycleStore::open_or_create(&state_root).unwrap());
}

#[test]
fn fresh_first_start_classifies_as_uninitialized_without_database() {
    let (_dir, path) = state_root();
    let outcome = classify_restricted_startup(&path).expect("first start must succeed");
    assert_eq!(
        outcome.outcome(),
        StartupOutcome::UninitializedWithoutDatabase
    );
}

#[test]
fn restart_without_selection_is_stable() {
    let (_dir, path) = state_root();
    classify_restricted_startup(&path).unwrap();
    let outcome = classify_restricted_startup(&path).expect("restart must succeed");
    assert_eq!(
        outcome.outcome(),
        StartupOutcome::UninitializedWithoutDatabase
    );
}

#[test]
fn startup_exposes_sqlite_log_module_without_opening_or_delivering_to_it() {
    let (_dir, path) = state_root();
    let startup = classify_restricted_startup(&path).unwrap();
    let sqlite = startup
        .log_catalog()
        .declarations()
        .find(|declaration| declaration.identifier().as_str() == "sqlite")
        .expect("compiled-in catalog must expose the SQLite Log Module");

    assert!(
        sqlite
            .capabilities()
            .supports(weavelit_server_log::LogRecordType::System)
    );
    assert!(
        sqlite
            .capabilities()
            .supports(weavelit_server_log::LogRecordType::Audit)
    );
    for name in [
        "log.sqlite3",
        "log.sqlite3-journal",
        "log.sqlite3-wal",
        "log.sqlite3-shm",
    ] {
        assert!(
            !path.join(name).exists(),
            "unconfigured startup must not create {name}"
        );
    }
}

#[test]
fn restart_with_selected_database_classifies_as_uninitialized_with_database() {
    let (_dir, path) = state_root();
    {
        let mut store = LifecycleStore::open_or_create(&path).unwrap();
        store
            .select_database(&sqlite_catalog(), &context(&path), &sqlite(), vec![])
            .unwrap();
    }
    let outcome = classify_restricted_startup(&path).expect("restart with selection must succeed");
    assert_eq!(outcome.outcome(), StartupOutcome::UninitializedWithDatabase);
}

#[test]
fn retained_init_checkpoint_without_wal_reports_redeploy_new_without_mutation_or_bind() {
    let (_dir, path) = state_root();
    {
        let mut store = LifecycleStore::open_or_create(&path).unwrap();
        let database = store
            .select_database(&sqlite_catalog(), &context(&path), &sqlite(), vec![])
            .unwrap();
        let dep_id = store.record().deployment_identifier();
        drop(database);
        {
            let writer = Connection::open(path.join("application.sqlite3")).unwrap();
            writer
                .execute(
                    "INSERT INTO weavelit_lifecycle_state \
                     (singleton, deployment_identifier, state, workflow_kind, checkpoint_metadata) \
                     VALUES (1, ?1, 'pending', 'init', ?2)",
                    params![dep_id.as_bytes().as_slice(), b"meta"],
                )
                .unwrap();
        }
    }
    checkpoint_and_close_wal(&path);
    assert_interrupted_startup(&path, "operator_redeploy_new");
}

#[test]
fn retained_restore_checkpoint_without_wal_reports_redeploy_restore_without_mutation_or_bind() {
    let (_dir, path) = state_root();
    {
        let mut store = LifecycleStore::open_or_create(&path).unwrap();
        let database = store
            .select_database(&sqlite_catalog(), &context(&path), &sqlite(), vec![])
            .unwrap();
        let dep_id = store.record().deployment_identifier();
        drop(database);
        {
            let writer = Connection::open(path.join("application.sqlite3")).unwrap();
            writer
                .execute(
                    "INSERT INTO weavelit_lifecycle_state \
                     (singleton, deployment_identifier, state, workflow_kind, checkpoint_metadata) \
                     VALUES (1, ?1, 'pending', 'restore', ?2)",
                    params![dep_id.as_bytes().as_slice(), b"meta"],
                )
                .unwrap();
        }
    }
    checkpoint_and_close_wal(&path);
    assert_interrupted_startup(&path, "operator_redeploy_restore");
}

#[test]
fn retained_wal_requires_generic_redeploy_without_mutation_or_bind() {
    let (_dir, path) = state_root();
    let writer;
    {
        let mut store = LifecycleStore::open_or_create(&path).unwrap();
        let database = store
            .select_database(&sqlite_catalog(), &context(&path), &sqlite(), vec![])
            .unwrap();
        let dep_id = store.record().deployment_identifier();
        drop(database);
        writer = Connection::open(path.join("application.sqlite3")).unwrap();
        writer
            .execute(
                "INSERT INTO weavelit_lifecycle_state \
                 (singleton, deployment_identifier, state, workflow_kind, checkpoint_metadata) \
                 VALUES (1, ?1, 'pending', 'init', ?2)",
                params![dep_id.as_bytes().as_slice(), b"meta"],
            )
            .unwrap();
    }
    assert!(path.join("application.sqlite3-wal").exists());
    assert!(path.join("application.sqlite3-shm").exists());
    assert_interrupted_startup(&path, "operator_redeploy_required");
    drop(writer);
}

#[test]
fn retained_initialized_database_reports_redeploy_required_without_mutation_or_bind() {
    let (_dir, path) = state_root();
    let mut store = LifecycleStore::open_or_create(&path).unwrap();
    let deployment_identifier = store.record().deployment_identifier();
    store
        .select_database(&sqlite_catalog(), &context(&path), &sqlite(), vec![])
        .unwrap();
    drop(store);
    let database = Connection::open(path.join("application.sqlite3")).unwrap();
    database
        .execute(
            "INSERT INTO weavelit_lifecycle_state \
             (singleton, deployment_identifier, state, workflow_kind, checkpoint_metadata) \
             VALUES (1, ?1, 'initialized', NULL, NULL)",
            params![deployment_identifier.as_bytes().as_slice()],
        )
        .unwrap();

    assert_interrupted_startup(&path, "operator_redeploy_required");
    drop(database);
}

#[test]
fn retained_wal_over_a_pending_record_requires_generic_redeploy_without_mutation_or_bind() {
    let (_dir, path) = state_root();
    let writer;
    {
        let mut store = LifecycleStore::open_or_create(&path).unwrap();
        let catalog = sqlite_catalog();
        let database_context = context(&path);
        store
            .select_database(&catalog, &database_context, &sqlite(), vec![])
            .unwrap();
        let arbiter = WorkflowArbiter::new(store);
        // Advances the record to InitializationPending over a durable checkpoint.
        drop(
            arbiter
                .authorize_workflow(&catalog, &database_context)
                .unwrap()
                .create_checkpoint(
                    WorkflowKind::Init,
                    CheckpointMetadata::from_bytes(b"init-checkpoint-metadata".as_slice()).unwrap(),
                )
                .unwrap(),
        );
        drop(arbiter);
        // Rewrites the checkpoint row with its own bytes, so a real write-ahead
        // log exists over unchanged retained state, and holds it open.
        writer = Connection::open(path.join("application.sqlite3")).unwrap();
        writer
            .execute_batch(
                "PRAGMA wal_autocheckpoint = 0;\
                 UPDATE weavelit_lifecycle_state SET checkpoint_metadata = checkpoint_metadata;",
            )
            .unwrap();
    }
    assert!(path.join("application.sqlite3-wal").exists());
    assert!(path.join("application.sqlite3-shm").exists());
    assert_interrupted_startup(&path, "operator_redeploy_required");
    drop(writer);
}

// ---------------------------------------------------------------------------
// Tests: sealed startup over retained recovery data
// ---------------------------------------------------------------------------

#[test]
fn sealed_startup_recovers_a_retained_wal_and_loads_its_application_state() {
    let (_dir, path) = state_root();
    let deployment_identifier = seal_deployment(&path);
    abandon_write_ahead_log(&path);

    let startup = classify_restricted_startup(&path)
        .expect("a sealed deployment must start over a retained write-ahead log");

    assert_eq!(startup.outcome(), StartupOutcome::Initialized);
    let loaded = startup
        .initialized_state()
        .expect("a sealed deployment must retain its loaded application state");
    assert_eq!(loaded.deployment_identifier(), deployment_identifier);
    assert!(loaded.completion_acknowledged());
    drop(startup);

    // The probe write became durable in the main database, so the authoritative
    // read-write open recovered the log rather than discarding or ignoring it.
    let database_path = path.join("application.sqlite3");
    let recovered = Connection::open(&database_path).unwrap();
    let value: i64 = recovered
        .query_row("SELECT value FROM wal_recovery_probe", [], |row| row.get(0))
        .unwrap();
    assert_eq!(value, WAL_PROBE_VALUE);
}

#[test]
fn sealed_startup_over_an_unrecoverable_database_fails_closed_without_binding() {
    let (_dir, path) = state_root();
    seal_deployment(&path);

    // Nothing in the retained file is a database any more, so the authoritative
    // read-write open cannot succeed and no recovery can rescue it.
    let database_path = path.join("application.sqlite3");
    let length = fs::metadata(&database_path).unwrap().len();
    fs::write(&database_path, vec![0xA5; usize::try_from(length).unwrap()]).unwrap();

    assert_startup_failure(&path, "storage_unavailable", "database_unavailable");
}

#[test]
fn sealed_startup_over_another_deployments_database_fails_closed_without_binding() {
    let (_dir, path) = state_root();
    seal_deployment(&path);

    let database = Connection::open(path.join("application.sqlite3")).unwrap();
    database
        .execute(
            "UPDATE weavelit_lifecycle_state SET deployment_identifier = ?1",
            params![[0xA5_u8; 16].as_slice()],
        )
        .unwrap();
    drop(database);

    assert_startup_failure(&path, "storage_integrity_failure", "anchor_binding_invalid");
}

// ---------------------------------------------------------------------------
// Tests: fail-closed retained-state failures
// ---------------------------------------------------------------------------

#[test]
fn corrupted_anchor_fails_closed_with_anchor_set_invalid() {
    let (_dir, path) = state_root();
    classify_restricted_startup(&path).unwrap();

    let record_path = path.join("deployment-record.json");
    let mut bytes = fs::read(&record_path).unwrap();
    let pos = bytes.len() - 4;
    bytes[pos] = if bytes[pos] == b'A' { b'B' } else { b'A' };
    fs::write(&record_path, bytes).unwrap();

    let error = classify_restricted_startup(&path).unwrap_err();
    assert_eq!(error, StartupError::AnchorSetInvalid);
    let (category, _) = error.category_reason();
    assert_eq!(category, "storage_integrity_failure");
}

#[test]
fn orphaned_application_database_artifact_exits_before_binding() {
    let (_state_directory, state_root) = state_root();
    let artifact = state_root.join("application.sqlite3");
    fs::write(&artifact, b"retained application database artifact").unwrap();
    fs::set_permissions(&artifact, fs::Permissions::from_mode(0o600)).unwrap();
    let (_tls_directory, certificate_path, private_key_path) = tls_material();
    let address = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_weavelit-server"))
        .env("WEAVELIT_HTTPS_LISTENER_ADDRESS", address.to_string())
        .env("WEAVELIT_TLS_CERTIFICATE_PATH", certificate_path)
        .env("WEAVELIT_TLS_PRIVATE_KEY_PATH", private_key_path)
        .env("WEAVELIT_STATE_ROOT", state_root)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        output.stderr,
        b"{\"category\":\"storage_integrity_failure\",\"reason\":\"anchor_set_invalid\"}\n"
    );
    assert!(TcpStream::connect(address).is_err());
}

#[test]
fn retained_temporary_file_exits_before_binding_after_creating_the_lock() {
    let (_state_directory, state_root) = state_root();
    let temporary = state_root.join("deployment-record.json.tmp-ICEiIyQlJicoKSorLC0uLw");
    let temporary_bytes = b"retained lifecycle temporary";
    fs::write(&temporary, temporary_bytes).unwrap();
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600)).unwrap();
    let (_tls_directory, certificate_path, private_key_path) = tls_material();
    let address = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_weavelit-server"))
        .env("WEAVELIT_HTTPS_LISTENER_ADDRESS", address.to_string())
        .env("WEAVELIT_TLS_CERTIFICATE_PATH", certificate_path)
        .env("WEAVELIT_TLS_PRIVATE_KEY_PATH", private_key_path)
        .env("WEAVELIT_STATE_ROOT", &state_root)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        output.stderr,
        b"{\"category\":\"storage_integrity_failure\",\"reason\":\"anchor_set_invalid\"}\n"
    );
    assert!(TcpStream::connect(address).is_err());
    assert_eq!(fs::read(&temporary).unwrap(), temporary_bytes);
    assert!(state_root.join("lifecycle.lock").exists());
}

#[test]
fn concurrent_startup_fails_closed_as_state_root_in_use() {
    let (_dir, path) = state_root();
    let _store = LifecycleStore::open_or_create(&path).unwrap();

    let error = classify_restricted_startup(&path).unwrap_err();
    assert_eq!(error, StartupError::StateRootInUse);
    let (category, reason) = error.category_reason();
    assert_eq!(category, "preoperational_unavailable");
    assert_eq!(reason, "state_root_in_use");
}

// ---------------------------------------------------------------------------
// Tests: error category/reason pairs are well-formed and redacted
// ---------------------------------------------------------------------------

#[test]
fn every_startup_error_has_well_formed_category_reason() {
    let errors = [
        StartupError::ListenerNotConfigured,
        StartupError::ListenerAddressInvalid,
        StartupError::TlsCertificateNotConfigured,
        StartupError::TlsPrivateKeyNotConfigured,
        StartupError::TlsMaterialInvalid,
        StartupError::HttpsListenerUnavailable,
        StartupError::StateRootNotConfigured,
        StartupError::StateRootPathInvalid,
        StartupError::StateRootInUse,
        StartupError::StorageOperationFailed,
        StartupError::DatabaseUnavailable,
        StartupError::AnchorSetInvalid,
        StartupError::AnchorVersionUnsupported,
        StartupError::AnchorBindingInvalid,
        StartupError::DatabaseIntegrityFailure,
        StartupError::StateCombinationInvalid,
        StartupError::LifecycleInterruptedRedeployNew,
        StartupError::LifecycleInterruptedRedeployRestore,
        StartupError::LifecycleInterruptedRedeployRequired,
        StartupError::ShutdownIncomplete,
    ];
    let sensitive = "/private/weavelit/state-root/sensitive";
    for error in &errors {
        let (category, reason) = error.category_reason();
        assert!(
            !category.is_empty(),
            "category must not be empty for {error:?}"
        );
        assert!(!reason.is_empty(), "reason must not be empty for {error:?}");
        assert!(!format!("{error:?}").contains(sensitive));
        assert!(!category.contains(sensitive));
        assert!(!reason.contains(sensitive));
    }
}
