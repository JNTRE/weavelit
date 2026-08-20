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
    LogAssignment, LogConfigurationGeneration, LogConfigurationGenerationKey,
    LogModuleConfiguration, LogType, MAX_AUDIT_TERMINAL_REPLAY_BATCH_SIZE,
    PreparedLogConfigurationMutation, StateIdentifier,
};
use weavelit_server_log::{
    AuditDestinationBinding, AuditTerminalReplayError, ConfiguredLogDestination,
    DestinationSettings, LogDeliveryError, LogModuleCatalog, LogModuleIdentifier, LogRecordType,
    ResolvedAuditDestination, TrustedLogModuleContext, TrustedRecordIssuer,
};
use weavelit_server_log_authority::ServerLogAuthority;

use crate::{
    operational::{OperationalDatabase, OperationalRuntime},
    operational_logging::OperationalLogSupport,
};

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
    log_catalog: Arc<LogModuleCatalog>,
    state_root: PathBuf,
    deployment_identifier: DeploymentIdentifier,
    reporting: OperationalAuditReporting,
    #[cfg(test)]
    destination_override: Option<OperationalAuditGenerationDestination>,
    drain_permit: Mutex<()>,
    state: Mutex<OperationalAuditRecoveryState>,
}

impl OperationalAuditRecovery {
    /// Composes recovery from one operational deployment and its committed assignments.
    pub(crate) fn new(
        runtime: &OperationalRuntime,
        state: &InitializedState,
        database: OperationalDatabase,
    ) -> Self {
        let authority = ServerLogAuthority::new();
        Self {
            database,
            producer: ServerAudit::new(TrustedRecordIssuer::from_server_authority(&authority)),
            log_catalog: Arc::clone(&runtime.log_catalog),
            state_root: runtime.state_root.clone(),
            deployment_identifier: state.deployment_identifier(),
            reporting: OperationalAuditReporting::new(runtime, state),
            #[cfg(test)]
            destination_override: None,
            drain_permit: Mutex::new(()),
            state: Mutex::new(OperationalAuditRecoveryState {
                active: AuditRecoverySequenceState::RecoveryRequired,
                late_delivery: AuditRecoverySequenceState::RecoveryRequired,
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        database: OperationalDatabase,
        log_catalog: Arc<LogModuleCatalog>,
        module: LogModuleIdentifier,
        destination: ConfiguredLogDestination,
    ) -> Self {
        let authority = ServerLogAuthority::new();
        let binding = AuditDestinationBinding::from_server_authority(&authority, [0x68; 16], 1)
            .expect("the fixed test binding is valid");
        let reporting_authority = ServerLogAuthority::new();
        Self {
            database,
            producer: ServerAudit::new(TrustedRecordIssuer::from_server_authority(&authority)),
            log_catalog,
            state_root: PathBuf::from("/unused"),
            deployment_identifier: DeploymentIdentifier::from_bytes([0x42; 16])
                .expect("the fixed deployment identifier is valid"),
            reporting: OperationalAuditReporting {
                support: OperationalLogSupport::new(
                    TrustedRecordIssuer::from_server_authority(&reporting_authority),
                    None,
                ),
                fallback_module: module.clone(),
            },
            destination_override: Some(OperationalAuditGenerationDestination {
                authority,
                binding,
                module,
                destination,
            }),
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

    /// Runs the post-commit bounded active-then-late recovery drain.
    pub(crate) fn drain_after_consequential_operation(&self) -> OperationalAuditRecoveryState {
        self.drain()
    }

    /// Borrows the producer that constructs records for the owning workflow.
    pub(crate) const fn producer(&self) -> &ServerAudit {
        &self.producer
    }

    /// Resolves and retains the exact current Audit generation for one workflow.
    pub(crate) fn with_current_destination<R>(
        &self,
        operation: impl FnOnce(&OperationalAuditGenerationDestination) -> R,
    ) -> Result<R, DatabaseError> {
        #[cfg(test)]
        if let Some(destination) = self.destination_override.as_ref() {
            return Ok(operation(destination));
        }

        let generation = self
            .database
            .load_current_audit_log_configuration_generation()?
            .ok_or(DatabaseError::Unavailable)?;
        let destination = OperationalAuditGenerationDestination::resolve(
            &self.log_catalog,
            &self.state_root,
            self.deployment_identifier,
            generation.key(),
            Some(&generation),
            ServerLogAuthority::new(),
        )
        .map_err(|_| DatabaseError::Unavailable)?;
        Ok(operation(&destination))
    }

    /// Resolves an exact still-current Audit generation and proves its commit path.
    pub(crate) fn with_expected_current_destination<R>(
        &self,
        expected: LogConfigurationGenerationKey,
        operation: impl FnOnce(&OperationalAuditGenerationDestination) -> R,
    ) -> Result<R, DatabaseError> {
        #[cfg(test)]
        if let Some(destination) = self.destination_override.as_ref() {
            if destination.binding.identifier() != expected.configuration().as_bytes()
                || destination.binding.version() != expected.version().get()
            {
                return Err(DatabaseError::Unavailable);
            }
            destination
                .destination
                .preflight(LogRecordType::Audit)
                .map_err(|_| DatabaseError::Unavailable)?;
            return Ok(operation(destination));
        }

        let generation = self
            .database
            .load_current_audit_log_configuration_generation()?
            .filter(|generation| generation.key() == expected)
            .ok_or(DatabaseError::Unavailable)?;
        let destination = OperationalAuditGenerationDestination::resolve(
            &self.log_catalog,
            &self.state_root,
            self.deployment_identifier,
            expected,
            Some(&generation),
            ServerLogAuthority::new(),
        )
        .map_err(|_| DatabaseError::Unavailable)?;
        destination
            .destination
            .preflight(LogRecordType::Audit)
            .map_err(|_| DatabaseError::Unavailable)?;
        Ok(operation(&destination))
    }

    /// Validates every resultant configuration and preflights both assigned commit paths.
    pub(crate) fn preflight_log_configuration_mutation(
        &self,
        mutation: &PreparedLogConfigurationMutation,
    ) -> Result<(), DatabaseError> {
        for generation in mutation.resultant_generations() {
            let module = LogModuleIdentifier::new(generation.module().as_str())
                .map_err(|_| DatabaseError::IntegrityFailure)?;
            let settings = destination_settings(generation)?;
            let declaration = self
                .log_catalog
                .declaration(&module)
                .ok_or(DatabaseError::IntegrityFailure)?;
            if !declaration.accepted_settings().accepts(&settings) {
                return Err(DatabaseError::IntegrityFailure);
            }
        }

        for assignment in mutation.desired_assignments() {
            let generation = mutation
                .resultant_generations()
                .iter()
                .find(|generation| generation.key().configuration() == assignment.configuration)
                .filter(|generation| generation.enabled())
                .ok_or(DatabaseError::IntegrityFailure)?;
            let module = LogModuleIdentifier::new(generation.module().as_str())
                .map_err(|_| DatabaseError::IntegrityFailure)?;
            let settings = destination_settings(generation)?;
            let record_type = log_record_type(assignment.log_type);
            let declaration = self
                .log_catalog
                .declaration(&module)
                .ok_or(DatabaseError::IntegrityFailure)?;
            if !declaration.capabilities().supports(record_type)
                || !declaration.accepted_settings().accepts(&settings)
            {
                return Err(DatabaseError::IntegrityFailure);
            }
            let context = TrustedLogModuleContext::from_server_authority(
                &ServerLogAuthority::new(),
                self.state_root.clone(),
                *self.deployment_identifier.as_bytes(),
            )
            .with_settings(settings);
            self.log_catalog
                .create_destination(&module, &context)
                .map_err(|_| DatabaseError::Unavailable)?
                .preflight(record_type)
                .map_err(|_| DatabaseError::Unavailable)?;
        }
        Ok(())
    }

    /// Best-effort reports an Attempt delivery failure through existing System Log support.
    pub(crate) fn reject_attempt_delivery(
        &self,
        error: LogDeliveryError,
        record_identifier: [u8; 16],
        event_time: u64,
        correlation_identifier: &str,
        destination_module: &LogModuleIdentifier,
        classification: weavelit_server_log::AuditLogClassification,
    ) {
        let Ok(record_identifier) = StateIdentifier::from_bytes(record_identifier) else {
            return;
        };
        let Ok(event_time) = i64::try_from(event_time) else {
            return;
        };
        let _ = self.reporting.support.reject_consequential_audit_failure(
            error,
            record_identifier,
            event_time,
            correlation_identifier,
            destination_module,
            classification,
        );
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

        let current_generation = match self
            .database
            .load_current_audit_log_configuration_generation()
        {
            Ok(Some(generation)) => generation,
            Ok(None) | Err(_) => {
                self.reporting.report(None);
                return self.recovery_required();
            }
        };
        if OperationalAuditGenerationDestination::validate(
            &self.log_catalog,
            current_generation.key(),
            Some(&current_generation),
        )
        .is_err()
        {
            self.reporting.report(None);
            return self.recovery_required();
        }

        self.drain_generation_backed(&current_generation)
    }

    #[cfg(test)]
    fn drain_resolved(
        &self,
        destination: &OperationalAuditGenerationDestination,
    ) -> OperationalAuditRecoveryState {
        self.replace_state(OperationalAuditRecoveryState {
            active: self.drain_sequence(RecoverySequence::Active, destination),
            late_delivery: self.drain_sequence(RecoverySequence::LateDelivery, destination),
        })
    }

    fn drain_generation_backed(
        &self,
        current_generation: &LogConfigurationGeneration,
    ) -> OperationalAuditRecoveryState {
        self.replace_state(OperationalAuditRecoveryState {
            active: self.drain_generation_sequence(RecoverySequence::Active, current_generation),
            late_delivery: self
                .drain_generation_sequence(RecoverySequence::LateDelivery, current_generation),
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

    #[cfg(test)]
    fn drain_sequence(
        &self,
        sequence: RecoverySequence,
        destination: &OperationalAuditGenerationDestination,
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
            let persistence = self.database.audit_terminal_recovery_persistence();
            let recovered = match self
                .producer
                .restore_terminal_recovery(persistence, &obligation)
            {
                Ok(recovered) => recovered,
                Err(_) => {
                    self.reporting.report(Some(destination.module()));
                    return AuditRecoverySequenceState::RecoveryRequired;
                }
            };
            let acknowledgement = match recovered.deliver(persistence, &destination.resolved()) {
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

    fn drain_generation_sequence(
        &self,
        sequence: RecoverySequence,
        current_generation: &LogConfigurationGeneration,
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
                self.reporting.report(None);
                return AuditRecoverySequenceState::RecoveryRequired;
            }
        };
        let batch_may_have_more = obligations.len() == batch_size.get();

        for obligation in obligations {
            let destination = match self
                .resolve_obligation_destination(current_generation, obligation.binding())
            {
                Ok(destination) => destination,
                Err(_) => {
                    self.reporting.report(None);
                    return AuditRecoverySequenceState::RecoveryRequired;
                }
            };
            let persistence = self.database.audit_terminal_recovery_persistence();
            let recovered = match self
                .producer
                .restore_terminal_recovery(persistence, &obligation)
            {
                Ok(recovered) => recovered,
                Err(_) => {
                    self.reporting.report(Some(destination.module()));
                    return AuditRecoverySequenceState::RecoveryRequired;
                }
            };
            let acknowledgement = match recovered.deliver(persistence, &destination.resolved()) {
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

    fn resolve_obligation_destination(
        &self,
        current_generation: &LogConfigurationGeneration,
        binding: &weavelit_server_database::StoredAuditDestinationBinding,
    ) -> Result<OperationalAuditGenerationDestination, OperationalAuditGenerationResolutionError>
    {
        let selected_key = self
            .database
            .log_configuration_generation_key(*binding.identifier(), binding.version())
            .map_err(|_| OperationalAuditGenerationResolutionError)?;
        if current_generation.key() == selected_key {
            return OperationalAuditGenerationDestination::resolve(
                &self.log_catalog,
                &self.state_root,
                self.deployment_identifier,
                selected_key,
                Some(current_generation),
                ServerLogAuthority::new(),
            );
        }
        let generation = self
            .database
            .load_log_configuration_generation(selected_key)
            .map_err(|_| OperationalAuditGenerationResolutionError)?;
        OperationalAuditGenerationDestination::resolve(
            &self.log_catalog,
            &self.state_root,
            self.deployment_identifier,
            selected_key,
            generation.as_ref(),
            ServerLogAuthority::new(),
        )
    }
}

impl fmt::Debug for OperationalAuditRecovery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OperationalAuditRecovery(REDACTED)")
    }
}

/// Resolver for one authority-selected immutable Audit configuration generation.
pub(crate) struct OperationalAuditGenerationDestination {
    authority: ServerLogAuthority,
    binding: AuditDestinationBinding,
    module: LogModuleIdentifier,
    destination: ConfiguredLogDestination,
}

impl OperationalAuditGenerationDestination {
    fn validate(
        log_catalog: &LogModuleCatalog,
        selected_key: LogConfigurationGenerationKey,
        generation: Option<&LogConfigurationGeneration>,
    ) -> Result<(), OperationalAuditGenerationResolutionError> {
        Self::validated_configuration(log_catalog, selected_key, generation).map(|_| ())
    }

    fn resolve(
        log_catalog: &LogModuleCatalog,
        state_root: &Path,
        deployment_identifier: DeploymentIdentifier,
        selected_key: LogConfigurationGenerationKey,
        generation: Option<&LogConfigurationGeneration>,
        authority: ServerLogAuthority,
    ) -> Result<Self, OperationalAuditGenerationResolutionError> {
        let (module, settings) =
            Self::validated_configuration(log_catalog, selected_key, generation)?;
        let binding = AuditDestinationBinding::from_server_authority(
            &authority,
            *selected_key.configuration().as_bytes(),
            selected_key.version().get(),
        )
        .map_err(|_| OperationalAuditGenerationResolutionError)?;
        let context = TrustedLogModuleContext::from_server_authority(
            &authority,
            state_root.to_path_buf(),
            *deployment_identifier.as_bytes(),
        )
        .with_settings(settings);
        let destination = log_catalog
            .create_destination(&module, &context)
            .map_err(|_| OperationalAuditGenerationResolutionError)?;

        Ok(Self {
            authority,
            binding,
            module,
            destination,
        })
    }

    fn validated_configuration(
        log_catalog: &LogModuleCatalog,
        selected_key: LogConfigurationGenerationKey,
        generation: Option<&LogConfigurationGeneration>,
    ) -> Result<(LogModuleIdentifier, DestinationSettings), OperationalAuditGenerationResolutionError>
    {
        let generation = generation.ok_or(OperationalAuditGenerationResolutionError)?;
        if generation.key() != selected_key
            || !generation.enabled()
            || !generation.contains_log_type(LogType::Audit)
        {
            return Err(OperationalAuditGenerationResolutionError);
        }

        let module = LogModuleIdentifier::new(generation.module().as_str())
            .map_err(|_| OperationalAuditGenerationResolutionError)?;
        let settings = DestinationSettings::new(
            generation
                .settings()
                .iter()
                .map(|setting| {
                    (
                        setting.key.as_str().to_owned(),
                        setting.value.as_str().to_owned(),
                    )
                })
                .collect(),
        )
        .map_err(|_| OperationalAuditGenerationResolutionError)?;
        let declaration = log_catalog
            .declaration(&module)
            .ok_or(OperationalAuditGenerationResolutionError)?;
        if !declaration.capabilities().supports(LogRecordType::Audit)
            || !declaration.accepted_settings().accepts(&settings)
        {
            return Err(OperationalAuditGenerationResolutionError);
        }
        Ok((module, settings))
    }

    pub(crate) fn module(&self) -> &LogModuleIdentifier {
        &self.module
    }

    pub(crate) const fn binding(&self) -> &AuditDestinationBinding {
        &self.binding
    }

    pub(crate) const fn destination(&self) -> &ConfiguredLogDestination {
        &self.destination
    }

    fn resolved(&self) -> ResolvedAuditDestination<'_> {
        ResolvedAuditDestination::from_server_authority(
            &self.authority,
            &self.binding,
            &self.destination,
        )
    }
}

impl fmt::Debug for OperationalAuditGenerationDestination {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OperationalAuditGenerationDestination(REDACTED)")
    }
}

struct OperationalAuditGenerationResolutionError;

impl fmt::Debug for OperationalAuditGenerationResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OperationalAuditGenerationResolutionError(REDACTED)")
    }
}

impl fmt::Display for OperationalAuditGenerationResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Audit destination generation is unavailable")
    }
}

impl std::error::Error for OperationalAuditGenerationResolutionError {}

fn destination_settings(
    generation: &LogConfigurationGeneration,
) -> Result<DestinationSettings, DatabaseError> {
    DestinationSettings::new(
        generation
            .settings()
            .iter()
            .map(|setting| {
                (
                    setting.key.as_str().to_owned(),
                    setting.value.as_str().to_owned(),
                )
            })
            .collect(),
    )
    .map_err(|_| DatabaseError::IntegrityFailure)
}

const fn log_record_type(log_type: LogType) -> LogRecordType {
    match log_type {
        LogType::System => LogRecordType::System,
        LogType::Audit => LogRecordType::Audit,
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
        Ok(Self { destination })
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
        collections::{BTreeMap, VecDeque},
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
        AuditReferencePersistence, AuditTerminalAcknowledgementProof, AuditTerminalObligation,
        AuditTerminalRecoveryPersistence, AuditTerminalRecoveryStore, AuditTerminalReplayBatchSize,
        CheckpointMetadata, CompletionObligation, ComponentEnablement, ConfigurationKey,
        ConfigurationValue, CorrelationIdentifier, DatabaseError, DatabaseInspection,
        DeploymentIdentifier, GroupAuditReference, HumanAuthorizationSnapshot, InitializedState,
        LogAssignment, LogClassification, LogConfigurationGeneration,
        LogConfigurationGenerationKey, LogConfigurationGenerationPersistence,
        LogConfigurationGenerationStore, LogConfigurationVersion, LogDetail,
        LogModuleConfiguration, LogModuleSetting, LogType, MAX_AUDIT_TERMINAL_REPLAY_BATCH_SIZE,
        MfaStore, Name, ReconciliationDigest, ReconciliationStore, RecoveryPublicKey, SessionStore,
        StateIdentifier, StoredAuditDestinationBinding, WorkflowCheckpoint, WorkflowKind,
    };
    use weavelit_server_database_authority::ServerDatabaseAuthority;
    use weavelit_server_database_sqlite::SqliteDatabase;
    use weavelit_server_lifecycle::ApplicationDatabase;
    use weavelit_server_log::{
        AuditDestinationBinding, CompleteLogRecord, ConfiguredLogDestination, CorrelationId,
        DurableAcknowledgement, EventTime, LogCapabilities, LogDestination, LogDestinationError,
        LogDestinationFactory, LogModuleCatalog, LogModuleFactoryContext, LogModuleIdentifier,
        LogModuleRegistration, LogRecordPersistenceView, LogRecordType, LogSettingsContract,
        TrustedLogModuleContext, TrustedRecordIssuer,
    };
    use weavelit_server_log_authority::ServerLogAuthority;

    use super::{
        AuditRecoverySequenceState, OperationalAuditGenerationDestination,
        OperationalAuditRecovery, OperationalAuditRecoveryState, OperationalAuditReporting,
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
        current_generation: Option<LogConfigurationGeneration>,
        generations: BTreeMap<LogConfigurationGenerationKey, LogConfigurationGeneration>,
        current_generation_reads: usize,
        exact_generation_reads: Vec<LogConfigurationGenerationKey>,
        fail_exact_generation_read: bool,
        active_list_calls: usize,
        late_list_calls: usize,
        acknowledgement_calls: usize,
        fail_acknowledgement: bool,
        fail_active_list: bool,
        fail_late_list: bool,
    }

    struct RecoveryDatabase {
        state: Arc<Mutex<StoreState>>,
        serves_recovery: bool,
        serves_generations: bool,
    }

    impl AuditTerminalRecoveryStore for RecoveryDatabase {
        fn list_pending_audit_terminal_obligations(
            &mut self,
            _persistence: &AuditTerminalRecoveryPersistence,
            batch_size: AuditTerminalReplayBatchSize,
        ) -> Result<Vec<AuditTerminalObligation>, DatabaseError> {
            let mut state = self.state.lock().map_err(|_| DatabaseError::Unavailable)?;
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
            let mut state = self.state.lock().map_err(|_| DatabaseError::Unavailable)?;
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
            acknowledgement: AuditTerminalAcknowledgementProof,
        ) -> Result<(), DatabaseError> {
            let mut state = self.state.lock().map_err(|_| DatabaseError::Unavailable)?;
            state.acknowledgement_calls += 1;
            if state.fail_acknowledgement {
                return Err(DatabaseError::Unavailable);
            }
            if state.active.front().is_some_and(|obligation| {
                obligation.identifier() == acknowledgement.identifier()
                    && obligation.binding() == acknowledgement.binding()
            }) {
                state.active.pop_front();
                return Ok(());
            }
            if state.late_delivery.front().is_some_and(|obligation| {
                obligation.identifier() == acknowledgement.identifier()
                    && obligation.binding() == acknowledgement.binding()
            }) {
                state.late_delivery.pop_front();
                return Ok(());
            }
            Err(DatabaseError::InvalidState)
        }
    }

    impl LogConfigurationGenerationStore for RecoveryDatabase {
        fn load_current_audit_log_configuration_generation(
            &mut self,
            _persistence: &LogConfigurationGenerationPersistence,
        ) -> Result<Option<LogConfigurationGeneration>, DatabaseError> {
            let mut state = self.state.lock().map_err(|_| DatabaseError::Unavailable)?;
            state.current_generation_reads += 1;
            Ok(state.current_generation.clone())
        }

        fn load_log_configuration_generation(
            &mut self,
            _persistence: &LogConfigurationGenerationPersistence,
            key: LogConfigurationGenerationKey,
        ) -> Result<Option<LogConfigurationGeneration>, DatabaseError> {
            let mut state = self.state.lock().map_err(|_| DatabaseError::Unavailable)?;
            state.exact_generation_reads.push(key);
            if state.fail_exact_generation_read {
                return Err(DatabaseError::IntegrityFailure);
            }
            Ok(state.generations.get(&key).cloned())
        }
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
            let mut state = self.state.lock().map_err(|_| DatabaseError::Unavailable)?;
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

        fn load_log_configuration_audit_reference(
            &mut self,
            _persistence: &AuditReferencePersistence,
            _configuration: StateIdentifier,
        ) -> Result<Option<weavelit_server_database::LogConfigurationAuditReference>, DatabaseError>
        {
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
            self.serves_recovery
                .then_some(self as &mut dyn AuditTerminalRecoveryStore)
        }

        fn log_configuration_generations(
            &mut self,
        ) -> Option<&mut dyn LogConfigurationGenerationStore> {
            self.serves_generations
                .then_some(self as &mut dyn LogConfigurationGenerationStore)
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
        arrivals: Option<mpsc::Sender<usize>>,
        first_delivery_release: Option<Arc<(Mutex<bool>, Condvar)>>,
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
                arrivals: self.arrivals.clone(),
                first_delivery_release: self.first_delivery_release.clone(),
            }))
        }
    }

    struct RecordingDestination {
        delivered: Arc<Mutex<Vec<[u8; 16]>>>,
        attempts: Arc<AtomicUsize>,
        fail_on_attempt: Option<usize>,
        reenter: Arc<Mutex<Option<OperationalDatabase>>>,
        arrivals: Option<mpsc::Sender<usize>>,
        first_delivery_release: Option<Arc<(Mutex<bool>, Condvar)>>,
    }

    impl LogDestination for RecordingDestination {
        fn deliver(
            &self,
            record: &CompleteLogRecord,
            acknowledgement: DurableAcknowledgement,
        ) -> Result<DurableAcknowledgement, LogDestinationError> {
            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst) + 1;
            if let Some(arrivals) = &self.arrivals {
                arrivals
                    .send(attempt)
                    .map_err(|_| LogDestinationError::Unavailable)?;
            }
            if attempt == 1
                && let Some(release) = &self.first_delivery_release
            {
                let (released, wake) = &**release;
                let mut released = released.lock().unwrap_or_else(PoisonError::into_inner);
                while !*released {
                    released = wake.wait(released).unwrap_or_else(PoisonError::into_inner);
                }
            }
            if self.fail_on_attempt == Some(attempt) {
                return Err(LogDestinationError::Unavailable);
            }
            if let Some(database) = self
                .reenter
                .lock()
                .map_err(|_| LogDestinationError::Unavailable)?
                .clone()
            {
                let (completed, completion) = mpsc::channel();
                thread::spawn(move || {
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
                .map_err(|_| LogDestinationError::Unavailable)?
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
                .map_err(|_| LogDestinationError::Unavailable)?
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

    struct GenerationResolverFactory {
        factory_calls: Arc<AtomicUsize>,
        delivery_calls: Arc<AtomicUsize>,
        received: Arc<Mutex<Option<String>>>,
    }

    impl LogDestinationFactory for GenerationResolverFactory {
        fn accepted_settings(&self) -> LogSettingsContract {
            LogSettingsContract::new(vec!["endpoint".to_owned()]).unwrap()
        }

        fn create(
            &self,
            context: &LogModuleFactoryContext<'_>,
        ) -> Result<Box<dyn LogDestination>, LogDestinationError> {
            self.factory_calls.fetch_add(1, Ordering::SeqCst);
            *self
                .received
                .lock()
                .map_err(|_| LogDestinationError::Unavailable)? =
                context.settings().get("endpoint").map(str::to_owned);
            Ok(Box::new(GenerationResolverDestination {
                delivery_calls: Arc::clone(&self.delivery_calls),
            }))
        }
    }

    struct GenerationResolverDestination {
        delivery_calls: Arc<AtomicUsize>,
    }

    impl LogDestination for GenerationResolverDestination {
        fn deliver(
            &self,
            _record: &CompleteLogRecord,
            acknowledgement: DurableAcknowledgement,
        ) -> Result<DurableAcknowledgement, LogDestinationError> {
            self.delivery_calls.fetch_add(1, Ordering::SeqCst);
            Ok(acknowledgement)
        }

        fn preflight(&self, _record_type: LogRecordType) -> Result<(), LogDestinationError> {
            Ok(())
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
            LogCapabilities::new(vec![LogRecordType::Audit]).unwrap(),
            Box::new(AcknowledgingFactory),
        )])
        .unwrap();
        catalog
            .create_destination(
                &LogModuleIdentifier::new("acknowledging").unwrap(),
                &TrustedLogModuleContext::from_server_authority(
                    &authority,
                    PathBuf::from("/unused"),
                    [0x71; 16],
                ),
            )
            .unwrap()
    }

    fn obligation(binding: &AuditDestinationBinding, value: u8) -> AuditTerminalObligation {
        let authority = ServerLogAuthority::new();
        let persistence = persistence();
        let producer = producer(&authority);
        let account = AccountAuditReference::new(
            identifier(value),
            AuditReferenceIdentifier::generate().unwrap(),
        );
        let attempt = producer
            .prepare_attempt(
                EventTime::from_unix_milliseconds(u64::from(value) * 2),
                CorrelationId::new(format!("recovery-{value}")).unwrap(),
                AuditActor::Human(account),
                AuditEvent::AuthenticationUserDisabled { account },
            )
            .unwrap()
            .deliver(&acknowledging_destination())
            .unwrap();
        let write = producer
            .prepare_completion(
                &attempt,
                EventTime::from_unix_milliseconds(u64::from(value) * 2 + 1),
                AuditOutcomeDetail::AuthenticationUserDisabled(StateChangeOutcome::Succeeded(
                    AccountStatus::Disabled,
                )),
            )
            .unwrap()
            .recovery_obligation(&persistence, binding)
            .unwrap();
        AuditTerminalObligation::from_persisted(
            &persistence,
            *write.identifier().as_bytes(),
            write.projection_bytes().to_vec(),
            write.binding().clone(),
        )
        .unwrap()
    }

    struct DestinationFixture {
        binding: AuditDestinationBinding,
        destination: OperationalAuditGenerationDestination,
        delivered: Arc<Mutex<Vec<[u8; 16]>>>,
        attempts: Arc<AtomicUsize>,
        reenter: Arc<Mutex<Option<OperationalDatabase>>>,
    }

    fn operational_destination(
        binding_identifier: [u8; 16],
        fail_on_attempt: Option<usize>,
        arrivals: Option<mpsc::Sender<usize>>,
        first_delivery_release: Option<Arc<(Mutex<bool>, Condvar)>>,
    ) -> DestinationFixture {
        let authority = ServerLogAuthority::new();
        let binding =
            AuditDestinationBinding::from_server_authority(&authority, binding_identifier, 1)
                .unwrap();
        let delivered = Arc::new(Mutex::new(Vec::new()));
        let attempts = Arc::new(AtomicUsize::new(0));
        let reenter = Arc::new(Mutex::new(None));
        let catalog = LogModuleCatalog::new(vec![LogModuleRegistration::new(
            "recording",
            LogCapabilities::new(vec![LogRecordType::Audit]).unwrap(),
            Box::new(RecordingFactory {
                delivered: Arc::clone(&delivered),
                attempts: Arc::clone(&attempts),
                fail_on_attempt,
                reenter: Arc::clone(&reenter),
                arrivals,
                first_delivery_release,
            }),
        )])
        .unwrap();
        let destination = catalog
            .create_destination(
                &LogModuleIdentifier::new("recording").unwrap(),
                &TrustedLogModuleContext::from_server_authority(
                    &authority,
                    PathBuf::from("/unused"),
                    [0x72; 16],
                ),
            )
            .unwrap();
        DestinationFixture {
            binding: binding.clone(),
            destination: OperationalAuditGenerationDestination {
                authority,
                binding,
                module: LogModuleIdentifier::new("recording").unwrap(),
                destination,
            },
            delivered,
            attempts,
            reenter,
        }
    }

    fn database(state: Arc<Mutex<StoreState>>) -> OperationalDatabase {
        OperationalDatabase::from_open(Box::new(RecoveryDatabase {
            state,
            serves_recovery: true,
            serves_generations: true,
        }))
    }

    fn recovery(
        database: OperationalDatabase,
        destination: OperationalAuditGenerationDestination,
    ) -> OperationalAuditRecovery {
        recovery_with_reporting(database, destination, reporting_without_system())
    }

    fn recovery_with_reporting(
        database: OperationalDatabase,
        destination: OperationalAuditGenerationDestination,
        reporting: OperationalAuditReporting,
    ) -> OperationalAuditRecovery {
        let authority = ServerLogAuthority::new();
        OperationalAuditRecovery {
            database,
            producer: producer(&authority),
            log_catalog: unused_catalog(),
            state_root: PathBuf::from("/unused"),
            deployment_identifier: DeploymentIdentifier::from_bytes([0x42; 16]).unwrap(),
            reporting,
            destination_override: Some(destination),
            drain_permit: Mutex::new(()),
            state: Mutex::new(OperationalAuditRecoveryState {
                active: AuditRecoverySequenceState::RecoveryRequired,
                late_delivery: AuditRecoverySequenceState::RecoveryRequired,
            }),
        }
    }

    fn generation_backed_recovery(
        database: OperationalDatabase,
        log_catalog: Arc<LogModuleCatalog>,
    ) -> OperationalAuditRecovery {
        OperationalAuditRecovery {
            database,
            producer: producer(&ServerLogAuthority::new()),
            log_catalog,
            state_root: PathBuf::from("/unused"),
            deployment_identifier: DeploymentIdentifier::from_bytes([0x42; 16]).unwrap(),
            reporting: reporting_without_system(),
            destination_override: None,
            drain_permit: Mutex::new(()),
            state: Mutex::new(OperationalAuditRecoveryState {
                active: AuditRecoverySequenceState::RecoveryRequired,
                late_delivery: AuditRecoverySequenceState::RecoveryRequired,
            }),
        }
    }

    fn reporting_without_system() -> OperationalAuditReporting {
        let authority = ServerLogAuthority::new();
        OperationalAuditReporting {
            support: OperationalLogSupport::new(
                TrustedRecordIssuer::from_server_authority(&authority),
                None,
            ),
            fallback_module: LogModuleIdentifier::new("recording").unwrap(),
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
            LogCapabilities::new(vec![LogRecordType::System]).unwrap(),
            Box::new(SystemRecordingFactory {
                records: Arc::clone(&records),
                attempts: Arc::clone(&attempts),
            }),
        )])
        .unwrap();
        let destination = catalog
            .create_destination(
                &LogModuleIdentifier::new("system-recording").unwrap(),
                &TrustedLogModuleContext::from_server_authority(
                    &authority,
                    PathBuf::from("/unused"),
                    [0x42; 16],
                ),
            )
            .unwrap();
        (
            OperationalAuditReporting {
                support: OperationalLogSupport::new(
                    TrustedRecordIssuer::from_server_authority(&authority),
                    Some(Arc::new(destination)),
                ),
                fallback_module: LogModuleIdentifier::new("recording").unwrap(),
            },
            records,
            attempts,
        )
    }

    fn unused_catalog() -> Arc<LogModuleCatalog> {
        Arc::new(
            LogModuleCatalog::new(vec![LogModuleRegistration::new(
                "acknowledging",
                LogCapabilities::new(vec![LogRecordType::Audit]).unwrap(),
                Box::new(AcknowledgingFactory),
            )])
            .unwrap(),
        )
    }

    #[test]
    fn activation_drains_active_then_late_oldest_first_without_holding_database_lane() {
        let DestinationFixture {
            binding,
            destination,
            delivered,
            reenter,
            ..
        } = operational_destination([0x31; 16], None, None, None);
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
        *reenter.lock().unwrap() = Some(database.clone());

        let state = recovery(database, destination).drain_for_activation();

        assert_eq!(state.active(), AuditRecoverySequenceState::Ready);
        assert_eq!(state.late_delivery(), AuditRecoverySequenceState::Ready);
        assert_eq!(*delivered.lock().unwrap(), expected);
        let store = store.lock().unwrap();
        assert!(store.active.is_empty());
        assert!(store.late_delivery.is_empty());
    }

    #[test]
    fn concurrent_drains_deliver_and_acknowledge_one_obligation_once() {
        let (arrivals, arrived) = mpsc::channel();
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let DestinationFixture {
            binding,
            destination,
            attempts,
            ..
        } = operational_destination([0x32; 16], None, Some(arrivals), Some(Arc::clone(&release)));
        let store = Arc::new(Mutex::new(StoreState {
            active: VecDeque::from([obligation(&binding, 10)]),
            ..StoreState::default()
        }));
        let recovery = Arc::new(recovery(database(Arc::clone(&store)), destination));

        let first_recovery = Arc::clone(&recovery);
        let first = thread::spawn(move || first_recovery.drain_for_activation());
        assert_eq!(arrived.recv_timeout(Duration::from_secs(1)), Ok(1));
        let second_recovery = Arc::clone(&recovery);
        let second = thread::spawn(move || second_recovery.drain_before_consequential_operation());
        assert!(arrived.recv_timeout(Duration::from_millis(100)).is_err());
        let (released, wake) = &*release;
        *released.lock().unwrap() = true;
        wake.notify_all();

        assert_eq!(
            first.join().unwrap().active(),
            AuditRecoverySequenceState::Ready
        );
        assert_eq!(
            second.join().unwrap().active(),
            AuditRecoverySequenceState::Ready
        );
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        let store = store.lock().unwrap();
        assert_eq!(store.acknowledgement_calls, 1);
        assert!(store.active.is_empty());
    }

    #[test]
    fn bounded_batches_continue_on_the_next_explicit_drain() {
        let DestinationFixture {
            binding,
            destination,
            attempts,
            ..
        } = operational_destination([0x33; 16], None, None, None);
        let active = (0..MAX_AUDIT_TERMINAL_REPLAY_BATCH_SIZE)
            .map(|index| obligation(&binding, u8::try_from(index + 20).unwrap()))
            .collect();
        let store = Arc::new(Mutex::new(StoreState {
            active,
            ..StoreState::default()
        }));
        let recovery = recovery(database(Arc::clone(&store)), destination);

        assert_eq!(
            recovery.drain_for_activation().active(),
            AuditRecoverySequenceState::Pending
        );
        assert_eq!(
            recovery.drain_before_consequential_operation().active(),
            AuditRecoverySequenceState::Ready
        );
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            MAX_AUDIT_TERMINAL_REPLAY_BATCH_SIZE
        );
    }

    #[test]
    fn each_drain_re_resolves_the_committed_assignment_after_repair() {
        let configuration_identifier = identifier(0x34);
        let persistence = generation_persistence();
        let key = persistence.key(configuration_identifier, LogConfigurationVersion::INITIAL);
        let generation = persistence
            .generation(
                key,
                Name::new("recording").unwrap(),
                Name::new("audit-primary").unwrap(),
                true,
                Vec::new(),
                vec![LogType::Audit],
            )
            .unwrap();
        let authority = ServerLogAuthority::new();
        let binding = AuditDestinationBinding::from_server_authority(
            &authority,
            *configuration_identifier.as_bytes(),
            1,
        )
        .unwrap();
        let store = Arc::new(Mutex::new(StoreState {
            active: VecDeque::from([obligation(&binding, 90)]),
            ..StoreState::default()
        }));
        let delivered = Arc::new(Mutex::new(Vec::new()));
        let attempts = Arc::new(AtomicUsize::new(0));
        let catalog = Arc::new(
            LogModuleCatalog::new(vec![LogModuleRegistration::new(
                "recording",
                LogCapabilities::new(vec![LogRecordType::Audit]).unwrap(),
                Box::new(RecordingFactory {
                    delivered: Arc::clone(&delivered),
                    attempts: Arc::clone(&attempts),
                    fail_on_attempt: None,
                    reenter: Arc::new(Mutex::new(None)),
                    arrivals: None,
                    first_delivery_release: None,
                }),
            )])
            .unwrap(),
        );
        let recovery = OperationalAuditRecovery {
            database: database(Arc::clone(&store)),
            producer: producer(&ServerLogAuthority::new()),
            log_catalog: catalog,
            state_root: PathBuf::from("/unused"),
            deployment_identifier: DeploymentIdentifier::from_bytes([0x42; 16]).unwrap(),
            reporting: reporting_without_system(),
            destination_override: None,
            drain_permit: Mutex::new(()),
            state: Mutex::new(OperationalAuditRecoveryState {
                active: AuditRecoverySequenceState::Ready,
                late_delivery: AuditRecoverySequenceState::Ready,
            }),
        };

        assert_eq!(
            recovery.drain_for_activation().active(),
            AuditRecoverySequenceState::RecoveryRequired
        );
        store.lock().unwrap().current_generation = Some(generation);
        assert_eq!(
            recovery.drain_before_consequential_operation().active(),
            AuditRecoverySequenceState::Ready
        );
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        let store = store.lock().unwrap();
        assert_eq!(store.current_generation_reads, 2);
        assert_eq!(store.initialized_state_loads, 0);
    }

    #[test]
    fn recovery_uses_current_and_retained_historical_generations_exactly() {
        let persistence = generation_persistence();
        let configuration = identifier(0x35);
        let historical_key = persistence.key(configuration, LogConfigurationVersion::INITIAL);
        let current_key = persistence.key(configuration, LogConfigurationVersion::new(2).unwrap());
        let historical = persistence
            .generation(
                historical_key,
                Name::new("historical-recording").unwrap(),
                Name::new("audit-primary").unwrap(),
                true,
                Vec::new(),
                vec![LogType::Audit],
            )
            .unwrap();
        let current = persistence
            .generation(
                current_key,
                Name::new("current-recording").unwrap(),
                Name::new("audit-primary").unwrap(),
                true,
                Vec::new(),
                vec![LogType::Audit],
            )
            .unwrap();
        let current_binding = AuditDestinationBinding::from_server_authority(
            &ServerLogAuthority::new(),
            *configuration.as_bytes(),
            current_key.version().get(),
        )
        .unwrap();
        let historical_binding = AuditDestinationBinding::from_server_authority(
            &ServerLogAuthority::new(),
            *configuration.as_bytes(),
            historical_key.version().get(),
        )
        .unwrap();
        let current_delivered = Arc::new(Mutex::new(Vec::new()));
        let current_attempts = Arc::new(AtomicUsize::new(0));
        let historical_delivered = Arc::new(Mutex::new(Vec::new()));
        let historical_attempts = Arc::new(AtomicUsize::new(0));
        let catalog = Arc::new(
            LogModuleCatalog::new(vec![
                LogModuleRegistration::new(
                    "current-recording",
                    LogCapabilities::new(vec![LogRecordType::Audit]).unwrap(),
                    Box::new(RecordingFactory {
                        delivered: Arc::clone(&current_delivered),
                        attempts: Arc::clone(&current_attempts),
                        fail_on_attempt: None,
                        reenter: Arc::new(Mutex::new(None)),
                        arrivals: None,
                        first_delivery_release: None,
                    }),
                ),
                LogModuleRegistration::new(
                    "historical-recording",
                    LogCapabilities::new(vec![LogRecordType::Audit]).unwrap(),
                    Box::new(RecordingFactory {
                        delivered: Arc::clone(&historical_delivered),
                        attempts: Arc::clone(&historical_attempts),
                        fail_on_attempt: None,
                        reenter: Arc::new(Mutex::new(None)),
                        arrivals: None,
                        first_delivery_release: None,
                    }),
                ),
            ])
            .unwrap(),
        );
        let active = obligation(&current_binding, 91);
        let late = obligation(&historical_binding, 92);
        let store = Arc::new(Mutex::new(StoreState {
            active: VecDeque::from([active.clone()]),
            late_delivery: VecDeque::from([late.clone()]),
            current_generation: Some(current),
            generations: BTreeMap::from([(historical_key, historical)]),
            ..StoreState::default()
        }));
        let recovery = generation_backed_recovery(database(Arc::clone(&store)), catalog);

        let state = recovery.drain_for_activation();

        assert_eq!(state.active(), AuditRecoverySequenceState::Ready);
        assert_eq!(state.late_delivery(), AuditRecoverySequenceState::Ready);
        assert_eq!(current_attempts.load(Ordering::SeqCst), 1);
        assert_eq!(historical_attempts.load(Ordering::SeqCst), 1);
        assert_eq!(
            *current_delivered.lock().unwrap(),
            vec![*active.identifier().as_bytes()]
        );
        assert_eq!(
            *historical_delivered.lock().unwrap(),
            vec![*late.identifier().as_bytes()]
        );
        let store = store.lock().unwrap();
        assert_eq!(store.current_generation_reads, 1);
        assert_eq!(store.exact_generation_reads, vec![historical_key]);
        assert_eq!(store.initialized_state_loads, 0);
    }

    #[test]
    fn healthy_version_one_generation_activates_without_mutable_state_or_factory_access() {
        let factory_calls = Arc::new(AtomicUsize::new(0));
        let delivery_calls = Arc::new(AtomicUsize::new(0));
        let received = Arc::new(Mutex::new(None));
        let catalog = Arc::new(generation_catalog(
            vec![LogRecordType::Audit],
            Arc::clone(&factory_calls),
            Arc::clone(&delivery_calls),
            Arc::clone(&received),
        ));
        let persistence = generation_persistence();
        let key = persistence.key(identifier(0x36), LogConfigurationVersion::INITIAL);
        let store = Arc::new(Mutex::new(StoreState {
            current_generation: Some(generation(
                &persistence,
                key,
                true,
                vec![LogType::Audit],
                "audit-primary",
            )),
            ..StoreState::default()
        }));

        let state = generation_backed_recovery(database(Arc::clone(&store)), catalog)
            .drain_for_activation();

        assert_eq!(state.active(), AuditRecoverySequenceState::Ready);
        assert_eq!(state.late_delivery(), AuditRecoverySequenceState::Ready);
        assert_eq!(factory_calls.load(Ordering::SeqCst), 0);
        assert_eq!(delivery_calls.load(Ordering::SeqCst), 0);
        assert_eq!(*received.lock().unwrap(), None);
        let store = store.lock().unwrap();
        assert_eq!(store.current_generation_reads, 1);
        assert_eq!(store.active_list_calls, 1);
        assert_eq!(store.late_list_calls, 1);
        assert_eq!(store.initialized_state_loads, 0);
    }

    #[test]
    fn missing_corrupt_or_mismatched_historical_generation_fails_before_factory_access() {
        let persistence = generation_persistence();
        let configuration = identifier(0x37);
        let historical_key = persistence.key(configuration, LogConfigurationVersion::INITIAL);
        let current_key = persistence.key(configuration, LogConfigurationVersion::new(2).unwrap());
        let mismatched_key =
            persistence.key(configuration, LogConfigurationVersion::new(3).unwrap());

        for (historical, fail_exact_generation_read) in [
            (None, false),
            (None, true),
            (
                Some(generation(
                    &persistence,
                    mismatched_key,
                    true,
                    vec![LogType::Audit],
                    "wrong-generation",
                )),
                false,
            ),
        ] {
            let factory_calls = Arc::new(AtomicUsize::new(0));
            let delivery_calls = Arc::new(AtomicUsize::new(0));
            let received = Arc::new(Mutex::new(None));
            let catalog = Arc::new(generation_catalog(
                vec![LogRecordType::Audit],
                Arc::clone(&factory_calls),
                Arc::clone(&delivery_calls),
                Arc::clone(&received),
            ));
            let retained_binding = AuditDestinationBinding::from_server_authority(
                &ServerLogAuthority::new(),
                *configuration.as_bytes(),
                historical_key.version().get(),
            )
            .unwrap();
            let mut generations = BTreeMap::new();
            if let Some(historical) = historical {
                generations.insert(historical_key, historical);
            }
            let store = Arc::new(Mutex::new(StoreState {
                active: VecDeque::from([obligation(&retained_binding, 93)]),
                current_generation: Some(generation(
                    &persistence,
                    current_key,
                    true,
                    vec![LogType::Audit],
                    "current-generation",
                )),
                generations,
                fail_exact_generation_read,
                ..StoreState::default()
            }));

            let state = generation_backed_recovery(database(Arc::clone(&store)), catalog)
                .drain_for_activation();

            assert_eq!(state.active(), AuditRecoverySequenceState::RecoveryRequired);
            assert_eq!(state.late_delivery(), AuditRecoverySequenceState::Ready);
            assert_eq!(factory_calls.load(Ordering::SeqCst), 0);
            assert_eq!(delivery_calls.load(Ordering::SeqCst), 0);
            assert_eq!(*received.lock().unwrap(), None);
            let store = store.lock().unwrap();
            assert_eq!(store.active.len(), 1);
            assert_eq!(store.exact_generation_reads, vec![historical_key]);
            assert_eq!(store.initialized_state_loads, 0);
        }
    }

    #[test]
    fn version_one_generation_activation_survives_sqlite_restart() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let path = temporary_directory
            .path()
            .canonicalize()
            .unwrap()
            .join("application.db");
        let initialized = initialized_state_with_settings(
            identifier(0x38),
            "generation-test",
            vec![LogModuleSetting {
                key: ConfigurationKey::new("endpoint").unwrap(),
                value: ConfigurationValue::new("restart-audit").unwrap(),
            }],
        );
        let checkpoint = WorkflowCheckpoint::new(
            initialized.deployment_identifier(),
            WorkflowKind::Restore,
            CheckpointMetadata::from_bytes(b"generation-restart".as_slice()).unwrap(),
        );
        let mut sqlite = SqliteDatabase::open(&path).unwrap();
        sqlite.create_checkpoint(&checkpoint).unwrap();
        sqlite
            .complete_checkpoint(
                &checkpoint,
                initialized.state(),
                &ReconciliationDigest::from_bytes([0x38; 32]),
            )
            .unwrap();
        drop(sqlite);

        for _ in 0..2 {
            let factory_calls = Arc::new(AtomicUsize::new(0));
            let delivery_calls = Arc::new(AtomicUsize::new(0));
            let received = Arc::new(Mutex::new(None));
            let catalog = Arc::new(generation_catalog(
                vec![LogRecordType::Audit],
                Arc::clone(&factory_calls),
                Arc::clone(&delivery_calls),
                Arc::clone(&received),
            ));
            let recovery = generation_backed_recovery(
                OperationalDatabase::from_open(Box::new(SqliteDatabase::open(&path).unwrap())),
                catalog,
            );

            let state = recovery.drain_for_activation();

            assert_eq!(state.active(), AuditRecoverySequenceState::Ready);
            assert_eq!(state.late_delivery(), AuditRecoverySequenceState::Ready);
            assert_eq!(factory_calls.load(Ordering::SeqCst), 0);
            assert_eq!(delivery_calls.load(Ordering::SeqCst), 0);
            assert_eq!(*received.lock().unwrap(), None);
        }
    }

    #[test]
    fn delivery_and_ack_failures_retain_the_oldest_while_late_delivery_continues() {
        let DestinationFixture {
            binding,
            destination,
            delivered,
            attempts,
            ..
        } = operational_destination([0x35; 16], Some(2), None, None);
        let active = [obligation(&binding, 5), obligation(&binding, 6)];
        let failed = active[1].identifier();
        let late = obligation(&binding, 7);
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
        assert_eq!(delivered.lock().unwrap().len(), 2);
        assert_eq!(
            store.lock().unwrap().active.front().unwrap().identifier(),
            failed
        );

        let DestinationFixture {
            binding,
            destination,
            attempts,
            ..
        } = operational_destination([0x36; 16], None, None, None);
        let failed_ack = Arc::new(Mutex::new(StoreState {
            active: VecDeque::from([obligation(&binding, 8)]),
            fail_acknowledgement: true,
            ..StoreState::default()
        }));
        assert_eq!(
            recovery(database(Arc::clone(&failed_ack)), destination)
                .drain_for_activation()
                .active(),
            AuditRecoverySequenceState::RecoveryRequired
        );
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert_eq!(failed_ack.lock().unwrap().active.len(), 1);
    }

    #[test]
    fn binding_mismatch_and_malformed_opaque_rows_never_reach_destination() {
        let DestinationFixture {
            binding,
            destination,
            attempts,
            ..
        } = operational_destination([0x37; 16], None, None, None);
        let authority = ServerLogAuthority::new();
        let retained = AuditDestinationBinding::from_server_authority(
            &authority,
            [0x38; 16],
            binding.version(),
        )
        .unwrap();
        let mismatch = Arc::new(Mutex::new(StoreState {
            active: VecDeque::from([obligation(&retained, 9)]),
            ..StoreState::default()
        }));
        assert_eq!(
            recovery(database(Arc::clone(&mismatch)), destination)
                .drain_for_activation()
                .active(),
            AuditRecoverySequenceState::RecoveryRequired
        );
        assert_eq!(attempts.load(Ordering::SeqCst), 0);

        let DestinationFixture {
            binding,
            destination,
            attempts,
            ..
        } = operational_destination([0x39; 16], None, None, None);
        let persistence = persistence();
        let stored_binding = StoredAuditDestinationBinding::from_persisted(
            &persistence,
            *binding.identifier(),
            binding.version(),
        )
        .unwrap();
        let malformed = AuditTerminalObligation::from_persisted(
            &persistence,
            [0xA1; 16],
            b"not-a-terminal-projection".to_vec(),
            stored_binding,
        )
        .expect("opaque storage accepts bounded bytes without parsing fields");
        let malformed_store = Arc::new(Mutex::new(StoreState {
            active: VecDeque::from([malformed]),
            ..StoreState::default()
        }));
        assert_eq!(
            recovery(database(Arc::clone(&malformed_store)), destination)
                .drain_for_activation()
                .active(),
            AuditRecoverySequenceState::RecoveryRequired
        );
        assert_eq!(attempts.load(Ordering::SeqCst), 0);
        assert_eq!(malformed_store.lock().unwrap().active.len(), 1);
    }

    #[test]
    fn list_failure_isolated_to_its_sequence_and_reports_safe_system_context() {
        let DestinationFixture {
            binding,
            destination,
            delivered,
            ..
        } = operational_destination([0x40; 16], None, None, None);
        let late = obligation(&binding, 12);
        let late_identifier = *late.identifier().as_bytes();
        let store = Arc::new(Mutex::new(StoreState {
            active: VecDeque::from([obligation(&binding, 11)]),
            late_delivery: VecDeque::from([late]),
            fail_active_list: true,
            ..StoreState::default()
        }));
        let (reporting, records, report_attempts) = recording_reporting();

        let state = recovery_with_reporting(database(Arc::clone(&store)), destination, reporting)
            .drain_for_activation();
        assert_eq!(state.active(), AuditRecoverySequenceState::RecoveryRequired);
        assert_eq!(state.late_delivery(), AuditRecoverySequenceState::Ready);
        assert_eq!(*delivered.lock().unwrap(), vec![late_identifier]);
        assert_eq!(report_attempts.load(Ordering::SeqCst), 1);
        let records = records.lock().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].classification,
            "dependency.audit-log-unavailable"
        );
        assert_eq!(records[0].correlation_identifier, "audit-terminal-recovery");
        assert_eq!(
            records[0].detail,
            "audit destination module recording unavailable for internal.log-policy.changed"
        );
        let rendered = format!("{records:?}");
        for forbidden in ["recovery-11", "database is locked", "request payload"] {
            assert!(!rendered.contains(forbidden), "{rendered}");
        }
    }

    #[test]
    fn import_delivery_and_ack_failures_each_report_one_safe_system_record() {
        let DestinationFixture {
            binding,
            destination,
            attempts,
            ..
        } = operational_destination([0x43; 16], None, None, None);
        let persistence = persistence();
        let stored_binding = StoredAuditDestinationBinding::from_persisted(
            &persistence,
            *binding.identifier(),
            binding.version(),
        )
        .unwrap();
        let malformed = AuditTerminalObligation::from_persisted(
            &persistence,
            [0xA2; 16],
            b"opaque-secret-bearing-invalid-document".to_vec(),
            stored_binding,
        )
        .unwrap();
        let store = Arc::new(Mutex::new(StoreState {
            active: VecDeque::from([malformed]),
            ..StoreState::default()
        }));
        let (reporting, records, report_attempts) = recording_reporting();
        assert_eq!(
            recovery_with_reporting(database(store), destination, reporting)
                .drain_for_activation()
                .active(),
            AuditRecoverySequenceState::RecoveryRequired
        );
        assert_eq!(attempts.load(Ordering::SeqCst), 0);
        assert_safe_reports(&records, &report_attempts, 1);

        let DestinationFixture {
            binding,
            destination,
            ..
        } = operational_destination([0x44; 16], Some(1), None, None);
        let store = Arc::new(Mutex::new(StoreState {
            active: VecDeque::from([obligation(&binding, 14)]),
            ..StoreState::default()
        }));
        let (reporting, records, report_attempts) = recording_reporting();
        assert_eq!(
            recovery_with_reporting(database(store), destination, reporting)
                .drain_for_activation()
                .active(),
            AuditRecoverySequenceState::Pending
        );
        assert_safe_reports(&records, &report_attempts, 1);

        let DestinationFixture {
            binding,
            destination,
            ..
        } = operational_destination([0x45; 16], None, None, None);
        let store = Arc::new(Mutex::new(StoreState {
            active: VecDeque::from([obligation(&binding, 15)]),
            fail_acknowledgement: true,
            ..StoreState::default()
        }));
        let (reporting, records, report_attempts) = recording_reporting();
        assert_eq!(
            recovery_with_reporting(database(store), destination, reporting)
                .drain_for_activation()
                .active(),
            AuditRecoverySequenceState::RecoveryRequired
        );
        assert_safe_reports(&records, &report_attempts, 1);
    }

    fn assert_safe_reports(
        records: &Arc<Mutex<Vec<ReportedSystemRecord>>>,
        attempts: &Arc<AtomicUsize>,
        expected: usize,
    ) {
        assert_eq!(attempts.load(Ordering::SeqCst), expected);
        let records = records.lock().unwrap();
        assert_eq!(records.len(), expected);
        let rendered = format!("{records:?}");
        assert!(rendered.contains("dependency.audit-log-unavailable"));
        assert!(rendered.contains("audit-terminal-recovery"));
        for forbidden in [
            "opaque-secret-bearing-invalid-document",
            "database is locked",
            "request payload",
        ] {
            assert!(!rendered.contains(forbidden), "{rendered}");
        }
    }

    #[test]
    fn generation_resolver_pairs_exact_binding_and_committed_settings_without_delivery() {
        let factory_calls = Arc::new(AtomicUsize::new(0));
        let delivery_calls = Arc::new(AtomicUsize::new(0));
        let received = Arc::new(Mutex::new(None));
        let catalog = generation_catalog(
            vec![LogRecordType::Audit],
            Arc::clone(&factory_calls),
            Arc::clone(&delivery_calls),
            Arc::clone(&received),
        );
        let persistence = generation_persistence();
        let key = persistence.key(identifier(0x51), LogConfigurationVersion::new(7).unwrap());
        let generation = generation(
            &persistence,
            key,
            true,
            vec![LogType::Audit],
            "audit-primary",
        );

        let destination = OperationalAuditGenerationDestination::resolve(
            &catalog,
            Path::new("/unused"),
            DeploymentIdentifier::from_bytes([0x52; 16]).unwrap(),
            key,
            Some(&generation),
            ServerLogAuthority::new(),
        )
        .unwrap();

        assert_eq!(destination.module().as_str(), "generation-test");
        assert_eq!(
            destination.resolved().binding().identifier(),
            identifier(0x51).as_bytes()
        );
        assert_eq!(destination.resolved().binding().version(), 7);
        assert_eq!(factory_calls.load(Ordering::SeqCst), 1);
        assert_eq!(delivery_calls.load(Ordering::SeqCst), 0);
        assert_eq!(received.lock().unwrap().as_deref(), Some("audit-primary"));
    }

    #[test]
    fn generation_resolver_rejects_untrusted_shapes_before_factory_or_delivery() {
        let factory_calls = Arc::new(AtomicUsize::new(0));
        let delivery_calls = Arc::new(AtomicUsize::new(0));
        let received = Arc::new(Mutex::new(None));
        let catalog = generation_catalog(
            vec![LogRecordType::Audit],
            Arc::clone(&factory_calls),
            Arc::clone(&delivery_calls),
            Arc::clone(&received),
        );
        let persistence = generation_persistence();
        let selected = persistence.key(identifier(0x61), LogConfigurationVersion::new(3).unwrap());
        let cases = [
            None,
            Some(generation(
                &persistence,
                selected,
                false,
                vec![LogType::Audit],
                "disabled",
            )),
            Some(generation(
                &persistence,
                persistence.key(identifier(0x62), LogConfigurationVersion::new(3).unwrap()),
                true,
                vec![LogType::Audit],
                "wrong-identity",
            )),
            Some(generation(
                &persistence,
                persistence.key(identifier(0x61), LogConfigurationVersion::new(4).unwrap()),
                true,
                vec![LogType::Audit],
                "wrong-version",
            )),
            Some(generation(
                &persistence,
                selected,
                true,
                vec![LogType::System],
                "non-audit",
            )),
            Some(
                persistence
                    .generation(
                        selected,
                        Name::new("unknown-module").unwrap(),
                        Name::new("audit-generation").unwrap(),
                        true,
                        vec![LogModuleSetting {
                            key: ConfigurationKey::new("endpoint").unwrap(),
                            value: ConfigurationValue::new("unknown-module").unwrap(),
                        }],
                        vec![LogType::Audit],
                    )
                    .unwrap(),
            ),
            Some(
                persistence
                    .generation(
                        selected,
                        Name::new("generation-test").unwrap(),
                        Name::new("audit-generation").unwrap(),
                        true,
                        vec![LogModuleSetting {
                            key: ConfigurationKey::new("undeclared").unwrap(),
                            value: ConfigurationValue::new("sensitive-setting").unwrap(),
                        }],
                        vec![LogType::Audit],
                    )
                    .unwrap(),
            ),
        ];

        for generation in &cases {
            let error = OperationalAuditGenerationDestination::resolve(
                &catalog,
                Path::new("/sensitive/path"),
                DeploymentIdentifier::from_bytes([0x63; 16]).unwrap(),
                selected,
                generation.as_ref(),
                ServerLogAuthority::new(),
            )
            .unwrap_err();
            assert_eq!(
                format!("{error:?} {error}"),
                "OperationalAuditGenerationResolutionError(REDACTED) Audit destination generation is unavailable"
            );
        }
        assert_eq!(factory_calls.load(Ordering::SeqCst), 0);
        assert_eq!(delivery_calls.load(Ordering::SeqCst), 0);
        assert_eq!(*received.lock().unwrap(), None);
    }

    #[test]
    fn generation_resolver_rejects_non_audit_module_before_factory_or_delivery() {
        let factory_calls = Arc::new(AtomicUsize::new(0));
        let delivery_calls = Arc::new(AtomicUsize::new(0));
        let received = Arc::new(Mutex::new(None));
        let catalog = generation_catalog(
            vec![LogRecordType::System],
            Arc::clone(&factory_calls),
            Arc::clone(&delivery_calls),
            Arc::clone(&received),
        );
        let persistence = generation_persistence();
        let key = persistence.key(identifier(0x71), LogConfigurationVersion::INITIAL);
        let generation = generation(
            &persistence,
            key,
            true,
            vec![LogType::Audit],
            "must-not-open",
        );

        assert!(
            OperationalAuditGenerationDestination::resolve(
                &catalog,
                Path::new("/unused"),
                DeploymentIdentifier::from_bytes([0x72; 16]).unwrap(),
                key,
                Some(&generation),
                ServerLogAuthority::new(),
            )
            .is_err()
        );
        assert_eq!(factory_calls.load(Ordering::SeqCst), 0);
        assert_eq!(delivery_calls.load(Ordering::SeqCst), 0);
        assert_eq!(*received.lock().unwrap(), None);
    }

    fn generation_persistence() -> LogConfigurationGenerationPersistence {
        LogConfigurationGenerationPersistence::from_server_authority(&ServerDatabaseAuthority::new())
    }

    fn generation(
        persistence: &LogConfigurationGenerationPersistence,
        key: LogConfigurationGenerationKey,
        enabled: bool,
        log_types: Vec<LogType>,
        endpoint: &str,
    ) -> LogConfigurationGeneration {
        persistence
            .generation(
                key,
                Name::new("generation-test").unwrap(),
                Name::new("audit-generation").unwrap(),
                enabled,
                vec![LogModuleSetting {
                    key: ConfigurationKey::new("endpoint").unwrap(),
                    value: ConfigurationValue::new(endpoint).unwrap(),
                }],
                log_types,
            )
            .unwrap()
    }

    fn generation_catalog(
        capabilities: Vec<LogRecordType>,
        factory_calls: Arc<AtomicUsize>,
        delivery_calls: Arc<AtomicUsize>,
        received: Arc<Mutex<Option<String>>>,
    ) -> LogModuleCatalog {
        LogModuleCatalog::new(vec![LogModuleRegistration::new(
            "generation-test",
            LogCapabilities::new(capabilities).unwrap(),
            Box::new(GenerationResolverFactory {
                factory_calls,
                delivery_calls,
                received,
            }),
        )])
        .unwrap()
    }

    #[test]
    fn absent_recovery_store_keeps_reads_available_for_later_gate_attempts() {
        let DestinationFixture { destination, .. } =
            operational_destination([0x42; 16], None, None, None);
        let store = Arc::new(Mutex::new(StoreState::default()));
        let database = OperationalDatabase::from_open(Box::new(RecoveryDatabase {
            state: store,
            serves_recovery: false,
            serves_generations: false,
        }));
        assert!(database.with(|_| ()).is_ok());
        let recovery = recovery(database.clone(), destination);
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
        assert!(database.with(|_| ()).is_ok());
    }

    #[test]
    fn load_generation_store_accessors_unavailable_when_no_store() {
        let database = OperationalDatabase::from_open(Box::new(RecoveryDatabase {
            state: Arc::new(Mutex::new(StoreState::default())),
            serves_recovery: false,
            serves_generations: false,
        }));
        let persistence = generation_persistence();
        let key = persistence.key(
            StateIdentifier::from_bytes([0xAA; 16]).unwrap(),
            LogConfigurationVersion::new(1).unwrap(),
        );

        let result_current = database.load_current_audit_log_configuration_generation();
        assert_eq!(result_current, Err(DatabaseError::Unavailable));

        let result_historical = database.load_log_configuration_generation(key);
        assert_eq!(result_historical, Err(DatabaseError::Unavailable));
    }

    #[test]
    fn load_generation_store_accessors_work_with_store() {
        let persistence = generation_persistence();
        let key = persistence.key(
            StateIdentifier::from_bytes([0xBB; 16]).unwrap(),
            LogConfigurationVersion::new(1).unwrap(),
        );

        let current_snapshot = generation(
            &persistence,
            key,
            true,
            vec![LogType::Audit],
            "test-endpoint",
        );
        let historical_snapshot = generation(
            &persistence,
            key,
            true,
            vec![LogType::System],
            "history-endpoint",
        );

        struct GenerationStoreDouble {
            current: LogConfigurationGeneration,
            historical: Option<LogConfigurationGeneration>,
        }

        impl LogConfigurationGenerationStore for GenerationStoreDouble {
            fn load_current_audit_log_configuration_generation(
                &mut self,
                _persistence: &LogConfigurationGenerationPersistence,
            ) -> Result<Option<LogConfigurationGeneration>, DatabaseError> {
                Ok(Some(self.current.clone()))
            }

            fn load_log_configuration_generation(
                &mut self,
                _persistence: &LogConfigurationGenerationPersistence,
                _key: LogConfigurationGenerationKey,
            ) -> Result<Option<LogConfigurationGeneration>, DatabaseError> {
                Ok(self.historical.clone())
            }
        }

        struct DatabaseDoubleWithStore {
            generation_store: GenerationStoreDouble,
        }

        impl ApplicationDatabase for DatabaseDoubleWithStore {
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
                _expected_deployment_identifier: DeploymentIdentifier,
            ) -> Result<InitializedState, DatabaseError> {
                Err(DatabaseError::Unavailable)
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

            fn load_log_configuration_audit_reference(
                &mut self,
                _persistence: &AuditReferencePersistence,
                _configuration: StateIdentifier,
            ) -> Result<
                Option<weavelit_server_database::LogConfigurationAuditReference>,
                DatabaseError,
            > {
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
                None
            }

            fn log_configuration_generations(
                &mut self,
            ) -> Option<&mut dyn LogConfigurationGenerationStore> {
                Some(&mut self.generation_store)
            }

            fn close(self: Box<Self>) -> Result<(), DatabaseError> {
                Ok(())
            }
        }

        let database = OperationalDatabase::from_open(Box::new(DatabaseDoubleWithStore {
            generation_store: GenerationStoreDouble {
                current: current_snapshot.clone(),
                historical: Some(historical_snapshot.clone()),
            },
        }));

        let result_current = database.load_current_audit_log_configuration_generation();
        assert_eq!(result_current, Ok(Some(current_snapshot)));

        let result_historical = database.load_log_configuration_generation(key);
        assert_eq!(result_historical, Ok(Some(historical_snapshot)));
    }

    #[test]
    fn load_generation_store_accessor_passes_through_absent_generation() {
        let persistence = generation_persistence();
        let key = persistence.key(
            StateIdentifier::from_bytes([0xCC; 16]).unwrap(),
            LogConfigurationVersion::new(1).unwrap(),
        );

        struct AbsentGenerationStoreDouble;

        impl LogConfigurationGenerationStore for AbsentGenerationStoreDouble {
            fn load_current_audit_log_configuration_generation(
                &mut self,
                _persistence: &LogConfigurationGenerationPersistence,
            ) -> Result<Option<LogConfigurationGeneration>, DatabaseError> {
                Ok(None)
            }

            fn load_log_configuration_generation(
                &mut self,
                _persistence: &LogConfigurationGenerationPersistence,
                _key: LogConfigurationGenerationKey,
            ) -> Result<Option<LogConfigurationGeneration>, DatabaseError> {
                Ok(None)
            }
        }

        struct DatabaseDoubleWithAbsentStore {
            generation_store: AbsentGenerationStoreDouble,
        }

        impl ApplicationDatabase for DatabaseDoubleWithAbsentStore {
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
                _expected_deployment_identifier: DeploymentIdentifier,
            ) -> Result<InitializedState, DatabaseError> {
                Err(DatabaseError::Unavailable)
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

            fn load_log_configuration_audit_reference(
                &mut self,
                _persistence: &AuditReferencePersistence,
                _configuration: StateIdentifier,
            ) -> Result<
                Option<weavelit_server_database::LogConfigurationAuditReference>,
                DatabaseError,
            > {
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
                None
            }

            fn log_configuration_generations(
                &mut self,
            ) -> Option<&mut dyn LogConfigurationGenerationStore> {
                Some(&mut self.generation_store)
            }

            fn close(self: Box<Self>) -> Result<(), DatabaseError> {
                Ok(())
            }
        }

        let database = OperationalDatabase::from_open(Box::new(DatabaseDoubleWithAbsentStore {
            generation_store: AbsentGenerationStoreDouble,
        }));

        let result_current = database.load_current_audit_log_configuration_generation();
        assert_eq!(result_current, Ok(None));

        let result_historical = database.load_log_configuration_generation(key);
        assert_eq!(result_historical, Ok(None));
    }

    fn initialized_state_with_settings(
        configuration_identifier: StateIdentifier,
        module: &str,
        settings: Vec<LogModuleSetting>,
    ) -> InitializedState {
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
            recovery_public_key: RecoveryPublicKey::new(RECOVERY_PUBLIC_KEY).unwrap(),
            log_module_configurations: vec![LogModuleConfiguration {
                identifier: configuration_identifier,
                module: Name::new(module).unwrap(),
                name: Name::new("audit-primary").unwrap(),
                enabled: true,
                settings,
            }],
            log_configuration_audit_references: vec![
                weavelit_server_database::LogConfigurationAuditReference::new(
                    configuration_identifier,
                    weavelit_server_database::AuditReferenceIdentifier::generate().unwrap(),
                ),
            ],
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
            completion_obligation: CompletionObligation::new(
                identifier(0xF1),
                WorkflowKind::Restore,
                LogClassification::new("lifecycle.restore").unwrap(),
                CorrelationIdentifier::new("recovery-composition").unwrap(),
                1,
                LogDetail::new("restore completed").unwrap(),
            )
            .unwrap(),
        })
        .unwrap();
        InitializedState::new(
            DeploymentIdentifier::from_bytes([0x42; 16]).unwrap(),
            state,
            true,
        )
    }
}
