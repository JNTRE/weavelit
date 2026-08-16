//! Live authorization composition for the operational request path.
//!
//! [`weavelit_server_authorization`] owns the two decisions and the proofs they
//! produce, but it reads nothing. This module is the Server-side composition
//! that supplies what those decisions are evaluated against, and it is the only
//! place a request path enters them.
//!
//! Three properties are load-bearing here.
//!
//! The order is carried by the types. The entry points take a
//! [`ValidatedSession`], whose constructor is private to [`crate::authentication`],
//! so session validation cannot be skipped; and they return an
//! [`AuthorizedOperation`] or [`AuthorizedAdministration`] proof, whose
//! constructors are private to the authorization crate, so Service Connection
//! selection and provider execution cannot be reached without a decision.
//!
//! Nothing an authorization decision reads is cached. The account's grants and
//! the component enablement are read from the Application Database inside every
//! call, in [`AuthorizationRuntime::live_inputs`], and the values are dropped
//! when the call returns. This runtime holds no grant, no enablement, and no
//! snapshot taken at login, and the session carries none either, so a Group
//! change or a component enablement change takes effect on the very next
//! request without a new session.
//!
//! Every denial is delivered to the System Log before it is returned, and every
//! failure inside delivery is absorbed, so a logging problem can change what is
//! recorded but can never turn a denial into an allow.

use std::sync::Arc;

use weavelit_module_client::AuthorizationRejection;
use weavelit_server_authorization::{
    AdministrationRequest, AuthorizationCatalog, AuthorizationDenied, AuthorizedAdministration,
    AuthorizedOperation, ClientModuleDeclaration, OperationDeclaration, Plane,
    ServiceModuleDeclaration, UserOperationRequest, authorize_administration,
    authorize_user_operation,
};
use weavelit_server_database::{
    ComponentEnablement, ComponentKind, HumanAuthorizationSnapshot, Name, StateIdentifier,
};
use weavelit_server_lifecycle::DatabaseError;
use weavelit_server_log::{ConfiguredLogDestination, TrustedRecordIssuer};
use weavelit_server_log_authority::ServerLogAuthority;
use weavelit_server_observability::ServerObservability;

use crate::{
    authentication::{ValidatedSession, WallClock, random_bytes},
    operational::OperationalDatabase,
};

/// One Client Module this build compiles in and the planes it declares.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServedClientModule {
    /// The Client Module's name.
    pub name: Name,
    /// The planes its authenticated surface declares.
    pub planes: Vec<Plane>,
}

/// One named Operation this build compiles in and the Service Module that owns it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServedOperation {
    /// The Operation's name.
    pub name: Name,
    /// The Service Module that implements it.
    pub service_module: Name,
}

/// The components this build compiles in, without their enablement.
///
/// Which components exist, which planes a Client Module declares, and which
/// Service Module owns an Operation are properties of the build. Whether a
/// component is enabled is not: it is administrator-configured state, so it is
/// deliberately absent here and read from the Application Database on every
/// decision instead.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ServedComponents {
    client_modules: Vec<ServedClientModule>,
    service_modules: Vec<Name>,
    operations: Vec<ServedOperation>,
}

impl ServedComponents {
    /// Declares the components an authorization decision may catalogue.
    #[must_use]
    pub fn new(
        client_modules: Vec<ServedClientModule>,
        service_modules: Vec<Name>,
        operations: Vec<ServedOperation>,
    ) -> Self {
        Self {
            client_modules,
            service_modules,
            operations,
        }
    }

    /// Returns what this build actually compiles in.
    ///
    /// The Web UI is the one Client Module, and it serves both planes. This
    /// build compiles in no Service Module and no named Operation, so every
    /// User Plane request denies by default until one is registered, which is
    /// the same inventory a Restore is judged against.
    #[must_use]
    pub fn compiled_in() -> Self {
        Self::new(
            Name::new(weavelit_module_client_webui::MODULE_IDENTIFIER)
                .into_iter()
                .map(|name| ServedClientModule {
                    name,
                    planes: vec![Plane::User, Plane::Administration],
                })
                .collect(),
            Vec::new(),
            Vec::new(),
        )
    }

    /// Catalogues these components against one live enablement read.
    ///
    /// MFA Module enablement is deliberately not represented: the catalog holds
    /// only what the two decisions evaluate, and neither consults an MFA
    /// Module. A disabled MFA Module therefore cannot deny the Administration
    /// Plane function that re-enables it.
    fn catalog(&self, enablement: &ComponentEnablement) -> Option<AuthorizationCatalog> {
        AuthorizationCatalog::new(
            self.client_modules
                .iter()
                .map(|client_module| {
                    ClientModuleDeclaration::new(
                        client_module.name.clone(),
                        enablement.is_enabled(ComponentKind::ClientModule, &client_module.name),
                        &client_module.planes,
                    )
                })
                .collect(),
            self.service_modules
                .iter()
                .map(|service_module| {
                    ServiceModuleDeclaration::new(
                        service_module.clone(),
                        enablement.is_enabled(ComponentKind::ServiceModule, service_module),
                    )
                })
                .collect(),
            self.operations
                .iter()
                .map(|operation| {
                    OperationDeclaration::new(
                        operation.name.clone(),
                        operation.service_module.clone(),
                        enablement.is_enabled(ComponentKind::Operation, &operation.name),
                    )
                })
                .collect(),
        )
        .ok()
    }
}

/// The Server-side collaborators one authorization decision is made through.
///
/// The fields are the open Application Database, the compiled-in component
/// inventory, the clock, and the System Log. None of them is, or can hold, an
/// account's grants or a component's enablement, because both are read live.
pub struct AuthorizationRuntime {
    database: OperationalDatabase,
    components: ServedComponents,
    clock: WallClock,
    observability: ServerObservability,
    /// The System Log destination denials are recorded to, when one is
    /// configured and could be opened.
    ///
    /// A Server whose System Log cannot be opened still denies correctly; it
    /// records nothing. Delivery never gates the denial.
    system_log: Option<Arc<ConfiguredLogDestination>>,
}

impl AuthorizationRuntime {
    /// Composes authorization over an operational database.
    #[must_use]
    pub fn new(
        database: OperationalDatabase,
        components: ServedComponents,
        clock: WallClock,
        system_log: Option<Arc<ConfiguredLogDestination>>,
    ) -> Self {
        Self {
            database,
            components,
            clock,
            observability: ServerObservability::new(TrustedRecordIssuer::from_server_authority(
                &ServerLogAuthority::new(),
            )),
            system_log,
        }
    }

    /// Authorizes one User Plane Operation for a validated session.
    ///
    /// The account's grants and the component enablement are read from the
    /// Application Database inside this call.
    pub fn authorize_operation(
        &self,
        session: &ValidatedSession,
        client_module: &Name,
        service_module: &Name,
        operation: &Name,
        correlation_id: &str,
    ) -> Result<AuthorizedOperation, AuthorizationRejection> {
        self.decide(
            session,
            client_module,
            correlation_id,
            |account, catalog| {
                authorize_user_operation(
                    account,
                    catalog,
                    UserOperationRequest {
                        client_module,
                        service_module,
                        operation,
                    },
                )
            },
        )
    }

    /// Authorizes one Administration Plane function for a validated session.
    ///
    /// No target component participates, so an administration function that
    /// enables a disabled component is not denied by that component's own
    /// enablement. The Client Module the request arrives through, its
    /// Administration Plane declaration, and the Server Administration
    /// Permission are the whole decision.
    pub fn authorize_administration(
        &self,
        session: &ValidatedSession,
        client_module: &Name,
        correlation_id: &str,
    ) -> Result<AuthorizedAdministration, AuthorizationRejection> {
        self.decide(
            session,
            client_module,
            correlation_id,
            |account, catalog| {
                authorize_administration(account, catalog, AdministrationRequest { client_module })
            },
        )
    }

    /// Runs one decision against live inputs and delivers every denial.
    ///
    /// Delivery is attempted before the denial is returned and absorbs its own
    /// failures, so the System Log cannot change the answer.
    fn decide<T>(
        &self,
        session: &ValidatedSession,
        client_module: &Name,
        correlation_id: &str,
        evaluate: impl FnOnce(
            &HumanAuthorizationSnapshot,
            &AuthorizationCatalog,
        ) -> Result<T, AuthorizationDenied>,
    ) -> Result<T, AuthorizationRejection> {
        match self.evaluate(session, client_module, evaluate) {
            Ok(authorized) => Ok(authorized),
            Err(AuthorizationRejection) => {
                self.record_denial(correlation_id);
                Err(AuthorizationRejection)
            }
        }
    }

    /// Decides one request, reporting the single denial for every failure.
    ///
    /// A read that could not run, an account the database no longer holds, and
    /// an inventory that could not be catalogued are denials rather than
    /// reported failures, so an unavailable input closes access instead of
    /// opening it and discloses nothing more than a missing grant does.
    fn evaluate<T>(
        &self,
        session: &ValidatedSession,
        client_module: &Name,
        evaluate: impl FnOnce(
            &HumanAuthorizationSnapshot,
            &AuthorizationCatalog,
        ) -> Result<T, AuthorizationDenied>,
    ) -> Result<T, AuthorizationRejection> {
        // A request that names a Client Module other than the one the session
        // was established for is denied: the session authorizes through the
        // surface it was issued to and no other.
        if session.client_module() != client_module {
            return Err(AuthorizationRejection);
        }

        let (account, catalog) = self.live_inputs(session.account())?;
        evaluate(&account, &catalog).map_err(|AuthorizationDenied| AuthorizationRejection)
    }

    /// Reads everything one decision is evaluated against, on every call.
    ///
    /// Both reads happen here, against the Application Database, inside the
    /// single decision that uses them, and both results are returned by value
    /// and dropped when that decision returns. Nothing derived from them is
    /// stored on this runtime, on the session, or anywhere else that outlives
    /// the call, so a revoked grant or a disabled component denies the very
    /// next request.
    ///
    /// The two reads share one acquisition of the database lane, so a decision
    /// cannot mix a grant set read before an administrator's change with an
    /// enablement read after it.
    fn live_inputs(
        &self,
        account: StateIdentifier,
    ) -> Result<(HumanAuthorizationSnapshot, AuthorizationCatalog), AuthorizationRejection> {
        let (snapshot, enablement) = self
            .database
            .with(|database| {
                let snapshot = database.load_human_authorization(account)?;
                let enablement = database.load_component_enablement()?;
                Ok((snapshot, enablement))
            })
            .map_err(|_| AuthorizationRejection)?
            .map_err(|_: DatabaseError| AuthorizationRejection)?;

        let account = snapshot.ok_or(AuthorizationRejection)?;
        let catalog = self
            .components
            .catalog(&enablement)
            .ok_or(AuthorizationRejection)?;

        Ok((account, catalog))
    }

    /// Attempts to record one authorization denial in the System Log.
    ///
    /// Every failure is absorbed: an unconfigured destination, an unreadable
    /// clock, a randomness failure, and a delivery failure all leave the denial
    /// exactly as it was.
    fn record_denial(&self, correlation_id: &str) {
        let Some(destination) = self.system_log.as_ref() else {
            return;
        };
        let (Some(entropy), Some(event_time)) = (random_bytes::<16>(), (self.clock)()) else {
            return;
        };
        let Ok(identifier) = StateIdentifier::from_bytes(entropy) else {
            return;
        };
        let Ok(record) =
            self.observability
                .prepare_authorization_denial(identifier, event_time, correlation_id)
        else {
            return;
        };
        // A delivery failure is absorbed here and nowhere else, so it cannot
        // reach the denial the caller is about to return.
        let _ = destination.deliver(&record);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        fs,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
        sync::Mutex,
    };

    use axum::http::StatusCode;
    use rusqlite::Connection;
    use weavelit_module_client::typed_json::TypedJsonEnvelope;
    use weavelit_server_authentication::{
        Argon2Engine, Argon2Profile, AuthenticationError, SessionSecrets,
    };
    use weavelit_server_database::{
        Account, Group, GroupGrant, GroupGrantRecord, GroupMembership, NewSession, SessionCsrfHash,
        SessionInstant, SessionTokenHash,
    };

    use super::*;
    use crate::{
        APPLICATION_DATABASE_FILE, RestrictedStartup, StartupOutcome,
        authentication::{
            AuthenticationRuntime,
            tests::{DeliveredRecord, recording_log},
        },
        classify_restricted_startup,
        tests::{SealedStateParts, seal_deployment_with, sealed_application_state_from},
    };

    const CLIENT_MODULE: &str = "web-ui";
    const OTHER_CLIENT_MODULE: &str = "mcp";
    const SERVICE_MODULE: &str = "zendesk";
    const OPERATION: &str = "zendesk.ticket.create";
    const MFA_MODULE: &str = "totp";
    const CORRELATION: &str = "0123456789abcdef";
    const DENIAL_CLASSIFICATION: &str = "authorization.denial";
    const DENIAL_DETAIL: &str = "request authorization denied";

    /// The instant every seeded session is issued at and decided against.
    const ISSUED_AT: i64 = 1_700_000_000_000;

    const OPERATOR_BYTES: [u8; 16] = [0xa1; 16];
    const ADMINISTRATOR_BYTES: [u8; 16] = [0xa2; 16];
    const OPERATORS_GROUP_BYTES: [u8; 16] = [0x70; 16];
    const ADMINISTRATORS_GROUP_BYTES: [u8; 16] = [0x30; 16];

    fn name(value: &str) -> Name {
        Name::new(value).expect("the test name must be accepted")
    }

    fn identifier(bytes: [u8; 16]) -> StateIdentifier {
        StateIdentifier::from_bytes(bytes).expect("the test identifier must be accepted")
    }

    /// A verification engine no authorization test reaches.
    ///
    /// Authorization never verifies a password: these tests establish a session
    /// through the session store directly, so this engine exists only to
    /// compose the runtime that validates one.
    #[derive(Clone, Copy, Debug)]
    struct UnusedEngine;

    impl Argon2Engine for UnusedEngine {
        fn verify(&self, _password: &[u8], _profile: &Argon2Profile, _encoded: &str) -> bool {
            unreachable!("an authorization test never verifies a password")
        }

        fn hash(
            &self,
            _password: &[u8],
            _profile: &Argon2Profile,
            _salt: &[u8],
        ) -> Result<String, AuthenticationError> {
            unreachable!("an authorization test never hashes a password")
        }
    }

    /// A live authorization surface over a real sealed deployment.
    ///
    /// The deployment, the Application Database, the session store, and both
    /// decisions are real. Only the clock and the System Log destination are
    /// injected. Every state change a test makes is written straight into the
    /// Application Database, as an administrator's change would be, so nothing
    /// re-reads state through the path under test.
    struct AuthorizationSurface {
        /// Held so the Application Database and its state-root lock stay open.
        /// Dropped before the temporary state root it lives in.
        _startup: RestrictedStartup,
        _root: tempfile::TempDir,
        database_path: PathBuf,
        database: OperationalDatabase,
        authentication: Arc<AuthenticationRuntime<UnusedEngine>>,
        runtime: AuthorizationRuntime,
        delivered: Arc<Mutex<Vec<DeliveredRecord>>>,
    }

    impl AuthorizationSurface {
        fn new() -> Self {
            let root = tempfile::tempdir().expect("the test state root must be created");
            fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700))
                .expect("the test state root must be private");
            let state_root = root
                .path()
                .canonicalize()
                .expect("the test state root must resolve");
            seal_deployment_with(&state_root, &authorization_state());

            let startup = classify_restricted_startup(&state_root)
                .expect("a sealed state root must classify");
            assert_eq!(startup.outcome(), StartupOutcome::Initialized);
            let database = startup
                .application_database()
                .expect("a sealed startup hands over its Application Database")
                .clone();
            let clock: WallClock = Arc::new(|| Some(ISSUED_AT));
            let (system_log, delivered) = recording_log(false);
            let authentication = AuthenticationRuntime::with_engine(
                UnusedEngine,
                database.clone(),
                startup
                    .initialized_state()
                    .expect("a sealed startup hands over its loaded state"),
                BTreeSet::from([name(CLIENT_MODULE)]),
                crate::authentication::AuthenticationClocks {
                    wall: Arc::clone(&clock),
                    elapsed: crate::authentication::monotonic_clock(),
                },
                None,
                startup.protection(),
            )
            .expect("the authentication runtime must compose");
            let runtime = AuthorizationRuntime::new(
                database.clone(),
                served_components(),
                clock,
                Some(system_log),
            );

            Self {
                _startup: startup,
                _root: root,
                database_path: state_root.join(APPLICATION_DATABASE_FILE),
                database,
                authentication,
                runtime,
                delivered,
            }
        }

        /// Establishes one live session and validates it exactly once.
        ///
        /// Every test then reuses the returned value across its state change,
        /// so a denial after that change is reached with no re-login and no new
        /// session.
        fn session(&self, account: StateIdentifier, client_module: &str) -> ValidatedSession {
            let secrets = SessionSecrets::generate().expect("the test session must be generated");
            let (session_digest, csrf_digest) = secrets.digests();
            let stored = NewSession::new(
                SessionTokenHash::from_bytes(*session_digest.as_bytes())
                    .expect("the session digest must be accepted"),
                SessionCsrfHash::from_bytes(*csrf_digest.as_bytes())
                    .expect("the csrf digest must be accepted"),
                account,
                name(client_module),
                SessionInstant::from_unix_milliseconds(ISSUED_AT)
                    .expect("the test instant must be accepted"),
            );
            self.database
                .with(|database| {
                    database
                        .sessions()
                        .expect("the sqlite backend serves a session store")
                        .create(&stored)
                })
                .expect("the database lane must be usable")
                .expect("the session must be stored");

            self.authentication
                .validated_session(secrets.session().as_str(), secrets.csrf().as_str())
                .expect("the freshly stored session must validate")
        }

        fn authorize_operation(
            &self,
            session: &ValidatedSession,
        ) -> Result<AuthorizedOperation, AuthorizationRejection> {
            self.runtime.authorize_operation(
                session,
                &name(CLIENT_MODULE),
                &name(SERVICE_MODULE),
                &name(OPERATION),
                CORRELATION,
            )
        }

        fn authorize_administration(
            &self,
            session: &ValidatedSession,
        ) -> Result<AuthorizedAdministration, AuthorizationRejection> {
            self.runtime
                .authorize_administration(session, &name(CLIENT_MODULE), CORRELATION)
        }

        /// Reads the enablement the next decision will be evaluated against.
        fn enablement(&self) -> ComponentEnablement {
            self.database
                .with(|database| database.load_component_enablement())
                .expect("the database lane must be usable")
                .expect("the enablement projection must be readable")
        }

        fn delivered(&self) -> Vec<DeliveredRecord> {
            self.delivered
                .lock()
                .expect("the delivered record log must not poison")
                .clone()
        }
    }

    /// The components the fixture deployment serves.
    ///
    /// This build compiles in no Service Module and no named Operation, so the
    /// inventory a decision is catalogued against is supplied here rather than
    /// taken from [`ServedComponents::compiled_in`], which would deny every
    /// User Plane request for want of a catalogued Operation.
    fn served_components() -> ServedComponents {
        ServedComponents::new(
            vec![ServedClientModule {
                name: name(CLIENT_MODULE),
                planes: vec![Plane::User, Plane::Administration],
            }],
            vec![name(SERVICE_MODULE)],
            vec![ServedOperation {
                name: name(OPERATION),
                service_module: name(SERVICE_MODULE),
            }],
        )
    }

    /// The sealed application state every authorization test decides against.
    ///
    /// The operator reaches the Client Module, the Service Module, and the one
    /// Operation. The administrator reaches the Client Module and holds the
    /// Server Administration Permission and nothing else, exactly like the
    /// Administrators Group created during Init.
    fn authorization_state() -> weavelit_server_lifecycle::ApplicationState {
        sealed_application_state_from(SealedStateParts {
            accounts: vec![
                Account {
                    identifier: identifier(OPERATOR_BYTES),
                    username: name("operator"),
                    display_name: None,
                    active: true,
                    mfa_required: false,
                },
                Account {
                    identifier: identifier(ADMINISTRATOR_BYTES),
                    username: name("administrator"),
                    display_name: None,
                    active: true,
                    mfa_required: false,
                },
            ],
            groups: vec![
                Group {
                    identifier: identifier(OPERATORS_GROUP_BYTES),
                    name: name("Ticket Operators"),
                    description: None,
                },
                Group {
                    identifier: identifier(ADMINISTRATORS_GROUP_BYTES),
                    name: name("Administrators"),
                    description: None,
                },
            ],
            group_memberships: vec![
                GroupMembership {
                    group: identifier(OPERATORS_GROUP_BYTES),
                    account: identifier(OPERATOR_BYTES),
                },
                GroupMembership {
                    group: identifier(ADMINISTRATORS_GROUP_BYTES),
                    account: identifier(ADMINISTRATOR_BYTES),
                },
            ],
            group_grants: vec![
                GroupGrantRecord {
                    group: identifier(OPERATORS_GROUP_BYTES),
                    grant: GroupGrant::ClientModule(name(CLIENT_MODULE)),
                },
                GroupGrantRecord {
                    group: identifier(OPERATORS_GROUP_BYTES),
                    grant: GroupGrant::ServiceModule(name(SERVICE_MODULE)),
                },
                GroupGrantRecord {
                    group: identifier(OPERATORS_GROUP_BYTES),
                    grant: GroupGrant::Operation(name(OPERATION)),
                },
                GroupGrantRecord {
                    group: identifier(ADMINISTRATORS_GROUP_BYTES),
                    grant: GroupGrant::ClientModule(name(CLIENT_MODULE)),
                },
                GroupGrantRecord {
                    group: identifier(ADMINISTRATORS_GROUP_BYTES),
                    grant: GroupGrant::ServerAdministration,
                },
            ],
            ..SealedStateParts::default()
        })
    }

    /// Removes one Group grant directly, as an administrator's Group change
    /// would, without reopening or reloading the Application Database.
    fn revoke_grant(path: &Path, group: [u8; 16], kind: &str, value: &str) {
        let removed = Connection::open(path)
            .expect("the test connection must open")
            .execute(
                "DELETE FROM weavelit_group_grant \
                 WHERE group_id = ?1 AND grant_kind = ?2 AND grant_value = ?3",
                rusqlite::params![group.as_slice(), kind, value],
            )
            .expect("the grant removal must run");

        assert_eq!(removed, 1, "the seeded grant must have been removed");
    }

    /// Disables one component directly, as an administrator's enablement change
    /// would, without reopening or reloading the Application Database.
    fn disable_component(path: &Path, kind: ComponentKind, component: &str) {
        Connection::open(path)
            .expect("the test connection must open")
            .execute(
                "INSERT OR REPLACE INTO weavelit_configuration \
                 (component, setting_key, setting_value) VALUES (?1, ?2, 'false')",
                rusqlite::params![component, kind.enablement_key()],
            )
            .expect("the enablement change must run");
    }

    #[test]
    fn an_authorized_operation_names_what_the_live_decision_allowed() {
        let surface = AuthorizationSurface::new();
        let session = surface.session(identifier(OPERATOR_BYTES), CLIENT_MODULE);

        let authorized = surface
            .authorize_operation(&session)
            .expect("the fully granted request must be authorized");

        // Asserted through the accessors, because the bounded name types redact
        // their contents in `Debug`: a scan of a rendering could never match.
        assert_eq!(authorized.client_module().as_str(), CLIENT_MODULE);
        assert_eq!(authorized.service_module().as_str(), SERVICE_MODULE);
        assert_eq!(authorized.operation().as_str(), OPERATION);
        // An allowed request records no denial.
        assert_eq!(surface.delivered(), Vec::new());
    }

    #[test]
    fn a_revoked_group_grant_denies_the_very_next_request() {
        let surface = AuthorizationSurface::new();
        let session = surface.session(identifier(OPERATOR_BYTES), CLIENT_MODULE);
        assert!(surface.authorize_operation(&session).is_ok());

        revoke_grant(
            &surface.database_path,
            OPERATORS_GROUP_BYTES,
            "operation",
            OPERATION,
        );

        // The same session value, never revalidated against a new login.
        assert_eq!(
            surface.authorize_operation(&session),
            Err(AuthorizationRejection)
        );
    }

    #[test]
    fn a_disabled_client_module_denies_the_very_next_request() {
        let surface = AuthorizationSurface::new();
        let session = surface.session(identifier(OPERATOR_BYTES), CLIENT_MODULE);
        assert!(surface.authorize_operation(&session).is_ok());

        disable_component(
            &surface.database_path,
            ComponentKind::ClientModule,
            CLIENT_MODULE,
        );

        assert_eq!(
            surface.authorize_operation(&session),
            Err(AuthorizationRejection)
        );
    }

    #[test]
    fn a_disabled_service_module_denies_the_very_next_request() {
        let surface = AuthorizationSurface::new();
        let session = surface.session(identifier(OPERATOR_BYTES), CLIENT_MODULE);
        assert!(surface.authorize_operation(&session).is_ok());

        disable_component(
            &surface.database_path,
            ComponentKind::ServiceModule,
            SERVICE_MODULE,
        );

        assert_eq!(
            surface.authorize_operation(&session),
            Err(AuthorizationRejection)
        );
    }

    #[test]
    fn a_disabled_operation_denies_the_very_next_request() {
        let surface = AuthorizationSurface::new();
        let session = surface.session(identifier(OPERATOR_BYTES), CLIENT_MODULE);
        assert!(surface.authorize_operation(&session).is_ok());

        disable_component(&surface.database_path, ComponentKind::Operation, OPERATION);

        assert_eq!(
            surface.authorize_operation(&session),
            Err(AuthorizationRejection)
        );
    }

    #[test]
    fn a_session_established_for_another_client_module_is_denied() {
        let surface = AuthorizationSurface::new();
        let session = surface.session(identifier(OPERATOR_BYTES), OTHER_CLIENT_MODULE);

        assert_eq!(
            surface.authorize_operation(&session),
            Err(AuthorizationRejection)
        );
    }

    #[test]
    fn a_denial_records_the_system_log_record_and_renders_the_forbidden_response() {
        let surface = AuthorizationSurface::new();
        let session = surface.session(identifier(OPERATOR_BYTES), CLIENT_MODULE);
        disable_component(&surface.database_path, ComponentKind::Operation, OPERATION);

        let denial = surface
            .authorize_operation(&session)
            .expect_err("a disabled Operation must deny");
        let response = denial.response(CORRELATION);

        assert_eq!(
            surface.delivered(),
            vec![DeliveredRecord {
                correlation_id: CORRELATION.to_owned(),
                classification: DENIAL_CLASSIFICATION.to_owned(),
                detail: DENIAL_DETAIL.to_owned(),
            }]
        );
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            response
                .extensions()
                .get::<TypedJsonEnvelope>()
                .map(TypedJsonEnvelope::serialize),
            Some(
                format!(
                    "{{\"error\":\"authorization_denied\",\"correlation_id\":\"{CORRELATION}\"}}"
                )
                .into()
            )
        );
    }

    #[test]
    fn a_disabled_mfa_module_does_not_block_the_administration_function_that_re_enables_it() {
        let surface = AuthorizationSurface::new();
        let administrator = surface.session(identifier(ADMINISTRATOR_BYTES), CLIENT_MODULE);
        let operator = surface.session(identifier(OPERATOR_BYTES), CLIENT_MODULE);

        disable_component(&surface.database_path, ComponentKind::MfaModule, MFA_MODULE);

        // The disable is genuinely in force for the very read the decision
        // below makes, so the test cannot pass because it never took effect.
        assert!(
            !surface
                .enablement()
                .is_enabled(ComponentKind::MfaModule, &name(MFA_MODULE))
        );
        // The administration function that re-enables the MFA Module stays
        // reachable, so a disabled MFA Module cannot be permanently disabled.
        assert_eq!(
            surface
                .authorize_administration(&administrator)
                .expect("an administrator must still reach the Administration Plane")
                .client_module()
                .as_str(),
            CLIENT_MODULE
        );
        // The exception is not a hole: it admits an administrator and no one
        // else.
        assert_eq!(
            surface.authorize_administration(&operator),
            Err(AuthorizationRejection)
        );
    }
}
