use rusqlite::{OptionalExtension as _, params};
use weavelit_server_database::{
    DatabaseError, RECONCILIATION_DIGEST_LENGTH, ReconciliationDigest, ReconciliationStore,
};

use crate::SqliteDatabase;
use crate::error::{ErrorContext, map_sqlite_error};

const SELECT_DIGEST: &str = "SELECT digest FROM weavelit_lifecycle_reconciliation \
     WHERE singleton = 1";
const REPLACE_DIGEST: &str = "INSERT INTO weavelit_lifecycle_reconciliation (singleton, digest) \
     VALUES (1, ?1) ON CONFLICT(singleton) DO UPDATE SET digest = excluded.digest";

/// Replaces the completed deployment's reconciliation digest inside the
/// caller's checkpoint-completion transaction.
pub(super) fn replace(
    connection: &rusqlite::Connection,
    digest: &ReconciliationDigest,
) -> Result<(), DatabaseError> {
    connection
        .execute(REPLACE_DIGEST, params![digest.as_bytes().as_slice()])
        .map(|_| ())
        .map_err(|error| map_sqlite_error(error, ErrorContext::Reconciliation))
}

impl ReconciliationStore for SqliteDatabase {
    fn matches_reconciliation(
        &mut self,
        digest: &ReconciliationDigest,
    ) -> Result<bool, DatabaseError> {
        let stored = self
            .connection
            .query_row(SELECT_DIGEST, [], |row| row.get::<_, Vec<u8>>(0))
            .optional()
            .map_err(|error| map_sqlite_error(error, ErrorContext::Reconciliation))?;
        let Some(stored) = stored else {
            return Ok(false);
        };
        let bytes: [u8; RECONCILIATION_DIGEST_LENGTH] = stored
            .try_into()
            .map_err(|_| DatabaseError::IntegrityFailure)?;
        let stored = ReconciliationDigest::from_bytes(bytes);
        Ok(stored.matches(digest))
    }
}
