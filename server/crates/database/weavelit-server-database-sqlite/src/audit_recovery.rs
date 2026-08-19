use rusqlite::{Connection, OptionalExtension as _, Transaction, TransactionBehavior, params};
use weavelit_server_database::{
    AUDIT_TERMINAL_OBLIGATION_IDENTIFIER_LENGTH, AuditTerminalAcknowledgementProof,
    AuditTerminalObligation, AuditTerminalRecoveryPersistence, AuditTerminalRecoveryStore,
    AuditTerminalRecoveryTransaction, AuditTerminalReplayBatchSize, AuditTerminalSupersession,
    DatabaseError, MAX_AUDIT_TERMINAL_OBLIGATION_BYTES,
    MAX_AUDIT_TERMINAL_SUPERSESSION_DISPOSITION_BYTES, StoredAuditDestinationBinding,
    ValidatedAuditTerminalObligationWrite,
};

use crate::SqliteDatabase;
use crate::error::{ErrorContext, map_sqlite_error};

const INSERT_OBLIGATION: &str = "INSERT INTO weavelit_audit_terminal_obligation \
     (record_identifier, projection, binding_identifier, binding_version) \
     VALUES (?1, ?2, ?3, ?4)";
const SELECT_OBLIGATION: &str = "SELECT record_identifier, projection, binding_identifier, \
     binding_version, acknowledged FROM weavelit_audit_terminal_obligation \
     WHERE record_identifier = ?1";
const SELECT_ACTIVE: &str = "SELECT obligation.record_identifier, obligation.projection, \
     obligation.binding_identifier, obligation.binding_version \
     FROM weavelit_audit_terminal_obligation AS obligation \
     LEFT JOIN weavelit_audit_terminal_supersession AS supersession \
       ON supersession.original_record_identifier = obligation.record_identifier \
     WHERE obligation.acknowledged = 0 \
       AND supersession.original_record_identifier IS NULL \
     ORDER BY obligation.sequence_number LIMIT ?1";
const SELECT_LATE: &str = "SELECT original.record_identifier, original.projection, \
     original.binding_identifier, original.binding_version, supersession.disposition, \
     supersession.original_binding_identifier, supersession.original_binding_version, \
     supersession.replacement_binding_identifier, supersession.replacement_binding_version, \
     replacement.binding_identifier, replacement.binding_version \
     FROM weavelit_audit_terminal_obligation AS original \
     JOIN weavelit_audit_terminal_supersession AS supersession \
       ON supersession.original_record_identifier = original.record_identifier \
     JOIN weavelit_audit_terminal_obligation AS replacement \
       ON replacement.record_identifier = supersession.replacement_record_identifier \
     WHERE original.acknowledged = 0 \
     ORDER BY original.sequence_number LIMIT ?1";
const SELECT_SUPERSESSION: &str = "SELECT disposition, original_binding_identifier, \
     original_binding_version, replacement_record_identifier, replacement_binding_identifier, \
     replacement_binding_version FROM weavelit_audit_terminal_supersession \
     WHERE original_record_identifier = ?1";
const INSERT_SUPERSESSION: &str = "INSERT INTO weavelit_audit_terminal_supersession \
     (original_record_identifier, disposition, original_binding_identifier, \
      original_binding_version, replacement_record_identifier, \
      replacement_binding_identifier, replacement_binding_version) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)";
const SELECT_OLDEST_ACTIVE: &str = "SELECT obligation.record_identifier \
     FROM weavelit_audit_terminal_obligation AS obligation \
     LEFT JOIN weavelit_audit_terminal_supersession AS supersession \
       ON supersession.original_record_identifier = obligation.record_identifier \
     WHERE obligation.acknowledged = 0 \
       AND supersession.original_record_identifier IS NULL \
     ORDER BY obligation.sequence_number LIMIT 1";
const SELECT_OLDEST_LATE: &str = "SELECT original.record_identifier \
     FROM weavelit_audit_terminal_obligation AS original \
     JOIN weavelit_audit_terminal_supersession AS supersession \
       ON supersession.original_record_identifier = original.record_identifier \
     WHERE original.acknowledged = 0 \
     ORDER BY original.sequence_number LIMIT 1";
const ACKNOWLEDGE: &str = "UPDATE weavelit_audit_terminal_obligation SET acknowledged = 1 \
     WHERE record_identifier = ?1 AND binding_identifier = ?2 AND binding_version = ?3 \
       AND acknowledged = 0";

struct StoredObligationRow {
    identifier: Vec<u8>,
    projection: Vec<u8>,
    binding_identifier: Vec<u8>,
    binding_version: Vec<u8>,
    acknowledged: i64,
}

struct StoredSupersessionRow {
    disposition: Vec<u8>,
    original_binding_identifier: Vec<u8>,
    original_binding_version: Vec<u8>,
    replacement_identifier: Vec<u8>,
    replacement_binding_identifier: Vec<u8>,
    replacement_binding_version: Vec<u8>,
}

/// Private SQLite transaction adapter used by an owning application-state mutation.
pub(crate) struct SqliteAuditTerminalRecoveryTransaction<'a> {
    transaction: Transaction<'a>,
}

impl<'a> SqliteAuditTerminalRecoveryTransaction<'a> {
    pub(crate) fn begin(database: &'a mut SqliteDatabase) -> Result<Self, DatabaseError> {
        let transaction = database
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| map_sqlite_error(error, ErrorContext::AuditRecovery))?;
        Ok(Self { transaction })
    }

    pub(crate) fn commit(self) -> Result<(), DatabaseError> {
        self.transaction
            .commit()
            .map_err(|error| map_sqlite_error(error, ErrorContext::AuditRecovery))
    }
}

impl AuditTerminalRecoveryTransaction for SqliteAuditTerminalRecoveryTransaction<'_> {
    fn persist_audit_terminal_obligation(
        &mut self,
        obligation: &ValidatedAuditTerminalObligationWrite,
    ) -> Result<(), DatabaseError> {
        insert_exact(&self.transaction, obligation).map(|_| ())
    }

    fn append_audit_terminal_supersession(
        &mut self,
        supersession: &AuditTerminalSupersession,
    ) -> Result<(), DatabaseError> {
        append_supersession(&self.transaction, supersession)
    }
}

impl AuditTerminalRecoveryStore for SqliteDatabase {
    fn list_pending_audit_terminal_obligations(
        &mut self,
        persistence: &AuditTerminalRecoveryPersistence,
        batch_size: AuditTerminalReplayBatchSize,
    ) -> Result<Vec<AuditTerminalObligation>, DatabaseError> {
        list_active(&self.connection, persistence, batch_size)
    }

    fn list_late_delivery_audit_terminal_obligations(
        &mut self,
        persistence: &AuditTerminalRecoveryPersistence,
        batch_size: AuditTerminalReplayBatchSize,
    ) -> Result<Vec<AuditTerminalObligation>, DatabaseError> {
        list_late(&self.connection, persistence, batch_size)
    }

    fn acknowledge_audit_terminal_obligation(
        &mut self,
        acknowledgement: AuditTerminalAcknowledgementProof,
    ) -> Result<(), DatabaseError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| map_sqlite_error(error, ErrorContext::AuditRecovery))?;
        acknowledge(&transaction, &acknowledgement)?;
        transaction
            .commit()
            .map_err(|error| map_sqlite_error(error, ErrorContext::AuditRecovery))
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum InsertResult {
    Inserted,
    ExactExisting,
}

fn insert_exact(
    connection: &Connection,
    obligation: &ValidatedAuditTerminalObligationWrite,
) -> Result<InsertResult, DatabaseError> {
    if let Some(stored) =
        load_raw_obligation(connection, obligation.identifier().as_bytes().as_slice())?
    {
        return if raw_matches_write(&stored, obligation)? {
            Ok(InsertResult::ExactExisting)
        } else {
            Err(DatabaseError::InvalidState)
        };
    }

    connection
        .execute(
            INSERT_OBLIGATION,
            params![
                obligation.identifier().as_bytes().as_slice(),
                obligation.projection_bytes(),
                obligation.binding().identifier().as_slice(),
                obligation.binding().version().to_be_bytes().as_slice(),
            ],
        )
        .map_err(|error| map_sqlite_error(error, ErrorContext::AuditRecovery))?;
    Ok(InsertResult::Inserted)
}

fn append_supersession(
    connection: &Connection,
    supersession: &AuditTerminalSupersession,
) -> Result<(), DatabaseError> {
    let original_identifier = supersession.original_obligation().identifier();
    if let Some(stored) = load_supersession(connection, original_identifier.as_bytes())? {
        let original = load_raw_obligation(connection, original_identifier.as_bytes())?
            .ok_or(DatabaseError::InvalidState)?;
        let replacement = load_raw_obligation(
            connection,
            supersession
                .replacement_obligation()
                .identifier()
                .as_bytes(),
        )?
        .ok_or(DatabaseError::InvalidState)?;
        return if raw_matches_supersession_original(&original, supersession)?
            && raw_matches_write(&replacement, supersession.replacement_obligation())?
            && supersession_row_matches(&stored, supersession)?
        {
            Ok(())
        } else {
            Err(DatabaseError::InvalidState)
        };
    }

    let oldest = oldest_identifier(connection, false)?.ok_or(DatabaseError::InvalidState)?;
    if oldest.as_slice() != original_identifier.as_bytes() {
        return Err(DatabaseError::InvalidState);
    }
    let original = load_raw_obligation(connection, original_identifier.as_bytes())?
        .ok_or(DatabaseError::InvalidState)?;
    if !raw_matches_supersession_original(&original, supersession)?
        || load_raw_obligation(
            connection,
            supersession
                .replacement_obligation()
                .identifier()
                .as_bytes(),
        )?
        .is_some()
    {
        return Err(DatabaseError::InvalidState);
    }

    if insert_exact(connection, supersession.replacement_obligation())? != InsertResult::Inserted {
        return Err(DatabaseError::InvalidState);
    }
    connection
        .execute(
            INSERT_SUPERSESSION,
            params![
                original_identifier.as_bytes().as_slice(),
                supersession.disposition_bytes(),
                supersession.original_binding().identifier().as_slice(),
                supersession
                    .original_binding()
                    .version()
                    .to_be_bytes()
                    .as_slice(),
                supersession
                    .replacement_obligation()
                    .identifier()
                    .as_bytes()
                    .as_slice(),
                supersession.replacement_binding().identifier().as_slice(),
                supersession
                    .replacement_binding()
                    .version()
                    .to_be_bytes()
                    .as_slice(),
            ],
        )
        .map_err(|error| map_sqlite_error(error, ErrorContext::AuditRecovery))?;
    Ok(())
}

fn list_active(
    connection: &Connection,
    persistence: &AuditTerminalRecoveryPersistence,
    batch_size: AuditTerminalReplayBatchSize,
) -> Result<Vec<AuditTerminalObligation>, DatabaseError> {
    let mut statement = connection
        .prepare(SELECT_ACTIVE)
        .map_err(|error| map_sqlite_error(error, ErrorContext::AuditRecovery))?;
    let rows = statement
        .query_map([batch_size_i64(batch_size)?], |row| {
            Ok(StoredObligationRow {
                identifier: row.get(0)?,
                projection: row.get(1)?,
                binding_identifier: row.get(2)?,
                binding_version: row.get(3)?,
                acknowledged: 0,
            })
        })
        .map_err(|error| map_sqlite_error(error, ErrorContext::AuditRecovery))?;
    rows.map(|row| {
        row.map_err(|error| map_sqlite_error(error, ErrorContext::AuditRecovery))
            .and_then(|row| decode_obligation(persistence, row))
    })
    .collect()
}

fn list_late(
    connection: &Connection,
    persistence: &AuditTerminalRecoveryPersistence,
    batch_size: AuditTerminalReplayBatchSize,
) -> Result<Vec<AuditTerminalObligation>, DatabaseError> {
    let mut statement = connection
        .prepare(SELECT_LATE)
        .map_err(|error| map_sqlite_error(error, ErrorContext::AuditRecovery))?;
    let rows = statement
        .query_map([batch_size_i64(batch_size)?], |row| {
            Ok((
                StoredObligationRow {
                    identifier: row.get(0)?,
                    projection: row.get(1)?,
                    binding_identifier: row.get(2)?,
                    binding_version: row.get(3)?,
                    acknowledged: 0,
                },
                row.get::<_, Vec<u8>>(4)?,
                row.get::<_, Vec<u8>>(5)?,
                row.get::<_, Vec<u8>>(6)?,
                row.get::<_, Vec<u8>>(7)?,
                row.get::<_, Vec<u8>>(8)?,
                row.get::<_, Vec<u8>>(9)?,
                row.get::<_, Vec<u8>>(10)?,
            ))
        })
        .map_err(|error| map_sqlite_error(error, ErrorContext::AuditRecovery))?;

    rows.map(|row| {
        let (
            row,
            disposition,
            original_binding_identifier,
            original_binding_version,
            replacement_binding_identifier,
            replacement_binding_version,
            stored_replacement_binding_identifier,
            stored_replacement_binding_version,
        ) = row.map_err(|error| map_sqlite_error(error, ErrorContext::AuditRecovery))?;
        let obligation = decode_obligation(persistence, row)?;
        validate_disposition_bytes(&disposition)?;
        let original_binding = decode_binding(
            persistence,
            original_binding_identifier,
            original_binding_version,
        )?;
        let replacement_binding = decode_binding(
            persistence,
            replacement_binding_identifier,
            replacement_binding_version,
        )?;
        let stored_replacement_binding = decode_binding(
            persistence,
            stored_replacement_binding_identifier,
            stored_replacement_binding_version,
        )?;
        if obligation.binding() != &original_binding
            || replacement_binding != stored_replacement_binding
            || replacement_binding == original_binding
        {
            return Err(DatabaseError::IntegrityFailure);
        }
        Ok(obligation)
    })
    .collect()
}

fn acknowledge(
    transaction: &Transaction<'_>,
    acknowledgement: &AuditTerminalAcknowledgementProof,
) -> Result<(), DatabaseError> {
    let row = load_raw_obligation(transaction, acknowledgement.identifier().as_bytes())?
        .ok_or(DatabaseError::InvalidState)?;
    if row.acknowledged != 0 {
        return Err(DatabaseError::InvalidState);
    }
    validate_projection_bytes(&row.projection)?;
    let stored_identifier = fixed_identifier(row.identifier.clone())?;
    if stored_identifier.as_slice() != acknowledgement.identifier().as_bytes() {
        return Err(DatabaseError::IntegrityFailure);
    }
    let stored_binding = decode_binding_without_persistence(
        row.binding_identifier.clone(),
        row.binding_version.clone(),
    )?;
    if stored_binding.0.as_slice() != acknowledgement.binding().identifier()
        || stored_binding.1 != acknowledgement.binding().version()
    {
        return Err(DatabaseError::InvalidState);
    }

    let is_late = load_supersession(transaction, acknowledgement.identifier().as_bytes())?
        .map(|supersession| validate_supersession_for_ack(transaction, &row, &supersession))
        .transpose()?
        .is_some();
    let oldest = oldest_identifier(transaction, is_late)?.ok_or(DatabaseError::InvalidState)?;
    if oldest.as_slice() != acknowledgement.identifier().as_bytes() {
        return Err(DatabaseError::InvalidState);
    }

    let changed = transaction
        .execute(
            ACKNOWLEDGE,
            params![
                acknowledgement.identifier().as_bytes().as_slice(),
                acknowledgement.binding().identifier().as_slice(),
                acknowledgement.binding().version().to_be_bytes().as_slice(),
            ],
        )
        .map_err(|error| map_sqlite_error(error, ErrorContext::AuditRecovery))?;
    if changed != 1 {
        return Err(DatabaseError::InvalidState);
    }
    Ok(())
}

fn load_raw_obligation(
    connection: &Connection,
    identifier: &[u8],
) -> Result<Option<StoredObligationRow>, DatabaseError> {
    connection
        .query_row(SELECT_OBLIGATION, [identifier], |row| {
            Ok(StoredObligationRow {
                identifier: row.get(0)?,
                projection: row.get(1)?,
                binding_identifier: row.get(2)?,
                binding_version: row.get(3)?,
                acknowledged: row.get(4)?,
            })
        })
        .optional()
        .map_err(|error| map_sqlite_error(error, ErrorContext::AuditRecovery))
}

fn load_supersession(
    connection: &Connection,
    original_identifier: &[u8],
) -> Result<Option<StoredSupersessionRow>, DatabaseError> {
    connection
        .query_row(SELECT_SUPERSESSION, [original_identifier], |row| {
            Ok(StoredSupersessionRow {
                disposition: row.get(0)?,
                original_binding_identifier: row.get(1)?,
                original_binding_version: row.get(2)?,
                replacement_identifier: row.get(3)?,
                replacement_binding_identifier: row.get(4)?,
                replacement_binding_version: row.get(5)?,
            })
        })
        .optional()
        .map_err(|error| map_sqlite_error(error, ErrorContext::AuditRecovery))
}

fn oldest_identifier(
    connection: &Connection,
    late: bool,
) -> Result<Option<Vec<u8>>, DatabaseError> {
    connection
        .query_row(
            if late {
                SELECT_OLDEST_LATE
            } else {
                SELECT_OLDEST_ACTIVE
            },
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| map_sqlite_error(error, ErrorContext::AuditRecovery))
}

fn decode_obligation(
    persistence: &AuditTerminalRecoveryPersistence,
    row: StoredObligationRow,
) -> Result<AuditTerminalObligation, DatabaseError> {
    if row.acknowledged != 0 && row.acknowledged != 1 {
        return Err(DatabaseError::IntegrityFailure);
    }
    let identifier = fixed_identifier(row.identifier)?;
    let binding = decode_binding(persistence, row.binding_identifier, row.binding_version)?;
    AuditTerminalObligation::from_persisted(persistence, identifier, row.projection, binding)
        .map_err(|_| DatabaseError::IntegrityFailure)
}

fn decode_binding(
    persistence: &AuditTerminalRecoveryPersistence,
    identifier: Vec<u8>,
    version: Vec<u8>,
) -> Result<StoredAuditDestinationBinding, DatabaseError> {
    let (identifier, version) = decode_binding_without_persistence(identifier, version)?;
    StoredAuditDestinationBinding::from_persisted(persistence, identifier, version)
        .map_err(|_| DatabaseError::IntegrityFailure)
}

fn decode_binding_without_persistence(
    identifier: Vec<u8>,
    version: Vec<u8>,
) -> Result<([u8; AUDIT_TERMINAL_OBLIGATION_IDENTIFIER_LENGTH], u64), DatabaseError> {
    let identifier = fixed_identifier(identifier)?;
    let version: [u8; 8] = version
        .try_into()
        .map_err(|_| DatabaseError::IntegrityFailure)?;
    let version = u64::from_be_bytes(version);
    if version == 0 {
        return Err(DatabaseError::IntegrityFailure);
    }
    Ok((identifier, version))
}

fn fixed_identifier(
    bytes: Vec<u8>,
) -> Result<[u8; AUDIT_TERMINAL_OBLIGATION_IDENTIFIER_LENGTH], DatabaseError> {
    let bytes: [u8; AUDIT_TERMINAL_OBLIGATION_IDENTIFIER_LENGTH] = bytes
        .try_into()
        .map_err(|_| DatabaseError::IntegrityFailure)?;
    if bytes == [0; AUDIT_TERMINAL_OBLIGATION_IDENTIFIER_LENGTH] {
        return Err(DatabaseError::IntegrityFailure);
    }
    Ok(bytes)
}

fn raw_matches_write(
    stored: &StoredObligationRow,
    expected: &ValidatedAuditTerminalObligationWrite,
) -> Result<bool, DatabaseError> {
    if stored.acknowledged != 0 && stored.acknowledged != 1 {
        return Err(DatabaseError::IntegrityFailure);
    }
    let identifier = fixed_identifier(stored.identifier.clone())?;
    let (binding_identifier, binding_version) = decode_binding_without_persistence(
        stored.binding_identifier.clone(),
        stored.binding_version.clone(),
    )?;
    validate_projection_bytes(&stored.projection)?;
    Ok(identifier.as_slice() == expected.identifier().as_bytes()
        && stored.projection == expected.projection_bytes()
        && binding_identifier.as_slice() == expected.binding().identifier()
        && binding_version == expected.binding().version())
}

fn raw_matches_supersession_original(
    stored: &StoredObligationRow,
    expected: &AuditTerminalSupersession,
) -> Result<bool, DatabaseError> {
    if stored.acknowledged != 0 && stored.acknowledged != 1 {
        return Err(DatabaseError::IntegrityFailure);
    }
    let identifier = fixed_identifier(stored.identifier.clone())?;
    let (binding_identifier, binding_version) = decode_binding_without_persistence(
        stored.binding_identifier.clone(),
        stored.binding_version.clone(),
    )?;
    validate_projection_bytes(&stored.projection)?;
    Ok(
        identifier.as_slice() == expected.original_obligation().identifier().as_bytes()
            && stored.projection.as_slice() == expected.original_projection_bytes()
            && binding_identifier.as_slice() == expected.original_binding().identifier()
            && binding_version == expected.original_binding().version(),
    )
}

fn supersession_row_matches(
    stored: &StoredSupersessionRow,
    expected: &AuditTerminalSupersession,
) -> Result<bool, DatabaseError> {
    validate_disposition_bytes(&stored.disposition)?;
    let (original_identifier, original_version) = decode_binding_without_persistence(
        stored.original_binding_identifier.clone(),
        stored.original_binding_version.clone(),
    )?;
    let (replacement_identifier, replacement_version) = decode_binding_without_persistence(
        stored.replacement_binding_identifier.clone(),
        stored.replacement_binding_version.clone(),
    )?;
    Ok(stored.disposition == expected.disposition_bytes()
        && original_identifier.as_slice() == expected.original_binding().identifier()
        && original_version == expected.original_binding().version()
        && stored.replacement_identifier.as_slice()
            == expected.replacement_obligation().identifier().as_bytes()
        && replacement_identifier.as_slice() == expected.replacement_binding().identifier()
        && replacement_version == expected.replacement_binding().version())
}

fn validate_supersession_for_ack(
    connection: &Connection,
    original: &StoredObligationRow,
    supersession: &StoredSupersessionRow,
) -> Result<(), DatabaseError> {
    validate_disposition_bytes(&supersession.disposition)?;
    let (original_binding_identifier, original_binding_version) =
        decode_binding_without_persistence(
            supersession.original_binding_identifier.clone(),
            supersession.original_binding_version.clone(),
        )?;
    let (stored_original_identifier, stored_original_version) = decode_binding_without_persistence(
        original.binding_identifier.clone(),
        original.binding_version.clone(),
    )?;
    let replacement = load_raw_obligation(connection, &supersession.replacement_identifier)?
        .ok_or(DatabaseError::IntegrityFailure)?;
    let (replacement_binding_identifier, replacement_binding_version) =
        decode_binding_without_persistence(
            supersession.replacement_binding_identifier.clone(),
            supersession.replacement_binding_version.clone(),
        )?;
    let (stored_replacement_identifier, stored_replacement_version) =
        decode_binding_without_persistence(
            replacement.binding_identifier,
            replacement.binding_version,
        )?;
    if original_binding_identifier != stored_original_identifier
        || original_binding_version != stored_original_version
        || replacement_binding_identifier != stored_replacement_identifier
        || replacement_binding_version != stored_replacement_version
        || (original_binding_identifier == replacement_binding_identifier
            && original_binding_version == replacement_binding_version)
    {
        return Err(DatabaseError::IntegrityFailure);
    }
    Ok(())
}

fn validate_projection_bytes(bytes: &[u8]) -> Result<(), DatabaseError> {
    if bytes.is_empty() || bytes.len() > MAX_AUDIT_TERMINAL_OBLIGATION_BYTES {
        return Err(DatabaseError::IntegrityFailure);
    }
    Ok(())
}

fn validate_disposition_bytes(bytes: &[u8]) -> Result<(), DatabaseError> {
    if bytes.is_empty() || bytes.len() > MAX_AUDIT_TERMINAL_SUPERSESSION_DISPOSITION_BYTES {
        return Err(DatabaseError::IntegrityFailure);
    }
    Ok(())
}

fn batch_size_i64(batch_size: AuditTerminalReplayBatchSize) -> Result<i64, DatabaseError> {
    i64::try_from(batch_size.get()).map_err(|_| DatabaseError::IntegrityFailure)
}

#[cfg(test)]
mod tests {
    use super::*;
    use weavelit_server_database::ApplicationDatabase;
    use weavelit_server_database_authority::ServerDatabaseAuthority;

    fn database() -> (tempfile::TempDir, std::path::PathBuf, SqliteDatabase) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory
            .path()
            .canonicalize()
            .unwrap()
            .join("application.db");
        let database = SqliteDatabase::open(&path).unwrap();
        (directory, path, database)
    }

    fn persistence() -> AuditTerminalRecoveryPersistence {
        AuditTerminalRecoveryPersistence::from_server_authority(&ServerDatabaseAuthority::new())
    }

    fn binding(
        persistence: &AuditTerminalRecoveryPersistence,
        byte: u8,
        version: u64,
    ) -> StoredAuditDestinationBinding {
        StoredAuditDestinationBinding::from_persisted(persistence, [byte; 16], version).unwrap()
    }

    fn write(
        persistence: &AuditTerminalRecoveryPersistence,
        identifier: u8,
        projection: &[u8],
        binding_byte: u8,
        version: u64,
    ) -> ValidatedAuditTerminalObligationWrite {
        ValidatedAuditTerminalObligationWrite::from_server_audit(
            persistence,
            [identifier; 16],
            projection.to_vec(),
            binding(persistence, binding_byte, version),
        )
        .unwrap()
    }

    fn persist(
        database: &mut SqliteDatabase,
        obligation: &ValidatedAuditTerminalObligationWrite,
    ) -> Result<(), DatabaseError> {
        let mut transaction = SqliteAuditTerminalRecoveryTransaction::begin(database)?;
        transaction.persist_audit_terminal_obligation(obligation)?;
        transaction.commit()
    }

    fn append(
        database: &mut SqliteDatabase,
        supersession: &AuditTerminalSupersession,
    ) -> Result<(), DatabaseError> {
        let mut transaction = SqliteAuditTerminalRecoveryTransaction::begin(database)?;
        transaction.append_audit_terminal_supersession(supersession)?;
        transaction.commit()
    }

    fn list_active(
        database: &mut SqliteDatabase,
        persistence: &AuditTerminalRecoveryPersistence,
    ) -> Vec<AuditTerminalObligation> {
        database
            .list_pending_audit_terminal_obligations(
                persistence,
                AuditTerminalReplayBatchSize::new(64).unwrap(),
            )
            .unwrap()
    }

    fn list_late(
        database: &mut SqliteDatabase,
        persistence: &AuditTerminalRecoveryPersistence,
    ) -> Vec<AuditTerminalObligation> {
        database
            .list_late_delivery_audit_terminal_obligations(
                persistence,
                AuditTerminalReplayBatchSize::new(64).unwrap(),
            )
            .unwrap()
    }

    #[test]
    fn recovery_is_fifo_restart_durable_and_exactly_idempotent() {
        let (_directory, path, mut database) = database();
        let persistence = persistence();
        let first = write(&persistence, 1, b"first-original-bytes", 7, 3);
        let second = write(&persistence, 2, b"second-original-bytes", 7, 3);
        persist(&mut database, &first).unwrap();
        persist(&mut database, &first).unwrap();
        persist(&mut database, &second).unwrap();
        assert_eq!(
            persist(
                &mut database,
                &write(&persistence, 1, b"byte-different", 7, 3)
            ),
            Err(DatabaseError::InvalidState)
        );

        SqliteDatabase::close(database).unwrap();
        let mut database = SqliteDatabase::open(&path).unwrap();
        let active = list_active(&mut database, &persistence);
        assert_eq!(active.len(), 2);
        assert_eq!(active[0].identifier().as_bytes(), &[1; 16]);
        assert_eq!(active[1].identifier().as_bytes(), &[2; 16]);
        assert_eq!(
            persistence.projection_bytes(&active[0]),
            b"first-original-bytes"
        );

        let byte_different_original = AuditTerminalObligation::from_persisted(
            &persistence,
            [1; 16],
            b"byte-different-original".to_vec(),
            binding(&persistence, 7, 3),
        )
        .unwrap();
        let byte_different_supersession = AuditTerminalSupersession::from_server_audit(
            &persistence,
            &byte_different_original,
            b"opaque-fixed-disposition".to_vec(),
            binding(&persistence, 7, 3),
            binding(&persistence, 8, 1),
            write(&persistence, 3, b"replacement-original-bytes", 8, 1),
        )
        .unwrap();
        assert_eq!(
            append(&mut database, &byte_different_supersession),
            Err(DatabaseError::InvalidState)
        );
        assert!(list_late(&mut database, &persistence).is_empty());

        let replacement = write(&persistence, 3, b"replacement-original-bytes", 8, 1);
        let supersession = AuditTerminalSupersession::from_server_audit(
            &persistence,
            &active[0],
            b"opaque-fixed-disposition".to_vec(),
            binding(&persistence, 7, 3),
            binding(&persistence, 8, 1),
            replacement,
        )
        .unwrap();
        append(&mut database, &supersession).unwrap();
        append(&mut database, &supersession).unwrap();

        let active = list_active(&mut database, &persistence);
        let late = list_late(&mut database, &persistence);
        assert_eq!(
            active
                .iter()
                .map(AuditTerminalObligation::identifier)
                .collect::<Vec<_>>(),
            vec![
                weavelit_server_database::AuditTerminalObligationIdentifier::from_persisted(
                    &persistence,
                    [2; 16]
                )
                .unwrap(),
                weavelit_server_database::AuditTerminalObligationIdentifier::from_persisted(
                    &persistence,
                    [3; 16]
                )
                .unwrap(),
            ]
        );
        assert_eq!(late.len(), 1);
        assert_eq!(
            persistence.projection_bytes(&late[0]),
            b"first-original-bytes"
        );

        let mismatched = AuditTerminalSupersession::from_server_audit(
            &persistence,
            &late[0],
            b"different-disposition".to_vec(),
            binding(&persistence, 7, 3),
            binding(&persistence, 8, 1),
            write(&persistence, 3, b"replacement-original-bytes", 8, 1),
        )
        .unwrap();
        assert_eq!(
            append(&mut database, &mismatched),
            Err(DatabaseError::InvalidState)
        );

        let third_ack = AuditTerminalAcknowledgementProof::from_server_audit(
            &persistence,
            [3; 16],
            binding(&persistence, 8, 1),
        )
        .unwrap();
        assert_eq!(
            database.acknowledge_audit_terminal_obligation(third_ack),
            Err(DatabaseError::InvalidState)
        );
        for (identifier, binding) in [
            ([2; 16], binding(&persistence, 7, 3)),
            ([1; 16], binding(&persistence, 7, 3)),
            ([3; 16], binding(&persistence, 8, 1)),
        ] {
            database
                .acknowledge_audit_terminal_obligation(
                    AuditTerminalAcknowledgementProof::from_server_audit(
                        &persistence,
                        identifier,
                        binding,
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        assert!(list_active(&mut database, &persistence).is_empty());
        assert!(list_late(&mut database, &persistence).is_empty());

        persist(&mut database, &first).unwrap();
        assert!(list_active(&mut database, &persistence).is_empty());
        append(&mut database, &supersession).unwrap();
    }

    #[test]
    fn transaction_failure_rolls_back_obligation_and_supersession_together() {
        let (_directory, _path, mut database) = database();
        let persistence = persistence();
        let original = write(&persistence, 4, b"original", 9, 1);
        {
            let mut transaction =
                SqliteAuditTerminalRecoveryTransaction::begin(&mut database).unwrap();
            transaction
                .persist_audit_terminal_obligation(&original)
                .unwrap();
        }
        assert!(list_active(&mut database, &persistence).is_empty());

        persist(&mut database, &original).unwrap();
        let stored_original = list_active(&mut database, &persistence).remove(0);
        let supersession = AuditTerminalSupersession::from_server_audit(
            &persistence,
            &stored_original,
            b"disposition".to_vec(),
            binding(&persistence, 9, 1),
            binding(&persistence, 10, 1),
            write(&persistence, 5, b"replacement", 10, 1),
        )
        .unwrap();
        database
            .connection
            .execute_batch(
                "CREATE TEMP TRIGGER fail_audit_terminal_supersession \
                 BEFORE INSERT ON main.weavelit_audit_terminal_supersession \
                 BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
            )
            .unwrap();
        assert_eq!(
            append(&mut database, &supersession),
            Err(DatabaseError::IntegrityFailure)
        );
        database
            .connection
            .execute_batch("DROP TRIGGER fail_audit_terminal_supersession")
            .unwrap();
        let active = list_active(&mut database, &persistence);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0], stored_original);
        assert!(list_late(&mut database, &persistence).is_empty());
    }

    #[test]
    fn malformed_opaque_rows_fail_closed_without_field_parsing() {
        let (_directory, _path, mut database) = database();
        let persistence = persistence();
        let obligation = write(&persistence, 6, b"opaque-not-json", 11, u64::MAX);
        persist(&mut database, &obligation).unwrap();
        assert_eq!(
            persistence.projection_bytes(&list_active(&mut database, &persistence)[0]),
            b"opaque-not-json"
        );

        database
            .connection
            .execute_batch(
                "DROP TRIGGER weavelit_audit_terminal_obligation_reject_rewrite; \
                 PRAGMA ignore_check_constraints = ON; \
                 UPDATE weavelit_audit_terminal_obligation SET projection = X''; \
                 PRAGMA ignore_check_constraints = OFF;",
            )
            .unwrap();
        assert_eq!(
            database.list_pending_audit_terminal_obligations(
                &persistence,
                AuditTerminalReplayBatchSize::new(1).unwrap(),
            ),
            Err(DatabaseError::IntegrityFailure)
        );
    }

    #[test]
    fn malformed_disposition_rows_fail_closed_without_parsing() {
        let (_directory, _path, mut database) = database();
        let persistence = persistence();
        persist(&mut database, &write(&persistence, 7, b"original", 12, 1)).unwrap();
        let original = list_active(&mut database, &persistence).remove(0);
        let supersession = AuditTerminalSupersession::from_server_audit(
            &persistence,
            &original,
            b"opaque-disposition".to_vec(),
            binding(&persistence, 12, 1),
            binding(&persistence, 13, 1),
            write(&persistence, 8, b"replacement", 13, 1),
        )
        .unwrap();
        append(&mut database, &supersession).unwrap();

        database
            .connection
            .execute_batch(
                "DROP TRIGGER weavelit_audit_terminal_supersession_reject_update; \
                 PRAGMA ignore_check_constraints = ON; \
                 UPDATE weavelit_audit_terminal_supersession SET disposition = X''; \
                 PRAGMA ignore_check_constraints = OFF;",
            )
            .unwrap();
        assert_eq!(
            database.list_late_delivery_audit_terminal_obligations(
                &persistence,
                AuditTerminalReplayBatchSize::new(1).unwrap(),
            ),
            Err(DatabaseError::IntegrityFailure)
        );
    }

    #[test]
    fn application_database_exposes_the_private_recovery_store() {
        let (_directory, _path, mut database) = database();
        assert!(ApplicationDatabase::audit_terminal_recovery(&mut database).is_some());
    }

    #[test]
    fn normalized_state_and_backup_paths_do_not_reference_recovery_tables() {
        assert!(!include_str!("state.rs").contains("weavelit_audit_terminal_"));
        assert!(
            !include_str!("../migrations/0003_create_application_state.sql")
                .contains("weavelit_audit_terminal_")
        );
    }

    #[test]
    fn same_identifier_version_bump_v1_to_v2_succeeds_as_full_binding_tuple() {
        let (_directory, _path, mut database) = database();
        let persistence = persistence();

        // Create original obligation with identifier 0x42, binding with version 1
        let original_write = write(&persistence, 0x42, b"original-v1", 0x51, 1);
        persist(&mut database, &original_write).unwrap();

        // Load it back
        let original = list_active(&mut database, &persistence)
            .into_iter()
            .find(|o| o.identifier().as_bytes() == &[0x42; 16])
            .expect("original obligation must be present");

        // Create replacement with same binding identifier but higher version
        let replacement_write = write(&persistence, 0x43, b"replacement-v2", 0x51, 2);
        let supersession = AuditTerminalSupersession::from_server_audit(
            &persistence,
            &original,
            b"disposition-bytes".to_vec(),
            binding(&persistence, 0x51, 1), // original: binding id 0x51, version 1
            binding(&persistence, 0x51, 2), // replacement: same id, version 2
            replacement_write,
        )
        .expect("supersession creation should succeed with same-id version bump");

        // Append supersession must succeed (validates full binding tuple distinctness)
        append(&mut database, &supersession).unwrap();

        // List late delivery should return the original
        let late = list_late(&mut database, &persistence);
        assert_eq!(late.len(), 1);
        assert_eq!(late[0].identifier().as_bytes(), &[0x42; 16]);

        // Acknowledge the late obligation must succeed
        let proof = AuditTerminalAcknowledgementProof::from_server_audit(
            &persistence,
            [0x42; 16],
            binding(&persistence, 0x51, 1),
        )
        .unwrap();
        database
            .acknowledge_audit_terminal_obligation(proof)
            .unwrap();

        // Verify acknowledgement succeeded
        let late_after = list_late(&mut database, &persistence);
        assert_eq!(late_after.len(), 0);
    }
}
