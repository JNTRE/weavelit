//! Operational Audit assignment resolution and terminal recovery draining.
//!
//! Runtime resolves one committed Audit assignment into an inseparable binding
//! and destination, then performs one bounded drain on activation or immediately
//! before a consequential workflow. It owns no timer, queue, or client mapping.

use std::{
    fmt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};

use weavelit_server_audit::ServerAudit;
use weavelit_server_database::{
    AuditTerminalObligation, AuditTerminalRecoveryPersistence, AuditTerminalRecoveryStore,
    AuditTerminalReplayBatchSize, DatabaseError, DeploymentIdentifier, InitializedState,
    LogAssignment, LogModuleConfiguration, LogType, MAX_AUDIT_TERMINAL_REPLAY_BATCH_SIZE,
};
use weavelit_server_log::{
    AuditDestinationBinding, AuditTerminalReplayError, ConfiguredLogDestination,
    DestinationSettings, LogModuleCatalog, LogModuleIdentifier, ResolvedAuditDestination,
    TrustedLogModuleContext, TrustedRecordIssuer,
};
use weavelit_server_log_authority::ServerLogAuthority;

use crate::{
    operational::{OperationalDatabase, OperationalRuntime},
    operational_logging::OperationalLogSupport,
};

const INITIAL_AUDIT_BINDING_VERSION: u64 = 1;

/// Result of draining one independently ordered recovery sequence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AuditRecoverySequenceState {
    /// No obligation remains in the bounded sequence read.
    Ready,
    /// Exact delivery remains pending or another bounded batch may remain.
    Pending,
    /// Recovery cannot proceed without repairing trusted state or a dependency.
    RecoveryRequired,
}

/// Internal recovery state observed at activation or a pre-consequential gate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OperationalAuditRecoveryState {
    active: AuditRecoverySequenceState,
    late_delivery: AuditRecoverySequenceState,
}

impl OperationalAuditRecoveryState {
    /// Returns whether active obligations permit a consequential workflow to proceed.
    #[must_use]
    #[allow(dead_code)]
    pub(crate) const fn active(self) -> AuditRecoverySequenceState {
        self.active
    }

    /// Returns the independently drained degraded-completeness sequence state.
    #[must_use]
    #[allow(dead_code)]
    pub(crate) const fn late_delivery(self) -> AuditRecoverySequenceState {
        self.late_delivery
    }
}

/// Event-driven coordinator for live, non-restorable Audit terminal recovery.
pub(crate) struct OperationalAuditRecovery {
    database: OperationalDatabase,
    producer: ServerAudit,
    log_catalog: Arc<weavelit_server_log::LogModuleCatalog>,
    state_root: PathBuf,
    deployment_identifier: DeploymentIdentifier,
    reporting: OperationalAuditReporting,
    #[cfg(test)]
    destination_override: Option<OperationalAuditDestination>,
    drain_permit: Mutex<()>,
    state: Mutex<OperationalAuditRecoveryState>,
}

impl OperationalAuditRecovery {
    /// Resolves the immutable version-1 Audit assignment for one operational deployment.
    pub(crate) fn new(
        runtime: &OperationalRuntime,
        state: &InitializedState,
        database: OperationalDatabase,
    ) -> Self {
        let authority = ServerLogAuthority::new();
        let producer = ServerAudit::new(TrustedRecordIssuer::from_server_authority(&authority));
        let reporting = OperationalAuditReporting::new(runtime, state);
        Self {
            database,
            producer,
            log_catalog: Arc::clone(&runtime.log_catalog),
            state_root: runtime.state_root.clone(),
            deployment_identifier: state.deployment_identifier(),
            reporting,
            #[cfg(test)]
            destination_override: None,
            drain_permit: Mutex::new(()),
            state: Mutex::new(OperationalAuditRecoveryState {
                active: AuditRecoverySequenceState::RecoveryRequired,
                late_delivery: AuditRecoverySequenceState::RecoveryRequired,
            }),
        }
    }

    /// Runs one bounded activation drain without deciding whether the Server may start.
    pub(crate) fn drain_for_activation(&self) -> OperationalAuditRecoveryState {
        self.drain()
    }

    /// Runs one bounded drain before a consequential workflow makes its mutation decision.
    #[allow(dead_code)]
    pub(crate) fn drain_before_consequential_operation(&self) -> OperationalAuditRecoveryState {
        self.drain()
    }

    fn drain(&self) -> OperationalAuditRecoveryState {
        let _permit = self
            .drain_permit
            .lock()
            .unwrap_or_else(PoisonError::into_inner);

        #[cfg(test)]
        if let Some(destination) = self.destination_override.as_ref() {
            return self.drain_resolved(destination);
        }

        let initialized = match self
            .database
            .load_initialized_state(self.deployment_identifier)
        {
            Ok(initialized) => initialized,
            Err(_) => {
                self.reporting.report(None);
                return self.recovery_required();
            }
        };
        let destination = match OperationalAuditDestination::resolve(
            &self.log_catalog,
            &self.state_root,
            &initialized,
            ServerLogAuthority::new(),
        ) {
            Ok(destination) => destination,
            Err(_) => {
                self.reporting
                    .report(assigned_module(&initialized, LogType::Audit).as_ref());
                return self.recovery_required();
            }
        };
        self.drain_resolved(&destination)
    }

    fn drain_resolved(
        &self,
        destination: &OperationalAuditDestination,
    ) -> OperationalAuditRecoveryState {
        self.replace_state(OperationalAuditRecoveryState {
            active: self.drain_sequence(RecoverySequence::Active, destination),
            late_delivery: self.drain_sequence(RecoverySequence::LateDelivery, destination),
        })
    }

    fn replace_state(&self, next: OperationalAuditRecoveryState) -> OperationalAuditRecoveryState {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        *state = next;
        *state
    }

    fn recovery_required(&self) -> OperationalAuditRecoveryState {
        self.replace_state(OperationalAuditRecoveryState {
            active: AuditRecoverySequenceState::RecoveryRequired,
            late_delivery: AuditRecoverySequenceState::RecoveryRequired,
        })
    }

    fn drain_sequence(
        &self,
        sequence: RecoverySequence,
        destination: &OperationalAuditDestination,
    ) -> AuditRecoverySequenceState {
        let batch_size = AuditTerminalReplayBatchSize::new(MAX_AUDIT_TERMINAL_REPLAY_BATCH_SIZE)
            .expect("the contract maximum is a valid replay batch size");
        let obligations = match self
            .database
            .with_audit_terminal_recovery(|persistence, store| {
                sequence.list(store, persistence, batch_size)
            }) {
            Ok(obligations) => obligations,
            Err(_) => {
                self.reporting.report(Some(destination.module()));
                return AuditRecoverySequenceState::RecoveryRequired;
            }
        };
        let batch_may_have_more = obligations.len() == batch_size.get();

        for obligation in obligations {
            let recovered = match self.producer.restore_terminal_recovery(&obligation) {
                Ok(recovered) => recovered,
                Err(_) => {
                    self.reporting.report(Some(destination.module()));
                    return AuditRecoverySequenceState::RecoveryRequired;
                }
            };
            let acknowledgement = match recovered.deliver(&destination.resolved()) {
                Ok(acknowledgement) => acknowledgement,
                Err(AuditTerminalReplayError::DeliveryPending(_)) => {
                    self.reporting.report(Some(destination.module()));
                    return AuditRecoverySequenceState::Pending;
                }
                Err(AuditTerminalReplayError::DestinationBindingChanged) => {
                    self.reporting.report(Some(destination.module()));
                    return AuditRecoverySequenceState::RecoveryRequired;
                }
            };
            if self
                .database
                .with_audit_terminal_recovery(|_, store| acknowledgement.acknowledge(store))
                .is_err()
            {
                self.reporting.report(Some(destination.module()));
                return AuditRecoverySequenceState::RecoveryRequired;
            }
        }

        if batch_may_have_more {
            AuditRecoverySequenceState::Pending
        } else {
            AuditRecoverySequenceState::Ready
        }
    }
}

/// Best-effort System Log support retained independently of Audit resolution.
struct OperationalAuditReporting {
    support: OperationalLogSupport,
    fallback_module: LogModuleIdentifier,
}

impl OperationalAuditReporting {
    fn new(runtime: &OperationalRuntime, state: &InitializedState) -> Self {
        let authority = ServerLogAuthority::new();
        let system_log = OperationalLogDestination::resolve(
            &runtime.log_catalog,
            &runtime.state_root,
            state,
            LogType::System,
            &authority,
        )
        .ok()
        .map(|resolved| Arc::new(resolved.destination));
        Self {
            support: OperationalLogSupport::new(
                TrustedRecordIssuer::from_server_authority(&authority),
                system_log,
            ),
            fallback_module: assigned_module(state, LogType::Audit)
                .unwrap_or_else(unresolved_module),
        }
    }

    fn report(&self, destination_module: Option<&LogModuleIdentifier>) {
        self.support.report_audit_terminal_recovery_failure(
            destination_module.unwrap_or(&self.fallback_module),
        );
    }
}

/// One configured destination resolved from a trusted assignment and configuration.
struct OperationalLogDestination {
    module: LogModuleIdentifier,
    configuration_identifier: [u8; 16],
    destination: ConfiguredLogDestination,
}

impl OperationalLogDestination {
    fn resolve(
        log_catalog: &LogModuleCatalog,
        state_root: &Path,
        state: &InitializedState,
        log_type: LogType,
        authority: &ServerLogAuthority,
    ) -> Result<Self, OperationalAuditResolutionError> {
        let configuration = assigned_configuration(
            state.state().log_module_configurations(),
            state.state().log_assignments(),
            log_type,
        )?;
        let module = LogModuleIdentifier::new(configuration.module.as_str())
            .map_err(|_| OperationalAuditResolutionError)?;
        let settings = DestinationSettings::new(
            configuration
                .settings
                .iter()
                .map(|setting| {
                    (
                        setting.key.as_str().to_owned(),
                        setting.value.as_str().to_owned(),
                    )
                })
                .collect(),
        )
        .map_err(|_| OperationalAuditResolutionError)?;
        let context = TrustedLogModuleContext::from_server_authority(
            authority,
            state_root.to_path_buf(),
            *state.deployment_identifier().as_bytes(),
        )
        .with_settings(settings);
        let destination = log_catalog
            .create_destination(&module, &context)
            .map_err(|_| OperationalAuditResolutionError)?;
        Ok(Self {
            module,
            configuration_identifier: *configuration.identifier.as_bytes(),
            destination,
        })
    }
}

impl fmt::Debug for OperationalAuditRecovery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OperationalAuditRecovery(REDACTED)")
    }
}

/// Runtime-owned binding and destination derived from one committed assignment.
struct OperationalAuditDestination {
    authority: ServerLogAuthority,
    binding: AuditDestinationBinding,
    module: LogModuleIdentifier,
    destination: ConfiguredLogDestination,
}

impl OperationalAuditDestination {
    fn resolve(
        log_catalog: &weavelit_server_log::LogModuleCatalog,
        state_root: &Path,
        state: &InitializedState,
        authority: ServerLogAuthority,
    ) -> Result<Self, OperationalAuditResolutionError> {
        let resolved = OperationalLogDestination::resolve(
            log_catalog,
            state_root,
            state,
            LogType::Audit,
            &authority,
        )?;
        let binding = AuditDestinationBinding::from_server_authority(
            &authority,
            resolved.configuration_identifier,
            INITIAL_AUDIT_BINDING_VERSION,
        )
        .map_err(|_| OperationalAuditResolutionError)?;
        Ok(Self {
            authority,
            binding,
            module: resolved.module,
            destination: resolved.destination,
        })
    }

    fn module(&self) -> &LogModuleIdentifier {
        &self.module
    }

    fn resolved(&self) -> ResolvedAuditDestination<'_> {
        ResolvedAuditDestination::from_server_authority(
            &self.authority,
            &self.binding,
            &self.destination,
        )
    }
}

fn assigned_configuration<'a>(
    configurations: &'a [LogModuleConfiguration],
    assignments: &[LogAssignment],
    log_type: LogType,
) -> Result<&'a LogModuleConfiguration, OperationalAuditResolutionError> {
    let mut assigned = assignments
        .iter()
        .filter(|assignment| assignment.log_type == log_type);
    let assignment = assigned.next().ok_or(OperationalAuditResolutionError)?;
    if assigned.next().is_some() {
        return Err(OperationalAuditResolutionError);
    }
    configurations
        .iter()
        .find(|configuration| {
            configuration.identifier == assignment.configuration && configuration.enabled
        })
        .ok_or(OperationalAuditResolutionError)
}

fn assigned_module(state: &InitializedState, log_type: LogType) -> Option<LogModuleIdentifier> {
    assigned_configuration(
        state.state().log_module_configurations(),
        state.state().log_assignments(),
        log_type,
    )
    .ok()
    .and_then(|configuration| LogModuleIdentifier::new(configuration.module.as_str()).ok())
}

fn unresolved_module() -> LogModuleIdentifier {
    LogModuleIdentifier::new("unresolved").expect("the fixed fallback module is valid")
}

impl fmt::Debug for OperationalAuditDestination {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OperationalAuditDestination(REDACTED)")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct OperationalAuditResolutionError;

#[derive(Clone, Copy)]
enum RecoverySequence {
    Active,
    LateDelivery,
}

impl RecoverySequence {
    fn list(
        self,
        store: &mut dyn AuditTerminalRecoveryStore,
        persistence: &AuditTerminalRecoveryPersistence,
        batch_size: AuditTerminalReplayBatchSize,
    ) -> Result<Vec<AuditTerminalObligation>, DatabaseError> {
        match self {
            Self::Active => store.list_pending_audit_terminal_obligations(persistence, batch_size),
            Self::LateDelivery => {
                store.list_late_delivery_audit_terminal_obligations(persistence, batch_size)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        path::{Path, PathBuf},
        sync::{
            Arc, Condvar, Mutex, PoisonError,
            atomic::{AtomicUsize, Ordering},
            mpsc,
        },
        thread,
        time::Duration,
    };

    use weavelit_server_audit::{
        AccountStatus, AuditActor, AuditEvent, AuditOutcomeDetail, ServerAudit, StateChangeOutcome,
    };
    use weavelit_server_database::{
        AccountAuditReference, ApplicationState, ApplicationStateInput, AuditReferenceIdentifier,
        AuditReferencePersistence, AuditTerminalObligation, AuditTerminalRecoveryPersistence,
        AuditTerminalRecoveryStore, AuditTerminalReplayBatchSize, CompletionObligation,
        ComponentEnablement, ConfigurationKey, ConfigurationValue, CorrelationIdentifier,
        DatabaseError, DatabaseInspection, DeploymentIdentifier, GroupAuditReference,
        HumanAuthorizationSnapshot, InitializedState, LogAssignment, LogClassification, LogDetail,
        LogModuleConfiguration, LogModuleSetting, LogType, MAX_AUDIT_TERMINAL_REPLAY_BATCH_SIZE,
        MfaStore, Name, ReconciliationDigest, ReconciliationStore, RecoveryPublicKey, SessionStore,
        StateIdentifier, WorkflowCheckpoint, WorkflowKind,
    };
    use weavelit_server_database_authority::ServerDatabaseAuthority;
    use weavelit_server_lifecycle::ApplicationDatabase;
    use weavelit_server_log::{
        AuditDestinationBinding, AuditTerminalDeliveryAcknowledgement, CompleteLogRecord,
        ConfiguredLogDestination, CorrelationId, DurableAcknowledgement, EventTime,
        LogCapabilities, LogDestination, LogDestinationError, LogDestinationFactory,
        LogModuleCatalog, LogModuleFactoryContext, LogModuleIdentifier, LogModuleRegistration,
        LogRecordPersistenceView, LogRecordType, LogSettingsContract, TrustedLogModuleContext,
        TrustedRecordIssuer,
    };
    use weavelit_server_log_authority::ServerLogAuthority;

    use super::{
        AuditRecoverySequenceState, OperationalAuditDestination, OperationalAuditRecovery,
        OperationalAuditRecoveryState, OperationalAuditReporting, assigned_configuration,
    };
    use crate::{operational::OperationalDatabase, operational_logging::OperationalLogSupport};

    const RECOVERY_PUBLIC_KEY: &str =
        "age1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqsm5xurc";

    #[derive(Default)]
    struct StoreState {
        active: VecDeque<AuditTerminalObligation>,
        late_delivery: VecDeque<AuditTerminalObligation>,
        initialized_state: Option<InitializedState>,
        initialized_state_loads: usize,
        active_list_calls: usize,
        late_list_calls: usize,
        acknowledgement_calls: usize,
        fail_acknowledgement: bool,
        fail_active_list: bool,
        fail_late_list: bool,
    }

    struct FakeRecoveryStore {
        state: Arc<Mutex<StoreState>>,
    }

    impl AuditTerminalRecoveryStore for FakeRecoveryStore {
        fn list_pending_audit_terminal_obligations(
            &mut self,
            _persistence: &AuditTerminalRecoveryPersistence,
            batch_size: AuditTerminalReplayBatchSize,
        ) -> Result<Vec<AuditTerminalObligation>, DatabaseError> {
            let mut state = self.state.lock().expect("the fake store must not poison");
            state.active_list_calls += 1;
            if state.fail_active_list {
                return Err(DatabaseError::Unavailable);
            }
            Ok(state
                .active
                .iter()
                .take(batch_size.get())
                .cloned()
                .collect())
        }

        fn list_late_delivery_audit_terminal_obligations(
            &mut self,
            _persistence: &AuditTerminalRecoveryPersistence,
            batch_size: AuditTerminalReplayBatchSize,
        ) -> Result<Vec<AuditTerminalObligation>, DatabaseError> {
            let mut state = self.state.lock().expect("the fake store must not poison");
            state.late_list_calls += 1;
            if state.fail_late_list {
                return Err(DatabaseError::Unavailable);
            }
            Ok(state
                .late_delivery
                .iter()
                .take(batch_size.get())
                .cloned()
                .collect())
        }

        fn acknowledge_audit_terminal_obligation(
            &mut self,
            acknowledgement: AuditTerminalDeliveryAcknowledgement,
        ) -> Result<(), DatabaseError> {
            let mut state = self.state.lock().expect("the fake store must not poison");
            state.acknowledgement_calls += 1;
            if state.fail_acknowledgement {
                return Err(DatabaseError::Unavailable);
            }
            if state
                .active
                .front()
                .map(AuditTerminalObligation::identifier)
                .map(|identifier| *identifier.as_bytes())
                == Some(*acknowledgement.record_id())
            {
                state.active.pop_front();
                return Ok(());
            }
            if state
                .late_delivery
                .front()
                .map(AuditTerminalObligation::identifier)
                .map(|identifier| *identifier.as_bytes())
                == Some(*acknowledgement.record_id())
            {
                state.late_delivery.pop_front();
                return Ok(());
            }
            Err(DatabaseError::InvalidState)
        }
    }

    struct RecoveryDatabase {
        store: Option<FakeRecoveryStore>,
    }

    impl ApplicationDatabase for RecoveryDatabase {
        fn inspect(
            &mut self,
            _expected_deployment_identifier: DeploymentIdentifier,
        ) -> Result<DatabaseInspection, DatabaseError> {
            Err(DatabaseError::Unavailable)
        }

        fn create_checkpoint(
            &mut self,
            _checkpoint: &WorkflowCheckpoint,
        ) -> Result<(), DatabaseError> {
            Err(DatabaseError::Unavailable)
        }

        fn complete_checkpoint(
            &mut self,
            _checkpoint: &WorkflowCheckpoint,
            _state: &ApplicationState,
            _reconciliation: &ReconciliationDigest,
        ) -> Result<(), DatabaseError> {
            Err(DatabaseError::Unavailable)
        }

        fn load_initialized_state(
            &mut self,
            _persistence: &AuditReferencePersistence,
            expected_deployment_identifier: DeploymentIdentifier,
        ) -> Result<InitializedState, DatabaseError> {
            let store = self.store.as_ref().ok_or(DatabaseError::Unavailable)?;
            let mut state = store.state.lock().map_err(|_| DatabaseError::Unavailable)?;
            state.initialized_state_loads += 1;
            let initialized = state
                .initialized_state
                .clone()
                .ok_or(DatabaseError::Unavailable)?;
            if initialized.deployment_identifier() != expected_deployment_identifier {
                return Err(DatabaseError::InvalidState);
            }
            Ok(initialized)
        }

        fn acknowledge_completion(
            &mut self,
            _expected_deployment_identifier: DeploymentIdentifier,
            _record_identifier: StateIdentifier,
        ) -> Result<(), DatabaseError> {
            Err(DatabaseError::Unavailable)
        }

        fn load_human_authorization(
            &mut self,
            _account: StateIdentifier,
        ) -> Result<Option<HumanAuthorizationSnapshot>, DatabaseError> {
            Err(DatabaseError::Unavailable)
        }

        fn load_account_audit_reference(
            &mut self,
            _persistence: &AuditReferencePersistence,
            _account: StateIdentifier,
        ) -> Result<Option<AccountAuditReference>, DatabaseError> {
            Err(DatabaseError::Unavailable)
        }

        fn load_group_audit_reference(
            &mut self,
            _persistence: &AuditReferencePersistence,
            _group: StateIdentifier,
        ) -> Result<Option<GroupAuditReference>, DatabaseError> {
            Err(DatabaseError::Unavailable)
        }

        fn load_component_enablement(&mut self) -> Result<ComponentEnablement, DatabaseError> {
            Err(DatabaseError::Unavailable)
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

        fn audit_terminal_recovery(&mut self) -> Option<&mut dyn AuditTerminalRecoveryStore> {
            self.store
                .as_mut()
                .map(|store| store as &mut dyn AuditTerminalRecoveryStore)
        }

        fn close(self: Box<Self>) -> Result<(), DatabaseError> {
            Ok(())
        }
    }

    struct RecordingFactory {
        delivered: Arc<Mutex<Vec<[u8; 16]>>>,
        attempts: Arc<AtomicUsize>,
        fail_on_attempt: Option<usize>,
        reenter: Arc<Mutex<Option<OperationalDatabase>>>,
    }

    impl LogDestinationFactory for RecordingFactory {
        fn accepted_settings(&self) -> LogSettingsContract {
            LogSettingsContract::none()
        }

        fn create(
            &self,
            _context: &LogModuleFactoryContext<'_>,
        ) -> Result<Box<dyn LogDestination>, LogDestinationError> {
            Ok(Box::new(RecordingDestination {
                delivered: Arc::clone(&self.delivered),
                attempts: Arc::clone(&self.attempts),
                fail_on_attempt: self.fail_on_attempt,
                reenter: Arc::clone(&self.reenter),
            }))
        }
    }

    struct RecordingDestination {
        delivered: Arc<Mutex<Vec<[u8; 16]>>>,
        attempts: Arc<AtomicUsize>,
        fail_on_attempt: Option<usize>,
        reenter: Arc<Mutex<Option<OperationalDatabase>>>,
    }

    impl LogDestination for RecordingDestination {
        fn deliver(
            &self,
            record: &CompleteLogRecord,
            acknowledgement: DurableAcknowledgement,
        ) -> Result<DurableAcknowledgement, LogDestinationError> {
            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst) + 1;
            if self.fail_on_attempt == Some(attempt) {
                return Err(LogDestinationError::Unavailable);
            }
            if let Some(database) = self
                .reenter
                .lock()
                .expect("the reentry slot must not poison")
                .clone()
            {
                let (completed, completion) = mpsc::channel();
                std::thread::spawn(move || {
                    let _ = completed.send(database.with(|_| ()));
                });
                match completion.recv_timeout(Duration::from_secs(1)) {
                    Ok(Ok(())) => {}
                    Ok(Err(_)) | Err(_) => return Err(LogDestinationError::Unavailable),
                }
            }
            let LogRecordPersistenceView::Audit(view) = record.persistence_view() else {
                return Err(LogDestinationError::IntegrityFailure);
            };
            self.delivered
                .lock()
                .expect("the delivery log must not poison")
                .push(*view.record_id().as_bytes());
            Ok(acknowledgement)
        }

        fn preflight(&self, _record_type: LogRecordType) -> Result<(), LogDestinationError> {
            Ok(())
        }
    }

    struct AcknowledgingFactory;

    impl LogDestinationFactory for AcknowledgingFactory {
        fn accepted_settings(&self) -> LogSettingsContract {
            LogSettingsContract::none()
        }

        fn create(
            &self,
            _context: &LogModuleFactoryContext<'_>,
        ) -> Result<Box<dyn LogDestination>, LogDestinationError> {
            Ok(Box::new(AcknowledgingDestination))
        }
    }

    struct AcknowledgingDestination;

    impl LogDestination for AcknowledgingDestination {
        fn deliver(
            &self,
            _record: &CompleteLogRecord,
            acknowledgement: DurableAcknowledgement,
        ) -> Result<DurableAcknowledgement, LogDestinationError> {
            Ok(acknowledgement)
        }

        fn preflight(&self, _record_type: LogRecordType) -> Result<(), LogDestinationError> {
            Ok(())
        }
    }

    struct ControlledFactory {
        attempts: Arc<AtomicUsize>,
        arrivals: mpsc::Sender<usize>,
        first_delivery_release: Arc<(Mutex<bool>, Condvar)>,
    }

    impl LogDestinationFactory for ControlledFactory {
        fn accepted_settings(&self) -> LogSettingsContract {
            LogSettingsContract::none()
        }

        fn create(
            &self,
            _context: &LogModuleFactoryContext<'_>,
        ) -> Result<Box<dyn LogDestination>, LogDestinationError> {
            Ok(Box::new(ControlledDestination {
                attempts: Arc::clone(&self.attempts),
                arrivals: self.arrivals.clone(),
                first_delivery_release: Arc::clone(&self.first_delivery_release),
            }))
        }
    }

    struct ControlledDestination {
        attempts: Arc<AtomicUsize>,
        arrivals: mpsc::Sender<usize>,
        first_delivery_release: Arc<(Mutex<bool>, Condvar)>,
    }

    impl LogDestination for ControlledDestination {
        fn deliver(
            &self,
            _record: &CompleteLogRecord,
            acknowledgement: DurableAcknowledgement,
        ) -> Result<DurableAcknowledgement, LogDestinationError> {
            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst) + 1;
            self.arrivals
                .send(attempt)
                .map_err(|_| LogDestinationError::Unavailable)?;
            if attempt == 1 {
                let (released, wake) = &*self.first_delivery_release;
                let mut released = released.lock().unwrap_or_else(PoisonError::into_inner);
                while !*released {
                    released = wake.wait(released).unwrap_or_else(PoisonError::into_inner);
                }
            }
            Ok(acknowledgement)
        }

        fn preflight(&self, _record_type: LogRecordType) -> Result<(), LogDestinationError> {
            Ok(())
        }
    }

    #[derive(Debug, Eq, PartialEq)]
    struct ReportedSystemRecord {
        classification: String,
        detail: String,
        correlation_identifier: String,
    }

    struct SystemRecordingFactory {
        records: Arc<Mutex<Vec<ReportedSystemRecord>>>,
        attempts: Arc<AtomicUsize>,
    }

    impl LogDestinationFactory for SystemRecordingFactory {
        fn accepted_settings(&self) -> LogSettingsContract {
            LogSettingsContract::none()
        }

        fn create(
            &self,
            _context: &LogModuleFactoryContext<'_>,
        ) -> Result<Box<dyn LogDestination>, LogDestinationError> {
            Ok(Box::new(SystemRecordingDestination {
                records: Arc::clone(&self.records),
                attempts: Arc::clone(&self.attempts),
            }))
        }
    }

    struct SystemRecordingDestination {
        records: Arc<Mutex<Vec<ReportedSystemRecord>>>,
        attempts: Arc<AtomicUsize>,
    }

    impl LogDestination for SystemRecordingDestination {
        fn deliver(
            &self,
            record: &CompleteLogRecord,
            acknowledgement: DurableAcknowledgement,
        ) -> Result<DurableAcknowledgement, LogDestinationError> {
            self.attempts.fetch_add(1, Ordering::SeqCst);
            let LogRecordPersistenceView::System(view) = record.persistence_view() else {
                return Err(LogDestinationError::IntegrityFailure);
            };
            self.records
                .lock()
                .expect("the System record log must not poison")
                .push(ReportedSystemRecord {
                    classification: view.body().classification().to_owned(),
                    detail: view.body().detail().to_owned(),
                    correlation_identifier: view.correlation_id().as_str().to_owned(),
                });
            Ok(acknowledgement)
        }

        fn preflight(&self, _record_type: LogRecordType) -> Result<(), LogDestinationError> {
            Ok(())
        }
    }

    struct SettingsFactory {
        received: Arc<Mutex<Option<String>>>,
    }

    impl SettingsFactory {
        fn contract() -> LogSettingsContract {
            LogSettingsContract::new(vec!["endpoint".to_owned()])
                .expect("the settings contract is valid")
        }
    }

    impl LogDestinationFactory for SettingsFactory {
        fn accepted_settings(&self) -> LogSettingsContract {
            Self::contract()
        }

        fn create(
            &self,
            context: &LogModuleFactoryContext<'_>,
        ) -> Result<Box<dyn LogDestination>, LogDestinationError> {
            if !Self::contract().accepts(context.settings()) {
                return Err(LogDestinationError::ConfigurationInvalid);
            }
            let endpoint = context
                .settings()
                .get("endpoint")
                .ok_or(LogDestinationError::ConfigurationInvalid)?;
            *self
                .received
                .lock()
                .expect("the settings recorder must not poison") = Some(endpoint.to_owned());
            Ok(Box::new(AcknowledgingDestination))
        }
    }

    fn persistence() -> AuditTerminalRecoveryPersistence {
        AuditTerminalRecoveryPersistence::from_server_authority(&ServerDatabaseAuthority::new())
    }

    fn identifier(byte: u8) -> StateIdentifier {
        StateIdentifier::from_bytes([byte; 16]).expect("the test identifier is nonzero")
    }

    fn producer(authority: &ServerLogAuthority) -> ServerAudit {
        ServerAudit::new(TrustedRecordIssuer::from_server_authority(authority))
    }

    fn acknowledging_destination() -> ConfiguredLogDestination {
        let authority = ServerLogAuthority::new();
        let catalog = LogModuleCatalog::new(vec![LogModuleRegistration::new(
            "acknowledging",
            LogCapabilities::new(vec![LogRecordType::Audit]).expect("the capability is valid"),
            Box::new(AcknowledgingFactory),
        )])
        .expect("the catalog is valid");
        catalog
            .create_destination(
                &LogModuleIdentifier::new("acknowledging").expect("the identifier is valid"),
                &TrustedLogModuleContext::from_server_authority(
                    &authority,
                    PathBuf::from("/unused"),
                    [0x71; 16],
                ),
            )
            .expect("the destination opens")
    }

    fn obligation(binding: &AuditDestinationBinding, value: u8) -> AuditTerminalObligation {
        let authority = ServerLogAuthority::new();
        let producer = producer(&authority);
        let account = AccountAuditReference::new(
            identifier(value),
            AuditReferenceIdentifier::generate().expect("the Audit reference generates"),
        );
        let attempt = producer
            .prepare_attempt(
                EventTime::from_unix_milliseconds(u64::from(value) * 2),
                CorrelationId::new(format!("recovery-{value}")).expect("the correlation is valid"),
                AuditActor::Human(account),
                AuditEvent::AuthenticationUserDisabled { account },
            )
            .expect("the attempt prepares")
            .deliver(&acknowledging_destination())
            .expect("the attempt delivers");
        producer
            .prepare_completion(
                &attempt,
                EventTime::from_unix_milliseconds(u64::from(value) * 2 + 1),
                AuditOutcomeDetail::AuthenticationUserDisabled(StateChangeOutcome::Succeeded(
                    AccountStatus::Disabled,
                )),
            )
            .expect("the terminal prepares")
            .recovery_obligation(&persistence(), binding)
            .expect("the obligation exports")
    }

    struct DestinationFixture {
        binding: AuditDestinationBinding,
        destination: OperationalAuditDestination,
        delivered: Arc<Mutex<Vec<[u8; 16]>>>,
        attempts: Arc<AtomicUsize>,
        reenter: Arc<Mutex<Option<OperationalDatabase>>>,
    }

    struct ControlledDestinationFixture {
        binding: AuditDestinationBinding,
        destination: OperationalAuditDestination,
        attempts: Arc<AtomicUsize>,
        arrivals: mpsc::Receiver<usize>,
        first_delivery_release: Arc<(Mutex<bool>, Condvar)>,
    }

    type RecordingCatalogFixture = (
        Arc<LogModuleCatalog>,
        Arc<Mutex<Vec<[u8; 16]>>>,
        Arc<AtomicUsize>,
    );

    fn operational_destination(
        binding_identifier: [u8; 16],
        fail_on_attempt: Option<usize>,
    ) -> DestinationFixture {
        let authority = ServerLogAuthority::new();
        let binding =
            AuditDestinationBinding::from_server_authority(&authority, binding_identifier, 1)
                .expect("the binding is valid");
        let delivered = Arc::new(Mutex::new(Vec::new()));
        let attempts = Arc::new(AtomicUsize::new(0));
        let reenter = Arc::new(Mutex::new(None));
        let catalog = LogModuleCatalog::new(vec![LogModuleRegistration::new(
            "recording",
            LogCapabilities::new(vec![LogRecordType::Audit]).expect("the capability is valid"),
            Box::new(RecordingFactory {
                delivered: Arc::clone(&delivered),
                attempts: Arc::clone(&attempts),
                fail_on_attempt,
                reenter: Arc::clone(&reenter),
            }),
        )])
        .expect("the catalog is valid");
        let destination = catalog
            .create_destination(
                &LogModuleIdentifier::new("recording").expect("the identifier is valid"),
                &TrustedLogModuleContext::from_server_authority(
                    &authority,
                    PathBuf::from("/unused"),
                    [0x72; 16],
                ),
            )
            .expect("the destination opens");
        DestinationFixture {
            binding: binding.clone(),
            destination: OperationalAuditDestination {
                authority,
                binding,
                module: LogModuleIdentifier::new("recording")
                    .expect("the module identifier is valid"),
                destination,
            },
            delivered,
            attempts,
            reenter,
        }
    }

    fn controlled_destination(binding_identifier: [u8; 16]) -> ControlledDestinationFixture {
        let authority = ServerLogAuthority::new();
        let binding =
            AuditDestinationBinding::from_server_authority(&authority, binding_identifier, 1)
                .expect("the binding is valid");
        let attempts = Arc::new(AtomicUsize::new(0));
        let (arrivals, arrived) = mpsc::channel();
        let first_delivery_release = Arc::new((Mutex::new(false), Condvar::new()));
        let catalog = LogModuleCatalog::new(vec![LogModuleRegistration::new(
            "controlled",
            LogCapabilities::new(vec![LogRecordType::Audit]).expect("the capability is valid"),
            Box::new(ControlledFactory {
                attempts: Arc::clone(&attempts),
                arrivals,
                first_delivery_release: Arc::clone(&first_delivery_release),
            }),
        )])
        .expect("the catalog is valid");
        let destination = catalog
            .create_destination(
                &LogModuleIdentifier::new("controlled").expect("the identifier is valid"),
                &TrustedLogModuleContext::from_server_authority(
                    &authority,
                    PathBuf::from("/unused"),
                    [0x73; 16],
                ),
            )
            .expect("the destination opens");
        ControlledDestinationFixture {
            binding: binding.clone(),
            destination: OperationalAuditDestination {
                authority,
                binding,
                module: LogModuleIdentifier::new("controlled")
                    .expect("the module identifier is valid"),
                destination,
            },
            attempts,
            arrivals: arrived,
            first_delivery_release,
        }
    }

    fn recording_catalog(fail_on_attempt: Option<usize>) -> RecordingCatalogFixture {
        let delivered = Arc::new(Mutex::new(Vec::new()));
        let attempts = Arc::new(AtomicUsize::new(0));
        let catalog = LogModuleCatalog::new(vec![LogModuleRegistration::new(
            "recording",
            LogCapabilities::new(vec![LogRecordType::Audit]).expect("the capability is valid"),
            Box::new(RecordingFactory {
                delivered: Arc::clone(&delivered),
                attempts: Arc::clone(&attempts),
                fail_on_attempt,
                reenter: Arc::new(Mutex::new(None)),
            }),
        )])
        .expect("the catalog is valid");
        (Arc::new(catalog), delivered, attempts)
    }

    fn database(state: Arc<Mutex<StoreState>>) -> OperationalDatabase {
        OperationalDatabase::from_open(Box::new(RecoveryDatabase {
            store: Some(FakeRecoveryStore { state }),
        }))
    }

    fn recovery(
        database: OperationalDatabase,
        destination: OperationalAuditDestination,
    ) -> OperationalAuditRecovery {
        recovery_with_reporting(database, destination, reporting_without_system())
    }

    fn recovery_with_reporting(
        database: OperationalDatabase,
        destination: OperationalAuditDestination,
        reporting: OperationalAuditReporting,
    ) -> OperationalAuditRecovery {
        let authority = ServerLogAuthority::new();
        OperationalAuditRecovery {
            database,
            producer: producer(&authority),
            log_catalog: unused_catalog(),
            state_root: PathBuf::from("/unused"),
            deployment_identifier: DeploymentIdentifier::from_bytes([0x42; 16])
                .expect("the deployment identifier is valid"),
            reporting,
            destination_override: Some(destination),
            drain_permit: Mutex::new(()),
            state: Mutex::new(OperationalAuditRecoveryState {
                active: AuditRecoverySequenceState::RecoveryRequired,
                late_delivery: AuditRecoverySequenceState::RecoveryRequired,
            }),
        }
    }

    fn recording_reporting() -> (
        OperationalAuditReporting,
        Arc<Mutex<Vec<ReportedSystemRecord>>>,
        Arc<AtomicUsize>,
    ) {
        let records = Arc::new(Mutex::new(Vec::new()));
        let attempts = Arc::new(AtomicUsize::new(0));
        let authority = ServerLogAuthority::new();
        let catalog = LogModuleCatalog::new(vec![LogModuleRegistration::new(
            "system-recording",
            LogCapabilities::new(vec![LogRecordType::System]).expect("the capability is valid"),
            Box::new(SystemRecordingFactory {
                records: Arc::clone(&records),
                attempts: Arc::clone(&attempts),
            }),
        )])
        .expect("the System catalog is valid");
        let destination = catalog
            .create_destination(
                &LogModuleIdentifier::new("system-recording")
                    .expect("the module identifier is valid"),
                &TrustedLogModuleContext::from_server_authority(
                    &authority,
                    PathBuf::from("/unused"),
                    [0x42; 16],
                ),
            )
            .expect("the System destination opens");
        let reporting = OperationalAuditReporting {
            support: OperationalLogSupport::new(
                TrustedRecordIssuer::from_server_authority(&authority),
                Some(Arc::new(destination)),
            ),
            fallback_module: LogModuleIdentifier::new("recording")
                .expect("the module identifier is valid"),
        };
        (reporting, records, attempts)
    }

    fn reporting_without_system() -> OperationalAuditReporting {
        let authority = ServerLogAuthority::new();
        OperationalAuditReporting {
            support: OperationalLogSupport::new(
                TrustedRecordIssuer::from_server_authority(&authority),
                None,
            ),
            fallback_module: LogModuleIdentifier::new("recording")
                .expect("the module identifier is valid"),
        }
    }

    fn unused_catalog() -> Arc<LogModuleCatalog> {
        Arc::new(
            LogModuleCatalog::new(vec![LogModuleRegistration::new(
                "acknowledging",
                LogCapabilities::new(vec![LogRecordType::Audit]).expect("the capability is valid"),
                Box::new(AcknowledgingFactory),
            )])
            .expect("the test catalog is valid"),
        )
    }

    #[test]
    fn activation_drains_active_then_late_oldest_first_without_holding_the_database_lane() {
        let DestinationFixture {
            binding,
            destination,
            delivered,
            reenter,
            ..
        } = operational_destination([0x31; 16], None);
        let active = [obligation(&binding, 1), obligation(&binding, 2)];
        let late = [obligation(&binding, 3), obligation(&binding, 4)];
        let expected = active
            .iter()
            .chain(&late)
            .map(|obligation| *obligation.identifier().as_bytes())
            .collect::<Vec<_>>();
        let store = Arc::new(Mutex::new(StoreState {
            active: active.into_iter().collect(),
            late_delivery: late.into_iter().collect(),
            ..StoreState::default()
        }));
        let database = database(Arc::clone(&store));
        *reenter.lock().expect("the reentry slot must not poison") = Some(database.clone());

        let state = recovery(database, destination).drain_for_activation();

        assert_eq!(state.active(), AuditRecoverySequenceState::Ready);
        assert_eq!(state.late_delivery(), AuditRecoverySequenceState::Ready);
        assert_eq!(
            *delivered.lock().expect("the delivery log must not poison"),
            expected
        );
        let store = store.lock().expect("the fake store must not poison");
        assert!(store.active.is_empty());
        assert!(store.late_delivery.is_empty());
    }

    #[test]
    fn concurrent_drains_deliver_and_acknowledge_one_obligation_once() {
        let ControlledDestinationFixture {
            binding,
            destination,
            attempts,
            arrivals,
            first_delivery_release,
        } = controlled_destination([0x3D; 16]);
        let store = Arc::new(Mutex::new(StoreState {
            active: VecDeque::from([obligation(&binding, 92)]),
            ..StoreState::default()
        }));
        let recovery = Arc::new(recovery(database(Arc::clone(&store)), destination));

        let first_recovery = Arc::clone(&recovery);
        let first = thread::spawn(move || first_recovery.drain_before_consequential_operation());
        assert_eq!(
            arrivals.recv_timeout(Duration::from_secs(1)),
            Ok(1),
            "the first drain must reach destination delivery"
        );

        let (second_started, second_is_running) = mpsc::channel();
        let second_recovery = Arc::clone(&recovery);
        let second = thread::spawn(move || {
            second_started
                .send(())
                .expect("the test must observe the second drain start");
            second_recovery.drain_before_consequential_operation()
        });
        second_is_running
            .recv_timeout(Duration::from_secs(1))
            .expect("the second drain must start");
        let concurrent_delivery = arrivals.recv_timeout(Duration::from_secs(1));

        let (released, wake) = &*first_delivery_release;
        *released.lock().unwrap_or_else(PoisonError::into_inner) = true;
        wake.notify_all();

        let first = first.join().expect("the first drain must not panic");
        let second = second.join().expect("the second drain must not panic");
        assert!(
            concurrent_delivery.is_err(),
            "the second drain must wait before destination delivery"
        );
        assert_eq!(first.active(), AuditRecoverySequenceState::Ready);
        assert_eq!(first.late_delivery(), AuditRecoverySequenceState::Ready);
        assert_eq!(second.active(), AuditRecoverySequenceState::Ready);
        assert_eq!(second.late_delivery(), AuditRecoverySequenceState::Ready);
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        let store = store.lock().expect("the fake store must not poison");
        assert_eq!(store.acknowledgement_calls, 1);
        assert_eq!(store.active_list_calls, 2);
        assert_eq!(store.late_list_calls, 2);
        assert!(store.active.is_empty());
    }

    #[test]
    fn a_full_bounded_batch_continues_on_the_next_drain_invocation() {
        let DestinationFixture {
            binding,
            destination,
            attempts,
            ..
        } = operational_destination([0x39; 16], None);
        let active = (0..MAX_AUDIT_TERMINAL_REPLAY_BATCH_SIZE)
            .map(|index| {
                obligation(
                    &binding,
                    u8::try_from(index + 20).expect("the bounded index fits"),
                )
            })
            .collect();
        let store = Arc::new(Mutex::new(StoreState {
            active,
            ..StoreState::default()
        }));
        let recovery = recovery(database(Arc::clone(&store)), destination);

        let first = recovery.drain_for_activation();
        assert_eq!(first.active(), AuditRecoverySequenceState::Pending);
        assert_eq!(first.late_delivery(), AuditRecoverySequenceState::Ready);
        assert!(
            store
                .lock()
                .expect("the fake store must not poison")
                .active
                .is_empty()
        );

        let second = recovery.drain_before_consequential_operation();
        assert_eq!(second.active(), AuditRecoverySequenceState::Ready);
        assert_eq!(second.late_delivery(), AuditRecoverySequenceState::Ready);
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            MAX_AUDIT_TERMINAL_REPLAY_BATCH_SIZE
        );
        let store = store.lock().expect("the fake store must not poison");
        assert_eq!(store.active_list_calls, 2);
        assert_eq!(store.late_list_calls, 2);
    }

    #[test]
    fn a_resolution_failure_is_not_latched_across_drain_invocations() {
        let configuration_identifier = identifier(0x3A);
        let authority = ServerLogAuthority::new();
        let binding = AuditDestinationBinding::from_server_authority(
            &authority,
            *configuration_identifier.as_bytes(),
            1,
        )
        .expect("the binding is valid");
        let store = Arc::new(Mutex::new(StoreState {
            active: VecDeque::from([obligation(&binding, 90)]),
            ..StoreState::default()
        }));
        let (log_catalog, delivered, attempts) = recording_catalog(None);
        let database = database(Arc::clone(&store));
        let recovery = OperationalAuditRecovery {
            database,
            producer: producer(&ServerLogAuthority::new()),
            log_catalog,
            state_root: PathBuf::from("/unused"),
            deployment_identifier: DeploymentIdentifier::from_bytes([0x42; 16])
                .expect("the deployment identifier is valid"),
            reporting: reporting_without_system(),
            destination_override: None,
            drain_permit: Mutex::new(()),
            state: Mutex::new(OperationalAuditRecoveryState {
                active: AuditRecoverySequenceState::Ready,
                late_delivery: AuditRecoverySequenceState::Ready,
            }),
        };

        let unresolved = recovery.drain_for_activation();
        assert_eq!(
            unresolved.active(),
            AuditRecoverySequenceState::RecoveryRequired
        );
        assert_eq!(attempts.load(Ordering::SeqCst), 0);

        store
            .lock()
            .expect("the fake store must not poison")
            .initialized_state = Some(initialized_state(configuration_identifier, "recording"));
        let repaired = recovery.drain_before_consequential_operation();

        assert_eq!(repaired.active(), AuditRecoverySequenceState::Ready);
        assert_eq!(repaired.late_delivery(), AuditRecoverySequenceState::Ready);
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert_eq!(
            delivered
                .lock()
                .expect("the delivery log must not poison")
                .len(),
            1
        );
        let store = store.lock().expect("the fake store must not poison");
        assert_eq!(store.initialized_state_loads, 2);
        assert!(store.active.is_empty());
    }

    #[test]
    fn delivery_failure_stops_only_its_sequence_and_retains_the_failed_oldest_obligation() {
        let DestinationFixture {
            binding,
            destination,
            delivered,
            attempts,
            ..
        } = operational_destination([0x32; 16], Some(2));
        let active = [
            obligation(&binding, 5),
            obligation(&binding, 6),
            obligation(&binding, 7),
        ];
        let failed_identifier = *active[1].identifier().as_bytes();
        let late = obligation(&binding, 8);
        let store = Arc::new(Mutex::new(StoreState {
            active: active.into_iter().collect(),
            late_delivery: VecDeque::from([late]),
            ..StoreState::default()
        }));

        let state = recovery(database(Arc::clone(&store)), destination)
            .drain_before_consequential_operation();

        assert_eq!(state.active(), AuditRecoverySequenceState::Pending);
        assert_eq!(state.late_delivery(), AuditRecoverySequenceState::Ready);
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
        assert_eq!(
            delivered
                .lock()
                .expect("the delivery log must not poison")
                .len(),
            2
        );
        let store = store.lock().expect("the fake store must not poison");
        assert_eq!(store.active.len(), 2);
        assert_eq!(
            store
                .active
                .front()
                .expect("the failed obligation remains")
                .identifier()
                .as_bytes(),
            &failed_identifier
        );
        assert!(store.late_delivery.is_empty());
    }

    #[test]
    fn delivery_failure_reports_one_safe_system_record() {
        let DestinationFixture {
            binding,
            destination,
            ..
        } = operational_destination([0x3B; 16], Some(1));
        let store = Arc::new(Mutex::new(StoreState {
            active: VecDeque::from([obligation(&binding, 91)]),
            ..StoreState::default()
        }));
        let (reporting, records, report_attempts) = recording_reporting();

        let state = recovery_with_reporting(database(store), destination, reporting)
            .drain_before_consequential_operation();

        assert_eq!(state.active(), AuditRecoverySequenceState::Pending);
        assert_eq!(state.late_delivery(), AuditRecoverySequenceState::Ready);
        assert_eq!(report_attempts.load(Ordering::SeqCst), 1);
        let records = records
            .lock()
            .expect("the System record log must not poison");
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0],
            ReportedSystemRecord {
                classification: "dependency.audit-log-unavailable".to_owned(),
                detail:
                    "audit destination module recording unavailable for internal.log-policy.changed"
                        .to_owned(),
                correlation_identifier: "audit-terminal-recovery".to_owned(),
            }
        );
        let rendered = format!("{records:?}");
        for forbidden in ["recovery-91", "database is locked", "request payload"] {
            assert!(!rendered.contains(forbidden), "{rendered}");
        }
    }

    #[test]
    fn resolution_failure_reports_once_without_committed_settings() {
        let configuration_identifier = identifier(0x3C);
        let store = Arc::new(Mutex::new(StoreState {
            initialized_state: Some(initialized_state_with_settings(
                configuration_identifier,
                "missing",
                vec![LogModuleSetting {
                    key: ConfigurationKey::new("endpoint").expect("the setting key is valid"),
                    value: ConfigurationValue::new("secret-setting-value")
                        .expect("the setting value is valid"),
                }],
            )),
            ..StoreState::default()
        }));
        let (log_catalog, _delivered, audit_attempts) = recording_catalog(None);
        let (reporting, records, report_attempts) = recording_reporting();
        let recovery = OperationalAuditRecovery {
            database: database(Arc::clone(&store)),
            producer: producer(&ServerLogAuthority::new()),
            log_catalog,
            state_root: PathBuf::from("/unused"),
            deployment_identifier: DeploymentIdentifier::from_bytes([0x42; 16])
                .expect("the deployment identifier is valid"),
            reporting,
            destination_override: None,
            drain_permit: Mutex::new(()),
            state: Mutex::new(OperationalAuditRecoveryState {
                active: AuditRecoverySequenceState::Ready,
                late_delivery: AuditRecoverySequenceState::Ready,
            }),
        };

        let state = recovery.drain_for_activation();

        assert_eq!(state.active(), AuditRecoverySequenceState::RecoveryRequired);
        assert_eq!(
            state.late_delivery(),
            AuditRecoverySequenceState::RecoveryRequired
        );
        assert_eq!(audit_attempts.load(Ordering::SeqCst), 0);
        assert_eq!(report_attempts.load(Ordering::SeqCst), 1);
        let records = records
            .lock()
            .expect("the System record log must not poison");
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].classification,
            "dependency.audit-log-unavailable"
        );
        assert_eq!(records[0].correlation_identifier, "audit-terminal-recovery");
        assert_eq!(
            records[0].detail,
            "audit destination module missing unavailable for internal.log-policy.changed"
        );
        let rendered = format!("{records:?}");
        for forbidden in ["secret-setting-value", "endpoint", "unknown module"] {
            assert!(!rendered.contains(forbidden), "{rendered}");
        }
        let store = store.lock().expect("the fake store must not poison");
        assert_eq!(store.initialized_state_loads, 1);
        assert_eq!(store.active_list_calls, 0);
        assert_eq!(store.late_list_calls, 0);
    }

    #[test]
    fn binding_mismatch_never_reaches_the_destination_or_acknowledgement() {
        let DestinationFixture {
            binding: current_binding,
            destination,
            attempts,
            ..
        } = operational_destination([0x33; 16], None);
        let authority = ServerLogAuthority::new();
        let retained_binding = AuditDestinationBinding::from_server_authority(
            &authority,
            [0x34; 16],
            current_binding.version(),
        )
        .expect("the retained binding is valid");
        let retained = obligation(&retained_binding, 9);
        let store = Arc::new(Mutex::new(StoreState {
            active: VecDeque::from([retained]),
            ..StoreState::default()
        }));

        let state = recovery(database(Arc::clone(&store)), destination)
            .drain_before_consequential_operation();

        assert_eq!(state.active(), AuditRecoverySequenceState::RecoveryRequired);
        assert_eq!(attempts.load(Ordering::SeqCst), 0);
        assert_eq!(
            store
                .lock()
                .expect("the fake store must not poison")
                .active
                .len(),
            1
        );
    }

    #[test]
    fn malformed_import_and_failed_acknowledgement_each_retain_the_obligation() {
        let DestinationFixture {
            binding,
            destination: malformed_destination,
            attempts: malformed_attempts,
            ..
        } = operational_destination([0x35; 16], None);
        let malformed = AuditTerminalObligation::from_persisted(
            &persistence(),
            [0xA1; 16],
            b"not-a-terminal-projection".to_vec(),
        )
        .expect("the opaque contract accepts bounded bytes");
        let malformed_store = Arc::new(Mutex::new(StoreState {
            active: VecDeque::from([malformed]),
            ..StoreState::default()
        }));
        let malformed_state = recovery(
            database(Arc::clone(&malformed_store)),
            malformed_destination,
        )
        .drain_for_activation();
        assert_eq!(
            malformed_state.active(),
            AuditRecoverySequenceState::RecoveryRequired
        );
        assert_eq!(malformed_attempts.load(Ordering::SeqCst), 0);
        assert_eq!(
            malformed_store
                .lock()
                .expect("the fake store must not poison")
                .active
                .len(),
            1
        );

        let DestinationFixture {
            destination: acknowledgement_destination,
            attempts: acknowledgement_attempts,
            ..
        } = operational_destination([0x35; 16], None);
        let acknowledgement_store = Arc::new(Mutex::new(StoreState {
            active: VecDeque::from([obligation(&binding, 10)]),
            fail_acknowledgement: true,
            ..StoreState::default()
        }));
        let acknowledgement_state = recovery(
            database(Arc::clone(&acknowledgement_store)),
            acknowledgement_destination,
        )
        .drain_for_activation();
        assert_eq!(
            acknowledgement_state.active(),
            AuditRecoverySequenceState::RecoveryRequired
        );
        assert_eq!(acknowledgement_attempts.load(Ordering::SeqCst), 1);
        assert_eq!(
            acknowledgement_store
                .lock()
                .expect("the fake store must not poison")
                .active
                .len(),
            1
        );
    }

    #[test]
    fn unavailable_store_is_recovery_required_without_preventing_repeated_gate_checks() {
        let DestinationFixture {
            destination,
            attempts,
            ..
        } = operational_destination([0x36; 16], None);
        let database = OperationalDatabase::from_open(Box::new(RecoveryDatabase { store: None }));
        let recovery = recovery(database, destination);

        for state in [
            recovery.drain_for_activation(),
            recovery.drain_before_consequential_operation(),
        ] {
            assert_eq!(state.active(), AuditRecoverySequenceState::RecoveryRequired);
            assert_eq!(
                state.late_delivery(),
                AuditRecoverySequenceState::RecoveryRequired
            );
        }
        assert_eq!(attempts.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn unresolved_assignment_requires_recovery_without_reading_or_changing_the_store() {
        let DestinationFixture { binding, .. } = operational_destination([0x38; 16], None);
        let store = Arc::new(Mutex::new(StoreState {
            active: VecDeque::from([obligation(&binding, 13)]),
            fail_active_list: true,
            fail_late_list: true,
            ..StoreState::default()
        }));
        let database = database(Arc::clone(&store));
        let authority = ServerLogAuthority::new();
        let recovery = OperationalAuditRecovery {
            database,
            producer: producer(&authority),
            log_catalog: unused_catalog(),
            state_root: PathBuf::from("/unused"),
            deployment_identifier: DeploymentIdentifier::from_bytes([0x42; 16])
                .expect("the deployment identifier is valid"),
            reporting: reporting_without_system(),
            destination_override: None,
            drain_permit: Mutex::new(()),
            state: Mutex::new(OperationalAuditRecoveryState {
                active: AuditRecoverySequenceState::Ready,
                late_delivery: AuditRecoverySequenceState::Ready,
            }),
        };

        let state = recovery.drain_for_activation();

        assert_eq!(state.active(), AuditRecoverySequenceState::RecoveryRequired);
        assert_eq!(
            state.late_delivery(),
            AuditRecoverySequenceState::RecoveryRequired
        );
        assert_eq!(
            store
                .lock()
                .expect("the fake store must not poison")
                .active
                .len(),
            1
        );
    }

    #[test]
    fn list_failure_stops_that_sequence_but_still_drains_the_other_sequence() {
        let DestinationFixture {
            binding,
            destination,
            delivered,
            ..
        } = operational_destination([0x37; 16], None);
        let active = obligation(&binding, 11);
        let late = obligation(&binding, 12);
        let late_identifier = *late.identifier().as_bytes();
        let store = Arc::new(Mutex::new(StoreState {
            active: VecDeque::from([active]),
            late_delivery: VecDeque::from([late]),
            fail_active_list: true,
            ..StoreState::default()
        }));

        let state = recovery(database(Arc::clone(&store)), destination).drain_for_activation();

        assert_eq!(state.active(), AuditRecoverySequenceState::RecoveryRequired);
        assert_eq!(state.late_delivery(), AuditRecoverySequenceState::Ready);
        assert_eq!(
            *delivered.lock().expect("the delivery log must not poison"),
            vec![late_identifier]
        );
        let store = store.lock().expect("the fake store must not poison");
        assert_eq!(store.active.len(), 1);
        assert!(store.late_delivery.is_empty());
    }

    #[test]
    fn committed_assignment_resolution_owns_version_one_binding_and_destination() {
        let delivered = Arc::new(Mutex::new(Vec::new()));
        let attempts = Arc::new(AtomicUsize::new(0));
        let catalog = LogModuleCatalog::new(vec![LogModuleRegistration::new(
            "recording",
            LogCapabilities::new(vec![LogRecordType::Audit]).expect("the capability is valid"),
            Box::new(RecordingFactory {
                delivered: Arc::clone(&delivered),
                attempts: Arc::clone(&attempts),
                fail_on_attempt: None,
                reenter: Arc::new(Mutex::new(None)),
            }),
        )])
        .expect("the catalog is valid");
        let state = initialized_state(identifier(0x41), "recording");
        let authority = ServerLogAuthority::new();

        let destination = OperationalAuditDestination::resolve(
            &catalog,
            PathBuf::from("/unused").as_path(),
            &state,
            authority,
        )
        .expect("the committed assignment resolves");

        assert_eq!(
            destination.binding.identifier(),
            identifier(0x41).as_bytes()
        );
        assert_eq!(destination.binding.version(), 1);
        assert_eq!(attempts.load(Ordering::SeqCst), 0);
        assert!(
            OperationalAuditDestination::resolve(
                &catalog,
                PathBuf::from("/unused").as_path(),
                &initialized_state(identifier(0x41), "unregistered"),
                ServerLogAuthority::new(),
            )
            .is_err()
        );
    }

    #[test]
    fn resolver_rejects_missing_and_disabled_assigned_configurations() {
        let configuration_identifier = identifier(0x43);
        let assignment = LogAssignment {
            log_type: LogType::Audit,
            configuration: configuration_identifier,
        };
        assert!(
            assigned_configuration(&[], std::slice::from_ref(&assignment), LogType::Audit).is_err()
        );

        let disabled = LogModuleConfiguration {
            identifier: configuration_identifier,
            module: Name::new("recording").expect("the module name is valid"),
            name: Name::new("audit-primary").expect("the configuration name is valid"),
            enabled: false,
            settings: Vec::new(),
        };
        assert!(assigned_configuration(&[disabled], &[assignment], LogType::Audit).is_err());
    }

    #[test]
    fn resolver_forwards_accepted_committed_settings_and_rejects_undeclared_keys() {
        let received = Arc::new(Mutex::new(None));
        let catalog = LogModuleCatalog::new(vec![LogModuleRegistration::new(
            "settings",
            LogCapabilities::new(vec![LogRecordType::Audit]).expect("the capability is valid"),
            Box::new(SettingsFactory {
                received: Arc::clone(&received),
            }),
        )])
        .expect("the catalog is valid");
        let accepted = initialized_state_with_settings(
            identifier(0x44),
            "settings",
            vec![LogModuleSetting {
                key: ConfigurationKey::new("endpoint").expect("the setting key is valid"),
                value: ConfigurationValue::new("audit-primary")
                    .expect("the setting value is valid"),
            }],
        );

        OperationalAuditDestination::resolve(
            &catalog,
            Path::new("/unused"),
            &accepted,
            ServerLogAuthority::new(),
        )
        .expect("the accepted committed setting resolves");
        assert_eq!(
            received
                .lock()
                .expect("the settings recorder must not poison")
                .as_deref(),
            Some("audit-primary")
        );

        let rejected = initialized_state_with_settings(
            identifier(0x45),
            "settings",
            vec![LogModuleSetting {
                key: ConfigurationKey::new("undeclared").expect("the setting key is valid"),
                value: ConfigurationValue::new("must-not-be-ignored")
                    .expect("the setting value is valid"),
            }],
        );
        assert!(
            OperationalAuditDestination::resolve(
                &catalog,
                Path::new("/unused"),
                &rejected,
                ServerLogAuthority::new(),
            )
            .is_err()
        );
    }

    fn initialized_state(
        configuration_identifier: StateIdentifier,
        module: &str,
    ) -> InitializedState {
        initialized_state_with_settings(configuration_identifier, module, Vec::new())
    }

    fn initialized_state_with_settings(
        configuration_identifier: StateIdentifier,
        module: &str,
        settings: Vec<LogModuleSetting>,
    ) -> InitializedState {
        let completion_obligation = CompletionObligation::new(
            identifier(0xF1),
            WorkflowKind::Restore,
            LogClassification::new("lifecycle.restore").expect("the classification is valid"),
            CorrelationIdentifier::new("recovery-composition").expect("the correlation is valid"),
            1,
            LogDetail::new("restore completed").expect("the detail is valid"),
        )
        .expect("the completion obligation is valid");
        let state = ApplicationState::new(ApplicationStateInput {
            configuration: Vec::new(),
            protected_secrets: Vec::new(),
            accounts: Vec::new(),
            account_audit_references: Vec::new(),
            password_verifiers: Vec::new(),
            groups: Vec::new(),
            group_audit_references: Vec::new(),
            group_memberships: Vec::new(),
            group_grants: Vec::new(),
            mfa_factors: Vec::new(),
            service_connections: Vec::new(),
            recovery_public_key: RecoveryPublicKey::new(RECOVERY_PUBLIC_KEY)
                .expect("the recovery key is valid"),
            log_module_configurations: vec![LogModuleConfiguration {
                identifier: configuration_identifier,
                module: Name::new(module).expect("the module name is valid"),
                name: Name::new("audit-primary").expect("the configuration name is valid"),
                enabled: true,
                settings,
            }],
            log_assignments: vec![
                LogAssignment {
                    log_type: LogType::System,
                    configuration: configuration_identifier,
                },
                LogAssignment {
                    log_type: LogType::Audit,
                    configuration: configuration_identifier,
                },
            ],
            completion_obligation,
        })
        .expect("the committed state is valid");
        InitializedState::new(
            DeploymentIdentifier::from_bytes([0x42; 16])
                .expect("the deployment identifier is valid"),
            state,
            true,
        )
    }
}
