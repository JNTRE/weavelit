//! Server-owned Restore orchestration.
//!
//! The cryptographic validation crate and the lifecycle typestate chain each
//! own one half of a Restore. This module is the only place that joins them:
//! it authorizes the workflow, validates the submitted backup, builds the
//! replacement application state, moves the deployment through its checkpoint,
//! delivers the System Log acknowledgement to the destination the restored
//! backup itself declares, seals the deployment, and activates normal
//! operation in-process.
//!
//! This module owns no transport. Its entry point takes already-received bytes
//! so the HTTP upload route can be composed over it without changing any
//! ordering guarantee established here.

use std::{
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use tokio::{sync::Semaphore, task};
use weavelit_server_lifecycle::{
    ApplicationState, BackendCatalog, CheckpointMetadata, DeploymentIdentifier, InitializedState,
    LifecycleError, StateIdentifier, TrustedBackendContext, WorkflowArbiter, WorkflowError,
    WorkflowKind, WorkflowPermit,
};
use weavelit_server_log::{
    CompleteLogRecord, ConfiguredLogDestination, LogModuleCatalog, LogModuleIdentifier,
    TrustedLogModuleContext, TrustedRecordIssuer,
};
use weavelit_server_log_authority::ServerLogAuthority;
use weavelit_server_observability::ServerObservability;
use weavelit_server_restore::{
    AvailableComponents, LogAssignment, LogModuleConfiguration, LogType, RequestBudget,
    RestoreAuthority, RestoreError, RestoreRequest, RestoreTarget, RestoreValidator,
    ValidatedBackup, build_application_state,
};
use zeroize::Zeroizing;

use crate::{RestrictedStartup, ServingModeSwitch};

/// Opaque non-secret checkpoint metadata this workflow writes.
///
/// The lifecycle crate stores it without interpretation, so it carries only
/// the workflow's own fixed marker and never any backup content.
const RESTORE_CHECKPOINT_METADATA: &[u8] = b"weavelit.restore.v1";

/// Server-owned composition that runs one Restore at a time.
///
/// It shares the startup composition's `WorkflowArbiter` and mutation lane, so
/// a Restore serializes against pre-operational database selection rather than
/// racing it.
pub struct RestoreOrchestrator {
    arbiter: Arc<WorkflowArbiter>,
    catalog: Arc<BackendCatalog>,
    context: Arc<TrustedBackendContext>,
    log_catalog: Arc<LogModuleCatalog>,
    state_root: PathBuf,
    mutation_lane: Arc<Semaphore>,
    serving_modes: Arc<ServingModeSwitch>,
    validator: RestoreValidator,
    observability: ServerObservability,
    /// Retained privately so no caller outside this composition can mint a
    /// trusted Log Module context or a trusted record issuer.
    log_authority: ServerLogAuthority,
}

impl RestoreOrchestrator {
    /// Composes Restore over a restricted startup's lifecycle authority.
    ///
    /// `components` is the inventory a backup may reference. It is supplied by
    /// the composing runtime rather than derived here, because the compiled-in
    /// Client Module, MFA Module, Service Module, and operation inventory is
    /// not yet reported by a single Server-owned registry.
    #[must_use]
    pub fn new(
        startup: &RestrictedStartup,
        components: AvailableComponents,
        serving_modes: Arc<ServingModeSwitch>,
    ) -> Arc<Self> {
        let log_authority = ServerLogAuthority::new();
        let observability =
            ServerObservability::new(TrustedRecordIssuer::from_server_authority(&log_authority));

        Arc::new(Self {
            arbiter: Arc::clone(&startup.composition.adapter.arbiter),
            catalog: Arc::clone(&startup.composition.catalog),
            context: Arc::clone(&startup.composition.context),
            log_catalog: Arc::clone(&startup.log_catalog),
            state_root: startup.state_root.clone(),
            mutation_lane: Arc::clone(&startup.composition.adapter.mutation_lane),
            serving_modes,
            validator: RestoreValidator::new(components),
            observability,
            log_authority,
        })
    }

    /// Runs one Restore from an already-received artifact and recovery key.
    ///
    /// The request budget starts before anything else, so validation deadlines
    /// cover the whole operation. Ownership of both sensitive inputs is taken
    /// so they can be cleared inside the operation instead of outliving it in
    /// a caller's buffer.
    ///
    /// Returns the sealed deployment's loaded state on success. The Server is
    /// already serving its operational surface by then.
    pub async fn restore(
        self: &Arc<Self>,
        artifact: Vec<u8>,
        recovery_key: String,
    ) -> Result<InitializedState, RestoreError> {
        let budget = RequestBudget::start();

        // Fail fast instead of queueing: a Restore that waited here would hold
        // its artifact and recovery key resident for the whole wait, and the
        // Restore contract already admits only one operation at a time.
        let lane = Arc::clone(&self.mutation_lane)
            .try_acquire_owned()
            .map_err(|_| RestoreError::RestorePending)?;

        let artifact = Zeroizing::new(artifact);
        let recovery_key = Zeroizing::new(recovery_key);
        let orchestrator = Arc::clone(self);

        // The entire authorize-through-seal chain is blocking work. Running it
        // in one closure keeps the workflow permit, the checkpoint, and the
        // seal on a single thread and outside any cancellation point, so no
        // caller timeout can abandon the deployment mid-replacement.
        task::spawn_blocking(move || {
            let _lane = lane;
            orchestrator.run(&budget, artifact, recovery_key)
        })
        .await
        .map_err(|_| RestoreError::RestoreFailed)?
    }

    /// Runs the blocking Restore chain.
    fn run(
        &self,
        budget: &RequestBudget,
        artifact: Zeroizing<Vec<u8>>,
        recovery_key: Zeroizing<String>,
    ) -> Result<InitializedState, RestoreError> {
        let permit = self
            .arbiter
            .authorize_workflow(&self.catalog, &self.context)
            .map_err(map_workflow_error)?;

        // The lifecycle authority is consulted before any sensitive input is
        // read: this target exists only because `authorize_workflow` already
        // re-verified the deployment record and the selected database under
        // the exclusive permit held for the rest of this operation.
        let authority = AuthorizedTarget(RestoreTarget::new(
            permit.deployment_identifier(),
            permit.selected_backend().clone(),
        ));

        let validated = self.validator.validate(
            &authority,
            budget,
            RestoreRequest {
                artifact: &artifact,
                recovery_key: &recovery_key,
            },
        );
        // Neither input is needed after validation, so both are cleared here
        // rather than at the end of the operation.
        drop(recovery_key);
        drop(artifact);
        let validated = validated?;

        let deployment_identifier = validated.deployment_identifier();
        let record_identifier = StateIdentifier::from_bytes(random_bytes()?)
            .map_err(|_| RestoreError::RestoreFailed)?;
        let correlation_identifier = correlation_identifier()?;

        let (record, obligation) = self
            .observability
            .prepare_restore_completion(
                record_identifier,
                deployment_identifier,
                event_time_milliseconds()?,
                &correlation_identifier,
            )
            .map_err(|_| RestoreError::RestoreFailed)?
            .into_parts();

        let state = build_application_state(&validated, permit.sealer(), obligation)?;
        // Resolved before the point of no return so a backup naming a Log
        // Module this Server cannot serve fails while failing is still free.
        let log_module = system_log_module(&validated)?;
        drop(validated);

        // Point of no return begins here. Every later connection observes the
        // fail-closed surface before any durable state changes, and keeps
        // observing it if the replacement does not complete.
        self.serving_modes.publish_fail_closed();

        let state = self.replace_state(
            permit,
            &state,
            &log_module,
            deployment_identifier,
            record_identifier,
            &record,
        )?;

        self.serving_modes
            .publish_operational(weavelit_module_client_webui::operational_surface());
        Ok(state)
    }

    /// Replaces retained state atomically and seals the deployment.
    ///
    /// A failure anywhere in this chain leaves the Server fail-closed with the
    /// interrupted state retained. No rollback is attempted, because the
    /// replaced state is exactly what an operator asked to discard.
    fn replace_state(
        &self,
        permit: WorkflowPermit<'_>,
        state: &ApplicationState,
        log_module: &LogModuleIdentifier,
        deployment_identifier: DeploymentIdentifier,
        record_identifier: StateIdentifier,
        record: &CompleteLogRecord,
    ) -> Result<InitializedState, RestoreError> {
        let metadata = CheckpointMetadata::from_bytes(RESTORE_CHECKPOINT_METADATA)
            .map_err(|_| RestoreError::RestoreFailed)?;

        let committed = permit
            .create_checkpoint(WorkflowKind::Restore, metadata)
            .map_err(map_workflow_error)?
            .complete_checkpoint(state)
            .map_err(map_workflow_error)?;

        // Opened only now: creating the Log Module's local storage before the
        // checkpoint would write durable state a pre-checkpoint failure had
        // promised not to leave behind.
        self.open_system_log(log_module, deployment_identifier)?
            .deliver(record)
            .map_err(|_| RestoreError::RestoreFailed)?;

        committed
            .acknowledge_completion(record_identifier)
            .map_err(map_workflow_error)?
            .seal()
            .map_err(map_workflow_error)
    }

    /// Builds the System Log destination under the trusted Log Module context.
    fn open_system_log(
        &self,
        module: &LogModuleIdentifier,
        deployment_identifier: DeploymentIdentifier,
    ) -> Result<ConfiguredLogDestination, RestoreError> {
        let context = TrustedLogModuleContext::from_server_authority(
            &self.log_authority,
            self.state_root.clone(),
            *deployment_identifier.as_bytes(),
        );
        self.log_catalog
            .create_destination(module, &context)
            .map_err(|_| RestoreError::RestoreFailed)
    }
}

/// Restore authority backed by an already-granted workflow permit.
struct AuthorizedTarget(RestoreTarget);

impl RestoreAuthority for AuthorizedTarget {
    fn authorize(&self) -> Result<RestoreTarget, RestoreError> {
        Ok(self.0.clone())
    }
}

/// Selects the Log Module the restored backup assigns the System Log to.
///
/// Reads only the validated in-memory backup, so the acknowledgement is
/// delivered under the configuration being restored rather than any
/// configuration this Server retained before the Restore.
fn system_log_module(validated: &ValidatedBackup) -> Result<LogModuleIdentifier, RestoreError> {
    let backup = validated.backup();
    let configuration = assigned_configuration(
        backup.log_module_configurations(),
        backup.log_assignments(),
        LogType::System,
    )
    .ok_or(RestoreError::BackupIncompatible)?;

    LogModuleIdentifier::new(configuration.module.as_str())
        .map_err(|_| RestoreError::BackupIncompatible)
}

/// Returns the enabled configuration assigned to one log type.
fn assigned_configuration<'backup>(
    configurations: &'backup [LogModuleConfiguration],
    assignments: &[LogAssignment],
    log_type: LogType,
) -> Option<&'backup LogModuleConfiguration> {
    let assignment = assignments
        .iter()
        .find(|assignment| assignment.log_type == log_type)?;
    configurations.iter().find(|configuration| {
        configuration.identifier == assignment.configuration && configuration.enabled
    })
}

/// Maps a lifecycle workflow failure to a stable Restore failure category.
fn map_workflow_error(error: WorkflowError) -> RestoreError {
    match error {
        WorkflowError::Lifecycle(error) => RestoreError::Lifecycle(error),
        WorkflowError::AlreadyPending => RestoreError::RestorePending,
        _ => RestoreError::Lifecycle(LifecycleError::InvalidState),
    }
}

/// Fills a fixed-size buffer from operating-system randomness.
fn random_bytes<const BYTES: usize>() -> Result<[u8; BYTES], RestoreError> {
    let mut bytes = [0_u8; BYTES];
    getrandom::fill(&mut bytes).map_err(|_| RestoreError::RestoreFailed)?;
    Ok(bytes)
}

/// Generates a correlation identifier that carries no request content.
fn correlation_identifier() -> Result<String, RestoreError> {
    const HEX: [u8; 16] = *b"0123456789abcdef";

    let entropy = random_bytes::<16>()?;
    let mut identifier = String::with_capacity(entropy.len() * 2);
    for byte in entropy {
        identifier.push(char::from(HEX[usize::from(byte >> 4)]));
        identifier.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    Ok(identifier)
}

/// Reads the current UTC event time in Unix milliseconds.
fn event_time_milliseconds() -> Result<i64, RestoreError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
        .ok_or(RestoreError::RestoreFailed)
}
