#![forbid(unsafe_code)]

//! Shared Client Module contract for the Weavelit Server's restricted
//! pre-operational surface.
//!
//! This crate owns the request schemas, validation, handlers, fixed response
//! profile, canonical route paths, and capability declaration that every Client
//! Module shares. A per-client crate owns only what genuinely differs for its
//! client, so two Client Modules that declare the same function cannot diverge.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::Request,
    http::{
        HeaderMap, HeaderValue, Method, StatusCode,
        header::{
            ACCEPT, ALLOW, AsHeaderName, CONTENT_LENGTH, CONTENT_TYPE, HOST, ORIGIN,
            TRANSFER_ENCODING,
        },
    },
    response::Response,
    routing::{MethodRouter, any},
};
use serde::Deserialize;
use weavelit_server_lifecycle::{LifecycleProjection, SelectionFailureKind};
use zeroize::Zeroize;

pub mod administration;
pub mod authentication;
pub mod authorization;
pub mod cookie;
pub mod credential_issuance;
pub mod init;
pub mod mfa;
pub mod mfa_policy;
pub mod password_change;
pub mod reconciliation;
pub mod restore;
pub mod typed_json;

pub use administration::{
    ACCOUNTS_LIST_ROUTE, ACCOUNTS_STATUS_ROUTE, ACCOUNTS_VIEW_ROUTE,
    AccountAdministrationCapability, AccountAdministrationDeclaration,
    AccountAdministrationEnvelope, AccountAdministrationInputRejected,
    AccountAdministrationProjection, AccountAdministrationRejection, AccountAdministrationRequest,
    AccountAdministrationResult, AccountAdministrationSubmission, AccountsListRequest,
    AccountsPage, AccountsStatusRequest, AccountsViewRequest, DEFAULT_ACCOUNTS_PAGE_LIMIT,
    MAX_ACCOUNT_ADMINISTRATION_BODY_BYTES, MAX_ACCOUNT_ADMINISTRATION_RESPONSE_BYTES,
    MAX_ACCOUNTS_PAGE_LIMIT,
};
pub use authentication::{
    AUTH_LOGIN_ROUTE, AUTH_LOGOUT_ROUTE, AUTH_SESSION_ROUTE, AuthenticationCapability,
    AuthenticationDeclaration, AuthenticationRejection, CorrelationSource, LoginCommit,
    LoginOutcome, LoginSubmission, MAX_LOGIN_BODY_BYTES, SessionEstablished, SessionIdentity,
    SessionRevoke, SessionSubmission, SessionValidate, submitted_csrf_token,
    submitted_session_token, validate_login_request, validate_session_request,
};
pub use authorization::{
    AUTHORIZATION_DENIED_CODE, AUTHORIZATION_DENIED_STATUS, AuthorizationRejection,
};
pub use cookie::{
    CSRF_COOKIE_NAME, CookieEffect, CookieLines, CookieValue, MAX_COOKIE_HEADER_BYTES,
    MAX_COOKIE_LINES, MAX_COOKIE_VALUE_BYTES, SESSION_COOKIE_NAME,
};
pub use credential_issuance::{
    ACCOUNTS_CREATE_ROUTE, ACCOUNTS_RESET_PASSWORD_ROUTE, AccountCreateCommit,
    AccountCreateSubmission, AccountCredentialIssued, AccountPasswordResetCommit,
    AccountPasswordResetSubmission, CREDENTIAL_ISSUANCE_STEP_UP_ROUTE,
    CredentialIssuanceCapability, CredentialIssuanceDeclaration, CredentialIssuanceRejection,
    CredentialIssuanceStepUpCommit, CredentialIssuanceStepUpSubmission,
    CredentialIssuanceTicketIssued, MAX_CREDENTIAL_ISSUANCE_BODY_BYTES,
    MAX_CREDENTIAL_ISSUANCE_PASSWORD_BYTES, validate_credential_issuance_request,
};
pub use init::{
    INIT_RECOVERY_KEY_ROUTE, INIT_ROUTE, InitAdministratorSubmission, InitCapability,
    InitCompleted, InitDeclaration, InitFinalizeCommit, InitFinalizeSubmission,
    InitLogModuleSettingSubmission, InitLogModuleSubmission, InitProtectedSettingSubmission,
    InitRecoveryKeyCommit, InitRecoveryKeyPrepared, InitRecoveryKeySubmission, InitRejection,
    InitRequestSubmission, MAX_INIT_BODY_BYTES, MAX_INIT_LOG_MODULE_SETTINGS, MAX_INIT_LOG_MODULES,
    MAX_INIT_PROTECTED_LOG_MODULE_SETTINGS, RECOVERY_PROOF_BASE64_CHARS, validate_init_request,
};
pub use mfa::{
    AUTH_MFA_ENROLLMENT_CONFIRM_ROUTE, AUTH_MFA_ENROLLMENT_ROUTE, AUTH_MFA_SELF_ENROLLMENT_ROUTE,
    AUTH_MFA_VERIFY_ROUTE, MAX_MFA_BODY_BYTES, MFA_CODE_DIGITS, MFA_ENROLLMENT_REQUIRED_CODE,
    MFA_REQUIRED_CODE, MfaCapability, MfaCodeCommit, MfaCodeSubmission, MfaDeclaration,
    MfaEnrollmentCommit, MfaEnrollmentConfirmCommit, MfaEnrollmentOpened, MfaEnrollmentSubmission,
    MfaSelfEnrollmentCommit, MfaSelfEnrollmentSubmission,
};
pub use mfa_policy::{
    ACCOUNTS_MFA_REQUIREMENT_ROUTE, ACCOUNTS_MFA_RESET_ROUTE, MAX_MFA_POLICY_BODY_BYTES,
    MFA_POLICY_STEP_UP_ROUTE, MfaPolicyCapability, MfaPolicyDeclaration, MfaPolicyRejection,
    MfaPolicyStepUpFamily, MfaPolicyStepUpSubmission, MfaPolicyTicketIssued,
    MfaRequirementSubmission, MfaResetSubmission, validate_mfa_policy_request,
};
pub use password_change::{
    AUTH_PASSWORD_CHANGE_ROUTE, MAX_PASSWORD_CHANGE_BODY_BYTES, MAX_PASSWORD_CHANGE_PASSWORD_BYTES,
    PasswordChangeCapability, PasswordChangeCommit, PasswordChangeDeclaration,
    PasswordChangeSubmission, validate_password_change_request,
};
pub use reconciliation::{
    LIFECYCLE_RECONCILIATION_ROUTE, MAX_RECONCILIATION_BODY_BYTES, ReconciliationCapability,
    ReconciliationCommit, ReconciliationOutcome, ReconciliationRejection, ReconciliationSubmission,
    reconciliation_outcome_response, validate_reconciliation_request,
};
pub use restore::{
    MAX_RESTORE_KEY_BODY_BYTES, RESTORE_ARTIFACT_ROUTE, RESTORE_ROUTE, RESTORE_TICKET_HEADER_NAME,
    RestoreArtifactCommit, RestoreArtifactSubmission, RestoreCapability, RestoreCompleted,
    RestoreDeclaration, RestoreKeyCommit, RestoreKeySubmission, RestoreRejection,
    RestoreTicketIssued, submitted_restore_ticket, validate_restore_artifact_request,
    validate_restore_key_request,
};

/// The canonical route for the live pre-operational lifecycle projection.
pub const STATUS_ROUTE: &str = "/api/v1/status";

/// The sole pre-operational route that may change lifecycle state.
pub const APPLICATION_DATABASE_ROUTE: &str = "/api/v1/application-database";

/// Live lifecycle projection source the Server core supplies to a mounted route.
///
/// A Client Module holds no lifecycle state of its own; it calls this once per
/// request, so a response never reports a value captured at startup. `None`
/// means the trusted lifecycle boundary could not be read.
pub type ProjectionSource = Arc<
    dyn Fn() -> Pin<Box<dyn Future<Output = Option<LifecycleProjection>> + Send>> + Send + Sync,
>;

/// Server-core commit hook for a validated Application Database selection.
///
/// A Client Module is not the selection authority: it hands the validated
/// backend to this hook, which owns the lifecycle mutation and returns the
/// projection observed under the same mutation permit.
pub type SelectionCommit = Arc<
    dyn Fn(
            SelectedBackend,
        ) -> Pin<
            Box<
                dyn Future<Output = Result<LifecycleProjection, DatabaseSelectionRejection>> + Send,
            >,
        > + Send
        + Sync,
>;

// ---------------------------------------------------------------------------
// Capability declaration
// ---------------------------------------------------------------------------

/// A Client Module's declared pre-operational interface surface.
///
/// Presence is the declaration. A capability exists only once its collaborators
/// were supplied, so a Client Module can neither claim a capability it did not
/// supply nor supply one it did not claim. Declaration determines interface
/// capability only; the Server core independently authorizes every request.
#[derive(Default)]
pub struct PreoperationalSurface {
    status: Option<ProjectionSource>,
    database_selection: Option<(ExpectedOrigin, SelectionCommit)>,
    restore: Option<RestoreDeclaration>,
    init: Option<InitDeclaration>,
    assets: Option<Router>,
}

impl PreoperationalSurface {
    /// Declares the live status projection capability.
    pub fn with_status(mut self, projection: ProjectionSource) -> Self {
        self.status = Some(projection);
        self
    }

    /// Declares the Application Database selection capability.
    pub fn with_database_selection(
        mut self,
        expected_origin: ExpectedOrigin,
        commit: SelectionCommit,
    ) -> Self {
        self.database_selection = Some((expected_origin, commit));
        self
    }

    /// Declares the two-step Restore submission capability.
    ///
    /// Restore is declared only where it is eligible. A surface composed before
    /// an Application Database has been selected, a fail-closed surface, and an
    /// operational surface carry no declaration at all, so neither route exists
    /// to be denied.
    pub fn with_restore(mut self, capability: RestoreCapability) -> Self {
        self.restore = Some(RestoreDeclaration::new(capability));
        self
    }

    /// Separates the declared Restore capability from the rest of the surface.
    ///
    /// Both Restore routes need a transport registration the Server core owns,
    /// and a registration may only travel with the mount that serves it. This
    /// hands the declaration back so the core mounts each Restore route
    /// together with its registration, and mounts everything else through
    /// [`PreoperationalSurface::mount`].
    #[must_use]
    pub fn split_restore(mut self) -> (Self, Option<RestoreDeclaration>) {
        let restore = self.restore.take();
        (self, restore)
    }

    /// Declares the two-step Init submission capability.
    ///
    /// Init is declared only where it is eligible, exactly as Restore is. A
    /// surface composed before an Application Database has been selected, a
    /// fail-closed surface, and an operational surface carry no declaration at
    /// all, so neither Init route exists to be denied.
    pub fn with_init(mut self, capability: InitCapability) -> Self {
        self.init = Some(InitDeclaration::new(capability));
        self
    }

    /// Separates the declared Init capability from the rest of the surface.
    ///
    /// Both Init routes need a transport registration the Server core owns,
    /// and the two are never published together: the finalization route
    /// becomes reachable only once the recovery key has actually been
    /// delivered. Handing the declaration back is what lets the core mount
    /// each route with its own registration at its own moment.
    #[must_use]
    pub fn split_init(mut self) -> (Self, Option<InitDeclaration>) {
        let init = self.init.take();
        (self, init)
    }

    /// Declares client-specific asset delivery, which owns its own exact paths.
    pub fn with_assets(mut self, assets: Router) -> Self {
        self.assets = Some(assets);
        self
    }

    /// Mounts every declared capability at its canonical path.
    ///
    /// An undeclared capability is absent rather than present and denied, so
    /// the Server core mounts exactly what the Client Module returned.
    ///
    /// A Restore declaration this surface still holds is mounted here without
    /// a transport registration, which grants it only the listener's default
    /// body bound. A composer that must serve real artifact uploads takes the
    /// declaration through [`PreoperationalSurface::split_restore`] first and
    /// mounts each route together with its registration. An Init declaration
    /// this surface still holds is mounted the same way, and mounts both Init
    /// routes at once; a composer that must publish finalization only after a
    /// delivered key takes the declaration through
    /// [`PreoperationalSurface::split_init`] first.
    pub fn mount(self, router: Router) -> Router {
        let router = match self.status {
            Some(projection) => router.route(STATUS_ROUTE, preoperational_status_route(projection)),
            None => router,
        };
        let router = match self.database_selection {
            Some((expected_origin, commit)) => router.route(
                APPLICATION_DATABASE_ROUTE,
                database_selection_route(expected_origin, commit),
            ),
            None => router,
        };
        let router = match self.restore {
            Some(restore) => router
                .route(restore::RESTORE_ROUTE, restore.key_route())
                .route(restore::RESTORE_ARTIFACT_ROUTE, restore.artifact_route()),
            None => router,
        };
        let router = match self.init {
            Some(init) => router
                .route(init::INIT_RECOVERY_KEY_ROUTE, init.recovery_key_route())
                .route(init::INIT_ROUTE, init.finalize_route()),
            None => router,
        };
        match self.assets {
            Some(assets) => router.merge(assets),
            None => router,
        }
    }
}

/// A Client Module's declared operational interface surface.
///
/// Presence is the declaration, exactly as it is for
/// [`PreoperationalSurface`]. This surface has no status or Application
/// Database capability: those are pre-operational contracts, so a sealed
/// deployment cannot mount them at all rather than mounting and denying them.
#[derive(Default)]
pub struct OperationalSurface {
    account_administration: Option<AccountAdministrationDeclaration>,
    credential_issuance: Option<CredentialIssuanceDeclaration>,
    mfa_policy: Option<MfaPolicyDeclaration>,
    assets: Option<Router>,
}

impl OperationalSurface {
    /// Declares authenticated account reads and status changes.
    pub fn with_account_administration(
        mut self,
        capability: AccountAdministrationCapability,
    ) -> Self {
        self.account_administration = Some(AccountAdministrationDeclaration::new(capability));
        self
    }

    /// Separates account routes so the Server can mount each with its transport registration.
    #[must_use]
    pub fn split_account_administration(
        mut self,
    ) -> (Self, Option<AccountAdministrationDeclaration>) {
        let declaration = self.account_administration.take();
        (self, declaration)
    }

    /// Declares credential assurance, account creation, and password reset.
    pub fn with_credential_issuance(mut self, capability: CredentialIssuanceCapability) -> Self {
        self.credential_issuance = Some(CredentialIssuanceDeclaration::new(capability));
        self
    }

    /// Separates credential issuance so each route retains its registration.
    #[must_use]
    pub fn split_credential_issuance(mut self) -> (Self, Option<CredentialIssuanceDeclaration>) {
        let declaration = self.credential_issuance.take();
        (self, declaration)
    }

    /// Declares TOTP step-up, MFA requirement changes, and enrollment reset.
    pub fn with_mfa_policy(mut self, capability: MfaPolicyCapability) -> Self {
        self.mfa_policy = Some(MfaPolicyDeclaration::new(capability));
        self
    }

    /// Separates MFA policy routes so each retains its transport registration.
    #[must_use]
    pub fn split_mfa_policy(mut self) -> (Self, Option<MfaPolicyDeclaration>) {
        let declaration = self.mfa_policy.take();
        (self, declaration)
    }

    /// Declares client-specific asset delivery, which owns its own exact paths.
    pub fn with_assets(mut self, assets: Router) -> Self {
        self.assets = Some(assets);
        self
    }

    /// Mounts every declared capability at its canonical path.
    pub fn mount(self, router: Router) -> Router {
        let router = match self.account_administration {
            Some(administration) => router
                .route(ACCOUNTS_LIST_ROUTE, administration.list_route())
                .route(ACCOUNTS_VIEW_ROUTE, administration.view_route())
                .route(ACCOUNTS_STATUS_ROUTE, administration.status_route()),
            None => router,
        };
        let router = match self.credential_issuance {
            Some(credential_issuance) => router
                .route(
                    CREDENTIAL_ISSUANCE_STEP_UP_ROUTE,
                    credential_issuance.step_up_route(),
                )
                .route(ACCOUNTS_CREATE_ROUTE, credential_issuance.create_route())
                .route(
                    ACCOUNTS_RESET_PASSWORD_ROUTE,
                    credential_issuance.reset_password_route(),
                ),
            None => router,
        };
        let router = match self.mfa_policy {
            Some(mfa_policy) => router
                .route(MFA_POLICY_STEP_UP_ROUTE, mfa_policy.step_up_route())
                .route(
                    ACCOUNTS_MFA_REQUIREMENT_ROUTE,
                    mfa_policy.requirement_route(),
                )
                .route(ACCOUNTS_MFA_RESET_ROUTE, mfa_policy.reset_route()),
            None => router,
        };
        match self.assets {
            Some(assets) => router.merge(assets),
            None => router,
        }
    }
}

/// Returns the Client Module route for the live status projection.
pub fn preoperational_status_route(projection: ProjectionSource) -> MethodRouter {
    any(move |request| status_response(request, Arc::clone(&projection)))
}

/// Returns the Client Module route for Application Database selection.
///
/// The route validates the method, same-origin and CSRF preconditions, media
/// types, and request schema, then delegates the decision to `commit`.
pub fn database_selection_route(
    expected_origin: ExpectedOrigin,
    commit: SelectionCommit,
) -> MethodRouter {
    any(move |request| {
        database_selection_route_response(request, expected_origin, Arc::clone(&commit))
    })
}

async fn database_selection_route_response(
    request: Request,
    expected_origin: ExpectedOrigin,
    commit: SelectionCommit,
) -> Response {
    let (parts, body) = request.into_parts();
    if let Err(rejection) =
        validate_database_selection_request(&parts.method, &parts.headers, expected_origin)
    {
        return rejection.response();
    }
    let Ok(body) = to_bytes(body, MAX_DATABASE_SELECTION_BODY_BYTES).await else {
        return DatabaseSelectionRejection::BadRequest.response();
    };
    let selection = match DatabaseSelectionRequest::from_json(&body) {
        Ok(selection) => selection,
        Err(rejection) => return rejection.response(),
    };
    match commit(selection.backend()).await {
        Ok(projection) => database_selection_response(&projection),
        Err(rejection) => rejection.response(),
    }
}

async fn status_response(request: Request, projection: ProjectionSource) -> Response {
    let (parts, _body) = request.into_parts();
    if parts.method != Method::GET {
        return json_response_with_allow(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
    }
    if has_request_body(&parts.headers) || !accepts_json(&parts.headers) {
        return json_response(StatusCode::BAD_REQUEST, "bad_request");
    }

    let Some(projection) = projection().await else {
        return json_response(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable");
    };
    json_response_body(StatusCode::OK, projection_body(&projection))
}

// ---------------------------------------------------------------------------
// Application Database selection contract
// ---------------------------------------------------------------------------

/// Largest request body a Client Module accepts for database selection.
pub const MAX_DATABASE_SELECTION_BODY_BYTES: usize = 1024;

/// The exact request media type and the only negotiable response media type.
pub(crate) const JSON_MEDIA_TYPE: &[u8] = b"application/json";

/// Non-simple header a browser cannot send cross-site without a preflight.
pub const CSRF_HEADER_NAME: &str = "x-weavelit-csrf";

/// The only accepted value of [`CSRF_HEADER_NAME`].
const CSRF_HEADER_VALUE: &[u8] = b"1";

/// Default port of the `https` scheme, which an authority may omit.
const HTTPS_DEFAULT_PORT: u16 = 443;

/// Compiled-in **Application Database** backend a client may select.
///
/// Version 1 accepts exactly one literal, so an unknown or renamed backend value
/// is a request error rather than a negotiated capability.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub enum SelectedBackend {
    /// The Milestone 1 MVP backend, wire literal `sqlite`.
    #[serde(rename = "sqlite")]
    Sqlite,
}

impl SelectedBackend {
    /// Returns the stable wire literal for this backend.
    pub const fn identifier(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
        }
    }
}

/// The connection settings object, which carries no client-supplied value.
///
/// The Server derives every SQLite artifact path, so the only accepted value is
/// an empty JSON object. The manual implementation is required because a derived
/// empty struct also accepts an empty sequence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EmptyConnectionSettings;

impl<'de> Deserialize<'de> for EmptyConnectionSettings {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct EmptyObject;

        impl<'de> serde::de::Visitor<'de> for EmptyObject {
            type Value = EmptyConnectionSettings;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an empty JSON object")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                match map.next_key::<serde::de::IgnoredAny>()? {
                    Some(_) => Err(serde::de::Error::custom("unexpected connection setting")),
                    None => Ok(EmptyConnectionSettings),
                }
            }
        }

        deserializer.deserialize_map(EmptyObject)
    }
}

/// A strictly validated Application Database selection request body.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DatabaseSelectionRequest {
    backend: SelectedBackend,
    settings: EmptyConnectionSettings,
}

impl DatabaseSelectionRequest {
    /// Parses the exact accepted body `{"backend":"sqlite","settings":{}}`.
    ///
    /// Insignificant whitespace and member ordering are accepted. An unknown
    /// field, duplicate key, missing field, wrongly typed value, unknown backend
    /// literal, trailing content, empty body, or oversized body is rejected.
    pub fn from_json(body: &[u8]) -> Result<Self, DatabaseSelectionRejection> {
        if body.len() > MAX_DATABASE_SELECTION_BODY_BYTES {
            return Err(DatabaseSelectionRejection::BadRequest);
        }
        let mut deserializer = serde_json::Deserializer::from_slice(body);
        let request = Self::deserialize(&mut deserializer)
            .map_err(|_| DatabaseSelectionRejection::BadRequest)?;
        deserializer
            .end()
            .map_err(|_| DatabaseSelectionRejection::BadRequest)?;
        Ok(request)
    }

    /// Returns the selected backend.
    pub const fn backend(&self) -> SelectedBackend {
        self.backend
    }
}

/// The complete, payload-free rejection contract for database selection.
///
/// Each variant carries no diagnostic detail; the fixed body is the whole
/// response payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatabaseSelectionRejection {
    /// `400` for a malformed body, media type, or `Accept` value.
    BadRequest,
    /// `403` for a failed same-origin, `Host`, or CSRF header check.
    RequestOriginDenied,
    /// `405` for any method other than `PUT`.
    MethodNotAllowed,
    /// `409` for a lifecycle state that no longer permits selection.
    DatabaseSelectionNotAllowed,
    /// `503` for a backend, persistence, or integrity failure.
    ServiceUnavailable,
}

impl DatabaseSelectionRejection {
    /// Maps a lifecycle selection failure family onto this transport contract.
    pub const fn from_selection_failure(kind: SelectionFailureKind) -> Self {
        match kind {
            SelectionFailureKind::RequestInvalid => Self::BadRequest,
            SelectionFailureKind::Conflict => Self::DatabaseSelectionNotAllowed,
            SelectionFailureKind::Unavailable => Self::ServiceUnavailable,
        }
    }

    /// Returns the documented status code.
    pub const fn status(self) -> StatusCode {
        match self {
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::RequestOriginDenied => StatusCode::FORBIDDEN,
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::DatabaseSelectionNotAllowed => StatusCode::CONFLICT,
            Self::ServiceUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    /// Returns the documented fixed JSON body.
    pub const fn body(self) -> &'static str {
        match self {
            Self::BadRequest => "{\"error\":\"bad_request\"}",
            Self::RequestOriginDenied => "{\"error\":\"request_origin_denied\"}",
            Self::MethodNotAllowed => "{\"error\":\"method_not_allowed\"}",
            Self::DatabaseSelectionNotAllowed => "{\"error\":\"database_selection_not_allowed\"}",
            Self::ServiceUnavailable => "{\"error\":\"service_unavailable\"}",
        }
    }

    /// Builds the fixed response, including `Allow: PUT` for `405`.
    pub fn response(self) -> Response {
        let mut response = json_response_body(self.status(), self.body());
        if self == Self::MethodNotAllowed {
            response
                .headers_mut()
                .insert(ALLOW, HeaderValue::from_static("PUT"));
        }
        response
    }
}

/// Returns the success response for a completed database selection.
pub fn database_selection_response(projection: &LifecycleProjection) -> Response {
    json_response_body(StatusCode::OK, projection_body(projection))
}

/// Returns the single projection body shape both pre-operational routes emit.
const fn projection_body(projection: &LifecycleProjection) -> &'static str {
    if projection.database_selected() {
        "{\"lifecycle\":\"uninitialized\",\"database_selected\":true}"
    } else {
        "{\"lifecycle\":\"uninitialized\",\"database_selected\":false}"
    }
}

/// The single authority a state-changing request must target.
///
/// It is derived only from the trusted listener address the Server actually
/// bound. It is never derived from a certificate subject alternative name or
/// from any request header, so a client cannot influence what it is compared
/// against.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExpectedOrigin {
    address: IpAddr,
    port: u16,
}

impl ExpectedOrigin {
    /// Derives the expected authority from the bound listener address.
    pub const fn from_listener(listener: SocketAddr) -> Self {
        Self {
            address: listener.ip(),
            port: listener.port(),
        }
    }

    /// Validates the request `Origin`, `Host`, and CSRF headers.
    ///
    /// Requires exactly one `Origin`, exactly one `Host`, and exactly one
    /// [`CSRF_HEADER_NAME`] header whose value is `1`. Both authorities must
    /// normalize to this expected IP literal and effective port, and the origin
    /// must use the `https` scheme.
    pub fn validate(self, headers: &HeaderMap) -> Result<(), DatabaseSelectionRejection> {
        if self.is_trusted(headers) {
            Ok(())
        } else {
            Err(DatabaseSelectionRejection::RequestOriginDenied)
        }
    }

    /// Reports whether the request satisfies the same-origin and CSRF checks.
    ///
    /// Every state-changing pre-operational route shares this one predicate, so
    /// two routes cannot disagree about what a trusted request looks like.
    #[must_use]
    pub fn is_trusted(self, headers: &HeaderMap) -> bool {
        let Some(csrf) = single_header(headers, CSRF_HEADER_NAME) else {
            return false;
        };
        if csrf.as_bytes() != CSRF_HEADER_VALUE {
            return false;
        }

        self.is_same_origin(headers)
    }

    /// Reports whether the request's `Origin` and `Host` are this authority.
    ///
    /// This is the origin half of [`Self::is_trusted`] alone. A route uses it
    /// only when it carries its own cross-site request forgery token instead
    /// of the pre-session literal, so the two checks cannot drift apart.
    #[must_use]
    pub fn is_same_origin(self, headers: &HeaderMap) -> bool {
        let (Some(origin), Some(host)) =
            (single_header(headers, ORIGIN), single_header(headers, HOST))
        else {
            return false;
        };

        let origin_authority = origin
            .to_str()
            .ok()
            .and_then(|value| value.strip_prefix("https://"))
            .and_then(normalize_authority);
        let host_authority = host.to_str().ok().and_then(normalize_authority);

        let expected = Some((self.address, self.port));
        origin_authority == expected && host_authority == expected
    }
}

/// Normalizes an authority to an IP literal and its effective `https` port.
///
/// Accepts a bare IPv4 literal or a bracketed IPv6 literal, with the port either
/// explicit or omitted for the `https` default. Rejects a DNS name, unbracketed
/// IPv6 literal, userinfo, path, query, fragment, empty port, or non-numeric
/// port.
fn normalize_authority(authority: &str) -> Option<(IpAddr, u16)> {
    if authority
        .bytes()
        .any(|byte| matches!(byte, b'@' | b'/' | b'?' | b'#') || byte <= b' ' || byte >= 0x7f)
    {
        return None;
    }
    let (address, port) = match authority.strip_prefix('[') {
        Some(rest) => {
            let (literal, remainder) = rest.split_once(']')?;
            (IpAddr::V6(literal.parse::<Ipv6Addr>().ok()?), remainder)
        }
        None => {
            let (literal, remainder) = match authority.split_once(':') {
                Some((literal, port)) => (literal, Some(port)),
                None => (authority, None),
            };
            let address = IpAddr::V4(literal.parse::<Ipv4Addr>().ok()?);
            return Some((
                address,
                match remainder {
                    Some(port) => parse_explicit_port(port)?,
                    None => HTTPS_DEFAULT_PORT,
                },
            ));
        }
    };
    let port = match port.strip_prefix(':') {
        Some(explicit) => parse_explicit_port(explicit)?,
        None if port.is_empty() => HTTPS_DEFAULT_PORT,
        None => return None,
    };
    Some((address, port))
}

/// Parses an explicit port with no leading zero, sign, or empty value.
fn parse_explicit_port(port: &str) -> Option<u16> {
    if port.is_empty() || (port.len() > 1 && port.starts_with('0')) {
        return None;
    }
    if !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    port.parse().ok()
}

/// Validates the request media type and response negotiation headers.
///
/// Requires exactly one `Content-Type: application/json`, and either no `Accept`
/// header or exactly one `Accept: application/json`.
pub fn validate_database_selection_media(
    headers: &HeaderMap,
) -> Result<(), DatabaseSelectionRejection> {
    let content_type =
        single_header(headers, CONTENT_TYPE).ok_or(DatabaseSelectionRejection::BadRequest)?;
    if content_type.as_bytes() != JSON_MEDIA_TYPE || !accepts_json(headers) {
        return Err(DatabaseSelectionRejection::BadRequest);
    }
    Ok(())
}

/// Validates every header precondition for an Application Database selection.
///
/// The same-origin and CSRF trust check runs before media-type validation so a
/// cross-site request is denied without revealing body or negotiation detail.
pub fn validate_database_selection_request(
    method: &Method,
    headers: &HeaderMap,
    expected_origin: ExpectedOrigin,
) -> Result<(), DatabaseSelectionRejection> {
    if method != Method::PUT {
        return Err(DatabaseSelectionRejection::MethodNotAllowed);
    }
    expected_origin.validate(headers)?;
    validate_database_selection_media(headers)
}

pub(crate) fn single_header<N: AsHeaderName>(headers: &HeaderMap, name: N) -> Option<&HeaderValue> {
    let mut values = headers.get_all(name).iter();
    match (values.next(), values.next()) {
        (Some(value), None) => Some(value),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Shared request-body clearing
// ---------------------------------------------------------------------------

/// A collected request body this crate owns uniquely and clears when dropped.
///
/// The login, Init, second-factor, and Restore bodies carry plaintext secret
/// material, so the collected bytes must not outlive their handler as readable
/// memory. The wipe runs in `Drop` rather than at one exit point, so every
/// return path — parsed, rejected, or added later — clears the same buffer.
///
/// This is defense in depth, not a whole-process guarantee: the transport
/// layer's own read buffers are outside this crate's control, so what is
/// promised here is that this crate retains no unwiped copy.
pub(crate) struct WipedBody<B: AsMut<[u8]>> {
    buffer: B,
}

impl<B: AsMut<[u8]>> WipedBody<B> {
    pub(crate) fn new(buffer: B) -> Self {
        Self { buffer }
    }

    pub(crate) fn bytes(&mut self) -> &[u8] {
        self.buffer.as_mut()
    }
}

impl<B: AsMut<[u8]>> Drop for WipedBody<B> {
    fn drop(&mut self) {
        self.buffer.as_mut().zeroize();
    }
}

/// Shared observation helpers for the release-time body-clearing guard.
#[cfg(test)]
pub(crate) mod wiped_body_support {
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A buffer that records its own contents at the moment it is dropped.
    ///
    /// [`super::WipedBody`] clears the buffer in its own `Drop`, which runs
    /// before this field is dropped, so the recorded contents show whether the
    /// wipe happened. The observation is made from inside the buffer rather
    /// than from freed memory, which would be undefined behavior.
    pub(crate) struct SpyBuffer {
        pub(crate) bytes: Vec<u8>,
        pub(crate) observed: Rc<RefCell<Option<Vec<u8>>>>,
    }

    impl AsMut<[u8]> for SpyBuffer {
        fn as_mut(&mut self) -> &mut [u8] {
            &mut self.bytes
        }
    }

    impl Drop for SpyBuffer {
        fn drop(&mut self) {
            *self.observed.borrow_mut() = Some(self.bytes.clone());
        }
    }

    /// Parses `body` through a spying buffer and returns what it released.
    pub(crate) fn parse_and_observe<T, E>(
        body: &str,
        parse: impl FnOnce(SpyBuffer) -> Result<T, E>,
    ) -> (Result<T, E>, Vec<u8>) {
        let observed = Rc::new(RefCell::new(None));
        let parsed = parse(SpyBuffer {
            bytes: body.as_bytes().to_vec(),
            observed: Rc::clone(&observed),
        });
        let released = observed
            .borrow_mut()
            .take()
            .expect("the guard must release the buffer it owned");
        (parsed, released)
    }
}

// ---------------------------------------------------------------------------
// Shared fixed-profile response helpers
// ---------------------------------------------------------------------------

/// Reports whether the request head declares any body at all.
pub fn has_request_body(headers: &HeaderMap) -> bool {
    headers
        .get_all(CONTENT_LENGTH)
        .iter()
        .any(|value| parse_content_length(value) != Some(0))
        || headers.contains_key(TRANSFER_ENCODING)
}

/// Parses a `Content-Length` value as strict decimal digits with no overflow.
pub fn parse_content_length(value: &HeaderValue) -> Option<u64> {
    let bytes = value.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    bytes.iter().try_fold(0_u64, |length, byte| {
        let digit = match byte {
            b'0'..=b'9' => u64::from(*byte - b'0'),
            _ => return None,
        };
        length.checked_mul(10)?.checked_add(digit)
    })
}

/// Reports whether the request negotiates the only served response media type.
///
/// Accepts either no `Accept` header or exactly one `Accept: application/json`.
pub fn accepts_json(headers: &HeaderMap) -> bool {
    let mut values = headers.get_all(ACCEPT).iter();
    match (values.next(), values.next()) {
        (None, _) => true,
        (Some(value), None) => value.as_bytes() == b"application/json",
        _ => false,
    }
}

/// Builds the fixed response for one of the shared pre-operational error codes.
pub fn json_response(status: StatusCode, error: &'static str) -> Response {
    let body = match error {
        "bad_request" => "{\"error\":\"bad_request\"}",
        "method_not_allowed" => "{\"error\":\"method_not_allowed\"}",
        "service_unavailable" => "{\"error\":\"service_unavailable\"}",
        _ => unreachable!("all pre-operational errors use fixed codes"),
    };
    json_response_body(status, body)
}

/// Builds a [`json_response`] that also advertises `Allow: GET`.
pub fn json_response_with_allow(status: StatusCode, error: &'static str) -> Response {
    let mut response = json_response(status, error);
    response.headers_mut().insert(ALLOW, "GET".parse().unwrap());
    response
}

/// Builds a fixed-profile JSON response from a compile-time body.
pub fn json_response_body(status: StatusCode, body: &'static str) -> Response {
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "application/json; charset=utf-8")
        .body(Body::from(body))
        .expect("fixed pre-operational responses must be valid")
}

#[cfg(test)]
mod wiped_body_tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::{WipedBody, wiped_body_support::SpyBuffer};

    #[test]
    fn the_body_guard_clears_the_buffer_it_owns_when_dropped() {
        let observed = Rc::new(RefCell::new(None));
        drop(WipedBody::new(SpyBuffer {
            bytes: b"AGE-SECRET-KEY-1TEST".to_vec(),
            observed: Rc::clone(&observed),
        }));
        assert_eq!(
            observed.borrow().as_deref(),
            Some(&[0u8; 20][..]),
            "the guard must clear the buffer before releasing it"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Request, StatusCode},
        response::Response,
    };
    use tower::ServiceExt;
    use weavelit_server_lifecycle::LifecycleProjection;

    use super::{
        APPLICATION_DATABASE_ROUTE, ExpectedOrigin, OperationalSurface, PreoperationalSurface,
        ProjectionSource, STATUS_ROUTE, SelectionCommit,
    };

    /// Builds a projection source; `None` models an unreadable lifecycle boundary.
    fn projection_source(database_selected: Option<bool>) -> ProjectionSource {
        Arc::new(move || {
            let result = database_selected.map(LifecycleProjection::new);
            Box::pin(async move { result })
        })
    }

    async fn status_response(request: Request<Body>, database_selected: bool) -> Response {
        super::status_response(request, projection_source(Some(database_selected))).await
    }

    async fn response_body(response: axum::response::Response) -> String {
        String::from_utf8(to_bytes(response.into_body(), 128).await.unwrap().to_vec()).unwrap()
    }

    #[tokio::test]
    async fn a_declared_capability_mounts_only_at_its_canonical_path() {
        let commit: SelectionCommit =
            Arc::new(|_backend| Box::pin(async { Ok(LifecycleProjection::new(true)) }));
        let assets = Router::new().route("/", axum::routing::get(|| async { "asset" }));
        let router = PreoperationalSurface::default()
            .with_status(projection_source(Some(false)))
            .with_database_selection(
                ExpectedOrigin::from_listener("127.0.0.1:8443".parse().unwrap()),
                commit,
            )
            .with_assets(assets)
            .mount(Router::new());

        for (target, status) in [
            (STATUS_ROUTE, StatusCode::OK),
            (APPLICATION_DATABASE_ROUTE, StatusCode::METHOD_NOT_ALLOWED),
            ("/", StatusCode::OK),
        ] {
            let response = router
                .clone()
                .oneshot(Request::get(target).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), status, "{target}");
        }
    }

    #[tokio::test]
    async fn an_undeclared_capability_mounts_no_route() {
        let router = PreoperationalSurface::default()
            .with_status(projection_source(Some(false)))
            .mount(Router::new());

        for (target, status) in [
            (STATUS_ROUTE, StatusCode::OK),
            (APPLICATION_DATABASE_ROUTE, StatusCode::NOT_FOUND),
            ("/", StatusCode::NOT_FOUND),
        ] {
            let response = router
                .clone()
                .oneshot(Request::get(target).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), status, "{target}");
        }

        let empty = PreoperationalSurface::default().mount(Router::new());
        let response = empty
            .oneshot(Request::get(STATUS_ROUTE).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn an_undeclared_operational_capability_mounts_no_route() {
        let router = OperationalSurface::default().mount(Router::new());

        for target in [STATUS_ROUTE, APPLICATION_DATABASE_ROUTE, "/"] {
            let response = router
                .clone()
                .oneshot(Request::get(target).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{target}");
        }
    }

    #[tokio::test]
    async fn the_operational_surface_mounts_only_declared_asset_delivery() {
        let assets = Router::new().route("/", axum::routing::get(|| async { "asset" }));
        let router = OperationalSurface::default()
            .with_assets(assets)
            .mount(Router::new());

        for (target, status) in [
            ("/", StatusCode::OK),
            (STATUS_ROUTE, StatusCode::NOT_FOUND),
            (APPLICATION_DATABASE_ROUTE, StatusCode::NOT_FOUND),
        ] {
            let response = router
                .clone()
                .oneshot(Request::get(target).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), status, "{target}");
        }
    }

    #[tokio::test]
    async fn status_translation_returns_the_exact_lifecycle_projection() {
        let response = status_response(
            Request::get("/api/v1/status").body(Body::empty()).unwrap(),
            false,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/json; charset=utf-8"
        );
        assert_eq!(
            response_body(response).await,
            "{\"lifecycle\":\"uninitialized\",\"database_selected\":false}"
        );

        let accepted_media_type = status_response(
            Request::builder()
                .uri("/api/v1/status")
                .header("accept", "application/json")
                .body(Body::empty())
                .unwrap(),
            true,
        )
        .await;
        assert_eq!(accepted_media_type.status(), StatusCode::OK);
        assert_eq!(
            response_body(accepted_media_type).await,
            "{\"lifecycle\":\"uninitialized\",\"database_selected\":true}"
        );
    }

    #[tokio::test]
    async fn status_translation_reports_an_unreadable_projection_as_unavailable() {
        let response = super::status_response(
            Request::get("/api/v1/status").body(Body::empty()).unwrap(),
            projection_source(None),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/json; charset=utf-8"
        );
        assert_eq!(
            response_body(response).await,
            "{\"error\":\"service_unavailable\"}"
        );
    }

    #[tokio::test]
    async fn status_translation_rejects_unsupported_requests() {
        let method = status_response(
            Request::builder()
                .method("POST")
                .uri("/api/v1/status")
                .body(Body::empty())
                .unwrap(),
            false,
        )
        .await;
        assert_eq!(method.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(method.headers().get("allow").unwrap(), "GET");
        assert_eq!(
            response_body(method).await,
            "{\"error\":\"method_not_allowed\"}"
        );

        for accept in ["text/html", "application/json, text/html"] {
            let response = status_response(
                Request::builder()
                    .uri("/api/v1/status")
                    .header("accept", accept)
                    .body(Body::empty())
                    .unwrap(),
                false,
            )
            .await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert_eq!(response_body(response).await, "{\"error\":\"bad_request\"}");
        }

        let duplicate_accept = status_response(
            Request::builder()
                .uri("/api/v1/status")
                .header("accept", "application/json")
                .header("accept", "text/html")
                .body(Body::empty())
                .unwrap(),
            false,
        )
        .await;
        assert_eq!(duplicate_accept.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response_body(duplicate_accept).await,
            "{\"error\":\"bad_request\"}"
        );
    }

    #[tokio::test]
    async fn status_translation_rejects_conflicting_content_length_fields() {
        for content_length in ["0", "00"] {
            let response = status_response(
                Request::builder()
                    .uri("/api/v1/status")
                    .header("content-length", content_length)
                    .body(Body::empty())
                    .unwrap(),
                false,
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response_body(response).await,
                "{\"lifecycle\":\"uninitialized\",\"database_selected\":false}"
            );
        }

        let duplicate_zero = status_response(
            Request::builder()
                .uri("/api/v1/status")
                .header("content-length", "0")
                .header("content-length", "00")
                .body(Body::empty())
                .unwrap(),
            false,
        )
        .await;
        assert_eq!(duplicate_zero.status(), StatusCode::OK);

        let conflicting = status_response(
            Request::builder()
                .uri("/api/v1/status")
                .header("content-length", "0")
                .header("content-length", "1")
                .body(Body::empty())
                .unwrap(),
            false,
        )
        .await;
        assert_eq!(conflicting.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response_body(conflicting).await,
            "{\"error\":\"bad_request\"}"
        );

        for content_length in ["1", "01", "00x"] {
            let response = status_response(
                Request::builder()
                    .uri("/api/v1/status")
                    .header("content-length", content_length)
                    .body(Body::empty())
                    .unwrap(),
                false,
            )
            .await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert_eq!(response_body(response).await, "{\"error\":\"bad_request\"}");
        }

        let transfer_encoding = status_response(
            Request::builder()
                .uri("/api/v1/status")
                .header("transfer-encoding", "chunked")
                .body(Body::empty())
                .unwrap(),
            false,
        )
        .await;
        assert_eq!(transfer_encoding.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response_body(transfer_encoding).await,
            "{\"error\":\"bad_request\"}"
        );
    }
}

#[cfg(test)]
mod database_selection_tests {
    use axum::{
        body::to_bytes,
        http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode},
    };
    use weavelit_server_lifecycle::{LifecycleProjection, SelectionFailureKind};

    use super::{
        DatabaseSelectionRejection, DatabaseSelectionRequest, ExpectedOrigin,
        MAX_DATABASE_SELECTION_BODY_BYTES, SelectedBackend, database_selection_response,
        validate_database_selection_request,
    };

    const VALID_BODY: &str = "{\"backend\":\"sqlite\",\"settings\":{}}";
    const SUCCESS_BODY: &str = "{\"lifecycle\":\"uninitialized\",\"database_selected\":true}";

    const FORBIDDEN_RESPONSE_HEADERS: [&str; 8] = [
        "access-control-allow-origin",
        "access-control-allow-credentials",
        "access-control-allow-headers",
        "access-control-allow-methods",
        "access-control-expose-headers",
        "access-control-max-age",
        "set-cookie",
        "vary",
    ];

    fn expected_origin(listener: &str) -> ExpectedOrigin {
        ExpectedOrigin::from_listener(listener.parse().unwrap())
    }

    fn headers(entries: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in entries {
            map.append(
                HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_bytes(value.as_bytes()).unwrap(),
            );
        }
        map
    }

    /// Headers that satisfy every precondition for a `127.0.0.1:8443` listener.
    fn valid_headers() -> HeaderMap {
        headers(&[
            ("origin", "https://127.0.0.1:8443"),
            ("host", "127.0.0.1:8443"),
            ("x-weavelit-csrf", "1"),
            ("content-type", "application/json"),
        ])
    }

    async fn response_body(response: axum::response::Response) -> String {
        String::from_utf8(to_bytes(response.into_body(), 128).await.unwrap().to_vec()).unwrap()
    }

    // -----------------------------------------------------------------------
    // Request body schema
    // -----------------------------------------------------------------------

    #[test]
    fn selection_body_accepts_only_the_exact_schema() {
        let padding = " ".repeat(MAX_DATABASE_SELECTION_BODY_BYTES - VALID_BODY.len());
        let at_limit = format!("{VALID_BODY}{padding}");
        assert_eq!(at_limit.len(), MAX_DATABASE_SELECTION_BODY_BYTES);

        let accepted: [(&str, &str); 5] = [
            ("canonical", VALID_BODY),
            (
                "insignificant whitespace",
                "{ \"backend\" : \"sqlite\" , \"settings\" : { } }",
            ),
            (
                "newline separated",
                "{\n\"backend\":\"sqlite\",\n\"settings\":{}\n}",
            ),
            (
                "member reordering",
                "{\"settings\":{},\"backend\":\"sqlite\"}",
            ),
            ("exactly at the body limit", at_limit.as_str()),
        ];
        for (case, body) in accepted {
            let request = DatabaseSelectionRequest::from_json(body.as_bytes())
                .unwrap_or_else(|error| panic!("{case}: unexpected rejection {error:?}"));
            assert_eq!(request.backend(), SelectedBackend::Sqlite, "{case}");
            assert_eq!(request.backend().identifier(), "sqlite", "{case}");
        }
    }

    #[test]
    fn selection_body_rejects_every_deviation_from_the_schema() {
        let over_limit = format!(
            "{VALID_BODY}{}",
            " ".repeat(MAX_DATABASE_SELECTION_BODY_BYTES - VALID_BODY.len() + 1)
        );
        assert_eq!(over_limit.len(), MAX_DATABASE_SELECTION_BODY_BYTES + 1);

        let rejected: [(&str, &str); 24] = [
            ("empty body", ""),
            ("whitespace only", "   "),
            (
                "unknown top-level field",
                "{\"backend\":\"sqlite\",\"settings\":{},\"extra\":1}",
            ),
            (
                "unknown settings field",
                "{\"backend\":\"sqlite\",\"settings\":{\"path\":\"/tmp/db\"}}",
            ),
            (
                "duplicate backend key",
                "{\"backend\":\"sqlite\",\"backend\":\"sqlite\",\"settings\":{}}",
            ),
            (
                "duplicate settings key",
                "{\"backend\":\"sqlite\",\"settings\":{},\"settings\":{}}",
            ),
            ("missing settings", "{\"backend\":\"sqlite\"}"),
            ("missing backend", "{\"settings\":{}}"),
            ("empty object", "{}"),
            (
                "backend wrongly typed as number",
                "{\"backend\":1,\"settings\":{}}",
            ),
            (
                "backend wrongly typed as null",
                "{\"backend\":null,\"settings\":{}}",
            ),
            (
                "backend wrongly typed as object",
                "{\"backend\":{},\"settings\":{}}",
            ),
            (
                "settings wrongly typed as array",
                "{\"backend\":\"sqlite\",\"settings\":[]}",
            ),
            (
                "settings wrongly typed as null",
                "{\"backend\":\"sqlite\",\"settings\":null}",
            ),
            (
                "settings wrongly typed as string",
                "{\"backend\":\"sqlite\",\"settings\":\"\"}",
            ),
            (
                "settings wrongly typed as number",
                "{\"backend\":\"sqlite\",\"settings\":0}",
            ),
            (
                "settings wrongly typed as boolean",
                "{\"backend\":\"sqlite\",\"settings\":true}",
            ),
            (
                "unknown backend literal",
                "{\"backend\":\"postgres\",\"settings\":{}}",
            ),
            (
                "wrong backend literal case",
                "{\"backend\":\"SQLite\",\"settings\":{}}",
            ),
            (
                "trailing object",
                "{\"backend\":\"sqlite\",\"settings\":{}}{}",
            ),
            (
                "trailing literal",
                "{\"backend\":\"sqlite\",\"settings\":{}}null",
            ),
            (
                "trailing garbage",
                "{\"backend\":\"sqlite\",\"settings\":{}} trailing",
            ),
            ("not an object", "[\"sqlite\"]"),
            ("over the body limit", over_limit.as_str()),
        ];
        for (case, body) in rejected {
            assert_eq!(
                DatabaseSelectionRequest::from_json(body.as_bytes()),
                Err(DatabaseSelectionRejection::BadRequest),
                "{case}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Method, media type, and negotiation
    // -----------------------------------------------------------------------

    #[test]
    fn selection_rejects_every_method_other_than_put() {
        let origin = expected_origin("127.0.0.1:8443");
        for method in ["GET", "POST", "PATCH", "DELETE", "OPTIONS", "HEAD"] {
            let method = Method::from_bytes(method.as_bytes()).unwrap();
            assert_eq!(
                validate_database_selection_request(&method, &valid_headers(), origin),
                Err(DatabaseSelectionRejection::MethodNotAllowed),
                "{method}"
            );
        }
        assert_eq!(
            validate_database_selection_request(&Method::PUT, &valid_headers(), origin),
            Ok(())
        );
    }

    #[test]
    fn selection_enforces_the_request_and_response_media_types() {
        let origin = expected_origin("127.0.0.1:8443");
        let base: [(&str, &str); 3] = [
            ("origin", "https://127.0.0.1:8443"),
            ("host", "127.0.0.1:8443"),
            ("x-weavelit-csrf", "1"),
        ];
        let with = |extra: &[(&str, &str)]| {
            let mut entries = base.to_vec();
            entries.extend_from_slice(extra);
            headers(&entries)
        };

        let accepted: [(&str, Vec<(&str, &str)>); 2] = [
            ("accept absent", vec![("content-type", "application/json")]),
            (
                "accept exactly matching",
                vec![
                    ("content-type", "application/json"),
                    ("accept", "application/json"),
                ],
            ),
        ];
        for (case, extra) in accepted {
            assert_eq!(
                validate_database_selection_request(&Method::PUT, &with(&extra), origin),
                Ok(()),
                "{case}"
            );
        }

        let rejected: [(&str, Vec<(&str, &str)>); 8] = [
            ("content-type absent", vec![]),
            ("content-type empty", vec![("content-type", "")]),
            (
                "content-type with charset parameter",
                vec![("content-type", "application/json; charset=utf-8")],
            ),
            (
                "content-type text/plain",
                vec![("content-type", "text/plain")],
            ),
            (
                "content-type form encoded",
                vec![("content-type", "application/x-www-form-urlencoded")],
            ),
            (
                "duplicate content-type",
                vec![
                    ("content-type", "application/json"),
                    ("content-type", "application/json"),
                ],
            ),
            (
                "non-matching accept",
                vec![
                    ("content-type", "application/json"),
                    ("accept", "text/html"),
                ],
            ),
            (
                "duplicate accept",
                vec![
                    ("content-type", "application/json"),
                    ("accept", "application/json"),
                    ("accept", "application/json"),
                ],
            ),
        ];
        for (case, extra) in rejected {
            assert_eq!(
                validate_database_selection_request(&Method::PUT, &with(&extra), origin),
                Err(DatabaseSelectionRejection::BadRequest),
                "{case}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Same-origin and CSRF trust
    // -----------------------------------------------------------------------

    #[test]
    fn selection_accepts_normalized_matching_authorities() {
        let accepted: [(&str, &str, &str, &str); 8] = [
            (
                "ipv4 explicit non-default port",
                "127.0.0.1:8443",
                "https://127.0.0.1:8443",
                "127.0.0.1:8443",
            ),
            (
                "ipv4 default port omitted in both",
                "127.0.0.1:443",
                "https://127.0.0.1",
                "127.0.0.1",
            ),
            (
                "ipv4 default port explicit in both",
                "127.0.0.1:443",
                "https://127.0.0.1:443",
                "127.0.0.1:443",
            ),
            (
                "ipv4 default port explicit in origin only",
                "127.0.0.1:443",
                "https://127.0.0.1:443",
                "127.0.0.1",
            ),
            (
                "ipv4 default port explicit in host only",
                "127.0.0.1:443",
                "https://127.0.0.1",
                "127.0.0.1:443",
            ),
            (
                "ipv6 explicit non-default port",
                "[::1]:8443",
                "https://[::1]:8443",
                "[::1]:8443",
            ),
            (
                "ipv6 default port omitted in both",
                "[::1]:443",
                "https://[::1]",
                "[::1]",
            ),
            (
                "ipv6 default port explicit in both",
                "[::1]:443",
                "https://[::1]:443",
                "[::1]:443",
            ),
        ];
        for (case, listener, origin, host) in accepted {
            assert_eq!(
                validate_database_selection_request(
                    &Method::PUT,
                    &headers(&[
                        ("origin", origin),
                        ("host", host),
                        ("x-weavelit-csrf", "1"),
                        ("content-type", "application/json"),
                    ]),
                    expected_origin(listener),
                ),
                Ok(()),
                "{case}"
            );
        }
    }

    #[test]
    fn selection_denies_every_untrusted_origin_host_or_csrf_header() {
        let origin = expected_origin("127.0.0.1:8443");
        let rejected: [(&str, Vec<(&str, &str)>); 20] = [
            (
                "missing origin",
                vec![
                    ("host", "127.0.0.1:8443"),
                    ("x-weavelit-csrf", "1"),
                    ("content-type", "application/json"),
                ],
            ),
            (
                "missing host",
                vec![
                    ("origin", "https://127.0.0.1:8443"),
                    ("x-weavelit-csrf", "1"),
                    ("content-type", "application/json"),
                ],
            ),
            (
                "duplicate origin",
                vec![
                    ("origin", "https://127.0.0.1:8443"),
                    ("origin", "https://127.0.0.1:8443"),
                    ("host", "127.0.0.1:8443"),
                    ("x-weavelit-csrf", "1"),
                    ("content-type", "application/json"),
                ],
            ),
            (
                "duplicate host",
                vec![
                    ("origin", "https://127.0.0.1:8443"),
                    ("host", "127.0.0.1:8443"),
                    ("host", "127.0.0.1:8443"),
                    ("x-weavelit-csrf", "1"),
                    ("content-type", "application/json"),
                ],
            ),
            (
                "origin port mismatch",
                origin_case("https://127.0.0.1:9443", "127.0.0.1:8443"),
            ),
            (
                "origin address mismatch",
                origin_case("https://127.0.0.2:8443", "127.0.0.1:8443"),
            ),
            (
                "host port mismatch",
                origin_case("https://127.0.0.1:8443", "127.0.0.1:9443"),
            ),
            (
                "host address mismatch",
                origin_case("https://127.0.0.1:8443", "127.0.0.2:8443"),
            ),
            (
                "origin and host disagree",
                origin_case("https://127.0.0.1:8443", "[::1]:8443"),
            ),
            (
                "non-https origin scheme",
                origin_case("http://127.0.0.1:8443", "127.0.0.1:8443"),
            ),
            (
                "scheme-relative origin",
                origin_case("//127.0.0.1:8443", "127.0.0.1:8443"),
            ),
            (
                "origin missing scheme",
                origin_case("127.0.0.1:8443", "127.0.0.1:8443"),
            ),
            (
                "dns name origin",
                origin_case("https://localhost:8443", "localhost:8443"),
            ),
            (
                "dns name host",
                origin_case("https://127.0.0.1:8443", "localhost:8443"),
            ),
            (
                "origin userinfo",
                origin_case("https://user@127.0.0.1:8443", "127.0.0.1:8443"),
            ),
            (
                "origin trailing path",
                origin_case("https://127.0.0.1:8443/", "127.0.0.1:8443"),
            ),
            ("opaque origin", origin_case("null", "127.0.0.1:8443")),
            (
                "unbracketed ipv6 origin",
                vec![
                    ("origin", "https://::1:8443"),
                    ("host", "::1:8443"),
                    ("x-weavelit-csrf", "1"),
                    ("content-type", "application/json"),
                ],
            ),
            (
                "missing csrf header",
                vec![
                    ("origin", "https://127.0.0.1:8443"),
                    ("host", "127.0.0.1:8443"),
                    ("content-type", "application/json"),
                ],
            ),
            (
                "duplicate csrf header",
                vec![
                    ("origin", "https://127.0.0.1:8443"),
                    ("host", "127.0.0.1:8443"),
                    ("x-weavelit-csrf", "1"),
                    ("x-weavelit-csrf", "1"),
                    ("content-type", "application/json"),
                ],
            ),
        ];
        for (case, entries) in rejected {
            assert_eq!(
                validate_database_selection_request(&Method::PUT, &headers(&entries), origin),
                Err(DatabaseSelectionRejection::RequestOriginDenied),
                "{case}"
            );
        }

        for value in ["", "0", "true", "1 ", " 1", "11"] {
            assert_eq!(
                validate_database_selection_request(
                    &Method::PUT,
                    &headers(&[
                        ("origin", "https://127.0.0.1:8443"),
                        ("host", "127.0.0.1:8443"),
                        ("x-weavelit-csrf", value),
                        ("content-type", "application/json"),
                    ]),
                    origin,
                ),
                Err(DatabaseSelectionRejection::RequestOriginDenied),
                "csrf value {value:?}"
            );
        }
    }

    fn origin_case<'a>(origin: &'a str, host: &'a str) -> Vec<(&'a str, &'a str)> {
        vec![
            ("origin", origin),
            ("host", host),
            ("x-weavelit-csrf", "1"),
            ("content-type", "application/json"),
        ]
    }

    // -----------------------------------------------------------------------
    // Response contract
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn selection_success_returns_the_exact_projection_body() {
        let response = database_selection_response(&LifecycleProjection::new(true));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/json; charset=utf-8"
        );
        assert!(response.headers().get("allow").is_none());
        assert_eq!(response_body(response).await, SUCCESS_BODY);
        assert_eq!(SUCCESS_BODY.len(), 54);
    }

    #[tokio::test]
    async fn every_rejection_returns_its_documented_status_and_fixed_body() {
        let contract: [(DatabaseSelectionRejection, u16, &str); 5] = [
            (
                DatabaseSelectionRejection::BadRequest,
                400,
                "{\"error\":\"bad_request\"}",
            ),
            (
                DatabaseSelectionRejection::RequestOriginDenied,
                403,
                "{\"error\":\"request_origin_denied\"}",
            ),
            (
                DatabaseSelectionRejection::MethodNotAllowed,
                405,
                "{\"error\":\"method_not_allowed\"}",
            ),
            (
                DatabaseSelectionRejection::DatabaseSelectionNotAllowed,
                409,
                "{\"error\":\"database_selection_not_allowed\"}",
            ),
            (
                DatabaseSelectionRejection::ServiceUnavailable,
                503,
                "{\"error\":\"service_unavailable\"}",
            ),
        ];
        for (rejection, status, body) in contract {
            assert_eq!(rejection.status().as_u16(), status, "{rejection:?}");
            assert_eq!(rejection.body(), body, "{rejection:?}");
            assert!(body.len() <= 128, "{rejection:?} exceeds the JSON profile");

            let response = rejection.response();
            assert_eq!(response.status().as_u16(), status, "{rejection:?}");
            assert_eq!(
                response.headers().get("content-type").unwrap(),
                "application/json; charset=utf-8",
                "{rejection:?}"
            );
            let allow = response.headers().get("allow").cloned();
            if rejection == DatabaseSelectionRejection::MethodNotAllowed {
                assert_eq!(allow.unwrap(), "PUT", "{rejection:?}");
            } else {
                assert!(allow.is_none(), "{rejection:?}");
            }
            assert_eq!(response_body(response).await, body, "{rejection:?}");
        }
    }

    #[test]
    fn every_selection_failure_kind_maps_to_the_documented_rejection() {
        let contract: [(SelectionFailureKind, DatabaseSelectionRejection, u16); 3] = [
            (
                SelectionFailureKind::RequestInvalid,
                DatabaseSelectionRejection::BadRequest,
                400,
            ),
            (
                SelectionFailureKind::Conflict,
                DatabaseSelectionRejection::DatabaseSelectionNotAllowed,
                409,
            ),
            (
                SelectionFailureKind::Unavailable,
                DatabaseSelectionRejection::ServiceUnavailable,
                503,
            ),
        ];
        for (kind, expected, status) in contract {
            let rejection = DatabaseSelectionRejection::from_selection_failure(kind);
            assert_eq!(rejection, expected, "{kind:?}");
            assert_eq!(rejection.status().as_u16(), status, "{kind:?}");
        }
    }

    #[test]
    fn no_selection_response_emits_a_cors_or_cookie_header() {
        let mut responses = vec![
            database_selection_response(&LifecycleProjection::new(true)),
            database_selection_response(&LifecycleProjection::new(false)),
        ];
        for rejection in [
            DatabaseSelectionRejection::BadRequest,
            DatabaseSelectionRejection::RequestOriginDenied,
            DatabaseSelectionRejection::MethodNotAllowed,
            DatabaseSelectionRejection::DatabaseSelectionNotAllowed,
            DatabaseSelectionRejection::ServiceUnavailable,
        ] {
            responses.push(rejection.response());
        }
        for response in responses {
            for forbidden in FORBIDDEN_RESPONSE_HEADERS {
                assert!(
                    !response.headers().contains_key(forbidden),
                    "{forbidden} must never be emitted"
                );
            }
        }
    }
}
