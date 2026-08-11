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

use weavelit_module_client::ExpectedOrigin;
use weavelit_server_authentication::RustCryptoArgon2;
use weavelit_server_lifecycle::{
    ApplicationDatabase, DatabaseError, InitializedState, SealedDeployment,
};
use weavelit_server_log::LogModuleCatalog;
use weavelit_server_restore::Name;

use crate::{
    authentication::AuthenticationRuntime,
    fallback_router,
    transport::{MountedSurface, TransportCapability},
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
}

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
}

impl ActiveDatabase {
    /// Records the database an operational composition serves from.
    fn activate(&self, database: OperationalDatabase) {
        *self.held() = Some(database);
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
    database: Arc<Mutex<Option<Box<dyn ApplicationDatabase>>>>,
}

impl OperationalDatabase {
    /// Takes ownership of a sealed deployment's loaded state and open database.
    pub(crate) fn from_sealed(sealed: SealedDeployment) -> (InitializedState, Self) {
        let (state, database) = sealed.into_parts();
        (state, Self::from_open(database))
    }

    /// Takes ownership of an already-open database.
    pub(crate) fn from_open(database: Box<dyn ApplicationDatabase>) -> Self {
        Self {
            database: Arc::new(Mutex::new(Some(database))),
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
        Ok(operation(&mut **database))
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
        let closed = taken.map_or(Ok(()), ApplicationDatabase::close);

        closed.and(if poisoned {
            Err(DatabaseError::Unavailable)
        } else {
            Ok(())
        })
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

/// Composes the operational surface a sealed deployment serves.
///
/// The composer owns the deployment's open Application Database, so a route it
/// mounts shares that one handle instead of reopening the target per request.
pub struct OperationalComposer {
    runtime: Arc<OperationalRuntime>,
    database: OperationalDatabase,
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
        let authentication = AuthenticationRuntime::new(
            database.clone(),
            state,
            runtime.client_modules.clone(),
            runtime.state_root.clone(),
            &runtime.log_catalog,
        );

        Self {
            runtime,
            database,
            authentication,
        }
    }

    /// Returns the single open database every operational route shares.
    pub(crate) const fn database(&self) -> &OperationalDatabase {
        &self.database
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
    /// Authentication is the only family this build serves. Each of its three
    /// routes arrives as a [`TransportCapability`], so login's single-permit
    /// admission lane travels with the route it bounds.
    fn capabilities(&self) -> Vec<TransportCapability> {
        self.authentication
            .as_ref()
            .map_or_else(Vec::new, |runtime| {
                runtime.capabilities(ExpectedOrigin::from_listener(self.runtime.listener))
            })
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
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use weavelit_server_database::SessionStore;
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
            _checkpoint: &WorkflowCheckpoint,
            _state: &ApplicationState,
        ) -> Result<(), DatabaseError> {
            Err(DatabaseError::Unavailable)
        }

        fn load_initialized_state(
            &mut self,
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
        ) -> Result<Option<weavelit_server_database::HumanAuthorizationSnapshot>, DatabaseError>
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

        fn close(self: Box<Self>) -> Result<(), DatabaseError> {
            self.closes.fetch_add(1, Ordering::SeqCst);
            self.outcome
        }
    }

    /// Registers a database that closes cleanly with a process-wide owner.
    pub(crate) fn activated() -> (ActiveDatabase, OperationalDatabase, Arc<AtomicUsize>) {
        activated_closing(Ok(()))
    }

    /// Registers a database whose own close reports `outcome`.
    pub(crate) fn activated_closing(
        outcome: Result<(), DatabaseError>,
    ) -> (ActiveDatabase, OperationalDatabase, Arc<AtomicUsize>) {
        let closes = Arc::new(AtomicUsize::new(0));
        let database = OperationalDatabase::from_open(Box::new(CountingDatabase {
            closes: Arc::clone(&closes),
            outcome,
        }));
        let active = ActiveDatabase::default();
        active.activate(database.clone());
        (active, database, closes)
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
