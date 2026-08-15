use rusqlite::{Connection, OptionalExtension as _, TransactionBehavior, params};
use weavelit_server_database::{
    COMPONENT_ENABLED_VALUE, DatabaseError, MfaAcceptance, MfaEnablementOutcome, MfaEnrollment,
    MfaFactor, MfaModuleTarget, MfaStore, MfaTimeStep, StateIdentifier,
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
/// Writes the factor only when its account holds no factor for that module.
///
/// The uniqueness the schema already declares is restated as a conflict clause
/// rather than left to raise, so a second enrollment for the same module is a
/// reported outcome instead of an integrity failure indistinguishable from a
/// corrupted database.
const INSERT_FACTOR: &str = "INSERT INTO weavelit_mfa_factor \
     (factor_id, account_id, module, protected_factor_data) VALUES (?1, ?2, ?3, ?4) \
     ON CONFLICT (account_id, module) DO NOTHING";
const COUNT_ENROLLED_ACCOUNTS: &str =
    "SELECT COUNT(DISTINCT account_id) FROM weavelit_mfa_factor WHERE module = ?1";
const SELECT_ENABLEMENT: &str = "SELECT setting_value FROM weavelit_configuration \
     WHERE component = ?1 AND setting_key = ?2";
const SET_ENABLEMENT: &str = "INSERT INTO weavelit_configuration \
     (component, setting_key, setting_value) VALUES (?1, ?2, ?3) \
     ON CONFLICT (component, setting_key) DO UPDATE SET setting_value = excluded.setting_value";
/// Removes the live sessions of every account enrolled in one module.
const REVOKE_ENROLLED_SESSIONS: &str = "DELETE FROM weavelit_session WHERE account_id IN \
     (SELECT account_id FROM weavelit_mfa_factor WHERE module = ?1)";

/// The configuration key one MFA Module's enabled state is stored under.
///
/// The key is owned by the module's own configuration component, so the entry
/// that disables the TOTP Module belongs to `mfa.totp`.
const MODULE_ENABLED_KEY: &str = "enabled";

/// The stored value that leaves an MFA Module disabled.
///
/// Only [`COMPONENT_ENABLED_VALUE`] enables a module, so this value is written
/// for clarity rather than recognized: a reader treats every value that is not
/// the enabled one, and a missing entry, as disabled.
const MODULE_DISABLED_VALUE: &str = "false";

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

    fn enroll(
        &mut self,
        target: &MfaModuleTarget,
        factor: &MfaFactor,
        accepted_step: MfaTimeStep,
    ) -> Result<MfaEnrollment, DatabaseError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| map_sqlite_error(error, ErrorContext::Mfa))?;
        // The enablement read and the two writes share this one transaction,
        // so a module disabled between opening the enrollment and confirming
        // it cannot have a factor written against it, and no session can be
        // issued behind one.
        let enabled: Option<String> = transaction
            .query_row(
                SELECT_ENABLEMENT,
                params![target.component.as_str(), MODULE_ENABLED_KEY],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| map_sqlite_error(error, ErrorContext::Mfa))?;
        // Rolled back rather than committed, so a disabled module writes nothing.
        if enabled.as_deref() != Some(COMPONENT_ENABLED_VALUE) {
            return Ok(MfaEnrollment::ModuleDisabled);
        }
        let written = transaction
            .execute(
                INSERT_FACTOR,
                params![
                    factor.identifier.as_bytes().as_slice(),
                    factor.account.as_bytes().as_slice(),
                    factor.module.as_str(),
                    factor.protected_factor_data.as_bytes(),
                ],
            )
            .map_err(|error| map_sqlite_error(error, ErrorContext::Mfa))?;
        // Rolled back rather than committed, so an account that already holds a
        // factor keeps the watermark belonging to that factor.
        if written == 0 {
            return Ok(MfaEnrollment::AlreadyEnrolled);
        }
        transaction
            .execute(
                ADVANCE_WATERMARK,
                params![
                    factor.identifier.as_bytes().as_slice(),
                    accepted_step.as_stored()
                ],
            )
            .map_err(|error| map_sqlite_error(error, ErrorContext::Mfa))?;
        transaction
            .commit()
            .map_err(|error| map_sqlite_error(error, ErrorContext::Mfa))?;

        Ok(MfaEnrollment::Enrolled)
    }

    fn set_module_enabled(
        &mut self,
        target: &MfaModuleTarget,
        enabled: bool,
        expected_enrolled: usize,
    ) -> Result<MfaEnablementOutcome, DatabaseError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| map_sqlite_error(error, ErrorContext::Mfa))?;
        let enrolled: i64 = transaction
            .query_row(
                COUNT_ENROLLED_ACCOUNTS,
                params![target.module.as_str()],
                |row| row.get(0),
            )
            .map_err(|error| map_sqlite_error(error, ErrorContext::Mfa))?;
        let enrolled = usize::try_from(enrolled).map_err(|_| DatabaseError::IntegrityFailure)?;
        // Rolled back rather than committed, so a stale preview writes nothing.
        if enrolled != expected_enrolled {
            return Ok(MfaEnablementOutcome::EnrolledCountChanged { enrolled });
        }

        let value = if enabled {
            COMPONENT_ENABLED_VALUE
        } else {
            MODULE_DISABLED_VALUE
        };
        transaction
            .execute(
                SET_ENABLEMENT,
                params![target.component.as_str(), MODULE_ENABLED_KEY, value],
            )
            .map_err(|error| map_sqlite_error(error, ErrorContext::Mfa))?;
        let revoked_sessions = if enabled {
            0
        } else {
            transaction
                .execute(REVOKE_ENROLLED_SESSIONS, params![target.module.as_str()])
                .map_err(|error| map_sqlite_error(error, ErrorContext::Mfa))?
        };
        transaction
            .commit()
            .map_err(|error| map_sqlite_error(error, ErrorContext::Mfa))?;

        Ok(MfaEnablementOutcome::Applied { revoked_sessions })
    }
}

fn step(stored: i64) -> Result<MfaTimeStep, DatabaseError> {
    let stored = u64::try_from(stored).map_err(|_| DatabaseError::IntegrityFailure)?;

    MfaTimeStep::from_step(stored).map_err(|_| DatabaseError::IntegrityFailure)
}
