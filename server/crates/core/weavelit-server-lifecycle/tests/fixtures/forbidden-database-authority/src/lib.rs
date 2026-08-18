use weavelit_server_database::{
    AccountAuditReference, ApplicationDatabase, ApplicationState, AuditReferencePersistence,
    ComponentEnablement, DatabaseError, DatabaseInspection, DeploymentIdentifier,
    GroupAuditReference, HumanAuthorizationSnapshot, InitializedState, MfaStore,
    ReconciliationDigest, ReconciliationStore, SessionStore, StateIdentifier, WorkflowCheckpoint,
};

pub struct ExternalDatabase;

impl ApplicationDatabase for ExternalDatabase {
    fn inspect(
        &mut self,
        _expected_deployment_identifier: DeploymentIdentifier,
    ) -> Result<DatabaseInspection, DatabaseError> {
        Ok(DatabaseInspection::Uninitialized)
    }

    fn create_checkpoint(&mut self, _checkpoint: &WorkflowCheckpoint) -> Result<(), DatabaseError> {
        Ok(())
    }

    fn complete_checkpoint(
        &mut self,
        _checkpoint: &WorkflowCheckpoint,
        _state: &ApplicationState,
        _reconciliation: &ReconciliationDigest,
    ) -> Result<(), DatabaseError> {
        Err(DatabaseError::InvalidState)
    }

    fn load_initialized_state(
        &mut self,
        _persistence: &AuditReferencePersistence,
        _expected_deployment_identifier: DeploymentIdentifier,
    ) -> Result<InitializedState, DatabaseError> {
        Err(DatabaseError::NotInitialized)
    }

    fn acknowledge_completion(
        &mut self,
        _expected_deployment_identifier: DeploymentIdentifier,
        _record_identifier: StateIdentifier,
    ) -> Result<(), DatabaseError> {
        Err(DatabaseError::NotInitialized)
    }

    fn load_human_authorization(
        &mut self,
        _account: StateIdentifier,
    ) -> Result<Option<HumanAuthorizationSnapshot>, DatabaseError> {
        Ok(None)
    }

    fn load_account_audit_reference(
        &mut self,
        _persistence: &AuditReferencePersistence,
        _account: StateIdentifier,
    ) -> Result<Option<AccountAuditReference>, DatabaseError> {
        Ok(None)
    }

    fn load_group_audit_reference(
        &mut self,
        _persistence: &AuditReferencePersistence,
        _group: StateIdentifier,
    ) -> Result<Option<GroupAuditReference>, DatabaseError> {
        Ok(None)
    }

    fn load_component_enablement(&mut self) -> Result<ComponentEnablement, DatabaseError> {
        Ok(ComponentEnablement::default())
    }

    fn sessions(&mut self) -> Option<&mut dyn SessionStore> {
        None
    }

    fn mfa(&mut self) -> Option<&mut dyn MfaStore> {
        None
    }

    fn reconciliation(&mut self) -> Option<&mut dyn ReconciliationStore> {
        None
    }

    fn close(self: Box<Self>) -> Result<(), DatabaseError> {
        Ok(())
    }
}
