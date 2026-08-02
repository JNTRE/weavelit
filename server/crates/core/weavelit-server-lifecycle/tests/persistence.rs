use std::{
    collections::HashSet,
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use weavelit_server_lifecycle::{AnchorLoadState, LifecycleError, LifecycleState, LifecycleStore};

const KEY_VECTOR: &str = "{\"format_version\":1,\"key_algorithm\":\"xchacha20-poly1305\",\"key\":\"AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8\"}";
type Mutation = Box<dyn Fn(&Path)>;

fn state_root() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let canonical = directory.path().canonicalize().unwrap();
    (directory, canonical)
}

fn expect_open_error(path: &Path, expected: LifecycleError) {
    let error = LifecycleStore::open_or_create(path).unwrap_err();
    assert_eq!(error, expected);
    let output = format!("{error:?} {error}");
    assert!(!output.contains(path.to_string_lossy().as_ref()));
}

#[test]
fn first_start_and_restart_preserve_one_identity_with_restrictive_files() {
    let (_directory, path) = state_root();
    let store = LifecycleStore::open_or_create(&path).unwrap();
    let deployment_identifier = store.record().deployment_identifier();

    assert_eq!(store.load_state(), AnchorLoadState::FirstStartCreated);
    assert_eq!(store.record().state(), LifecycleState::Uninitialized);
    assert!(store.locator().is_none());
    for name in [
        "lifecycle.lock",
        "lifecycle-key.json",
        "deployment-record.json",
    ] {
        let metadata = fs::metadata(path.join(name)).unwrap();
        assert!(metadata.is_file());
        assert_eq!(metadata.mode() & 0o777, 0o600);
        assert_eq!(metadata.nlink(), 1);
    }
    drop(store);

    let reopened = LifecycleStore::open_or_create(&path).unwrap();
    assert_eq!(reopened.load_state(), AnchorLoadState::Retained);
    assert_eq!(
        reopened.record().deployment_identifier(),
        deployment_identifier
    );
    assert!(reopened.locator().is_none());
}

#[test]
fn key_only_interruption_resumes_with_a_fresh_deployment_identity() {
    let (_directory, path) = state_root();
    let store = LifecycleStore::open_or_create(&path).unwrap();
    let original_identifier = store.record().deployment_identifier();
    drop(store);
    fs::remove_file(path.join("deployment-record.json")).unwrap();

    let resumed = LifecycleStore::open_or_create(&path).unwrap();
    assert_eq!(resumed.load_state(), AnchorLoadState::FirstStartResumed);
    assert_ne!(
        resumed.record().deployment_identifier(),
        original_identifier
    );
}

#[test]
fn process_lifetime_lock_rejects_a_second_store() {
    let (_directory, path) = state_root();
    let _store = LifecycleStore::open_or_create(&path).unwrap();
    expect_open_error(&path, LifecycleError::LockContended);
}

#[test]
fn missing_wrong_and_tampered_key_or_record_fail_closed() {
    let mut cases: Vec<Mutation> = Vec::new();
    cases.push(Box::new(|path| {
        fs::remove_file(path.join("lifecycle-key.json")).unwrap();
    }));
    cases.push(Box::new(|path| {
        fs::write(path.join("lifecycle-key.json"), KEY_VECTOR).unwrap();
    }));
    cases.push(Box::new(|path| {
        let record_path = path.join("deployment-record.json");
        let mut bytes = fs::read(&record_path).unwrap();
        let position = bytes.len() - 3;
        bytes[position] = if bytes[position] == b'A' { b'B' } else { b'A' };
        fs::write(record_path, bytes).unwrap();
    }));

    for mutate in cases {
        let (_directory, path) = state_root();
        let store = LifecycleStore::open_or_create(&path).unwrap();
        drop(store);
        mutate(&path);
        expect_open_error(&path, LifecycleError::IntegrityFailure);
    }
}

#[test]
fn unknown_version_and_oversized_key_fail_closed() {
    let (_directory, path) = state_root();
    let store = LifecycleStore::open_or_create(&path).unwrap();
    drop(store);
    let version_two = KEY_VECTOR.replace("\"format_version\":1", "\"format_version\":2");
    fs::write(path.join("lifecycle-key.json"), version_two).unwrap();
    expect_open_error(&path, LifecycleError::UnsupportedVersion);

    fs::write(path.join("lifecycle-key.json"), vec![b'x'; 513]).unwrap();
    expect_open_error(&path, LifecycleError::IntegrityFailure);
}

#[test]
fn unsafe_mode_symlink_hardlink_and_unknown_entry_fail_closed() {
    let mutations: Vec<Mutation> = vec![
        Box::new(|path| {
            fs::set_permissions(
                path.join("lifecycle-key.json"),
                fs::Permissions::from_mode(0o640),
            )
            .unwrap();
        }),
        Box::new(|path| {
            fs::remove_file(path.join("deployment-record.json")).unwrap();
            symlink("lifecycle-key.json", path.join("deployment-record.json")).unwrap();
        }),
        Box::new(|path| {
            fs::hard_link(
                path.join("lifecycle-key.json"),
                path.join("unexpected-hardlink"),
            )
            .unwrap();
        }),
        Box::new(|path| {
            fs::write(path.join("unexpected-entry"), b"unexpected").unwrap();
        }),
    ];

    for mutate in mutations {
        let (_directory, path) = state_root();
        let store = LifecycleStore::open_or_create(&path).unwrap();
        drop(store);
        mutate(&path);
        let error = LifecycleStore::open_or_create(&path).unwrap_err();
        assert!(matches!(
            error,
            LifecycleError::ConfigurationInvalid | LifecycleError::IntegrityFailure
        ));
    }
}

#[test]
fn database_without_locator_and_locator_without_record_fail_closed() {
    let (_directory, path) = state_root();
    let store = LifecycleStore::open_or_create(&path).unwrap();
    drop(store);
    fs::write(path.join("application.sqlite3"), b"not opened by lifecycle").unwrap();
    expect_open_error(&path, LifecycleError::IntegrityFailure);

    fs::remove_file(path.join("application.sqlite3")).unwrap();
    let store = LifecycleStore::open_or_create(&path).unwrap();
    drop(store);
    fs::write(
        path.join("database-locator-ICEiIyQlJicoKSorLC0uLw.json"),
        b"orphan locator",
    )
    .unwrap();
    fs::remove_file(path.join("deployment-record.json")).unwrap();
    expect_open_error(&path, LifecycleError::IntegrityFailure);
}

#[test]
fn symlinked_root_and_wrong_root_mode_are_rejected() {
    let (directory, canonical) = state_root();
    let link = directory.path().with_extension("link");
    symlink(&canonical, &link).unwrap();
    expect_open_error(&link, LifecycleError::ConfigurationInvalid);

    fs::set_permissions(&canonical, fs::Permissions::from_mode(0o750)).unwrap();
    expect_open_error(&canonical, LifecycleError::ConfigurationInvalid);
    fs::remove_file(link).unwrap();

    let parent = tempfile::tempdir().unwrap();
    let parent = parent.path().canonicalize().unwrap();
    let real_parent = parent.join("real-parent");
    let nested_root = real_parent.join("state-root");
    fs::create_dir(&real_parent).unwrap();
    fs::create_dir(&nested_root).unwrap();
    fs::set_permissions(&nested_root, fs::Permissions::from_mode(0o700)).unwrap();
    let linked_parent = parent.join("linked-parent");
    symlink(&real_parent, &linked_parent).unwrap();
    expect_open_error(
        &linked_parent.join("state-root"),
        LifecycleError::ConfigurationInvalid,
    );
}

#[test]
fn state_root_entry_count_is_bounded_before_loading() {
    let (_directory, path) = state_root();
    let store = LifecycleStore::open_or_create(&path).unwrap();
    drop(store);
    let mut casefolded_tokens = HashSet::new();
    let mut index = 0_u32;
    while casefolded_tokens.len() < 254 {
        let mut bytes = [0_u8; 16];
        bytes[0] = 1;
        bytes[12..].copy_from_slice(&index.to_be_bytes());
        index += 1;
        let token = URL_SAFE_NO_PAD.encode(bytes);
        if !casefolded_tokens.insert(token.to_ascii_lowercase()) {
            continue;
        }
        fs::write(
            path.join(format!("deployment-record.json.tmp-{token}")),
            b"temporary",
        )
        .unwrap();
    }
    assert_eq!(fs::read_dir(&path).unwrap().count(), 257);
    expect_open_error(&path, LifecycleError::IntegrityFailure);
}
