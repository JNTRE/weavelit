use std::{fs, io::ErrorKind, os::unix::ffi::OsStrExt, path::Path, time::Duration};

use percent_encoding::{AsciiSet, CONTROLS, percent_encode};
use rusqlite::{Connection, OpenFlags};
use weavelit_server_database::{DatabaseError, DatabaseInspection, DeploymentIdentifier};

use crate::error::{ErrorContext, map_sqlite_error};
use crate::migrations::apply_pending;

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const BUSY_TIMEOUT_MILLISECONDS: i64 = 5_000;
const EXPECTED_HEALTH_RESULT: i64 = 1;
/// The value `PRAGMA wal_checkpoint` reports when nothing blocked it.
const CHECKPOINT_NOT_BLOCKED: i64 = 0;
/// The frame count a truncating checkpoint must leave behind.
const CHECKPOINT_NO_FRAMES_REMAIN: i64 = 0;
const RETAINED_INSPECTION_URI_PATH: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'!')
    .add(b'"')
    .add(b'#')
    .add(b'$')
    .add(b'%')
    .add(b'&')
    .add(b'\'')
    .add(b'(')
    .add(b')')
    .add(b'*')
    .add(b'+')
    .add(b',')
    .add(b':')
    .add(b';')
    .add(b'<')
    .add(b'=')
    .add(b'>')
    .add(b'?')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

/// Non-mutating result of inspecting retained SQLite state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RetainedSqliteInspection {
    /// SQLite safely inspected retained main-database state without a WAL.
    Inspected(DatabaseInspection),
    /// A retained WAL is present and must not be opened during classification.
    WalPresent,
}

/// SQLite Application Database backend with one privately owned connection.
pub struct SqliteDatabase {
    pub(super) connection: Connection,
}

impl SqliteDatabase {
    /// Opens and verifies a SQLite database at a trusted Server-supplied path.
    pub fn open(path: &Path) -> Result<Self, DatabaseError> {
        let connection = Connection::open_with_flags(path, trusted_open_flags())
            .map_err(|error| map_sqlite_error(error, ErrorContext::Open))?;

        configure_connection(&connection)?;
        let mut database = Self { connection };
        database.verify_health()?;
        apply_pending(&mut database.connection)?;

        Ok(database)
    }

    /// Checkpoints the write-ahead log and closes the connection.
    ///
    /// Consuming the database is what makes a use-after-close unwriteable. The
    /// connection is closed even when the checkpoint fails, because a database
    /// stopped with a retained write-ahead log still depends on SQLite's own
    /// recovery, and neither failure is reported as a clean stop.
    pub fn close(self) -> Result<(), DatabaseError> {
        let Self { connection } = self;
        let checkpointed = checkpoint_truncate(&connection);
        let closed = connection.close().map_err(|(connection, error)| {
            // A failed close hands the connection back rather than dropping it,
            // so it is disposed of here instead of escaping as a live handle.
            drop(connection);
            map_sqlite_error(error, ErrorContext::Close)
        });

        checkpointed.and(closed)
    }

    /// Inspects an existing trusted database without applying connection setup or migrations.
    pub fn inspect_retained(
        path: &Path,
        expected_deployment_identifier: DeploymentIdentifier,
    ) -> Result<RetainedSqliteInspection, DatabaseError> {
        if retained_wal_exists(path)? {
            return Ok(RetainedSqliteInspection::WalPresent);
        }
        let uri = retained_inspection_uri(path)?;
        let connection =
            Connection::open_with_flags(Path::new(&uri), retained_inspection_open_flags())
                .map_err(|error| map_sqlite_error(error, ErrorContext::Open))?;
        crate::inspection::inspect_connection(&connection, expected_deployment_identifier)
            .map(RetainedSqliteInspection::Inspected)
    }

    fn verify_health(&self) -> Result<(), DatabaseError> {
        let result: i64 = self
            .connection
            .query_row("SELECT 1", [], |row| row.get(0))
            .map_err(|error| map_sqlite_error(error, ErrorContext::Health))?;
        if result != EXPECTED_HEALTH_RESULT {
            return Err(DatabaseError::IntegrityFailure);
        }

        Ok(())
    }
}

fn trusted_open_flags() -> OpenFlags {
    OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_NOFOLLOW
}

fn retained_inspection_open_flags() -> OpenFlags {
    OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_NOFOLLOW
        | OpenFlags::SQLITE_OPEN_URI
}

fn retained_inspection_uri(path: &Path) -> Result<String, DatabaseError> {
    if !path.is_absolute() {
        return Err(DatabaseError::ConfigurationInvalid);
    }
    Ok(format!(
        "file:{}?immutable=1",
        percent_encode(path.as_os_str().as_bytes(), RETAINED_INSPECTION_URI_PATH)
    ))
}

/// Empties the write-ahead log and verifies that it was actually emptied.
///
/// A truncating checkpoint reports whether it was blocked and how many frames
/// remain, and SQLite returns both without failing the statement, so the
/// reported outcome is checked rather than assumed from a successful query.
fn checkpoint_truncate(connection: &Connection) -> Result<(), DatabaseError> {
    let (blocked, remaining_frames, _checkpointed_frames): (i64, i64, i64) = connection
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .map_err(|error| map_sqlite_error(error, ErrorContext::Close))?;
    if blocked != CHECKPOINT_NOT_BLOCKED || remaining_frames != CHECKPOINT_NO_FRAMES_REMAIN {
        return Err(DatabaseError::Unavailable);
    }

    Ok(())
}

fn retained_wal_exists(path: &Path) -> Result<bool, DatabaseError> {
    let mut wal_name = path
        .file_name()
        .ok_or(DatabaseError::ConfigurationInvalid)?
        .to_os_string();
    wal_name.push("-wal");
    match fs::symlink_metadata(path.with_file_name(wal_name)) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(_) => Err(DatabaseError::Unavailable),
    }
}

fn configure_connection(connection: &Connection) -> Result<(), DatabaseError> {
    connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(|error| map_sqlite_error(error, ErrorContext::Configure))?;
    let foreign_keys: i64 = connection
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .map_err(|error| map_sqlite_error(error, ErrorContext::Configure))?;
    if foreign_keys != 1 {
        return Err(DatabaseError::ConfigurationInvalid);
    }

    let journal_mode: String = connection
        .pragma_update_and_check(None, "journal_mode", "wal", |row| row.get(0))
        .map_err(|error| map_sqlite_error(error, ErrorContext::Configure))?;
    if !journal_mode.eq_ignore_ascii_case("wal") {
        return Err(DatabaseError::ConfigurationInvalid);
    }

    connection
        .busy_timeout(BUSY_TIMEOUT)
        .map_err(|error| map_sqlite_error(error, ErrorContext::Configure))?;
    let busy_timeout: i64 = connection
        .pragma_query_value(None, "busy_timeout", |row| row.get(0))
        .map_err(|error| map_sqlite_error(error, ErrorContext::Configure))?;
    if busy_timeout != BUSY_TIMEOUT_MILLISECONDS {
        return Err(DatabaseError::ConfigurationInvalid);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_configures_and_verifies_real_sqlite_connection() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let database_path = temporary_directory
            .path()
            .canonicalize()
            .unwrap()
            .join("application.db");
        let database = SqliteDatabase::open(&database_path).expect("a new database should open");

        let foreign_keys: i64 = database
            .connection
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .unwrap();
        let journal_mode: String = database
            .connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        let busy_timeout: i64 = database
            .connection
            .pragma_query_value(None, "busy_timeout", |row| row.get(0))
            .unwrap();
        let health_result: i64 = database
            .connection
            .query_row("SELECT 1", [], |row| row.get(0))
            .unwrap();

        assert_eq!(foreign_keys, 1);
        assert_eq!(journal_mode, "wal");
        assert_eq!(busy_timeout, BUSY_TIMEOUT_MILLISECONDS);
        assert_eq!(health_result, EXPECTED_HEALTH_RESULT);
    }

    /// Builds a database with a real, non-empty write-ahead log beside it.
    fn database_with_write_ahead_log() -> (tempfile::TempDir, std::path::PathBuf, SqliteDatabase) {
        let temporary_directory = tempfile::tempdir().unwrap();
        let database_path = temporary_directory
            .path()
            .canonicalize()
            .unwrap()
            .join("application.db");
        let database = SqliteDatabase::open(&database_path).expect("a new database should open");
        database
            .connection
            .execute_batch(
                "PRAGMA wal_autocheckpoint = 0;\
                 CREATE TABLE close_probe (value INTEGER NOT NULL);\
                 INSERT INTO close_probe (value) VALUES (7);",
            )
            .unwrap();
        assert!(
            database_path
                .with_extension("db-wal")
                .metadata()
                .unwrap()
                .len()
                > 0,
            "the committed write must live in a non-empty write-ahead log"
        );

        (temporary_directory, database_path, database)
    }

    #[test]
    fn close_truncates_the_write_ahead_log_and_leaves_no_sidecar() {
        let (_directory, database_path, database) = database_with_write_ahead_log();

        SqliteDatabase::close(database).expect("a sole connection must close cleanly");

        assert!(!database_path.with_extension("db-wal").exists());
        assert!(!database_path.with_extension("db-shm").exists());
        // The committed write survived the checkpoint into the main database.
        let uri = format!("file:{}?immutable=1", database_path.display());
        let retained = Connection::open_with_flags(
            Path::new(&uri),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )
        .unwrap();
        let value: i64 = retained
            .query_row("SELECT value FROM close_probe", [], |row| row.get(0))
            .unwrap();
        assert_eq!(value, 7);
    }

    #[test]
    fn a_blocked_checkpoint_reports_failure_and_still_releases_the_connection() {
        let (_directory, database_path, database) = database_with_write_ahead_log();

        // A second connection holding a read transaction blocks a truncating
        // checkpoint, which SQLite reports through the pragma rather than by
        // failing the statement.
        let reader = Connection::open(&database_path).unwrap();
        reader
            .execute_batch("BEGIN; SELECT count(*) FROM close_probe;")
            .unwrap();

        let error = SqliteDatabase::close(database)
            .expect_err("a blocked truncating checkpoint must not report a clean stop");
        assert_eq!(error, DatabaseError::Unavailable);

        // The connection was closed anyway: the reader can finish and the
        // database is fully usable through a newly opened connection.
        reader.execute_batch("COMMIT;").unwrap();
        drop(reader);
        let reopened = SqliteDatabase::open(&database_path).expect("the database must reopen");
        let value: i64 = reopened
            .connection
            .query_row("SELECT value FROM close_probe", [], |row| row.get(0))
            .unwrap();
        assert_eq!(value, 7);
    }
}
