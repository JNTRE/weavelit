use rusqlite::{Connection, OptionalExtension, params};
use weavelit_server_database::{
    ConfigurationKey, ConfigurationValue, DatabaseError, LogConfigurationGeneration,
    LogConfigurationGenerationKey, LogConfigurationGenerationPersistence,
    LogConfigurationGenerationStore, LogConfigurationVersion, LogModuleSetting, LogType, Name,
    STATE_IDENTIFIER_LENGTH, StateIdentifier,
};

use crate::SqliteDatabase;
use crate::error::{ErrorContext, map_sqlite_error};

const INITIAL_VERSION: [u8; 8] = LogConfigurationVersion::INITIAL.get().to_be_bytes();
const AUDIT_ASSIGNMENT_QUERY: &str = "SELECT configuration_id FROM weavelit_log_assignment \
     WHERE log_type = 'audit'";
const CURRENT_VERSION_QUERY: &str = "SELECT generation_version \
     FROM weavelit_log_configuration_current_generation WHERE configuration_id = ?1";
const GENERATION_QUERY: &str = "SELECT configuration_id, generation_version, module, name, enabled \
     FROM weavelit_log_configuration_generation \
     WHERE configuration_id = ?1 AND generation_version = ?2";
const GENERATION_SETTINGS_QUERY: &str = "SELECT setting_key, setting_value \
     FROM weavelit_log_configuration_generation_setting \
     WHERE configuration_id = ?1 AND generation_version = ?2 ORDER BY setting_key";
const GENERATION_LOG_TYPES_QUERY: &str = "SELECT log_type \
     FROM weavelit_log_configuration_generation_log_type \
     WHERE configuration_id = ?1 AND generation_version = ?2 \
     ORDER BY CASE log_type WHEN 'system' THEN 0 WHEN 'audit' THEN 1 END";
const CURRENT_CONFIGURATION_QUERY: &str = "SELECT module, name, enabled \
     FROM weavelit_log_module_configuration WHERE configuration_id = ?1";
const CURRENT_SETTINGS_QUERY: &str = "SELECT setting_key, setting_value \
     FROM weavelit_log_module_setting WHERE configuration_id = ?1 ORDER BY setting_key";

type GenerationRow = (Vec<u8>, Vec<u8>, String, String, i64);
type ConfigurationRow = (String, String, i64);

impl SqliteDatabase {
    fn load_current_audit_generation_atomic(
        &mut self,
        persistence: &LogConfigurationGenerationPersistence,
    ) -> Result<Option<LogConfigurationGeneration>, DatabaseError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| map_sqlite_error(error, ErrorContext::State))?;
        let assigned_configuration = transaction
            .query_row(AUDIT_ASSIGNMENT_QUERY, [], |row| row.get::<_, Vec<u8>>(0))
            .optional()
            .map_err(|error| map_sqlite_error(error, ErrorContext::State))?;
        let Some(assigned_configuration) = assigned_configuration else {
            return Ok(None);
        };
        let configuration = decode_identifier(&assigned_configuration)?;
        let stored_version = transaction
            .query_row(
                CURRENT_VERSION_QUERY,
                params![configuration.as_bytes().as_slice()],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()
            .map_err(|error| map_sqlite_error(error, ErrorContext::State))?
            .ok_or(DatabaseError::IntegrityFailure)?;
        let key = persistence.key(configuration, decode_version(&stored_version)?);
        let generation = load_generation(&transaction, persistence, key)?
            .ok_or(DatabaseError::IntegrityFailure)?;

        validate_current_generation(&transaction, &generation)?;
        Ok(Some(generation))
    }

    fn load_generation_atomic(
        &mut self,
        persistence: &LogConfigurationGenerationPersistence,
        key: LogConfigurationGenerationKey,
    ) -> Result<Option<LogConfigurationGeneration>, DatabaseError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| map_sqlite_error(error, ErrorContext::State))?;
        load_generation(&transaction, persistence, key)
    }
}

impl LogConfigurationGenerationStore for SqliteDatabase {
    fn load_current_audit_log_configuration_generation(
        &mut self,
        persistence: &LogConfigurationGenerationPersistence,
    ) -> Result<Option<LogConfigurationGeneration>, DatabaseError> {
        self.load_current_audit_generation_atomic(persistence)
    }

    fn load_log_configuration_generation(
        &mut self,
        persistence: &LogConfigurationGenerationPersistence,
        key: LogConfigurationGenerationKey,
    ) -> Result<Option<LogConfigurationGeneration>, DatabaseError> {
        self.load_generation_atomic(persistence, key)
    }
}

pub(super) fn seed_initial_generations(connection: &Connection) -> Result<(), DatabaseError> {
    for statement in [
        "INSERT INTO weavelit_log_configuration_generation \
         (configuration_id, generation_version, module, name, enabled) \
         SELECT configuration_id, ?1, module, name, enabled \
         FROM weavelit_log_module_configuration",
        "INSERT INTO weavelit_log_configuration_generation_setting \
         (configuration_id, generation_version, setting_key, setting_value) \
         SELECT configuration_id, ?1, setting_key, setting_value \
         FROM weavelit_log_module_setting",
        "INSERT INTO weavelit_log_configuration_generation_log_type \
         (configuration_id, generation_version, log_type) \
         SELECT configuration_id, ?1, log_type FROM weavelit_log_assignment",
        "INSERT INTO weavelit_log_configuration_current_generation \
         (configuration_id, generation_version) \
         SELECT configuration_id, ?1 FROM weavelit_log_module_configuration",
    ] {
        connection
            .execute(statement, params![INITIAL_VERSION.as_slice()])
            .map_err(|error| map_sqlite_error(error, ErrorContext::Completion))?;
    }
    Ok(())
}

fn load_generation(
    connection: &Connection,
    persistence: &LogConfigurationGenerationPersistence,
    key: LogConfigurationGenerationKey,
) -> Result<Option<LogConfigurationGeneration>, DatabaseError> {
    let version = key.version().get().to_be_bytes();
    let row = connection
        .query_row(
            GENERATION_QUERY,
            params![
                key.configuration().as_bytes().as_slice(),
                version.as_slice()
            ],
            |row| -> rusqlite::Result<GenerationRow> {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()
        .map_err(|error| map_sqlite_error(error, ErrorContext::State))?;
    let Some((configuration, stored_version, module, name, enabled)) = row else {
        return Ok(None);
    };
    if decode_identifier(&configuration)? != key.configuration()
        || decode_version(&stored_version)? != key.version()
    {
        return Err(DatabaseError::IntegrityFailure);
    }

    let settings = load_settings(connection, GENERATION_SETTINGS_QUERY, key)?;
    let log_types = load_log_types(connection, key)?;
    persistence
        .generation(
            key,
            Name::new(module).map_err(|_| DatabaseError::IntegrityFailure)?,
            Name::new(name).map_err(|_| DatabaseError::IntegrityFailure)?,
            decode_boolean(enabled)?,
            settings,
            log_types,
        )
        .map(Some)
        .map_err(|_| DatabaseError::IntegrityFailure)
}

fn validate_current_generation(
    connection: &Connection,
    generation: &LogConfigurationGeneration,
) -> Result<(), DatabaseError> {
    let configuration = generation.key().configuration();
    let current = connection
        .query_row(
            CURRENT_CONFIGURATION_QUERY,
            params![configuration.as_bytes().as_slice()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|error| map_sqlite_error(error, ErrorContext::State))?
        .ok_or(DatabaseError::IntegrityFailure)?;
    let current = decode_configuration(current)?;
    let current_settings = load_current_settings(connection, configuration)?;
    if current.0 != *generation.module()
        || current.1 != *generation.name()
        || current.2 != generation.enabled()
        || current_settings != generation.settings()
        || !generation.contains_log_type(LogType::Audit)
    {
        return Err(DatabaseError::IntegrityFailure);
    }
    Ok(())
}

fn load_settings(
    connection: &Connection,
    query: &str,
    key: LogConfigurationGenerationKey,
) -> Result<Vec<LogModuleSetting>, DatabaseError> {
    let version = key.version().get().to_be_bytes();
    let mut statement = connection
        .prepare(query)
        .map_err(|error| map_sqlite_error(error, ErrorContext::State))?;
    let rows = statement
        .query_map(
            params![
                key.configuration().as_bytes().as_slice(),
                version.as_slice()
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .map_err(|error| map_sqlite_error(error, ErrorContext::State))?;
    rows.map(|row| {
        let (key, value) = row.map_err(|error| map_sqlite_error(error, ErrorContext::State))?;
        Ok(LogModuleSetting {
            key: ConfigurationKey::new(key).map_err(|_| DatabaseError::IntegrityFailure)?,
            value: ConfigurationValue::new(value).map_err(|_| DatabaseError::IntegrityFailure)?,
        })
    })
    .collect()
}

fn load_current_settings(
    connection: &Connection,
    configuration: StateIdentifier,
) -> Result<Vec<LogModuleSetting>, DatabaseError> {
    let mut statement = connection
        .prepare(CURRENT_SETTINGS_QUERY)
        .map_err(|error| map_sqlite_error(error, ErrorContext::State))?;
    let rows = statement
        .query_map(params![configuration.as_bytes().as_slice()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| map_sqlite_error(error, ErrorContext::State))?;
    rows.map(|row| {
        let (key, value) = row.map_err(|error| map_sqlite_error(error, ErrorContext::State))?;
        Ok(LogModuleSetting {
            key: ConfigurationKey::new(key).map_err(|_| DatabaseError::IntegrityFailure)?,
            value: ConfigurationValue::new(value).map_err(|_| DatabaseError::IntegrityFailure)?,
        })
    })
    .collect()
}

fn load_log_types(
    connection: &Connection,
    key: LogConfigurationGenerationKey,
) -> Result<Vec<LogType>, DatabaseError> {
    let version = key.version().get().to_be_bytes();
    let mut statement = connection
        .prepare(GENERATION_LOG_TYPES_QUERY)
        .map_err(|error| map_sqlite_error(error, ErrorContext::State))?;
    let rows = statement
        .query_map(
            params![
                key.configuration().as_bytes().as_slice(),
                version.as_slice()
            ],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| map_sqlite_error(error, ErrorContext::State))?;
    rows.map(|row| {
        match row
            .map_err(|error| map_sqlite_error(error, ErrorContext::State))?
            .as_str()
        {
            "system" => Ok(LogType::System),
            "audit" => Ok(LogType::Audit),
            _ => Err(DatabaseError::IntegrityFailure),
        }
    })
    .collect()
}

fn decode_configuration(row: ConfigurationRow) -> Result<(Name, Name, bool), DatabaseError> {
    Ok((
        Name::new(row.0).map_err(|_| DatabaseError::IntegrityFailure)?,
        Name::new(row.1).map_err(|_| DatabaseError::IntegrityFailure)?,
        decode_boolean(row.2)?,
    ))
}

fn decode_identifier(bytes: &[u8]) -> Result<StateIdentifier, DatabaseError> {
    let bytes: [u8; STATE_IDENTIFIER_LENGTH] = bytes
        .try_into()
        .map_err(|_| DatabaseError::IntegrityFailure)?;
    StateIdentifier::from_bytes(bytes).map_err(|_| DatabaseError::IntegrityFailure)
}

fn decode_version(bytes: &[u8]) -> Result<LogConfigurationVersion, DatabaseError> {
    let bytes: [u8; 8] = bytes
        .try_into()
        .map_err(|_| DatabaseError::IntegrityFailure)?;
    LogConfigurationVersion::new(u64::from_be_bytes(bytes)).ok_or(DatabaseError::IntegrityFailure)
}

fn decode_boolean(value: i64) -> Result<bool, DatabaseError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(DatabaseError::IntegrityFailure),
    }
}
