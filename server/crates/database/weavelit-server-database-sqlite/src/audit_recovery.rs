use rusqlite::{OptionalExtension as _, Transaction, TransactionBehavior, params};
use weavelit_server_database::{
    AUDIT_TERMINAL_OBLIGATION_IDENTIFIER_LENGTH, AuditTerminalObligation,
    AuditTerminalRecoveryPersistence, AuditTerminalRecoveryStore, AuditTerminalRecoveryTransaction,
    AuditTerminalReplayBatchSize, AuditTerminalSupersession, DatabaseError,
};
use weavelit_server_log::{
    AuditDestinationBinding, AuditTerminalDeliveryAcknowledgement, AuditTerminalRecoveryProjection,
    AuditTerminalSupersessionDisposition, TrustedRecordIssuer,
};
use weavelit_server_log_authority::ServerLogAuthority;

use crate::SqliteDatabase;
use crate::error::{ErrorContext, map_sqlite_error};

const INSERT_OBLIGATION: &str = "INSERT INTO weavelit_audit_terminal_outbox \
     (obligation_identifier, projection, binding_identifier, binding_version) \
     VALUES (?1, ?2, ?3, ?4)";
const INSERT_SUPERSESSION: &str = "INSERT INTO weavelit_audit_terminal_supersession \
     (original_obligation_identifier, disposition, replacement_obligation_identifier) \
     VALUES (?1, ?2, ?3)";
const SELECT_ACTIVE: &str = "SELECT outbox.sequence_number, outbox.obligation_identifier, \
     outbox.projection, outbox.binding_identifier, outbox.binding_version, NULL \
     FROM weavelit_audit_terminal_outbox AS outbox \
     LEFT JOIN weavelit_audit_terminal_supersession AS supersession \
     ON supersession.original_obligation_identifier = outbox.obligation_identifier \
     WHERE supersession.original_obligation_identifier IS NULL \
     ORDER BY outbox.sequence_number ASC LIMIT ?1";
const SELECT_LATE: &str = "SELECT outbox.sequence_number, outbox.obligation_identifier, \
     outbox.projection, outbox.binding_identifier, outbox.binding_version, \
     supersession.disposition \
     FROM weavelit_audit_terminal_outbox AS outbox \
     INNER JOIN weavelit_audit_terminal_supersession AS supersession \
     ON supersession.original_obligation_identifier = outbox.obligation_identifier \
     ORDER BY outbox.sequence_number ASC LIMIT ?1";
const DELETE_OBLIGATION: &str = "DELETE FROM weavelit_audit_terminal_outbox \
     WHERE sequence_number = ?1 AND obligation_identifier = ?2";
const BEGIN_SUPERSESSION_SAVEPOINT: &str = "SAVEPOINT weavelit_audit_terminal_supersession_write";
const COMMIT_SUPERSESSION_SAVEPOINT: &str =
    "RELEASE SAVEPOINT weavelit_audit_terminal_supersession_write";
const ROLLBACK_SUPERSESSION_SAVEPOINT: &str = "ROLLBACK TO SAVEPOINT \
    weavelit_audit_terminal_supersession_write; \
    RELEASE SAVEPOINT weavelit_audit_terminal_supersession_write";

type StoredRow = (i64, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, Option<Vec<u8>>);

/// Transaction-scoped adapter used by future authoritative mutation stores.
///
/// The caller owns the surrounding immediate transaction and its business
/// mutation. This adapter never commits, so the mutation and terminal recovery
/// write become durable together or roll back together.
pub(super) struct AuditTerminalRecoveryTransactionAdapter<'transaction, 'connection> {
    transaction: &'transaction Transaction<'connection>,
}

impl<'transaction, 'connection> AuditTerminalRecoveryTransactionAdapter<'transaction, 'connection> {
    pub(super) const fn new(transaction: &'transaction Transaction<'connection>) -> Self {
        Self { transaction }
    }

    fn acknowledge(
        &mut self,
        acknowledgement: AuditTerminalDeliveryAcknowledgement,
    ) -> Result<(), DatabaseError> {
        let eligible = match load_first_matching(
            self.transaction,
            SELECT_ACTIVE,
            acknowledgement.record_id(),
        )? {
            Some(active) => active,
            None => {
                load_first_matching(self.transaction, SELECT_LATE, acknowledgement.record_id())?
                    .ok_or(DatabaseError::InvalidState)?
            }
        };
        if !acknowledgement.matches(&eligible.identifier, &eligible.binding) {
            return Err(DatabaseError::InvalidState);
        }
        let deleted = self
            .transaction
            .execute(
                DELETE_OBLIGATION,
                params![eligible.sequence, eligible.identifier.as_slice()],
            )
            .map_err(|error| map_sqlite_error(error, ErrorContext::AuditTerminal))?;
        if deleted != 1 {
            return Err(DatabaseError::IntegrityFailure);
        }
        Ok(())
    }
}

impl AuditTerminalRecoveryTransaction for AuditTerminalRecoveryTransactionAdapter<'_, '_> {
    fn persist_audit_terminal_obligation(
        &mut self,
        obligation: &AuditTerminalObligation,
    ) -> Result<(), DatabaseError> {
        let binding = validate_input_obligation(obligation)?;
        if obligation_exists(self.transaction, obligation.identifier().as_bytes())? {
            return Err(DatabaseError::InvalidState);
        }
        insert_obligation(self.transaction, obligation, &binding)
    }

    fn append_audit_terminal_supersession(
        &mut self,
        supersession: &AuditTerminalSupersession,
    ) -> Result<(), DatabaseError> {
        let original = supersession.original_obligation();
        let replacement = supersession.replacement_obligation();
        let replacement_binding = validate_input_obligation(replacement)?;
        let disposition = supersession.disposition();
        validate_input_disposition(disposition)?;

        let Some(stored_original) = load_first(self.transaction, SELECT_ACTIVE)? else {
            return Err(DatabaseError::InvalidState);
        };
        if stored_original.identifier != *original.identifier().as_bytes()
            || stored_original.projection.as_slice() != original.projection()
            || stored_original.binding != *disposition.original_binding()
            || replacement_binding != *disposition.replacement_binding()
            || obligation_exists(self.transaction, replacement.identifier().as_bytes())?
        {
            return Err(DatabaseError::InvalidState);
        }

        self.transaction
            .execute_batch(BEGIN_SUPERSESSION_SAVEPOINT)
            .map_err(|error| map_sqlite_error(error, ErrorContext::AuditTerminal))?;
        let write_result = insert_obligation(self.transaction, replacement, &replacement_binding)
            .and_then(|()| {
                self.transaction
                    .execute(
                        INSERT_SUPERSESSION,
                        params![
                            original.identifier().as_bytes().as_slice(),
                            disposition.as_bytes(),
                            replacement.identifier().as_bytes().as_slice(),
                        ],
                    )
                    .map(|_| ())
                    .map_err(|error| map_sqlite_error(error, ErrorContext::AuditTerminal))
            });
        match write_result {
            Ok(()) => match self
                .transaction
                .execute_batch(COMMIT_SUPERSESSION_SAVEPOINT)
            {
                Ok(()) => Ok(()),
                Err(release_error) => {
                    let error = map_sqlite_error(release_error, ErrorContext::AuditTerminal);
                    self.transaction
                        .execute_batch(ROLLBACK_SUPERSESSION_SAVEPOINT)
                        .map_err(|rollback_error| {
                            map_sqlite_error(rollback_error, ErrorContext::AuditTerminal)
                        })?;
                    Err(error)
                }
            },
            Err(error) => {
                self.transaction
                    .execute_batch(ROLLBACK_SUPERSESSION_SAVEPOINT)
                    .map_err(|rollback_error| {
                        map_sqlite_error(rollback_error, ErrorContext::AuditTerminal)
                    })?;
                Err(error)
            }
        }
    }
}

impl AuditTerminalRecoveryStore for SqliteDatabase {
    fn list_pending_audit_terminal_obligations(
        &mut self,
        persistence: &AuditTerminalRecoveryPersistence,
        batch_size: AuditTerminalReplayBatchSize,
    ) -> Result<Vec<AuditTerminalObligation>, DatabaseError> {
        list(&self.connection, SELECT_ACTIVE, persistence, batch_size)
    }

    fn list_late_delivery_audit_terminal_obligations(
        &mut self,
        persistence: &AuditTerminalRecoveryPersistence,
        batch_size: AuditTerminalReplayBatchSize,
    ) -> Result<Vec<AuditTerminalObligation>, DatabaseError> {
        list(&self.connection, SELECT_LATE, persistence, batch_size)
    }

    fn acknowledge_audit_terminal_obligation(
        &mut self,
        acknowledgement: AuditTerminalDeliveryAcknowledgement,
    ) -> Result<(), DatabaseError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| map_sqlite_error(error, ErrorContext::AuditTerminal))?;
        AuditTerminalRecoveryTransactionAdapter::new(&transaction).acknowledge(acknowledgement)?;
        transaction
            .commit()
            .map_err(|error| map_sqlite_error(error, ErrorContext::AuditTerminal))
    }
}

struct StoredObligation {
    sequence: i64,
    identifier: [u8; AUDIT_TERMINAL_OBLIGATION_IDENTIFIER_LENGTH],
    projection: Vec<u8>,
    binding: AuditDestinationBinding,
}

fn list(
    connection: &rusqlite::Connection,
    sql: &str,
    persistence: &AuditTerminalRecoveryPersistence,
    batch_size: AuditTerminalReplayBatchSize,
) -> Result<Vec<AuditTerminalObligation>, DatabaseError> {
    let limit = i64::try_from(batch_size.get()).map_err(|_| DatabaseError::IntegrityFailure)?;
    let rows = load_rows(connection, sql, limit)?;
    rows.into_iter()
        .map(|row| {
            let stored = decode_stored(row)?;
            AuditTerminalObligation::from_persisted(
                persistence,
                stored.identifier,
                stored.projection,
            )
            .map_err(|_| DatabaseError::IntegrityFailure)
        })
        .collect()
}

fn load_first(
    transaction: &Transaction<'_>,
    sql: &str,
) -> Result<Option<StoredObligation>, DatabaseError> {
    load_rows(transaction, sql, 1)?
        .into_iter()
        .next()
        .map(decode_stored)
        .transpose()
}

fn load_first_matching(
    transaction: &Transaction<'_>,
    sql: &str,
    record_identifier: &[u8; AUDIT_TERMINAL_OBLIGATION_IDENTIFIER_LENGTH],
) -> Result<Option<StoredObligation>, DatabaseError> {
    let Some(row) = load_rows(transaction, sql, 1)?.into_iter().next() else {
        return Ok(None);
    };
    if row.1.as_slice() != record_identifier {
        return Ok(None);
    }
    decode_stored(row).map(Some)
}

fn load_rows(
    connection: &rusqlite::Connection,
    sql: &str,
    limit: i64,
) -> Result<Vec<StoredRow>, DatabaseError> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|error| map_sqlite_error(error, ErrorContext::AuditTerminal))?;
    statement
        .query_map([limit], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })
        .map_err(|error| map_sqlite_error(error, ErrorContext::AuditTerminal))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| map_sqlite_error(error, ErrorContext::AuditTerminal))
}

fn decode_stored(row: StoredRow) -> Result<StoredObligation, DatabaseError> {
    let (sequence, identifier, projection, binding_identifier, binding_version, disposition) = row;
    let identifier = identifier_bytes(&identifier)?;
    let binding = decode_binding(&binding_identifier, &binding_version)?;
    let recovered_binding =
        validate_projection(&identifier, &projection, DatabaseError::IntegrityFailure)?;
    if binding != recovered_binding {
        return Err(DatabaseError::IntegrityFailure);
    }
    if let Some(disposition) = disposition {
        let disposition = AuditTerminalSupersessionDisposition::from_persisted(disposition)
            .map_err(|_| DatabaseError::IntegrityFailure)?;
        if disposition.original_record_id() != &identifier
            || disposition.original_binding() != &binding
        {
            return Err(DatabaseError::IntegrityFailure);
        }
    }
    Ok(StoredObligation {
        sequence,
        identifier,
        projection,
        binding,
    })
}

fn validate_input_obligation(
    obligation: &AuditTerminalObligation,
) -> Result<AuditDestinationBinding, DatabaseError> {
    validate_projection(
        obligation.identifier().as_bytes(),
        obligation.projection(),
        DatabaseError::InvalidState,
    )
}

fn validate_projection(
    identifier: &[u8; AUDIT_TERMINAL_OBLIGATION_IDENTIFIER_LENGTH],
    projection: &[u8],
    invalid: DatabaseError,
) -> Result<AuditDestinationBinding, DatabaseError> {
    let authority = ServerLogAuthority::new();
    let issuer = TrustedRecordIssuer::from_server_authority(&authority);
    let terminal = AuditTerminalRecoveryProjection::from_persisted(projection.to_vec())
        .and_then(|projection| projection.restore(&issuer))
        .map_err(|_| invalid)?;
    if terminal.record().record_id().as_bytes() != identifier {
        return Err(invalid);
    }
    Ok(terminal.binding().clone())
}

fn validate_input_disposition(
    disposition: &AuditTerminalSupersessionDisposition,
) -> Result<(), DatabaseError> {
    let decoded =
        AuditTerminalSupersessionDisposition::from_persisted(disposition.as_bytes().to_vec())
            .map_err(|_| DatabaseError::InvalidState)?;
    if decoded.original_record_id() != disposition.original_record_id()
        || decoded.original_binding() != disposition.original_binding()
        || decoded.replacement_binding() != disposition.replacement_binding()
    {
        return Err(DatabaseError::InvalidState);
    }
    Ok(())
}

fn insert_obligation(
    transaction: &Transaction<'_>,
    obligation: &AuditTerminalObligation,
    binding: &AuditDestinationBinding,
) -> Result<(), DatabaseError> {
    transaction
        .execute(
            INSERT_OBLIGATION,
            params![
                obligation.identifier().as_bytes().as_slice(),
                obligation.projection(),
                binding.identifier().as_slice(),
                binding.version().to_be_bytes().as_slice(),
            ],
        )
        .map_err(|error| map_sqlite_error(error, ErrorContext::AuditTerminal))?;
    Ok(())
}

fn obligation_exists(
    transaction: &Transaction<'_>,
    identifier: &[u8; AUDIT_TERMINAL_OBLIGATION_IDENTIFIER_LENGTH],
) -> Result<bool, DatabaseError> {
    transaction
        .query_row(
            "SELECT 1 FROM (\
                 SELECT obligation_identifier AS retained_identifier \
                 FROM weavelit_audit_terminal_outbox \
                 UNION ALL \
                 SELECT original_obligation_identifier \
                 FROM weavelit_audit_terminal_supersession \
                 UNION ALL \
                 SELECT replacement_obligation_identifier \
                 FROM weavelit_audit_terminal_supersession\
             ) WHERE retained_identifier = ?1 LIMIT 1",
            [identifier.as_slice()],
            |_| Ok(()),
        )
        .optional()
        .map(|value| value.is_some())
        .map_err(|error| map_sqlite_error(error, ErrorContext::AuditTerminal))
}

fn decode_binding(
    identifier: &[u8],
    version: &[u8],
) -> Result<AuditDestinationBinding, DatabaseError> {
    let identifier = identifier_bytes(identifier)?;
    let version = u64::from_be_bytes(
        version
            .try_into()
            .map_err(|_| DatabaseError::IntegrityFailure)?,
    );
    AuditDestinationBinding::from_server_authority(&ServerLogAuthority::new(), identifier, version)
        .map_err(|_| DatabaseError::IntegrityFailure)
}

fn identifier_bytes(
    bytes: &[u8],
) -> Result<[u8; AUDIT_TERMINAL_OBLIGATION_IDENTIFIER_LENGTH], DatabaseError> {
    bytes
        .try_into()
        .map_err(|_| DatabaseError::IntegrityFailure)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use rusqlite::{Connection, TransactionBehavior, params};
    use weavelit_server_database::{
        ApplicationDatabase, AuditTerminalRecoveryStore as _,
        AuditTerminalRecoveryTransaction as _, AuditTerminalReplayBatchSize,
    };
    use weavelit_server_database_authority::ServerDatabaseAuthority;
    use weavelit_server_log::{
        AuditDestinationBindingTransition, AuditLogBody, AuditLogClassification, AuditPrincipal,
        AuditTerminalRecoveryProjection, AuditTerminalSupersessionAuthorization,
        AuditTerminalSupersessionConfirmation, CompleteLogRecord, ConfiguredLogDestination,
        CorrelationId, DurableAcknowledgement, EventTime, LogCapabilities, LogDestination,
        LogDestinationError, LogDestinationFactory, LogModuleCatalog, LogModuleFactoryContext,
        LogModuleIdentifier, LogModuleRegistration, LogRecordType, LogResult, LogSettingsContract,
        ResolvedAuditDestination, TrustedLogModuleContext,
    };

    use super::*;

    const SENSITIVE_PROJECTION: &[u8] = b"temporary-password=never-disclose";

    fn database_path(directory: &tempfile::TempDir) -> PathBuf {
        directory
            .path()
            .canonicalize()
            .unwrap()
            .join("application.db")
    }

    fn persistence() -> AuditTerminalRecoveryPersistence {
        AuditTerminalRecoveryPersistence::from_server_authority(&ServerDatabaseAuthority::new())
    }

    fn binding(identity: u8, version: u64) -> AuditDestinationBinding {
        AuditDestinationBinding::from_server_authority(
            &ServerLogAuthority::new(),
            [identity; 16],
            version,
        )
        .unwrap()
    }

    fn obligation(
        record_identity: u8,
        retained_binding: &AuditDestinationBinding,
        detail: &str,
    ) -> AuditTerminalObligation {
        let authority = ServerLogAuthority::new();
        let issuer = TrustedRecordIssuer::from_server_authority(&authority);
        let attempt = CompleteLogRecord::audit_attempt(
            issuer
                .issue([record_identity.wrapping_add(0x40); 16])
                .unwrap(),
            EventTime::from_unix_milliseconds(10),
            CorrelationId::new("sqlite-recovery").unwrap(),
            AuditLogBody::new(
                AuditLogClassification::AuthenticationUserDisabled,
                AuditPrincipal::human("account:ar-11111111111111111111111111111111").unwrap(),
                "disable",
                "account:ar-22222222222222222222222222222222",
                "accountable action accepted",
            )
            .unwrap(),
        )
        .unwrap();
        let terminal = CompleteLogRecord::audit_completion(
            issuer.issue([record_identity; 16]).unwrap(),
            EventTime::from_unix_milliseconds(11),
            attempt.attempt_record_id().unwrap(),
            LogResult::Success,
            CorrelationId::new("sqlite-recovery").unwrap(),
            AuditLogBody::new(
                AuditLogClassification::AuthenticationUserDisabled,
                AuditPrincipal::human("account:ar-11111111111111111111111111111111").unwrap(),
                "disable",
                "account:ar-22222222222222222222222222222222",
                detail,
            )
            .unwrap(),
        )
        .unwrap();
        let projection =
            AuditTerminalRecoveryProjection::capture(&terminal, retained_binding).unwrap();
        AuditTerminalObligation::from_persisted(
            &persistence(),
            [record_identity; 16],
            projection.as_bytes().to_vec(),
        )
        .unwrap()
    }

    fn persist(database: &mut SqliteDatabase, obligations: &[&AuditTerminalObligation]) {
        let transaction = database
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        {
            let mut adapter = AuditTerminalRecoveryTransactionAdapter::new(&transaction);
            for obligation in obligations {
                adapter
                    .persist_audit_terminal_obligation(obligation)
                    .unwrap();
            }
        }
        transaction.commit().unwrap();
    }

    fn append(
        database: &mut SqliteDatabase,
        supersession: &AuditTerminalSupersession,
    ) -> Result<(), DatabaseError> {
        let transaction = database
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let result = AuditTerminalRecoveryTransactionAdapter::new(&transaction)
            .append_audit_terminal_supersession(supersession);
        if result.is_ok() {
            transaction.commit().unwrap();
        }
        result
    }

    fn listed(
        database: &mut SqliteDatabase,
        late: bool,
    ) -> Result<Vec<AuditTerminalObligation>, DatabaseError> {
        let batch = AuditTerminalReplayBatchSize::new(64).unwrap();
        if late {
            database.list_late_delivery_audit_terminal_obligations(&persistence(), batch)
        } else {
            database.list_pending_audit_terminal_obligations(&persistence(), batch)
        }
    }

    fn acknowledgement(
        obligation: &AuditTerminalObligation,
        retained_binding: &AuditDestinationBinding,
    ) -> AuditTerminalDeliveryAcknowledgement {
        let authority = ServerLogAuthority::new();
        let issuer = TrustedRecordIssuer::from_server_authority(&authority);
        let recovered =
            AuditTerminalRecoveryProjection::from_persisted(obligation.projection().to_vec())
                .unwrap()
                .restore(&issuer)
                .unwrap();
        let destination = acknowledging_destination(&authority);
        let resolved = ResolvedAuditDestination::from_server_authority(
            &authority,
            retained_binding,
            &destination,
        );
        recovered.deliver(&resolved).unwrap()
    }

    fn supersession(
        original: &AuditTerminalObligation,
        original_binding: &AuditDestinationBinding,
        disposition_replacement_binding: &AuditDestinationBinding,
        replacement: AuditTerminalObligation,
    ) -> AuditTerminalSupersession {
        let authority = ServerLogAuthority::new();
        let issuer = TrustedRecordIssuer::from_server_authority(&authority);
        let recovered =
            AuditTerminalRecoveryProjection::from_persisted(original.projection().to_vec())
                .unwrap()
                .restore(&issuer)
                .unwrap();
        assert_eq!(recovered.binding(), original_binding);
        let transition = AuditDestinationBindingTransition::from_server_authority(
            &authority,
            original_binding,
            disposition_replacement_binding,
        )
        .unwrap();
        let authorization =
            AuditTerminalSupersessionAuthorization::from_server_authority(&authority, &recovered);
        let confirmation = AuditTerminalSupersessionConfirmation::from_server_authority(
            &authority,
            &recovered,
            &transition,
            &authorization,
        )
        .unwrap();
        let destination = acknowledging_destination(&authority);
        let resolved = ResolvedAuditDestination::from_server_authority(
            &authority,
            disposition_replacement_binding,
            &destination,
        );
        let preflighted = resolved.preflight_for_terminal_supersession().unwrap();
        let disposition = AuditTerminalSupersessionDisposition::capture(
            &recovered,
            &transition,
            &authorization,
            &confirmation,
            &preflighted,
        )
        .unwrap();
        AuditTerminalSupersession::new(original, disposition, replacement).unwrap()
    }

    fn acknowledging_destination(authority: &ServerLogAuthority) -> ConfiguredLogDestination {
        let catalog = LogModuleCatalog::new(vec![LogModuleRegistration::new(
            "sqlite-recovery-test",
            LogCapabilities::new(vec![LogRecordType::Audit]).unwrap(),
            Box::new(AcknowledgingFactory),
        )])
        .unwrap();
        catalog
            .create_destination(
                &LogModuleIdentifier::new("sqlite-recovery-test").unwrap(),
                &TrustedLogModuleContext::from_server_authority(
                    authority,
                    PathBuf::from("/unused"),
                    [9; 16],
                ),
            )
            .unwrap()
    }

    fn row_count(path: &Path, table: &str) -> i64 {
        Connection::open(path)
            .unwrap()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    fn insert_corrupt_obligation(
        database: &SqliteDatabase,
        record_identity: u8,
        retained_binding: &AuditDestinationBinding,
        late: bool,
    ) {
        database
            .connection
            .execute(
                INSERT_OBLIGATION,
                params![
                    [record_identity; 16].as_slice(),
                    SENSITIVE_PROJECTION,
                    retained_binding.identifier().as_slice(),
                    retained_binding.version().to_be_bytes().as_slice(),
                ],
            )
            .unwrap();
        if late {
            database
                .connection
                .execute(
                    INSERT_SUPERSESSION,
                    params![
                        [record_identity; 16].as_slice(),
                        SENSITIVE_PROJECTION,
                        [record_identity.wrapping_add(1); 16].as_slice(),
                    ],
                )
                .unwrap();
        }
    }

    #[test]
    fn obligations_persist_in_fifo_order_and_require_exact_oldest_acknowledgement() {
        let directory = tempfile::tempdir().unwrap();
        let path = database_path(&directory);
        let original_binding = binding(0x71, 3);
        let changed_binding = binding(0x72, 1);
        let first = obligation(0x11, &original_binding, "first terminal");
        let second = obligation(0x12, &original_binding, "second terminal");
        let changed_first = obligation(0x11, &changed_binding, "first terminal");
        let mut database = SqliteDatabase::open(&path).unwrap();
        assert!(ApplicationDatabase::audit_terminal_recovery(&mut database).is_some());

        persist(&mut database, &[&first, &second]);
        assert_eq!(
            listed(&mut database, false).unwrap(),
            vec![first.clone(), second.clone()]
        );
        let stored: (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>) = Connection::open(&path)
            .unwrap()
            .query_row(
                "SELECT obligation_identifier, projection, binding_identifier, binding_version \
                 FROM weavelit_audit_terminal_outbox ORDER BY sequence_number LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(stored.0, first.identifier().as_bytes());
        assert_eq!(stored.1, first.projection());
        assert_eq!(stored.2, original_binding.identifier());
        assert_eq!(stored.3, original_binding.version().to_be_bytes());

        assert_eq!(
            database.acknowledge_audit_terminal_obligation(acknowledgement(
                &changed_first,
                &changed_binding,
            )),
            Err(DatabaseError::InvalidState)
        );
        assert_eq!(
            database
                .acknowledge_audit_terminal_obligation(
                    acknowledgement(&second, &original_binding,)
                ),
            Err(DatabaseError::InvalidState)
        );
        database
            .acknowledge_audit_terminal_obligation(acknowledgement(&first, &original_binding))
            .unwrap();
        assert_eq!(
            database
                .acknowledge_audit_terminal_obligation(acknowledgement(&first, &original_binding,)),
            Err(DatabaseError::InvalidState)
        );
        drop(database);

        let mut reopened = SqliteDatabase::open(&path).unwrap();
        assert_eq!(listed(&mut reopened, false).unwrap(), vec![second.clone()]);
        reopened
            .acknowledge_audit_terminal_obligation(acknowledgement(&second, &original_binding))
            .unwrap();
        assert!(listed(&mut reopened, false).unwrap().is_empty());
    }

    #[test]
    fn supersession_separates_active_and_late_queues_and_retains_disposition_history() {
        let directory = tempfile::tempdir().unwrap();
        let path = database_path(&directory);
        let original_binding = binding(0x81, 4);
        let replacement_binding = binding(0x82, 1);
        let original = obligation(0x21, &original_binding, "original terminal");
        let follower = obligation(0x22, &original_binding, "following terminal");
        let replacement = obligation(0x23, &replacement_binding, "supersession terminal");
        let supersession = supersession(
            &original,
            &original_binding,
            &replacement_binding,
            replacement.clone(),
        );
        let disposition_bytes = supersession.disposition().as_bytes().to_vec();
        let mut database = SqliteDatabase::open(&path).unwrap();
        persist(&mut database, &[&original, &follower]);

        append(&mut database, &supersession).unwrap();
        assert_eq!(
            listed(&mut database, false).unwrap(),
            vec![follower.clone(), replacement.clone()]
        );
        assert_eq!(listed(&mut database, true).unwrap(), vec![original.clone()]);
        assert_eq!(
            append(&mut database, &supersession),
            Err(DatabaseError::InvalidState)
        );
        drop(database);

        let mut reopened = SqliteDatabase::open(&path).unwrap();
        assert_eq!(listed(&mut reopened, true).unwrap(), vec![original.clone()]);
        let stored_disposition: Vec<u8> = Connection::open(&path)
            .unwrap()
            .query_row(
                "SELECT disposition FROM weavelit_audit_terminal_supersession",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored_disposition, disposition_bytes);
        reopened
            .acknowledge_audit_terminal_obligation(acknowledgement(&original, &original_binding))
            .unwrap();
        let transaction = reopened
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        assert_eq!(
            AuditTerminalRecoveryTransactionAdapter::new(&transaction)
                .persist_audit_terminal_obligation(&original),
            Err(DatabaseError::InvalidState)
        );
        transaction.commit().unwrap();
        assert!(listed(&mut reopened, true).unwrap().is_empty());
        assert_eq!(
            listed(&mut reopened, false).unwrap(),
            vec![follower, replacement]
        );
        assert_eq!(row_count(&path, "weavelit_audit_terminal_supersession"), 1);
    }

    #[test]
    fn corrupt_oldest_late_obligation_does_not_block_active_acknowledgement() {
        let directory = tempfile::tempdir().unwrap();
        let path = database_path(&directory);
        let retained_binding = binding(0x83, 1);
        let active = obligation(0x63, &retained_binding, "active terminal");
        let mut database = SqliteDatabase::open(&path).unwrap();
        insert_corrupt_obligation(&database, 0x61, &retained_binding, true);
        persist(&mut database, &[&active]);

        assert_eq!(
            listed(&mut database, true),
            Err(DatabaseError::IntegrityFailure)
        );
        database
            .acknowledge_audit_terminal_obligation(acknowledgement(&active, &retained_binding))
            .unwrap();

        assert!(listed(&mut database, false).unwrap().is_empty());
        assert_eq!(row_count(&path, "weavelit_audit_terminal_outbox"), 1);
    }

    #[test]
    fn corrupt_oldest_active_obligation_does_not_block_late_acknowledgement() {
        let directory = tempfile::tempdir().unwrap();
        let path = database_path(&directory);
        let original_binding = binding(0x84, 2);
        let replacement_binding = binding(0x85, 1);
        let original = obligation(0x64, &original_binding, "late terminal");
        let replacement = obligation(0x65, &replacement_binding, "replacement terminal");
        let supersession = supersession(
            &original,
            &original_binding,
            &replacement_binding,
            replacement.clone(),
        );
        let mut database = SqliteDatabase::open(&path).unwrap();
        persist(&mut database, &[&original]);
        append(&mut database, &supersession).unwrap();
        database
            .acknowledge_audit_terminal_obligation(acknowledgement(
                &replacement,
                &replacement_binding,
            ))
            .unwrap();
        insert_corrupt_obligation(&database, 0x66, &original_binding, false);

        assert_eq!(
            listed(&mut database, false),
            Err(DatabaseError::IntegrityFailure)
        );
        database
            .acknowledge_audit_terminal_obligation(acknowledgement(&original, &original_binding))
            .unwrap();

        assert!(listed(&mut database, true).unwrap().is_empty());
        assert_eq!(row_count(&path, "weavelit_audit_terminal_outbox"), 1);
    }

    #[test]
    fn invalid_supersession_and_forced_failure_roll_back_without_partial_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let path = database_path(&directory);
        let original_binding = binding(0x91, 2);
        let replacement_binding = binding(0x92, 1);
        let other_binding = binding(0x93, 1);
        let original = obligation(0x31, &original_binding, "original terminal");
        let replacement = obligation(0x32, &replacement_binding, "replacement terminal");
        let mismatched_replacement = obligation(0x33, &other_binding, "wrong replacement");
        let exact = supersession(
            &original,
            &original_binding,
            &replacement_binding,
            replacement,
        );
        let mismatch = supersession(
            &original,
            &original_binding,
            &replacement_binding,
            mismatched_replacement,
        );
        let changed_original = obligation(0x31, &original_binding, "changed original bytes");
        let changed = supersession(
            &changed_original,
            &original_binding,
            &replacement_binding,
            obligation(0x34, &replacement_binding, "changed replacement"),
        );
        let mut database = SqliteDatabase::open(&path).unwrap();
        persist(&mut database, &[&original]);

        assert_eq!(
            append(&mut database, &mismatch),
            Err(DatabaseError::InvalidState)
        );
        assert_eq!(
            append(&mut database, &changed),
            Err(DatabaseError::InvalidState)
        );
        database
            .connection
            .execute_batch(
                "CREATE TRIGGER fail_audit_terminal_disposition \
                 BEFORE INSERT ON weavelit_audit_terminal_supersession \
                 BEGIN SELECT RAISE(ABORT, 'forced rollback'); END;",
            )
            .unwrap();
        let transaction = database
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let error = AuditTerminalRecoveryTransactionAdapter::new(&transaction)
            .append_audit_terminal_supersession(&exact)
            .unwrap_err();
        assert_eq!(error, DatabaseError::IntegrityFailure);
        transaction
            .commit()
            .expect("the adapter savepoint must leave the outer transaction usable");
        assert_eq!(row_count(&path, "weavelit_audit_terminal_outbox"), 1);
        assert_eq!(row_count(&path, "weavelit_audit_terminal_supersession"), 0);
        assert_eq!(listed(&mut database, false).unwrap(), vec![original]);
    }

    #[test]
    fn transaction_adapter_rejects_duplicates_and_non_oldest_supersession_atomically() {
        let directory = tempfile::tempdir().unwrap();
        let path = database_path(&directory);
        let retained_binding = binding(0xA1, 2);
        let replacement_binding = binding(0xA2, 1);
        let blocker = obligation(0x51, &retained_binding, "oldest active terminal");
        let original = obligation(0x52, &retained_binding, "non-oldest terminal");
        let replacement = obligation(0x53, &replacement_binding, "replacement terminal");
        let supersession = supersession(
            &original,
            &retained_binding,
            &replacement_binding,
            replacement,
        );
        let mut database = SqliteDatabase::open(&path).unwrap();
        persist(&mut database, &[&blocker, &original]);
        database
            .connection
            .execute_batch("CREATE TABLE business_mutation_probe (value INTEGER NOT NULL);")
            .unwrap();

        let transaction = database
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        transaction
            .execute("INSERT INTO business_mutation_probe VALUES (1)", [])
            .unwrap();
        assert_eq!(
            AuditTerminalRecoveryTransactionAdapter::new(&transaction)
                .persist_audit_terminal_obligation(&blocker),
            Err(DatabaseError::InvalidState)
        );
        drop(transaction);
        assert_eq!(
            Connection::open(&path)
                .unwrap()
                .query_row("SELECT count(*) FROM business_mutation_probe", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            0,
            "a failed recovery enqueue must roll back its caller's business write"
        );
        assert_eq!(
            append(&mut database, &supersession),
            Err(DatabaseError::InvalidState)
        );
        assert_eq!(
            listed(&mut database, false).unwrap(),
            vec![blocker, original]
        );
        assert!(listed(&mut database, true).unwrap().is_empty());
    }

    #[test]
    fn malformed_projection_and_disposition_fail_closed_without_payload_disclosure() {
        let directory = tempfile::tempdir().unwrap();
        let path = database_path(&directory);
        let mut database = SqliteDatabase::open(&path).unwrap();
        database
            .connection
            .execute(
                INSERT_OBLIGATION,
                params![
                    [0x41_u8; 16].as_slice(),
                    SENSITIVE_PROJECTION,
                    [0xA1_u8; 16].as_slice(),
                    1_u64.to_be_bytes().as_slice(),
                ],
            )
            .unwrap();

        let error = listed(&mut database, false).unwrap_err();
        assert_eq!(error, DatabaseError::IntegrityFailure);
        assert!(!error.to_string().contains("temporary-password"));
        database
            .connection
            .execute(
                INSERT_SUPERSESSION,
                params![
                    [0x41_u8; 16].as_slice(),
                    SENSITIVE_PROJECTION,
                    [0x42_u8; 16].as_slice(),
                ],
            )
            .unwrap();
        let error = listed(&mut database, true).unwrap_err();
        assert_eq!(error, DatabaseError::IntegrityFailure);
        assert!(!format!("{error:?}").contains("temporary-password"));
    }

    struct AcknowledgingFactory;

    impl LogDestinationFactory for AcknowledgingFactory {
        fn accepted_settings(&self) -> LogSettingsContract {
            LogSettingsContract::none()
        }

        fn create(
            &self,
            _context: &LogModuleFactoryContext<'_>,
        ) -> Result<Box<dyn LogDestination>, LogDestinationError> {
            Ok(Box::new(AcknowledgingDestination))
        }
    }

    struct AcknowledgingDestination;

    impl LogDestination for AcknowledgingDestination {
        fn deliver(
            &self,
            _record: &CompleteLogRecord,
            acknowledgement: DurableAcknowledgement,
        ) -> Result<DurableAcknowledgement, LogDestinationError> {
            Ok(acknowledgement)
        }

        fn preflight(&self, _record_type: LogRecordType) -> Result<(), LogDestinationError> {
            Ok(())
        }
    }
}
