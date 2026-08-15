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
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Router,
    http::{HeaderMap, Method, Uri},
};
use tokio::{sync::Semaphore, task};
use weavelit_module_client::{
    AUTH_LOGIN_ROUTE, AUTH_LOGOUT_ROUTE, AUTH_MFA_ENROLLMENT_CONFIRM_ROUTE,
    AUTH_MFA_ENROLLMENT_ROUTE, AUTH_MFA_SELF_ENROLLMENT_ROUTE, AUTH_MFA_VERIFY_ROUTE,
    AUTH_SESSION_ROUTE, AuthenticationCapability, AuthenticationDeclaration,
    AuthenticationRejection, ExpectedOrigin, LoginOutcome, LoginSubmission, MfaCapability,
    MfaCodeSubmission, MfaDeclaration, MfaEnrollmentOpened, MfaEnrollmentSubmission,
    MfaSelfEnrollmentSubmission, SessionEstablished, SessionIdentity, SessionSubmission,
    mfa::{validate_mfa_request, validate_mfa_session_request},
    typed_json::{MAX_PROVISIONING_URI_BYTES, ProvisioningUri},
    validate_login_request,
};
use weavelit_module_mfa_totp::{SECRET_LENGTH, TotpSecret};
use weavelit_server_authentication::{
    ACCEPTED_ARGON2_PROFILES, Argon2Engine, Continuation, ContinuationDigest, CsrfTokenDigest,
    PasswordAuthenticator, PasswordPolicy, PasswordVerdict, RustCryptoArgon2, SessionSecrets,
    SessionTokenDigest, StoredCredential,
};
use weavelit_server_database::{
    Account, ApplicationState, DatabaseError, DeploymentIdentifier, InitializedState, LogType,
    MfaAcceptance, MfaEnablementOutcome, MfaEnrollment, MfaFactor, MfaModuleTarget, MfaStore,
    MfaTimeStep, Name, NewSession, SessionCsrfHash, SessionInstant, SessionStore, SessionTokenHash,
    SessionValidation, StateIdentifier, StoredSession,
};
use weavelit_server_lifecycle::{ProtectedValueAccess, ProtectedValueKind};
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

/// The MFA Module this build verifies second factors through.
///
/// A factor records this name, so a factor enrolled for another module is not
/// a second factor this Server can verify and is not treated as enrollment.
const TOTP_MODULE: &str = weavelit_module_mfa_totp::MODULE_IDENTIFIER;

/// The configuration component that owns the TOTP Module's enabled setting.
pub const TOTP_COMPONENT: &str = "mfa.totp";

/// The setting key an MFA Module's enabled state is stored under.
const MFA_ENABLED_KEY: &str = "enabled";

/// The stored value that enables an MFA Module.
///
/// Anything else, including a missing entry, leaves the module disabled, so a
/// deployment whose configuration was truncated or never written verifies no
/// second factor rather than failing open in either direction.
const MFA_ENABLED_VALUE: &str = "true";

/// The issuer label an enrollment's provisioning URI carries.
const PROVISIONING_ISSUER: &str = "Weavelit";

/// How long a continuation stays claimable, in milliseconds.
///
/// A continuation is a verified password waiting for its second factor, so it
/// is short-lived: past this window the caller logs in again rather than
/// holding an indefinitely resumable half-authentication.
const CONTINUATION_LIFETIME_MILLISECONDS: i64 = 5 * 60 * 1000;

/// Continuations retained at one time.
///
/// Each one is issued only by a verified password or a verified enrollment
/// step, so the bound is reached only by real successful verifications. It
/// exists so the store cannot grow without limit.
const MAX_OUTSTANDING_CONTINUATIONS: usize = 256;

const _: () = assert!(
    CONTINUATION_LIFETIME_MILLISECONDS > 0 && MAX_OUTSTANDING_CONTINUATIONS > 0,
    "a continuation store that retains nothing, or retains forever, is not a bounded handoff"
);

/// Reads the current UTC time in Unix milliseconds.
///
/// Injected so a test observes session lifetime decisions at chosen instants
/// without waiting for real time to pass.
pub type WallClock = Arc<dyn Fn() -> Option<i64> + Send + Sync>;

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
pub struct AuthenticationRuntime<E> {
    database: OperationalDatabase,
    deployment: DeploymentIdentifier,
    client_modules: BTreeSet<Name>,
    authenticator: PasswordAuthenticator<E>,
    login_lane: Arc<Semaphore>,
    clock: WallClock,
    observability: ServerObservability,
    /// The deployment's at-rest protection for enrolled factor data.
    ///
    /// Sealing and opening pass through the same lifecycle authority a Restore
    /// holds while it replaces state, so a factor cannot be sealed against a
    /// key generation a running workflow is replacing.
    protection: Arc<dyn ProtectedValueAccess>,
    /// The half-authenticated logins waiting for their second factor.
    continuations: ContinuationStore,
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
        protection: Arc<dyn ProtectedValueAccess>,
    ) -> Option<Arc<Self>> {
        Self::with_engine(
            RustCryptoArgon2::new(PasswordPolicy::approved()),
            database,
            state,
            client_modules,
            system_clock(),
            open_system_log(state, state_root, log_catalog).map(Arc::new),
            protection,
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
        protection: Arc<dyn ProtectedValueAccess>,
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
            protection,
            continuations: ContinuationStore::default(),
            system_log,
        }))
    }

    /// Returns the authentication routes, each paired with the transport
    /// registration that admits it.
    ///
    /// Only the two routes that verify a password declare the single-permit
    /// admission lane. A second-factor verification costs one HMAC rather than
    /// one Argon2 profile ceiling, so admitting it into that lane would make a
    /// held password verification block a code that reserves no such memory.
    pub(crate) fn capabilities(
        self: &Arc<Self>,
        expected_origin: ExpectedOrigin,
    ) -> Vec<TransportCapability> {
        let declaration = Arc::new(AuthenticationDeclaration::new(
            self.capability(expected_origin),
        ));
        let session = Arc::clone(&declaration);
        let logout = Arc::clone(&declaration);

        let mfa = Arc::new(MfaDeclaration::new(self.mfa_capability(expected_origin)));
        let enrollment = Arc::clone(&mfa);
        let self_enrollment = Arc::clone(&mfa);
        let confirm = Arc::clone(&mfa);

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
            TransportCapability::new(
                TransportRegistration::new(
                    Method::PUT,
                    AUTH_MFA_VERIFY_ROUTE,
                    TransportProfile::DEFAULT,
                )
                .with_pre_body_check(Arc::new(MfaPreconditions { expected_origin })),
                move |router: Router| router.route(AUTH_MFA_VERIFY_ROUTE, mfa.verify_route()),
            ),
            TransportCapability::new(
                TransportRegistration::new(
                    Method::PUT,
                    AUTH_MFA_ENROLLMENT_ROUTE,
                    TransportProfile::DEFAULT,
                )
                .with_pre_body_check(Arc::new(MfaPreconditions { expected_origin })),
                move |router: Router| {
                    router.route(AUTH_MFA_ENROLLMENT_ROUTE, enrollment.enrollment_route())
                },
            ),
            TransportCapability::new(
                TransportRegistration::new(
                    Method::PUT,
                    AUTH_MFA_SELF_ENROLLMENT_ROUTE,
                    TransportProfile::DEFAULT,
                )
                .with_pre_body_check(Arc::new(MfaSessionPreconditions { expected_origin }))
                // Self-enrollment re-verifies the account's password, so it
                // enters the same single verification lane a login does.
                .with_admission(Arc::clone(&self.login_lane)),
                move |router: Router| {
                    router.route(
                        AUTH_MFA_SELF_ENROLLMENT_ROUTE,
                        self_enrollment.self_enrollment_route(),
                    )
                },
            ),
            TransportCapability::new(
                TransportRegistration::new(
                    Method::PUT,
                    AUTH_MFA_ENROLLMENT_CONFIRM_ROUTE,
                    TransportProfile::DEFAULT,
                )
                .with_pre_body_check(Arc::new(MfaPreconditions { expected_origin })),
                move |router: Router| {
                    router.route(
                        AUTH_MFA_ENROLLMENT_CONFIRM_ROUTE,
                        confirm.enrollment_confirm_route(),
                    )
                },
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

    /// Binds this runtime's second-factor decisions to the route contract.
    fn mfa_capability(self: &Arc<Self>, expected_origin: ExpectedOrigin) -> MfaCapability {
        let verifying = Arc::clone(self);
        let opening = Arc::clone(self);
        let self_opening = Arc::clone(self);
        let confirming = Arc::clone(self);

        MfaCapability {
            expected_origin,
            correlate: Arc::new(correlation_identifier),
            verify: Arc::new(move |submission| {
                let runtime = Arc::clone(&verifying);
                Box::pin(async move { runtime.verify_second_factor(submission).await })
            }),
            open_enrollment: Arc::new(move |submission| {
                let runtime = Arc::clone(&opening);
                Box::pin(async move { runtime.open_enrollment(submission).await })
            }),
            open_self_enrollment: Arc::new(move |submission| {
                let runtime = Arc::clone(&self_opening);
                Box::pin(async move { runtime.open_self_enrollment(submission).await })
            }),
            confirm_enrollment: Arc::new(move |submission| {
                let runtime = Arc::clone(&confirming);
                Box::pin(async move { runtime.confirm_enrollment(submission).await })
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
    ) -> Result<LoginOutcome, AuthenticationRejection> {
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
    ) -> Result<LoginOutcome, AuthenticationRejection> {
        // Rejected before any credential work, and independently of the
        // submitted account, so it discloses nothing about an account.
        let Some(client_module) = Name::new(client_module)
            .ok()
            .filter(|name| self.client_modules.contains(name))
        else {
            return Err(AuthenticationRejection::BadRequest);
        };
        let state = self.initialized_state()?;

        match self.verify_password(state.state(), username, password.as_bytes()) {
            PasswordOutcome::Verified { account } => {
                self.admit(state.state(), account, &client_module, correlation_id)
            }
            PasswordOutcome::Denied => {
                // Delivery is attempted before the denial is returned, and every
                // failure inside is absorbed, so the System Log records the
                // attempt without being able to change what the caller answers.
                self.record_denial(correlation_id);
                Err(AuthenticationRejection::AuthenticationFailed)
            }
        }
    }

    /// Decides what a verified password is admitted to.
    ///
    /// The decision is exactly three inputs: whether the MFA Module is enabled
    /// for the deployment, whether this account holds a factor for it, and
    /// whether this account is required to hold one. Every combination is
    /// decided here, so no caller can reach session issuance by skipping one.
    ///
    /// A required account whose module is disabled is denied. The deployment
    /// stated that the account must present a second factor and it currently
    /// cannot verify one, so admitting the account would silently drop a
    /// requirement rather than enforce it.
    fn admit(
        &self,
        state: &ApplicationState,
        account: StateIdentifier,
        client_module: &Name,
        correlation_id: &str,
    ) -> Result<LoginOutcome, AuthenticationRejection> {
        let enabled = module_enabled(state);
        let enrolled = enrolled_factor(state, account).is_some();
        let required = state
            .accounts()
            .iter()
            .find(|candidate| candidate.identifier == account)
            .is_some_and(|candidate| candidate.mfa_required);

        match (enabled, enrolled, required) {
            (true, true, _) => self
                .continuation(PendingClaim::SecondFactor {
                    account,
                    client_module: client_module.clone(),
                })
                .map(|continuation| LoginOutcome::SecondFactorRequired { continuation }),
            (true, false, true) => self
                .continuation(PendingClaim::Enrollment {
                    account,
                    client_module: client_module.clone(),
                })
                .map(|continuation| LoginOutcome::EnrollmentRequired { continuation }),
            (false, _, true) => {
                self.record_denial(correlation_id);
                Err(AuthenticationRejection::AuthenticationFailed)
            }
            (true | false, _, false) => self
                .issue_session(account, client_module)
                .map(LoginOutcome::SessionEstablished),
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
    // Second factor
    // -----------------------------------------------------------------------

    /// Verifies a time-based code against the continuation that requires it.
    ///
    /// No admission permit is taken. The work here is one decryption and one
    /// HMAC, neither of which reserves the memory the login lane exists to
    /// bound, so a code is answered while a password verification is running.
    async fn verify_second_factor(
        self: Arc<Self>,
        submission: MfaCodeSubmission,
    ) -> Result<SessionEstablished, AuthenticationRejection> {
        let MfaCodeSubmission {
            continuation,
            code,
            correlation_id,
            ..
        } = submission;

        task::spawn_blocking(move || {
            self.verify_second_factor_blocking(&continuation, &code, &correlation_id)
        })
        .await
        .unwrap_or(Err(AuthenticationRejection::ServiceUnavailable))
    }

    fn verify_second_factor_blocking(
        &self,
        continuation: &str,
        code: &str,
        correlation_id: &str,
    ) -> Result<SessionEstablished, AuthenticationRejection> {
        let now = self.milliseconds()?;
        // Claimed before the code is examined, so one continuation admits one
        // attempt whether or not the code was right.
        let Some(PendingClaim::SecondFactor {
            account,
            client_module,
        }) = self.continuations.claim(continuation, now)
        else {
            return Err(self.deny(correlation_id));
        };

        let state = self.initialized_state()?;
        if !module_enabled(state.state()) {
            return Err(self.deny(correlation_id));
        }
        let Some(factor) = enrolled_factor(state.state(), account) else {
            return Err(self.deny(correlation_id));
        };
        let secret = self.open_factor(factor)?;
        let Some(step) = secret.verify(code, unix_seconds(now)?) else {
            return Err(self.deny(correlation_id));
        };
        let step = MfaTimeStep::from_step(step.as_u64())
            .map_err(|_| AuthenticationRejection::ServiceUnavailable)?;

        // The watermark decides replay atomically, so a code presented twice
        // inside its own step is accepted at most once.
        match self.with_mfa(|store| store.accept_step(factor.identifier, step))? {
            MfaAcceptance::Accepted => self.issue_session(account, &client_module),
            MfaAcceptance::Replayed => Err(self.deny(correlation_id)),
        }
    }

    /// Opens an enrollment for a login that must enroll before it may proceed.
    async fn open_enrollment(
        self: Arc<Self>,
        submission: MfaEnrollmentSubmission,
    ) -> Result<MfaEnrollmentOpened, AuthenticationRejection> {
        let MfaEnrollmentSubmission {
            continuation,
            correlation_id,
            ..
        } = submission;

        task::spawn_blocking(move || self.open_enrollment_blocking(&continuation, &correlation_id))
            .await
            .unwrap_or(Err(AuthenticationRejection::ServiceUnavailable))
    }

    fn open_enrollment_blocking(
        &self,
        continuation: &str,
        correlation_id: &str,
    ) -> Result<MfaEnrollmentOpened, AuthenticationRejection> {
        let now = self.milliseconds()?;
        let Some(PendingClaim::Enrollment {
            account,
            client_module,
        }) = self.continuations.claim(continuation, now)
        else {
            return Err(self.deny(correlation_id));
        };

        let state = self.initialized_state()?;
        if !module_enabled(state.state()) {
            return Err(self.deny(correlation_id));
        }
        // An account that already holds a factor enrolls no second one, so an
        // enrollment continuation cannot be used to replace a live factor.
        if enrolled_factor(state.state(), account).is_some() {
            return Err(self.deny(correlation_id));
        }
        let Some(username) = account_username(state.state(), account) else {
            return Err(self.deny(correlation_id));
        };

        self.open_secret(account, client_module, &username, now)
    }

    /// Opens an enrollment for an account that is already signed in.
    ///
    /// The account re-enters its password, which is verified through the same
    /// single call every login uses, so enrolling a factor from a stolen
    /// session still costs the account's password.
    async fn open_self_enrollment(
        self: Arc<Self>,
        submission: MfaSelfEnrollmentSubmission,
    ) -> Result<MfaEnrollmentOpened, AuthenticationRejection> {
        let Some(admission) = submission.context.get::<BodyAdmission>().cloned() else {
            return Err(AuthenticationRejection::ServiceUnavailable);
        };
        let MfaSelfEnrollmentSubmission {
            session_token,
            csrf_token,
            password,
            correlation_id,
            ..
        } = submission;

        task::spawn_blocking(move || {
            let _admission = admission;
            self.open_self_enrollment_blocking(
                &session_token,
                &csrf_token,
                &password,
                &correlation_id,
            )
        })
        .await
        .unwrap_or(Err(AuthenticationRejection::ServiceUnavailable))
    }

    fn open_self_enrollment_blocking(
        &self,
        session_token: &str,
        csrf_token: &str,
        password: &str,
        correlation_id: &str,
    ) -> Result<MfaEnrollmentOpened, AuthenticationRejection> {
        let session = self.authorized_session(session_token, csrf_token)?.1;
        let now = self.milliseconds()?;
        let state = self.initialized_state()?;

        // Resolved before verification and handed to the same call even when it
        // resolves to nothing, so a session naming an account this state no
        // longer holds still costs exactly one verification.
        let username = account_username(state.state(), session.account());
        let submitted = username.as_ref().map_or("", Name::as_str);
        let verified = match self.verify_password(state.state(), submitted, password.as_bytes()) {
            PasswordOutcome::Verified { account } if account == session.account() => account,
            PasswordOutcome::Verified { .. } | PasswordOutcome::Denied => {
                self.record_denial(correlation_id);
                return Err(AuthenticationRejection::AuthenticationFailed);
            }
        };

        if !module_enabled(state.state()) || enrolled_factor(state.state(), verified).is_some() {
            return Err(self.deny(correlation_id));
        }
        let Some(username) = username else {
            return Err(self.deny(correlation_id));
        };

        self.open_secret(verified, session.client_module().clone(), &username, now)
    }

    /// Generates one enrollment secret and the ticket that confirms it.
    ///
    /// The secret is disclosed here and nowhere else. It is held only in the
    /// continuation this returns and is written to the database only once a
    /// code proves the caller holds it, so an abandoned enrollment leaves no
    /// factor behind.
    ///
    /// The provisioning URI is built and accepted into its response-bearing
    /// type before the confirmation ticket is issued. Nothing between the
    /// ticket and the returned response can refuse, so a caller either receives
    /// a ticket it can confirm or receives nothing and can open an enrollment
    /// again. Issuing the ticket first would let a refusal here burn the
    /// one-time claim that ticket names and leave an account that must enroll
    /// unable to.
    fn open_secret(
        &self,
        account: StateIdentifier,
        client_module: Name,
        username: &Name,
        now: i64,
    ) -> Result<MfaEnrollmentOpened, AuthenticationRejection> {
        let unavailable = AuthenticationRejection::ServiceUnavailable;
        let bytes = random_bytes::<SECRET_LENGTH>().ok_or(unavailable)?;
        let secret = TotpSecret::from_bytes(bytes);
        let base32 = secret.base32();
        let uri = secret
            .provisioning_uri(
                PROVISIONING_ISSUER,
                username.as_str(),
                MAX_PROVISIONING_URI_BYTES,
            )
            .map_err(|_| unavailable)?;
        let provisioning_uri = ProvisioningUri::new(uri.expose()).ok_or(unavailable)?;

        let enrollment = self.continuations.issue(
            PendingClaim::EnrollmentConfirm {
                account,
                client_module,
                secret: Zeroizing::new(bytes),
            },
            now,
        )?;

        Ok(MfaEnrollmentOpened {
            secret: Zeroizing::new(base32.expose().to_owned()),
            provisioning_uri,
            enrollment: Zeroizing::new(enrollment.as_str().to_owned()),
        })
    }

    /// Confirms an opened enrollment by verifying a code from its secret.
    async fn confirm_enrollment(
        self: Arc<Self>,
        submission: MfaCodeSubmission,
    ) -> Result<SessionEstablished, AuthenticationRejection> {
        let MfaCodeSubmission {
            continuation,
            code,
            correlation_id,
            ..
        } = submission;

        task::spawn_blocking(move || {
            self.confirm_enrollment_blocking(&continuation, &code, &correlation_id)
        })
        .await
        .unwrap_or(Err(AuthenticationRejection::ServiceUnavailable))
    }

    fn confirm_enrollment_blocking(
        &self,
        enrollment: &str,
        code: &str,
        correlation_id: &str,
    ) -> Result<SessionEstablished, AuthenticationRejection> {
        let now = self.milliseconds()?;
        let Some(PendingClaim::EnrollmentConfirm {
            account,
            client_module,
            secret,
        }) = self.continuations.claim(enrollment, now)
        else {
            return Err(self.deny(correlation_id));
        };

        let unavailable = AuthenticationRejection::ServiceUnavailable;
        let Some(step) = TotpSecret::from_bytes(*secret).verify(code, unix_seconds(now)?) else {
            return Err(self.deny(correlation_id));
        };
        let step = MfaTimeStep::from_step(step.as_u64()).map_err(|_| unavailable)?;

        let identifier = StateIdentifier::from_bytes(random_bytes::<16>().ok_or(unavailable)?)
            .map_err(|_| unavailable)?;
        let target = totp_target()?;
        let protected_factor_data = self
            .protection
            .seal(ProtectedValueKind::MfaFactorData, secret.as_slice())
            .map_err(|_| unavailable)?;
        let factor = MfaFactor {
            identifier,
            account,
            module: target.module.clone(),
            protected_factor_data,
        };

        // The factor and the watermark that already consumed the confirming
        // code are written together, so the code that enrolled the factor
        // cannot immediately be replayed against it. The module's enabled
        // state is read inside that same transaction, so an enrollment opened
        // while the module was enabled and confirmed after it was disabled
        // persists no factor and reaches no session issuance.
        match self.with_mfa(|store| store.enroll(&target, &factor, step))? {
            MfaEnrollment::Enrolled => self.issue_session(account, &client_module),
            MfaEnrollment::AlreadyEnrolled | MfaEnrollment::ModuleDisabled => {
                Err(self.deny(correlation_id))
            }
        }
    }

    /// Reports how many accounts currently hold a factor for the MFA Module.
    ///
    /// This is the preview an administrator decides against before disabling
    /// the module, and the same number is presented back to
    /// [`Self::set_module_enabled`] so the decision is checked against the
    /// count that was actually shown.
    pub fn enrolled_accounts(&self) -> Result<usize, AuthenticationRejection> {
        let state = self.initialized_state()?;
        let mut accounts: Vec<_> = state
            .state()
            .mfa_factors()
            .iter()
            .filter(|factor| factor.module.as_str() == TOTP_MODULE)
            .map(|factor| factor.account)
            .collect();
        accounts.sort_unstable_by_key(|account| *account.as_bytes());
        accounts.dedup();
        Ok(accounts.len())
    }

    /// Enables or disables the MFA Module against a previewed enrolled count.
    ///
    /// Disabling revokes every live session of every enrolled account, because
    /// those sessions were established behind a factor this deployment is no
    /// longer willing to verify.
    pub fn set_module_enabled(
        &self,
        enabled: bool,
        expected_enrolled: usize,
    ) -> Result<MfaEnablementOutcome, AuthenticationRejection> {
        let target = totp_target()?;
        self.with_mfa(|store| store.set_module_enabled(&target, enabled, expected_enrolled))
    }

    /// Recovers the enrolled secret one factor holds.
    fn open_factor(&self, factor: &MfaFactor) -> Result<TotpSecret, AuthenticationRejection> {
        let unavailable = AuthenticationRejection::ServiceUnavailable;
        let opened = self
            .protection
            .open(
                ProtectedValueKind::MfaFactorData,
                &factor.protected_factor_data,
            )
            .map_err(|_| unavailable)?;
        let bytes: [u8; SECRET_LENGTH] = opened.as_slice().try_into().map_err(|_| unavailable)?;
        Ok(TotpSecret::from_bytes(bytes))
    }

    /// Issues one continuation for a claim this runtime already decided.
    fn continuation(
        &self,
        claim: PendingClaim,
    ) -> Result<Zeroizing<String>, AuthenticationRejection> {
        let now = self.milliseconds()?;
        self.continuations
            .issue(claim, now)
            .map(|continuation| Zeroizing::new(continuation.as_str().to_owned()))
    }

    /// Records one denial and returns the single rejection every one answers.
    fn deny(&self, correlation_id: &str) -> AuthenticationRejection {
        self.record_denial(correlation_id);
        AuthenticationRejection::AuthenticationFailed
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
    ///
    /// The store compares the presented token inside the same atomic operation
    /// that resolves the session and advances its activity, so a request that
    /// fails the token check does not refresh the idle timeout.
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
        let SessionValidation::Valid(session) = self
            .with_sessions(|sessions| sessions.validate_and_touch(&token_hash, &presented, now))?
        else {
            return Err(invalid);
        };
        Ok((token_hash, session))
    }

    /// Validates a presented session for a later authorization decision.
    ///
    /// This is the only way to obtain a [`ValidatedSession`], so an
    /// authorization decision reaches its account and Client Module through a
    /// session this runtime validated rather than through a comment saying
    /// validation should have happened first.
    pub fn validated_session(
        &self,
        session_token: &str,
        csrf_token: &str,
    ) -> Result<ValidatedSession, AuthenticationRejection> {
        self.authorized_session(session_token, csrf_token)
            .map(|(_token_hash, session)| ValidatedSession::established(&session))
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

    /// Runs one operation against the live MFA store.
    ///
    /// A backend that serves no MFA store is refused rather than verifying a
    /// second factor without a durable replay watermark.
    fn with_mfa<R>(
        &self,
        operation: impl FnOnce(&mut dyn MfaStore) -> Result<R, DatabaseError>,
    ) -> Result<R, AuthenticationRejection> {
        self.database
            .with(|database| {
                database
                    .mfa()
                    .map_or(Err(DatabaseError::Unavailable), operation)
            })
            .map_err(|_| AuthenticationRejection::ServiceUnavailable)?
            .map_err(|_| AuthenticationRejection::ServiceUnavailable)
    }

    /// Loads the deployment's initialized application state.
    fn initialized_state(&self) -> Result<InitializedState, AuthenticationRejection> {
        self.database
            .with(|database| database.load_initialized_state(self.deployment))
            .map_err(|_| AuthenticationRejection::ServiceUnavailable)?
            .map_err(|_| AuthenticationRejection::ServiceUnavailable)
    }

    /// Reads the current instant sessions are decided against.
    fn now(&self) -> Result<SessionInstant, AuthenticationRejection> {
        SessionInstant::from_unix_milliseconds(self.milliseconds()?)
            .map_err(|_| AuthenticationRejection::ServiceUnavailable)
    }

    /// Reads the current instant in Unix milliseconds.
    fn milliseconds(&self) -> Result<i64, AuthenticationRejection> {
        (self.clock)().ok_or(AuthenticationRejection::ServiceUnavailable)
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
// Continuations
// ---------------------------------------------------------------------------

/// What one continuation entitles its holder to do next.
///
/// The claim is held here rather than encoded into the continuation itself, so
/// the value handed to the caller carries no account, no Client Module, and no
/// secret, and cannot be edited into a claim on something else.
enum PendingClaim {
    /// A verified password waiting for a code from an enrolled factor.
    SecondFactor {
        account: StateIdentifier,
        client_module: Name,
    },
    /// A verified password that must enroll a factor before it may proceed.
    Enrollment {
        account: StateIdentifier,
        client_module: Name,
    },
    /// An opened enrollment waiting for a code proving its secret was stored.
    EnrollmentConfirm {
        account: StateIdentifier,
        client_module: Name,
        /// Held only here until a code confirms it, so an abandoned enrollment
        /// leaves nothing durable behind.
        secret: Zeroizing<[u8; SECRET_LENGTH]>,
    },
}

/// One outstanding continuation and the instant it stops being claimable.
struct Pending {
    digest: ContinuationDigest,
    expires_at: i64,
    claim: PendingClaim,
}

/// The half-authenticated logins this Server is currently holding.
///
/// Only the digest is retained, so the store cannot reproduce a continuation it
/// issued. Claiming removes the entry before its caller examines any code, so
/// one continuation admits exactly one attempt. The store lives in memory: a
/// restart therefore invalidates every outstanding continuation, which is the
/// safe direction for a partially completed authentication.
#[derive(Default)]
struct ContinuationStore {
    pending: Mutex<Vec<Pending>>,
}

impl ContinuationStore {
    /// Issues one continuation for a claim the runtime already decided.
    fn issue(
        &self,
        claim: PendingClaim,
        now: i64,
    ) -> Result<Continuation, AuthenticationRejection> {
        let unavailable = AuthenticationRejection::ServiceUnavailable;
        let expires_at = now
            .checked_add(CONTINUATION_LIFETIME_MILLISECONDS)
            .ok_or(unavailable)?;
        let continuation = Continuation::generate().map_err(|_| unavailable)?;

        let mut pending = self.pending.lock().map_err(|_| unavailable)?;
        pending.retain(|entry| entry.expires_at > now);
        if pending.len() >= MAX_OUTSTANDING_CONTINUATIONS {
            return Err(unavailable);
        }
        pending.push(Pending {
            digest: continuation.digest(),
            expires_at,
            claim,
        });
        Ok(continuation)
    }

    /// Consumes the claim a submitted continuation entitles, when it is live.
    ///
    /// The entry is removed whatever the caller then decides, so a wrong code
    /// costs the continuation rather than allowing another attempt against it.
    fn claim(&self, submitted: &str, now: i64) -> Option<PendingClaim> {
        let submitted = ContinuationDigest::of(submitted);
        let mut pending = self.pending.lock().ok()?;
        pending.retain(|entry| entry.expires_at > now);
        let found = pending
            .iter()
            .position(|entry| entry.digest.matches(&submitted))?;
        Some(pending.swap_remove(found).claim)
    }
}

/// One session that has already passed validation.
///
/// The fields and the constructor are private to this module and the only call
/// site of that constructor is the success path of
/// [`AuthenticationRuntime::validated_session`], so no other module can produce
/// this value. An authorization decision that takes one therefore cannot be
/// reached before session validation.
///
/// It carries the account and the issuing Client Module and nothing else. No
/// Group, grant, or component enablement is captured here, because
/// authorization reads all of those live on every request.
#[derive(Debug)]
pub struct ValidatedSession {
    account: StateIdentifier,
    client_module: Name,
}

impl ValidatedSession {
    fn established(session: &StoredSession) -> Self {
        Self {
            account: session.account(),
            client_module: session.client_module().clone(),
        }
    }

    /// Returns the account the session authenticates.
    #[must_use]
    pub const fn account(&self) -> StateIdentifier {
        self.account
    }

    /// Returns the Client Module the session was established for.
    #[must_use]
    pub const fn client_module(&self) -> &Name {
        &self.client_module
    }
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

/// The continuation-bearing second-factor checks that run before a body exists.
struct MfaPreconditions {
    expected_origin: ExpectedOrigin,
}

impl PreBodyCheck for MfaPreconditions {
    fn check(
        &self,
        method: &Method,
        _target: &Uri,
        headers: &HeaderMap,
    ) -> Result<PreBodyGrant, PreBodyRejection> {
        match validate_mfa_request(method, headers, self.expected_origin) {
            Ok(()) => Ok(PreBodyGrant::default()),
            Err(AuthenticationRejection::RequestOriginDenied) => {
                Err(PreBodyRejection::RequestOriginDenied)
            }
            Err(_) => Err(PreBodyRejection::BadRequest),
        }
    }
}

/// The session-bearing self-enrollment checks that run before a body exists.
///
/// Running here means a request that carries no session or no cross-site
/// request forgery header is refused before it can occupy the single
/// verification permit this route shares with login.
struct MfaSessionPreconditions {
    expected_origin: ExpectedOrigin,
}

impl PreBodyCheck for MfaSessionPreconditions {
    fn check(
        &self,
        method: &Method,
        _target: &Uri,
        headers: &HeaderMap,
    ) -> Result<PreBodyGrant, PreBodyRejection> {
        match validate_mfa_session_request(method, headers, self.expected_origin) {
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

/// Returns the two names this build's MFA Module is addressed by.
///
/// A factor records the module name while the module's enabled state belongs
/// to its own configuration component, so an operation spanning both is given
/// both from one place.
fn totp_target() -> Result<MfaModuleTarget, AuthenticationRejection> {
    let unavailable = AuthenticationRejection::ServiceUnavailable;

    Ok(MfaModuleTarget {
        module: Name::new(TOTP_MODULE).map_err(|_| unavailable)?,
        component: Name::new(TOTP_COMPONENT).map_err(|_| unavailable)?,
    })
}

/// Reports whether the deployment currently verifies second factors.
///
/// Any value other than the exact enabled value, including a missing entry,
/// leaves the module disabled.
fn module_enabled(state: &ApplicationState) -> bool {
    state.configuration().iter().any(|entry| {
        entry.component.as_str() == TOTP_COMPONENT
            && entry.key.as_str() == MFA_ENABLED_KEY
            && entry.value.as_str() == MFA_ENABLED_VALUE
    })
}

/// Returns the factor one account holds for the MFA Module, when it holds one.
fn enrolled_factor(state: &ApplicationState, account: StateIdentifier) -> Option<&MfaFactor> {
    state
        .mfa_factors()
        .iter()
        .find(|factor| factor.account == account && factor.module.as_str() == TOTP_MODULE)
}

/// Returns the username one account is addressed by, when the state holds it.
fn account_username(state: &ApplicationState, account: StateIdentifier) -> Option<Name> {
    state
        .accounts()
        .iter()
        .find(|candidate: &&Account| candidate.identifier == account)
        .map(|candidate| candidate.username.clone())
}

/// Converts Unix milliseconds into the whole seconds a time step counts from.
fn unix_seconds(milliseconds: i64) -> Result<u64, AuthenticationRejection> {
    u64::try_from(milliseconds / 1_000).map_err(|_| AuthenticationRejection::ServiceUnavailable)
}

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
pub(crate) mod tests {
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
        mfa::{MFA_ENROLLMENT_REQUIRED_CODE, MFA_REQUIRED_CODE},
        typed_json::{
            MAX_STABLE_CODE_BYTES, ResponseCorrelation, StableCode, TypedJsonEnvelope, TypedResult,
            TypedValue,
        },
    };
    use weavelit_module_mfa_totp::STEP_SECONDS;
    use weavelit_server_authentication::{
        Argon2Profile, AuthenticationError, CONTINUATION_TEXT_BYTES, SESSION_TOKEN_TEXT_BYTES,
    };
    use weavelit_server_database::{
        Account, AccountPasswordVerifier, MAX_NAME_LENGTH, PasswordVerifier,
        SESSION_ABSOLUTE_LIFETIME_MILLISECONDS, SESSION_IDLE_TIMEOUT_MILLISECONDS,
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
            UNBOUND_LISTENER, process_over_duplex, published_serving_modes, seal_deployment_from,
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

    /// The identifier of the factor an already-enrolled fixture account holds.
    const ENROLLED_FACTOR_BYTES: [u8; 16] = [0xf1; 16];

    /// The shared secret an already-enrolled fixture account holds.
    ///
    /// This is the twenty-byte secret RFC 6238 publishes its vectors against,
    /// so a fixture code can be derived from the same values the TOTP Module's
    /// own tests pin rather than from a second, unverified secret.
    const ENROLLED_SECRET: [u8; 20] = *b"12345678901234567890";

    /// The instant, in whole Unix seconds, a published RFC 6238 vector for
    /// [`ENROLLED_SECRET`] was issued at.
    ///
    /// The instant divides exactly by [`STEP_SECONDS`], so it is the first
    /// second of its own time step and an offset of whole steps stays inside
    /// the step it names.
    const ENROLLED_VECTOR_SECONDS: u64 = 1_234_567_890;

    /// The six digits that vector publishes for [`ENROLLED_VECTOR_SECONDS`].
    const ENROLLED_VECTOR_CODE: &str = "005924";

    const _: () = assert!(
        ENROLLED_VECTOR_SECONDS.is_multiple_of(STEP_SECONDS),
        "the fixture instant must sit on a time step boundary"
    );

    /// A well-shaped code [`ENROLLED_SECRET`] does not verify at that instant.
    ///
    /// Every test that presents it first asserts that refusal through the TOTP
    /// Module's own verification path, so the fixture cannot silently become a
    /// code the Server accepts.
    const REFUSED_CODE: &str = "000000";

    /// A well-shaped bearer value this Server never issued.
    const UNISSUED_TICKET: &str = "not-a-continuation-this-server-ever-issued";

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
    pub(crate) struct DeliveredRecord {
        pub(crate) correlation_id: String,
        pub(crate) classification: String,
        pub(crate) detail: String,
    }

    #[derive(Debug)]
    struct RecordingDestination {
        delivered: Arc<Mutex<Vec<DeliveredRecord>>>,
        fails: bool,
    }

    impl LogDestination for RecordingDestination {
        fn preflight(&self, _record_type: LogRecordType) -> Result<(), LogDestinationError> {
            if self.fails {
                return Err(LogDestinationError::Unavailable);
            }
            Ok(())
        }

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
    pub(crate) fn recording_log(
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
        /// The username the sealed active account answers to.
        username: String,
    }

    impl AuthSurface {
        fn new() -> Self {
            Self::build(true, None, MfaFixture::default())
        }

        fn gated() -> Self {
            Self::build(false, None, MfaFixture::default())
        }

        fn with_log(system_log: Arc<ConfiguredLogDestination>) -> Self {
            Self::build(true, Some(system_log), MfaFixture::default())
        }

        /// Builds a surface whose sealed state carries the described MFA setup.
        fn with_mfa(fixture: MfaFixture) -> Self {
            Self::build(true, None, fixture)
        }

        fn build(
            open_engine: bool,
            system_log: Option<Arc<ConfiguredLogDestination>>,
            fixture: MfaFixture,
        ) -> Self {
            let root = tempfile::tempdir().expect("the test state root must be created");
            fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700))
                .expect("the test state root must be private");
            let state_root = root
                .path()
                .canonicalize()
                .expect("the test state root must resolve");
            let username = fixture.username().to_owned();
            seal_deployment_from(&state_root, |sealer| authentication_state(&fixture, sealer));

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
                startup.protection(),
            )
            .expect("the authentication runtime must compose");

            Self {
                _startup: startup,
                _root: root,
                runtime,
                engine,
                clock,
                username,
            }
        }

        /// Builds the mounted surface the authentication routes serve on.
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

        /// Returns the instant the injected clock currently reports.
        fn clock(&self) -> i64 {
            self.clock.load(Ordering::SeqCst)
        }

        /// Returns the username the sealed active account answers to.
        fn username(&self) -> &str {
            &self.username
        }
    }

    /// The MFA setup one sealed deployment is built with.
    ///
    /// Enablement is deployment-wide configuration while the requirement and
    /// the enrolled secret belong to the one active account this state holds,
    /// so a single fixture describes exactly one combination of the three
    /// inputs admission decides against. A test that decides several
    /// combinations seals a fresh deployment from a fresh fixture for each one.
    #[derive(Default)]
    struct MfaFixture {
        /// Whether the deployment's configuration enables the MFA Module.
        enabled: bool,
        /// Whether the active account is required to hold a second factor.
        required: bool,
        /// The secret the active account is enrolled with, when it is enrolled.
        secret: Option<[u8; SECRET_LENGTH]>,
        /// The active account's username, when it is not [`ACTIVE_USERNAME`].
        username: Option<String>,
    }

    impl MfaFixture {
        fn enabled() -> Self {
            Self {
                enabled: true,
                ..Self::default()
            }
        }

        fn requiring(mut self) -> Self {
            self.required = true;
            self
        }

        fn enrolled(mut self) -> Self {
            self.secret = Some(ENROLLED_SECRET);
            self
        }

        /// Seals the active account under `username` rather than the default.
        fn named(mut self, username: &str) -> Self {
            self.username = Some(username.to_owned());
            self
        }

        /// Returns the username the active account is sealed under.
        fn username(&self) -> &str {
            self.username.as_deref().unwrap_or(ACTIVE_USERNAME)
        }
    }

    /// The sealed application state every authentication test decides against.
    fn authentication_state(
        fixture: &MfaFixture,
        sealer: &dyn weavelit_server_lifecycle::ProtectedValueSealer,
    ) -> ApplicationState {
        let active = StateIdentifier::from_bytes(ACTIVE_ACCOUNT_BYTES)
            .expect("the active account identifier must be accepted");
        let inactive = StateIdentifier::from_bytes(INACTIVE_ACCOUNT_BYTES)
            .expect("the inactive account identifier must be accepted");

        let configuration = if fixture.enabled {
            vec![weavelit_server_database::ConfigurationEntry {
                component: name(TOTP_COMPONENT),
                key: weavelit_server_database::ConfigurationKey::new(MFA_ENABLED_KEY)
                    .expect("the enablement key must be accepted"),
                value: weavelit_server_database::ConfigurationValue::new(MFA_ENABLED_VALUE)
                    .expect("the enablement value must be accepted"),
            }]
        } else {
            Vec::new()
        };
        let mfa_factors = fixture.secret.map_or_else(Vec::new, |secret| {
            vec![MfaFactor {
                identifier: StateIdentifier::from_bytes(ENROLLED_FACTOR_BYTES)
                    .expect("the factor identifier must be accepted"),
                account: active,
                module: name(TOTP_MODULE),
                protected_factor_data: sealer
                    .seal(ProtectedValueKind::MfaFactorData, &secret)
                    .expect("the enrolled secret must seal"),
            }]
        });

        crate::tests::sealed_application_state_from(crate::tests::SealedStateParts {
            configuration,
            accounts: vec![
                Account {
                    identifier: active,
                    username: name(fixture.username()),
                    display_name: None,
                    active: true,
                    mfa_required: fixture.required,
                },
                Account {
                    identifier: inactive,
                    username: name(INACTIVE_USERNAME),
                    display_name: None,
                    active: false,
                    mfa_required: false,
                },
            ],
            password_verifiers: vec![
                AccountPasswordVerifier {
                    account: active,
                    verifier: verifier(),
                },
                AccountPasswordVerifier {
                    account: inactive,
                    verifier: verifier(),
                },
            ],
            mfa_factors,
            ..crate::tests::SealedStateParts::default()
        })
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

    /// Builds a continuation-bearing second-factor request head.
    ///
    /// These routes carry no session, exactly as login does not, so they are
    /// trusted by the same exact origin plus the literal CSRF header value.
    fn mfa_head(target: &str, body_length: usize) -> String {
        format!(
            "PUT {target} HTTP/1.1\r\n\
             Host: {UNBOUND_LISTENER}\r\n\
             Origin: https://{UNBOUND_LISTENER}\r\n\
             X-Weavelit-CSRF: 1\r\n\
             Accept: application/json\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {body_length}\r\n\r\n"
        )
    }

    /// Submits one code against the ticket the named body field carries.
    async fn submit_code(
        surface: &AuthSurface,
        timeouts: ConnectionTimeouts,
        target: &str,
        ticket: (&str, &str),
        code: &str,
    ) -> BoundedResponse {
        let (field, value) = ticket;
        let body = format!("{{\"{field}\":\"{value}\",\"code\":\"{code}\"}}");
        request(
            surface.surface(),
            timeouts,
            mfa_head(target, body.len()),
            body,
        )
        .await
    }

    /// Submits one code against a login continuation.
    async fn verify_code(surface: &AuthSurface, continuation: &str, code: &str) -> BoundedResponse {
        submit_code(
            surface,
            default_timeouts(),
            AUTH_MFA_VERIFY_ROUTE,
            ("continuation", continuation),
            code,
        )
        .await
    }

    /// Submits one code against an opened enrollment.
    async fn confirm_code(surface: &AuthSurface, enrollment: &str, code: &str) -> BoundedResponse {
        submit_code(
            surface,
            default_timeouts(),
            AUTH_MFA_ENROLLMENT_CONFIRM_ROUTE,
            ("enrollment", enrollment),
            code,
        )
        .await
    }

    /// Opens an enrollment against a login continuation.
    async fn opened_enrollment(surface: &AuthSurface, continuation: &str) -> BoundedResponse {
        let body = format!("{{\"continuation\":\"{continuation}\"}}");
        request(
            surface.surface(),
            default_timeouts(),
            mfa_head(AUTH_MFA_ENROLLMENT_ROUTE, body.len()),
            body,
        )
        .await
    }

    /// Builds a session-bearing self-enrollment request head.
    ///
    /// The session and the token are optional so a test can present a request
    /// that carries neither, which is the shape the route must refuse before it
    /// allocates a body.
    fn self_enrollment_head(
        origin: &str,
        session_token: Option<&str>,
        csrf_token: Option<&str>,
        body_length: usize,
    ) -> String {
        let cookie = session_token.map_or_else(String::new, |value| {
            format!("Cookie: {SESSION_COOKIE_NAME}={value}\r\n")
        });
        let csrf =
            csrf_token.map_or_else(String::new, |value| format!("X-Weavelit-CSRF: {value}\r\n"));
        format!(
            "PUT {AUTH_MFA_SELF_ENROLLMENT_ROUTE} HTTP/1.1\r\n\
             Host: {UNBOUND_LISTENER}\r\n\
             Origin: {origin}\r\n\
             {csrf}\
             {cookie}\
             Accept: application/json\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {body_length}\r\n\r\n"
        )
    }

    /// Opens an enrollment from a live session and a re-entered password.
    async fn self_enrollment(
        surface: &AuthSurface,
        session_token: Option<&str>,
        csrf_token: Option<&str>,
        password: &str,
    ) -> BoundedResponse {
        let body = format!("{{\"password\":\"{password}\"}}");
        request(
            surface.surface(),
            default_timeouts(),
            self_enrollment_head(
                &format!("https://{UNBOUND_LISTENER}"),
                session_token,
                csrf_token,
                body.len(),
            ),
            body,
        )
        .await
    }

    /// Builds the fixture one row of the admission matrix is decided against.
    fn mfa_fixture(enabled: bool, enrolled: bool, required: bool) -> MfaFixture {
        let mut fixture = if enabled {
            MfaFixture::enabled()
        } else {
            MfaFixture::default()
        };
        if enrolled {
            fixture = fixture.enrolled();
        }
        if required {
            fixture = fixture.requiring();
        }
        fixture
    }

    /// Returns the instant, in Unix milliseconds, `steps` time steps away from
    /// the instant [`ENROLLED_VECTOR_CODE`] was published for.
    fn vector_milliseconds(steps: i64) -> i64 {
        let step = i64::try_from(STEP_SECONDS).expect("the step length fits a signed instant");
        let published = i64::try_from(ENROLLED_VECTOR_SECONDS)
            .expect("the vector instant fits a signed instant");
        (published + steps * step) * 1_000
    }

    /// Reports whether the TOTP Module itself verifies the fixture code at an
    /// instant `steps` time steps from the published one.
    ///
    /// Every expectation about skew is read from the Module's own verification
    /// path here rather than restated by hand, so a fixture that stopped
    /// matching fails the assertion instead of silently passing.
    fn module_verifies_fixture_code(steps: i64) -> bool {
        let seconds = u64::try_from(vector_milliseconds(steps) / 1_000)
            .expect("every fixture instant is after the Unix epoch");
        TotpSecret::from_bytes(ENROLLED_SECRET)
            .verify(ENROLLED_VECTOR_CODE, seconds)
            .is_some()
    }

    /// Returns the length of `steps` whole time steps in milliseconds.
    fn step_milliseconds(steps: i64) -> i64 {
        let step = i64::try_from(STEP_SECONDS).expect("the step length fits a signed instant");
        steps * step * 1_000
    }

    /// Returns the whole Unix seconds one injected instant names.
    fn seconds_at(milliseconds: i64) -> u64 {
        u64::try_from(milliseconds / 1_000).expect("every test instant is after the Unix epoch")
    }

    /// Rebuilds the secret one enrollment response disclosed.
    ///
    /// The rebuilt secret is re-encoded and compared with the disclosed text,
    /// so a decoding mistake fails here rather than silently producing codes
    /// for a secret other than the one the Server is about to bind.
    fn disclosed_secret(base32: &str) -> TotpSecret {
        /// The unpadded RFC 4648 alphabet a disclosed secret is written in.
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

        assert_eq!(
            base32.len(),
            SECRET_LENGTH * 8 / 5,
            "a disclosed secret is one whole unpadded Base32 encoding"
        );
        let mut bytes = [0_u8; SECRET_LENGTH];
        let mut accumulator = 0_u16;
        let mut bits = 0_u8;
        let mut written = 0_usize;
        for symbol in base32.bytes() {
            let value = ALPHABET
                .iter()
                .position(|candidate| *candidate == symbol)
                .expect("a disclosed secret carries only Base32 symbols");
            accumulator = (accumulator << 5)
                | u16::try_from(value).expect("a Base32 symbol value is under thirty-two");
            bits += 5;
            if bits >= 8 {
                bits -= 8;
                bytes[written] = u8::try_from((accumulator >> bits) & 0xff)
                    .expect("a masked octet fits in a byte");
                written += 1;
            }
        }

        let secret = TotpSecret::from_bytes(bytes);
        assert_eq!(
            secret.base32().expose(),
            base32,
            "the rebuilt secret must re-encode to the text that was disclosed"
        );
        secret
    }

    /// Reads the secret and the confirming ticket one opened enrollment
    /// disclosed.
    fn opened_parts(response: &BoundedResponse) -> (TotpSecret, String) {
        let body = body_text(response);
        (
            disclosed_secret(&string_field(&body, "secret")),
            string_field(&body, "enrollment"),
        )
    }

    /// Returns the code one secret produces at an injected instant.
    ///
    /// The generated code is offered back to the Module's own verification path
    /// before it is returned, so a test that presents it is presenting a code
    /// this profile really accepts rather than one it assumed was current.
    fn current_code(secret: &TotpSecret, milliseconds: i64) -> String {
        let seconds = seconds_at(milliseconds);
        let code = secret.code_at(seconds);
        assert!(
            secret.verify(&code, seconds).is_some(),
            "a generated code must verify at the instant it was generated for"
        );
        code
    }

    /// Returns a well-shaped code produced by a secret other than this one.
    ///
    /// The candidates are fixed variations of the RFC 6238 test secret and each
    /// is decided by the Module's own verification path first, so the returned
    /// code is one this secret really refuses rather than one assumed to be
    /// wrong. That keeps the refusal deterministic even though the secret the
    /// Server disclosed is drawn at random.
    fn code_from_another_secret(secret: &TotpSecret, milliseconds: i64) -> String {
        let seconds = seconds_at(milliseconds);
        (0..u8::MAX)
            .find_map(|nonce| {
                let mut bytes = ENROLLED_SECRET;
                bytes[0] ^= nonce;
                let code = TotpSecret::from_bytes(bytes).code_at(seconds);
                secret.verify(&code, seconds).is_none().then_some(code)
            })
            .expect("some other secret must produce a code this one refuses")
    }

    /// Reads one JSON string field out of a typed response body.
    fn string_field(body: &str, name: &str) -> String {
        let field = format!("\"{name}\":\"");
        let start = body
            .find(&field)
            .unwrap_or_else(|| panic!("the body must carry {name}: {body}"))
            + field.len();
        let rest = &body[start..];
        let end = rest
            .find('"')
            .expect("a JSON string field must be terminated");
        rest[..end].to_owned()
    }

    /// Logs in and returns the continuation a login stopped at `stage` issued.
    async fn stopped_login(surface: &AuthSurface, stage: &str) -> String {
        let response = login(surface, surface.username(), CORRECT_PASSWORD).await;
        assert_eq!(response.status, StatusCode::ACCEPTED);
        let body = body_text(&response);
        assert_eq!(string_field(&body, "mfa"), stage);
        string_field(&body, "continuation")
    }

    /// Renders every text field one delivered System Log record carries.
    ///
    /// The two remaining fields are a Server-drawn record identifier and the
    /// event time, neither of which is built from request content.
    fn record_text(record: &DeliveredRecord) -> String {
        format!(
            "{}\n{}\n{}",
            record.correlation_id, record.classification, record.detail
        )
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

    /// A request failing the CSRF check does not extend the idle lifetime.
    ///
    /// The idle timeout bounds how long a session survives without legitimate
    /// use, so a rejected request must not refresh it. Holding only the session
    /// cookie would otherwise keep a session alive indefinitely through
    /// requests that are all refused. The clock is injected, so the deadline
    /// here is crossed by choice rather than by waiting.
    #[tokio::test]
    async fn a_request_failing_the_csrf_check_does_not_extend_the_idle_lifetime() {
        let surface = AuthSurface::new();
        let (session, csrf) = established_session(&surface).await;

        surface.set_clock(ISSUED_AT + SESSION_IDLE_TIMEOUT_MILLISECONDS - 1);
        let refused = request(
            surface.surface(),
            default_timeouts(),
            session_head(AUTH_SESSION_ROUTE, &session, "not-this-sessions-token"),
            String::new(),
        )
        .await;
        assert_eq!(refused.status, StatusCode::UNAUTHORIZED);

        // Past the original idle deadline the correct pair must also fail. It
        // would still succeed if the refused request had touched the session.
        surface.set_clock(ISSUED_AT + SESSION_IDLE_TIMEOUT_MILLISECONDS);
        assert_session_invalid(&surface, &session, &csrf).await;
    }

    /// A request passing the CSRF check still extends the idle lifetime.
    #[tokio::test]
    async fn a_request_passing_the_csrf_check_extends_the_idle_lifetime() {
        let surface = AuthSurface::new();
        let (session, csrf) = established_session(&surface).await;

        surface.set_clock(ISSUED_AT + SESSION_IDLE_TIMEOUT_MILLISECONDS - 1);
        let touched = request(
            surface.surface(),
            default_timeouts(),
            session_head(AUTH_SESSION_ROUTE, &session, &csrf),
            String::new(),
        )
        .await;
        assert_eq!(touched.status, StatusCode::OK);

        surface.set_clock(ISSUED_AT + SESSION_IDLE_TIMEOUT_MILLISECONDS);
        let still_live = request(
            surface.surface(),
            default_timeouts(),
            session_head(AUTH_SESSION_ROUTE, &session, &csrf),
            String::new(),
        )
        .await;
        assert_eq!(
            still_live.status,
            StatusCode::OK,
            "an accepted request must still refresh the idle timeout"
        );
    }

    /// A wrong CSRF token is refused exactly as an unknown session token is.
    ///
    /// Both responses are compared byte for byte once their own correlation
    /// identifiers are normalized, so the fact that one of the two sessions
    /// exists is not observable.
    #[tokio::test]
    async fn a_wrong_csrf_token_is_refused_exactly_as_an_unknown_session_is() {
        let surface = AuthSurface::new();
        let (session, _csrf) = established_session(&surface).await;
        let unknown = "z".repeat(SESSION_TOKEN_TEXT_BYTES);

        let mut wire = Vec::new();
        for (presented_session, presented_csrf) in [
            (session.as_str(), "not-this-sessions-token"),
            (unknown.as_str(), "not-this-sessions-token"),
        ] {
            let response = request(
                surface.surface(),
                default_timeouts(),
                session_head(AUTH_SESSION_ROUTE, presented_session, presented_csrf),
                String::new(),
            )
            .await;
            assert_eq!(response.status, StatusCode::UNAUTHORIZED);
            assert!(response.cookies.is_none());
            let correlation = correlation_of(&response);
            assert!(is_correlation_identifier(&correlation));
            wire.push(normalized(&rendered(&response).await, &correlation));
        }

        assert_eq!(wire[0], wire[1]);
        assert_eq!(
            wire[0],
            "HTTP/1.1 401 \r\nContent-Type: application/json; charset=utf-8\r\n\r\n\
             {\"error\":\"session_invalid\",\"correlation_id\":\"<correlation>\"}"
        );
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
    // Second-factor admission
    // -----------------------------------------------------------------------

    /// What one row of the admission matrix must produce.
    #[derive(Clone, Copy)]
    enum Admitted {
        /// A `202` naming the stage, carrying a continuation and no cookie.
        Continuation(&'static str),
        /// A `200` carrying the session and the CSRF cookie.
        Session,
        /// A `401` indistinguishable from the denial a wrong password produces.
        Denied,
    }

    /// Every combination of enablement, enrollment, and requirement is decided.
    ///
    /// The three inputs are independent, so all eight rows are stated here
    /// rather than sampled. Each row asserts the status, the code the body
    /// carries, and the presence or absence of both cookies, because a row that
    /// answered the right status while still issuing a session would otherwise
    /// pass.
    ///
    /// The two rows that deny a required account whose Module is disabled also
    /// assert byte identity with a wrong-password denial on the same
    /// deployment, so a disabled Module is not detectable from the response.
    #[tokio::test]
    async fn login_admission_decides_every_enablement_enrollment_and_requirement_row() {
        for (enabled, enrolled, required, expected) in [
            (true, true, true, Admitted::Continuation(MFA_REQUIRED_CODE)),
            (true, true, false, Admitted::Continuation(MFA_REQUIRED_CODE)),
            (
                true,
                false,
                true,
                Admitted::Continuation(MFA_ENROLLMENT_REQUIRED_CODE),
            ),
            (true, false, false, Admitted::Session),
            (false, true, true, Admitted::Denied),
            (false, true, false, Admitted::Session),
            (false, false, true, Admitted::Denied),
            (false, false, false, Admitted::Session),
        ] {
            let row = format!("enabled={enabled} enrolled={enrolled} required={required}");
            let surface = AuthSurface::with_mfa(mfa_fixture(enabled, enrolled, required));
            let response = login(&surface, ACTIVE_USERNAME, CORRECT_PASSWORD).await;
            let wire = rendered(&response).await;
            let correlation = correlation_of(&response);
            assert!(is_correlation_identifier(&correlation), "{row}");

            match expected {
                Admitted::Continuation(stage) => {
                    assert_eq!(response.status, StatusCode::ACCEPTED, "{row}");
                    let continuation = string_field(&body_text(&response), "continuation");
                    assert_eq!(continuation.len(), CONTINUATION_TEXT_BYTES, "{row}");
                    assert_eq!(
                        body_text(&response),
                        format!(
                            "{{\"result\":{{\"mfa\":\"{stage}\",\"continuation\":\"{continuation}\"}},\
                             \"correlation_id\":\"{correlation}\"}}"
                        ),
                        "{row}"
                    );
                    assert!(response.cookies.is_none(), "{row}");
                    assert!(!wire.contains("Set-Cookie"), "{row}");
                }
                Admitted::Session => {
                    assert_eq!(response.status, StatusCode::OK, "{row}");
                    assert_eq!(set_cookie_lines(&wire).len(), 2, "{row}");
                    assert_eq!(
                        cookie_value(&wire, SESSION_COOKIE_NAME).len(),
                        SESSION_TOKEN_TEXT_BYTES,
                        "{row}"
                    );
                    assert_eq!(
                        cookie_value(&wire, CSRF_COOKIE_NAME).len(),
                        SESSION_TOKEN_TEXT_BYTES,
                        "{row}"
                    );
                    assert_eq!(
                        body_text(&response),
                        format!(
                            "{{\"result\":{{\"authenticated\":true}},\
                             \"correlation_id\":\"{correlation}\"}}"
                        ),
                        "{row}"
                    );
                }
                Admitted::Denied => {
                    assert_eq!(response.status, StatusCode::UNAUTHORIZED, "{row}");
                    assert_eq!(
                        body_text(&response),
                        format!(
                            "{{\"error\":\"authentication_failed\",\
                             \"correlation_id\":\"{correlation}\"}}"
                        ),
                        "{row}"
                    );
                    assert!(response.cookies.is_none(), "{row}");
                    assert!(!wire.contains("Set-Cookie"), "{row}");

                    let wrong = login(&surface, ACTIVE_USERNAME, WRONG_PASSWORD).await;
                    let wrong_correlation = correlation_of(&wrong);
                    assert!(is_correlation_identifier(&wrong_correlation), "{row}");
                    assert_ne!(correlation, wrong_correlation, "{row}");
                    assert_eq!(
                        normalized(&wire, &correlation),
                        normalized(&rendered(&wrong).await, &wrong_correlation),
                        "{row}"
                    );
                }
            }
        }
    }

    /// Every login path costs exactly one password verification.
    ///
    /// The five paths are an unknown username, a wrong password, an accepted
    /// login with no second factor, an accepted password that stops at an
    /// enrolled factor, and an accepted password that stops at enrollment. The
    /// count comes from the injected engine, so nothing here measures time.
    #[tokio::test]
    async fn every_login_path_performs_exactly_one_password_verification() {
        let mut counted = Vec::new();
        for (fixture, username, password, expected) in [
            (
                mfa_fixture(false, false, false),
                UNKNOWN_USERNAME,
                CORRECT_PASSWORD,
                StatusCode::UNAUTHORIZED,
            ),
            (
                mfa_fixture(false, false, false),
                ACTIVE_USERNAME,
                WRONG_PASSWORD,
                StatusCode::UNAUTHORIZED,
            ),
            (
                mfa_fixture(false, false, false),
                ACTIVE_USERNAME,
                CORRECT_PASSWORD,
                StatusCode::OK,
            ),
            (
                mfa_fixture(true, true, true),
                ACTIVE_USERNAME,
                CORRECT_PASSWORD,
                StatusCode::ACCEPTED,
            ),
            (
                mfa_fixture(true, false, true),
                ACTIVE_USERNAME,
                CORRECT_PASSWORD,
                StatusCode::ACCEPTED,
            ),
        ] {
            let surface = AuthSurface::with_mfa(fixture);
            let response = login(&surface, username, password).await;
            assert_eq!(response.status, expected, "{username}");
            assert_eq!(surface.engine.hashes(), 0, "{username}");
            counted.push(surface.engine.verifications());
        }

        assert_eq!(
            counted,
            vec![1; 5],
            "every login path must perform exactly one verification"
        );
    }

    // -----------------------------------------------------------------------
    // Replay and the continuation ticket
    // -----------------------------------------------------------------------

    /// A code accepted once is refused when it is presented again.
    ///
    /// The second attempt carries its own fresh continuation, so the only thing
    /// left to refuse it is the factor's replay watermark.
    #[tokio::test]
    async fn an_accepted_code_is_refused_when_it_is_presented_again() {
        let surface = AuthSurface::with_mfa(mfa_fixture(true, true, true));
        surface.set_clock(vector_milliseconds(0));

        let first = stopped_login(&surface, MFA_REQUIRED_CODE).await;
        let accepted = verify_code(&surface, &first, ENROLLED_VECTOR_CODE).await;
        assert_eq!(accepted.status, StatusCode::OK);
        assert_eq!(set_cookie_lines(&rendered(&accepted).await).len(), 2);

        let second = stopped_login(&surface, MFA_REQUIRED_CODE).await;
        assert_ne!(first, second);
        let replayed = verify_code(&surface, &second, ENROLLED_VECTOR_CODE).await;

        assert_eq!(replayed.status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            body_text(&replayed),
            format!(
                "{{\"error\":\"authentication_failed\",\"correlation_id\":\"{}\"}}",
                correlation_of(&replayed)
            )
        );
        assert!(replayed.cookies.is_none());
        assert!(!rendered(&replayed).await.contains("Set-Cookie"));
    }

    /// An invalid code consumes the continuation it was presented with.
    ///
    /// This is deliberate: one continuation admits one attempt, so retrying the
    /// correct code against the same ticket fails. The last step presents the
    /// same correct code against a fresh continuation and succeeds, which is
    /// what makes the retry failure the ticket rather than the code.
    #[tokio::test]
    async fn an_invalid_code_consumes_the_continuation_it_was_presented_with() {
        assert!(
            TotpSecret::from_bytes(ENROLLED_SECRET)
                .verify(REFUSED_CODE, ENROLLED_VECTOR_SECONDS)
                .is_none(),
            "the refused fixture code must be one the enrolled secret does not verify"
        );

        let surface = AuthSurface::with_mfa(mfa_fixture(true, true, true));
        surface.set_clock(vector_milliseconds(0));

        let continuation = stopped_login(&surface, MFA_REQUIRED_CODE).await;
        let refused = verify_code(&surface, &continuation, REFUSED_CODE).await;
        assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
        assert!(refused.cookies.is_none());

        let retried = verify_code(&surface, &continuation, ENROLLED_VECTOR_CODE).await;
        assert_eq!(retried.status, StatusCode::UNAUTHORIZED);
        assert!(retried.cookies.is_none());
        assert!(!rendered(&retried).await.contains("Set-Cookie"));

        let fresh = stopped_login(&surface, MFA_REQUIRED_CODE).await;
        let accepted = verify_code(&surface, &fresh, ENROLLED_VECTOR_CODE).await;
        assert_eq!(accepted.status, StatusCode::OK);
        assert_eq!(set_cookie_lines(&rendered(&accepted).await).len(), 2);
    }

    /// One time step of skew is accepted on either side and no more.
    ///
    /// Each offset is decided against the TOTP Module's own verification path
    /// first, so the route's answer is compared with what the Module itself
    /// says rather than with a hand-written expectation.
    #[tokio::test]
    async fn a_code_is_accepted_one_step_either_side_and_refused_two_steps_away() {
        for (steps, accepted) in [
            (-2_i64, false),
            (-1, true),
            (0, true),
            (1, true),
            (2, false),
        ] {
            assert_eq!(
                module_verifies_fixture_code(steps),
                accepted,
                "the TOTP Module must decide {steps} steps away as this row states"
            );

            let surface = AuthSurface::with_mfa(mfa_fixture(true, true, true));
            surface.set_clock(vector_milliseconds(steps));
            let continuation = stopped_login(&surface, MFA_REQUIRED_CODE).await;
            let response = verify_code(&surface, &continuation, ENROLLED_VECTOR_CODE).await;
            let wire = rendered(&response).await;

            if accepted {
                assert_eq!(response.status, StatusCode::OK, "{steps}");
                assert_eq!(set_cookie_lines(&wire).len(), 2, "{steps}");
            } else {
                assert_eq!(response.status, StatusCode::UNAUTHORIZED, "{steps}");
                assert!(response.cookies.is_none(), "{steps}");
                assert!(!wire.contains("Set-Cookie"), "{steps}");
            }
        }
    }

    /// A code submission does not enter the single password verification lane.
    ///
    /// The permit is held for the whole submission. A code costs one decryption
    /// and one HMAC rather than an Argon2 profile ceiling, so admitting it into
    /// that lane would make a held password verification block it; the engine's
    /// count proves the submission performed no verification of its own.
    #[tokio::test]
    async fn a_second_factor_submission_does_not_take_the_password_verification_lane() {
        let surface = AuthSurface::with_mfa(mfa_fixture(true, true, true));
        surface.set_clock(vector_milliseconds(0));
        let continuation = stopped_login(&surface, MFA_REQUIRED_CODE).await;
        assert_eq!(surface.engine.verifications(), 1);

        let held = Arc::clone(&surface.runtime.login_lane)
            .try_acquire_owned()
            .expect("the single login permit must be free once the login returned");
        let response = submit_code(
            &surface,
            ConnectionTimeouts {
                handshake: TLS_HANDSHAKE_TIMEOUT,
                request_read: REQUEST_READ_TIMEOUT,
                processing: SHORT_PROCESSING,
            },
            AUTH_MFA_VERIFY_ROUTE,
            ("continuation", &continuation),
            ENROLLED_VECTOR_CODE,
        )
        .await;

        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(set_cookie_lines(&rendered(&response).await).len(), 2);
        assert_eq!(
            surface.engine.verifications(),
            1,
            "a code submission must cost no password verification"
        );
        drop(held);
    }

    // -----------------------------------------------------------------------
    // One-time provisioning disclosure
    // -----------------------------------------------------------------------

    /// Provisioning data is disclosed in one response and nowhere else.
    ///
    /// The secret and the URI are taken from the response that discloses them
    /// and are proved present there first, so every later search is for a real
    /// non-empty value that could have matched. Nothing asserts that any log
    /// output was captured, because a correct Server may legitimately emit
    /// none.
    ///
    /// The searches run over rendered bytes rather than a `Debug` rendering,
    /// because the bounded text types redact themselves in `Debug` and a scan
    /// of that rendering could never match.
    #[tokio::test]
    async fn opened_provisioning_data_is_disclosed_exactly_once() {
        let (destination, delivered) = recording_log(false);
        let surface = AuthSurface::build(true, Some(destination), mfa_fixture(true, false, true));

        let continuation = stopped_login(&surface, MFA_ENROLLMENT_REQUIRED_CODE).await;
        let opened = opened_enrollment(&surface, &continuation).await;
        assert_eq!(opened.status, StatusCode::OK);
        let opened_wire = rendered(&opened).await;
        let secret = string_field(&body_text(&opened), "secret");
        let uri = string_field(&body_text(&opened), "provisioning_uri");
        let ticket = string_field(&body_text(&opened), "enrollment");

        // Every needle is a real observed value, so a later search that finds
        // nothing is a real absence rather than an empty pattern.
        assert!(!secret.is_empty());
        assert!(!uri.is_empty());
        assert_eq!(ticket.len(), CONTINUATION_TEXT_BYTES);
        assert!(opened_wire.contains(&secret));
        assert!(opened_wire.contains(&uri));
        assert!(uri.starts_with("otpauth://totp/Weavelit:"));
        assert!(uri.contains(&format!("secret={secret}")));

        // Each of these is refused for a reason the code never reaches, so the
        // rejections are decided rather than drawn against a random secret.
        let later = [
            opened_enrollment(&surface, &continuation).await,
            verify_code(&surface, &ticket, REFUSED_CODE).await,
            confirm_code(&surface, &ticket, REFUSED_CODE).await,
            confirm_code(&surface, UNISSUED_TICKET, REFUSED_CODE).await,
        ];
        for (index, response) in later.iter().enumerate() {
            assert_eq!(response.status, StatusCode::UNAUTHORIZED, "{index}");
            let wire = rendered(response).await;
            assert!(!wire.contains(&secret), "{index} disclosed the secret");
            assert!(
                !wire.contains(&uri),
                "{index} disclosed the provisioning URI"
            );
        }

        // A fresh enrollment discloses fresh values, so the first pair is gone
        // rather than reissued to whoever asks next.
        let resumed = stopped_login(&surface, MFA_ENROLLMENT_REQUIRED_CODE).await;
        let reopened = opened_enrollment(&surface, &resumed).await;
        assert_eq!(reopened.status, StatusCode::OK);
        let reopened_wire = rendered(&reopened).await;
        assert_ne!(string_field(&body_text(&reopened), "secret"), secret);
        assert_ne!(string_field(&body_text(&reopened), "provisioning_uri"), uri);
        assert!(!reopened_wire.contains(&secret));
        assert!(!reopened_wire.contains(&uri));

        for record in delivered
            .lock()
            .expect("the delivered record log must not poison")
            .iter()
        {
            let text = record_text(record);
            assert!(!text.contains(&secret), "a record disclosed the secret");
            assert!(!text.contains(&uri), "a record disclosed the URI");
        }
    }

    // -----------------------------------------------------------------------
    // Enrollment confirmation
    // -----------------------------------------------------------------------

    /// An account whose username is at its bound still enrolls end to end.
    ///
    /// The account name is as long as a `Name` accepts, so its percent-encoded
    /// label cannot fit the provisioning URI the typed response profile is
    /// bounded to. The label is cosmetic and is shortened to fit rather than
    /// refused, so the account receives the secret, a conforming URI, and a
    /// ticket it can actually confirm. Refusing instead would spend the
    /// one-time enrollment claim on a response the account cannot act on and
    /// leave an MFA-required account permanently unable to sign in.
    ///
    /// The replay in the middle proves the shortened label bought nothing: the
    /// continuation the enrollment was opened from is still spent by that one
    /// opening, and the ticket that opening returned still confirms afterwards.
    #[tokio::test]
    async fn an_account_with_a_maximal_username_opens_and_confirms_an_enrollment() {
        let username = "u".repeat(MAX_NAME_LENGTH);
        let surface = AuthSurface::with_mfa(mfa_fixture(true, false, true).named(&username));

        let continuation = stopped_login(&surface, MFA_ENROLLMENT_REQUIRED_CODE).await;
        let opened = opened_enrollment(&surface, &continuation).await;
        assert_eq!(opened.status, StatusCode::OK);

        let uri = string_field(&body_text(&opened), "provisioning_uri");
        assert!(uri.len() <= MAX_PROVISIONING_URI_BYTES, "{}", uri.len());
        assert!(uri.starts_with("otpauth://totp/Weavelit:uuu"), "{uri}");
        assert!(uri.contains("~?secret="), "{uri}");
        assert!(uri.contains("&issuer=Weavelit&algorithm=SHA1&digits=6&period=30"));

        // The opening continuation is spent by the one enrollment it opened.
        let replayed = opened_enrollment(&surface, &continuation).await;
        assert_eq!(replayed.status, StatusCode::UNAUTHORIZED);
        assert!(!body_text(&replayed).contains("secret"));

        let (disclosed, ticket) = opened_parts(&opened);
        let code = current_code(&disclosed, surface.clock());
        let confirmed = confirm_code(&surface, &ticket, &code).await;
        assert_eq!(confirmed.status, StatusCode::OK);
        assert_eq!(set_cookie_lines(&rendered(&confirmed).await).len(), 2);

        // The account holds a factor from here on, so a later login stops at
        // the second factor rather than at enrollment again.
        stopped_login(&surface, MFA_REQUIRED_CODE).await;
    }

    /// Confirming an enrollment costs a verified password and a current code.
    ///
    /// The password half is the login that issues the enrollment continuation,
    /// so a wrong password reaches no enrollment at all; the code half is
    /// decided against the secret that enrollment disclosed. The correct code
    /// is refused when it is presented without the enrollment a verified
    /// password opened, and the same code is accepted once it is presented with
    /// it, so neither half is doing the other's work.
    ///
    /// Each refusal is followed by a login that still stops at enrollment, so a
    /// refusal that answered the right status while still writing a factor
    /// would not pass.
    #[tokio::test]
    async fn confirming_an_enrollment_requires_a_verified_password_and_a_current_code() {
        let surface = AuthSurface::with_mfa(mfa_fixture(true, false, true));

        // A wrong password issues no continuation, so nothing an unissued
        // ticket names can be opened.
        let denied = login(&surface, ACTIVE_USERNAME, WRONG_PASSWORD).await;
        assert_eq!(denied.status, StatusCode::UNAUTHORIZED);
        assert!(!body_text(&denied).contains("continuation"));
        let unopened = opened_enrollment(&surface, UNISSUED_TICKET).await;
        assert_eq!(unopened.status, StatusCode::UNAUTHORIZED);
        assert!(!body_text(&unopened).contains("secret"));

        let stopped = stopped_login(&surface, MFA_ENROLLMENT_REQUIRED_CODE).await;
        let opened = opened_enrollment(&surface, &stopped).await;
        assert_eq!(opened.status, StatusCode::OK);
        let (disclosed, ticket) = opened_parts(&opened);
        let code = current_code(&disclosed, ISSUED_AT);

        // The right code, with no enrollment a verified password opened.
        let unbacked = confirm_code(&surface, UNISSUED_TICKET, &code).await;
        assert_eq!(unbacked.status, StatusCode::UNAUTHORIZED);
        assert!(unbacked.cookies.is_none());
        assert!(!rendered(&unbacked).await.contains("Set-Cookie"));
        stopped_login(&surface, MFA_ENROLLMENT_REQUIRED_CODE).await;

        // A verified password's enrollment, with the wrong code.
        let stopped = stopped_login(&surface, MFA_ENROLLMENT_REQUIRED_CODE).await;
        let second = opened_enrollment(&surface, &stopped).await;
        assert_eq!(second.status, StatusCode::OK);
        let (other_secret, other_ticket) = opened_parts(&second);
        let wrong = code_from_another_secret(&other_secret, ISSUED_AT);
        let rejected = confirm_code(&surface, &other_ticket, &wrong).await;
        assert_eq!(rejected.status, StatusCode::UNAUTHORIZED);
        assert!(rejected.cookies.is_none());
        assert!(!rendered(&rejected).await.contains("Set-Cookie"));
        stopped_login(&surface, MFA_ENROLLMENT_REQUIRED_CODE).await;

        // Both halves, on the enrollment the first verified password opened.
        let confirmed = confirm_code(&surface, &ticket, &code).await;
        assert_eq!(confirmed.status, StatusCode::OK);
        assert_eq!(set_cookie_lines(&rendered(&confirmed).await).len(), 2);
        assert_eq!(
            body_text(&confirmed),
            format!(
                "{{\"result\":{{\"authenticated\":true}},\"correlation_id\":\"{}\"}}",
                correlation_of(&confirmed)
            )
        );

        // The account holds a factor from here on, so a login stops at the
        // second factor rather than at enrollment or at a session.
        stopped_login(&surface, MFA_REQUIRED_CODE).await;
    }

    /// A confirmed enrollment binds exactly the secret it disclosed.
    ///
    /// The secret is rebuilt from the response that disclosed it, so the code
    /// presented at the second-factor step is derived from that exact value.
    /// A code from another secret is refused at the same instant, and the
    /// disclosed secret's code is then accepted at that same instant, so the
    /// refusal is the secret rather than the clock.
    #[tokio::test]
    async fn a_confirmed_enrollment_binds_the_disclosed_secret_and_no_other() {
        let surface = AuthSurface::with_mfa(mfa_fixture(true, false, true));
        let stopped = stopped_login(&surface, MFA_ENROLLMENT_REQUIRED_CODE).await;
        let opened = opened_enrollment(&surface, &stopped).await;
        assert_eq!(opened.status, StatusCode::OK);
        let (disclosed, ticket) = opened_parts(&opened);

        let confirmed = confirm_code(&surface, &ticket, &current_code(&disclosed, ISSUED_AT)).await;
        assert_eq!(confirmed.status, StatusCode::OK);

        // A later step, because the confirming code's own step is spent.
        let later = ISSUED_AT + step_milliseconds(1);
        surface.set_clock(later);

        let stopped = stopped_login(&surface, MFA_REQUIRED_CODE).await;
        let foreign = code_from_another_secret(&disclosed, later);
        let refused = verify_code(&surface, &stopped, &foreign).await;
        assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
        assert!(refused.cookies.is_none());
        assert!(!rendered(&refused).await.contains("Set-Cookie"));

        let stopped = stopped_login(&surface, MFA_REQUIRED_CODE).await;
        let accepted = verify_code(&surface, &stopped, &current_code(&disclosed, later)).await;
        assert_eq!(accepted.status, StatusCode::OK);
        assert_eq!(set_cookie_lines(&rendered(&accepted).await).len(), 2);
    }

    /// The code that confirmed an enrollment cannot then satisfy a login.
    ///
    /// The enrollment writes the factor and the watermark that consumed the
    /// confirming code together, so presenting that same code inside its own
    /// time step is a replay. The last step presents the next step's code from
    /// the same secret and succeeds, which is what makes the refusal the spent
    /// step rather than the code or the factor.
    #[tokio::test]
    async fn the_code_that_confirmed_an_enrollment_cannot_satisfy_a_later_login() {
        let surface = AuthSurface::with_mfa(mfa_fixture(true, false, true));
        let stopped = stopped_login(&surface, MFA_ENROLLMENT_REQUIRED_CODE).await;
        let opened = opened_enrollment(&surface, &stopped).await;
        assert_eq!(opened.status, StatusCode::OK);
        let (disclosed, ticket) = opened_parts(&opened);
        let code = current_code(&disclosed, ISSUED_AT);

        let confirmed = confirm_code(&surface, &ticket, &code).await;
        assert_eq!(confirmed.status, StatusCode::OK);

        let stopped = stopped_login(&surface, MFA_REQUIRED_CODE).await;
        let replayed = verify_code(&surface, &stopped, &code).await;
        assert_eq!(replayed.status, StatusCode::UNAUTHORIZED);
        assert!(replayed.cookies.is_none());
        assert!(!rendered(&replayed).await.contains("Set-Cookie"));

        let later = ISSUED_AT + step_milliseconds(1);
        surface.set_clock(later);
        let stopped = stopped_login(&surface, MFA_REQUIRED_CODE).await;
        let accepted = verify_code(&surface, &stopped, &current_code(&disclosed, later)).await;
        assert_eq!(accepted.status, StatusCode::OK);
        assert_eq!(set_cookie_lines(&rendered(&accepted).await).len(), 2);
    }

    /// Disabling the module between opening and confirming refuses the
    /// confirmation.
    ///
    /// The deployment stops verifying second factors while an enrollment is in
    /// flight. The confirmation is still inside its window and carries a code
    /// the module really accepts, so the only thing refusing it is the
    /// enablement the write itself is decided against. Nothing is written and
    /// no session is issued, which is what keeps an `mfa_required` account from
    /// signing in behind a module the deployment is no longer willing to
    /// verify. Re-enabling and enrolling again then succeeds, so the refusal is
    /// the disabled module rather than a spent or broken enrollment.
    #[tokio::test]
    async fn an_enrollment_confirmed_after_the_module_was_disabled_writes_nothing() {
        let surface = AuthSurface::with_mfa(mfa_fixture(true, false, true));
        let stopped = stopped_login(&surface, MFA_ENROLLMENT_REQUIRED_CODE).await;
        let opened = opened_enrollment(&surface, &stopped).await;
        assert_eq!(opened.status, StatusCode::OK);
        let (disclosed, ticket) = opened_parts(&opened);

        // No account holds a factor yet, so the preview the decision is checked
        // against is zero and no live session is revoked by disabling.
        assert_eq!(surface.runtime.enrolled_accounts().unwrap(), 0);
        assert_eq!(
            surface.runtime.set_module_enabled(false, 0).unwrap(),
            MfaEnablementOutcome::Applied {
                revoked_sessions: 0
            }
        );

        // The clock has not moved, so the enrollment is well inside its window
        // and the code is current for the secret that enrollment disclosed.
        let code = current_code(&disclosed, surface.clock());
        let refused = confirm_code(&surface, &ticket, &code).await;
        assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
        assert!(refused.cookies.is_none());
        assert!(!rendered(&refused).await.contains("Set-Cookie"));
        assert_eq!(
            surface.runtime.enrolled_accounts().unwrap(),
            0,
            "a refused confirmation must persist no factor"
        );

        // The account is required to enroll, so with the module disabled its
        // login is denied outright rather than admitted without a factor.
        let denied = login(&surface, surface.username(), CORRECT_PASSWORD).await;
        assert_eq!(denied.status, StatusCode::UNAUTHORIZED);
        assert!(!rendered(&denied).await.contains("Set-Cookie"));

        // Enabling again and enrolling from scratch still succeeds.
        assert_eq!(
            surface.runtime.set_module_enabled(true, 0).unwrap(),
            MfaEnablementOutcome::Applied {
                revoked_sessions: 0
            }
        );
        let resumed = stopped_login(&surface, MFA_ENROLLMENT_REQUIRED_CODE).await;
        let reopened = opened_enrollment(&surface, &resumed).await;
        assert_eq!(reopened.status, StatusCode::OK);
        let (reopened_secret, reopened_ticket) = opened_parts(&reopened);
        let confirmed = confirm_code(
            &surface,
            &reopened_ticket,
            &current_code(&reopened_secret, surface.clock()),
        )
        .await;
        assert_eq!(confirmed.status, StatusCode::OK);
        assert_eq!(set_cookie_lines(&rendered(&confirmed).await).len(), 2);
        assert_eq!(surface.runtime.enrolled_accounts().unwrap(), 1);
    }

    // -----------------------------------------------------------------------
    // Self-enrollment from a live session
    // -----------------------------------------------------------------------

    /// Self-enrollment costs a live session, its own token, and the password.
    ///
    /// The two header preconditions are refused before a body exists, so they
    /// answer with the fixed pre-body body that carries no correlation
    /// identifier. Every refusal is followed by the engine's verification
    /// count, so a refusal that still paid for a verification would not pass.
    #[tokio::test]
    async fn self_enrollment_requires_a_live_session_its_token_and_the_current_password() {
        let surface = AuthSurface::with_mfa(mfa_fixture(true, false, false));
        let (session, csrf) = established_session(&surface).await;
        let verified = surface.engine.verifications();
        assert_eq!(
            verified, 1,
            "establishing the session costs one verification"
        );

        for (row, presented_session, presented_csrf) in [
            ("no session", None, Some(csrf.as_str())),
            ("no token", Some(session.as_str()), None),
            ("neither", None, None),
        ] {
            let response = self_enrollment(
                &surface,
                presented_session,
                presented_csrf,
                CORRECT_PASSWORD,
            )
            .await;
            assert_eq!(response.status, StatusCode::BAD_REQUEST, "{row}");
            assert_eq!(body_text(&response), "{\"error\":\"bad_request\"}", "{row}");
            assert!(response.cookies.is_none(), "{row}");
            assert_eq!(surface.engine.verifications(), verified, "{row}");
        }

        let body = format!("{{\"password\":\"{CORRECT_PASSWORD}\"}}");
        let foreign_origin = request(
            surface.surface(),
            default_timeouts(),
            self_enrollment_head(
                "https://weavelit.example:8443",
                Some(&session),
                Some(&csrf),
                body.len(),
            ),
            body,
        )
        .await;
        assert_eq!(foreign_origin.status, StatusCode::FORBIDDEN);
        assert_eq!(
            body_text(&foreign_origin),
            "{\"error\":\"request_origin_denied\"}"
        );
        assert_eq!(surface.engine.verifications(), verified);

        for (row, presented_session, presented_csrf) in [
            ("wrong token", session.as_str(), "not-this-sessions-token"),
            (
                "unknown session",
                "not-a-session-this-server-issued",
                csrf.as_str(),
            ),
        ] {
            let response = self_enrollment(
                &surface,
                Some(presented_session),
                Some(presented_csrf),
                CORRECT_PASSWORD,
            )
            .await;
            assert_eq!(response.status, StatusCode::UNAUTHORIZED, "{row}");
            assert!(body_text(&response).contains("session_invalid"), "{row}");
            assert!(response.cookies.is_none(), "{row}");
            assert_eq!(surface.engine.verifications(), verified, "{row}");
        }

        // A live session with the wrong password still enrolls nothing, and is
        // refused in the same vocabulary a wrong login password is.
        let wrong = self_enrollment(&surface, Some(&session), Some(&csrf), WRONG_PASSWORD).await;
        assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            body_text(&wrong),
            format!(
                "{{\"error\":\"authentication_failed\",\"correlation_id\":\"{}\"}}",
                correlation_of(&wrong)
            )
        );
        assert!(!body_text(&wrong).contains("secret"));
        assert_eq!(surface.engine.verifications(), verified + 1);

        // The correct password on the same session succeeds, so every refusal
        // above is its own check and not a broken request shape.
        let opened = self_enrollment(&surface, Some(&session), Some(&csrf), CORRECT_PASSWORD).await;
        assert_eq!(opened.status, StatusCode::OK);
        assert_eq!(surface.engine.verifications(), verified + 2);

        // A session outside its idle limit is no longer a session to enroll
        // from, whatever the password is.
        surface.set_clock(ISSUED_AT + SESSION_IDLE_TIMEOUT_MILLISECONDS);
        let expired =
            self_enrollment(&surface, Some(&session), Some(&csrf), CORRECT_PASSWORD).await;
        assert_eq!(expired.status, StatusCode::UNAUTHORIZED);
        assert!(body_text(&expired).contains("session_invalid"));
        assert_eq!(surface.engine.verifications(), verified + 2);
    }

    /// A self-enrollment discloses fresh data and enrolls once it is confirmed.
    ///
    /// Every needle is a value the first response really carried, so the
    /// searches over the second response's rendered bytes are for values that
    /// could have matched. They run over rendered bytes rather than a `Debug`
    /// rendering, because the bounded text types redact themselves there.
    #[tokio::test]
    async fn self_enrollment_discloses_fresh_provisioning_data_and_enrolls_on_confirmation() {
        let surface = AuthSurface::with_mfa(mfa_fixture(true, false, false));
        let (session, csrf) = established_session(&surface).await;

        let first = self_enrollment(&surface, Some(&session), Some(&csrf), CORRECT_PASSWORD).await;
        assert_eq!(first.status, StatusCode::OK);
        let first_body = body_text(&first);
        let first_secret = string_field(&first_body, "secret");
        let first_uri = string_field(&first_body, "provisioning_uri");
        assert!(!first_secret.is_empty());
        assert!(!first_uri.is_empty());
        assert_eq!(
            string_field(&first_body, "enrollment").len(),
            CONTINUATION_TEXT_BYTES
        );
        let first_wire = rendered(&first).await;
        assert!(first_wire.contains(&first_secret));
        assert!(first_wire.contains(&first_uri));
        assert!(first_uri.contains(&format!("secret={first_secret}")));

        // Re-opening issues a new secret rather than redisclosing the first.
        let second = self_enrollment(&surface, Some(&session), Some(&csrf), CORRECT_PASSWORD).await;
        assert_eq!(second.status, StatusCode::OK);
        let second_wire = rendered(&second).await;
        let (disclosed, ticket) = opened_parts(&second);
        assert_ne!(string_field(&body_text(&second), "secret"), first_secret);
        assert_ne!(
            string_field(&body_text(&second), "provisioning_uri"),
            first_uri
        );
        assert!(!second_wire.contains(&first_secret));
        assert!(!second_wire.contains(&first_uri));

        // Confirming the second enrollment binds it and enrolls the account.
        let confirmed = confirm_code(&surface, &ticket, &current_code(&disclosed, ISSUED_AT)).await;
        assert_eq!(confirmed.status, StatusCode::OK);
        assert_eq!(set_cookie_lines(&rendered(&confirmed).await).len(), 2);
        assert!(!rendered(&confirmed).await.contains(&first_secret));

        // The account holds a factor from here on, so its next login stops at
        // the second factor rather than establishing a session directly.
        stopped_login(&surface, MFA_REQUIRED_CODE).await;
    }

    /// Self-enrollment enters the single verification lane exactly once.
    ///
    /// The route re-verifies the account's password, so it takes the same
    /// single permit a login takes: with that permit held the request is
    /// refused at the admission deadline having verified nothing, and once it
    /// is released each attempt costs exactly one verification whether the
    /// password was right or wrong.
    #[tokio::test]
    async fn self_enrollment_takes_the_single_password_verification_lane_exactly_once() {
        assert_eq!(MAX_CONCURRENT_LOGIN_VERIFICATIONS, 1);

        let surface = AuthSurface::with_mfa(mfa_fixture(true, false, false));
        let (session, csrf) = established_session(&surface).await;
        let verified = surface.engine.verifications();
        assert_eq!(verified, 1);

        let held = Arc::clone(&surface.runtime.login_lane)
            .try_acquire_owned()
            .expect("the single login permit must be free once the login returned");
        let body = format!("{{\"password\":\"{CORRECT_PASSWORD}\"}}");
        let unadmitted = request(
            surface.surface(),
            ConnectionTimeouts {
                handshake: TLS_HANDSHAKE_TIMEOUT,
                request_read: REQUEST_READ_TIMEOUT,
                processing: SHORT_PROCESSING,
            },
            self_enrollment_head(
                &format!("https://{UNBOUND_LISTENER}"),
                Some(&session),
                Some(&csrf),
                body.len(),
            ),
            body,
        )
        .await;
        assert_eq!(unadmitted.status, StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(body_text(&unadmitted), "{\"error\":\"gateway_timeout\"}");
        assert_eq!(
            surface.engine.verifications(),
            verified,
            "no verification may begin for a request that was never admitted"
        );
        drop(held);

        let denied = self_enrollment(&surface, Some(&session), Some(&csrf), WRONG_PASSWORD).await;
        assert_eq!(denied.status, StatusCode::UNAUTHORIZED);
        assert_eq!(surface.engine.verifications(), verified + 1);

        let opened = self_enrollment(&surface, Some(&session), Some(&csrf), CORRECT_PASSWORD).await;
        assert_eq!(opened.status, StatusCode::OK);
        assert_eq!(surface.engine.verifications(), verified + 2);
        assert_eq!(surface.engine.hashes(), 0);

        // The permit is released once the request has returned, so a held
        // permit above was the admission lane and not a leak.
        assert!(
            surface.runtime.login_lane.try_acquire().is_ok(),
            "the single permit must be free again once the request returned"
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
