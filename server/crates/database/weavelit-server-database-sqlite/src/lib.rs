#![forbid(unsafe_code)]

//! SQLite implementation of the Weavelit Application Database contract.

mod account_status;
mod account_writer;
#[cfg_attr(not(test), allow(dead_code))]
mod audit_recovery;
mod checkpoint;
mod completion;
mod connection;
mod error;
mod group_administration;
mod group_mutation;
mod inspection;
mod log_configuration;
mod mfa;
mod mfa_policy;
mod migrations;
mod password_change;
mod reconciliation;
mod session;
mod state;

pub use connection::{RetainedSqliteInspection, SqliteDatabase};

use weavelit_server_database::{
    AccountAdministrationStore, AccountAuditReference, AccountCredentialWriterStore,
    AccountPublicIdentifier, AccountPublicIdentifierPersistence, AccountPublicIdentity,
    AccountStatusWriterStore, ApplicationDatabase, ApplicationState, AuditReferencePersistence,
    AuditTerminalRecoveryStore, ComponentEnablement, DatabaseError, DatabaseInspection,
    DeploymentIdentifier, GroupAdministrationStore, GroupAuditReference, GroupMutationStore,
    HumanAuthorizationSnapshot, InitializedState, LogConfigurationAuditReference,
    LogConfigurationGenerationStore, LogConfigurationMutationStore, MfaPolicyWriterStore, MfaStore,
    PasswordChangeWriterStore, ReconciliationDigest, ReconciliationStore, SessionStore,
    StateIdentifier, WorkflowCheckpoint,
};

impl ApplicationDatabase for SqliteDatabase {
    fn inspect(
        &mut self,
        expected_deployment_identifier: DeploymentIdentifier,
    ) -> Result<DatabaseInspection, DatabaseError> {
        SqliteDatabase::inspect(self, expected_deployment_identifier)
    }

    fn create_checkpoint(&mut self, checkpoint: &WorkflowCheckpoint) -> Result<(), DatabaseError> {
        self.create_checkpoint_atomic(checkpoint)
    }

    fn complete_checkpoint(
        &mut self,
        public_identity_persistence: &AccountPublicIdentifierPersistence,
        checkpoint: &WorkflowCheckpoint,
        state: &ApplicationState,
        reconciliation: &ReconciliationDigest,
    ) -> Result<(), DatabaseError> {
        self.complete_checkpoint_atomic(
            public_identity_persistence,
            checkpoint,
            state,
            reconciliation,
        )
    }

    fn load_initialized_state(
        &mut self,
        public_identity_persistence: &AccountPublicIdentifierPersistence,
        audit_reference_persistence: &AuditReferencePersistence,
        expected_deployment_identifier: DeploymentIdentifier,
    ) -> Result<InitializedState, DatabaseError> {
        self.load_initialized_state_atomic(
            public_identity_persistence,
            audit_reference_persistence,
            expected_deployment_identifier,
        )
    }

    fn load_account_public_identity(
        &mut self,
        persistence: &AccountPublicIdentifierPersistence,
        public_identifier: AccountPublicIdentifier,
    ) -> Result<Option<AccountPublicIdentity>, DatabaseError> {
        self.load_account_public_identity_atomic(persistence, public_identifier)
    }

    fn acknowledge_completion(
        &mut self,
        expected_deployment_identifier: DeploymentIdentifier,
        record_identifier: StateIdentifier,
    ) -> Result<(), DatabaseError> {
        self.acknowledge_completion_atomic(expected_deployment_identifier, record_identifier)
    }

    fn load_human_authorization(
        &mut self,
        account: StateIdentifier,
    ) -> Result<Option<HumanAuthorizationSnapshot>, DatabaseError> {
        self.load_human_authorization_atomic(account)
    }

    fn load_account_audit_reference(
        &mut self,
        persistence: &AuditReferencePersistence,
        account: StateIdentifier,
    ) -> Result<Option<AccountAuditReference>, DatabaseError> {
        self.load_account_audit_reference_atomic(persistence, account)
    }

    fn load_group_audit_reference(
        &mut self,
        persistence: &AuditReferencePersistence,
        group: StateIdentifier,
    ) -> Result<Option<GroupAuditReference>, DatabaseError> {
        self.load_group_audit_reference_atomic(persistence, group)
    }

    fn load_log_configuration_audit_reference(
        &mut self,
        persistence: &AuditReferencePersistence,
        configuration: StateIdentifier,
    ) -> Result<Option<LogConfigurationAuditReference>, DatabaseError> {
        self.load_log_configuration_audit_reference_atomic(persistence, configuration)
    }

    fn load_component_enablement(&mut self) -> Result<ComponentEnablement, DatabaseError> {
        self.load_component_enablement_atomic()
    }

    fn sessions(&mut self) -> Option<&mut dyn SessionStore> {
        Some(self)
    }

    fn mfa(&mut self) -> Option<&mut dyn MfaStore> {
        Some(self)
    }

    fn reconciliation(&mut self) -> Option<&mut dyn ReconciliationStore> {
        Some(self)
    }

    fn audit_terminal_recovery(&mut self) -> Option<&mut dyn AuditTerminalRecoveryStore> {
        Some(self)
    }

    fn account_administration(&mut self) -> Option<&mut dyn AccountAdministrationStore> {
        Some(self)
    }

    fn group_administration(&mut self) -> Option<&mut dyn GroupAdministrationStore> {
        Some(self)
    }

    fn account_credential_writers(&mut self) -> Option<&mut dyn AccountCredentialWriterStore> {
        Some(self)
    }

    fn password_change_writers(&mut self) -> Option<&mut dyn PasswordChangeWriterStore> {
        Some(self)
    }

    fn account_status_writers(&mut self) -> Option<&mut dyn AccountStatusWriterStore> {
        Some(self)
    }

    fn mfa_policy_writers(&mut self) -> Option<&mut dyn MfaPolicyWriterStore> {
        Some(self)
    }

    fn group_mutations(&mut self) -> Option<&mut dyn GroupMutationStore> {
        Some(self)
    }

    fn log_configuration_generations(
        &mut self,
    ) -> Option<&mut dyn LogConfigurationGenerationStore> {
        Some(self)
    }

    fn log_configuration_mutations(&mut self) -> Option<&mut dyn LogConfigurationMutationStore> {
        Some(self)
    }

    fn close(self: Box<Self>) -> Result<(), DatabaseError> {
        SqliteDatabase::close(*self)
    }
}
