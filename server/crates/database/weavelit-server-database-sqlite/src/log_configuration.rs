use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use weavelit_server_database::{
    ConfigurationKey, ConfigurationValue, DatabaseError, LogAssignment,
    LogConfigurationAuditTerminalWrites, LogConfigurationGeneration, LogConfigurationGenerationKey,
    LogConfigurationGenerationPersistence, LogConfigurationGenerationStore,
    LogConfigurationMutationOutcome, LogConfigurationMutationPersistence,
    LogConfigurationMutationRequest, LogConfigurationMutationStore, LogConfigurationPreparation,
    LogConfigurationVersion, LogModuleSetting, LogType, Name, PreparedLogConfigurationMutation,
    STATE_IDENTIFIER_LENGTH, StateIdentifier,
};

use crate::SqliteDatabase;
use crate::audit_recovery;
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
const CURRENT_LOG_TYPES_QUERY: &str = "SELECT log_type FROM weavelit_log_assignment \
    WHERE configuration_id = ?1 \
    ORDER BY CASE log_type WHEN 'system' THEN 0 WHEN 'audit' THEN 1 END";

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

impl LogConfigurationMutationStore for SqliteDatabase {
    fn prepare_log_configuration_mutation(
        &mut self,
        generation_persistence: &LogConfigurationGenerationPersistence,
        mutation_persistence: &LogConfigurationMutationPersistence,
        request: &LogConfigurationMutationRequest,
    ) -> Result<LogConfigurationPreparation, DatabaseError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(log_configuration_error)?;
        let current = load_current_generations(&transaction, generation_persistence)?;
        let expected_assignments = load_current_assignments(&transaction)?;
        validate_complete_topology(&current, &expected_assignments)?;

        let Some(primary) = current
            .iter()
            .position(|generation| generation.key().configuration() == request.primary())
        else {
            return Ok(LogConfigurationPreparation::Invalid);
        };
        let mut desired_assignments = expected_assignments.clone();
        for requested in request.assignments() {
            if !current
                .iter()
                .any(|generation| generation.key().configuration() == requested.configuration)
            {
                return Ok(LogConfigurationPreparation::Invalid);
            }
            let assigned = desired_assignments
                .iter_mut()
                .find(|assignment| assignment.log_type == requested.log_type)
                .ok_or(DatabaseError::IntegrityFailure)?;
            if assigned.configuration != requested.configuration
                && assigned.configuration != request.primary()
                && requested.configuration != request.primary()
            {
                return Ok(LogConfigurationPreparation::Invalid);
            }
            assigned.configuration = requested.configuration;
        }

        let mut candidates = Vec::new();
        for (index, generation) in current.iter().enumerate() {
            let configuration = generation.key().configuration();
            let enabled = if index == primary {
                request.enabled().unwrap_or(generation.enabled())
            } else {
                generation.enabled()
            };
            let settings = if index == primary {
                request.settings().unwrap_or(generation.settings()).to_vec()
            } else {
                generation.settings().to_vec()
            };
            let log_types = desired_assignments
                .iter()
                .filter(|assignment| assignment.configuration == configuration)
                .map(|assignment| assignment.log_type)
                .collect::<Vec<_>>();

            if desired_assignments
                .iter()
                .any(|assignment| assignment.configuration == configuration)
                && !enabled
            {
                return Ok(LogConfigurationPreparation::Invalid);
            }
            if enabled == generation.enabled()
                && settings == generation.settings()
                && log_types == generation.log_types()
            {
                continue;
            }

            let Some(version) = generation.key().version().checked_next() else {
                return Ok(LogConfigurationPreparation::VersionExhausted);
            };
            let candidate = generation_persistence
                .generation(
                    generation_persistence.key(configuration, version),
                    generation.module().clone(),
                    generation.name().clone(),
                    enabled,
                    settings,
                    log_types,
                )
                .map_err(|_| DatabaseError::IntegrityFailure)?;
            candidates.push((generation.clone(), candidate));
        }

        if candidates.is_empty() {
            return Ok(LogConfigurationPreparation::Unchanged);
        }
        let resultant_generations = current
            .iter()
            .map(|generation| {
                candidates
                    .iter()
                    .find(|(expected, _)| expected.key() == generation.key())
                    .map_or_else(|| generation.clone(), |(_, candidate)| candidate.clone())
            })
            .collect();
        mutation_persistence
            .prepare(
                candidates,
                expected_assignments,
                desired_assignments,
                resultant_generations,
            )
            .map(LogConfigurationPreparation::Prepared)
            .map_err(|_| DatabaseError::IntegrityFailure)
    }

    fn commit_log_configuration_mutation(
        &mut self,
        mutation: &PreparedLogConfigurationMutation,
        audit_terminals: &LogConfigurationAuditTerminalWrites<'_>,
    ) -> Result<LogConfigurationMutationOutcome, DatabaseError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(log_configuration_error)?;
        if !matches_prepared_state(&transaction, mutation)? {
            audit_recovery::persist_in_transaction(&transaction, audit_terminals.stale())?;
            transaction.commit().map_err(log_configuration_error)?;
            return Ok(LogConfigurationMutationOutcome::Stale);
        }

        for entry in mutation.entries() {
            insert_generation(&transaction, entry.candidate())?;
            write_current_configuration(&transaction, entry.candidate())?;
        }
        write_assignments(&transaction, mutation.desired_assignments())?;
        audit_recovery::persist_in_transaction(&transaction, audit_terminals.applied())?;
        for entry in mutation.entries() {
            let version = entry.candidate().key().version().get().to_be_bytes();
            let written = transaction
                .execute(
                    "UPDATE weavelit_log_configuration_current_generation \
                     SET generation_version = ?1 WHERE configuration_id = ?2",
                    params![
                        version.as_slice(),
                        entry
                            .candidate()
                            .key()
                            .configuration()
                            .as_bytes()
                            .as_slice(),
                    ],
                )
                .map_err(log_configuration_error)?;
            if written != 1 {
                return Err(DatabaseError::IntegrityFailure);
            }
        }
        transaction.commit().map_err(log_configuration_error)?;

        Ok(LogConfigurationMutationOutcome::Applied {
            generation_count: mutation.entries().len(),
        })
    }
}

fn load_current_generations(
    connection: &Connection,
    persistence: &LogConfigurationGenerationPersistence,
) -> Result<Vec<LogConfigurationGeneration>, DatabaseError> {
    let configuration_ids = query_identifier_column(
        connection,
        "SELECT configuration_id FROM weavelit_log_module_configuration \
         ORDER BY configuration_id",
    )?;
    let pointer_ids = query_identifier_column(
        connection,
        "SELECT configuration_id FROM weavelit_log_configuration_current_generation \
         ORDER BY configuration_id",
    )?;
    if configuration_ids != pointer_ids {
        return Err(DatabaseError::IntegrityFailure);
    }

    configuration_ids
        .into_iter()
        .map(|configuration| {
            let stored_version = connection
                .query_row(
                    CURRENT_VERSION_QUERY,
                    params![configuration.as_bytes().as_slice()],
                    |row| row.get::<_, Vec<u8>>(0),
                )
                .map_err(log_configuration_error)?;
            let key = persistence.key(configuration, decode_version(&stored_version)?);
            let generation = load_generation(connection, persistence, key)?
                .ok_or(DatabaseError::IntegrityFailure)?;
            if !current_generation_matches(connection, &generation)? {
                return Err(DatabaseError::IntegrityFailure);
            }
            Ok(generation)
        })
        .collect()
}

fn query_identifier_column(
    connection: &Connection,
    query: &str,
) -> Result<Vec<StateIdentifier>, DatabaseError> {
    let mut statement = connection.prepare(query).map_err(log_configuration_error)?;
    let rows = statement
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .map_err(log_configuration_error)?;
    rows.map(|row| decode_identifier(&row.map_err(log_configuration_error)?))
        .collect()
}

fn load_current_assignments(connection: &Connection) -> Result<Vec<LogAssignment>, DatabaseError> {
    let mut statement = connection
        .prepare(
            "SELECT log_type, configuration_id FROM weavelit_log_assignment \
             ORDER BY CASE log_type WHEN 'system' THEN 0 WHEN 'audit' THEN 1 END",
        )
        .map_err(log_configuration_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
        })
        .map_err(log_configuration_error)?;
    rows.map(|row| {
        let (log_type, configuration) = row.map_err(log_configuration_error)?;
        Ok(LogAssignment {
            log_type: decode_log_type(&log_type)?,
            configuration: decode_identifier(&configuration)?,
        })
    })
    .collect()
}

fn validate_complete_topology(
    generations: &[LogConfigurationGeneration],
    assignments: &[LogAssignment],
) -> Result<(), DatabaseError> {
    if assignments.len() != LogType::ALL.len()
        || assignments
            .iter()
            .zip(LogType::ALL)
            .any(|(assignment, expected)| assignment.log_type != expected)
        || assignments.iter().any(|assignment| {
            !generations.iter().any(|generation| {
                generation.key().configuration() == assignment.configuration && generation.enabled()
            })
        })
    {
        return Err(DatabaseError::IntegrityFailure);
    }
    Ok(())
}

fn current_generation_matches(
    connection: &Connection,
    generation: &LogConfigurationGeneration,
) -> Result<bool, DatabaseError> {
    let configuration = generation.key().configuration();
    let current = connection
        .query_row(
            CURRENT_CONFIGURATION_QUERY,
            params![configuration.as_bytes().as_slice()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(log_configuration_error)?;
    let Some(current) = current else {
        return Ok(false);
    };
    let current = decode_configuration(current)?;
    let current_settings = load_current_settings(connection, configuration)?;
    let current_log_types = load_current_log_types(connection, configuration)?;
    Ok(current.0 == *generation.module()
        && current.1 == *generation.name()
        && current.2 == generation.enabled()
        && current_settings == generation.settings()
        && current_log_types == generation.log_types())
}

fn load_current_log_types(
    connection: &Connection,
    configuration: StateIdentifier,
) -> Result<Vec<LogType>, DatabaseError> {
    let mut statement = connection
        .prepare(CURRENT_LOG_TYPES_QUERY)
        .map_err(log_configuration_error)?;
    let rows = statement
        .query_map(params![configuration.as_bytes().as_slice()], |row| {
            row.get::<_, String>(0)
        })
        .map_err(log_configuration_error)?;
    rows.map(|row| decode_log_type(&row.map_err(log_configuration_error)?))
        .collect()
}

fn matches_prepared_state(
    connection: &Connection,
    mutation: &PreparedLogConfigurationMutation,
) -> Result<bool, DatabaseError> {
    if load_current_assignments(connection)? != mutation.expected_assignments() {
        return Ok(false);
    }
    for entry in mutation.entries() {
        let expected = entry.expected();
        let configuration = expected.key().configuration();
        let stored_version = connection
            .query_row(
                CURRENT_VERSION_QUERY,
                params![configuration.as_bytes().as_slice()],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()
            .map_err(log_configuration_error)?;
        if stored_version.as_deref().map(decode_version).transpose()?
            != Some(expected.key().version())
            || !current_generation_matches(connection, expected)?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn insert_generation(
    connection: &Connection,
    generation: &LogConfigurationGeneration,
) -> Result<(), DatabaseError> {
    let configuration = generation.key().configuration();
    let version = generation.key().version().get().to_be_bytes();
    connection
        .execute(
            "INSERT INTO weavelit_log_configuration_generation \
             (configuration_id, generation_version, module, name, enabled) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                configuration.as_bytes().as_slice(),
                version.as_slice(),
                generation.module().as_str(),
                generation.name().as_str(),
                i64::from(generation.enabled()),
            ],
        )
        .map_err(log_configuration_error)?;
    for setting in generation.settings() {
        connection
            .execute(
                "INSERT INTO weavelit_log_configuration_generation_setting \
                 (configuration_id, generation_version, setting_key, setting_value) \
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    configuration.as_bytes().as_slice(),
                    version.as_slice(),
                    setting.key.as_str(),
                    setting.value.as_str(),
                ],
            )
            .map_err(log_configuration_error)?;
    }
    for log_type in generation.log_types() {
        connection
            .execute(
                "INSERT INTO weavelit_log_configuration_generation_log_type \
                 (configuration_id, generation_version, log_type) VALUES (?1, ?2, ?3)",
                params![
                    configuration.as_bytes().as_slice(),
                    version.as_slice(),
                    encode_log_type(*log_type),
                ],
            )
            .map_err(log_configuration_error)?;
    }
    Ok(())
}

fn write_current_configuration(
    connection: &Connection,
    generation: &LogConfigurationGeneration,
) -> Result<(), DatabaseError> {
    let configuration = generation.key().configuration();
    let written = connection
        .execute(
            "UPDATE weavelit_log_module_configuration SET enabled = ?1 \
             WHERE configuration_id = ?2",
            params![
                i64::from(generation.enabled()),
                configuration.as_bytes().as_slice(),
            ],
        )
        .map_err(log_configuration_error)?;
    if written != 1 {
        return Err(DatabaseError::IntegrityFailure);
    }
    connection
        .execute(
            "DELETE FROM weavelit_log_module_setting WHERE configuration_id = ?1",
            params![configuration.as_bytes().as_slice()],
        )
        .map_err(log_configuration_error)?;
    for setting in generation.settings() {
        connection
            .execute(
                "INSERT INTO weavelit_log_module_setting \
                 (configuration_id, setting_key, setting_value) VALUES (?1, ?2, ?3)",
                params![
                    configuration.as_bytes().as_slice(),
                    setting.key.as_str(),
                    setting.value.as_str(),
                ],
            )
            .map_err(log_configuration_error)?;
    }
    Ok(())
}

fn write_assignments(
    connection: &Connection,
    assignments: &[LogAssignment],
) -> Result<(), DatabaseError> {
    for assignment in assignments {
        let written = connection
            .execute(
                "UPDATE weavelit_log_assignment SET configuration_id = ?1 WHERE log_type = ?2",
                params![
                    assignment.configuration.as_bytes().as_slice(),
                    encode_log_type(assignment.log_type),
                ],
            )
            .map_err(log_configuration_error)?;
        if written != 1 {
            return Err(DatabaseError::IntegrityFailure);
        }
    }
    Ok(())
}

fn encode_log_type(log_type: LogType) -> &'static str {
    match log_type {
        LogType::System => "system",
        LogType::Audit => "audit",
    }
}

fn decode_log_type(log_type: &str) -> Result<LogType, DatabaseError> {
    match log_type {
        "system" => Ok(LogType::System),
        "audit" => Ok(LogType::Audit),
        _ => Err(DatabaseError::IntegrityFailure),
    }
}

fn log_configuration_error(error: rusqlite::Error) -> DatabaseError {
    map_sqlite_error(error, ErrorContext::LogConfiguration)
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
