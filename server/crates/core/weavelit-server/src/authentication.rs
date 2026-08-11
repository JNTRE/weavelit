//! Server-owned local authentication for the operational surface.
//!
//! This module owns the decisions the Client Module contract deliberately
//! cannot make: which account a username resolves to, whether a password
//! verifies, whether a presented session is still live, and what the System Log
//! records when an attempt is denied. It answers the contract only in that
//! contract's closed rejection vocabulary, so no decision detail reaches a
//! response.
//!
//! Two properties are load-bearing here.
//!
//! Every denial performs the same work. Password verification is delegated to
//! [`PasswordAuthenticator`], which runs exactly one Argon2 verification on
//! every path, including one against a decoy verifier when no account, no
//! active account, or no usable verifier was found. This module preserves that
//! by resolving the credential into a [`StoredCredential`] and handing every
//! case to the same call, rather than returning early.
//!
//! Verification is bounded before it is admitted. Argon2 at the approved
//! profile costs [`MAX_VERIFICATION_MEMORY_KIB`] per verification, so login
//! declares a single-permit admission lane on its transport registration. The
//! listener acquires that permit in its admission stage, which runs before the
//! request body is allocated, and the permit is then carried into the blocking
//! task that performs the verification. Verification is therefore serialized
//! rather than merely queued, and a request that cannot be admitted inside the
//! existing deadlines is rejected before any of its body exists.

use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Router,
    http::{HeaderMap, Method, Uri},
};
use tokio::{sync::Semaphore, task};
use weavelit_module_client::{
    AUTH_LOGIN_ROUTE, AUTH_LOGOUT_ROUTE, AUTH_SESSION_ROUTE, AuthenticationCapability,
    AuthenticationDeclaration, AuthenticationRejection, ExpectedOrigin, LoginSubmission,
    SessionEstablished, SessionIdentity, SessionSubmission, validate_login_request,
};
use weavelit_server_authentication::{
    ACCEPTED_ARGON2_PROFILES, Argon2Engine, CsrfTokenDigest, PasswordAuthenticator, PasswordPolicy,
    PasswordVerdict, RustCryptoArgon2, SessionSecrets, SessionTokenDigest, StoredCredential,
};
use weavelit_server_database::{
    ApplicationState, DatabaseError, DeploymentIdentifier, InitializedState, LogType, Name,
    NewSession, SessionCsrfHash, SessionInstant, SessionStore, SessionTokenHash, SessionValidation,
    StateIdentifier, StoredSession,
};
use weavelit_server_log::{
    ConfiguredLogDestination, LogModuleCatalog, LogModuleIdentifier, TrustedLogModuleContext,
    TrustedRecordIssuer,
};
use weavelit_server_log_authority::ServerLogAuthority;
use weavelit_server_observability::ServerObservability;
use zeroize::Zeroizing;

use crate::{
    operational::OperationalDatabase,
    restore::assigned_configuration,
    transport::{
        BodyAdmission, PreBodyCheck, PreBodyGrant, PreBodyRejection, TransportCapability,
        TransportProfile, TransportRegistration,
    },
};

/// Password verifications this Server performs at one time.
///
/// Each verification reserves [`MAX_VERIFICATION_MEMORY_KIB`], so the listener's
/// concurrent connection allowance would otherwise multiply into that much
/// memory again for every admitted login.
const MAX_CONCURRENT_LOGIN_VERIFICATIONS: usize = 1;

const _: () = assert!(
    MAX_CONCURRENT_LOGIN_VERIFICATIONS == 1,
    "login verification is serialized so its peak memory is one profile ceiling, not a multiple"
);

const _: () = assert!(
    ACCEPTED_ARGON2_PROFILES.len() == 1,
    "a second accepted profile makes a replacement verifier reachable, and operational state has \
     no path that persists one; add that path before widening the allowlist"
);

/// Reads the current UTC time in Unix milliseconds.
///
/// Injected so a test observes session lifetime decisions at chosen instants
/// without waiting for real time to pass.
pub(crate) type WallClock = Arc<dyn Fn() -> Option<i64> + Send + Sync>;

/// Returns the production clock.
pub(crate) fn system_clock() -> WallClock {
    Arc::new(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
    })
}

// ---------------------------------------------------------------------------
// Runtime
// ---------------------------------------------------------------------------

/// The Server-side collaborators the authentication routes decide through.
pub(crate) struct AuthenticationRuntime<E> {
    database: OperationalDatabase,
    deployment: DeploymentIdentifier,
    client_modules: BTreeSet<Name>,
    authenticator: PasswordAuthenticator<E>,
    login_lane: Arc<Semaphore>,
    clock: WallClock,
    observability: ServerObservability,
    /// The System Log destination denials are recorded to, when one is
    /// configured and could be opened.
    ///
    /// A Server whose System Log cannot be opened still denies correctly; it
    /// records nothing. Delivery never gates or delays the denial.
    system_log: Option<Arc<ConfiguredLogDestination>>,
}

impl AuthenticationRuntime<RustCryptoArgon2> {
    /// Composes authentication over an operational database and its state.
    ///
    /// Returns `None` when the Server cannot build an authenticator it could
    /// deny safely with. The composer then mounts no authentication route at
    /// all, so the surface fails closed instead of serving a route that cannot
    /// perform equal work.
    pub(crate) fn new(
        database: OperationalDatabase,
        state: &InitializedState,
        client_modules: BTreeSet<Name>,
        state_root: PathBuf,
        log_catalog: &LogModuleCatalog,
    ) -> Option<Arc<Self>> {
        Self::with_engine(
            RustCryptoArgon2::new(PasswordPolicy::approved()),
            database,
            state,
            client_modules,
            system_clock(),
            open_system_log(state, state_root, log_catalog).map(Arc::new),
        )
    }
}

impl<E: Argon2Engine + Send + Sync + 'static> AuthenticationRuntime<E> {
    /// Composes authentication over an explicit verification engine and clock.
    ///
    /// A test injects a counting engine here to observe that a denial performs
    /// exactly as many verifications as an acceptance, which is a property of
    /// the work performed rather than of elapsed time.
    pub(crate) fn with_engine(
        engine: E,
        database: OperationalDatabase,
        state: &InitializedState,
        client_modules: BTreeSet<Name>,
        clock: WallClock,
        system_log: Option<Arc<ConfiguredLogDestination>>,
    ) -> Option<Arc<Self>> {
        let log_authority = ServerLogAuthority::new();

        Some(Arc::new(Self {
            deployment: state.deployment_identifier(),
            database,
            client_modules,
            authenticator: PasswordAuthenticator::new(engine, PasswordPolicy::approved()).ok()?,
            login_lane: Arc::new(Semaphore::new(MAX_CONCURRENT_LOGIN_VERIFICATIONS)),
            clock,
            observability: ServerObservability::new(TrustedRecordIssuer::from_server_authority(
                &log_authority,
            )),
            system_log,
        }))
    }

    /// Returns the three authentication routes, each paired with the transport
    /// registration that admits it.
    pub(crate) fn capabilities(
        self: &Arc<Self>,
        expected_origin: ExpectedOrigin,
    ) -> Vec<TransportCapability> {
        let declaration = Arc::new(AuthenticationDeclaration::new(
            self.capability(expected_origin),
        ));
        let session = Arc::clone(&declaration);
        let logout = Arc::clone(&declaration);

        vec![
            TransportCapability::new(
                TransportRegistration::new(
                    Method::PUT,
                    AUTH_LOGIN_ROUTE,
                    TransportProfile::DEFAULT,
                )
                .with_pre_body_check(Arc::new(LoginPreconditions { expected_origin }))
                // Acquired by the listener's admission stage, which runs after
                // this check and before the body is allocated.
                .with_admission(Arc::clone(&self.login_lane)),
                move |router: Router| router.route(AUTH_LOGIN_ROUTE, declaration.login_route()),
            ),
            TransportCapability::new(
                TransportRegistration::new(
                    Method::PUT,
                    AUTH_SESSION_ROUTE,
                    TransportProfile::DEFAULT,
                ),
                move |router: Router| router.route(AUTH_SESSION_ROUTE, session.session_route()),
            ),
            TransportCapability::new(
                TransportRegistration::new(
                    Method::PUT,
                    AUTH_LOGOUT_ROUTE,
                    TransportProfile::DEFAULT,
                ),
                move |router: Router| router.route(AUTH_LOGOUT_ROUTE, logout.logout_route()),
            ),
        ]
    }

    /// Binds this runtime's decisions to the Client Module route contract.
    fn capability(self: &Arc<Self>, expected_origin: ExpectedOrigin) -> AuthenticationCapability {
        let logging_in = Arc::clone(self);
        let validating = Arc::clone(self);
        let revoking = Arc::clone(self);

        AuthenticationCapability {
            expected_origin,
            correlate: Arc::new(correlation_identifier),
            login: Arc::new(move |submission| {
                let runtime = Arc::clone(&logging_in);
                Box::pin(async move { runtime.login(submission).await })
            }),
            validate_session: Arc::new(move |submission| {
                let runtime = Arc::clone(&validating);
                Box::pin(async move { runtime.validate_session(submission).await })
            }),
            logout: Arc::new(move |submission| {
                let runtime = Arc::clone(&revoking);
                Box::pin(async move { runtime.revoke_session(submission).await })
            }),
        }
    }

    // -----------------------------------------------------------------------
    // Login
    // -----------------------------------------------------------------------

    /// Verifies a submission and issues a session for it.
    ///
    /// The admission permit the listener acquired is taken from the request and
    /// moved into the blocking task, so the permit is released only once the
    /// Argon2 verification and the synchronous database work have finished. A
    /// submission that arrives without a permit is refused rather than run
    /// outside the lane.
    async fn login(
        self: Arc<Self>,
        submission: LoginSubmission,
    ) -> Result<SessionEstablished, AuthenticationRejection> {
        let Some(admission) = submission.context.get::<BodyAdmission>().cloned() else {
            return Err(AuthenticationRejection::ServiceUnavailable);
        };
        let LoginSubmission {
            username,
            password,
            client_module,
            correlation_id,
            ..
        } = submission;

        task::spawn_blocking(move || {
            let _admission = admission;
            self.login_blocking(&username, &password, &client_module, &correlation_id)
        })
        .await
        .unwrap_or(Err(AuthenticationRejection::ServiceUnavailable))
    }

    /// Runs the whole synchronous login decision under the admission permit.
    fn login_blocking(
        &self,
        username: &str,
        password: &str,
        client_module: &str,
        correlation_id: &str,
    ) -> Result<SessionEstablished, AuthenticationRejection> {
        // Rejected before any credential work, and independently of the
        // submitted account, so it discloses nothing about an account.
        let Some(client_module) = Name::new(client_module)
            .ok()
            .filter(|name| self.client_modules.contains(name))
        else {
            return Err(AuthenticationRejection::BadRequest);
        };
        let state = self
            .database
            .with(|database| database.load_initialized_state(self.deployment))
            .map_err(|_| AuthenticationRejection::ServiceUnavailable)?
            .map_err(|_| AuthenticationRejection::ServiceUnavailable)?;

        match self.verify_password(state.state(), username, password.as_bytes()) {
            PasswordOutcome::Verified { account } => self.issue_session(account, &client_module),
            PasswordOutcome::Denied => {
                // Delivery is attempted before the denial is returned, and every
                // failure inside is absorbed, so the System Log records the
                // attempt without being able to change what the caller answers.
                self.record_denial(correlation_id);
                Err(AuthenticationRejection::AuthenticationFailed)
            }
        }
    }

    /// Decides whether the submitted password authenticates an account.
    ///
    /// Kept separate from session issuance so a later elevation factor can be
    /// required between the two without replacing either.
    ///
    /// Every case reaches the same single [`PasswordAuthenticator::authenticate`]
    /// call: an unresolvable username, an unknown account, an inactive account,
    /// an account with no verifier, and a wrong password all perform one
    /// verification and produce [`PasswordOutcome::Denied`]. An operational
    /// failure inside verification is also denied rather than reported, because
    /// reporting it would separate a correct password from an incorrect one.
    fn verify_password(
        &self,
        state: &ApplicationState,
        username: &str,
        password: &[u8],
    ) -> PasswordOutcome {
        let submitted = Name::new(username).ok();
        let account = submitted.as_ref().and_then(|name| {
            state
                .accounts()
                .iter()
                .find(|account| &account.username == name)
        });
        let (credential, authenticated) = match account {
            None => (StoredCredential::UnknownAccount, None),
            Some(account) if !account.active => (StoredCredential::InactiveAccount, None),
            Some(account) => state
                .password_verifiers()
                .iter()
                .find(|verifier| verifier.account == account.identifier)
                .map_or((StoredCredential::NoVerifier, None), |verifier| {
                    (
                        StoredCredential::Verifier(verifier.verifier.as_str()),
                        Some(account.identifier),
                    )
                }),
        };

        match self.authenticator.authenticate(credential, password) {
            // A verified decoy is unreachable, so `authenticated` is always
            // present here; the match makes that unreachable case deny rather
            // than authenticate an account this Server never resolved.
            Ok(PasswordVerdict::Verified { .. }) => authenticated
                .map_or(PasswordOutcome::Denied, |account| {
                    PasswordOutcome::Verified { account }
                }),
            Ok(PasswordVerdict::Denied) | Err(_) => PasswordOutcome::Denied,
        }
    }

    /// Issues and persists a session for an already-authenticated account.
    ///
    /// Kept separate from password verification so a later elevation factor can
    /// gate this step, and so rotating the cross-site request forgery token on
    /// elevation reuses the same issuance path.
    fn issue_session(
        &self,
        account: StateIdentifier,
        client_module: &Name,
    ) -> Result<SessionEstablished, AuthenticationRejection> {
        let unavailable = AuthenticationRejection::ServiceUnavailable;
        // Generated fresh on every login, so the cross-site request forgery
        // token a session carries is rotated by logging in.
        let secrets = SessionSecrets::generate().map_err(|_| unavailable)?;
        let (session_digest, csrf_digest) = secrets.digests();
        let session = NewSession::new(
            SessionTokenHash::from_bytes(*session_digest.as_bytes()).map_err(|_| unavailable)?,
            SessionCsrfHash::from_bytes(*csrf_digest.as_bytes()).map_err(|_| unavailable)?,
            account,
            client_module.clone(),
            self.now()?,
        );
        self.with_sessions(|sessions| sessions.create(&session))?;

        Ok(SessionEstablished {
            session_token: Zeroizing::new(secrets.session().as_str().to_owned()),
            csrf_token: Zeroizing::new(secrets.csrf().as_str().to_owned()),
        })
    }

    // -----------------------------------------------------------------------
    // Session validation and revocation
    // -----------------------------------------------------------------------

    /// Reports the identity a presented session authenticates.
    async fn validate_session(
        self: Arc<Self>,
        submission: SessionSubmission,
    ) -> Result<SessionIdentity, AuthenticationRejection> {
        let SessionSubmission {
            session_token,
            csrf_token,
            ..
        } = submission;

        task::spawn_blocking(move || {
            let session = self.authorized_session(&session_token, &csrf_token)?.1;
            Ok(SessionIdentity {
                // Only the account and the issuing Client Module. No Group,
                // grant, or other authorization decision is reported or
                // cached, because authorization is evaluated live.
                account_id: hexadecimal(session.account().as_bytes()),
                client_module: session.client_module().as_str().to_owned(),
            })
        })
        .await
        .unwrap_or(Err(AuthenticationRejection::ServiceUnavailable))
    }

    /// Revokes a presented session.
    async fn revoke_session(
        self: Arc<Self>,
        submission: SessionSubmission,
    ) -> Result<(), AuthenticationRejection> {
        let SessionSubmission {
            session_token,
            csrf_token,
            ..
        } = submission;

        task::spawn_blocking(move || {
            let token_hash = self.authorized_session(&session_token, &csrf_token)?.0;
            self.with_sessions(|sessions| sessions.revoke(&token_hash))?;
            Ok(())
        })
        .await
        .unwrap_or(Err(AuthenticationRejection::ServiceUnavailable))
    }

    /// Resolves a presented session and proves its request carries its token.
    ///
    /// The session must exist, be inside both its idle and absolute lifetimes,
    /// and the request must echo the cross-site request forgery token bound to
    /// that exact session. Every failure is one rejection, so a live session
    /// with a wrong token is indistinguishable from an unknown one.
    fn authorized_session(
        &self,
        session_token: &str,
        csrf_token: &str,
    ) -> Result<(SessionTokenHash, StoredSession), AuthenticationRejection> {
        let invalid = AuthenticationRejection::SessionInvalid;
        let token_hash =
            SessionTokenHash::from_bytes(*SessionTokenDigest::of(session_token).as_bytes())
                .map_err(|_| invalid)?;
        let presented = SessionCsrfHash::from_bytes(*CsrfTokenDigest::of(csrf_token).as_bytes())
            .map_err(|_| invalid)?;

        let now = self.now()?;
        let SessionValidation::Valid(session) =
            self.with_sessions(|sessions| sessions.validate_and_touch(&token_hash, now))?
        else {
            return Err(invalid);
        };
        if !session.csrf_hash().matches(&presented) {
            return Err(invalid);
        }
        Ok((token_hash, session))
    }

    // -----------------------------------------------------------------------
    // Shared plumbing
    // -----------------------------------------------------------------------

    /// Runs one operation against the live session store.
    ///
    /// A backend that serves no session store is refused rather than
    /// authenticated without durable sessions.
    fn with_sessions<R>(
        &self,
        operation: impl FnOnce(&mut dyn SessionStore) -> Result<R, DatabaseError>,
    ) -> Result<R, AuthenticationRejection> {
        self.database
            .with(|database| {
                database
                    .sessions()
                    .map_or(Err(DatabaseError::Unavailable), operation)
            })
            .map_err(|_| AuthenticationRejection::ServiceUnavailable)?
            .map_err(|_| AuthenticationRejection::ServiceUnavailable)
    }

    /// Reads the current instant sessions are decided against.
    fn now(&self) -> Result<SessionInstant, AuthenticationRejection> {
        (self.clock)()
            .and_then(|milliseconds| SessionInstant::from_unix_milliseconds(milliseconds).ok())
            .ok_or(AuthenticationRejection::ServiceUnavailable)
    }

    /// Attempts to record one denial in the System Log.
    ///
    /// Every failure is absorbed: an unconfigured destination, an unreadable
    /// clock, a randomness failure, and a delivery failure all leave the denial
    /// exactly as it was. Delivery is attempted before the denial is returned
    /// and runs inside the request's existing processing budget, so it can
    /// neither enrich, delay beyond that budget, nor appear in the response.
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
        let Ok(record) = self.observability.prepare_authentication_failure(
            identifier,
            event_time,
            correlation_id,
        ) else {
            return;
        };
        // A delivery failure is absorbed here and nowhere else, so it cannot
        // reach the denial the caller is about to return.
        let _ = destination.deliver(&record);
    }
}

/// Whether a submitted password authenticated an account.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PasswordOutcome {
    /// The password verified against this account's stored verifier.
    Verified {
        /// The account the verifier belongs to.
        account: StateIdentifier,
    },
    /// The submission was denied, for a reason this value cannot report.
    Denied,
}

// ---------------------------------------------------------------------------
// Transport preconditions
// ---------------------------------------------------------------------------

/// The login checks that run before the listener allocates a request body.
///
/// Running here means an untrusted or malformed login is denied before it can
/// allocate its body and, because the listener runs this stage before
/// admission, before it can occupy the single verification permit.
struct LoginPreconditions {
    expected_origin: ExpectedOrigin,
}

impl PreBodyCheck for LoginPreconditions {
    fn check(
        &self,
        method: &Method,
        _target: &Uri,
        headers: &HeaderMap,
    ) -> Result<PreBodyGrant, PreBodyRejection> {
        match validate_login_request(method, headers, self.expected_origin) {
            Ok(()) => Ok(PreBodyGrant::default()),
            Err(AuthenticationRejection::RequestOriginDenied) => {
                Err(PreBodyRejection::RequestOriginDenied)
            }
            Err(_) => Err(PreBodyRejection::BadRequest),
        }
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

/// Opens the System Log destination the initialized state assigns.
///
/// Returns `None` when no enabled configuration is assigned to the System Log
/// or the assigned module cannot be opened. Authentication still decides
/// correctly; it records nothing.
fn open_system_log(
    state: &InitializedState,
    state_root: PathBuf,
    log_catalog: &LogModuleCatalog,
) -> Option<ConfiguredLogDestination> {
    let assigned = assigned_configuration(
        state.state().log_module_configurations(),
        state.state().log_assignments(),
        LogType::System,
    )?;
    let module = LogModuleIdentifier::new(assigned.module.as_str()).ok()?;
    let context = TrustedLogModuleContext::from_server_authority(
        &ServerLogAuthority::new(),
        state_root,
        *state.deployment_identifier().as_bytes(),
    );
    log_catalog.create_destination(&module, &context).ok()
}

/// Fills a fixed-size buffer from operating-system randomness.
pub(crate) fn random_bytes<const BYTES: usize>() -> Option<[u8; BYTES]> {
    let mut bytes = [0_u8; BYTES];
    getrandom::fill(&mut bytes).ok()?;
    Some(bytes)
}

/// Renders bytes as lowercase hexadecimal.
pub(crate) fn hexadecimal(bytes: &[u8]) -> String {
    const HEX: [u8; 16] = *b"0123456789abcdef";

    let mut rendered = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        rendered.push(char::from(HEX[usize::from(byte >> 4)]));
        rendered.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    rendered
}

/// Generates a correlation identifier that carries no request content.
pub(crate) fn correlation_identifier() -> Option<String> {
    Some(hexadecimal(&random_bytes::<16>()?))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        sync::{
            Condvar, Mutex,
            atomic::{AtomicI64, AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use axum::http::StatusCode;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use weavelit_module_client::{
        cookie::{CSRF_COOKIE_NAME, CookieEffect, CookieValue, SESSION_COOKIE_NAME},
        typed_json::{
            MAX_STABLE_CODE_BYTES, ResponseCorrelation, StableCode, TypedJsonEnvelope, TypedResult,
            TypedValue,
        },
    };
    use weavelit_server_authentication::{
        Argon2Profile, AuthenticationError, SESSION_TOKEN_TEXT_BYTES,
    };
    use weavelit_server_database::{
        Account, AccountPasswordVerifier, PasswordVerifier, SESSION_ABSOLUTE_LIFETIME_MILLISECONDS,
        SESSION_IDLE_TIMEOUT_MILLISECONDS,
    };
    use weavelit_server_log::{
        CompleteLogRecord, DurableAcknowledgement, LogCapabilities, LogDestination,
        LogDestinationError, LogDestinationFactory, LogModuleFactoryContext, LogModuleRegistration,
        LogRecordPersistenceView, LogRecordType,
    };

    use super::*;
    use crate::{
        BoundedResponse, ConnectionTimeouts, MAX_TYPED_JSON_BODY_BYTES, REQUEST_PROCESSING_TIMEOUT,
        REQUEST_READ_TIMEOUT, StartupOutcome, TLS_HANDSHAKE_TIMEOUT, bounded_response_from_axum,
        classify_restricted_startup, fallback_router,
        tests::{
            UNBOUND_LISTENER, process_over_duplex, published_serving_modes, seal_deployment_with,
            sealed_application_state_with,
        },
        transport::MountedSurface,
        write_bounded_response,
    };

    /// A stored verifier at the approved profile whose salt and output are
    /// fixed, so a test seeds real credential state without paying Argon2's
    /// approved memory and time cost even once.
    const STORED_VERIFIER: &str = "$argon2id$v=19$m=65536,t=3,p=1$\
                                   BwcHBwcHBwcHBwcHBwcHBw$\
                                   CQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQk";

    /// The one password the injected engine reports as verifying.
    const CORRECT_PASSWORD: &str = "correct-horse-battery-staple";
    const WRONG_PASSWORD: &str = "wrong-horse-battery-staple";

    const ACTIVE_USERNAME: &str = "alice";
    const INACTIVE_USERNAME: &str = "dormant";
    const UNKNOWN_USERNAME: &str = "ghost";
    const CLIENT_MODULE: &str = "web-ui";

    const ACTIVE_ACCOUNT_BYTES: [u8; 16] = [0xa1; 16];
    const INACTIVE_ACCOUNT_BYTES: [u8; 16] = [0xd0; 16];

    /// The instant every seeded session is issued at.
    const ISSUED_AT: i64 = 1_700_000_000_000;

    /// Long enough for a login that is admitted immediately, short enough that
    /// a login blocked on the single permit is refused promptly.
    const SHORT_PROCESSING: Duration = Duration::from_millis(250);

    // -----------------------------------------------------------------------
    // Injected verification engine
    // -----------------------------------------------------------------------

    /// One observed transition through the injected verification engine.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Transition {
        Entered,
        Left,
    }

    /// Gate and counters shared with every clone of the injected engine.
    #[derive(Debug)]
    struct EngineState {
        verifications: AtomicUsize,
        hashes: AtomicUsize,
        gate: Mutex<Gate>,
        changed: Condvar,
    }

    #[derive(Debug, Default)]
    struct Gate {
        /// Verifications that have entered the engine.
        entered: usize,
        /// Verifications the test has permitted to complete.
        released: usize,
        /// Whether verification runs without waiting for a release.
        open: bool,
        /// Verifications currently inside the engine.
        in_flight: usize,
        /// The largest observed concurrent verification count.
        peak_in_flight: usize,
        transitions: Vec<Transition>,
    }

    impl EngineState {
        fn new(open: bool) -> Arc<Self> {
            Arc::new(Self {
                verifications: AtomicUsize::new(0),
                hashes: AtomicUsize::new(0),
                gate: Mutex::new(Gate {
                    open,
                    ..Gate::default()
                }),
                changed: Condvar::new(),
            })
        }

        /// Records entry and blocks until the test releases one verification.
        fn enter(&self) {
            let mut gate = self.gate.lock().expect("the test gate must not poison");
            gate.entered += 1;
            gate.in_flight += 1;
            gate.peak_in_flight = gate.peak_in_flight.max(gate.in_flight);
            gate.transitions.push(Transition::Entered);
            self.changed.notify_all();
            while !gate.open && gate.released == 0 {
                gate = self
                    .changed
                    .wait(gate)
                    .expect("the test gate must not poison");
            }
            if !gate.open {
                gate.released -= 1;
            }
        }

        /// Records that one verification finished inside the engine.
        fn leave(&self) {
            let mut gate = self.gate.lock().expect("the test gate must not poison");
            gate.in_flight -= 1;
            gate.transitions.push(Transition::Left);
            self.changed.notify_all();
        }

        fn verifications(&self) -> usize {
            self.verifications.load(Ordering::SeqCst)
        }

        fn hashes(&self) -> usize {
            self.hashes.load(Ordering::SeqCst)
        }

        /// Blocks until at least `count` verifications have entered.
        fn await_entered(&self, count: usize) {
            let mut gate = self.gate.lock().expect("the test gate must not poison");
            while gate.entered < count {
                gate = self
                    .changed
                    .wait(gate)
                    .expect("the test gate must not poison");
            }
        }

        /// Permits one more verification to complete.
        fn release(&self) {
            let mut gate = self.gate.lock().expect("the test gate must not poison");
            gate.released += 1;
            self.changed.notify_all();
        }

        fn transitions(&self) -> Vec<Transition> {
            self.gate
                .lock()
                .expect("the test gate must not poison")
                .transitions
                .clone()
        }

        fn peak_in_flight(&self) -> usize {
            self.gate
                .lock()
                .expect("the test gate must not poison")
                .peak_in_flight
        }
    }

    /// A verification engine that counts work instead of performing it.
    ///
    /// Counting is the seam that lets a test compare the work a denial performs
    /// against the work an acceptance performs without measuring elapsed time.
    #[derive(Clone, Debug)]
    struct CountingEngine {
        state: Arc<EngineState>,
    }

    impl Argon2Engine for CountingEngine {
        fn verify(&self, password: &[u8], _profile: &Argon2Profile, encoded: &str) -> bool {
            self.state.verifications.fetch_add(1, Ordering::SeqCst);
            self.state.enter();
            let verified = encoded == STORED_VERIFIER && password == CORRECT_PASSWORD.as_bytes();
            self.state.leave();
            verified
        }

        fn hash(
            &self,
            _password: &[u8],
            _profile: &Argon2Profile,
            _salt: &[u8],
        ) -> Result<String, AuthenticationError> {
            self.state.hashes.fetch_add(1, Ordering::SeqCst);
            Ok(STORED_VERIFIER.to_owned())
        }
    }

    // -----------------------------------------------------------------------
    // Injected System Log destination
    // -----------------------------------------------------------------------

    /// The pre-redacted content of one delivered System Log record.
    #[derive(Clone, Debug, Eq, PartialEq)]
    struct DeliveredRecord {
        correlation_id: String,
        classification: String,
        detail: String,
    }

    #[derive(Debug)]
    struct RecordingDestination {
        delivered: Arc<Mutex<Vec<DeliveredRecord>>>,
        fails: bool,
    }

    impl LogDestination for RecordingDestination {
        fn deliver(
            &self,
            record: &CompleteLogRecord,
            acknowledgement: DurableAcknowledgement,
        ) -> Result<DurableAcknowledgement, LogDestinationError> {
            let LogRecordPersistenceView::System(view) = record.persistence_view() else {
                return Err(LogDestinationError::IntegrityFailure);
            };
            self.delivered
                .lock()
                .expect("the delivered record log must not poison")
                .push(DeliveredRecord {
                    correlation_id: view.correlation_id().as_str().to_owned(),
                    classification: view.body().classification().to_owned(),
                    detail: view.body().detail().to_owned(),
                });
            if self.fails {
                return Err(LogDestinationError::Unavailable);
            }
            Ok(acknowledgement)
        }
    }

    struct RecordingFactory {
        delivered: Arc<Mutex<Vec<DeliveredRecord>>>,
        fails: bool,
    }

    impl LogDestinationFactory for RecordingFactory {
        fn create(
            &self,
            _context: &LogModuleFactoryContext<'_>,
        ) -> Result<Box<dyn LogDestination>, LogDestinationError> {
            Ok(Box::new(RecordingDestination {
                delivered: Arc::clone(&self.delivered),
                fails: self.fails,
            }))
        }
    }

    /// Builds a System Log destination that records every delivered record.
    fn recording_log(
        fails: bool,
    ) -> (
        Arc<ConfiguredLogDestination>,
        Arc<Mutex<Vec<DeliveredRecord>>>,
    ) {
        let delivered = Arc::new(Mutex::new(Vec::new()));
        let catalog = LogModuleCatalog::new(vec![LogModuleRegistration::new(
            "log-test",
            LogCapabilities::new(vec![LogRecordType::System])
                .expect("the test capability declaration must be accepted"),
            Box::new(RecordingFactory {
                delivered: Arc::clone(&delivered),
                fails,
            }),
        )])
        .expect("the test catalog must be accepted");
        let context = TrustedLogModuleContext::from_server_authority(
            &ServerLogAuthority::new(),
            PathBuf::from("/nonexistent-test-log-root"),
            [0x33; 16],
        );
        let destination = catalog
            .create_destination(
                &LogModuleIdentifier::new("log-test")
                    .expect("the test module identifier must be accepted"),
                &context,
            )
            .expect("the test destination must open");

        (Arc::new(destination), delivered)
    }

    // -----------------------------------------------------------------------
    // Harness
    // -----------------------------------------------------------------------

    /// An operational authentication surface over a real sealed deployment.
    ///
    /// The deployment, the Application Database, the session store, and the
    /// listener path are all real. Only the verification engine, the clock, and
    /// the System Log destination are injected, because those are the three
    /// collaborators a deterministic test cannot otherwise observe.
    struct AuthSurface {
        /// Held so the Application Database and its state-root lock stay open
        /// for as long as the surface serves. Dropped before the temporary
        /// state root it lives in.
        _startup: crate::RestrictedStartup,
        _root: tempfile::TempDir,
        runtime: Arc<AuthenticationRuntime<CountingEngine>>,
        engine: Arc<EngineState>,
        clock: Arc<AtomicI64>,
    }

    impl AuthSurface {
        fn new() -> Self {
            Self::build(true, None)
        }

        fn gated() -> Self {
            Self::build(false, None)
        }

        fn with_log(system_log: Arc<ConfiguredLogDestination>) -> Self {
            Self::build(true, Some(system_log))
        }

        fn build(open_engine: bool, system_log: Option<Arc<ConfiguredLogDestination>>) -> Self {
            let root = tempfile::tempdir().expect("the test state root must be created");
            fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700))
                .expect("the test state root must be private");
            let state_root = root
                .path()
                .canonicalize()
                .expect("the test state root must resolve");
            seal_deployment_with(&state_root, &authentication_state());

            let startup = classify_restricted_startup(&state_root)
                .expect("a sealed state root must classify");
            assert_eq!(startup.outcome(), StartupOutcome::Initialized);

            let engine = EngineState::new(open_engine);
            let clock = Arc::new(AtomicI64::new(ISSUED_AT));
            let reading = Arc::clone(&clock);
            let runtime = AuthenticationRuntime::with_engine(
                CountingEngine {
                    state: Arc::clone(&engine),
                },
                startup
                    .application_database()
                    .expect("a sealed startup hands over its Application Database")
                    .clone(),
                startup
                    .initialized_state()
                    .expect("a sealed startup hands over its loaded state"),
                client_modules(),
                Arc::new(move || Some(reading.load(Ordering::SeqCst))),
                system_log,
            )
            .expect("the authentication runtime must compose");

            Self {
                _startup: startup,
                _root: root,
                runtime,
                engine,
                clock,
            }
        }

        /// Builds the mounted surface the three authentication routes serve on.
        fn surface(&self) -> MountedSurface {
            let mut surface = MountedSurface::without_registrations(fallback_router());
            for capability in self.runtime.capabilities(expected_origin()) {
                surface = surface.with_capability(capability);
            }
            surface
        }

        fn set_clock(&self, milliseconds: i64) {
            self.clock.store(milliseconds, Ordering::SeqCst);
        }
    }

    /// The sealed application state every authentication test decides against.
    fn authentication_state() -> ApplicationState {
        let active = StateIdentifier::from_bytes(ACTIVE_ACCOUNT_BYTES)
            .expect("the active account identifier must be accepted");
        let inactive = StateIdentifier::from_bytes(INACTIVE_ACCOUNT_BYTES)
            .expect("the inactive account identifier must be accepted");

        sealed_application_state_with(
            vec![
                Account {
                    identifier: active,
                    username: name(ACTIVE_USERNAME),
                    display_name: None,
                    active: true,
                },
                Account {
                    identifier: inactive,
                    username: name(INACTIVE_USERNAME),
                    display_name: None,
                    active: false,
                },
            ],
            vec![
                AccountPasswordVerifier {
                    account: active,
                    verifier: verifier(),
                },
                AccountPasswordVerifier {
                    account: inactive,
                    verifier: verifier(),
                },
            ],
        )
    }

    fn verifier() -> PasswordVerifier {
        PasswordVerifier::new(STORED_VERIFIER).expect("the stored verifier must be accepted")
    }

    fn name(value: &str) -> Name {
        Name::new(value).expect("the test name must be accepted")
    }

    fn client_modules() -> BTreeSet<Name> {
        BTreeSet::from([name(CLIENT_MODULE)])
    }

    fn expected_origin() -> ExpectedOrigin {
        ExpectedOrigin::from_listener(
            UNBOUND_LISTENER
                .parse()
                .expect("the test listener authority must parse"),
        )
    }

    // -----------------------------------------------------------------------
    // Request construction and rendering
    // -----------------------------------------------------------------------

    fn default_timeouts() -> ConnectionTimeouts {
        ConnectionTimeouts {
            handshake: TLS_HANDSHAKE_TIMEOUT,
            request_read: REQUEST_READ_TIMEOUT,
            processing: REQUEST_PROCESSING_TIMEOUT,
        }
    }

    fn login_body(username: &str, password: &str, client_module: &str) -> String {
        format!(
            "{{\"username\":\"{username}\",\"password\":\"{password}\",\
             \"client_module\":\"{client_module}\"}}"
        )
    }

    /// Builds a login request head whose origin headers are trusted.
    fn login_head(body_length: usize) -> String {
        login_head_with(
            &format!("https://{UNBOUND_LISTENER}"),
            Some("1"),
            body_length,
        )
    }

    fn login_head_with(origin: &str, csrf: Option<&str>, body_length: usize) -> String {
        let csrf = csrf.map_or_else(String::new, |value| format!("X-Weavelit-CSRF: {value}\r\n"));
        format!(
            "PUT {AUTH_LOGIN_ROUTE} HTTP/1.1\r\n\
             Host: {UNBOUND_LISTENER}\r\n\
             Origin: {origin}\r\n\
             {csrf}\
             Accept: application/json\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {body_length}\r\n\r\n"
        )
    }

    /// Builds a session-bearing request head for the session or logout route.
    fn session_head(target: &str, session_token: &str, csrf_token: &str) -> String {
        session_head_with(
            target,
            &format!("https://{UNBOUND_LISTENER}"),
            session_token,
            Some(csrf_token),
        )
    }

    fn session_head_with(
        target: &str,
        origin: &str,
        session_token: &str,
        csrf_token: Option<&str>,
    ) -> String {
        let csrf =
            csrf_token.map_or_else(String::new, |value| format!("X-Weavelit-CSRF: {value}\r\n"));
        format!(
            "PUT {target} HTTP/1.1\r\n\
             Host: {UNBOUND_LISTENER}\r\n\
             Origin: {origin}\r\n\
             {csrf}\
             Cookie: {SESSION_COOKIE_NAME}={session_token}\r\n\
             Accept: application/json\r\n\
             Content-Length: 0\r\n\r\n"
        )
    }

    /// Drives one complete request through the production listener path.
    async fn request(
        surface: MountedSurface,
        timeouts: ConnectionTimeouts,
        head: String,
        body: String,
    ) -> BoundedResponse {
        process_over_duplex(surface, timeouts, move |stream| {
            tokio::spawn(async move {
                let mut stream = stream;
                stream.write_all(head.as_bytes()).await.ok();
                stream.write_all(body.as_bytes()).await.ok();
                std::future::pending::<()>().await;
            })
        })
        .await
    }

    /// Logs in over the real route and returns the response with its tokens.
    async fn login(surface: &AuthSurface, username: &str, password: &str) -> BoundedResponse {
        let body = login_body(username, password, CLIENT_MODULE);
        request(
            surface.surface(),
            default_timeouts(),
            login_head(body.len()),
            body,
        )
        .await
    }

    /// Renders a bounded response exactly as the listener writes it.
    async fn rendered(response: &BoundedResponse) -> String {
        let (mut client, mut server) = tokio::io::duplex(64 * 1024);
        write_bounded_response(&mut client, response.clone())
            .await
            .expect("the test response must be written");
        let mut bytes = Vec::new();
        server
            .read_to_end(&mut bytes)
            .await
            .expect("the rendered response must be readable");
        String::from_utf8(bytes).expect("the rendered response must be UTF-8")
    }

    fn body_text(response: &BoundedResponse) -> String {
        String::from_utf8(response.body.to_vec()).expect("the response body must be UTF-8")
    }

    /// Extracts the correlation identifier a typed envelope carries.
    fn correlation_of(response: &BoundedResponse) -> String {
        const FIELD: &str = "\"correlation_id\":\"";
        let body = body_text(response);
        let start = body
            .find(FIELD)
            .expect("a typed envelope must carry a correlation identifier")
            + FIELD.len();
        let rest = &body[start..];
        let end = rest
            .find('"')
            .expect("a correlation identifier must be terminated");
        rest[..end].to_owned()
    }

    /// Replaces the response's own correlation identifier with a fixed marker.
    ///
    /// Every response carries a fresh identifier by design, so byte comparison
    /// across responses is only meaningful once that one varying field is
    /// normalized. Each test that normalizes also asserts the identifiers were
    /// well formed and distinct, so normalization cannot hide a difference.
    fn normalized(rendered: &str, correlation_id: &str) -> String {
        rendered.replace(correlation_id, "<correlation>")
    }

    fn is_correlation_identifier(value: &str) -> bool {
        value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    }

    fn set_cookie_lines(rendered: &str) -> Vec<String> {
        rendered
            .split("\r\n")
            .filter(|line| line.starts_with("Set-Cookie: "))
            .map(str::to_owned)
            .collect()
    }

    /// Reads a cookie value out of the rendered response head.
    fn cookie_value(rendered: &str, name: &str) -> String {
        let prefix = format!("Set-Cookie: {name}=");
        let line = set_cookie_lines(rendered)
            .into_iter()
            .find(|line| line.starts_with(&prefix))
            .unwrap_or_else(|| panic!("the response must carry a {name} cookie"));
        let value = &line[prefix.len()..];
        value
            .split(';')
            .next()
            .expect("a cookie line always has a value segment")
            .to_owned()
    }

    /// Logs in and returns the issued session and CSRF token pair.
    async fn established_session(surface: &AuthSurface) -> (String, String) {
        let response = login(surface, ACTIVE_USERNAME, CORRECT_PASSWORD).await;
        assert_eq!(response.status, StatusCode::OK);
        let head = rendered(&response).await;
        (
            cookie_value(&head, SESSION_COOKIE_NAME),
            cookie_value(&head, CSRF_COOKIE_NAME),
        )
    }

    /// The transitions a correct, serialized engine has recorded once `entered`
    /// verifications have begun.
    fn serialized_transitions(entered: usize) -> Vec<Transition> {
        let mut expected = Vec::new();
        for index in 0..entered {
            if index > 0 {
                expected.push(Transition::Left);
            }
            expected.push(Transition::Entered);
        }
        expected
    }

    // -----------------------------------------------------------------------
    // Admission and cost bounding
    // -----------------------------------------------------------------------

    /// The permit is taken before the body exists, not after it is read.
    ///
    /// Only the request head is written, so the body never arrives. With the
    /// permit already held, the current order rejects at the admission
    /// deadline, which is the processing budget. Acquiring the permit after the
    /// body was allocated would instead block in the body read, which the
    /// login profile bounds by the head deadline, and would answer with the
    /// request-timeout body rather than the gateway-timeout one. The two
    /// outcomes are distinct fixed bodies, so this asserts the order directly.
    #[tokio::test]
    async fn login_admission_is_acquired_before_the_request_body_is_allocated() {
        let surface = AuthSurface::new();
        let held = Arc::clone(&surface.runtime.login_lane)
            .try_acquire_owned()
            .expect("the single login permit must start free");

        let body = login_body(ACTIVE_USERNAME, CORRECT_PASSWORD, CLIENT_MODULE);
        let head = login_head(body.len());
        let timeouts = ConnectionTimeouts {
            handshake: TLS_HANDSHAKE_TIMEOUT,
            request_read: REQUEST_READ_TIMEOUT,
            processing: SHORT_PROCESSING,
        };
        let response = process_over_duplex(surface.surface(), timeouts, move |stream| {
            tokio::spawn(async move {
                let mut stream = stream;
                stream.write_all(head.as_bytes()).await.ok();
                std::future::pending::<()>().await;
            })
        })
        .await;

        assert_eq!(response.status, StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(body_text(&response), "{\"error\":\"gateway_timeout\"}");
        assert!(response.cookies.is_none());
        assert_eq!(
            surface.engine.verifications(),
            0,
            "no verification may begin for a request that was never admitted"
        );
        drop(held);
    }

    /// Concurrent logins verify one at a time, not merely queue.
    ///
    /// Three real requests are driven at once and the injected engine reports
    /// every entry and exit. A correct lane produces a strictly alternating
    /// transition log, because the permit is released only after the blocking
    /// login has returned. A wider lane would record a second entry before the
    /// first exit.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn at_most_one_password_verification_runs_at_a_time() {
        assert_eq!(MAX_CONCURRENT_LOGIN_VERIFICATIONS, 1);

        let surface = AuthSurface::gated();
        let mut attempts = Vec::new();
        for _ in 0..3 {
            let mounted = surface.surface();
            attempts.push(tokio::spawn(async move {
                let body = login_body(ACTIVE_USERNAME, CORRECT_PASSWORD, CLIENT_MODULE);
                request(mounted, default_timeouts(), login_head(body.len()), body).await
            }));
        }

        for entered in 1..=3_usize {
            let engine = Arc::clone(&surface.engine);
            task::spawn_blocking(move || engine.await_entered(entered))
                .await
                .expect("the gate wait must not panic");
            assert_eq!(
                surface.engine.transitions(),
                serialized_transitions(entered),
                "verification {entered} began before its predecessor finished"
            );
            assert!(
                surface.runtime.login_lane.try_acquire().is_err(),
                "the permit must be held for the whole verification"
            );
            surface.engine.release();
        }

        for attempt in attempts {
            let response = attempt.await.expect("each login attempt must complete");
            assert_eq!(response.status, StatusCode::OK);
        }
        assert_eq!(surface.engine.verifications(), 3);
        assert_eq!(surface.engine.peak_in_flight(), 1);
        let mut expected = serialized_transitions(3);
        expected.push(Transition::Left);
        assert_eq!(surface.engine.transitions(), expected);
    }

    /// An unadmitted login is refused inside the existing budgets.
    ///
    /// The first half holds the permit and shows the refusal arrives at the
    /// processing deadline with no verification started. The second half shows
    /// the login registration still reads its body on the head's deadline, so
    /// it extends neither the read nor the processing budget.
    #[tokio::test]
    async fn an_unadmitted_login_is_refused_without_extending_either_deadline() {
        assert_eq!(REQUEST_READ_TIMEOUT, Duration::from_secs(5));
        assert_eq!(REQUEST_PROCESSING_TIMEOUT, Duration::from_secs(10));

        let surface = AuthSurface::new();
        let held = Arc::clone(&surface.runtime.login_lane)
            .try_acquire_owned()
            .expect("the single login permit must start free");
        let body = login_body(ACTIVE_USERNAME, CORRECT_PASSWORD, CLIENT_MODULE);
        let refused = request(
            surface.surface(),
            ConnectionTimeouts {
                handshake: TLS_HANDSHAKE_TIMEOUT,
                request_read: REQUEST_READ_TIMEOUT,
                processing: SHORT_PROCESSING,
            },
            login_head(body.len()),
            body,
        )
        .await;
        assert_eq!(refused.status, StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(body_text(&refused), "{\"error\":\"gateway_timeout\"}");
        assert_eq!(surface.engine.verifications(), 0);
        drop(held);

        let surface = AuthSurface::new();
        let body = login_body(ACTIVE_USERNAME, CORRECT_PASSWORD, CLIENT_MODULE);
        let head = login_head(body.len());
        let timed_out = process_over_duplex(
            surface.surface(),
            ConnectionTimeouts {
                handshake: TLS_HANDSHAKE_TIMEOUT,
                request_read: Duration::from_millis(200),
                processing: REQUEST_PROCESSING_TIMEOUT,
            },
            move |stream| {
                tokio::spawn(async move {
                    let mut stream = stream;
                    stream.write_all(head.as_bytes()).await.ok();
                    std::future::pending::<()>().await;
                })
            },
        )
        .await;
        assert_eq!(timed_out.status, StatusCode::REQUEST_TIMEOUT);
        assert_eq!(body_text(&timed_out), "{\"error\":\"request_timeout\"}");
        assert_eq!(surface.engine.verifications(), 0);
    }

    // -----------------------------------------------------------------------
    // Equal-work denial at the route layer
    // -----------------------------------------------------------------------

    /// Unknown, inactive, and wrong-password denials are the same bytes.
    ///
    /// Every response carries its own correlation identifier by design, so the
    /// comparison normalizes that one field. The test first proves each
    /// identifier is well formed and that all three differ, so normalization
    /// cannot be hiding a difference the responses actually had.
    #[tokio::test]
    async fn every_account_denial_returns_identical_response_bytes() {
        let surface = AuthSurface::new();
        let mut wire = Vec::new();
        let mut correlations = Vec::new();
        for (username, password) in [
            (UNKNOWN_USERNAME, CORRECT_PASSWORD),
            (INACTIVE_USERNAME, CORRECT_PASSWORD),
            (ACTIVE_USERNAME, WRONG_PASSWORD),
        ] {
            let response = login(&surface, username, password).await;
            assert_eq!(response.status, StatusCode::UNAUTHORIZED, "{username}");
            assert!(response.cookies.is_none(), "{username}");
            let correlation = correlation_of(&response);
            assert!(is_correlation_identifier(&correlation), "{correlation}");
            wire.push(normalized(&rendered(&response).await, &correlation));
            correlations.push(correlation);
        }

        correlations.sort();
        correlations.dedup();
        assert_eq!(
            correlations.len(),
            3,
            "each denial must carry its own correlation identifier"
        );
        assert_eq!(wire[0], wire[1]);
        assert_eq!(wire[1], wire[2]);
        assert_eq!(
            wire[0],
            "HTTP/1.1 401 \r\nContent-Type: application/json; charset=utf-8\r\n\r\n\
             {\"error\":\"authentication_failed\",\"correlation_id\":\"<correlation>\"}"
        );
    }

    /// Every denial performs exactly the verification an acceptance performs.
    ///
    /// The count comes from the injected engine. Nothing here measures elapsed
    /// time, because a timing comparison would be a race rather than a proof.
    #[tokio::test]
    async fn every_account_denial_performs_the_same_verification_work() {
        for (username, password, expected) in [
            (UNKNOWN_USERNAME, CORRECT_PASSWORD, StatusCode::UNAUTHORIZED),
            (
                INACTIVE_USERNAME,
                CORRECT_PASSWORD,
                StatusCode::UNAUTHORIZED,
            ),
            (ACTIVE_USERNAME, WRONG_PASSWORD, StatusCode::UNAUTHORIZED),
            (ACTIVE_USERNAME, CORRECT_PASSWORD, StatusCode::OK),
        ] {
            let surface = AuthSurface::new();
            let response = login(&surface, username, password).await;
            assert_eq!(response.status, expected, "{username}");
            assert_eq!(
                surface.engine.verifications(),
                1,
                "{username} must cost exactly one verification"
            );
            assert_eq!(surface.engine.hashes(), 0, "{username}");
        }
    }

    /// No denial names an account through its code or any header.
    #[tokio::test]
    async fn no_denial_reveals_account_existence_in_its_code_or_headers() {
        let surface = AuthSurface::new();
        let mut heads = Vec::new();
        for (username, password) in [
            (UNKNOWN_USERNAME, CORRECT_PASSWORD),
            (INACTIVE_USERNAME, CORRECT_PASSWORD),
            (ACTIVE_USERNAME, WRONG_PASSWORD),
        ] {
            let response = login(&surface, username, password).await;
            let correlation = correlation_of(&response);
            assert_eq!(
                body_text(&response),
                format!(
                    "{{\"error\":\"authentication_failed\",\"correlation_id\":\"{correlation}\"}}"
                ),
                "{username}"
            );
            assert!(response.allow.is_none(), "{username}");

            let wire = rendered(&response).await;
            let head = wire
                .split_once("\r\n\r\n")
                .expect("a rendered response has a head")
                .0
                .to_owned();
            for account in [ACTIVE_USERNAME, INACTIVE_USERNAME, UNKNOWN_USERNAME] {
                assert!(!wire.contains(account), "{username} leaked {account}");
            }
            heads.push(head);
        }

        assert_eq!(heads[0], heads[1]);
        assert_eq!(heads[1], heads[2]);
        assert_eq!(
            heads[0],
            "HTTP/1.1 401 \r\nContent-Type: application/json; charset=utf-8"
        );
    }

    // -----------------------------------------------------------------------
    // Cookies on real responses
    // -----------------------------------------------------------------------

    /// A successful login emits exactly the two approved cookie lines.
    #[tokio::test]
    async fn a_successful_login_emits_exactly_the_two_approved_cookie_lines() {
        let surface = AuthSurface::new();
        let response = login(&surface, ACTIVE_USERNAME, CORRECT_PASSWORD).await;
        assert_eq!(response.status, StatusCode::OK);

        let wire = rendered(&response).await;
        let lines = set_cookie_lines(&wire);
        assert_eq!(lines.len(), 2);

        let session = cookie_value(&wire, SESSION_COOKIE_NAME);
        let csrf = cookie_value(&wire, CSRF_COOKIE_NAME);
        assert_eq!(session.len(), SESSION_TOKEN_TEXT_BYTES);
        assert_eq!(csrf.len(), SESSION_TOKEN_TEXT_BYTES);
        assert_ne!(session, csrf);

        assert_eq!(
            lines[0],
            format!(
                "Set-Cookie: {SESSION_COOKIE_NAME}={session}; \
                 Secure; HttpOnly; SameSite=Strict; Path=/"
            )
        );
        assert_eq!(
            lines[1],
            format!("Set-Cookie: {CSRF_COOKIE_NAME}={csrf}; Secure; SameSite=Strict; Path=/")
        );
        assert!(
            !lines[1].contains("HttpOnly"),
            "the CSRF cookie must be readable by the Client Module"
        );
        for line in &lines {
            for attribute in ["Domain", "Max-Age", "Expires"] {
                assert!(!line.contains(attribute), "{line} carries {attribute}");
            }
        }
        assert_eq!(
            body_text(&response),
            format!(
                "{{\"result\":{{\"authenticated\":true}},\"correlation_id\":\"{}\"}}",
                correlation_of(&response)
            )
        );
    }

    /// A failed login emits no cookie at all.
    #[tokio::test]
    async fn a_failed_login_emits_no_cookie_at_all() {
        let surface = AuthSurface::new();
        for (username, password, client_module) in [
            (UNKNOWN_USERNAME, CORRECT_PASSWORD, CLIENT_MODULE),
            (INACTIVE_USERNAME, CORRECT_PASSWORD, CLIENT_MODULE),
            (ACTIVE_USERNAME, WRONG_PASSWORD, CLIENT_MODULE),
            (ACTIVE_USERNAME, CORRECT_PASSWORD, "unregistered-module"),
        ] {
            let body = login_body(username, password, client_module);
            let response = request(
                surface.surface(),
                default_timeouts(),
                login_head(body.len()),
                body,
            )
            .await;
            assert_ne!(response.status, StatusCode::OK, "{username}");
            assert!(response.cookies.is_none(), "{username}");
            let wire = rendered(&response).await;
            assert!(!wire.contains("Set-Cookie"), "{username}");
        }
    }

    /// An invalid cookie effect redacts to the fixed failure with no cookie.
    ///
    /// A cookie effect is only valid on a typed envelope. The two invalid
    /// compositions here are the constructible ones, and both must redact
    /// rather than drop the cookie and keep the route's body. The last section
    /// pins the two fail-closed bounds by rendering the largest values the
    /// closed types accept: no constructible envelope or effect can exceed
    /// them, which is exactly why they are the guard rails for a future
    /// attribute or field change.
    #[tokio::test]
    async fn an_invalid_or_oversized_cookie_effect_redacts_and_emits_no_cookie() {
        for media_type in ["application/json; charset=utf-8", "text/plain"] {
            let mut response =
                axum::response::Response::new(axum::body::Body::from("{\"error\":\"not_found\"}"));
            *response.status_mut() = StatusCode::OK;
            response.headers_mut().insert(
                axum::http::header::CONTENT_TYPE,
                axum::http::HeaderValue::from_static("text/plain"),
            );
            response.headers_mut().insert(
                axum::http::header::CONTENT_TYPE,
                axum::http::HeaderValue::from_str(media_type)
                    .expect("the test media type must be a valid header value"),
            );
            response
                .extensions_mut()
                .insert(CookieEffect::IssueSession {
                    session: cookie_token("session"),
                    csrf: cookie_token("csrf"),
                });

            let bounded = bounded_response_from_axum(response).await;
            assert_eq!(bounded.status, StatusCode::OK, "{media_type}");
            assert_eq!(
                body_text(&bounded),
                "{\"error\":\"gateway_timeout\"}",
                "{media_type}"
            );
            assert!(bounded.cookies.is_none(), "{media_type}");
            let wire = rendered(&bounded).await;
            assert!(!wire.contains("Set-Cookie"), "{media_type}");
        }

        let widest = "a".repeat(MAX_STABLE_CODE_BYTES);
        let mut result = TypedResult::new();
        for index in 0..4_usize {
            let field = format!("{}{index}", "f".repeat(MAX_STABLE_CODE_BYTES - 1));
            result = result
                .with_field(
                    StableCode::new(&field).expect("the widest field name must be accepted"),
                    TypedValue::Code(
                        StableCode::new(&widest).expect("the widest code must be accepted"),
                    ),
                )
                .expect("four fields are within the field bound");
        }
        let envelope = TypedJsonEnvelope::Result {
            result,
            correlation_id: ResponseCorrelation::new(&"c".repeat(64))
                .expect("the widest correlation identifier must be accepted"),
        };
        assert!(
            envelope.serialize().len() <= MAX_TYPED_JSON_BODY_BYTES,
            "no constructible typed envelope may exceed the listener's bound"
        );

        let mut response = axum::response::Response::new(axum::body::Body::empty());
        response.extensions_mut().insert(envelope);
        response
            .extensions_mut()
            .insert(CookieEffect::IssueSession {
                session: cookie_token(&"s".repeat(48)),
                csrf: cookie_token(&"c".repeat(48)),
            });
        let bounded = bounded_response_from_axum(response).await;
        assert_ne!(body_text(&bounded), "{\"error\":\"gateway_timeout\"}");
        assert_eq!(set_cookie_lines(&rendered(&bounded).await).len(), 2);
    }

    fn cookie_token(value: &str) -> CookieValue {
        CookieValue::new(value).expect("the test cookie value must be accepted")
    }

    /// Logout deletes both cookies, and only deletion carries an expiry.
    #[tokio::test]
    async fn logout_deletes_both_cookies_with_the_only_expiry_attribute() {
        let surface = AuthSurface::new();
        let issued = login(&surface, ACTIVE_USERNAME, CORRECT_PASSWORD).await;
        let issued_wire = rendered(&issued).await;
        assert!(!issued_wire.contains("Max-Age"));
        assert!(!issued_wire.contains("Expires"));

        let session = cookie_value(&issued_wire, SESSION_COOKIE_NAME);
        let csrf = cookie_value(&issued_wire, CSRF_COOKIE_NAME);
        let response = request(
            surface.surface(),
            default_timeouts(),
            session_head(AUTH_LOGOUT_ROUTE, &session, &csrf),
            String::new(),
        )
        .await;

        assert_eq!(response.status, StatusCode::OK);
        let wire = rendered(&response).await;
        assert_eq!(
            set_cookie_lines(&wire),
            vec![
                format!(
                    "Set-Cookie: {SESSION_COOKIE_NAME}=; \
                     Secure; HttpOnly; SameSite=Strict; Path=/; Max-Age=0"
                ),
                format!(
                    "Set-Cookie: {CSRF_COOKIE_NAME}=; \
                     Secure; SameSite=Strict; Path=/; Max-Age=0"
                ),
            ]
        );
        assert!(!wire.contains("Expires"));
        assert_eq!(
            body_text(&response),
            format!(
                "{{\"result\":{{\"session\":\"ended\"}},\"correlation_id\":\"{}\"}}",
                correlation_of(&response)
            )
        );
    }

    /// Logging in again rotates the cross-site request forgery token.
    #[tokio::test]
    async fn the_csrf_token_rotates_on_every_login() {
        let surface = AuthSurface::new();
        let (first_session, first_csrf) = established_session(&surface).await;
        let (second_session, second_csrf) = established_session(&surface).await;

        assert_ne!(first_csrf, second_csrf);
        assert_ne!(first_session, second_session);

        // The first session's own token still belongs to it, so the rotation
        // issued a new pair rather than invalidating the header check.
        let response = request(
            surface.surface(),
            default_timeouts(),
            session_head(AUTH_SESSION_ROUTE, &first_session, &second_csrf),
            String::new(),
        )
        .await;
        assert_eq!(response.status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            body_text(&response),
            format!(
                "{{\"error\":\"session_invalid\",\"correlation_id\":\"{}\"}}",
                correlation_of(&response)
            )
        );
    }

    // -----------------------------------------------------------------------
    // Request preconditions
    // -----------------------------------------------------------------------

    /// A mutating request needs its own CSRF token and an exact same origin.
    #[tokio::test]
    async fn a_mutating_request_requires_its_csrf_token_and_exact_same_origin() {
        let surface = AuthSurface::new();
        let (session, csrf) = established_session(&surface).await;
        let trusted = format!("https://{UNBOUND_LISTENER}");

        for target in [AUTH_SESSION_ROUTE, AUTH_LOGOUT_ROUTE] {
            let missing = request(
                surface.surface(),
                default_timeouts(),
                session_head_with(target, &trusted, &session, None),
                String::new(),
            )
            .await;
            assert_eq!(missing.status, StatusCode::UNAUTHORIZED, "{target}");
            assert!(body_text(&missing).contains("session_invalid"), "{target}");
            assert!(missing.cookies.is_none(), "{target}");

            let mismatched = request(
                surface.surface(),
                default_timeouts(),
                session_head_with(target, &trusted, &session, Some("not-this-sessions-token")),
                String::new(),
            )
            .await;
            assert_eq!(mismatched.status, StatusCode::UNAUTHORIZED, "{target}");
            assert!(
                body_text(&mismatched).contains("session_invalid"),
                "{target}"
            );

            for origin in [
                "https://127.0.0.1:9443",
                "https://127.0.0.2:8443",
                "http://127.0.0.1:8443",
                "https://weavelit.example:8443",
            ] {
                let denied = request(
                    surface.surface(),
                    default_timeouts(),
                    session_head_with(target, origin, &session, Some(&csrf)),
                    String::new(),
                )
                .await;
                assert_eq!(denied.status, StatusCode::FORBIDDEN, "{target} {origin}");
                assert!(
                    body_text(&denied).contains("request_origin_denied"),
                    "{target} {origin}"
                );
                assert!(denied.cookies.is_none(), "{target} {origin}");
            }
        }

        // The correct pair on the correct origin still succeeds, so the
        // rejections above are the checks and not a broken request shape.
        let accepted = request(
            surface.surface(),
            default_timeouts(),
            session_head(AUTH_SESSION_ROUTE, &session, &csrf),
            String::new(),
        )
        .await;
        assert_eq!(accepted.status, StatusCode::OK);
    }

    /// Login bootstrap requires same origin plus the literal CSRF header.
    ///
    /// Each rejection here is answered with the fixed pre-body body, which
    /// carries no correlation identifier. That is the proof the request was
    /// refused before the route ran and before a body could be allocated.
    #[tokio::test]
    async fn login_bootstrap_requires_same_origin_and_the_literal_csrf_header() {
        let surface = AuthSurface::new();
        let body = login_body(ACTIVE_USERNAME, CORRECT_PASSWORD, CLIENT_MODULE);
        let trusted = format!("https://{UNBOUND_LISTENER}");

        for (origin, csrf) in [
            (trusted.as_str(), None),
            (trusted.as_str(), Some("0")),
            (trusted.as_str(), Some("2")),
            (trusted.as_str(), Some("11")),
            ("https://127.0.0.1:9443", Some("1")),
            ("http://127.0.0.1:8443", Some("1")),
            ("https://weavelit.example:8443", Some("1")),
        ] {
            let response = request(
                surface.surface(),
                default_timeouts(),
                login_head_with(origin, csrf, body.len()),
                body.clone(),
            )
            .await;
            assert_eq!(response.status, StatusCode::FORBIDDEN, "{origin} {csrf:?}");
            assert_eq!(
                body_text(&response),
                "{\"error\":\"request_origin_denied\"}",
                "{origin} {csrf:?}"
            );
            assert!(response.cookies.is_none(), "{origin} {csrf:?}");
            assert_eq!(
                surface.engine.verifications(),
                0,
                "{origin} {csrf:?} must be refused before any verification"
            );
        }

        let accepted = request(
            surface.surface(),
            default_timeouts(),
            login_head_with(&trusted, Some("1"), body.len()),
            body,
        )
        .await;
        assert_eq!(accepted.status, StatusCode::OK);
        assert_eq!(surface.engine.verifications(), 1);
    }

    // -----------------------------------------------------------------------
    // Session validation
    // -----------------------------------------------------------------------

    /// Validation reports the account and Client Module and nothing else.
    #[tokio::test]
    async fn session_validation_reports_only_the_account_and_client_module() {
        let surface = AuthSurface::new();
        let (session, csrf) = established_session(&surface).await;
        let response = request(
            surface.surface(),
            default_timeouts(),
            session_head(AUTH_SESSION_ROUTE, &session, &csrf),
            String::new(),
        )
        .await;

        assert_eq!(response.status, StatusCode::OK);
        assert!(response.cookies.is_none());
        assert_eq!(
            body_text(&response),
            format!(
                "{{\"result\":{{\"account_id\":\"{}\",\"client_module\":\"{CLIENT_MODULE}\"}},\
                 \"correlation_id\":\"{}\"}}",
                hexadecimal(&ACTIVE_ACCOUNT_BYTES),
                correlation_of(&response)
            )
        );
        for absent in [
            "group",
            "grant",
            "role",
            "permission",
            "operation",
            "username",
            "alice",
        ] {
            assert!(!body_text(&response).contains(absent), "{absent}");
        }
    }

    /// An expired or revoked session is refused at the route.
    ///
    /// The clock is injected, so every instant here is chosen rather than
    /// waited for. The absolute case is touched just inside the idle limit
    /// until the absolute limit is the only limit that can have elapsed.
    #[tokio::test]
    async fn an_expired_or_revoked_session_is_refused_at_the_route() {
        let surface = AuthSurface::new();
        let (session, csrf) = established_session(&surface).await;
        surface.set_clock(ISSUED_AT + SESSION_IDLE_TIMEOUT_MILLISECONDS);
        assert_session_invalid(&surface, &session, &csrf).await;

        let surface = AuthSurface::new();
        let (session, csrf) = established_session(&surface).await;
        let step = SESSION_IDLE_TIMEOUT_MILLISECONDS - 1;
        let mut elapsed = 0;
        while elapsed + step < SESSION_ABSOLUTE_LIFETIME_MILLISECONDS {
            elapsed += step;
            surface.set_clock(ISSUED_AT + elapsed);
            let touched = request(
                surface.surface(),
                default_timeouts(),
                session_head(AUTH_SESSION_ROUTE, &session, &csrf),
                String::new(),
            )
            .await;
            assert_eq!(touched.status, StatusCode::OK, "at {elapsed} milliseconds");
        }
        assert!(
            elapsed + SESSION_IDLE_TIMEOUT_MILLISECONDS > SESSION_ABSOLUTE_LIFETIME_MILLISECONDS,
            "the session must still be inside its idle limit when the absolute limit elapses"
        );
        surface.set_clock(ISSUED_AT + SESSION_ABSOLUTE_LIFETIME_MILLISECONDS);
        assert_session_invalid(&surface, &session, &csrf).await;

        let surface = AuthSurface::new();
        let (session, csrf) = established_session(&surface).await;
        let ended = request(
            surface.surface(),
            default_timeouts(),
            session_head(AUTH_LOGOUT_ROUTE, &session, &csrf),
            String::new(),
        )
        .await;
        assert_eq!(ended.status, StatusCode::OK);
        assert_session_invalid(&surface, &session, &csrf).await;
    }

    /// Asserts both session-bearing routes refuse the presented session.
    async fn assert_session_invalid(surface: &AuthSurface, session: &str, csrf: &str) {
        for target in [AUTH_SESSION_ROUTE, AUTH_LOGOUT_ROUTE] {
            let response = request(
                surface.surface(),
                default_timeouts(),
                session_head(target, session, csrf),
                String::new(),
            )
            .await;
            assert_eq!(response.status, StatusCode::UNAUTHORIZED, "{target}");
            assert_eq!(
                body_text(&response),
                format!(
                    "{{\"error\":\"session_invalid\",\"correlation_id\":\"{}\"}}",
                    correlation_of(&response)
                ),
                "{target}"
            );
            assert!(response.cookies.is_none(), "{target}");
        }
    }

    // -----------------------------------------------------------------------
    // Logging
    // -----------------------------------------------------------------------

    /// A denial records its System Log entry before the response is returned.
    #[tokio::test]
    async fn a_denial_records_its_system_log_entry_before_it_answers() {
        let (destination, delivered) = recording_log(false);
        let surface = AuthSurface::with_log(destination);

        let response = login(&surface, ACTIVE_USERNAME, WRONG_PASSWORD).await;
        assert_eq!(response.status, StatusCode::UNAUTHORIZED);
        let correlation = correlation_of(&response);

        // Read without waiting: the record must already be delivered by the
        // time this response exists.
        let records = delivered
            .lock()
            .expect("the delivered record log must not poison")
            .clone();
        assert_eq!(
            records,
            vec![DeliveredRecord {
                correlation_id: correlation,
                classification: "authentication.failure".to_owned(),
                detail: "local password authentication denied".to_owned(),
            }]
        );

        let accepted = login(&surface, ACTIVE_USERNAME, CORRECT_PASSWORD).await;
        assert_eq!(accepted.status, StatusCode::OK);
        assert_eq!(
            delivered
                .lock()
                .expect("the delivered record log must not poison")
                .len(),
            1,
            "an accepted login records no authentication failure"
        );
    }

    /// A System Log delivery failure never reaches the response.
    #[tokio::test]
    async fn a_log_delivery_failure_does_not_change_the_denial() {
        let (failing, failed) = recording_log(true);
        let (working, worked) = recording_log(false);

        let mut wire = Vec::new();
        for surface in [
            AuthSurface::with_log(failing),
            AuthSurface::with_log(working),
            AuthSurface::new(),
        ] {
            let response = login(&surface, ACTIVE_USERNAME, WRONG_PASSWORD).await;
            assert_eq!(response.status, StatusCode::UNAUTHORIZED);
            assert!(response.cookies.is_none());
            let correlation = correlation_of(&response);
            wire.push(normalized(&rendered(&response).await, &correlation));
        }

        assert_eq!(
            failed
                .lock()
                .expect("the delivered record log must not poison")
                .len(),
            1,
            "the failing destination must have been reached"
        );
        assert_eq!(
            worked
                .lock()
                .expect("the delivered record log must not poison")
                .len(),
            1
        );
        assert_eq!(wire[0], wire[1]);
        assert_eq!(wire[1], wire[2]);
        assert!(!wire[0].contains("log"));
        assert_eq!(
            wire[0],
            "HTTP/1.1 401 \r\nContent-Type: application/json; charset=utf-8\r\n\r\n\
             {\"error\":\"authentication_failed\",\"correlation_id\":\"<correlation>\"}"
        );
    }

    // -----------------------------------------------------------------------
    // Frozen pre-operational surface
    // -----------------------------------------------------------------------

    /// The pre-operational surface is unchanged and mounts no login route.
    #[tokio::test]
    async fn the_preoperational_surface_is_frozen_and_mounts_no_login_route() {
        let root = tempfile::tempdir().expect("the test state root must be created");
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700))
            .expect("the test state root must be private");
        let state_root = root
            .path()
            .canonicalize()
            .expect("the test state root must resolve");
        let startup =
            classify_restricted_startup(&state_root).expect("an empty state root must classify");
        assert_eq!(
            startup.outcome(),
            StartupOutcome::UninitializedWithoutDatabase
        );

        let (_switch, modes) = published_serving_modes(
            &startup,
            UNBOUND_LISTENER
                .parse()
                .expect("the test listener authority must parse"),
        );
        let surface = modes.borrow().surface().clone();

        assert!(
            !surface
                .registry()
                .registered_routes()
                .iter()
                .any(
                    |(_, target)| [AUTH_LOGIN_ROUTE, AUTH_SESSION_ROUTE, AUTH_LOGOUT_ROUTE]
                        .contains(target)
                ),
            "the pre-operational surface must mount no authentication route"
        );

        for (target, expected) in [
            (
                "/api/v1/status",
                "HTTP/1.1 200 \r\nContent-Type: application/json; charset=utf-8\r\n\r\n\
                 {\"lifecycle\":\"uninitialized\",\"database_selected\":false}",
            ),
            (
                "/api/v1/application-database",
                "HTTP/1.1 405 \r\nContent-Type: application/json; charset=utf-8\r\n\
                 Allow: PUT\r\n\r\n{\"error\":\"method_not_allowed\"}",
            ),
        ] {
            let head = format!("GET {target} HTTP/1.1\r\nHost: {UNBOUND_LISTENER}\r\n\r\n");
            let response = request(surface.clone(), default_timeouts(), head, String::new()).await;
            assert_eq!(rendered(&response).await, expected, "{target}");
        }

        let login = request(
            surface,
            default_timeouts(),
            login_head_with(&format!("https://{UNBOUND_LISTENER}"), Some("1"), 0),
            String::new(),
        )
        .await;
        assert_eq!(login.status, StatusCode::NOT_FOUND);
        assert_eq!(body_text(&login), "{\"error\":\"not_found\"}");
    }
}
