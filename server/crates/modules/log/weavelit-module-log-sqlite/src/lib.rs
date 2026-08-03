#![forbid(unsafe_code)]

//! Durable SQLite destination for complete, pre-redacted Server log records.

use std::{fs, io::ErrorKind, path::Path, sync::Mutex, time::Duration};

use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};
use sha2::{Digest, Sha256};
use weavelit_server_log::{
    CompleteLogRecord, DurableAcknowledgement, LogCapabilities, LogDestination,
    LogDestinationError, LogDestinationFactory, LogModuleRegistration, LogRecordPersistenceView,
    LogRecordType, TrustedLogModuleContext,
};

const MODULE_IDENTIFIER: &str = "sqlite";
const DATABASE_FILENAME: &str = "log.sqlite3";
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const BUSY_TIMEOUT_MILLISECONDS: i64 = 5_000;
const EXPECTED_HEALTH_RESULT: i64 = 1;
const LEDGER_TABLE: &str = "weavelit_log_migration_ledger";
const DEPLOYMENT_TABLE: &str = "weavelit_log_deployment_binding";

struct Migration {
    sequence: i64,
    identifier: &'static str,
    sql: &'static str,
}

#[derive(Eq, PartialEq)]
struct SchemaObject {
    object_type: String,
    name: String,
    table_name: String,
    sql: Option<String>,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        sequence: 1,
        identifier: "0001_create_log_destination_schema",
        sql: "CREATE TABLE weavelit_log_migration_ledger (\
            sequence_number INTEGER PRIMARY KEY,\
            identifier TEXT NOT NULL UNIQUE,\
            checksum BLOB NOT NULL\
          );\
          CREATE TABLE weavelit_log_deployment_binding (\
            singleton INTEGER PRIMARY KEY CHECK (singleton = 1),\
            deployment_identity BLOB NOT NULL CHECK (length(deployment_identity) = 16)\
          );\
          CREATE TABLE weavelit_log_system_records (\
            record_id BLOB PRIMARY KEY CHECK (length(record_id) = 16),\
            event_time_milliseconds TEXT NOT NULL,\
            result INTEGER NOT NULL CHECK (result IN (0, 1)),\
            correlation_id TEXT NOT NULL,\
            classification TEXT NOT NULL,\
            detail TEXT NOT NULL\
          );\
          CREATE TABLE weavelit_log_audit_records (\
            record_id BLOB PRIMARY KEY CHECK (length(record_id) = 16),\
            event_time_milliseconds TEXT NOT NULL,\
            result INTEGER NOT NULL CHECK (result IN (0, 1)),\
            correlation_id TEXT NOT NULL,\
            principal TEXT NOT NULL,\
            action TEXT NOT NULL,\
            target TEXT NOT NULL,\
            detail TEXT NOT NULL\
          );",
    },
    Migration {
        sequence: 2,
        identifier: "0002_bound_record_payloads",
        sql: "ALTER TABLE weavelit_log_system_records RENAME TO weavelit_log_system_records_legacy;\
                    ALTER TABLE weavelit_log_audit_records RENAME TO weavelit_log_audit_records_legacy;\
                    CREATE TABLE weavelit_log_system_records (\
                        record_id BLOB PRIMARY KEY CHECK (length(record_id) = 16),\
                        event_time_milliseconds TEXT NOT NULL,\
                        result INTEGER NOT NULL CHECK (result IN (0, 1)),\
                        correlation_id TEXT NOT NULL CHECK (length(CAST(correlation_id AS BLOB)) BETWEEN 1 AND 64),\
                        classification TEXT NOT NULL CHECK (length(CAST(classification AS BLOB)) BETWEEN 1 AND 128),\
                        detail TEXT NOT NULL CHECK (length(CAST(detail AS BLOB)) BETWEEN 1 AND 4096),\
                        CHECK (length(CAST(correlation_id AS BLOB)) + length(CAST(classification AS BLOB)) + length(CAST(detail AS BLOB)) <= 8192)\
                    );\
                    CREATE TABLE weavelit_log_audit_records (\
                        record_id BLOB PRIMARY KEY CHECK (length(record_id) = 16),\
                        event_time_milliseconds TEXT NOT NULL,\
                        result INTEGER NOT NULL CHECK (result IN (0, 1)),\
                        correlation_id TEXT NOT NULL CHECK (length(CAST(correlation_id AS BLOB)) BETWEEN 1 AND 64),\
                        principal TEXT NOT NULL CHECK (length(CAST(principal AS BLOB)) BETWEEN 1 AND 256),\
                        action TEXT NOT NULL CHECK (length(CAST(action AS BLOB)) BETWEEN 1 AND 128),\
                        target TEXT NOT NULL CHECK (length(CAST(target AS BLOB)) BETWEEN 1 AND 1024),\
                        detail TEXT NOT NULL CHECK (length(CAST(detail AS BLOB)) BETWEEN 1 AND 4096),\
                        CHECK (length(CAST(correlation_id AS BLOB)) + length(CAST(principal AS BLOB)) + length(CAST(action AS BLOB)) + length(CAST(target AS BLOB)) + length(CAST(detail AS BLOB)) <= 8192)\
                    );\
                    INSERT INTO weavelit_log_system_records \
                        (record_id, event_time_milliseconds, result, correlation_id, classification, detail) \
                        SELECT record_id, event_time_milliseconds, result, correlation_id, classification, detail \
                        FROM weavelit_log_system_records_legacy;\
                    INSERT INTO weavelit_log_audit_records \
                        (record_id, event_time_milliseconds, result, correlation_id, principal, action, target, detail) \
                        SELECT record_id, event_time_milliseconds, result, correlation_id, principal, action, target, detail \
                        FROM weavelit_log_audit_records_legacy;\
                    DROP TABLE weavelit_log_system_records_legacy;\
                    DROP TABLE weavelit_log_audit_records_legacy;",
    },
];

/// Factory for the compiled-in SQLite Log Module destination.
pub struct SqliteLogDestinationFactory;

impl LogDestinationFactory for SqliteLogDestinationFactory {
    fn create(
        &self,
        context: &TrustedLogModuleContext,
    ) -> Result<Box<dyn LogDestination>, LogDestinationError> {
        Ok(Box::new(SqliteLogDestination::open(context)?))
    }
}

/// Returns the compiled-in SQLite Log Module registration.
pub fn registration() -> LogModuleRegistration {
    let capabilities = LogCapabilities::new(vec![LogRecordType::System, LogRecordType::Audit])
        .expect("the SQLite Log Module capability declaration is valid");
    LogModuleRegistration::new(
        MODULE_IDENTIFIER,
        capabilities,
        Box::new(SqliteLogDestinationFactory),
    )
}

/// SQLite destination with one privately owned, serialized connection.
pub struct SqliteLogDestination {
    connection: Mutex<Connection>,
}

impl SqliteLogDestination {
    /// Opens the fixed destination beneath the trusted Server-owned local root.
    pub fn open(context: &TrustedLogModuleContext) -> Result<Self, LogDestinationError> {
        validate_registry()?;
        let database_path = context.local_root().join(DATABASE_FILENAME);
        let fresh = database_is_fresh(&database_path)?;
        if fresh {
            reserve_fresh_database(&database_path)?;
        }
        let connection = Connection::open_with_flags(&database_path, trusted_open_flags())
            .map_err(|_| LogDestinationError::Unavailable)?;

        let mut destination = Self {
            connection: Mutex::new(connection),
        };
        destination.verify_health()?;
        if fresh {
            destination.configure_connection()?;
            destination.bootstrap(context.deployment_identity())?;
        } else {
            destination.validate_existing(context.deployment_identity())?;
            destination.configure_connection()?;
        }
        destination.apply_migrations(context.deployment_identity())?;
        Ok(destination)
    }

    fn verify_health(&self) -> Result<(), LogDestinationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| LogDestinationError::Unavailable)?;
        verify_health(&connection)
    }

    fn configure_connection(&mut self) -> Result<(), LogDestinationError> {
        let connection = self
            .connection
            .get_mut()
            .map_err(|_| LogDestinationError::Unavailable)?;
        configure_connection(connection)
    }

    fn validate_existing(
        &mut self,
        deployment_identity: &[u8; 16],
    ) -> Result<(), LogDestinationError> {
        let connection = self
            .connection
            .get_mut()
            .map_err(|_| LogDestinationError::Unavailable)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| LogDestinationError::Unavailable)?;
        let applied = load_and_validate_applied(&transaction)?;
        if applied == 0 {
            return Err(LogDestinationError::IntegrityFailure);
        }
        validate_schema(&transaction, &MIGRATIONS[..applied])?;
        validate_deployment_binding(&transaction, deployment_identity)?;
        transaction
            .commit()
            .map_err(|_| LogDestinationError::Unavailable)
    }

    fn bootstrap(&mut self, deployment_identity: &[u8; 16]) -> Result<(), LogDestinationError> {
        let connection = self
            .connection
            .get_mut()
            .map_err(|_| LogDestinationError::Unavailable)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| LogDestinationError::Unavailable)?;
        let migration = &MIGRATIONS[0];
        transaction
            .execute_batch(migration.sql)
            .map_err(|_| LogDestinationError::IntegrityFailure)?;
        insert_migration_ledger_entry(&transaction, migration)?;
        transaction
            .execute(
                &format!(
                    "INSERT INTO {DEPLOYMENT_TABLE} (singleton, deployment_identity) VALUES (1, ?1)"
                ),
                [deployment_identity.as_slice()],
            )
            .map_err(|_| LogDestinationError::IntegrityFailure)?;
        transaction
            .commit()
            .map_err(|_| LogDestinationError::Unavailable)
    }

    fn apply_migrations(
        &mut self,
        deployment_identity: &[u8; 16],
    ) -> Result<(), LogDestinationError> {
        let connection = self
            .connection
            .get_mut()
            .map_err(|_| LogDestinationError::Unavailable)?;

        loop {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|_| LogDestinationError::Unavailable)?;
            let applied = load_and_validate_applied(&transaction)?;
            validate_schema(&transaction, &MIGRATIONS[..applied])?;
            validate_deployment_binding(&transaction, deployment_identity)?;

            let Some(migration) = MIGRATIONS.get(applied) else {
                transaction
                    .commit()
                    .map_err(|_| LogDestinationError::Unavailable)?;
                return Ok(());
            };

            transaction
                .execute_batch(migration.sql)
                .map_err(|_| LogDestinationError::IntegrityFailure)?;
            insert_migration_ledger_entry(&transaction, migration)?;
            transaction
                .commit()
                .map_err(|_| LogDestinationError::Unavailable)?;
        }
    }
}

impl LogDestination for SqliteLogDestination {
    fn deliver(
        &self,
        record: &CompleteLogRecord,
    ) -> Result<DurableAcknowledgement, LogDestinationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| LogDestinationError::Unavailable)?;
        verify_health(&connection)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| LogDestinationError::Unavailable)?;

        match record_match(&transaction, record)? {
            RecordMatch::Exact => {}
            RecordMatch::Absent => insert_record(&transaction, record)?,
            RecordMatch::Conflicting => return Err(LogDestinationError::IntegrityFailure),
        }

        transaction
            .commit()
            .map_err(|_| LogDestinationError::Unavailable)?;
        Ok(DurableAcknowledgement::for_record(record))
    }
}

enum RecordMatch {
    Absent,
    Exact,
    Conflicting,
}

fn trusted_open_flags() -> OpenFlags {
    OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_NOFOLLOW
}

fn database_is_fresh(database_path: &Path) -> Result<bool, LogDestinationError> {
    match fs::symlink_metadata(database_path) {
        Ok(metadata) if metadata.len() == 0 => Err(LogDestinationError::IntegrityFailure),
        Ok(_) => Ok(false),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(true),
        Err(_) => Err(LogDestinationError::Unavailable),
    }
}

fn reserve_fresh_database(database_path: &Path) -> Result<(), LogDestinationError> {
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(database_path)
        .map(|_| ())
        .map_err(|_| LogDestinationError::Unavailable)
}

fn configure_connection(connection: &Connection) -> Result<(), LogDestinationError> {
    connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(|_| LogDestinationError::Unavailable)?;
    let foreign_keys: i64 = connection
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .map_err(|_| LogDestinationError::Unavailable)?;
    if foreign_keys != 1 {
        return Err(LogDestinationError::ConfigurationInvalid);
    }

    let journal_mode: String = connection
        .pragma_update_and_check(None, "journal_mode", "wal", |row| row.get(0))
        .map_err(|_| LogDestinationError::Unavailable)?;
    if !journal_mode.eq_ignore_ascii_case("wal") {
        return Err(LogDestinationError::ConfigurationInvalid);
    }

    connection
        .pragma_update(None, "synchronous", "FULL")
        .map_err(|_| LogDestinationError::Unavailable)?;
    let synchronous: i64 = connection
        .pragma_query_value(None, "synchronous", |row| row.get(0))
        .map_err(|_| LogDestinationError::Unavailable)?;
    if synchronous != 2 {
        return Err(LogDestinationError::ConfigurationInvalid);
    }

    connection
        .busy_timeout(BUSY_TIMEOUT)
        .map_err(|_| LogDestinationError::Unavailable)?;
    let busy_timeout: i64 = connection
        .pragma_query_value(None, "busy_timeout", |row| row.get(0))
        .map_err(|_| LogDestinationError::Unavailable)?;
    if busy_timeout != BUSY_TIMEOUT_MILLISECONDS {
        return Err(LogDestinationError::ConfigurationInvalid);
    }

    Ok(())
}

fn verify_health(connection: &Connection) -> Result<(), LogDestinationError> {
    let result: i64 = connection
        .query_row("SELECT 1", [], |row| row.get(0))
        .map_err(|_| LogDestinationError::Unavailable)?;
    if result != EXPECTED_HEALTH_RESULT {
        return Err(LogDestinationError::IntegrityFailure);
    }
    Ok(())
}

fn validate_registry() -> Result<(), LogDestinationError> {
    if MIGRATIONS.is_empty() {
        return Err(LogDestinationError::IntegrityFailure);
    }
    for (index, migration) in MIGRATIONS.iter().enumerate() {
        let expected_sequence =
            i64::try_from(index + 1).map_err(|_| LogDestinationError::IntegrityFailure)?;
        let expected_prefix = format!("{expected_sequence:04}_");
        if migration.sequence != expected_sequence
            || !migration.identifier.starts_with(&expected_prefix)
            || migration.sql.is_empty()
        {
            return Err(LogDestinationError::IntegrityFailure);
        }
    }
    Ok(())
}

fn insert_migration_ledger_entry(
    transaction: &Transaction<'_>,
    migration: &Migration,
) -> Result<(), LogDestinationError> {
    transaction
        .execute(
            "INSERT INTO weavelit_log_migration_ledger \
             (sequence_number, identifier, checksum) VALUES (?1, ?2, ?3)",
            params![
                migration.sequence,
                migration.identifier,
                checksum(migration.sql.as_bytes()).as_slice()
            ],
        )
        .map_err(|_| LogDestinationError::IntegrityFailure)?;
    Ok(())
}

fn validate_deployment_binding(
    transaction: &Transaction<'_>,
    deployment_identity: &[u8; 16],
) -> Result<(), LogDestinationError> {
    let bindings = transaction
        .prepare(&format!(
            "SELECT deployment_identity FROM {DEPLOYMENT_TABLE}"
        ))
        .map_err(|_| LogDestinationError::IntegrityFailure)?
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .map_err(|_| LogDestinationError::IntegrityFailure)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| LogDestinationError::IntegrityFailure)?;

    match bindings.as_slice() {
        [binding] if binding.as_slice() == deployment_identity => Ok(()),
        _ => Err(LogDestinationError::IntegrityFailure),
    }
}

fn load_and_validate_applied(transaction: &Transaction<'_>) -> Result<usize, LogDestinationError> {
    let ledger_exists: bool = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1)",
            [LEDGER_TABLE],
            |row| row.get(0),
        )
        .map_err(|_| LogDestinationError::IntegrityFailure)?;
    if !ledger_exists {
        let object_count: i64 = transaction
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
                [],
                |row| row.get(0),
            )
            .map_err(|_| LogDestinationError::IntegrityFailure)?;
        if object_count != 0 {
            return Err(LogDestinationError::IntegrityFailure);
        }
        return Ok(0);
    }

    let mut statement = transaction
        .prepare(
            "SELECT sequence_number, identifier, checksum \
             FROM weavelit_log_migration_ledger ORDER BY sequence_number ASC",
        )
        .map_err(|_| LogDestinationError::IntegrityFailure)?;
    let applied = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })
        .map_err(|_| LogDestinationError::IntegrityFailure)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| LogDestinationError::IntegrityFailure)?;
    if applied.is_empty() || applied.len() > MIGRATIONS.len() {
        return Err(LogDestinationError::IntegrityFailure);
    }
    for ((sequence, identifier, applied_checksum), migration) in applied.iter().zip(MIGRATIONS) {
        if *sequence != migration.sequence
            || identifier != migration.identifier
            || applied_checksum.as_slice() != checksum(migration.sql.as_bytes())
        {
            return Err(LogDestinationError::IntegrityFailure);
        }
    }
    Ok(applied.len())
}

fn validate_schema(
    transaction: &Transaction<'_>,
    applied_migrations: &[Migration],
) -> Result<(), LogDestinationError> {
    let expected = Connection::open_in_memory().map_err(|_| LogDestinationError::Unavailable)?;
    for migration in applied_migrations {
        expected
            .execute_batch(migration.sql)
            .map_err(|_| LogDestinationError::IntegrityFailure)?;
    }
    if schema_objects(transaction)? != schema_objects(&expected)? {
        return Err(LogDestinationError::IntegrityFailure);
    }
    Ok(())
}

fn schema_objects(connection: &Connection) -> Result<Vec<SchemaObject>, LogDestinationError> {
    let mut statement = connection
        .prepare(
            "SELECT type, name, tbl_name, sql FROM sqlite_schema \
             WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name, tbl_name",
        )
        .map_err(|_| LogDestinationError::IntegrityFailure)?;
    statement
        .query_map([], |row| {
            Ok(SchemaObject {
                object_type: row.get(0)?,
                name: row.get(1)?,
                table_name: row.get(2)?,
                sql: row.get(3)?,
            })
        })
        .map_err(|_| LogDestinationError::IntegrityFailure)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| LogDestinationError::IntegrityFailure)
}

fn checksum(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn record_match(
    transaction: &Transaction<'_>,
    record: &CompleteLogRecord,
) -> Result<RecordMatch, LogDestinationError> {
    let record_id = record.record_id().as_bytes().as_slice();
    match record.persistence_view() {
        LogRecordPersistenceView::System(view) => {
            let same_type = transaction
                .query_row(
                    "SELECT event_time_milliseconds, result, correlation_id, classification, detail \
                     FROM weavelit_log_system_records WHERE record_id = ?1",
                    [record_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                        ))
                    },
                )
                .optional()
                .map_err(|_| LogDestinationError::IntegrityFailure)?;
            let other_type = record_exists(transaction, "weavelit_log_audit_records", record_id)?;
            match (same_type, other_type) {
                (Some(_), true) => Ok(RecordMatch::Conflicting),
                (Some(row), false)
                    if row
                        == (
                            view.event_time().unix_milliseconds().to_string(),
                            result_value(view.result()),
                            view.correlation_id().as_str().to_owned(),
                            view.body().classification().to_owned(),
                            view.body().detail().to_owned(),
                        ) =>
                {
                    Ok(RecordMatch::Exact)
                }
                (Some(_), false) => Ok(RecordMatch::Conflicting),
                (None, false) => Ok(RecordMatch::Absent),
                (None, true) => Ok(RecordMatch::Conflicting),
            }
        }
        LogRecordPersistenceView::Audit(view) => {
            let same_type = transaction
                .query_row(
                    "SELECT event_time_milliseconds, result, correlation_id, principal, action, target, detail \
                     FROM weavelit_log_audit_records WHERE record_id = ?1",
                    [record_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, String>(6)?,
                        ))
                    },
                )
                .optional()
                .map_err(|_| LogDestinationError::IntegrityFailure)?;
            let other_type = record_exists(transaction, "weavelit_log_system_records", record_id)?;
            match (same_type, other_type) {
                (Some(_), true) => Ok(RecordMatch::Conflicting),
                (Some(row), false)
                    if row
                        == (
                            view.event_time().unix_milliseconds().to_string(),
                            result_value(view.result()),
                            view.correlation_id().as_str().to_owned(),
                            view.body().principal().to_owned(),
                            view.body().action().to_owned(),
                            view.body().target().to_owned(),
                            view.body().detail().to_owned(),
                        ) =>
                {
                    Ok(RecordMatch::Exact)
                }
                (Some(_), false) => Ok(RecordMatch::Conflicting),
                (None, false) => Ok(RecordMatch::Absent),
                (None, true) => Ok(RecordMatch::Conflicting),
            }
        }
    }
}

fn record_exists(
    transaction: &Transaction<'_>,
    table: &str,
    record_id: &[u8],
) -> Result<bool, LogDestinationError> {
    transaction
        .query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE record_id = ?1)"),
            [record_id],
            |row| row.get(0),
        )
        .map_err(|_| LogDestinationError::IntegrityFailure)
}

fn insert_record(
    transaction: &Transaction<'_>,
    record: &CompleteLogRecord,
) -> Result<(), LogDestinationError> {
    let inserted = match record.persistence_view() {
        LogRecordPersistenceView::System(view) => transaction.execute(
            "INSERT INTO weavelit_log_system_records \
             (record_id, event_time_milliseconds, result, correlation_id, classification, detail) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                view.record_id().as_bytes().as_slice(),
                view.event_time().unix_milliseconds().to_string(),
                result_value(view.result()),
                view.correlation_id().as_str(),
                view.body().classification(),
                view.body().detail(),
            ],
        ),
        LogRecordPersistenceView::Audit(view) => transaction.execute(
            "INSERT INTO weavelit_log_audit_records \
             (record_id, event_time_milliseconds, result, correlation_id, principal, action, target, detail) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                view.record_id().as_bytes().as_slice(),
                view.event_time().unix_milliseconds().to_string(),
                result_value(view.result()),
                view.correlation_id().as_str(),
                view.body().principal(),
                view.body().action(),
                view.body().target(),
                view.body().detail(),
            ],
        ),
    }
    .map_err(|_| LogDestinationError::Unavailable)?;
    if inserted != 1 {
        return Err(LogDestinationError::IntegrityFailure);
    }
    Ok(())
}

fn result_value(result: weavelit_server_log::LogResult) -> i64 {
    match result {
        weavelit_server_log::LogResult::Success => 1,
        weavelit_server_log::LogResult::Failure => 0,
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, io::ErrorKind, time::Duration};

    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    use rusqlite::Connection;
    use weavelit_server_log::{
        AuditLogBody, CompleteLogRecord, CorrelationId, EventTime, LogDestination,
        LogDestinationError, LogModuleCatalog, LogModuleIdentifier, LogResult, SystemLogBody,
        TrustedLogModuleContext, TrustedRecordIssuer,
    };

    use super::{MIGRATIONS, SqliteLogDestination, checksum, registration};

    fn context(
        temporary_directory: &tempfile::TempDir,
        deployment_identity: [u8; 16],
    ) -> TrustedLogModuleContext {
        TrustedLogModuleContext::new(
            temporary_directory.path().canonicalize().unwrap(),
            deployment_identity,
        )
    }

    fn database_path(temporary_directory: &tempfile::TempDir) -> std::path::PathBuf {
        temporary_directory.path().join("log.sqlite3")
    }

    fn database_snapshot(temporary_directory: &tempfile::TempDir) -> Vec<Option<Vec<u8>>> {
        ["log.sqlite3", "log.sqlite3-wal", "log.sqlite3-shm"]
            .into_iter()
            .map(
                |filename| match fs::read(temporary_directory.path().join(filename)) {
                    Ok(contents) => Some(contents),
                    Err(error) if error.kind() == ErrorKind::NotFound => None,
                    Err(error) => panic!("cannot snapshot {filename}: {error}"),
                },
            )
            .collect()
    }

    fn journal_mode(temporary_directory: &tempfile::TempDir) -> String {
        let connection = Connection::open(database_path(temporary_directory)).unwrap();
        connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap()
    }

    fn create_migration_one_destination(
        temporary_directory: &tempfile::TempDir,
        deployment_identity: [u8; 16],
    ) {
        let connection = Connection::open(database_path(temporary_directory)).unwrap();
        connection.execute_batch(MIGRATIONS[0].sql).unwrap();
        connection
            .execute(
                "INSERT INTO weavelit_log_migration_ledger \
                 (sequence_number, identifier, checksum) VALUES (?1, ?2, ?3)",
                rusqlite::params![
                    MIGRATIONS[0].sequence,
                    MIGRATIONS[0].identifier,
                    checksum(MIGRATIONS[0].sql.as_bytes()).as_slice()
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO weavelit_log_deployment_binding (singleton, deployment_identity) \
                 VALUES (1, ?1)",
                [deployment_identity.as_slice()],
            )
            .unwrap();
    }

    fn create_current_destination(
        temporary_directory: &tempfile::TempDir,
        deployment_identity: [u8; 16],
    ) {
        drop(
            SqliteLogDestination::open(&context(temporary_directory, deployment_identity)).unwrap(),
        );
    }

    fn assert_integrity_failure_without_mutation(
        temporary_directory: &tempfile::TempDir,
        context: &TrustedLogModuleContext,
    ) {
        let before = database_snapshot(temporary_directory);
        assert!(matches!(
            SqliteLogDestination::open(context),
            Err(LogDestinationError::IntegrityFailure)
        ));
        assert_eq!(database_snapshot(temporary_directory), before);
        assert_eq!(journal_mode(temporary_directory), "wal");
    }

    fn system_record(record_id: [u8; 16], detail: &str) -> CompleteLogRecord {
        let record_id = TrustedRecordIssuer::new().issue(record_id).unwrap();
        CompleteLogRecord::system(
            record_id,
            EventTime::from_unix_milliseconds(u64::MAX),
            LogResult::Success,
            CorrelationId::new("system-correlation").unwrap(),
            SystemLogBody::new("lifecycle", detail).unwrap(),
        )
        .unwrap()
    }

    fn audit_record(record_id: [u8; 16]) -> CompleteLogRecord {
        let record_id = TrustedRecordIssuer::new().issue(record_id).unwrap();
        CompleteLogRecord::audit(
            record_id,
            EventTime::from_unix_milliseconds(42),
            LogResult::Failure,
            CorrelationId::new("audit-correlation").unwrap(),
            AuditLogBody::new("operator", "init", "deployment", "pre-redacted").unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn first_open_and_reopen_preserve_a_healthy_destination() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let context = context(&temporary_directory, [7; 16]);

        let destination = SqliteLogDestination::open(&context).unwrap();
        drop(destination);
        let reopened = SqliteLogDestination::open(&context).unwrap();
        assert!(database_path(&temporary_directory).exists());
        let connection = reopened.connection.lock().unwrap();
        let journal_mode: String = connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        let synchronous: i64 = connection
            .pragma_query_value(None, "synchronous", |row| row.get(0))
            .unwrap();
        assert_eq!(journal_mode, "wal");
        assert_eq!(synchronous, 2);
    }

    #[cfg(unix)]
    #[test]
    fn database_file_symlink_is_rejected_without_following_target() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let target_directory = tempfile::tempdir().unwrap();
        let target_database = database_path(&target_directory);
        drop(Connection::open(&target_database).unwrap());
        symlink(&target_database, database_path(&temporary_directory)).unwrap();
        let context = context(&temporary_directory, [7; 16]);

        assert!(matches!(
            SqliteLogDestination::open(&context),
            Err(LogDestinationError::Unavailable)
        ));

        let target_connection = Connection::open(target_database).unwrap();
        let schema_objects: i64 = target_connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(schema_objects, 0);
    }

    #[test]
    fn registration_persists_complete_system_and_audit_records_separately() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let context = context(&temporary_directory, [7; 16]);
        let catalog = LogModuleCatalog::new(vec![registration()]).unwrap();
        let identifier = LogModuleIdentifier::new("sqlite").unwrap();
        let destination = catalog.create_destination(&identifier, &context).unwrap();
        let system = system_record([1; 16], "pre-redacted-system-detail");
        let audit = audit_record([2; 16]);

        destination.deliver(&system).unwrap();
        destination.deliver(&audit).unwrap();
        drop(destination);

        let connection = Connection::open(database_path(&temporary_directory)).unwrap();
        let system_row: (String, String) = connection
            .query_row(
                "SELECT event_time_milliseconds, detail FROM weavelit_log_system_records",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let audit_row: (String, String) = connection
            .query_row(
                "SELECT action, detail FROM weavelit_log_audit_records",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            system_row,
            (u64::MAX.to_string(), "pre-redacted-system-detail".into())
        );
        assert_eq!(audit_row, ("init".into(), "pre-redacted".into()));
    }

    #[test]
    fn registration_persists_records_at_the_byte_boundaries() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let context = context(&temporary_directory, [7; 16]);
        let destination = SqliteLogDestination::open(&context).unwrap();
        let issuer = TrustedRecordIssuer::new();
        let system = CompleteLogRecord::system(
            issuer.issue([1; 16]).unwrap(),
            EventTime::from_unix_milliseconds(1),
            LogResult::Success,
            CorrelationId::new("c".repeat(64)).unwrap(),
            SystemLogBody::new("s".repeat(128), "d".repeat(4 * 1024)).unwrap(),
        )
        .unwrap();
        let audit = CompleteLogRecord::audit(
            issuer.issue([2; 16]).unwrap(),
            EventTime::from_unix_milliseconds(1),
            LogResult::Success,
            CorrelationId::new("c".repeat(64)).unwrap(),
            AuditLogBody::new(
                "p".repeat(256),
                "a".repeat(128),
                "t".repeat(1024),
                "d".repeat(4 * 1024),
            )
            .unwrap(),
        )
        .unwrap();

        destination.deliver(&system).unwrap();
        destination.deliver(&audit).unwrap();
        drop(destination);

        let connection = Connection::open(database_path(&temporary_directory)).unwrap();
        let system_lengths: (i64, i64, i64) = connection
            .query_row(
                "SELECT length(CAST(correlation_id AS BLOB)), length(CAST(classification AS BLOB)), \
                 length(CAST(detail AS BLOB)) FROM weavelit_log_system_records",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let audit_lengths: (i64, i64, i64, i64, i64) = connection
            .query_row(
                "SELECT length(CAST(correlation_id AS BLOB)), length(CAST(principal AS BLOB)), \
                 length(CAST(action AS BLOB)), length(CAST(target AS BLOB)), \
                 length(CAST(detail AS BLOB)) FROM weavelit_log_audit_records",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(system_lengths, (64, 128, 4 * 1024));
        assert_eq!(audit_lengths, (64, 256, 128, 1024, 4 * 1024));
    }

    #[test]
    fn schema_rejects_direct_oversized_record_rows() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let context = context(&temporary_directory, [7; 16]);
        let destination = SqliteLogDestination::open(&context).unwrap();
        drop(destination);
        let connection = Connection::open(database_path(&temporary_directory)).unwrap();

        let result = connection.execute(
            "INSERT INTO weavelit_log_system_records \
             (record_id, event_time_milliseconds, result, correlation_id, classification, detail) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                vec![3_u8; 16],
                "1",
                1,
                "correlation",
                "classification",
                "d".repeat(4097)
            ],
        );

        assert!(result.is_err());
    }

    #[test]
    fn migration_fails_closed_when_an_existing_record_exceeds_new_bounds() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let database_path = database_path(&temporary_directory);
        let connection = Connection::open(&database_path).unwrap();
        connection.execute_batch(MIGRATIONS[0].sql).unwrap();
        connection
            .execute(
                "INSERT INTO weavelit_log_migration_ledger \
                 (sequence_number, identifier, checksum) VALUES (?1, ?2, ?3)",
                rusqlite::params![
                    MIGRATIONS[0].sequence,
                    MIGRATIONS[0].identifier,
                    checksum(MIGRATIONS[0].sql.as_bytes()).as_slice()
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO weavelit_log_deployment_binding (singleton, deployment_identity) \
                 VALUES (1, ?1)",
                [[7_u8; 16].as_slice()],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO weavelit_log_system_records \
                 (record_id, event_time_milliseconds, result, correlation_id, classification, detail) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![vec![4_u8; 16], "1", 1, "correlation", "classification", "d".repeat(4097)],
            )
            .unwrap();
        drop(connection);
        let context = context(&temporary_directory, [7; 16]);

        assert!(matches!(
            SqliteLogDestination::open(&context),
            Err(LogDestinationError::IntegrityFailure)
        ));

        let connection = Connection::open(database_path).unwrap();
        let applied_migrations: i64 = connection
            .query_row(
                "SELECT count(*) FROM weavelit_log_migration_ledger",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let persisted_oversized_rows: i64 = connection
            .query_row(
                "SELECT count(*) FROM weavelit_log_system_records",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(applied_migrations, 1);
        assert_eq!(persisted_oversized_rows, 1);
    }

    #[test]
    fn absent_destination_bootstraps_binding_ledger_and_schema() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let context = context(&temporary_directory, [7; 16]);

        drop(SqliteLogDestination::open(&context).unwrap());

        let connection = Connection::open(database_path(&temporary_directory)).unwrap();
        let ledger_entries: Vec<(i64, String, Vec<u8>)> = connection
            .prepare(
                "SELECT sequence_number, identifier, checksum \
                 FROM weavelit_log_migration_ledger ORDER BY sequence_number",
            )
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let binding: Vec<u8> = connection
            .query_row(
                "SELECT deployment_identity FROM weavelit_log_deployment_binding",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(ledger_entries.len(), MIGRATIONS.len());
        for ((sequence, identifier, applied_checksum), migration) in
            ledger_entries.iter().zip(MIGRATIONS)
        {
            assert_eq!(*sequence, migration.sequence);
            assert_eq!(identifier, migration.identifier);
            assert_eq!(
                applied_checksum.as_slice(),
                checksum(migration.sql.as_bytes())
            );
        }
        assert_eq!(binding, vec![7; 16]);
        assert_eq!(journal_mode(&temporary_directory), "wal");
    }

    #[test]
    fn foreign_older_destination_is_rejected_without_mutation() {
        let temporary_directory = tempfile::tempdir().unwrap();
        create_migration_one_destination(&temporary_directory, [8; 16]);
        let before = database_snapshot(&temporary_directory);
        assert_eq!(journal_mode(&temporary_directory), "delete");
        let foreign_context = context(&temporary_directory, [9; 16]);

        assert!(matches!(
            SqliteLogDestination::open(&foreign_context),
            Err(LogDestinationError::IntegrityFailure)
        ));

        assert_eq!(database_snapshot(&temporary_directory), before);
        assert_eq!(journal_mode(&temporary_directory), "delete");
    }

    #[test]
    fn foreign_current_destination_is_rejected_without_mutation() {
        let temporary_directory = tempfile::tempdir().unwrap();
        create_current_destination(&temporary_directory, [8; 16]);
        let foreign_context = context(&temporary_directory, [9; 16]);

        assert_integrity_failure_without_mutation(&temporary_directory, &foreign_context);
    }

    #[test]
    fn preexisting_empty_destination_is_rejected_without_mutation() {
        let temporary_directory = tempfile::tempdir().unwrap();
        fs::write(database_path(&temporary_directory), []).unwrap();
        let before = database_snapshot(&temporary_directory);
        let context = context(&temporary_directory, [7; 16]);

        assert!(matches!(
            SqliteLogDestination::open(&context),
            Err(LogDestinationError::IntegrityFailure)
        ));
        assert_eq!(database_snapshot(&temporary_directory), before);
    }

    #[test]
    fn invalid_existing_artifacts_are_rejected_without_mutation() {
        for (description, mutation) in [
            (
                "missing binding",
                "DELETE FROM weavelit_log_deployment_binding",
            ),
            (
                "mismatched binding",
                "UPDATE weavelit_log_deployment_binding SET deployment_identity = zeroblob(16)",
            ),
            (
                "malformed binding",
                "PRAGMA ignore_check_constraints = ON; \
                 UPDATE weavelit_log_deployment_binding SET deployment_identity = zeroblob(1)",
            ),
            (
                "duplicate binding",
                "CREATE TABLE weavelit_log_deployment_binding_replacement \
                 (singleton INTEGER NOT NULL, deployment_identity BLOB NOT NULL); \
                 INSERT INTO weavelit_log_deployment_binding_replacement \
                 SELECT singleton, deployment_identity FROM weavelit_log_deployment_binding; \
                 INSERT INTO weavelit_log_deployment_binding_replacement VALUES (2, zeroblob(16)); \
                 DROP TABLE weavelit_log_deployment_binding; \
                 ALTER TABLE weavelit_log_deployment_binding_replacement \
                 RENAME TO weavelit_log_deployment_binding",
            ),
            ("missing ledger", "DROP TABLE weavelit_log_migration_ledger"),
            (
                "checksum tampering",
                "UPDATE weavelit_log_migration_ledger SET checksum = zeroblob(32)",
            ),
            (
                "schema tampering",
                "ALTER TABLE weavelit_log_system_records ADD COLUMN unexpected TEXT",
            ),
        ] {
            let temporary_directory = tempfile::tempdir().unwrap();
            create_current_destination(&temporary_directory, [7; 16]);
            let connection = Connection::open(database_path(&temporary_directory)).unwrap();
            connection.execute_batch(mutation).unwrap();
            drop(connection);
            let context = context(&temporary_directory, [7; 16]);

            assert_integrity_failure_without_mutation(&temporary_directory, &context);
            assert_eq!(journal_mode(&temporary_directory), "wal", "{description}");
        }
    }

    #[test]
    fn matching_older_destination_migrates_and_reopens_idempotently() {
        let temporary_directory = tempfile::tempdir().unwrap();
        create_migration_one_destination(&temporary_directory, [7; 16]);
        let context = context(&temporary_directory, [7; 16]);

        drop(SqliteLogDestination::open(&context).unwrap());
        drop(SqliteLogDestination::open(&context).unwrap());

        let connection = Connection::open(database_path(&temporary_directory)).unwrap();
        let applied_migrations: i64 = connection
            .query_row(
                "SELECT count(*) FROM weavelit_log_migration_ledger",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let binding: Vec<u8> = connection
            .query_row(
                "SELECT deployment_identity FROM weavelit_log_deployment_binding",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(applied_migrations, i64::try_from(MIGRATIONS.len()).unwrap());
        assert_eq!(binding, vec![7; 16]);
    }

    #[test]
    fn exact_replay_is_acknowledged_and_changed_replay_is_rejected() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let context = context(&temporary_directory, [7; 16]);
        let destination = SqliteLogDestination::open(&context).unwrap();
        let record = system_record([3; 16], "pre-redacted-detail");

        destination.deliver(&record).unwrap();
        destination.deliver(&record).unwrap();
        assert_eq!(
            destination.deliver(&system_record([3; 16], "changed-detail")),
            Err(LogDestinationError::IntegrityFailure)
        );
        assert_eq!(
            destination.deliver(&audit_record([3; 16])),
            Err(LogDestinationError::IntegrityFailure)
        );
    }

    #[test]
    fn destination_rejects_different_deployment_identity() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let initial_context = context(&temporary_directory, [7; 16]);
        let destination = SqliteLogDestination::open(&initial_context).unwrap();
        drop(destination);

        let different_context = context(&temporary_directory, [8; 16]);
        assert!(matches!(
            SqliteLogDestination::open(&different_context),
            Err(LogDestinationError::IntegrityFailure)
        ));
    }

    #[test]
    fn unavailable_and_corrupt_destinations_return_redacted_errors() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let unavailable_root = temporary_directory.path().join("not-a-directory");
        fs::write(&unavailable_root, "not-a-directory").unwrap();
        let unavailable_context = TrustedLogModuleContext::new(unavailable_root.clone(), [7; 16]);
        let unavailable = match SqliteLogDestination::open(&unavailable_context) {
            Err(error) => error,
            Ok(_) => panic!("a file cannot be a destination root"),
        };
        assert_eq!(unavailable, LogDestinationError::Unavailable);
        assert!(
            !unavailable
                .to_string()
                .contains(unavailable_root.to_str().unwrap())
        );

        let corrupt_directory = tempfile::tempdir().unwrap();
        let corrupt_path = database_path(&corrupt_directory);
        fs::write(&corrupt_path, "not a SQLite database").unwrap();
        let corrupt_context = context(&corrupt_directory, [7; 16]);
        let corrupt = match SqliteLogDestination::open(&corrupt_context) {
            Err(error) => error,
            Ok(_) => panic!("a corrupt SQLite destination must be rejected"),
        };
        assert_eq!(corrupt, LogDestinationError::Unavailable);
        assert!(!corrupt.to_string().contains(corrupt_path.to_str().unwrap()));
    }

    #[test]
    fn locked_destination_refuses_delivery_without_exposing_record_content() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let context = context(&temporary_directory, [7; 16]);
        let destination = SqliteLogDestination::open(&context).unwrap();
        destination
            .connection
            .lock()
            .unwrap()
            .busy_timeout(Duration::ZERO)
            .unwrap();
        let lock = Connection::open(database_path(&temporary_directory)).unwrap();
        lock.execute_batch("BEGIN EXCLUSIVE").unwrap();

        let error = destination
            .deliver(&system_record([4; 16], "secret-record-content"))
            .unwrap_err();
        assert_eq!(error, LogDestinationError::Unavailable);
        assert!(!error.to_string().contains("secret-record-content"));
        lock.execute_batch("ROLLBACK").unwrap();
    }

    #[test]
    fn tampered_migration_ledger_fails_closed() {
        for mutation in [
            "UPDATE weavelit_log_migration_ledger SET checksum = zeroblob(32)",
            "UPDATE weavelit_log_migration_ledger SET sequence_number = 3 WHERE sequence_number = 2",
            "DELETE FROM weavelit_log_migration_ledger",
            "INSERT INTO weavelit_log_migration_ledger (sequence_number, identifier, checksum) \
             VALUES (3, '0003_unknown', zeroblob(32))",
        ] {
            let temporary_directory = tempfile::tempdir().unwrap();
            let context = context(&temporary_directory, [7; 16]);
            let destination = SqliteLogDestination::open(&context).unwrap();
            drop(destination);
            let connection = Connection::open(database_path(&temporary_directory)).unwrap();
            connection.execute_batch(mutation).unwrap();
            drop(connection);

            assert!(
                matches!(
                    SqliteLogDestination::open(&context),
                    Err(LogDestinationError::IntegrityFailure)
                ),
                "mutation {mutation} must be rejected"
            );
        }
    }
}
