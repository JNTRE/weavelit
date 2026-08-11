use rusqlite::{Connection, OptionalExtension as _, TransactionBehavior, params};
use weavelit_server_database::{
    DatabaseError, MfaAcceptance, MfaStore, MfaTimeStep, StateIdentifier,
};

use crate::SqliteDatabase;
use crate::error::{ErrorContext, map_sqlite_error};

const SELECT_WATERMARK: &str =
    "SELECT accepted_step FROM weavelit_mfa_replay_watermark WHERE factor_id = ?1";
/// Compares and writes the watermark in one statement.
///
/// The conflict clause performs the strictly-greater comparison inside the
/// same statement that would write, so there is no point between reading the
/// stored step and advancing it at which a concurrent presentation of the same
/// code could observe the earlier value.
const ADVANCE_WATERMARK: &str = "INSERT INTO weavelit_mfa_replay_watermark \
     (factor_id, accepted_step) VALUES (?1, ?2) \
     ON CONFLICT (factor_id) DO UPDATE SET accepted_step = excluded.accepted_step \
     WHERE excluded.accepted_step > weavelit_mfa_replay_watermark.accepted_step";
const DELETE_EVERY_WATERMARK: &str = "DELETE FROM weavelit_mfa_replay_watermark";

/// Removes every replay watermark.
///
/// This runs inside the caller's transaction. Checkpoint replacement calls it
/// so a Restore's watermark clearing commits or rolls back with the state
/// replacement itself rather than as a separate step.
pub(super) fn clear(connection: &Connection) -> Result<(), DatabaseError> {
    connection
        .execute(DELETE_EVERY_WATERMARK, [])
        .map(|_| ())
        .map_err(|error| map_sqlite_error(error, ErrorContext::Mfa))
}

impl MfaStore for SqliteDatabase {
    fn accepted_step(
        &mut self,
        factor: StateIdentifier,
    ) -> Result<Option<MfaTimeStep>, DatabaseError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| map_sqlite_error(error, ErrorContext::Mfa))?;
        let stored: Option<i64> = transaction
            .query_row(
                SELECT_WATERMARK,
                params![factor.as_bytes().as_slice()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| map_sqlite_error(error, ErrorContext::Mfa))?;
        transaction
            .commit()
            .map_err(|error| map_sqlite_error(error, ErrorContext::Mfa))?;

        stored.map(step).transpose()
    }

    fn accept_step(
        &mut self,
        factor: StateIdentifier,
        step: MfaTimeStep,
    ) -> Result<MfaAcceptance, DatabaseError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| map_sqlite_error(error, ErrorContext::Mfa))?;
        let written = transaction
            .execute(
                ADVANCE_WATERMARK,
                params![factor.as_bytes().as_slice(), step.as_stored()],
            )
            .map_err(|error| map_sqlite_error(error, ErrorContext::Mfa))?;
        transaction
            .commit()
            .map_err(|error| map_sqlite_error(error, ErrorContext::Mfa))?;

        if written == 0 {
            return Ok(MfaAcceptance::Replayed);
        }

        Ok(MfaAcceptance::Accepted)
    }
}

fn step(stored: i64) -> Result<MfaTimeStep, DatabaseError> {
    let stored = u64::try_from(stored).map_err(|_| DatabaseError::IntegrityFailure)?;

    MfaTimeStep::from_step(stored).map_err(|_| DatabaseError::IntegrityFailure)
}
