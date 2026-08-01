use std::{error::Error as StdError, fmt};

/// Invalid lifecycle domain value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomainError {
    /// A locator generation used the reserved all-zero representation.
    InvalidLocatorGeneration,
    /// A deployment record state did not include its required locator.
    InvalidDeploymentRecord,
}

impl fmt::Display for DomainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidLocatorGeneration => "locator generation is invalid",
            Self::InvalidDeploymentRecord => "deployment record is invalid",
        };
        formatter.write_str(message)
    }
}

impl StdError for DomainError {}

/// Invalid stable identifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentifierError {
    /// The identifier did not use the bounded lowercase kebab-case grammar.
    Invalid,
}

impl fmt::Display for IdentifierError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("identifier is invalid")
    }
}

impl StdError for IdentifierError {}

/// Invalid backend connection-field declaration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FieldDeclarationError {
    /// The field identifier was invalid.
    InvalidIdentifier,
    /// The field attempted to expose a local path or file reference.
    LocalReferenceForbidden,
}

impl fmt::Display for FieldDeclarationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidIdentifier => "connection field identifier is invalid",
            Self::LocalReferenceForbidden => "local connection references are forbidden",
        };
        formatter.write_str(message)
    }
}

impl StdError for FieldDeclarationError {}

/// Invalid runtime-supplied backend catalog.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogError {
    /// No backend was registered.
    Empty,
    /// More backends than the contract permits were registered.
    TooManyBackends,
    /// A backend identifier was empty or invalid.
    InvalidBackendIdentifier,
    /// More than one registration used the same backend identifier.
    DuplicateBackendIdentifier,
    /// A backend declared too many connection fields.
    TooManyConnectionFields,
    /// A backend declared the same connection field more than once.
    DuplicateConnectionField,
}

impl fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Empty => "backend catalog is empty",
            Self::TooManyBackends => "backend catalog is too large",
            Self::InvalidBackendIdentifier => "backend catalog identifier is invalid",
            Self::DuplicateBackendIdentifier => "backend catalog contains a duplicate",
            Self::TooManyConnectionFields => "backend declaration has too many fields",
            Self::DuplicateConnectionField => "backend declaration contains a duplicate field",
        };
        formatter.write_str(message)
    }
}

impl StdError for CatalogError {}

/// Invalid submitted connection fields rejected before a factory is invoked.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionValidationError {
    /// The selected backend was not registered.
    UnknownBackend,
    /// More values than the contract permits were submitted.
    TooManyFields,
    /// A submitted field was not declared by the backend.
    UnknownField,
    /// A field was submitted more than once.
    DuplicateField,
    /// A required field was absent.
    MissingRequiredField,
    /// A submitted value had the wrong scalar kind.
    WrongValueKind,
    /// A submitted field's secret classification did not match the declaration.
    ClassificationMismatch,
    /// A submitted string or byte value exceeded its bound.
    ValueTooLarge,
}

impl fmt::Display for ConnectionValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("connection settings are invalid")
    }
}

impl StdError for ConnectionValidationError {}

/// Stable payload-free lifecycle failure category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum LifecycleError {
    /// Lifecycle persistence could not complete safely.
    Persistence,
    /// Trusted host or backend configuration was invalid.
    ConfigurationInvalid,
    /// A required storage or backend dependency was unavailable.
    DependencyUnavailable,
    /// Persisted state or its integrity could not be trusted.
    IntegrityFailure,
    /// Durable state belonged to another deployment.
    DeploymentMismatch,
    /// The requested or retained lifecycle state was invalid.
    InvalidState,
    /// The retained format or algorithm version was unsupported.
    UnsupportedVersion,
}

impl fmt::Display for LifecycleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Persistence => "lifecycle persistence failed",
            Self::ConfigurationInvalid => "lifecycle configuration is invalid",
            Self::DependencyUnavailable => "lifecycle dependency is unavailable",
            Self::IntegrityFailure => "lifecycle integrity validation failed",
            Self::DeploymentMismatch => "lifecycle deployment does not match",
            Self::InvalidState => "lifecycle state is invalid",
            Self::UnsupportedVersion => "lifecycle version is unsupported",
        };
        formatter.write_str(message)
    }
}

impl StdError for LifecycleError {}

/// Failure to validate or open a selected backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendOpenError {
    /// Submitted connection fields failed common validation.
    ConnectionInvalid(ConnectionValidationError),
    /// The backend factory returned a stable lifecycle category.
    Factory(LifecycleError),
}

impl fmt::Display for BackendOpenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConnectionInvalid(error) => error.fmt(formatter),
            Self::Factory(error) => error.fmt(formatter),
        }
    }
}

impl StdError for BackendOpenError {}
