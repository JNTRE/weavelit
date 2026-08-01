use std::path::{Path, PathBuf};

use tempfile::TempDir;
use weavelit_server_database::DatabaseError;
use weavelit_server_database_sqlite::SqliteDatabase;

fn database_path(temporary_directory: &TempDir, file_name: &str) -> PathBuf {
    temporary_directory
        .path()
        .canonicalize()
        .unwrap()
        .join(file_name)
}

fn error_from_open(path: &Path) -> DatabaseError {
    match SqliteDatabase::open(path) {
        Ok(_) => panic!("database open should fail"),
        Err(error) => error,
    }
}

fn assert_redacted(error: DatabaseError, sensitive_path: &Path) {
    let message = error.to_string();
    let lower_message = message.to_ascii_lowercase();

    assert!(!message.contains(&sensitive_path.to_string_lossy().to_string()));
    assert!(!lower_message.contains("sqlite"));
    assert!(!lower_message.contains("unable to open"));
    assert!(!lower_message.contains("permission denied"));
    assert!(!lower_message.contains("select 1"));
}

#[test]
fn opens_closes_and_reopens_real_database_file() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory, "application.db");

    drop(SqliteDatabase::open(&path).unwrap());
    assert!(path.is_file());

    drop(SqliteDatabase::open(&path).unwrap());
}

#[test]
fn missing_parent_storage_is_unavailable_and_redacted() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory, "missing/application.db");

    let error = error_from_open(&path);

    assert_eq!(error, DatabaseError::Unavailable);
    assert_redacted(error, &path);
}

#[test]
fn non_database_file_is_an_integrity_failure_and_redacted() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory, "not-a-database.db");
    std::fs::write(&path, b"representative-sensitive-content").unwrap();

    let error = error_from_open(&path);

    assert_eq!(error, DatabaseError::IntegrityFailure);
    assert_redacted(error, &path);
    assert!(
        !error
            .to_string()
            .contains("representative-sensitive-content")
    );
}

#[test]
#[cfg(unix)]
fn final_component_symlink_is_rejected() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let target_path = database_path(&temporary_directory, "target.db");
    let symlink_path = database_path(&temporary_directory, "linked.db");
    drop(SqliteDatabase::open(&target_path).unwrap());
    std::os::unix::fs::symlink(&target_path, &symlink_path).unwrap();

    let error = error_from_open(&symlink_path);

    assert_eq!(error, DatabaseError::Unavailable);
    assert_redacted(error, &symlink_path);
}

#[test]
#[cfg(unix)]
fn parent_component_symlink_is_rejected() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let target_directory = database_path(&temporary_directory, "target");
    let symlink_directory = database_path(&temporary_directory, "linked");
    std::fs::create_dir(&target_directory).unwrap();
    std::os::unix::fs::symlink(&target_directory, &symlink_directory).unwrap();
    let path = symlink_directory.join("application.db");

    let error = error_from_open(&path);

    assert_eq!(error, DatabaseError::Unavailable);
    assert_redacted(error, &path);
}

#[test]
#[cfg(unix)]
fn query_like_filename_is_treated_as_a_literal_path() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(
        &temporary_directory,
        "application.db?mode=memory&cache=shared",
    );

    drop(SqliteDatabase::open(&path).unwrap());

    assert!(path.is_file());
}

#[test]
#[cfg(unix)]
fn unrepresentable_path_is_invalid_configuration_and_redacted() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let temporary_directory = tempfile::tempdir().unwrap();
    let mut path = temporary_directory.path().canonicalize().unwrap();
    path.push(OsString::from_vec(b"invalid\0database.db".to_vec()));

    let error = error_from_open(&path);

    assert_eq!(error, DatabaseError::ConfigurationInvalid);
    assert_redacted(error, &path);
}
