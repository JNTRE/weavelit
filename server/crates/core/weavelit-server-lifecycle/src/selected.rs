use std::{fmt, sync::Arc};

use weavelit_server_database::{
    AccountAuditReference, ApplicationDatabase, AuditReferencePersistence,
    AuditTerminalRecoveryPersistence, DatabaseError, DeploymentIdentifier, GroupAuditReference,
    InitializedState, LogConfigurationAuditReference, LogConfigurationGenerationPersistence,
    LogConfigurationMutationPersistence, StateIdentifier,
};
use weavelit_server_database_authority::ServerDatabaseAuthority;

/// An opened Application Database and its Server-issued persistence decoder.
///
/// Lifecycle constructs this only after database selection or reopening has
/// succeeded. Its private fields keep the decoder inseparable from that
/// selected handle and prevent callers from replacing either half.
pub struct SelectedDatabase {
    database: Box<dyn ApplicationDatabase>,
    persistence: AuditReferencePersistence,
    audit_terminal_recovery_persistence: Arc<AuditTerminalRecoveryPersistence>,
    log_configuration_generation_persistence: Arc<LogConfigurationGenerationPersistence>,
    log_configuration_mutation_persistence: Arc<LogConfigurationMutationPersistence>,
}

pub(crate) fn selected_database(database: Box<dyn ApplicationDatabase>) -> SelectedDatabase {
    let authority = ServerDatabaseAuthority::new();
    SelectedDatabase::from_server_authority(database, &authority)
}

impl SelectedDatabase {
    /// Binds an opened database to Server-owned selection authority.
    ///
    /// This constructor is lifecycle-private, and the authority crate is
    /// deliberately not reexported.
    #[must_use]
    pub(crate) fn from_server_authority(
        database: Box<dyn ApplicationDatabase>,
        authority: &ServerDatabaseAuthority,
    ) -> Self {
        Self {
            database,
            persistence: AuditReferencePersistence::from_server_authority(authority),
            audit_terminal_recovery_persistence: Arc::new(
                AuditTerminalRecoveryPersistence::from_server_authority(authority),
            ),
            log_configuration_generation_persistence: Arc::new(
                LogConfigurationGenerationPersistence::from_server_authority(authority),
            ),
            log_configuration_mutation_persistence: Arc::new(
                LogConfigurationMutationPersistence::from_server_authority(authority),
            ),
        }
    }

    /// Runs one backend-neutral operation against the selected database.
    pub fn with<R>(&mut self, operation: impl FnOnce(&mut dyn ApplicationDatabase) -> R) -> R {
        operation(&mut *self.database)
    }

    /// Returns the decoder bound to this selected database.
    #[must_use]
    pub const fn audit_reference_persistence(&self) -> AuditReferencePersistence {
        self.persistence
    }

    /// Returns the opaque recovery decoder bound to this selected database.
    #[must_use]
    pub fn audit_terminal_recovery_persistence(&self) -> Arc<AuditTerminalRecoveryPersistence> {
        Arc::clone(&self.audit_terminal_recovery_persistence)
    }

    /// Returns immutable configuration-generation persistence authority.
    #[must_use]
    pub fn log_configuration_generation_persistence(
        &self,
    ) -> Arc<LogConfigurationGenerationPersistence> {
        Arc::clone(&self.log_configuration_generation_persistence)
    }

    /// Returns Log Module configuration mutation persistence authority.
    #[must_use]
    pub fn log_configuration_mutation_persistence(
        &self,
    ) -> Arc<LogConfigurationMutationPersistence> {
        Arc::clone(&self.log_configuration_mutation_persistence)
    }

    /// Loads initialized state through this selected database's decoder.
    pub fn load_initialized_state(
        &mut self,
        expected_deployment_identifier: DeploymentIdentifier,
    ) -> Result<InitializedState, DatabaseError> {
        self.database
            .load_initialized_state(&self.persistence, expected_deployment_identifier)
    }

    /// Loads an account Audit Reference through this selected database's decoder.
    pub fn load_account_audit_reference(
        &mut self,
        account: StateIdentifier,
    ) -> Result<Option<AccountAuditReference>, DatabaseError> {
        self.database
            .load_account_audit_reference(&self.persistence, account)
    }

    /// Loads a Group Audit Reference through this selected database's decoder.
    pub fn load_group_audit_reference(
        &mut self,
        group: StateIdentifier,
    ) -> Result<Option<GroupAuditReference>, DatabaseError> {
        self.database
            .load_group_audit_reference(&self.persistence, group)
    }

    /// Loads a Log Module configuration Audit Reference through this selected database's decoder.
    pub fn load_log_configuration_audit_reference(
        &mut self,
        configuration: StateIdentifier,
    ) -> Result<Option<LogConfigurationAuditReference>, DatabaseError> {
        self.database
            .load_log_configuration_audit_reference(&self.persistence, configuration)
    }

    /// Closes the selected database and consumes its decoder with it.
    pub fn close(self) -> Result<(), DatabaseError> {
        self.database.close()
    }
}

impl fmt::Debug for SelectedDatabase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SelectedDatabase(REDACTED)")
    }
}
