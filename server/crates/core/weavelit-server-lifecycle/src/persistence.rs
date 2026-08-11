use std::{fmt, path::Path};

use weavelit_server_database::{DatabaseError, DatabaseInspection, ProtectedValue};

use crate::{
    BackendCatalog, BackendIdentifier, ConnectionFieldInput, DatabaseLocator, DeploymentRecord,
    LifecycleClassification, LifecycleError, LifecycleState, LocatorConnectionSettings,
    ProtectedValueKind, ProtectedValueSealer, RetainedDatabaseInspection, SelectionError,
    TrustedBackendContext, ValidatedConnectionSettings,
    filesystem::{Inventory, StateRoot},
    format::{
        AnchorKey, KEY_FILE_LIMIT, KEY_FILE_NAME, LOCATOR_ENVELOPE_LIMIT, RECORD_ENVELOPE_LIMIT,
        RECORD_FILE_NAME, decrypt_locator, decrypt_record, encrypt_locator,
        encrypt_protected_value, encrypt_record, generate_deployment_identifier, generate_key,
        generate_locator_generation, generate_nonce, locator_file_name, parse_key, serialize_key,
    },
};

/// How the current anchor set was obtained.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnchorLoadState {
    /// A new key and deployment record were created.
    FirstStartCreated,
    /// An existing complete anchor set was reopened.
    Retained,
}

/// Unforgeable capability granted only by lifecycle database-selection authority.
#[non_exhaustive]
pub struct LocatorPersistencePermit;

/// Unforgeable capability granted only by lifecycle state-transition authority.
#[non_exhaustive]
pub struct RecordPersistencePermit;

/// Open, locked, authenticated lifecycle anchor store.
pub struct LifecycleStore {
    root: StateRoot,
    key: AnchorKey,
    record: DeploymentRecord,
    locator: Option<DatabaseLocator>,
    load_state: AnchorLoadState,
}

impl LifecycleStore {
    /// Opens or creates the lifecycle anchor set beneath a trusted state root.
    pub fn open_or_create(path: &Path) -> Result<Self, LifecycleError> {
        let root = StateRoot::open(path)?;
        let inventory = root.inventory()?;
        match (inventory.has_key, inventory.has_record) {
            (false, false)
                if inventory.locator_files.is_empty()
                    && inventory.temporary_files.is_empty()
                    && !inventory.has_application_database_artifact
                    && !inventory.has_log_database_artifact =>
            {
                Self::create(root, AnchorLoadState::FirstStartCreated)
            }
            (true, true) => Self::load(root, inventory),
            _ => Err(LifecycleError::IntegrityFailure),
        }
    }

    /// Returns how this anchor set was obtained.
    pub const fn load_state(&self) -> AnchorLoadState {
        self.load_state
    }

    /// Returns the authenticated deployment record.
    pub const fn record(&self) -> &DeploymentRecord {
        &self.record
    }

    /// Returns the authenticated locator when a database is selected.
    pub const fn locator(&self) -> Option<&DatabaseLocator> {
        self.locator.as_ref()
    }

    /// Creates locator content bound to this deployment and a fresh generation.
    pub fn create_locator(
        &self,
        _permit: &LocatorPersistencePermit,
        settings: ValidatedConnectionSettings,
    ) -> Result<DatabaseLocator, LifecycleError> {
        Ok(DatabaseLocator::from_validated(
            self.record.deployment_identifier(),
            generate_locator_generation()?,
            settings,
        ))
    }

    /// Durably replaces the active locator using the deployment record as commit point.
    pub fn replace_locator(
        &mut self,
        _permit: &LocatorPersistencePermit,
        locator: DatabaseLocator,
    ) -> Result<(), LifecycleError> {
        if locator.deployment_identifier() != self.record.deployment_identifier() {
            return Err(LifecycleError::DeploymentMismatch);
        }
        let new_name = locator_file_name(locator.generation());
        let locator_bytes = encrypt_locator(&self.key, &locator, generate_nonce()?)?;
        self.root.publish_new(&new_name, &locator_bytes)?;
        let updated_record = DeploymentRecord::new(
            self.record.deployment_identifier(),
            self.record.state(),
            Some(locator.generation()),
        )
        .map_err(|_| LifecycleError::InvalidState)?;
        self.persist_record(&updated_record)?;

        let previous_name = self
            .locator
            .as_ref()
            .map(|current| locator_file_name(current.generation()));
        self.record = updated_record;
        self.locator = Some(locator);
        if let Some(previous_name) = previous_name
            && previous_name != new_name
        {
            self.root.remove(&previous_name)?;
        }
        Ok(())
    }

    /// Durably replaces the deployment record without changing the selected locator.
    pub fn replace_record(
        &mut self,
        _permit: &RecordPersistencePermit,
        record: DeploymentRecord,
    ) -> Result<(), LifecycleError> {
        if record.deployment_identifier() != self.record.deployment_identifier() {
            return Err(LifecycleError::DeploymentMismatch);
        }
        let active_generation = self.locator.as_ref().map(DatabaseLocator::generation);
        if record.locator_generation() != active_generation {
            return Err(LifecycleError::InvalidState);
        }
        self.persist_record(&record)?;
        self.record = record;
        Ok(())
    }

    /// Preflights, selects, or replaces the Application Database using the backend catalog.
    ///
    /// Requires the deployment record to be `Uninitialized`. If a locator already exists,
    /// reopens the current database and inspects it; the current selection can only be
    /// replaced while it remains uninitialized with no checkpoint. Fully preflights the
    /// candidate before atomically persisting the new locator. An exact replay of the
    /// persisted settings succeeds without creating, rotating, or rewriting a locator.
    pub fn select_database(
        &mut self,
        catalog: &BackendCatalog,
        context: &TrustedBackendContext,
        backend: &BackendIdentifier,
        inputs: Vec<ConnectionFieldInput>,
    ) -> Result<Box<dyn weavelit_server_database::ApplicationDatabase>, SelectionError> {
        if self.record.state() != LifecycleState::Uninitialized {
            return Err(SelectionError::NotAllowed);
        }

        // Replacement eligibility: current database must be uninitialized with no checkpoint.
        if let Some(locator) = &self.locator {
            let mut current_db = catalog
                .reopen(locator.settings(), context)
                .map_err(SelectionError::Lifecycle)?;
            match current_db
                .inspect(self.record.deployment_identifier())
                .map_err(map_database_error)?
            {
                DatabaseInspection::Uninitialized => {}
                DatabaseInspection::Pending(_) | DatabaseInspection::Initialized { .. } => {
                    return Err(SelectionError::ReplacementIneligible);
                }
            }
        }

        // Preflight: validate fields, open candidate, and inspect.
        let (settings, mut new_db) = catalog
            .validate_and_open(backend, context, inputs)
            .map_err(SelectionError::Open)?;
        match new_db
            .inspect(self.record.deployment_identifier())
            .map_err(map_database_error)?
        {
            DatabaseInspection::Uninitialized => {}
            DatabaseInspection::Pending(_) | DatabaseInspection::Initialized { .. } => {
                return Err(SelectionError::CandidateIneligible);
            }
        }

        // Exact replay: identical persisted settings require no durable locator change.
        if let Some(locator) = &self.locator
            && settings_match(locator.settings(), &settings)
        {
            return Ok(new_db);
        }

        // Persist the new locator atomically.
        let permit = LocatorPersistencePermit;
        let locator = self
            .create_locator(&permit, settings)
            .map_err(SelectionError::Lifecycle)?;
        self.replace_locator(&permit, locator)
            .map_err(SelectionError::Lifecycle)?;

        Ok(new_db)
    }

    /// Reopens the selected Application Database from the persisted locator.
    ///
    /// Requires a locator to be present. Reconstructs the backend from stored settings
    /// using the backend catalog and validates the deployment binding.
    pub fn reopen_selected_database(
        &self,
        catalog: &BackendCatalog,
        context: &TrustedBackendContext,
    ) -> Result<Box<dyn weavelit_server_database::ApplicationDatabase>, LifecycleError> {
        let locator = self.locator.as_ref().ok_or(LifecycleError::InvalidState)?;
        let mut database = catalog.reopen(locator.settings(), context)?;
        database
            .inspect(self.record.deployment_identifier())
            .map_err(map_database_error_to_lifecycle)?;
        Ok(database)
    }

    /// Classifies startup state from the current anchor set without mutating retained state.
    ///
    /// A pre-operational record is classified through non-mutating retained
    /// inspection, which cannot reconcile a write-ahead log and therefore
    /// refuses to guess past one. A sealed record is classified from the record
    /// alone and verified by the authoritative sealed load instead.
    pub fn classify_startup(
        &self,
        catalog: &BackendCatalog,
        context: &TrustedBackendContext,
    ) -> Result<LifecycleClassification, LifecycleError> {
        match self.record.state() {
            LifecycleState::Uninitialized => {
                let Some(locator) = self.locator.as_ref() else {
                    return Ok(LifecycleClassification::UninitializedWithoutDatabase);
                };
                let inspection = catalog.inspect_retained(
                    locator.settings(),
                    context,
                    self.record.deployment_identifier(),
                )?;
                let inspection = match inspection {
                    RetainedDatabaseInspection::Inspected(inspection) => inspection,
                    RetainedDatabaseInspection::RedeployRequired => {
                        return Ok(LifecycleClassification::Interrupted(
                            crate::InterruptedLifecycleAction::RedeployRequired,
                        ));
                    }
                };
                match inspection {
                    DatabaseInspection::Uninitialized => {
                        Ok(LifecycleClassification::UninitializedWithDatabase)
                    }
                    DatabaseInspection::Pending(checkpoint) => Ok(
                        LifecycleClassification::Interrupted(Self::interrupted_action(&checkpoint)),
                    ),
                    DatabaseInspection::Initialized { .. } => {
                        Ok(LifecycleClassification::Interrupted(
                            crate::InterruptedLifecycleAction::RedeployRequired,
                        ))
                    }
                }
            }
            LifecycleState::InitializationPending => {
                // Record must have a locator (enforced by domain invariant).
                let locator = self
                    .locator
                    .as_ref()
                    .ok_or(LifecycleError::IntegrityFailure)?;
                let inspection = catalog.inspect_retained(
                    locator.settings(),
                    context,
                    self.record.deployment_identifier(),
                )?;
                let inspection = match inspection {
                    RetainedDatabaseInspection::Inspected(inspection) => inspection,
                    RetainedDatabaseInspection::RedeployRequired => {
                        return Ok(LifecycleClassification::Interrupted(
                            crate::InterruptedLifecycleAction::RedeployRequired,
                        ));
                    }
                };
                match inspection {
                    DatabaseInspection::Pending(checkpoint) => Ok(
                        LifecycleClassification::Interrupted(Self::interrupted_action(&checkpoint)),
                    ),
                    DatabaseInspection::Initialized { .. } => {
                        Ok(LifecycleClassification::Interrupted(
                            crate::InterruptedLifecycleAction::RedeployRequired,
                        ))
                    }
                    DatabaseInspection::Uninitialized => Err(LifecycleError::IntegrityFailure),
                }
            }
            // A sealed record is classified as a sealed candidate without any
            // retained inspection. Immutable inspection deliberately refuses to
            // read past a write-ahead log, and an operational deployment always
            // leaves one, so consulting it here would brick every restart. The
            // authoritative sealed load reopens the database read-write, which
            // lets SQLite recover that log normally, and re-verifies the
            // deployment binding and initialized state before anything serves.
            LifecycleState::Initialized => {
                if self.locator.is_none() {
                    return Err(LifecycleError::IntegrityFailure);
                }
                Ok(LifecycleClassification::Initialized)
            }
        }
    }

    fn interrupted_action(
        checkpoint: &weavelit_server_database::WorkflowCheckpoint,
    ) -> crate::InterruptedLifecycleAction {
        match checkpoint.workflow() {
            weavelit_server_database::WorkflowKind::Init => {
                crate::InterruptedLifecycleAction::RedeployNew
            }
            weavelit_server_database::WorkflowKind::Restore => {
                crate::InterruptedLifecycleAction::RedeployRestore
            }
        }
    }

    fn create(root: StateRoot, load_state: AnchorLoadState) -> Result<Self, LifecycleError> {
        let key = generate_key()?;
        root.publish_new(KEY_FILE_NAME, &serialize_key(&key)?)?;
        let record = DeploymentRecord::new(
            generate_deployment_identifier()?,
            LifecycleState::Uninitialized,
            None,
        )
        .map_err(|_| LifecycleError::IntegrityFailure)?;
        let record_bytes = encrypt_record(&key, &record, generate_nonce()?)?;
        root.publish_new(RECORD_FILE_NAME, &record_bytes)?;
        let reopened = decrypt_record(&key, &root.read(RECORD_FILE_NAME, RECORD_ENVELOPE_LIMIT)?)?;
        if reopened != record {
            return Err(LifecycleError::IntegrityFailure);
        }
        Ok(Self {
            root,
            key,
            record,
            locator: None,
            load_state,
        })
    }

    fn load(root: StateRoot, inventory: Inventory) -> Result<Self, LifecycleError> {
        if !inventory.temporary_files.is_empty() {
            return Err(LifecycleError::IntegrityFailure);
        }
        let key = parse_key(&root.read(KEY_FILE_NAME, KEY_FILE_LIMIT)?)?;
        let record = decrypt_record(&key, &root.read(RECORD_FILE_NAME, RECORD_ENVELOPE_LIMIT)?)?;
        let active_generation = record.locator_generation();
        if active_generation.is_none() && inventory.has_application_database_artifact {
            return Err(LifecycleError::IntegrityFailure);
        }

        let active_name = active_generation.map(locator_file_name);
        let locator = if let Some(generation) = active_generation {
            let name = active_name
                .as_deref()
                .ok_or(LifecycleError::IntegrityFailure)?;
            if !inventory
                .locator_files
                .iter()
                .any(|(_, found)| found == name)
            {
                return Err(LifecycleError::IntegrityFailure);
            }
            let locator =
                decrypt_locator(&key, generation, &root.read(name, LOCATOR_ENVELOPE_LIMIT)?)?;
            if locator.deployment_identifier() != record.deployment_identifier() {
                return Err(LifecycleError::DeploymentMismatch);
            }
            Some(locator)
        } else {
            None
        };
        if inventory
            .locator_files
            .iter()
            .any(|(_, name)| Some(name.as_str()) != active_name.as_deref())
        {
            return Err(LifecycleError::IntegrityFailure);
        }
        Ok(Self {
            root,
            key,
            record,
            locator,
            load_state: AnchorLoadState::Retained,
        })
    }

    fn persist_record(&self, record: &DeploymentRecord) -> Result<(), LifecycleError> {
        let bytes = encrypt_record(&self.key, record, generate_nonce()?)?;
        self.root.replace(RECORD_FILE_NAME, &bytes)
    }
}

impl ProtectedValueSealer for LifecycleStore {
    fn seal(
        &self,
        kind: ProtectedValueKind,
        plaintext: &[u8],
    ) -> Result<ProtectedValue, LifecycleError> {
        let envelope =
            encrypt_protected_value(&self.key, kind.label(), plaintext, generate_nonce()?)?;
        ProtectedValue::new(envelope).map_err(|_| LifecycleError::IntegrityFailure)
    }
}

impl fmt::Debug for LifecycleStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LifecycleStore(REDACTED)")
    }
}

fn map_database_error(error: DatabaseError) -> SelectionError {
    SelectionError::Lifecycle(map_database_error_to_lifecycle(error))
}

/// Compares persisted locator settings with newly validated settings. Both are
/// canonically sorted by field identifier.
fn settings_match(
    persisted: &LocatorConnectionSettings,
    candidate: &ValidatedConnectionSettings,
) -> bool {
    persisted.backend_identifier() == candidate.backend_identifier()
        && persisted.len() == candidate.len()
        && persisted
            .iter()
            .zip(candidate.iter())
            .all(|(persisted, candidate)| {
                persisted.identifier() == candidate.identifier()
                    && persisted.value() == candidate.value()
            })
}

fn map_database_error_to_lifecycle(error: DatabaseError) -> LifecycleError {
    match error {
        DatabaseError::DeploymentMismatch => LifecycleError::DeploymentMismatch,
        DatabaseError::Unavailable => LifecycleError::DependencyUnavailable,
        DatabaseError::IntegrityFailure => LifecycleError::IntegrityFailure,
        DatabaseError::ConfigurationInvalid => LifecycleError::ConfigurationInvalid,
        DatabaseError::InvalidState
        | DatabaseError::AlreadyInitialized
        | DatabaseError::NotInitialized => LifecycleError::InvalidState,
        _ => LifecycleError::InvalidState,
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt};

    use crate::{
        BackendIdentifier, ConnectionFieldIdentifier, ConnectionValue, LocatorConnectionField,
        LocatorConnectionSettings, SecretClassification, ValidatedConnectionField,
    };

    use super::*;

    const SENSITIVE_SECRET: &str = "sensitive-locator-credential";

    fn root() -> (tempfile::TempDir, std::path::PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let canonical = directory.path().canonicalize().unwrap();
        (directory, canonical)
    }

    fn settings() -> ValidatedConnectionSettings {
        ValidatedConnectionSettings::new(
            BackendIdentifier::new("remote-postgres").unwrap(),
            vec![ValidatedConnectionField::new(
                ConnectionFieldIdentifier::new("credential").unwrap(),
                SecretClassification::Secret,
                ConnectionValue::string(SENSITIVE_SECRET),
            )],
        )
    }

    fn locator_files(path: &Path) -> Vec<std::path::PathBuf> {
        fs::read_dir(path)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.starts_with("database-locator-") && name.ends_with(".json")
                    })
            })
            .collect()
    }

    #[test]
    fn locator_and_record_round_trip_without_exposing_raw_mutation_publicly() {
        let (_directory, path) = root();
        let mut store = LifecycleStore::open_or_create(&path).unwrap();
        let locator_permit = LocatorPersistencePermit;
        let record_permit = RecordPersistencePermit;
        let first = store.create_locator(&locator_permit, settings()).unwrap();
        let first_generation = first.generation();
        store.replace_locator(&locator_permit, first).unwrap();
        let bytes = fs::read(&locator_files(&path)[0]).unwrap();
        assert!(
            !bytes
                .windows(SENSITIVE_SECRET.len())
                .any(|window| window == SENSITIVE_SECRET.as_bytes())
        );

        let second = store.create_locator(&locator_permit, settings()).unwrap();
        let second_generation = second.generation();
        assert_ne!(first_generation, second_generation);
        store.replace_locator(&locator_permit, second).unwrap();
        assert_eq!(locator_files(&path).len(), 1);

        let initialized = DeploymentRecord::new(
            store.record().deployment_identifier(),
            LifecycleState::Initialized,
            Some(second_generation),
        )
        .unwrap();
        store.replace_record(&record_permit, initialized).unwrap();
        drop(store);

        let reopened = LifecycleStore::open_or_create(&path).unwrap();
        assert_eq!(reopened.record().state(), LifecycleState::Initialized);
        let locator = reopened.locator().unwrap();
        assert_eq!(locator.generation(), second_generation);
        assert_eq!(locator.backend_identifier().as_str(), "remote-postgres");
        assert_eq!(
            locator
                .settings()
                .iter()
                .next()
                .and_then(|field| field.value().as_str()),
            Some(SENSITIVE_SECRET)
        );
        assert!(!format!("{reopened:?} {locator:?}").contains(SENSITIVE_SECRET));
    }

    #[test]
    fn persisted_locator_settings_do_not_retain_secret_classification() {
        let persisted = LocatorConnectionSettings::new(
            BackendIdentifier::new("remote-postgres").unwrap(),
            vec![LocatorConnectionField::new(
                ConnectionFieldIdentifier::new("credential").unwrap(),
                ConnectionValue::string(SENSITIVE_SECRET),
            )],
        );
        assert_eq!(persisted.len(), 1);
    }
}
