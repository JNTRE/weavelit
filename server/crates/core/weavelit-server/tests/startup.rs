use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::Command,
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

// ---------------------------------------------------------------------------
// Tests: fresh startup and restart
// ---------------------------------------------------------------------------

#[test]
fn fresh_first_start_classifies_as_uninitialized_without_database() {
    let (_dir, path) = state_root();
    let outcome = classify_restricted_startup(&path).expect("first start must succeed");
    assert_eq!(outcome, StartupOutcome::UninitializedWithoutDatabase);
}

#[test]
fn restart_without_selection_is_stable() {
    let (_dir, path) = state_root();
    classify_restricted_startup(&path).unwrap();
    let outcome = classify_restricted_startup(&path).expect("restart must succeed");
    assert_eq!(outcome, StartupOutcome::UninitializedWithoutDatabase);
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
    assert_eq!(outcome, StartupOutcome::UninitializedWithDatabase);
}

#[test]
fn restart_with_init_pending_classifies_as_initialization_pending() {
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
    let outcome = classify_restricted_startup(&path).expect("init-pending restart must succeed");
    assert_eq!(
        outcome,
        StartupOutcome::InitializationPending(WorkflowKind::Init)
    );
}

#[test]
fn restart_with_restore_pending_classifies_as_initialization_pending() {
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
    let outcome = classify_restricted_startup(&path).expect("restore-pending restart must succeed");
    assert_eq!(
        outcome,
        StartupOutcome::InitializationPending(WorkflowKind::Restore)
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
