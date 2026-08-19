#![forbid(unsafe_code)]

//! SQLite implementation of the Weavelit Application Database contract.

mod audit_recovery;
mod checkpoint;
mod completion;
mod connection;
mod error;
mod inspection;
mod mfa;
mod migrations;
mod reconciliation;
mod session;
mod state;

pub use connection::{RetainedSqliteInspection, SqliteDatabase};

use weavelit_server_database::{
    AccountAuditReference, ApplicationDatabase, ApplicationState, AuditReferencePersistence,
    AuditTerminalRecoveryStore, ComponentEnablement, DatabaseError, DatabaseInspection,
    DeploymentIdentifier, GroupAuditReference, HumanAuthorizationSnapshot, InitializedState,
    MfaStore, ReconciliationDigest, ReconciliationStore, SessionStore, StateIdentifier,
    WorkflowCheckpoint,
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
        checkpoint: &WorkflowCheckpoint,
        state: &ApplicationState,
        reconciliation: &ReconciliationDigest,
    ) -> Result<(), DatabaseError> {
        self.complete_checkpoint_atomic(checkpoint, state, reconciliation)
    }

    fn load_initialized_state(
        &mut self,
        persistence: &AuditReferencePersistence,
        expected_deployment_identifier: DeploymentIdentifier,
    ) -> Result<InitializedState, DatabaseError> {
        self.load_initialized_state_atomic(persistence, expected_deployment_identifier)
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

    fn close(self: Box<Self>) -> Result<(), DatabaseError> {
        SqliteDatabase::close(*self)
    }
}
