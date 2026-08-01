use std::{
    fmt,
    path::{Path, PathBuf},
};

use crate::{
    ApplicationDatabase, BackendIdentifier, BackendOpenError, CatalogError,
    ConnectionFieldIdentifier, ConnectionFieldRequirement, ConnectionValidationError,
    ConnectionValue, ConnectionValueKind, FieldDeclarationError, LifecycleError,
    SecretClassification, ValidatedConnectionField, ValidatedConnectionSettings,
    domain::MAX_CONNECTION_FIELDS,
};

const MAX_BACKENDS: usize = 64;
const FORBIDDEN_LOCAL_REFERENCE_TOKENS: &[&str] =
    &["dir", "directory", "file", "filename", "filepath", "path"];

/// Trusted Server-derived local context supplied separately from client values.
pub struct TrustedBackendContext {
    application_database_path: PathBuf,
}

impl TrustedBackendContext {
    /// Creates context from a path derived by trusted Server policy.
    pub fn new(application_database_path: PathBuf) -> Self {
        Self {
            application_database_path,
        }
    }

    /// Returns the trusted Application Database path.
    pub fn application_database_path(&self) -> &Path {
        &self.application_database_path
    }
}

impl fmt::Debug for TrustedBackendContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TrustedBackendContext")
            .finish_non_exhaustive()
    }
}

/// One trusted connection-field declaration from a compiled-in backend.
#[derive(Clone, Eq, PartialEq)]
pub struct ConnectionFieldDeclaration {
    identifier: ConnectionFieldIdentifier,
    value_kind: ConnectionValueKind,
    requirement: ConnectionFieldRequirement,
    classification: SecretClassification,
}

impl ConnectionFieldDeclaration {
    /// Creates a declaration that cannot represent a local path or file reference.
    pub fn new(
        identifier: impl Into<Box<str>>,
        value_kind: ConnectionValueKind,
        requirement: ConnectionFieldRequirement,
        classification: SecretClassification,
    ) -> Result<Self, FieldDeclarationError> {
        let identifier = ConnectionFieldIdentifier::new(identifier)
            .map_err(|_| FieldDeclarationError::InvalidIdentifier)?;
        if identifier
            .as_str()
            .split('-')
            .any(|token| FORBIDDEN_LOCAL_REFERENCE_TOKENS.contains(&token))
        {
            return Err(FieldDeclarationError::LocalReferenceForbidden);
        }
        Ok(Self {
            identifier,
            value_kind,
            requirement,
            classification,
        })
    }

    /// Returns the field identifier.
    pub const fn identifier(&self) -> &ConnectionFieldIdentifier {
        &self.identifier
    }

    /// Returns the required scalar kind.
    pub const fn value_kind(&self) -> ConnectionValueKind {
        self.value_kind
    }

    /// Returns whether this field is required.
    pub const fn requirement(&self) -> ConnectionFieldRequirement {
        self.requirement
    }

    /// Returns the trusted secret classification.
    pub const fn classification(&self) -> SecretClassification {
        self.classification
    }
}

impl fmt::Debug for ConnectionFieldDeclaration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectionFieldDeclaration")
            .field("value_kind", &self.value_kind)
            .field("requirement", &self.requirement)
            .field("classification", &self.classification)
            .finish_non_exhaustive()
    }
}

/// One caller-submitted field awaiting trusted declaration validation.
#[derive(Clone, Eq, PartialEq)]
pub struct ConnectionFieldInput {
    identifier: ConnectionFieldIdentifier,
    classification: SecretClassification,
    value: ConnectionValue,
}

impl ConnectionFieldInput {
    /// Creates an input value for common validation.
    pub const fn new(
        identifier: ConnectionFieldIdentifier,
        classification: SecretClassification,
        value: ConnectionValue,
    ) -> Self {
        Self {
            identifier,
            classification,
            value,
        }
    }
}

impl fmt::Debug for ConnectionFieldInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectionFieldInput")
            .field("classification", &self.classification)
            .field("kind", &self.value.kind())
            .finish_non_exhaustive()
    }
}

/// Factory for one runtime-supplied compiled-in Application Database backend.
pub trait ApplicationDatabaseFactory: Send + Sync {
    /// Opens the backend using trusted local context and validated settings only.
    fn open(
        &self,
        context: &TrustedBackendContext,
        settings: &ValidatedConnectionSettings,
    ) -> Result<Box<dyn ApplicationDatabase>, LifecycleError>;
}

/// Unvalidated backend registration consumed by catalog construction.
pub struct BackendRegistration {
    identifier: Box<str>,
    fields: Vec<ConnectionFieldDeclaration>,
    factory: Box<dyn ApplicationDatabaseFactory>,
}

impl BackendRegistration {
    /// Creates a registration for validation by `BackendCatalog`.
    pub fn new(
        identifier: impl Into<Box<str>>,
        fields: Vec<ConnectionFieldDeclaration>,
        factory: Box<dyn ApplicationDatabaseFactory>,
    ) -> Self {
        Self {
            identifier: identifier.into(),
            fields,
            factory,
        }
    }
}

impl fmt::Debug for BackendRegistration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BackendRegistration")
            .field("field_count", &self.fields.len())
            .finish_non_exhaustive()
    }
}

/// Public validated declaration for one backend.
#[derive(Clone, Eq, PartialEq)]
pub struct BackendDeclaration {
    identifier: BackendIdentifier,
    fields: Box<[ConnectionFieldDeclaration]>,
}

impl BackendDeclaration {
    /// Returns the stable backend identifier.
    pub const fn identifier(&self) -> &BackendIdentifier {
        &self.identifier
    }

    /// Returns fields in canonical identifier order.
    pub const fn fields(&self) -> &[ConnectionFieldDeclaration] {
        &self.fields
    }

    fn validate(
        &self,
        inputs: Vec<ConnectionFieldInput>,
    ) -> Result<ValidatedConnectionSettings, ConnectionValidationError> {
        if inputs.len() > MAX_CONNECTION_FIELDS {
            return Err(ConnectionValidationError::TooManyFields);
        }

        let mut validated = Vec::with_capacity(inputs.len());
        for input in inputs {
            if validated
                .iter()
                .any(|field: &ValidatedConnectionField| field.identifier() == &input.identifier)
            {
                return Err(ConnectionValidationError::DuplicateField);
            }
            let declaration = self
                .fields
                .binary_search_by(|field| field.identifier.cmp(&input.identifier))
                .ok()
                .map(|position| &self.fields[position])
                .ok_or(ConnectionValidationError::UnknownField)?;
            if declaration.value_kind != input.value.kind() {
                return Err(ConnectionValidationError::WrongValueKind);
            }
            if declaration.classification != input.classification {
                return Err(ConnectionValidationError::ClassificationMismatch);
            }
            if input.value.exceeds_bound() {
                return Err(ConnectionValidationError::ValueTooLarge);
            }
            validated.push(ValidatedConnectionField::new(
                input.identifier,
                declaration.classification,
                input.value,
            ));
        }

        for declaration in &self.fields {
            if declaration.requirement == ConnectionFieldRequirement::Required
                && !validated
                    .iter()
                    .any(|field| field.identifier() == declaration.identifier())
            {
                return Err(ConnectionValidationError::MissingRequiredField);
            }
        }
        validated.sort_by(|left, right| left.identifier().cmp(right.identifier()));
        Ok(ValidatedConnectionSettings::new(
            self.identifier.clone(),
            validated,
        ))
    }
}

impl fmt::Debug for BackendDeclaration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BackendDeclaration")
            .field("field_count", &self.fields.len())
            .finish_non_exhaustive()
    }
}

struct BackendEntry {
    declaration: BackendDeclaration,
    factory: Box<dyn ApplicationDatabaseFactory>,
}

/// Validated runtime-supplied catalog of compiled-in database backends.
pub struct BackendCatalog(Box<[BackendEntry]>);

impl BackendCatalog {
    /// Validates registrations and creates the catalog.
    pub fn new(registrations: Vec<BackendRegistration>) -> Result<Self, CatalogError> {
        if registrations.is_empty() {
            return Err(CatalogError::Empty);
        }
        if registrations.len() > MAX_BACKENDS {
            return Err(CatalogError::TooManyBackends);
        }

        let mut entries = Vec::with_capacity(registrations.len());
        for registration in registrations {
            let identifier = BackendIdentifier::new(registration.identifier)
                .map_err(|_| CatalogError::InvalidBackendIdentifier)?;
            if entries
                .iter()
                .any(|entry: &BackendEntry| entry.declaration.identifier == identifier)
            {
                return Err(CatalogError::DuplicateBackendIdentifier);
            }
            if registration.fields.len() > MAX_CONNECTION_FIELDS {
                return Err(CatalogError::TooManyConnectionFields);
            }
            let mut fields = registration.fields;
            fields.sort_by(|left, right| left.identifier.cmp(&right.identifier));
            if fields
                .windows(2)
                .any(|pair| pair[0].identifier == pair[1].identifier)
            {
                return Err(CatalogError::DuplicateConnectionField);
            }
            entries.push(BackendEntry {
                declaration: BackendDeclaration {
                    identifier,
                    fields: fields.into_boxed_slice(),
                },
                factory: registration.factory,
            });
        }
        entries.sort_by(|left, right| {
            left.declaration
                .identifier
                .cmp(&right.declaration.identifier)
        });
        Ok(Self(entries.into_boxed_slice()))
    }

    /// Iterates over validated backend declarations in canonical order.
    pub fn declarations(&self) -> impl ExactSizeIterator<Item = &BackendDeclaration> {
        self.0.iter().map(|entry| &entry.declaration)
    }

    /// Returns one backend declaration by identifier.
    pub fn declaration(&self, identifier: &BackendIdentifier) -> Option<&BackendDeclaration> {
        self.entry(identifier).map(|entry| &entry.declaration)
    }

    /// Validates submitted fields without invoking the backend factory.
    pub fn validate_connection(
        &self,
        identifier: &BackendIdentifier,
        inputs: Vec<ConnectionFieldInput>,
    ) -> Result<ValidatedConnectionSettings, ConnectionValidationError> {
        let entry = self
            .entry(identifier)
            .ok_or(ConnectionValidationError::UnknownBackend)?;
        entry.declaration.validate(inputs)
    }

    /// Validates submitted fields, then invokes the selected backend factory.
    pub fn open(
        &self,
        identifier: &BackendIdentifier,
        context: &TrustedBackendContext,
        inputs: Vec<ConnectionFieldInput>,
    ) -> Result<Box<dyn ApplicationDatabase>, BackendOpenError> {
        let entry = self
            .entry(identifier)
            .ok_or(BackendOpenError::ConnectionInvalid(
                ConnectionValidationError::UnknownBackend,
            ))?;
        let settings = entry
            .declaration
            .validate(inputs)
            .map_err(BackendOpenError::ConnectionInvalid)?;
        entry
            .factory
            .open(context, &settings)
            .map_err(BackendOpenError::Factory)
    }

    fn entry(&self, identifier: &BackendIdentifier) -> Option<&BackendEntry> {
        self.0
            .binary_search_by(|entry| entry.declaration.identifier.cmp(identifier))
            .ok()
            .map(|position| &self.0[position])
    }
}

impl fmt::Debug for BackendCatalog {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BackendCatalog")
            .field("backend_count", &self.0.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declarations_reject_local_reference_fields() {
        for identifier in [
            "path",
            "database-path",
            "credential-file",
            "config-filename",
            "storage-directory",
            "cache-dir",
        ] {
            assert_eq!(
                ConnectionFieldDeclaration::new(
                    identifier,
                    ConnectionValueKind::String,
                    ConnectionFieldRequirement::Optional,
                    SecretClassification::NonSecret,
                )
                .unwrap_err(),
                FieldDeclarationError::LocalReferenceForbidden
            );
        }
    }
}
