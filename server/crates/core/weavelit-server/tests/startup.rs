use std::{
    fs,
    io::Read,
    net::{TcpListener, TcpStream},
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use weavelit_server::{
    StartupError, StartupOutcome, classify_restricted_startup, sqlite_catalog,
    validate_trusted_https_listener,
};
use weavelit_server_lifecycle::{
    BackendIdentifier, CheckpointMetadata, DeploymentIdentifier, LifecycleStore,
    TrustedBackendContext, WorkflowCheckpoint, WorkflowKind,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn state_root() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let canonical = directory.path().canonicalize().unwrap();
    (directory, canonical)
}

fn root_snapshot(path: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut entries = fs::read_dir(path)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                PathBuf::from(entry.file_name()),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    entries
}

fn context(root: &std::path::Path) -> TrustedBackendContext {
    TrustedBackendContext::new(root.join("application.sqlite3"))
}

fn sqlite() -> BackendIdentifier {
    BackendIdentifier::new("sqlite").unwrap()
}

fn checkpoint(dep_id: DeploymentIdentifier, kind: WorkflowKind) -> WorkflowCheckpoint {
    WorkflowCheckpoint::new(
        dep_id,
        kind,
        CheckpointMetadata::from_bytes(b"meta".as_slice()).unwrap(),
    )
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
    let deadline = Instant::now() + Duration::from_secs(2);
    while TcpStream::connect(first_address).is_err() {
        assert!(
            Instant::now() < deadline,
            "first server did not bind in time"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

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
fn restart_with_init_pending_fails_closed() {
    let (_dir, path) = state_root();
    {
        let mut store = LifecycleStore::open_or_create(&path).unwrap();
        store
            .select_database(&sqlite_catalog(), &context(&path), &sqlite(), vec![])
            .unwrap();
        let dep_id = store.record().deployment_identifier();
        let mut db = store
            .reopen_selected_database(&sqlite_catalog(), &context(&path))
            .unwrap();
        db.create_checkpoint(&checkpoint(dep_id, WorkflowKind::Init))
            .unwrap();
        drop(db);
    }
    assert_eq!(
        classify_restricted_startup(&path).unwrap_err(),
        StartupError::DatabaseIntegrityFailure
    );
}

#[test]
fn restart_with_restore_pending_fails_closed() {
    let (_dir, path) = state_root();
    {
        let mut store = LifecycleStore::open_or_create(&path).unwrap();
        store
            .select_database(&sqlite_catalog(), &context(&path), &sqlite(), vec![])
            .unwrap();
        let dep_id = store.record().deployment_identifier();
        let mut db = store
            .reopen_selected_database(&sqlite_catalog(), &context(&path))
            .unwrap();
        db.create_checkpoint(&checkpoint(dep_id, WorkflowKind::Restore))
            .unwrap();
        drop(db);
    }
    assert_eq!(
        classify_restricted_startup(&path).unwrap_err(),
        StartupError::DatabaseIntegrityFailure
    );
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
fn retained_temporary_file_exits_before_binding_without_mutating_the_root() {
    let (_state_directory, state_root) = state_root();
    let temporary = state_root.join("deployment-record.json.tmp-ICEiIyQlJicoKSorLC0uLw");
    fs::write(&temporary, b"retained lifecycle temporary").unwrap();
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600)).unwrap();
    let before = root_snapshot(&state_root);
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
    assert_eq!(root_snapshot(&state_root), before);
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
