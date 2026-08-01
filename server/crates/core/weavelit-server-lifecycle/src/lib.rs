#![forbid(unsafe_code)]

//! Backend-neutral domain and catalog contract for the Server lifecycle.

mod catalog;
mod domain;
mod error;
mod filesystem;
mod format;
mod persistence;

pub use catalog::{
    ApplicationDatabaseFactory, BackendCatalog, BackendDeclaration, BackendRegistration,
    ConnectionFieldDeclaration, ConnectionFieldInput, TrustedBackendContext,
};
pub use domain::{
    BackendIdentifier, ConnectionFieldIdentifier, ConnectionFieldRequirement, ConnectionValue,
    ConnectionValueKind, DatabaseLocator, DeploymentRecord, LIFECYCLE_FORMAT_VERSION,
    LOCATOR_GENERATION_LENGTH, LifecycleClassification, LifecycleState, LocatorConnectionField,
    LocatorConnectionSettings, LocatorGeneration, MAX_CONNECTION_FIELDS,
    MAX_CONNECTION_VALUE_LENGTH, MAX_IDENTIFIER_LENGTH, SecretClassification,
    ValidatedConnectionField, ValidatedConnectionSettings,
};
pub use error::{
    BackendOpenError, CatalogError, ConnectionValidationError, DomainError, FieldDeclarationError,
    IdentifierError, LifecycleError,
};
pub use persistence::{
    AnchorLoadState, LifecycleStore, LocatorPersistencePermit, RecordPersistencePermit,
};
pub use weavelit_server_database::{
    ApplicationDatabase, DatabaseError, DatabaseInspection, DeploymentIdentifier,
    WorkflowCheckpoint, WorkflowKind,
};
