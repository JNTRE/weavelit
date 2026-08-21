use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use weavelit_server_lifecycle::{
    ApplicationDatabase, ApplicationDatabaseFactory, ApplicationState, BackendCatalog,
    BackendIdentifier, BackendOpenError, BackendRegistration, CatalogError,
    ConnectionFieldDeclaration, ConnectionFieldIdentifier, ConnectionFieldInput,
    ConnectionFieldRequirement, ConnectionValidationError, ConnectionValue, ConnectionValueKind,
    DatabaseError, DatabaseInspection, DeploymentIdentifier, InitializedState, LifecycleError,
    RetainedDatabaseInspection, SecretClassification, StateIdentifier, TrustedBackendContext,
    ValidatedConnectionSettings, WorkflowCheckpoint,
};

const SENSITIVE_PATH: &str = "/private/sensitive/application.sqlite3";
const SENSITIVE_SECRET: &str = "sensitive-credential-value";

struct FakeDatabase;

impl ApplicationDatabase for FakeDatabase {
    fn inspect(
        &mut self,
        _expected_deployment_identifier: DeploymentIdentifier,
    ) -> Result<DatabaseInspection, DatabaseError> {
        Ok(DatabaseInspection::Uninitialized)
    }

    fn create_checkpoint(&mut self, _checkpoint: &WorkflowCheckpoint) -> Result<(), DatabaseError> {
        Ok(())
    }

    fn complete_checkpoint(
        &mut self,
        _public_identity_persistence: &weavelit_server_database::AccountPublicIdentifierPersistence,
        _checkpoint: &WorkflowCheckpoint,
        _state: &ApplicationState,
        _reconciliation: &weavelit_server_database::ReconciliationDigest,
    ) -> Result<(), DatabaseError> {
        Err(DatabaseError::InvalidState)
    }

    fn load_initialized_state(
        &mut self,
        _public_identity_persistence: &weavelit_server_database::AccountPublicIdentifierPersistence,
        _audit_reference_persistence: &weavelit_server_database::AuditReferencePersistence,
        _expected_deployment_identifier: DeploymentIdentifier,
    ) -> Result<InitializedState, DatabaseError> {
        Err(DatabaseError::NotInitialized)
    }

    fn load_account_public_identity(
        &mut self,
        _persistence: &weavelit_server_database::AccountPublicIdentifierPersistence,
        _public_identifier: weavelit_server_database::AccountPublicIdentifier,
    ) -> Result<Option<weavelit_server_database::AccountPublicIdentity>, DatabaseError> {
        Err(DatabaseError::NotInitialized)
    }

    fn acknowledge_completion(
        &mut self,
        _expected_deployment_identifier: DeploymentIdentifier,
        _record_identifier: StateIdentifier,
    ) -> Result<(), DatabaseError> {
        Err(DatabaseError::NotInitialized)
    }

    fn load_human_authorization(
        &mut self,
        _account: StateIdentifier,
    ) -> Result<Option<weavelit_server_database::HumanAuthorizationSnapshot>, DatabaseError> {
        Err(DatabaseError::NotInitialized)
    }

    fn load_account_audit_reference(
        &mut self,
        _persistence: &weavelit_server_database::AuditReferencePersistence,
        _account: StateIdentifier,
    ) -> Result<Option<weavelit_server_database::AccountAuditReference>, DatabaseError> {
        Err(DatabaseError::NotInitialized)
    }

    fn load_group_audit_reference(
        &mut self,
        _persistence: &weavelit_server_database::AuditReferencePersistence,
        _group: StateIdentifier,
    ) -> Result<Option<weavelit_server_database::GroupAuditReference>, DatabaseError> {
        Err(DatabaseError::NotInitialized)
    }

    fn load_log_configuration_audit_reference(
        &mut self,
        _persistence: &weavelit_server_database::AuditReferencePersistence,
        _configuration: StateIdentifier,
    ) -> Result<Option<weavelit_server_database::LogConfigurationAuditReference>, DatabaseError>
    {
        Err(DatabaseError::NotInitialized)
    }

    fn load_component_enablement(
        &mut self,
    ) -> Result<weavelit_server_database::ComponentEnablement, DatabaseError> {
        Err(DatabaseError::NotInitialized)
    }

    fn sessions(&mut self) -> Option<&mut dyn weavelit_server_database::SessionStore> {
        None
    }

    fn mfa(&mut self) -> Option<&mut dyn weavelit_server_database::MfaStore> {
        None
    }

    fn reconciliation(&mut self) -> Option<&mut dyn weavelit_server_database::ReconciliationStore> {
        None
    }

    fn audit_terminal_recovery(
        &mut self,
    ) -> Option<&mut dyn weavelit_server_database::AuditTerminalRecoveryStore> {
        None
    }

    fn close(self: Box<Self>) -> Result<(), DatabaseError> {
        Ok(())
    }
}

struct FakeFactory {
    calls: Arc<AtomicUsize>,
    result: Result<(), LifecycleError>,
}

impl ApplicationDatabaseFactory for FakeFactory {
    fn open(
        &self,
        context: &TrustedBackendContext,
        settings: &ValidatedConnectionSettings,
    ) -> Result<Box<dyn ApplicationDatabase>, LifecycleError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(
            context.application_database_path(),
            Path::new(SENSITIVE_PATH)
        );
        let credential = ConnectionFieldIdentifier::new("credential").unwrap();
        assert_eq!(
            settings
                .get(&credential)
                .and_then(|field| field.value().as_str()),
            Some(SENSITIVE_SECRET)
        );
        self.result?;
        Ok(Box::new(FakeDatabase))
    }

    fn inspect_retained(
        &self,
        context: &TrustedBackendContext,
        settings: &ValidatedConnectionSettings,
        _expected_deployment_identifier: DeploymentIdentifier,
    ) -> Result<RetainedDatabaseInspection, LifecycleError> {
        self.open(context, settings)?;
        Ok(RetainedDatabaseInspection::Inspected(
            DatabaseInspection::Uninitialized,
        ))
    }
}

fn required_field(
    identifier: &str,
    value_kind: ConnectionValueKind,
    classification: SecretClassification,
) -> ConnectionFieldDeclaration {
    ConnectionFieldDeclaration::new(
        identifier,
        value_kind,
        ConnectionFieldRequirement::Required,
        classification,
    )
    .unwrap()
}

fn catalog(calls: Arc<AtomicUsize>, result: Result<(), LifecycleError>) -> BackendCatalog {
    BackendCatalog::new(vec![BackendRegistration::new(
        "remote-postgres",
        vec![
            required_field(
                "credential",
                ConnectionValueKind::String,
                SecretClassification::Secret,
            ),
            required_field(
                "port",
                ConnectionValueKind::Integer,
                SecretClassification::NonSecret,
            ),
        ],
        Box::new(FakeFactory { calls, result }),
    )])
    .unwrap()
}

fn valid_inputs() -> Vec<ConnectionFieldInput> {
    vec![
        ConnectionFieldInput::new(
            ConnectionFieldIdentifier::new("port").unwrap(),
            SecretClassification::NonSecret,
            ConnectionValue::integer(5432),
        ),
        ConnectionFieldInput::new(
            ConnectionFieldIdentifier::new("credential").unwrap(),
            SecretClassification::Secret,
            ConnectionValue::string(SENSITIVE_SECRET),
        ),
    ]
}

fn expect_open_error(
    result: Result<Box<dyn ApplicationDatabase>, BackendOpenError>,
) -> BackendOpenError {
    match result {
        Ok(_) => panic!("backend open unexpectedly succeeded"),
        Err(error) => error,
    }
}

#[test]
fn valid_inputs_are_canonicalized_before_factory_invocation() {
    let calls = Arc::new(AtomicUsize::new(0));
    let catalog = catalog(Arc::clone(&calls), Ok(()));
    let backend = BackendIdentifier::new("remote-postgres").unwrap();
    let context = TrustedBackendContext::new(PathBuf::from(SENSITIVE_PATH));

    let declaration = catalog.declaration(&backend).unwrap();
    assert_eq!(declaration.fields()[0].identifier().as_str(), "credential");
    assert_eq!(declaration.fields()[1].identifier().as_str(), "port");

    let mut database = catalog.open(&backend, &context, valid_inputs()).unwrap();
    let deployment_identifier = DeploymentIdentifier::from_bytes([1; 16]).unwrap();
    assert_eq!(
        database.inspect(deployment_identifier).unwrap(),
        DatabaseInspection::Uninitialized
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn catalog_rejects_empty_invalid_and_duplicate_backend_identifiers() {
    assert_eq!(
        BackendCatalog::new(vec![]).unwrap_err(),
        CatalogError::Empty
    );

    for invalid in ["", "SQLite", "sqlite--local"] {
        let error = BackendCatalog::new(vec![BackendRegistration::new(
            invalid,
            vec![],
            Box::new(FakeFactory {
                calls: Arc::new(AtomicUsize::new(0)),
                result: Ok(()),
            }),
        )])
        .unwrap_err();
        assert_eq!(error, CatalogError::InvalidBackendIdentifier);
    }

    let registrations = ["sqlite", "sqlite"].map(|identifier| {
        BackendRegistration::new(
            identifier,
            vec![],
            Box::new(FakeFactory {
                calls: Arc::new(AtomicUsize::new(0)),
                result: Ok(()),
            }) as Box<dyn ApplicationDatabaseFactory>,
        )
    });
    assert_eq!(
        BackendCatalog::new(Vec::from(registrations)).unwrap_err(),
        CatalogError::DuplicateBackendIdentifier
    );
}

#[test]
fn catalog_rejects_duplicate_connection_fields() {
    let duplicate = required_field(
        "host",
        ConnectionValueKind::String,
        SecretClassification::NonSecret,
    );
    let registration = BackendRegistration::new(
        "remote-postgres",
        vec![duplicate.clone(), duplicate],
        Box::new(FakeFactory {
            calls: Arc::new(AtomicUsize::new(0)),
            result: Ok(()),
        }),
    );

    assert_eq!(
        BackendCatalog::new(vec![registration]).unwrap_err(),
        CatalogError::DuplicateConnectionField
    );
}

#[test]
fn invalid_inputs_never_invoke_the_factory() {
    let mut too_many = Vec::new();
    for _ in 0..65 {
        too_many.push(valid_inputs()[0].clone());
    }
    let cases = vec![
        (too_many, ConnectionValidationError::TooManyFields),
        (
            vec![ConnectionFieldInput::new(
                ConnectionFieldIdentifier::new("unknown").unwrap(),
                SecretClassification::NonSecret,
                ConnectionValue::string("value"),
            )],
            ConnectionValidationError::UnknownField,
        ),
        (
            vec![valid_inputs()[0].clone(), valid_inputs()[0].clone()],
            ConnectionValidationError::DuplicateField,
        ),
        (
            vec![valid_inputs()[0].clone()],
            ConnectionValidationError::MissingRequiredField,
        ),
        (
            vec![
                valid_inputs()[0].clone(),
                ConnectionFieldInput::new(
                    ConnectionFieldIdentifier::new("credential").unwrap(),
                    SecretClassification::Secret,
                    ConnectionValue::boolean(true),
                ),
            ],
            ConnectionValidationError::WrongValueKind,
        ),
        (
            vec![
                valid_inputs()[0].clone(),
                ConnectionFieldInput::new(
                    ConnectionFieldIdentifier::new("credential").unwrap(),
                    SecretClassification::NonSecret,
                    ConnectionValue::string(SENSITIVE_SECRET),
                ),
            ],
            ConnectionValidationError::ClassificationMismatch,
        ),
        (
            vec![
                valid_inputs()[0].clone(),
                ConnectionFieldInput::new(
                    ConnectionFieldIdentifier::new("credential").unwrap(),
                    SecretClassification::Secret,
                    ConnectionValue::string("x".repeat(16 * 1024 + 1)),
                ),
            ],
            ConnectionValidationError::ValueTooLarge,
        ),
    ];

    for (inputs, expected) in cases {
        let calls = Arc::new(AtomicUsize::new(0));
        let catalog = catalog(Arc::clone(&calls), Ok(()));
        let backend = BackendIdentifier::new("remote-postgres").unwrap();
        let context = TrustedBackendContext::new(PathBuf::from(SENSITIVE_PATH));

        assert_eq!(
            expect_open_error(catalog.open(&backend, &context, inputs)),
            BackendOpenError::ConnectionInvalid(expected)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn unknown_backend_never_invokes_a_factory() {
    let calls = Arc::new(AtomicUsize::new(0));
    let catalog = catalog(Arc::clone(&calls), Ok(()));
    let unknown = BackendIdentifier::new("unknown").unwrap();
    let context = TrustedBackendContext::new(PathBuf::from(SENSITIVE_PATH));

    assert_eq!(
        expect_open_error(catalog.open(&unknown, &context, valid_inputs())),
        BackendOpenError::ConnectionInvalid(ConnectionValidationError::UnknownBackend)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn validation_accepts_every_scalar_kind_and_optional_omission() {
    let fields = vec![
        required_field(
            "boolean-value",
            ConnectionValueKind::Boolean,
            SecretClassification::NonSecret,
        ),
        required_field(
            "byte-value",
            ConnectionValueKind::Bytes,
            SecretClassification::Secret,
        ),
        required_field(
            "integer-value",
            ConnectionValueKind::Integer,
            SecretClassification::NonSecret,
        ),
        required_field(
            "string-value",
            ConnectionValueKind::String,
            SecretClassification::NonSecret,
        ),
        ConnectionFieldDeclaration::new(
            "optional-value",
            ConnectionValueKind::String,
            ConnectionFieldRequirement::Optional,
            SecretClassification::NonSecret,
        )
        .unwrap(),
    ];
    let catalog = BackendCatalog::new(vec![BackendRegistration::new(
        "all-types",
        fields,
        Box::new(FakeFactory {
            calls: Arc::new(AtomicUsize::new(0)),
            result: Ok(()),
        }),
    )])
    .unwrap();
    let backend = BackendIdentifier::new("all-types").unwrap();
    let settings = catalog
        .validate_connection(
            &backend,
            vec![
                ConnectionFieldInput::new(
                    ConnectionFieldIdentifier::new("string-value").unwrap(),
                    SecretClassification::NonSecret,
                    ConnectionValue::string("text"),
                ),
                ConnectionFieldInput::new(
                    ConnectionFieldIdentifier::new("integer-value").unwrap(),
                    SecretClassification::NonSecret,
                    ConnectionValue::integer(7),
                ),
                ConnectionFieldInput::new(
                    ConnectionFieldIdentifier::new("byte-value").unwrap(),
                    SecretClassification::Secret,
                    ConnectionValue::bytes([1_u8, 2, 3]),
                ),
                ConnectionFieldInput::new(
                    ConnectionFieldIdentifier::new("boolean-value").unwrap(),
                    SecretClassification::NonSecret,
                    ConnectionValue::boolean(true),
                ),
            ],
        )
        .unwrap();

    assert_eq!(settings.backend_identifier(), &backend);
    assert_eq!(settings.len(), 4);
    assert_eq!(
        settings
            .get(&ConnectionFieldIdentifier::new("boolean-value").unwrap())
            .and_then(|field| field.value().as_boolean()),
        Some(true)
    );
    assert_eq!(
        settings
            .get(&ConnectionFieldIdentifier::new("byte-value").unwrap())
            .and_then(|field| field.value().as_bytes()),
        Some([1_u8, 2, 3].as_slice())
    );
}

#[test]
fn factory_failures_use_payload_free_lifecycle_categories() {
    let calls = Arc::new(AtomicUsize::new(0));
    let catalog = catalog(
        Arc::clone(&calls),
        Err(LifecycleError::DependencyUnavailable),
    );
    let backend = BackendIdentifier::new("remote-postgres").unwrap();
    let context = TrustedBackendContext::new(PathBuf::from(SENSITIVE_PATH));

    let error = expect_open_error(catalog.open(&backend, &context, valid_inputs()));
    assert_eq!(
        error,
        BackendOpenError::Factory(LifecycleError::DependencyUnavailable)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(error.to_string(), "lifecycle dependency is unavailable");
}

#[test]
fn public_debug_and_error_output_redacts_sensitive_values_and_internals() {
    let calls = Arc::new(AtomicUsize::new(0));
    let catalog = catalog(calls, Err(LifecycleError::ConfigurationInvalid));
    let backend = BackendIdentifier::new("remote-postgres").unwrap();
    let context = TrustedBackendContext::new(PathBuf::from(SENSITIVE_PATH));
    let input = valid_inputs().pop().unwrap();

    for output in [
        format!("{catalog:?}"),
        format!("{backend:?}"),
        format!("{context:?}"),
        format!("{input:?}"),
        LifecycleError::ConfigurationInvalid.to_string(),
    ] {
        assert!(!output.contains(SENSITIVE_PATH));
        assert!(!output.contains(SENSITIVE_SECRET));
        assert!(!output.contains("remote-postgres"));
        assert!(!output.contains("credential"));
        assert!(!output.contains("FakeFactory"));
    }
}
