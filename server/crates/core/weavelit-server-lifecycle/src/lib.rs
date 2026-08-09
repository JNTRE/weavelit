#![forbid(unsafe_code)]

//! Backend-neutral domain and catalog contract for the Server lifecycle.

mod arbitration;
mod catalog;
mod domain;
mod error;
mod filesystem;
mod format;
mod persistence;

pub use arbitration::WorkflowArbiter;
pub use catalog::{
    ApplicationDatabaseFactory, BackendCatalog, BackendDeclaration, BackendRegistration,
    ConnectionFieldDeclaration, ConnectionFieldInput, RetainedDatabaseInspection,
    TrustedBackendContext,
};
pub use domain::{
    BackendIdentifier, ConnectionFieldIdentifier, ConnectionFieldRequirement, ConnectionValue,
    ConnectionValueKind, DatabaseLocator, DeploymentRecord, InterruptedLifecycleAction,
    LIFECYCLE_FORMAT_VERSION, LOCATOR_GENERATION_LENGTH, LifecycleClassification,
    LifecycleProjection, LifecycleState, LocatorConnectionField, LocatorConnectionSettings,
    LocatorGeneration, MAX_CONNECTION_FIELDS, MAX_CONNECTION_VALUE_LENGTH, MAX_IDENTIFIER_LENGTH,
    SecretClassification, ValidatedConnectionField, ValidatedConnectionSettings,
};
pub use error::{
    BackendOpenError, CatalogError, ConnectionValidationError, DomainError, FieldDeclarationError,
    IdentifierError, LifecycleError, SelectionError, SelectionFailureKind, WorkflowError,
};
pub use persistence::{
    AnchorLoadState, LifecycleStore, LocatorPersistencePermit, RecordPersistencePermit,
};
pub use weavelit_server_database::{
    ApplicationDatabase, ApplicationState, CheckpointMetadata, DatabaseError, DatabaseInspection,
    DeploymentIdentifier, InitializedState, StateIdentifier, WorkflowCheckpoint, WorkflowKind,
};
