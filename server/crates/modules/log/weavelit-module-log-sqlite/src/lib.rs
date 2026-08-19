#![forbid(unsafe_code)]

//! Durable SQLite destination for complete, pre-redacted Server log records.

use std::{fs, io::ErrorKind, path::Path, sync::Mutex, time::Duration};

use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};
use sha2::{Digest, Sha256};
use weavelit_server_log::{
    AuditRecordPhase, CompleteLogRecord, DurableAcknowledgement, LogCapabilities, LogDestination,
    LogDestinationError, LogDestinationFactory, LogModuleFactoryContext, LogModuleRegistration,
    LogRecordPersistenceView, LogRecordType, LogResult, LogSettingsContract,
    TrustedLogModuleContext,
};

/// The canonical identifier this Log Module is compiled in and registered under.
///
/// It is the single source of the Server's compiled-in Log Module inventory, so
/// a Restore cannot be judged against a component name the runtime restated by
/// hand.
pub const MODULE_IDENTIFIER: &str = "sqlite";
const DATABASE_FILENAME: &str = "log.sqlite3";
const DATABASE_SIDECAR_FILENAMES: [&str; 3] =
    ["log.sqlite3-journal", "log.sqlite3-wal", "log.sqlite3-shm"];
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const BUSY_TIMEOUT_MILLISECONDS: i64 = 5_000;
const EXPECTED_HEALTH_RESULT: i64 = 1;
const LEDGER_TABLE: &str = "weavelit_log_migration_ledger";
const DEPLOYMENT_TABLE: &str = "weavelit_log_deployment_binding";

/// The record identifier the preflight probe row is written under.
///
/// `weavelit_server_log::TrustedRecordIssuer` refuses to issue an all-zero
/// identifier, so no real record can ever collide with the probe and the probe
/// can never be mistaken for one.
const PREFLIGHT_PROBE_RECORD_ID: [u8; 16] = [0; 16];

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
    Migration {
        sequence: 3,
        identifier: "0003_add_audit_accountability_schema",
        sql: "ALTER TABLE weavelit_log_audit_records ADD COLUMN classification TEXT;\
              ALTER TABLE weavelit_log_audit_records ADD COLUMN principal_type TEXT;\
              ALTER TABLE weavelit_log_audit_records ADD COLUMN responsible_owner TEXT;",
    },
    Migration {
        sequence: 4,
        identifier: "0004_add_audit_attempt_link",
        sql: "ALTER TABLE weavelit_log_audit_records RENAME TO weavelit_log_audit_records_legacy;\
              CREATE TABLE weavelit_log_audit_records (\
                  record_id BLOB PRIMARY KEY CHECK (length(record_id) = 16),\
                  event_time_milliseconds TEXT NOT NULL,\
                  phase TEXT NOT NULL CHECK (phase IN ('attempt', 'completion', 'correction')),\
                  result INTEGER,\
                  attempt_record_id BLOB CHECK (\
                      (phase = 'attempt' AND result IS NULL AND attempt_record_id IS NULL) OR\
                      (phase = 'completion' AND result IS NOT NULL AND result IN (0, 1) AND (attempt_record_id IS NULL OR length(attempt_record_id) = 16)) OR\
                      (phase = 'correction' AND result IS NOT NULL AND result IN (0, 1) AND attempt_record_id IS NOT NULL AND length(attempt_record_id) = 16)\
                  ),\
                  correlation_id TEXT NOT NULL CHECK (length(CAST(correlation_id AS BLOB)) BETWEEN 1 AND 64),\
                  classification TEXT,\
                  principal TEXT NOT NULL CHECK (length(CAST(principal AS BLOB)) BETWEEN 1 AND 256),\
                  principal_type TEXT,\
                  responsible_owner TEXT,\
                  action TEXT NOT NULL CHECK (length(CAST(action AS BLOB)) BETWEEN 1 AND 128),\
                  target TEXT NOT NULL CHECK (length(CAST(target AS BLOB)) BETWEEN 1 AND 1024),\
                  detail TEXT NOT NULL CHECK (length(CAST(detail AS BLOB)) BETWEEN 1 AND 4096),\
                  CHECK (length(CAST(correlation_id AS BLOB)) + length(CAST(principal AS BLOB)) + length(CAST(action AS BLOB)) + length(CAST(target AS BLOB)) + length(CAST(detail AS BLOB)) <= 8192)\
              );\
              INSERT INTO weavelit_log_audit_records \
                  (record_id, event_time_milliseconds, phase, result, attempt_record_id, correlation_id, classification, principal, principal_type, responsible_owner, action, target, detail) \
                  SELECT record_id, event_time_milliseconds, 'completion', result, NULL, correlation_id, classification, principal, principal_type, responsible_owner, action, target, detail \
                  FROM weavelit_log_audit_records_legacy;\
              DROP TABLE weavelit_log_audit_records_legacy;\
              CREATE INDEX weavelit_log_audit_attempt_record_id_idx \
                  ON weavelit_log_audit_records (attempt_record_id) \
                  WHERE attempt_record_id IS NOT NULL;\
              CREATE TRIGGER weavelit_log_audit_terminal_attempt_guard \
                  BEFORE INSERT ON weavelit_log_audit_records \
                  WHEN NEW.phase IN ('completion', 'correction') AND (\
                      NEW.attempt_record_id IS NULL OR NOT EXISTS (\
                          SELECT 1 FROM weavelit_log_audit_records AS attempt \
                          WHERE attempt.record_id = NEW.attempt_record_id \
                            AND attempt.phase = 'attempt' \
                            AND attempt.correlation_id = NEW.correlation_id\
                      )\
                  ) \
                  BEGIN \
                      SELECT RAISE(ABORT, 'invalid Audit Attempt link');\
                  END;",
    },
];

/// Factory for the compiled-in SQLite Log Module destination.
pub struct SqliteLogDestinationFactory;

/// The non-secret settings this Log Module defines: none.
///
/// The destination is derived entirely from the trusted local root and the
/// deployment identity, so there is nothing to configure. This is the module's
/// single statement of that rule: the catalog publishes it, and the factory
/// refuses a configuration against it.
fn accepted_settings() -> LogSettingsContract {
    LogSettingsContract::none()
}

impl LogDestinationFactory for SqliteLogDestinationFactory {
    fn accepted_settings(&self) -> LogSettingsContract {
        accepted_settings()
    }

    fn create(
        &self,
        context: &LogModuleFactoryContext<'_>,
    ) -> Result<Box<dyn LogDestination>, LogDestinationError> {
        Ok(Box::new(SqliteLogDestination::open_from_factory_context(
            context,
        )?))
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
        Self::open_with_inputs(context.local_root(), context.deployment_identity())
    }

    fn open_from_factory_context(
        context: &LogModuleFactoryContext<'_>,
    ) -> Result<Self, LogDestinationError> {
        // Judged against the same declaration the catalog publishes, so a
        // configuration this module could never serve is refused rather than
        // silently ignored, and a caller can reach that rule without opening
        // anything.
        if !accepted_settings().accepts(context.settings()) {
            return Err(LogDestinationError::ConfigurationInvalid);
        }
        Self::open_with_inputs(context.local_root(), context.deployment_identity())
    }

    fn open_with_inputs(
        local_root: &Path,
        deployment_identity: &[u8; 16],
    ) -> Result<Self, LogDestinationError> {
        validate_registry()?;
        let database_path = local_root.join(DATABASE_FILENAME);
        let fresh = database_is_fresh(local_root, &database_path)?;
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
            destination.bootstrap(deployment_identity)?;
        } else {
            destination.validate_existing(deployment_identity)?;
            destination.configure_connection()?;
        }
        destination.apply_migrations(deployment_identity)?;
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

            apply_migration(&transaction, migration)?;
            transaction
                .commit()
                .map_err(|_| LogDestinationError::Unavailable)?;
        }
    }

    fn deliver_persisted(&self, record: &PersistedLogRecord) -> Result<(), LogDestinationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| LogDestinationError::Unavailable)?;
        verify_health(&connection)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| LogDestinationError::Unavailable)?;

        validate_attempt_link(&transaction, record)?;
        match record_match(&transaction, record)? {
            RecordMatch::Exact => {}
            RecordMatch::Absent => insert_record(&transaction, record)?,
            RecordMatch::Conflicting => return Err(LogDestinationError::IntegrityFailure),
        }

        transaction
            .commit()
            .map_err(|_| LogDestinationError::Unavailable)
    }

    /// Writes and removes a probe row through the exact delivery commit path.
    ///
    /// Delivery inserts into the assigned record table inside an immediate
    /// transaction and commits it. The probe does the same and then deletes the
    /// row in the same transaction, so a destination whose storage is
    /// read-only, out of space, schema-incompatible, or otherwise unable to
    /// reach commit is refused, and no record is left behind either way.
    fn probe_commit_path(&self, record_type: LogRecordType) -> Result<(), LogDestinationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| LogDestinationError::Unavailable)?;
        verify_health(&connection)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| LogDestinationError::Unavailable)?;

        let insert = match record_type {
            LogRecordType::System => transaction.execute(
                "INSERT INTO weavelit_log_system_records \
                    (record_id, event_time_milliseconds, result, correlation_id, classification, detail) \
                    VALUES (?1, '0', 0, 'preflight', 'lifecycle.preflight', 'preflight')",
                params![PREFLIGHT_PROBE_RECORD_ID.as_slice()],
            ),
            LogRecordType::Audit => transaction.execute(
                "INSERT INTO weavelit_log_audit_records \
                    (record_id, event_time_milliseconds, phase, result, attempt_record_id, correlation_id, classification, principal, principal_type, responsible_owner, action, target, detail) \
                    VALUES (?1, '0', 'attempt', NULL, NULL, 'preflight', 'lifecycle.backup.created', 'preflight', 'human', NULL, 'preflight', 'preflight', 'preflight')",
                params![PREFLIGHT_PROBE_RECORD_ID.as_slice()],
            ),
        };
        if insert.map_err(|_| LogDestinationError::IntegrityFailure)? != 1 {
            return Err(LogDestinationError::IntegrityFailure);
        }

        let table = match record_type {
            LogRecordType::System => "weavelit_log_system_records",
            LogRecordType::Audit => "weavelit_log_audit_records",
        };
        let removed = transaction
            .execute(
                &format!("DELETE FROM {table} WHERE record_id = ?1"),
                params![PREFLIGHT_PROBE_RECORD_ID.as_slice()],
            )
            .map_err(|_| LogDestinationError::IntegrityFailure)?;
        if removed != 1 {
            return Err(LogDestinationError::IntegrityFailure);
        }

        transaction
            .commit()
            .map_err(|_| LogDestinationError::Unavailable)
    }
}

impl LogDestination for SqliteLogDestination {
    fn deliver(
        &self,
        record: &CompleteLogRecord,
        acknowledgement: DurableAcknowledgement,
    ) -> Result<DurableAcknowledgement, LogDestinationError> {
        let record = PersistedLogRecord::from(record);
        self.deliver_persisted(&record)?;
        Ok(acknowledgement)
    }

    fn preflight(&self, record_type: LogRecordType) -> Result<(), LogDestinationError> {
        self.probe_commit_path(record_type)
    }
}

#[derive(Clone)]
enum PersistedLogRecord {
    System {
        record_id: [u8; 16],
        event_time: u64,
        result: weavelit_server_log::LogResult,
        correlation_id: Box<str>,
        classification: Box<str>,
        detail: Box<str>,
    },
    Audit {
        record_id: [u8; 16],
        event_time: u64,
        phase: PersistedAuditPhase,
        attempt_record_id: Option<[u8; 16]>,
        correlation_id: Box<str>,
        classification: Box<str>,
        principal: Box<str>,
        principal_type: Box<str>,
        responsible_owner: Option<Box<str>>,
        action: Box<str>,
        target: Box<str>,
        detail: Box<str>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PersistedAuditPhase {
    Attempt,
    Completion(LogResult),
    Correction(LogResult),
}

impl PersistedAuditPhase {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Attempt => "attempt",
            Self::Completion(_) => "completion",
            Self::Correction(_) => "correction",
        }
    }

    const fn result(self) -> Option<LogResult> {
        match self {
            Self::Attempt => None,
            Self::Completion(result) | Self::Correction(result) => Some(result),
        }
    }
}

impl From<&AuditRecordPhase> for PersistedAuditPhase {
    fn from(phase: &AuditRecordPhase) -> Self {
        match phase {
            AuditRecordPhase::Attempt => Self::Attempt,
            AuditRecordPhase::Completion { result, .. } => Self::Completion(*result),
            AuditRecordPhase::Correction { result, .. } => Self::Correction(*result),
        }
    }
}

impl From<&CompleteLogRecord> for PersistedLogRecord {
    fn from(record: &CompleteLogRecord) -> Self {
        match record.persistence_view() {
            LogRecordPersistenceView::System(view) => Self::System {
                record_id: *view.record_id().as_bytes(),
                event_time: view.event_time().unix_milliseconds(),
                result: view.result(),
                correlation_id: view.correlation_id().as_str().into(),
                classification: view.body().classification().into(),
                detail: view.body().detail().into(),
            },
            LogRecordPersistenceView::Audit(view) => Self::Audit {
                record_id: *view.record_id().as_bytes(),
                event_time: view.event_time().unix_milliseconds(),
                phase: view.phase().into(),
                attempt_record_id: view
                    .phase()
                    .attempt_record_id()
                    .map(|record_id| *record_id.as_bytes()),
                correlation_id: view.correlation_id().as_str().into(),
                classification: view.body().classification().into(),
                principal: view.body().principal().into(),
                principal_type: view.body().principal_type().as_str().into(),
                responsible_owner: view.body().responsible_owner().map(Into::into),
                action: view.body().action().into(),
                target: view.body().target().into(),
                detail: view.body().detail().into(),
            },
        }
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

fn database_is_fresh(local_root: &Path, database_path: &Path) -> Result<bool, LogDestinationError> {
    match fs::symlink_metadata(database_path) {
        Ok(metadata) if metadata.len() == 0 => Err(LogDestinationError::IntegrityFailure),
        Ok(_) => Ok(false),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            for sidecar_filename in DATABASE_SIDECAR_FILENAMES {
                match fs::symlink_metadata(local_root.join(sidecar_filename)) {
                    Ok(_) => return Err(LogDestinationError::IntegrityFailure),
                    Err(error) if error.kind() == ErrorKind::NotFound => {}
                    Err(_) => return Err(LogDestinationError::Unavailable),
                }
            }
            Ok(true)
        }
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

fn apply_migration(
    transaction: &Transaction<'_>,
    migration: &Migration,
) -> Result<(), LogDestinationError> {
    transaction
        .execute_batch(migration.sql)
        .map_err(|_| LogDestinationError::IntegrityFailure)?;
    insert_migration_ledger_entry(transaction, migration)
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

fn validate_attempt_link(
    transaction: &Transaction<'_>,
    record: &PersistedLogRecord,
) -> Result<(), LogDestinationError> {
    let PersistedLogRecord::Audit {
        event_time,
        phase,
        attempt_record_id,
        correlation_id,
        ..
    } = record
    else {
        return Ok(());
    };

    if *phase == PersistedAuditPhase::Attempt {
        return if attempt_record_id.is_none() {
            Ok(())
        } else {
            Err(LogDestinationError::IntegrityFailure)
        };
    }
    let attempt_record_id = attempt_record_id.ok_or(LogDestinationError::IntegrityFailure)?;
    let target = transaction
        .query_row(
            "SELECT event_time_milliseconds, phase, correlation_id \
             FROM weavelit_log_audit_records WHERE record_id = ?1",
            [attempt_record_id.as_slice()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|_| LogDestinationError::Unavailable)?;

    let Some((attempt_time, attempt_phase, attempt_correlation)) = target else {
        return Err(LogDestinationError::IntegrityFailure);
    };
    if attempt_phase != "attempt" || attempt_correlation != correlation_id.as_ref() {
        return Err(LogDestinationError::IntegrityFailure);
    }
    let attempt_time = attempt_time
        .parse::<u64>()
        .map_err(|_| LogDestinationError::Unavailable)?;
    if attempt_time > *event_time {
        return Err(LogDestinationError::IntegrityFailure);
    }
    Ok(())
}

fn record_match(
    transaction: &Transaction<'_>,
    record: &PersistedLogRecord,
) -> Result<RecordMatch, LogDestinationError> {
    match record {
        PersistedLogRecord::System {
            record_id,
            event_time,
            result,
            correlation_id,
            classification,
            detail,
        } => {
            let record_id = record_id.as_slice();
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
                .map_err(|_| LogDestinationError::Unavailable)?;
            let other_type = record_exists(transaction, "weavelit_log_audit_records", record_id)?;
            match (same_type, other_type) {
                (Some(_), true) => Ok(RecordMatch::Conflicting),
                (Some(row), false)
                    if row
                        == (
                            event_time.to_string(),
                            result_value(*result),
                            correlation_id.to_string(),
                            classification.to_string(),
                            detail.to_string(),
                        ) =>
                {
                    Ok(RecordMatch::Exact)
                }
                (Some(_), false) => Ok(RecordMatch::Conflicting),
                (None, false) => Ok(RecordMatch::Absent),
                (None, true) => Ok(RecordMatch::Conflicting),
            }
        }
        PersistedLogRecord::Audit {
            record_id,
            event_time,
            phase,
            attempt_record_id,
            correlation_id,
            classification,
            principal,
            principal_type,
            responsible_owner,
            action,
            target,
            detail,
        } => {
            let record_id = record_id.as_slice();
            let same_type = transaction
                .query_row(
                    "SELECT event_time_milliseconds, phase, result, attempt_record_id, correlation_id, classification, principal, principal_type, responsible_owner, action, target, detail \
                     FROM weavelit_log_audit_records WHERE record_id = ?1",
                    [record_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<i64>>(2)?,
                            row.get::<_, Option<Vec<u8>>>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, String>(6)?,
                            row.get::<_, String>(7)?,
                            row.get::<_, Option<String>>(8)?,
                            row.get::<_, String>(9)?,
                            row.get::<_, String>(10)?,
                            row.get::<_, String>(11)?,
                        ))
                    },
                )
                .optional()
                .map_err(|_| LogDestinationError::Unavailable)?;
            let other_type = record_exists(transaction, "weavelit_log_system_records", record_id)?;
            match (same_type, other_type) {
                (Some(_), true) => Ok(RecordMatch::Conflicting),
                (Some(row), false)
                    if row
                        == (
                            event_time.to_string(),
                            phase.as_str().to_owned(),
                            phase.result().map(result_value),
                            attempt_record_id.map(|record_id| record_id.to_vec()),
                            correlation_id.to_string(),
                            classification.to_string(),
                            principal.to_string(),
                            principal_type.to_string(),
                            responsible_owner.as_deref().map(str::to_owned),
                            action.to_string(),
                            target.to_string(),
                            detail.to_string(),
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
        .map_err(|_| LogDestinationError::Unavailable)
}

fn insert_record(
    transaction: &Transaction<'_>,
    record: &PersistedLogRecord,
) -> Result<(), LogDestinationError> {
    let inserted = match record {
        PersistedLogRecord::System {
            record_id,
            event_time,
            result,
            correlation_id,
            classification,
            detail,
        } => transaction.execute(
            "INSERT INTO weavelit_log_system_records \
             (record_id, event_time_milliseconds, result, correlation_id, classification, detail) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record_id.as_slice(),
                event_time.to_string(),
                result_value(*result),
                correlation_id.as_ref(),
                classification.as_ref(),
                detail.as_ref(),
            ],
        ),
        PersistedLogRecord::Audit {
            record_id,
            event_time,
            phase,
            attempt_record_id,
            correlation_id,
            classification,
            principal,
            principal_type,
            responsible_owner,
            action,
            target,
            detail,
        } => transaction.execute(
            "INSERT INTO weavelit_log_audit_records \
             (record_id, event_time_milliseconds, phase, result, attempt_record_id, correlation_id, classification, principal, principal_type, responsible_owner, action, target, detail) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                record_id.as_slice(),
                event_time.to_string(),
                phase.as_str(),
                phase.result().map(result_value),
                attempt_record_id.as_ref().map(<[u8; 16]>::as_slice),
                correlation_id.as_ref(),
                classification.as_ref(),
                principal.as_ref(),
                principal_type.as_ref(),
                responsible_owner.as_deref(),
                action.as_ref(),
                target.as_ref(),
                detail.as_ref(),
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

    use rusqlite::{Connection, TransactionBehavior};
    use weavelit_server_log::{
        DestinationSettings, LogDestination, LogDestinationError, LogDestinationFactory,
        LogRecordType, LogResult,
    };

    use super::{
        MIGRATIONS, Migration, PersistedAuditPhase, PersistedLogRecord,
        SqliteLogDestination as ProductionDestination, SqliteLogDestinationFactory,
        apply_migration, checksum,
    };

    type LegacyAuditFields = (
        String,
        i64,
        Option<Vec<u8>>,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        String,
        String,
        String,
    );
    type RecordMutation = (&'static str, fn(&mut PersistedLogRecord));
    type PersistedAuditRow = (
        String,
        Option<i64>,
        Option<Vec<u8>>,
        String,
        String,
        String,
        Option<String>,
        String,
        String,
    );

    struct TestDestinationInputs {
        local_root: std::path::PathBuf,
        deployment_identity: [u8; 16],
    }

    impl TestDestinationInputs {
        fn new(local_root: std::path::PathBuf, deployment_identity: [u8; 16]) -> Self {
            Self {
                local_root,
                deployment_identity,
            }
        }

        fn local_root(&self) -> &std::path::Path {
            &self.local_root
        }
    }

    struct SqliteLogDestination;

    impl SqliteLogDestination {
        fn open(
            context: &TestDestinationInputs,
        ) -> Result<ProductionDestination, LogDestinationError> {
            ProductionDestination::open_with_inputs(
                context.local_root(),
                &context.deployment_identity,
            )
        }
    }

    fn context(
        temporary_directory: &tempfile::TempDir,
        deployment_identity: [u8; 16],
    ) -> TestDestinationInputs {
        TestDestinationInputs::new(
            temporary_directory.path().canonicalize().unwrap(),
            deployment_identity,
        )
    }

    fn open(context: &TestDestinationInputs) -> Result<ProductionDestination, LogDestinationError> {
        SqliteLogDestination::open(context)
    }

    /// The declaration the catalog publishes is the rule `create` enforces.
    ///
    /// Both read `super::accepted_settings`, so a configuration can be judged
    /// before any destination exists and cannot be judged by a rule this module
    /// does not apply when it opens.
    #[test]
    fn the_module_declares_that_it_accepts_no_setting() {
        let declared = SqliteLogDestinationFactory.accepted_settings();

        assert_eq!(declared.keys().len(), 0);
        assert!(!declared.defines("retention-days"));
        assert!(declared.accepts(&DestinationSettings::default()));
        assert!(
            !declared.accepts(
                &DestinationSettings::new(vec![("retention-days".to_owned(), "30".to_owned())])
                    .expect("bounded settings")
            )
        );
    }

    fn database_path(temporary_directory: &tempfile::TempDir) -> std::path::PathBuf {
        temporary_directory.path().join("log.sqlite3")
    }

    #[cfg(target_os = "linux")]
    fn descriptor_context(
        root: &std::path::Path,
        deployment_identity: [u8; 16],
    ) -> (fs::File, TestDestinationInputs) {
        use std::os::fd::AsRawFd;

        let descriptor = fs::File::open(root).unwrap();
        let descriptor_root =
            std::path::PathBuf::from(format!("/proc/self/fd/{}", descriptor.as_raw_fd()));
        (
            descriptor,
            TestDestinationInputs::new(descriptor_root, deployment_identity),
        )
    }

    #[cfg(target_os = "linux")]
    fn replace_root(root: &std::path::Path) -> std::path::PathBuf {
        let relocated_root = root.with_file_name("relocated-state-root");
        fs::rename(root, &relocated_root).unwrap();
        fs::create_dir(root).unwrap();
        relocated_root
    }

    #[cfg(target_os = "linux")]
    fn assert_no_sqlite_artifacts(root: &std::path::Path) {
        for filename in [
            "log.sqlite3",
            "log.sqlite3-journal",
            "log.sqlite3-wal",
            "log.sqlite3-shm",
        ] {
            assert!(
                !root.join(filename).exists(),
                "replacement root unexpectedly contains {filename}"
            );
        }
    }

    fn database_snapshot(temporary_directory: &tempfile::TempDir) -> Vec<Option<Vec<u8>>> {
        database_snapshot_at(temporary_directory.path())
    }

    fn database_snapshot_at(root: &std::path::Path) -> Vec<Option<Vec<u8>>> {
        [
            "log.sqlite3",
            "log.sqlite3-journal",
            "log.sqlite3-wal",
            "log.sqlite3-shm",
        ]
        .into_iter()
        .map(|filename| match fs::read(root.join(filename)) {
            Ok(contents) => Some(contents),
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(error) => panic!("cannot snapshot {filename}: {error}"),
        })
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

    fn create_migration_two_destination(
        temporary_directory: &tempfile::TempDir,
        deployment_identity: [u8; 16],
    ) {
        create_migration_one_destination(temporary_directory, deployment_identity);
        let connection = Connection::open(database_path(temporary_directory)).unwrap();
        connection.execute_batch(MIGRATIONS[1].sql).unwrap();
        connection
            .execute(
                "INSERT INTO weavelit_log_migration_ledger \
                 (sequence_number, identifier, checksum) VALUES (?1, ?2, ?3)",
                rusqlite::params![
                    MIGRATIONS[1].sequence,
                    MIGRATIONS[1].identifier,
                    checksum(MIGRATIONS[1].sql.as_bytes()).as_slice()
                ],
            )
            .unwrap();
    }

    fn create_migration_three_destination(
        temporary_directory: &tempfile::TempDir,
        deployment_identity: [u8; 16],
    ) {
        create_migration_two_destination(temporary_directory, deployment_identity);
        let connection = Connection::open(database_path(temporary_directory)).unwrap();
        connection.execute_batch(MIGRATIONS[2].sql).unwrap();
        connection
            .execute(
                "INSERT INTO weavelit_log_migration_ledger \
                 (sequence_number, identifier, checksum) VALUES (?1, ?2, ?3)",
                rusqlite::params![
                    MIGRATIONS[2].sequence,
                    MIGRATIONS[2].identifier,
                    checksum(MIGRATIONS[2].sql.as_bytes()).as_slice()
                ],
            )
            .unwrap();
    }

    fn create_current_destination(
        temporary_directory: &tempfile::TempDir,
        deployment_identity: [u8; 16],
    ) {
        drop(open(&context(temporary_directory, deployment_identity)).unwrap());
    }

    fn assert_integrity_failure_without_mutation(
        temporary_directory: &tempfile::TempDir,
        context: &TestDestinationInputs,
    ) {
        let before = database_snapshot(temporary_directory);
        assert!(matches!(
            open(context),
            Err(LogDestinationError::IntegrityFailure)
        ));
        assert_eq!(database_snapshot(temporary_directory), before);
        assert_eq!(journal_mode(temporary_directory), "wal");
    }

    fn system_record(record_id: [u8; 16], detail: &str) -> PersistedLogRecord {
        PersistedLogRecord::System {
            record_id,
            event_time: u64::MAX,
            result: LogResult::Success,
            correlation_id: "system-correlation".into(),
            classification: "lifecycle".into(),
            detail: detail.into(),
        }
    }

    fn audit_attempt_record(record_id: [u8; 16]) -> PersistedLogRecord {
        audit_record_with(
            record_id,
            42,
            PersistedAuditPhase::Attempt,
            None,
            "audit-correlation",
            "lifecycle.backup.created",
            "human",
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn audit_record_with(
        record_id: [u8; 16],
        event_time: u64,
        phase: PersistedAuditPhase,
        attempt_record_id: Option<[u8; 16]>,
        correlation_id: &str,
        classification: &str,
        principal_type: &str,
        responsible_owner: Option<&str>,
    ) -> PersistedLogRecord {
        PersistedLogRecord::Audit {
            record_id,
            event_time,
            phase,
            attempt_record_id,
            correlation_id: correlation_id.into(),
            classification: classification.into(),
            principal: "operator".into(),
            principal_type: principal_type.into(),
            responsible_owner: responsible_owner.map(Into::into),
            action: "init".into(),
            target: "deployment".into(),
            detail: "pre-redacted".into(),
        }
    }

    fn audit_schema_columns(connection: &Connection) -> Vec<String> {
        connection
            .prepare("PRAGMA table_info(weavelit_log_audit_records)")
            .unwrap()
            .query_map([], |row| row.get(1))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn deliver(
        destination: &ProductionDestination,
        record: &PersistedLogRecord,
    ) -> Result<(), LogDestinationError> {
        destination.deliver_persisted(record)
    }

    #[test]
    fn preflight_proves_both_commit_paths_and_leaves_no_record() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let context = context(&temporary_directory, [7; 16]);
        let destination = SqliteLogDestination::open(&context).unwrap();

        assert_eq!(destination.preflight(LogRecordType::System), Ok(()));
        assert_eq!(destination.preflight(LogRecordType::Audit), Ok(()));

        let connection = destination.connection.lock().unwrap();
        for table in ["weavelit_log_system_records", "weavelit_log_audit_records"] {
            let remaining: i64 = connection
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(remaining, 0, "{table} retained a preflight probe row");
        }
    }

    #[test]
    fn preflight_refuses_a_destination_whose_commit_path_is_unreachable() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let context = context(&temporary_directory, [7; 16]);
        let destination = SqliteLogDestination::open(&context).unwrap();
        destination
            .connection
            .lock()
            .unwrap()
            .execute_batch("DROP TABLE weavelit_log_audit_records")
            .unwrap();

        assert_eq!(
            destination.preflight(LogRecordType::Audit),
            Err(LogDestinationError::IntegrityFailure)
        );
        assert_eq!(destination.preflight(LogRecordType::System), Ok(()));
    }

    #[test]
    fn first_open_and_reopen_preserve_a_healthy_destination() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let context = context(&temporary_directory, [7; 16]);

        let destination = SqliteLogDestination::open(&context).unwrap();
        deliver(&destination, &system_record([1; 16], "before-reopen")).unwrap();
        assert!(temporary_directory.path().join("log.sqlite3-wal").exists());
        assert!(temporary_directory.path().join("log.sqlite3-shm").exists());

        let reopened = SqliteLogDestination::open(&context).unwrap();
        assert!(database_path(&temporary_directory).exists());
        deliver(&reopened, &system_record([2; 16], "after-reopen")).unwrap();
        let connection = reopened.connection.lock().unwrap();
        let journal_mode: String = connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        let synchronous: i64 = connection
            .pragma_query_value(None, "synchronous", |row| row.get(0))
            .unwrap();
        assert_eq!(journal_mode, "wal");
        assert_eq!(synchronous, 2);
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
        let persisted_records: i64 = connection
            .query_row(
                "SELECT count(*) FROM weavelit_log_system_records",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(applied_migrations, MIGRATIONS.len() as i64);
        assert_eq!(binding, vec![7; 16]);
        assert_eq!(persisted_records, 2);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn descriptor_relative_fresh_destination_fails_closed_after_root_replacement() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let original_root = temporary_directory.path().join("state-root");
        fs::create_dir(&original_root).unwrap();
        let (_root_descriptor, context) = descriptor_context(&original_root, [7; 16]);
        let relocated_root = replace_root(&original_root);

        let error = match SqliteLogDestination::open(&context) {
            Err(error) => error,
            Ok(_) => panic!("descriptor-relative destination must fail closed"),
        };
        assert_eq!(error, LogDestinationError::Unavailable);
        assert_eq!(error.to_string(), "log destination is unavailable");
        assert!(!error.to_string().contains("state-root"));
        let sqlite_error = match Connection::open_with_flags(
            context.local_root().join("log.sqlite3"),
            super::trusted_open_flags(),
        ) {
            Err(error) => error,
            Ok(_) => panic!("SQLite must reject the descriptor-relative NOFOLLOW path"),
        };
        assert!(matches!(
            sqlite_error,
            rusqlite::Error::SqliteFailure(ref error, _)
                if error.extended_code == rusqlite::ffi::SQLITE_CANTOPEN_SYMLINK
        ));
        assert_eq!(
            fs::metadata(relocated_root.join("log.sqlite3"))
                .unwrap()
                .len(),
            0
        );
        for filename in ["log.sqlite3-journal", "log.sqlite3-wal", "log.sqlite3-shm"] {
            assert!(!relocated_root.join(filename).exists(), "{filename}");
        }
        assert_no_sqlite_artifacts(&original_root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn descriptor_relative_existing_destination_reopen_fails_without_mutation() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let original_root = temporary_directory.path().join("state-root");
        fs::create_dir(&original_root).unwrap();
        let initial_context = TestDestinationInputs::new(original_root.clone(), [7; 16]);
        let destination = SqliteLogDestination::open(&initial_context).unwrap();
        deliver(
            &destination,
            &system_record([1; 16], "existing-destination"),
        )
        .unwrap();
        drop(destination);

        let (_root_descriptor, context) = descriptor_context(&original_root, [7; 16]);
        let relocated_root = replace_root(&original_root);
        let before = database_snapshot_at(&relocated_root);

        assert!(matches!(
            SqliteLogDestination::open(&context),
            Err(LogDestinationError::Unavailable)
        ));
        assert_eq!(database_snapshot_at(&relocated_root), before);
        assert_no_sqlite_artifacts(&original_root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn descriptor_relative_orphan_sidecar_preflight_uses_the_held_root() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let original_root = temporary_directory.path().join("state-root");
        fs::create_dir(&original_root).unwrap();
        fs::write(original_root.join("log.sqlite3-wal"), b"orphaned-wal").unwrap();
        let (_root_descriptor, context) = descriptor_context(&original_root, [7; 16]);
        let relocated_root = replace_root(&original_root);
        let before = database_snapshot_at(&relocated_root);

        let error = match SqliteLogDestination::open(&context) {
            Err(error) => error,
            Ok(_) => panic!("an orphaned sidecar must fail closed"),
        };
        assert_eq!(error, LogDestinationError::IntegrityFailure);
        assert_eq!(
            error.to_string(),
            "log destination integrity validation failed"
        );
        assert_eq!(database_snapshot_at(&relocated_root), before);
        assert_no_sqlite_artifacts(&original_root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn descriptor_relative_final_component_symlink_is_rejected_without_following_target() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let original_root = temporary_directory.path().join("state-root");
        let target_database = temporary_directory.path().join("target.sqlite3");
        fs::create_dir(&original_root).unwrap();
        fs::write(&target_database, "target-marker").unwrap();
        symlink(&target_database, original_root.join("log.sqlite3")).unwrap();
        let (_root_descriptor, context) = descriptor_context(&original_root, [7; 16]);
        let _relocated_root = replace_root(&original_root);

        assert!(matches!(
            SqliteLogDestination::open(&context),
            Err(LogDestinationError::Unavailable)
        ));
        assert_eq!(fs::read(&target_database).unwrap(), b"target-marker");
        assert_no_sqlite_artifacts(&original_root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn unavailable_descriptor_root_returns_a_payload_free_error() {
        let unavailable_root =
            std::path::PathBuf::from("/proc/self/fd/2147483647/descriptor-root-secret");
        let context = TestDestinationInputs::new(unavailable_root.clone(), [7; 16]);

        let error = match SqliteLogDestination::open(&context) {
            Err(error) => error,
            Ok(_) => panic!("an unavailable descriptor root must be rejected"),
        };
        assert_eq!(error, LogDestinationError::Unavailable);
        assert_eq!(error.to_string(), "log destination is unavailable");
        assert!(!error.to_string().contains("descriptor-root-secret"));
        assert!(
            !error
                .to_string()
                .contains(unavailable_root.to_str().unwrap())
        );
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
    fn destination_persists_complete_system_and_audit_records_separately() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let context = context(&temporary_directory, [7; 16]);
        let destination = open(&context).unwrap();
        let system = system_record([1; 16], "pre-redacted-system-detail");
        let audit = audit_attempt_record([2; 16]);

        deliver(&destination, &system).unwrap();
        deliver(&destination, &audit).unwrap();
        drop(destination);

        let connection = Connection::open(database_path(&temporary_directory)).unwrap();
        let system_row: (String, String) = connection
            .query_row(
                "SELECT event_time_milliseconds, detail FROM weavelit_log_system_records",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let audit_row: PersistedAuditRow = connection
            .query_row(
                "SELECT phase, result, attempt_record_id, classification, principal, principal_type, responsible_owner, action, detail \
                 FROM weavelit_log_audit_records",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            system_row,
            (u64::MAX.to_string(), "pre-redacted-system-detail".into())
        );
        assert_eq!(
            audit_row,
            (
                "attempt".into(),
                None,
                None,
                "lifecycle.backup.created".into(),
                "operator".into(),
                "human".into(),
                None,
                "init".into(),
                "pre-redacted".into(),
            )
        );
    }

    #[test]
    fn destination_persists_records_at_the_byte_boundaries() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let context = context(&temporary_directory, [7; 16]);
        let destination = open(&context).unwrap();
        let system = PersistedLogRecord::System {
            record_id: [1; 16],
            event_time: 1,
            result: LogResult::Success,
            correlation_id: "c".repeat(64).into(),
            classification: "s".repeat(128).into(),
            detail: "d".repeat(4 * 1024).into(),
        };
        let audit = PersistedLogRecord::Audit {
            record_id: [2; 16],
            event_time: 1,
            phase: PersistedAuditPhase::Attempt,
            attempt_record_id: None,
            correlation_id: "c".repeat(64).into(),
            classification: "c".repeat(128).into(),
            principal: "p".repeat(256).into(),
            principal_type: "automation".into(),
            responsible_owner: Some("o".repeat(256).into()),
            action: "a".repeat(128).into(),
            target: "t".repeat(1024).into(),
            detail: "d".repeat(4 * 1024).into(),
        };

        deliver(&destination, &system).unwrap();
        deliver(&destination, &audit).unwrap();
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
        let audit_lengths: (i64, i64, i64, i64, i64, i64, i64, i64) = connection
            .query_row(
                "SELECT length(CAST(correlation_id AS BLOB)), length(CAST(classification AS BLOB)), \
                 length(CAST(principal AS BLOB)), length(CAST(principal_type AS BLOB)), \
                 length(CAST(responsible_owner AS BLOB)), length(CAST(action AS BLOB)), \
                 length(CAST(target AS BLOB)), length(CAST(detail AS BLOB)) \
                 FROM weavelit_log_audit_records",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(system_lengths, (64, 128, 4 * 1024));
        assert_eq!(
            audit_lengths,
            (
                64,
                128,
                256,
                i64::try_from("automation".len()).unwrap(),
                256,
                128,
                1024,
                4 * 1024
            )
        );
    }

    #[test]
    fn destination_persists_every_valid_audit_phase_and_result_combination() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let destination = open(&context(&temporary_directory, [7; 16])).unwrap();
        let attempt_record_id = [10; 16];
        let attempt = audit_record_with(
            attempt_record_id,
            40,
            PersistedAuditPhase::Attempt,
            None,
            "audit-correlation",
            "internal.log-policy.changed",
            "human",
            None,
        );
        deliver(&destination, &attempt).unwrap();
        let phases = [
            PersistedAuditPhase::Completion(LogResult::Success),
            PersistedAuditPhase::Completion(LogResult::Failure),
            PersistedAuditPhase::Correction(LogResult::Success),
            PersistedAuditPhase::Correction(LogResult::Failure),
        ];

        for (index, phase) in phases.into_iter().enumerate() {
            let record_id = [u8::try_from(index + 11).unwrap(); 16];
            deliver(
                &destination,
                &audit_record_with(
                    record_id,
                    42,
                    phase,
                    Some(attempt_record_id),
                    "audit-correlation",
                    "internal.log-policy.changed",
                    "human",
                    None,
                ),
            )
            .unwrap();
            let persisted: (String, Option<i64>, Option<Vec<u8>>) = destination
                .connection
                .lock()
                .unwrap()
                .query_row(
                    "SELECT phase, result, attempt_record_id FROM weavelit_log_audit_records WHERE record_id = ?1",
                    [record_id.as_slice()],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
            assert_eq!(
                persisted,
                (
                    phase.as_str().into(),
                    phase.result().map(super::result_value),
                    Some(attempt_record_id.to_vec()),
                )
            );
        }
    }

    #[test]
    fn schema_rejects_every_invalid_audit_phase_and_result_combination() {
        let temporary_directory = tempfile::tempdir().unwrap();
        drop(open(&context(&temporary_directory, [7; 16])).unwrap());
        let connection = Connection::open(database_path(&temporary_directory)).unwrap();
        let attempt_record_id = vec![0x19_u8; 16];
        connection
            .execute(
                "INSERT INTO weavelit_log_audit_records \
                 (record_id, event_time_milliseconds, phase, result, attempt_record_id, correlation_id, classification, principal, principal_type, responsible_owner, action, target, detail) \
                 VALUES (?1, '1', 'attempt', NULL, NULL, 'correlation', 'internal.log-policy.changed', 'principal', 'human', NULL, 'action', 'target', 'detail')",
                [attempt_record_id.as_slice()],
            )
            .unwrap();

        for (index, (phase, result, attempt_record_id)) in [
            ("attempt", Some(0_i64), None),
            ("attempt", Some(1_i64), None),
            ("attempt", None, Some(attempt_record_id.clone())),
            ("completion", None, Some(attempt_record_id.clone())),
            ("correction", None, Some(attempt_record_id.clone())),
            ("completion", Some(2_i64), Some(attempt_record_id.clone())),
            ("correction", Some(2_i64), Some(attempt_record_id.clone())),
            ("unknown", None, None),
        ]
        .into_iter()
        .enumerate()
        {
            let inserted = connection.execute(
                "INSERT INTO weavelit_log_audit_records \
                 (record_id, event_time_milliseconds, phase, result, attempt_record_id, correlation_id, classification, principal, principal_type, responsible_owner, action, target, detail) \
                 VALUES (?1, '1', ?2, ?3, ?4, 'correlation', 'internal.log-policy.changed', 'principal', 'human', NULL, 'action', 'target', 'detail')",
                rusqlite::params![
                    vec![u8::try_from(index + 20).unwrap(); 16],
                    phase,
                    result,
                    attempt_record_id,
                ],
            );
            assert!(
                matches!(
                    inserted,
                    Err(rusqlite::Error::SqliteFailure(error, _))
                        if error.code == rusqlite::ErrorCode::ConstraintViolation
                            && error.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_CHECK
                ),
                "invalid {phase}/{result:?} did not return SQLITE_CONSTRAINT_CHECK"
            );
        }
    }

    #[test]
    fn schema_rejects_a_malformed_terminal_attempt_link() {
        let temporary_directory = tempfile::tempdir().unwrap();
        drop(open(&context(&temporary_directory, [7; 16])).unwrap());
        let connection = Connection::open(database_path(&temporary_directory)).unwrap();

        let inserted = connection.execute(
            "INSERT INTO weavelit_log_audit_records \
             (record_id, event_time_milliseconds, phase, result, attempt_record_id, correlation_id, classification, principal, principal_type, responsible_owner, action, target, detail) \
             VALUES (?1, '1', 'correction', 1, ?2, 'correlation', 'internal.log-policy.changed', 'principal', 'human', NULL, 'action', 'target', 'detail')",
            rusqlite::params![vec![0x30_u8; 16], vec![0x19_u8; 15]],
        );

        assert!(matches!(
            inserted,
            Err(rusqlite::Error::SqliteFailure(error, _))
                if error.code == rusqlite::ErrorCode::ConstraintViolation
        ));
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
    fn orphaned_sqlite_sidecars_are_rejected_without_mutation() {
        for (filename, contents) in [
            ("log.sqlite3-journal", b"orphaned-journal".as_slice()),
            ("log.sqlite3-wal", b"orphaned-wal".as_slice()),
            ("log.sqlite3-shm", b"orphaned-shm".as_slice()),
        ] {
            let temporary_directory = tempfile::tempdir().unwrap();
            fs::write(temporary_directory.path().join(filename), contents).unwrap();
            let before = database_snapshot(&temporary_directory);
            let context = context(&temporary_directory, [7; 16]);

            assert!(matches!(
                SqliteLogDestination::open(&context),
                Err(LogDestinationError::IntegrityFailure)
            ));
            assert!(!database_path(&temporary_directory).exists());
            assert_eq!(
                database_snapshot(&temporary_directory),
                before,
                "{filename}"
            );
        }
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
    fn populated_migration_two_destination_upgrades_audit_schema_without_rewriting_legacy_rows() {
        let temporary_directory = tempfile::tempdir().unwrap();
        create_migration_two_destination(&temporary_directory, [7; 16]);
        let mut connection = Connection::open(database_path(&temporary_directory)).unwrap();
        connection
            .execute(
                "INSERT INTO weavelit_log_audit_records \
                 (record_id, event_time_milliseconds, result, correlation_id, principal, action, target, detail) \
                 VALUES (?1, '42', 0, 'legacy-correlation', 'legacy-principal', 'legacy-action', 'legacy-target', 'legacy-detail')",
                rusqlite::params![vec![0x31_u8; 16]],
            )
            .unwrap();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();

        apply_migration(&transaction, &MIGRATIONS[2]).unwrap();
        transaction.commit().unwrap();

        let legacy_fields: (Option<String>, Option<String>, Option<String>) = connection
            .query_row(
                "SELECT classification, principal_type, responsible_owner \
                 FROM weavelit_log_audit_records WHERE record_id = ?1",
                rusqlite::params![vec![0x31_u8; 16]],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let ledger: Vec<(i64, String)> = connection
            .prepare(
                "SELECT sequence_number, identifier FROM weavelit_log_migration_ledger \
                 ORDER BY sequence_number",
            )
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        assert_eq!(legacy_fields, (None, None, None));
        assert_eq!(
            ledger,
            vec![
                (1, "0001_create_log_destination_schema".into()),
                (2, "0002_bound_record_payloads".into()),
                (3, "0003_add_audit_accountability_schema".into()),
            ]
        );
    }

    #[test]
    fn failed_audit_accountability_migration_leaves_no_partial_schema_or_ledger_entry() {
        let temporary_directory = tempfile::tempdir().unwrap();
        create_migration_two_destination(&temporary_directory, [7; 16]);
        let mut connection = Connection::open(database_path(&temporary_directory)).unwrap();
        let before_columns = audit_schema_columns(&connection);
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let failing_migration = Migration {
            sequence: MIGRATIONS[2].sequence,
            identifier: MIGRATIONS[2].identifier,
            sql: "ALTER TABLE weavelit_log_audit_records ADD COLUMN classification TEXT; \
                  THIS IS NOT VALID SQL;",
        };

        assert_eq!(
            apply_migration(&transaction, &failing_migration),
            Err(LogDestinationError::IntegrityFailure)
        );
        drop(transaction);

        assert_eq!(audit_schema_columns(&connection), before_columns);
        let ledger_entries: i64 = connection
            .query_row(
                "SELECT count(*) FROM weavelit_log_migration_ledger",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(ledger_entries, 2);
    }

    #[test]
    fn migration_four_maps_legacy_rows_to_unlinked_completions_without_losing_fields() {
        let temporary_directory = tempfile::tempdir().unwrap();
        create_migration_three_destination(&temporary_directory, [7; 16]);
        let connection = Connection::open(database_path(&temporary_directory)).unwrap();
        for (record_id, result, classification, principal_type, responsible_owner) in [
            (0x32_u8, 0_i64, None, None, None),
            (
                0x33_u8,
                1_i64,
                Some("internal.log-policy.changed"),
                Some("automation"),
                Some("administrator"),
            ),
        ] {
            connection
                .execute(
                "INSERT INTO weavelit_log_audit_records \
                     (record_id, event_time_milliseconds, result, correlation_id, classification, principal, principal_type, responsible_owner, action, target, detail) \
                     VALUES (?1, '42', ?2, 'legacy-correlation', ?3, 'legacy-principal', ?4, ?5, 'legacy-action', 'legacy-target', 'legacy-detail')",
                    rusqlite::params![
                        vec![record_id; 16],
                        result,
                        classification,
                        principal_type,
                        responsible_owner,
                    ],
                )
                .unwrap();
        }
        drop(connection);
        let context = context(&temporary_directory, [7; 16]);

        drop(open(&context).unwrap());
        drop(open(&context).unwrap());

        let connection = Connection::open(database_path(&temporary_directory)).unwrap();
        let legacy_fields: Vec<LegacyAuditFields> = connection
            .prepare(
                "SELECT phase, result, attempt_record_id, classification, principal, principal_type, responsible_owner, action, target, detail \
                     FROM weavelit_log_audit_records ORDER BY record_id",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                ))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let ledger: Vec<(i64, String)> = connection
            .prepare(
                "SELECT sequence_number, identifier FROM weavelit_log_migration_ledger \
                 ORDER BY sequence_number",
            )
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        assert_eq!(
            legacy_fields,
            vec![
                (
                    "completion".into(),
                    0,
                    None,
                    None,
                    "legacy-principal".into(),
                    None,
                    None,
                    "legacy-action".into(),
                    "legacy-target".into(),
                    "legacy-detail".into(),
                ),
                (
                    "completion".into(),
                    1,
                    None,
                    Some("internal.log-policy.changed".into()),
                    "legacy-principal".into(),
                    Some("automation".into()),
                    Some("administrator".into()),
                    "legacy-action".into(),
                    "legacy-target".into(),
                    "legacy-detail".into(),
                ),
            ]
        );
        assert_eq!(
            ledger,
            vec![
                (1, "0001_create_log_destination_schema".into()),
                (2, "0002_bound_record_payloads".into()),
                (3, "0003_add_audit_accountability_schema".into()),
                (4, "0004_add_audit_attempt_link".into()),
            ]
        );
        let index_sql: String = connection
            .query_row(
                "SELECT sql FROM sqlite_schema \
                 WHERE type = 'index' AND name = 'weavelit_log_audit_attempt_record_id_idx'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(index_sql.contains("(attempt_record_id)"));
        assert!(index_sql.contains("WHERE attempt_record_id IS NOT NULL"));
        let terminal_guard_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema \
                 WHERE type = 'trigger' AND name = 'weavelit_log_audit_terminal_attempt_guard'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(terminal_guard_count, 1);
    }

    #[test]
    fn migration_four_trigger_rejects_invalid_terminal_attempt_links() {
        let temporary_directory = tempfile::tempdir().unwrap();
        create_migration_three_destination(&temporary_directory, [7; 16]);
        drop(open(&context(&temporary_directory, [7; 16])).unwrap());
        let connection = Connection::open(database_path(&temporary_directory)).unwrap();
        let cross_correlation_attempt_id = [0x34_u8; 16];
        connection
            .execute(
                "INSERT INTO weavelit_log_audit_records \
                 (record_id, event_time_milliseconds, phase, result, attempt_record_id, correlation_id, classification, principal, principal_type, responsible_owner, action, target, detail) \
                 VALUES (?1, '40', 'attempt', NULL, NULL, 'different-correlation', 'internal.log-policy.changed', 'principal', 'human', NULL, 'action', 'target', 'detail')",
                [cross_correlation_attempt_id.as_slice()],
            )
            .unwrap();

        for (record_id, attempt_record_id) in [
            ([0x35_u8; 16], [0x7f_u8; 16]),
            ([0x36_u8; 16], cross_correlation_attempt_id),
        ] {
            let error = connection
                .execute(
                    "INSERT INTO weavelit_log_audit_records \
                     (record_id, event_time_milliseconds, phase, result, attempt_record_id, correlation_id, classification, principal, principal_type, responsible_owner, action, target, detail) \
                     VALUES (?1, '42', 'completion', 1, ?2, 'audit-correlation', 'internal.log-policy.changed', 'principal', 'human', NULL, 'action', 'target', 'detail')",
                    rusqlite::params![record_id.as_slice(), attempt_record_id.as_slice()],
                )
                .unwrap_err();
            assert!(matches!(
                error,
                rusqlite::Error::SqliteFailure(sqlite_error, Some(message))
                    if sqlite_error.code == rusqlite::ErrorCode::ConstraintViolation
                        && sqlite_error.extended_code
                            == rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER
                        && message == "invalid Audit Attempt link"
            ));
        }
    }

    #[test]
    fn failed_audit_link_migration_leaves_no_partial_schema_or_ledger_entry() {
        let temporary_directory = tempfile::tempdir().unwrap();
        create_migration_three_destination(&temporary_directory, [7; 16]);
        let mut connection = Connection::open(database_path(&temporary_directory)).unwrap();
        let before_columns = audit_schema_columns(&connection);
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let failing_migration = Migration {
            sequence: MIGRATIONS[3].sequence,
            identifier: MIGRATIONS[3].identifier,
            sql: "ALTER TABLE weavelit_log_audit_records RENAME TO weavelit_log_audit_records_legacy; \
                  THIS IS NOT VALID SQL;",
        };

        assert_eq!(
            apply_migration(&transaction, &failing_migration),
            Err(LogDestinationError::IntegrityFailure)
        );
        drop(transaction);

        assert_eq!(audit_schema_columns(&connection), before_columns);
        let ledger_entries: i64 = connection
            .query_row(
                "SELECT count(*) FROM weavelit_log_migration_ledger",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(ledger_entries, 3);
    }

    #[test]
    fn exact_replay_is_acknowledged_and_changed_replay_is_rejected() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let context = context(&temporary_directory, [7; 16]);
        let destination = SqliteLogDestination::open(&context).unwrap();
        let record = system_record([3; 16], "pre-redacted-detail");

        deliver(&destination, &record).unwrap();
        deliver(&destination, &record).unwrap();
        assert_eq!(
            deliver(&destination, &system_record([3; 16], "changed-detail")),
            Err(LogDestinationError::IntegrityFailure)
        );
        assert_eq!(
            deliver(&destination, &audit_attempt_record([3; 16])),
            Err(LogDestinationError::IntegrityFailure)
        );

        let connection = destination.connection.lock().unwrap();
        let persisted: (i64, String) = connection
            .query_row(
                "SELECT count(*), detail FROM weavelit_log_system_records WHERE record_id = ?1",
                [[3_u8; 16].as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let cross_type_rows: i64 = connection
            .query_row(
                "SELECT count(*) FROM weavelit_log_audit_records WHERE record_id = ?1",
                [[3_u8; 16].as_slice()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(persisted, (1, "pre-redacted-detail".into()));
        assert_eq!(cross_type_rows, 0);
    }

    #[test]
    fn exact_replay_survives_reopen_without_a_second_row() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let context = context(&temporary_directory, [7; 16]);
        let system = system_record([0x30; 16], "pre-redacted-detail");
        let attempt = audit_attempt_record([0x31; 16]);
        let terminal = audit_record_with(
            [0x32; 16],
            43,
            PersistedAuditPhase::Completion(LogResult::Success),
            Some([0x31; 16]),
            "audit-correlation",
            "lifecycle.backup.created",
            "human",
            None,
        );

        let destination = SqliteLogDestination::open(&context).unwrap();
        for record in [&system, &attempt, &terminal] {
            deliver(&destination, record).unwrap();
        }
        drop(destination);

        let reopened = SqliteLogDestination::open(&context).unwrap();
        for record in [&system, &attempt, &terminal] {
            deliver(&reopened, record).unwrap();
        }
        let connection = reopened.connection.lock().unwrap();
        let system_rows: i64 = connection
            .query_row(
                "SELECT count(*) FROM weavelit_log_system_records",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let audit_rows: i64 = connection
            .query_row(
                "SELECT count(*) FROM weavelit_log_audit_records",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(system_rows, 1);
        assert_eq!(audit_rows, 2);
    }

    #[test]
    fn system_replay_compares_every_immutable_field_without_mutation() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let context = context(&temporary_directory, [7; 16]);
        let destination = SqliteLogDestination::open(&context).unwrap();
        let original = system_record([0x40; 16], "pre-redacted-detail");

        deliver(&destination, &original).unwrap();
        let mutations: [RecordMutation; 5] = [
            ("event_time", |record| {
                let PersistedLogRecord::System { event_time, .. } = record else {
                    unreachable!()
                };
                *event_time = (*event_time).saturating_sub(1);
            }),
            ("result", |record| {
                let PersistedLogRecord::System { result, .. } = record else {
                    unreachable!()
                };
                *result = LogResult::Failure;
            }),
            ("correlation_id", |record| {
                let PersistedLogRecord::System { correlation_id, .. } = record else {
                    unreachable!()
                };
                *correlation_id = "changed-correlation".into();
            }),
            ("classification", |record| {
                let PersistedLogRecord::System { classification, .. } = record else {
                    unreachable!()
                };
                *classification = "internal.error".into();
            }),
            ("detail", |record| {
                let PersistedLogRecord::System { detail, .. } = record else {
                    unreachable!()
                };
                *detail = "password=changed-secret".into();
            }),
        ];

        for (field, mutate) in mutations {
            let mut changed = original.clone();
            mutate(&mut changed);
            let error = deliver(&destination, &changed).unwrap_err();
            assert_eq!(error, LogDestinationError::IntegrityFailure, "{field}");
            assert!(!error.to_string().contains("changed-secret"), "{field}");
        }

        let persisted: (i64, String) = destination
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT count(*), detail FROM weavelit_log_system_records WHERE record_id = ?1",
                [[0x40_u8; 16].as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(persisted, (1, "pre-redacted-detail".into()));
    }

    #[test]
    fn audit_replay_compares_every_immutable_body_field_without_mutation() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let context = context(&temporary_directory, [7; 16]);
        let destination = SqliteLogDestination::open(&context).unwrap();
        let original = audit_record_with(
            [0x45; 16],
            42,
            PersistedAuditPhase::Attempt,
            None,
            "audit-correlation",
            "internal.log-policy.changed",
            "automation",
            Some("administrator"),
        );

        deliver(&destination, &original).unwrap();
        let mutations: [RecordMutation; 9] = [
            ("event_time", |record| {
                let PersistedLogRecord::Audit { event_time, .. } = record else {
                    unreachable!()
                };
                *event_time += 1;
            }),
            ("correlation_id", |record| {
                let PersistedLogRecord::Audit { correlation_id, .. } = record else {
                    unreachable!()
                };
                *correlation_id = "changed-correlation".into();
            }),
            ("classification", |record| {
                let PersistedLogRecord::Audit { classification, .. } = record else {
                    unreachable!()
                };
                *classification = "internal.server-configuration.changed".into();
            }),
            ("principal", |record| {
                let PersistedLogRecord::Audit { principal, .. } = record else {
                    unreachable!()
                };
                *principal = "different-operator".into();
            }),
            ("principal_type", |record| {
                let PersistedLogRecord::Audit { principal_type, .. } = record else {
                    unreachable!()
                };
                *principal_type = "human".into();
            }),
            ("responsible_owner", |record| {
                let PersistedLogRecord::Audit {
                    responsible_owner, ..
                } = record
                else {
                    unreachable!()
                };
                *responsible_owner = Some("different-administrator".into());
            }),
            ("action", |record| {
                let PersistedLogRecord::Audit { action, .. } = record else {
                    unreachable!()
                };
                *action = "different-action".into();
            }),
            ("target", |record| {
                let PersistedLogRecord::Audit { target, .. } = record else {
                    unreachable!()
                };
                *target = "different-target".into();
            }),
            ("detail", |record| {
                let PersistedLogRecord::Audit { detail, .. } = record else {
                    unreachable!()
                };
                *detail = "password=changed-secret".into();
            }),
        ];

        for (field, mutate) in mutations {
            let mut changed = original.clone();
            mutate(&mut changed);
            let error = deliver(&destination, &changed).unwrap_err();
            assert_eq!(error, LogDestinationError::IntegrityFailure, "{field}");
            assert!(!error.to_string().contains("changed-secret"), "{field}");
        }

        let persisted: (i64, String) = destination
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT count(*), detail FROM weavelit_log_audit_records WHERE record_id = ?1",
                [[0x45_u8; 16].as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(persisted, (1, "pre-redacted".into()));
    }

    #[test]
    fn audit_replay_compares_classification_principal_type_and_responsible_owner() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let context = context(&temporary_directory, [7; 16]);
        let destination = SqliteLogDestination::open(&context).unwrap();
        let original = audit_record_with(
            [0x45; 16],
            42,
            PersistedAuditPhase::Attempt,
            None,
            "audit-correlation",
            "internal.log-policy.changed",
            "automation",
            Some("administrator"),
        );

        deliver(&destination, &original).unwrap();
        deliver(&destination, &original).unwrap();
        for changed in [
            audit_record_with(
                [0x45; 16],
                42,
                PersistedAuditPhase::Attempt,
                None,
                "audit-correlation",
                "internal.server-configuration.changed",
                "automation",
                Some("administrator"),
            ),
            audit_record_with(
                [0x45; 16],
                42,
                PersistedAuditPhase::Attempt,
                None,
                "audit-correlation",
                "internal.log-policy.changed",
                "human",
                None,
            ),
            audit_record_with(
                [0x45; 16],
                42,
                PersistedAuditPhase::Attempt,
                None,
                "audit-correlation",
                "internal.log-policy.changed",
                "automation",
                Some("different-administrator"),
            ),
            audit_record_with(
                [0x45; 16],
                42,
                PersistedAuditPhase::Completion(LogResult::Success),
                Some([0x45; 16]),
                "audit-correlation",
                "internal.log-policy.changed",
                "automation",
                Some("administrator"),
            ),
        ] {
            assert_eq!(
                deliver(&destination, &changed),
                Err(LogDestinationError::IntegrityFailure)
            );
        }

        let connection = destination.connection.lock().unwrap();
        let fields: (String, String, Option<String>) = connection
            .query_row(
                "SELECT classification, principal_type, responsible_owner \
                 FROM weavelit_log_audit_records WHERE record_id = ?1",
                rusqlite::params![vec![0x45_u8; 16]],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            fields,
            (
                "internal.log-policy.changed".into(),
                "automation".into(),
                Some("administrator".into()),
            )
        );
    }

    #[test]
    fn audit_replay_compares_terminal_result_and_attempt_link() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let destination = open(&context(&temporary_directory, [7; 16])).unwrap();
        for attempt_record_id in [[0x50; 16], [0x51; 16]] {
            deliver(
                &destination,
                &audit_record_with(
                    attempt_record_id,
                    40,
                    PersistedAuditPhase::Attempt,
                    None,
                    "audit-correlation",
                    "internal.log-policy.changed",
                    "human",
                    None,
                ),
            )
            .unwrap();
        }
        let original = audit_record_with(
            [0x52; 16],
            42,
            PersistedAuditPhase::Completion(LogResult::Success),
            Some([0x50; 16]),
            "audit-correlation",
            "internal.log-policy.changed",
            "human",
            None,
        );

        deliver(&destination, &original).unwrap();
        deliver(&destination, &original).unwrap();
        for changed in [
            audit_record_with(
                [0x52; 16],
                42,
                PersistedAuditPhase::Correction(LogResult::Success),
                Some([0x50; 16]),
                "audit-correlation",
                "internal.log-policy.changed",
                "human",
                None,
            ),
            audit_record_with(
                [0x52; 16],
                42,
                PersistedAuditPhase::Completion(LogResult::Failure),
                Some([0x50; 16]),
                "audit-correlation",
                "internal.log-policy.changed",
                "human",
                None,
            ),
            audit_record_with(
                [0x52; 16],
                42,
                PersistedAuditPhase::Completion(LogResult::Success),
                Some([0x51; 16]),
                "audit-correlation",
                "internal.log-policy.changed",
                "human",
                None,
            ),
        ] {
            assert_eq!(
                deliver(&destination, &changed),
                Err(LogDestinationError::IntegrityFailure)
            );
        }

        let terminal_rows: i64 = destination
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM weavelit_log_audit_records WHERE record_id = ?1",
                [[0x52_u8; 16].as_slice()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(terminal_rows, 1);
    }

    #[test]
    fn terminal_links_reject_absent_non_attempt_cross_correlation_and_future_targets() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let destination = open(&context(&temporary_directory, [7; 16])).unwrap();
        for attempt in [
            audit_record_with(
                [0x60; 16],
                40,
                PersistedAuditPhase::Attempt,
                None,
                "audit-correlation",
                "internal.log-policy.changed",
                "human",
                None,
            ),
            audit_record_with(
                [0x61; 16],
                40,
                PersistedAuditPhase::Attempt,
                None,
                "different-correlation",
                "internal.log-policy.changed",
                "human",
                None,
            ),
            audit_record_with(
                [0x62; 16],
                50,
                PersistedAuditPhase::Attempt,
                None,
                "audit-correlation",
                "internal.log-policy.changed",
                "human",
                None,
            ),
        ] {
            deliver(&destination, &attempt).unwrap();
        }
        let prior_completion = audit_record_with(
            [0x63; 16],
            42,
            PersistedAuditPhase::Completion(LogResult::Success),
            Some([0x60; 16]),
            "audit-correlation",
            "internal.log-policy.changed",
            "human",
            None,
        );
        deliver(&destination, &prior_completion).unwrap();

        for rejected in [
            audit_record_with(
                [0x64; 16],
                42,
                PersistedAuditPhase::Completion(LogResult::Success),
                None,
                "audit-correlation",
                "internal.log-policy.changed",
                "human",
                None,
            ),
            audit_record_with(
                [0x65; 16],
                42,
                PersistedAuditPhase::Completion(LogResult::Success),
                Some([0x7f; 16]),
                "audit-correlation",
                "internal.log-policy.changed",
                "human",
                None,
            ),
            audit_record_with(
                [0x66; 16],
                42,
                PersistedAuditPhase::Completion(LogResult::Success),
                Some([0x61; 16]),
                "audit-correlation",
                "internal.log-policy.changed",
                "human",
                None,
            ),
            audit_record_with(
                [0x67; 16],
                42,
                PersistedAuditPhase::Correction(LogResult::Failure),
                Some([0x63; 16]),
                "audit-correlation",
                "internal.log-policy.changed",
                "human",
                None,
            ),
            audit_record_with(
                [0x68; 16],
                42,
                PersistedAuditPhase::Completion(LogResult::Success),
                Some([0x62; 16]),
                "audit-correlation",
                "internal.log-policy.changed",
                "human",
                None,
            ),
            audit_record_with(
                [0x69; 16],
                42,
                PersistedAuditPhase::Attempt,
                Some([0x60; 16]),
                "audit-correlation",
                "internal.log-policy.changed",
                "human",
                None,
            ),
        ] {
            let error = deliver(&destination, &rejected).unwrap_err();
            assert_eq!(error, LogDestinationError::IntegrityFailure);
            assert_eq!(
                error.to_string(),
                "log destination integrity validation failed"
            );
        }
    }

    #[test]
    fn terminal_delivery_reports_a_malformed_stored_attempt_time_as_unavailable() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let destination = open(&context(&temporary_directory, [7; 16])).unwrap();
        let attempt_record_id = [0x6a_u8; 16];
        let terminal_record_id = [0x6b_u8; 16];
        deliver(
            &destination,
            &audit_record_with(
                attempt_record_id,
                40,
                PersistedAuditPhase::Attempt,
                None,
                "audit-correlation",
                "internal.log-policy.changed",
                "human",
                None,
            ),
        )
        .unwrap();
        destination
            .connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE weavelit_log_audit_records \
                 SET event_time_milliseconds = 'not-a-timestamp' WHERE record_id = ?1",
                [attempt_record_id.as_slice()],
            )
            .unwrap();

        assert_eq!(
            deliver(
                &destination,
                &audit_record_with(
                    terminal_record_id,
                    42,
                    PersistedAuditPhase::Completion(LogResult::Success),
                    Some(attempt_record_id),
                    "audit-correlation",
                    "internal.log-policy.changed",
                    "human",
                    None,
                ),
            ),
            Err(LogDestinationError::Unavailable)
        );
        let terminal_count: i64 = destination
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM weavelit_log_audit_records WHERE record_id = ?1",
                [terminal_record_id.as_slice()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(terminal_count, 0);
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
        let unavailable_context = TestDestinationInputs::new(unavailable_root.clone(), [7; 16]);
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
        let record = system_record([4; 16], "secret-record-content");
        deliver(&destination, &record).unwrap();
        destination
            .connection
            .lock()
            .unwrap()
            .busy_timeout(Duration::ZERO)
            .unwrap();
        let lock = Connection::open(database_path(&temporary_directory)).unwrap();
        lock.execute_batch("BEGIN EXCLUSIVE").unwrap();

        let error = deliver(&destination, &record).unwrap_err();
        assert_eq!(error, LogDestinationError::Unavailable);
        assert!(!error.to_string().contains("secret-record-content"));
        lock.execute_batch("ROLLBACK").unwrap();

        let persisted_rows: i64 = destination
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM weavelit_log_system_records",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(persisted_rows, 1);
    }

    #[test]
    fn undecodable_existing_record_makes_replay_unavailable_without_mutation() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let context = context(&temporary_directory, [7; 16]);
        let record = system_record([0x70; 16], "secret-record-content");
        let destination = SqliteLogDestination::open(&context).unwrap();
        deliver(&destination, &record).unwrap();
        drop(destination);

        let connection = Connection::open(database_path(&temporary_directory)).unwrap();
        connection
            .execute(
                "UPDATE weavelit_log_system_records SET detail = zeroblob(8) WHERE record_id = ?1",
                [[0x70_u8; 16].as_slice()],
            )
            .unwrap();
        drop(connection);

        let reopened = SqliteLogDestination::open(&context).unwrap();
        let error = deliver(&reopened, &record).unwrap_err();
        assert_eq!(error, LogDestinationError::Unavailable);
        for secret in ["secret-record-content", "log.sqlite3", "detail"] {
            assert!(!error.to_string().contains(secret));
            assert!(!format!("{error:?}").contains(secret));
        }
        let persisted: (i64, String) = reopened
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT count(*), typeof(detail) FROM weavelit_log_system_records WHERE record_id = ?1",
                [[0x70_u8; 16].as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(persisted, (1, "blob".into()));
    }

    #[test]
    fn tampered_migration_ledger_fails_closed() {
        for mutation in [
            "UPDATE weavelit_log_migration_ledger SET checksum = zeroblob(32)",
            "UPDATE weavelit_log_migration_ledger SET sequence_number = 5 WHERE sequence_number = 2",
            "DELETE FROM weavelit_log_migration_ledger",
            "INSERT INTO weavelit_log_migration_ledger (sequence_number, identifier, checksum) \
                    VALUES (5, '0005_unknown', zeroblob(32))",
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
