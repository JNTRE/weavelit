//! Operational surface composition and the database a sealed deployment owns.
//!
//! A sealed deployment reaches normal operation from two directions: a startup
//! that classified an already-sealed record, and an in-process Restore that
//! sealed one. Both compose their surface here, so the routes a sealed
//! deployment serves cannot depend on how it became sealed.
//!
//! An operational route reaches the listener only inside an [`OperationalMount`],
//! whose field is private to this module and which nothing but
//! [`OperationalComposer::mount`] constructs. The serving-mode switch accepts
//! nothing else for its operational mode, so a surface composed elsewhere, or
//! one whose transport registrations were dropped on the way to the listener,
//! is not a value that can be published.

use std::{
    collections::BTreeSet,
    fmt,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
};

use axum::http::Method;
use tokio::task;
use weavelit_module_client::{
    ExpectedOrigin, LIFECYCLE_RECONCILIATION_ROUTE,
    ReconciliationCapability as ClientReconciliationCapability, ReconciliationOutcome,
    ReconciliationRejection, validate_reconciliation_request,
};
use weavelit_server_authentication::RustCryptoArgon2;
use weavelit_server_database::{
    AccountAdministrationStore, AccountAuditReference, AccountCreateMutation, AccountCreateOutcome,
    AccountCredentialAuditTerminalWrites, AccountCredentialWriterStore,
    AccountPasswordResetMutation, AccountPasswordResetOutcome, AccountPasswordResetTarget,
    AccountPublicIdentifier, AccountPublicIdentifierPersistence, AccountStatusAuditTerminalWrites,
    AccountStatusMutation, AccountStatusMutationOutcome, AccountStatusTarget,
    AccountStatusWriterStore, AuditReferencePersistence, AuditTerminalRecoveryPersistence,
    AuditTerminalRecoveryStore, LogConfigurationAuditReference,
    LogConfigurationAuditTerminalWrites, LogConfigurationGeneration, LogConfigurationGenerationKey,
    LogConfigurationGenerationPersistence, LogConfigurationGenerationStore,
    LogConfigurationMutationOutcome, LogConfigurationMutationPersistence,
    LogConfigurationMutationRequest, LogConfigurationPreparation, LogConfigurationVersion,
    MfaStore, PreparedLogConfigurationMutation, StateIdentifier,
};
use weavelit_server_lifecycle::{
    ApplicationDatabase, DatabaseError, InitializedState, ProtectedValueAccess, SealedDeployment,
    SelectedDatabase,
};
use weavelit_server_log::LogModuleCatalog;
use weavelit_server_restore::Name;
use zeroize::Zeroizing;

use crate::{
    authentication::AuthenticationRuntime,
    fallback_router,
    operational_audit::{OperationalAuditRecovery, OperationalAuditRecoveryState},
    transport::{
        MountedSurface, PreBodyCheck, PreBodyGrant, PreBodyRejection, TransportCapability,
        TransportProfile, TransportRegistration,
    },
};

/// The Server-wide values every operational composition is built against.
///
/// A sealed deployment reaches operation from startup or from a completed
/// Restore, and both compose from this one value, so the two paths cannot
/// disagree about the trusted authority, the state root, the Log Module
/// catalog, the Client Modules this build can serve, or the process-wide owner
/// that closes the deployment's Application Database.
pub struct OperationalRuntime {
    /// The authority every operational request must target.
    pub listener: SocketAddr,
    /// The Server's local state root.
    pub state_root: PathBuf,
    /// The Log Modules this build can open.
    pub log_catalog: Arc<LogModuleCatalog>,
    /// The Client Modules this build can issue a session for.
    pub client_modules: BTreeSet<Name>,
    /// The process-wide owner shutdown closes the database through.
    pub active_database: ActiveDatabase,
    /// The deployment's at-rest protection for enrolled MFA factor data.
    pub protection: Arc<dyn ProtectedValueAccess>,
}

/// The pause a test drives a database registration through.
///
/// Registering the database is the moment a shutdown's close stops being a
/// silent no-op, so a test that must place a stop inside the window between a
/// sealed record and its serving handle has no other deterministic boundary to
/// park at.
#[cfg(test)]
pub(crate) type ActivationHook = Arc<dyn Fn() + Send + Sync>;

/// The process-wide owner of whichever Application Database is serving.
///
/// A deployment becomes operational from a sealed startup or from an in-process
/// Restore, and each keeps its own composition afterwards, so shutdown cannot
/// close the database by asking either path. Composing an operational surface
/// registers its database here instead, which is the one place both paths pass
/// through, so what shutdown closes does not depend on how the deployment
/// became operational.
#[derive(Clone, Default)]
pub struct ActiveDatabase {
    active: Arc<Mutex<Option<OperationalDatabase>>>,
    /// The pause a test drives an activation through.
    #[cfg(test)]
    activating: Arc<Mutex<Option<ActivationHook>>>,
}

impl ActiveDatabase {
    /// Records the database an operational composition serves from.
    fn activate(&self, database: OperationalDatabase) {
        #[cfg(test)]
        self.pause_activation();
        *self.held() = Some(database);
    }

    /// Runs the installed pause, if any, immediately before the slot is filled.
    ///
    /// The hook is cloned out of its lock before it runs, so a parked chain
    /// holds nothing the test needs, and it runs outside the slot's own lock,
    /// so a close racing the pause observes the slot as it really is.
    #[cfg(test)]
    fn pause_activation(&self) {
        let hook = self
            .activating
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(hook) = hook {
            hook();
        }
    }

    /// Installs the pause a test drives an activation through.
    #[cfg(test)]
    pub(crate) fn pause_activation_with(&self, hook: ActivationHook) {
        *self
            .activating
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(hook);
    }

    /// Closes the operational database, if one was ever activated.
    ///
    /// Closing twice is not an error: the close below happens exactly once
    /// however many times this is called, so a duplicate shutdown reports the
    /// same clean result rather than a second, different one.
    pub fn close(&self) -> Result<(), DatabaseError> {
        let active = self.held().clone();
        active.map_or(Ok(()), |database| database.close())
    }

    /// Borrows the slot, recovering it from a panic that left it poisoned.
    ///
    /// Nothing but a move in or out happens under this lock, so a poisoned slot
    /// still holds a usable value. The database's own lane makes the decision
    /// about a poisoned application operation.
    fn held(&self) -> std::sync::MutexGuard<'_, Option<OperationalDatabase>> {
        self.active.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl fmt::Debug for ActiveDatabase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ActiveDatabase(REDACTED)")
    }
}

/// The one open Application Database a sealed deployment hands to its
/// operational runtime.
///
/// Cloning shares the same handle rather than opening another, so every
/// operational route serves from the descriptor sealing or startup already
/// opened. The contract's operations all require exclusive access, so they are
/// serialized here exactly as the lifecycle mutation authority serializes its
/// own store.
///
/// The handle is held as an option so shutdown can take it. Taking it is what
/// makes the close happen exactly once across every clone, and it leaves every
/// later operation unavailable rather than racing a closing backend.
#[derive(Clone)]
pub struct OperationalDatabase {
    database: Arc<Mutex<Option<OperationalDatabaseHandle>>>,
    account_public_identifier_persistence: AccountPublicIdentifierPersistence,
    audit_reference_persistence: AuditReferencePersistence,
    audit_terminal_recovery_persistence: Arc<AuditTerminalRecoveryPersistence>,
    log_configuration_generation_persistence: Arc<LogConfigurationGenerationPersistence>,
    log_configuration_mutation_persistence: Arc<LogConfigurationMutationPersistence>,
}

enum OperationalDatabaseHandle {
    Selected(SelectedDatabase),
    #[cfg(test)]
    UnselectedTest {
        database: Box<dyn ApplicationDatabase>,
        audit_reference_persistence: AuditReferencePersistence,
    },
}

impl OperationalDatabase {
    /// Takes ownership of a sealed deployment's loaded state and open database.
    pub(crate) fn from_sealed(sealed: SealedDeployment) -> (InitializedState, Self) {
        let (state, database) = sealed.into_parts();
        (state, Self::from_selected(database))
    }

    /// Takes ownership of an already-open database.
    #[cfg(test)]
    pub(crate) fn from_open(database: Box<dyn ApplicationDatabase>) -> Self {
        let authority = weavelit_server_database_authority::ServerDatabaseAuthority::new();
        Self {
            database: Arc::new(Mutex::new(Some(
                OperationalDatabaseHandle::UnselectedTest {
                    database,
                    audit_reference_persistence: AuditReferencePersistence::from_server_authority(
                        &authority,
                    ),
                },
            ))),
            account_public_identifier_persistence:
                AccountPublicIdentifierPersistence::from_server_authority(&authority),
            audit_reference_persistence: AuditReferencePersistence::from_server_authority(
                &authority,
            ),
            audit_terminal_recovery_persistence: Arc::new(
                AuditTerminalRecoveryPersistence::from_server_authority(&authority),
            ),
            log_configuration_generation_persistence: Arc::new(
                LogConfigurationGenerationPersistence::from_server_authority(&authority),
            ),
            log_configuration_mutation_persistence: Arc::new(
                LogConfigurationMutationPersistence::from_server_authority(&authority),
            ),
        }
    }

    /// Takes ownership of a lifecycle-selected database and its decoder.
    fn from_selected(database: SelectedDatabase) -> Self {
        let account_public_identifier_persistence =
            database.account_public_identifier_persistence();
        let audit_reference_persistence = database.audit_reference_persistence();
        let audit_terminal_recovery_persistence = database.audit_terminal_recovery_persistence();
        let log_configuration_generation_persistence =
            database.log_configuration_generation_persistence();
        let log_configuration_mutation_persistence =
            database.log_configuration_mutation_persistence();
        Self {
            database: Arc::new(Mutex::new(Some(OperationalDatabaseHandle::Selected(
                database,
            )))),
            account_public_identifier_persistence,
            audit_reference_persistence,
            audit_terminal_recovery_persistence,
            log_configuration_generation_persistence,
            log_configuration_mutation_persistence,
        }
    }

    /// Runs one operation against the handed-over database.
    ///
    /// A lane left unusable by a panicking operation is reported as an
    /// unavailable database rather than propagating the panic, because durable
    /// state cannot be trusted to have completed safely after one. A database
    /// shutdown already took is reported the same way.
    pub fn with<R>(
        &self,
        operation: impl FnOnce(&mut dyn ApplicationDatabase) -> R,
    ) -> Result<R, DatabaseError> {
        let mut database = self
            .database
            .lock()
            .map_err(|_| DatabaseError::Unavailable)?;
        let database = database.as_mut().ok_or(DatabaseError::Unavailable)?;
        Ok(match database {
            OperationalDatabaseHandle::Selected(database) => database.with(operation),
            #[cfg(test)]
            OperationalDatabaseHandle::UnselectedTest { database, .. } => {
                operation(&mut **database)
            }
        })
    }

    /// Runs one short target-scoped operation against MFA storage.
    pub(crate) fn with_mfa<R>(
        &self,
        operation: impl FnOnce(&mut dyn MfaStore) -> Result<R, DatabaseError>,
    ) -> Result<R, DatabaseError> {
        self.with(|database| operation(database.mfa().ok_or(DatabaseError::Unavailable)?))?
    }

    /// Runs one bounded account administration read with the selected decoder.
    pub(crate) fn with_account_administration<R>(
        &self,
        operation: impl FnOnce(
            &AccountPublicIdentifierPersistence,
            &mut dyn AccountAdministrationStore,
        ) -> Result<R, DatabaseError>,
    ) -> Result<R, DatabaseError> {
        self.with(|database| {
            operation(
                &self.account_public_identifier_persistence,
                database
                    .account_administration()
                    .ok_or(DatabaseError::Unavailable)?,
            )
        })?
    }

    /// Resolves one exact password-reset target through both selected decoders.
    pub(crate) fn prepare_account_password_reset_target(
        &self,
        target: AccountPublicIdentifier,
    ) -> Result<Option<AccountPasswordResetTarget>, DatabaseError> {
        self.with_account_credential_writers(|store| {
            store.prepare_password_reset_target(
                &self.account_public_identifier_persistence,
                &self.audit_reference_persistence,
                target,
            )
        })
    }

    /// Commits one account creation and exactly one selected Audit terminal.
    pub(crate) fn create_account(
        &self,
        mutation: &AccountCreateMutation,
        audit_terminals: &AccountCredentialAuditTerminalWrites<'_>,
    ) -> Result<AccountCreateOutcome, DatabaseError> {
        self.with_account_credential_writers(|store| {
            store.create_account(
                &self.account_public_identifier_persistence,
                mutation,
                audit_terminals,
            )
        })
    }

    /// Commits one password reset and exactly one selected Audit terminal.
    pub(crate) fn reset_account_password(
        &self,
        mutation: &AccountPasswordResetMutation,
        audit_terminals: &AccountCredentialAuditTerminalWrites<'_>,
    ) -> Result<AccountPasswordResetOutcome, DatabaseError> {
        self.with_account_credential_writers(|store| {
            store.reset_account_password(
                &self.account_public_identifier_persistence,
                mutation,
                audit_terminals,
            )
        })
    }

    fn with_account_credential_writers<R>(
        &self,
        operation: impl FnOnce(&mut dyn AccountCredentialWriterStore) -> Result<R, DatabaseError>,
    ) -> Result<R, DatabaseError> {
        self.with(|database| {
            operation(
                database
                    .account_credential_writers()
                    .ok_or(DatabaseError::Unavailable)?,
            )
        })?
    }

    /// Resolves one exact account status target through both selected decoders.
    pub(crate) fn prepare_account_status_target(
        &self,
        target: AccountPublicIdentifier,
    ) -> Result<Option<AccountStatusTarget>, DatabaseError> {
        self.with_account_status_writers(|store| {
            store.prepare_account_status_target(
                &self.account_public_identifier_persistence,
                &self.audit_reference_persistence,
                target,
            )
        })
    }

    /// Commits one account status change and exactly one selected Audit terminal.
    pub(crate) fn change_account_status(
        &self,
        mutation: &AccountStatusMutation,
        audit_terminals: &AccountStatusAuditTerminalWrites<'_>,
    ) -> Result<AccountStatusMutationOutcome, DatabaseError> {
        self.with_account_status_writers(|store| {
            store.change_account_status(
                &self.account_public_identifier_persistence,
                mutation,
                audit_terminals,
            )
        })
    }

    fn with_account_status_writers<R>(
        &self,
        operation: impl FnOnce(&mut dyn AccountStatusWriterStore) -> Result<R, DatabaseError>,
    ) -> Result<R, DatabaseError> {
        self.with(|database| {
            operation(
                database
                    .account_status_writers()
                    .ok_or(DatabaseError::Unavailable)?,
            )
        })?
    }

    /// Loads the authenticated actor's typed Audit Reference through database authority.
    pub(crate) fn load_account_audit_reference(
        &self,
        account: StateIdentifier,
    ) -> Result<Option<AccountAuditReference>, DatabaseError> {
        let mut database = self
            .database
            .lock()
            .map_err(|_| DatabaseError::Unavailable)?;
        match database.as_mut().ok_or(DatabaseError::Unavailable)? {
            OperationalDatabaseHandle::Selected(database) => {
                database.load_account_audit_reference(account)
            }
            #[cfg(test)]
            OperationalDatabaseHandle::UnselectedTest {
                database,
                audit_reference_persistence,
                ..
            } => database.load_account_audit_reference(audit_reference_persistence, account),
        }
    }

    /// Loads one Log Module configuration's typed Audit Reference.
    pub(crate) fn load_log_configuration_audit_reference(
        &self,
        configuration: StateIdentifier,
    ) -> Result<Option<LogConfigurationAuditReference>, DatabaseError> {
        let mut database = self
            .database
            .lock()
            .map_err(|_| DatabaseError::Unavailable)?;
        match database.as_mut().ok_or(DatabaseError::Unavailable)? {
            OperationalDatabaseHandle::Selected(database) => {
                database.load_log_configuration_audit_reference(configuration)
            }
            #[cfg(test)]
            OperationalDatabaseHandle::UnselectedTest {
                database,
                audit_reference_persistence,
                ..
            } => database
                .load_log_configuration_audit_reference(audit_reference_persistence, configuration),
        }
    }

    /// Prepares one Log configuration mutation from one atomic backend snapshot.
    pub(crate) fn prepare_log_configuration_mutation(
        &self,
        request: &LogConfigurationMutationRequest,
    ) -> Result<LogConfigurationPreparation, DatabaseError> {
        self.with(|database| {
            database
                .log_configuration_mutations()
                .ok_or(DatabaseError::Unavailable)?
                .prepare_log_configuration_mutation(
                    &self.log_configuration_generation_persistence,
                    &self.log_configuration_mutation_persistence,
                    request,
                )
        })?
    }

    /// Commits one prepared mutation and exactly one selected terminal obligation.
    pub(crate) fn commit_log_configuration_mutation(
        &self,
        mutation: &PreparedLogConfigurationMutation,
        audit_terminals: &LogConfigurationAuditTerminalWrites<'_>,
    ) -> Result<LogConfigurationMutationOutcome, DatabaseError> {
        self.with(|database| {
            database
                .log_configuration_mutations()
                .ok_or(DatabaseError::Unavailable)?
                .commit_log_configuration_mutation(mutation, audit_terminals)
        })?
    }

    /// Runs one short operation against terminal recovery storage and its decoder.
    pub(crate) fn with_audit_terminal_recovery<R>(
        &self,
        operation: impl FnOnce(
            &AuditTerminalRecoveryPersistence,
            &mut dyn AuditTerminalRecoveryStore,
        ) -> Result<R, DatabaseError>,
    ) -> Result<R, DatabaseError> {
        self.with(|database| {
            operation(
                &self.audit_terminal_recovery_persistence,
                database
                    .audit_terminal_recovery()
                    .ok_or(DatabaseError::Unavailable)?,
            )
        })?
    }

    /// Returns the selected decoder without taking the database operation lane.
    pub(crate) fn audit_terminal_recovery_persistence(&self) -> &AuditTerminalRecoveryPersistence {
        &self.audit_terminal_recovery_persistence
    }

    /// Loads the current Audit configuration generation from a capable backend.
    pub(crate) fn load_current_audit_log_configuration_generation(
        &self,
    ) -> Result<Option<LogConfigurationGeneration>, DatabaseError> {
        self.with_log_configuration_generations(|persistence, store| {
            store.load_current_audit_log_configuration_generation(persistence)
        })
    }

    /// Loads one exact historical configuration generation from a capable backend.
    pub(crate) fn load_log_configuration_generation(
        &self,
        key: LogConfigurationGenerationKey,
    ) -> Result<Option<LogConfigurationGeneration>, DatabaseError> {
        self.with_log_configuration_generations(|persistence, store| {
            store.load_log_configuration_generation(persistence, key)
        })
    }

    /// Converts one retained Audit binding into its exact generation key.
    pub(crate) fn log_configuration_generation_key(
        &self,
        configuration: [u8; 16],
        version: u64,
    ) -> Result<LogConfigurationGenerationKey, DatabaseError> {
        let configuration = StateIdentifier::from_bytes(configuration)
            .map_err(|_| DatabaseError::IntegrityFailure)?;
        let version =
            LogConfigurationVersion::new(version).ok_or(DatabaseError::IntegrityFailure)?;
        Ok(self
            .log_configuration_generation_persistence
            .key(configuration, version))
    }

    fn with_log_configuration_generations<R>(
        &self,
        operation: impl FnOnce(
            &LogConfigurationGenerationPersistence,
            &mut dyn LogConfigurationGenerationStore,
        ) -> Result<R, DatabaseError>,
    ) -> Result<R, DatabaseError> {
        self.with(|database| {
            operation(
                &self.log_configuration_generation_persistence,
                database
                    .log_configuration_generations()
                    .ok_or(DatabaseError::Unavailable)?,
            )
        })?
    }

    /// Loads initialized state through the selected database's decoder.
    pub fn load_initialized_state(
        &self,
        expected_deployment_identifier: weavelit_server_database::DeploymentIdentifier,
    ) -> Result<InitializedState, DatabaseError> {
        let mut database = self
            .database
            .lock()
            .map_err(|_| DatabaseError::Unavailable)?;
        match database.as_mut().ok_or(DatabaseError::Unavailable)? {
            OperationalDatabaseHandle::Selected(database) => {
                database.load_initialized_state(expected_deployment_identifier)
            }
            #[cfg(test)]
            OperationalDatabaseHandle::UnselectedTest {
                database,
                audit_reference_persistence,
            } => database.load_initialized_state(
                &self.account_public_identifier_persistence,
                audit_reference_persistence,
                expected_deployment_identifier,
            ),
        }
    }

    /// Takes the database out of every clone at once and closes it.
    ///
    /// A poisoned lane is recovered only far enough to take the database and
    /// close it. It stays poisoned, so no application operation can run
    /// afterwards, and the shutdown is reported as failed however cleanly the
    /// backend closed: the operation that poisoned the lane has an untrusted
    /// outcome, and a clean close does not make it trustworthy.
    fn close(&self) -> Result<(), DatabaseError> {
        let (taken, poisoned) = match self.database.lock() {
            Ok(mut database) => (database.take(), false),
            Err(poisoned) => (poisoned.into_inner().take(), true),
        };
        let closed = taken.map_or(Ok(()), |database| match database {
            OperationalDatabaseHandle::Selected(database) => database.close(),
            #[cfg(test)]
            OperationalDatabaseHandle::UnselectedTest { database, .. } => database.close(),
        });

        closed.and(if poisoned {
            Err(DatabaseError::Unavailable)
        } else {
            Ok(())
        })
    }

    #[cfg(test)]
    pub(crate) fn close_for_test(&self) -> Result<(), DatabaseError> {
        self.close()
    }

    /// Compares one submitted reconciliation capability against the live
    /// reconciliation store without reading sessions or application state.
    fn reconcile(&self, submitted: Zeroizing<String>) -> ReconciliationOutcome {
        let digest =
            crate::reconciliation::ReconciliationCapability::from_submitted(submitted).digest();
        match self.with(|database| {
            database
                .reconciliation()
                .ok_or(DatabaseError::Unavailable)
                .and_then(|store| store.matches_reconciliation(&digest))
        }) {
            Ok(Ok(true)) => ReconciliationOutcome::Confirmed,
            Ok(Ok(false)) => ReconciliationOutcome::NotFound,
            Ok(Err(_)) | Err(_) => ReconciliationOutcome::Unavailable,
        }
    }
}

impl fmt::Debug for OperationalDatabase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OperationalDatabase(REDACTED)")
    }
}

/// A composed operational surface: its router and its transport registrations
/// as one value.
///
/// Only [`OperationalComposer::mount`] builds this, so an operational route
/// cannot reach the listener without the registrations composed alongside it.
pub struct OperationalMount {
    surface: MountedSurface,
}

impl OperationalMount {
    /// Returns the router and registrations the listener serves together.
    pub(crate) fn surface(&self) -> &MountedSurface {
        &self.surface
    }
}

impl fmt::Debug for OperationalMount {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OperationalMount")
    }
}

/// Reconciliation request checks that run before the listener allocates a body.
struct ReconciliationPreconditions {
    expected_origin: ExpectedOrigin,
}

impl PreBodyCheck for ReconciliationPreconditions {
    fn check(
        &self,
        method: &Method,
        _uri: &axum::http::Uri,
        headers: &axum::http::HeaderMap,
    ) -> Result<PreBodyGrant, PreBodyRejection> {
        match validate_reconciliation_request(method, headers, self.expected_origin) {
            Ok(()) => Ok(PreBodyGrant::accepted()),
            Err(ReconciliationRejection::RequestOriginDenied) => {
                Err(PreBodyRejection::RequestOriginDenied)
            }
            Err(
                ReconciliationRejection::BadRequest | ReconciliationRejection::MethodNotAllowed,
            ) => Err(PreBodyRejection::BadRequest),
        }
    }
}

/// Composes the operational surface a sealed deployment serves.
///
/// The composer owns the deployment's open Application Database, so a route it
/// mounts shares that one handle instead of reopening the target per request.
pub struct OperationalComposer {
    runtime: Arc<OperationalRuntime>,
    database: OperationalDatabase,
    audit_recovery: OperationalAuditRecovery,
    #[cfg(test)]
    activation_audit_recovery_state: OperationalAuditRecoveryState,
    /// The authentication runtime, when one could be composed.
    ///
    /// Its absence is the declaration that this deployment serves no
    /// authentication route, so a Server that cannot deny safely serves no
    /// login at all rather than one that decides on a broken authenticator.
    authentication: Option<Arc<AuthenticationRuntime<RustCryptoArgon2>>>,
}

impl OperationalComposer {
    /// Composes over the database a sealed deployment handed over.
    ///
    /// Composing is also what registers the database with the process-wide
    /// owner, so a deployment that can serve is always a deployment shutdown
    /// can close, whichever path sealed it.
    pub(crate) fn new(
        runtime: Arc<OperationalRuntime>,
        state: &InitializedState,
        database: OperationalDatabase,
    ) -> Self {
        runtime.active_database.activate(database.clone());
        let audit_recovery = OperationalAuditRecovery::new(&runtime, state, database.clone());
        let activation_audit_recovery_state = audit_recovery.drain_for_activation();
        #[cfg(not(test))]
        let _ = activation_audit_recovery_state;
        let authentication = AuthenticationRuntime::new(
            database.clone(),
            state,
            runtime.client_modules.clone(),
            runtime.state_root.clone(),
            &runtime.log_catalog,
            Arc::clone(&runtime.protection),
        );

        Self {
            runtime,
            database,
            audit_recovery,
            #[cfg(test)]
            activation_audit_recovery_state,
            authentication,
        }
    }

    /// Composes with a test-controlled recovery coordinator before activation.
    #[cfg(test)]
    pub(crate) fn with_audit_recovery_for_test(
        runtime: Arc<OperationalRuntime>,
        state: &InitializedState,
        database: OperationalDatabase,
        audit_recovery: OperationalAuditRecovery,
    ) -> Self {
        runtime.active_database.activate(database.clone());
        let activation_audit_recovery_state = audit_recovery.drain_for_activation();
        let authentication = AuthenticationRuntime::new(
            database.clone(),
            state,
            runtime.client_modules.clone(),
            runtime.state_root.clone(),
            &runtime.log_catalog,
            Arc::clone(&runtime.protection),
        );

        Self {
            runtime,
            database,
            audit_recovery,
            activation_audit_recovery_state,
            authentication,
        }
    }

    /// Removes authentication only for a composition test.
    #[cfg(test)]
    pub(crate) fn without_authentication(mut self) -> Self {
        self.authentication = None;
        self
    }

    /// Returns the single open database every operational route shares.
    pub(crate) const fn database(&self) -> &OperationalDatabase {
        &self.database
    }

    /// Runs one bounded Audit recovery drain before a consequential workflow.
    #[allow(dead_code)]
    pub(crate) fn drain_audit_before_consequential_operation(
        &self,
    ) -> OperationalAuditRecoveryState {
        self.audit_recovery.drain_before_consequential_operation()
    }

    /// Returns the internal TOTP enablement workflow over this operational composition.
    #[allow(dead_code)]
    pub(crate) fn mfa_module_enablement_workflow(
        &self,
    ) -> crate::administration::MfaModuleEnablementWorkflow<'_> {
        crate::administration::MfaModuleEnablementWorkflow::new(
            &self.database,
            &self.audit_recovery,
        )
    }

    /// Returns the state observed by the activation drain.
    #[cfg(test)]
    pub(crate) const fn activation_audit_recovery_state(&self) -> OperationalAuditRecoveryState {
        self.activation_audit_recovery_state
    }

    /// Composes the sealed deployment's operational surface.
    ///
    /// The Client Module's declared asset delivery is mounted from its own
    /// declaration and needs no registration, so it keeps the listener's
    /// default profile. Every Server-owned operational route is added through
    /// [`MountedSurface::with_capability`], which takes a
    /// [`TransportCapability`] and therefore cannot be built without the
    /// registration that admits the route it mounts.
    pub(crate) fn mount(&self) -> OperationalMount {
        let declared = weavelit_module_client_webui::operational_surface();
        let mut surface = MountedSurface::without_registrations(declared.mount(fallback_router()));
        for capability in self.capabilities() {
            surface = surface.with_capability(capability);
        }
        OperationalMount { surface }
    }

    /// Returns every Server-owned operational route, each paired with the
    /// transport registration that admits it.
    ///
    /// Reconciliation is composed directly from the operational database, so
    /// it remains reachable when optional authentication cannot compose. Each
    /// authentication route arrives as a [`TransportCapability`], so login's
    /// single-permit admission lane travels with the route it bounds.
    fn capabilities(&self) -> Vec<TransportCapability> {
        let expected_origin = ExpectedOrigin::from_listener(self.runtime.listener);
        let database = self.database.clone();
        let route = ClientReconciliationCapability {
            expected_origin,
            reconcile: Arc::new(move |submission| {
                let database = database.clone();
                Box::pin(async move {
                    task::spawn_blocking(move || database.reconcile(submission.capability))
                        .await
                        .unwrap_or(ReconciliationOutcome::Unavailable)
                })
            }),
        }
        .route();
        let reconciliation = TransportCapability::new(
            TransportRegistration::new(
                Method::PUT,
                LIFECYCLE_RECONCILIATION_ROUTE,
                TransportProfile::DEFAULT,
            )
            .with_pre_body_check(Arc::new(ReconciliationPreconditions { expected_origin })),
            move |router| router.route(LIFECYCLE_RECONCILIATION_ROUTE, route),
        );
        let mut capabilities = vec![reconciliation];
        if let Some(runtime) = &self.authentication {
            capabilities.extend(runtime.capabilities(expected_origin));
        }
        capabilities
    }
}

impl fmt::Debug for OperationalComposer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OperationalComposer(REDACTED)")
    }
}

/// Fakes shared by this module's tests and the listener's shutdown tests.
///
/// The owner a shutdown closes through is defined here, so both suites count
/// closes through one fake rather than through two that could disagree about
/// what closing exactly once means.
#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };

    use tokio::sync::oneshot;
    use weavelit_server_database::{ReconciliationStore, SessionStore};
    use weavelit_server_lifecycle::{
        ApplicationDatabase, ApplicationState, DatabaseError, DatabaseInspection,
        DeploymentIdentifier, InitializedState, StateIdentifier, WorkflowCheckpoint,
    };

    use super::{ActiveDatabase, OperationalDatabase};

    /// A database that records how many times it was actually closed.
    ///
    /// Every other operation is refused, because these tests are about the
    /// close and nothing else.
    pub(crate) struct CountingDatabase {
        closes: Arc<AtomicUsize>,
        /// What this database's own close reports once it has been counted.
        outcome: Result<(), DatabaseError>,
        /// Test-only rendezvous for a close that must remain in progress.
        close_block: Option<CloseBlock>,
    }

    struct CloseBlock {
        arrival: oneshot::Sender<()>,
        release: mpsc::Receiver<()>,
    }

    impl ApplicationDatabase for CountingDatabase {
        fn inspect(
            &mut self,
            _expected_deployment_identifier: DeploymentIdentifier,
        ) -> Result<DatabaseInspection, DatabaseError> {
            Ok(DatabaseInspection::Uninitialized)
        }

        fn create_checkpoint(
            &mut self,
            _checkpoint: &WorkflowCheckpoint,
        ) -> Result<(), DatabaseError> {
            Err(DatabaseError::Unavailable)
        }

        fn complete_checkpoint(
            &mut self,
            _public_identity_persistence: &weavelit_server_database::AccountPublicIdentifierPersistence,
            _checkpoint: &WorkflowCheckpoint,
            _state: &ApplicationState,
            _reconciliation: &weavelit_server_database::ReconciliationDigest,
        ) -> Result<(), DatabaseError> {
            Err(DatabaseError::Unavailable)
        }

        fn load_initialized_state(
            &mut self,
            _public_identity_persistence: &weavelit_server_database::AccountPublicIdentifierPersistence,
            _audit_reference_persistence: &weavelit_server_database::AuditReferencePersistence,
            _expected_deployment_identifier: DeploymentIdentifier,
        ) -> Result<InitializedState, DatabaseError> {
            Err(DatabaseError::Unavailable)
        }

        fn load_account_public_identity(
            &mut self,
            _persistence: &weavelit_server_database::AccountPublicIdentifierPersistence,
            _public_identifier: weavelit_server_database::AccountPublicIdentifier,
        ) -> Result<Option<weavelit_server_database::AccountPublicIdentity>, DatabaseError>
        {
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
        ) -> Result<Option<weavelit_server_database::HumanAuthorizationSnapshot>, DatabaseError>
        {
            Err(DatabaseError::Unavailable)
        }

        fn load_account_audit_reference(
            &mut self,
            _persistence: &weavelit_server_database::AuditReferencePersistence,
            _account: StateIdentifier,
        ) -> Result<Option<weavelit_server_database::AccountAuditReference>, DatabaseError>
        {
            Err(DatabaseError::Unavailable)
        }

        fn load_group_audit_reference(
            &mut self,
            _persistence: &weavelit_server_database::AuditReferencePersistence,
            _group: StateIdentifier,
        ) -> Result<Option<weavelit_server_database::GroupAuditReference>, DatabaseError> {
            Err(DatabaseError::Unavailable)
        }

        fn load_log_configuration_audit_reference(
            &mut self,
            _persistence: &weavelit_server_database::AuditReferencePersistence,
            _configuration: StateIdentifier,
        ) -> Result<Option<weavelit_server_database::LogConfigurationAuditReference>, DatabaseError>
        {
            Err(DatabaseError::Unavailable)
        }

        fn load_component_enablement(
            &mut self,
        ) -> Result<weavelit_server_database::ComponentEnablement, DatabaseError> {
            Err(DatabaseError::Unavailable)
        }

        fn sessions(&mut self) -> Option<&mut dyn SessionStore> {
            None
        }

        fn mfa(&mut self) -> Option<&mut dyn weavelit_server_database::MfaStore> {
            None
        }

        fn reconciliation(&mut self) -> Option<&mut dyn ReconciliationStore> {
            None
        }

        fn audit_terminal_recovery(
            &mut self,
        ) -> Option<&mut dyn weavelit_server_database::AuditTerminalRecoveryStore> {
            None
        }

        fn close(self: Box<Self>) -> Result<(), DatabaseError> {
            self.closes.fetch_add(1, Ordering::SeqCst);
            if let Some(close_block) = self.close_block {
                let _ = close_block.arrival.send(());
                let _ = close_block.release.recv();
            }
            self.outcome
        }
    }

    /// Registers a database that closes cleanly with a process-wide owner.
    pub(crate) fn activated() -> (ActiveDatabase, OperationalDatabase, Arc<AtomicUsize>) {
        activated_closing(Ok(()))
    }

    /// Parks a lifecycle transition immediately before it registers the
    /// database it committed through.
    ///
    /// That window is the one a shutdown must not close through: the record is
    /// already sealed, but the owner's slot is still empty, so a close taken
    /// there closes nothing at all. Parking there lets a test place the stop
    /// inside the window by construction rather than by interleaving.
    ///
    /// Only the first activation parks, so a surface that composes again runs
    /// straight past this boundary.
    pub(crate) struct ActivationBarrier {
        arrived: oneshot::Receiver<()>,
        release: mpsc::Sender<()>,
    }

    /// A close parked after it starts, so shutdown tests can prove that an
    /// overdue close retains the listener without timing-based synchronization.
    pub(crate) struct CloseBarrier {
        arrived: oneshot::Receiver<()>,
        release: mpsc::Sender<()>,
    }

    impl CloseBarrier {
        pub(crate) async fn reached(&mut self) {
            (&mut self.arrived)
                .await
                .expect("the database close must begin");
        }

        pub(crate) fn release(self) {
            let _ = self.release.send(());
        }
    }

    impl ActivationBarrier {
        /// Installs the pause on the owner an operational composition registers
        /// its database with.
        pub(crate) fn install(active: &ActiveDatabase) -> Self {
            let (arrival, arrived) = oneshot::channel();
            let (release, blocked) = mpsc::channel();
            let arrival = Mutex::new(Some(arrival));
            let blocked = Mutex::new(Some(blocked));

            active.pause_activation_with(Arc::new(move || {
                let arrival = arrival
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take();
                let blocked = blocked
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take();
                if let Some(arrival) = arrival {
                    let _ = arrival.send(());
                }
                if let Some(blocked) = blocked {
                    let _ = blocked.recv();
                }
            }));

            Self { arrived, release }
        }

        /// Resolves once the transition is parked before its registration.
        pub(crate) async fn reached(&mut self) {
            (&mut self.arrived)
                .await
                .expect("the committing chain reaches its activation");
        }

        /// Lets the parked transition register its database and run on.
        pub(crate) fn release(self) {
            let _ = self.release.send(());
        }
    }

    /// Registers a database whose own close reports `outcome`.
    pub(crate) fn activated_closing(
        outcome: Result<(), DatabaseError>,
    ) -> (ActiveDatabase, OperationalDatabase, Arc<AtomicUsize>) {
        let closes = Arc::new(AtomicUsize::new(0));
        let database = OperationalDatabase::from_open(Box::new(CountingDatabase {
            closes: Arc::clone(&closes),
            outcome,
            close_block: None,
        }));
        let active = ActiveDatabase::default();
        active.activate(database.clone());
        (active, database, closes)
    }

    /// Registers a database whose close has started but cannot finish until
    /// the returned barrier is released.
    pub(crate) fn activated_blocking_close() -> (
        ActiveDatabase,
        OperationalDatabase,
        Arc<AtomicUsize>,
        CloseBarrier,
    ) {
        let closes = Arc::new(AtomicUsize::new(0));
        let (arrival, arrived) = oneshot::channel();
        let (release, blocked) = mpsc::channel();
        let database = OperationalDatabase::from_open(Box::new(CountingDatabase {
            closes: Arc::clone(&closes),
            outcome: Ok(()),
            close_block: Some(CloseBlock {
                arrival,
                release: blocked,
            }),
        }));
        let active = ActiveDatabase::default();
        active.activate(database.clone());
        (active, database, closes, CloseBarrier { arrived, release })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::{test_support::activated, *};

    #[test]
    fn an_owner_with_no_activated_database_closes_cleanly() {
        assert_eq!(ActiveDatabase::default().close(), Ok(()));
    }

    #[test]
    fn a_duplicate_shutdown_closes_the_database_exactly_once() {
        let (active, database, closes) = activated();
        // A second handle proves the take is shared rather than per-clone.
        let clone = database.clone();

        assert_eq!(active.close(), Ok(()));
        assert_eq!(active.close(), Ok(()));
        assert_eq!(ActiveDatabase::clone(&active).close(), Ok(()));

        assert_eq!(closes.load(Ordering::SeqCst), 1);
        assert_eq!(
            database.with(|_| ()).unwrap_err(),
            DatabaseError::Unavailable
        );
        assert_eq!(clone.with(|_| ()).unwrap_err(), DatabaseError::Unavailable);
    }

    #[test]
    fn a_closed_database_refuses_every_later_operation() {
        let (active, database, _closes) = activated();
        assert!(database.with(|_| ()).is_ok());

        assert_eq!(active.close(), Ok(()));

        assert_eq!(
            database.with(|_| ()).unwrap_err(),
            DatabaseError::Unavailable
        );
    }

    #[test]
    fn a_poisoned_lane_is_closed_once_and_reported_as_a_failed_shutdown() {
        let (active, database, closes) = activated();
        let panicking = database.clone();
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = panicking.with(|_| panic!("an operation panics under the lane lock"));
        }))
        .expect_err("the operation must panic");

        assert_eq!(active.close(), Err(DatabaseError::Unavailable));
        assert_eq!(active.close(), Err(DatabaseError::Unavailable));

        // The close still happened, exactly once, and nothing may run after it.
        assert_eq!(closes.load(Ordering::SeqCst), 1);
        assert_eq!(
            database.with(|_| ()).unwrap_err(),
            DatabaseError::Unavailable
        );
    }
}
