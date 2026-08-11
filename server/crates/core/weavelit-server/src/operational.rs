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
    fmt,
    sync::{Arc, Mutex},
};

use weavelit_server_lifecycle::{
    ApplicationDatabase, DatabaseError, InitializedState, SealedDeployment,
};

use crate::{
    fallback_router,
    transport::{MountedSurface, TransportCapability},
};

/// The one open Application Database a sealed deployment hands to its
/// operational runtime.
///
/// Cloning shares the same handle rather than opening another, so every
/// operational route serves from the descriptor sealing or startup already
/// opened. The contract's operations all require exclusive access, so they are
/// serialized here exactly as the lifecycle mutation authority serializes its
/// own store.
#[derive(Clone)]
pub struct OperationalDatabase {
    database: Arc<Mutex<Box<dyn ApplicationDatabase>>>,
}

impl OperationalDatabase {
    /// Takes ownership of a sealed deployment's loaded state and open database.
    pub(crate) fn from_sealed(sealed: SealedDeployment) -> (InitializedState, Self) {
        let (state, database) = sealed.into_parts();
        (
            state,
            Self {
                database: Arc::new(Mutex::new(database)),
            },
        )
    }

    /// Runs one operation against the handed-over database.
    ///
    /// A lane left unusable by a panicking operation is reported as an
    /// unavailable database rather than propagating the panic, because durable
    /// state cannot be trusted to have completed safely after one.
    pub fn with<R>(
        &self,
        operation: impl FnOnce(&mut dyn ApplicationDatabase) -> R,
    ) -> Result<R, DatabaseError> {
        let mut database = self
            .database
            .lock()
            .map_err(|_| DatabaseError::Unavailable)?;
        Ok(operation(&mut **database))
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
    database: OperationalDatabase,
}

impl OperationalComposer {
    /// Composes over the database a sealed deployment handed over.
    pub(crate) const fn new(database: OperationalDatabase) -> Self {
        Self { database }
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
    /// This build serves none yet, so the sealed deployment's surface carries
    /// only its Client Module's declared asset delivery.
    fn capabilities(&self) -> Vec<TransportCapability> {
        Vec::new()
    }
}

impl fmt::Debug for OperationalComposer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OperationalComposer(REDACTED)")
    }
}
