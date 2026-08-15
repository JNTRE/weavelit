//! Shared Client Module contract for the operational authentication surface.
//!
//! Three routes make up the surface: a login that exchanges a username and
//! password for a session, a session validation that reports the identity a
//! presented session authenticates, and a logout that revokes it. Every route
//! answers with a typed envelope, and only login and logout carry a cookie
//! effect.
//!
//! This module owns the canonical routes, the request schema, every header and
//! cookie precondition, the stable rejection contract, and the response
//! envelopes. It owns no credential store, no session store, no clock, and no
//! Argon2 work: it hands a validated submission to a Server-core hook and
//! renders exactly what that hook returns. It also cannot tell why an
//! authentication was denied, because the hook does not tell it.

use std::fmt;
use std::pin::Pin;
use std::sync::Arc;

use axum::{
    body::to_bytes,
    extract::Request,
    http::{
        Extensions, HeaderMap, HeaderValue, Method, StatusCode,
        header::{ALLOW, CONTENT_TYPE, COOKIE},
    },
    response::Response,
    routing::{MethodRouter, any},
};
use serde::de::{self, Deserialize, Deserializer, MapAccess, Visitor};
use zeroize::Zeroizing;

use crate::{
    CSRF_HEADER_NAME, ExpectedOrigin, JSON_MEDIA_TYPE, WipedBody, accepts_json,
    cookie::{CSRF_COOKIE_NAME, CookieEffect, CookieValue, SESSION_COOKIE_NAME},
    has_request_body, json_response_body, single_header,
    typed_json::{
        OpaqueToken, ResponseCorrelation, StableCode, TypedJsonEnvelope, TypedResult, TypedValue,
        typed_json_response, typed_json_response_with_cookies,
    },
};

/// The canonical route that exchanges credentials for a session.
pub const AUTH_LOGIN_ROUTE: &str = "/api/v1/auth/login";

/// The canonical route that reports the identity a session authenticates.
pub const AUTH_SESSION_ROUTE: &str = "/api/v1/auth/session";

/// The canonical route that revokes a session.
pub const AUTH_LOGOUT_ROUTE: &str = "/api/v1/auth/logout";

/// Largest request body accepted for a login submission.
///
/// The submission carries a username, a password, and a Client Module name, so
/// it stays inside the listener's default body bound.
pub const MAX_LOGIN_BODY_BYTES: usize = 1024;

/// The result field reporting that a session was established.
const AUTHENTICATED_FIELD: &str = "authenticated";

/// The result field carrying the authenticated account identifier.
const ACCOUNT_FIELD: &str = "account_id";

/// The result field carrying the session's issuing Client Module.
const CLIENT_MODULE_FIELD: &str = "client_module";

/// The result field reporting a revoked session.
const SESSION_FIELD: &str = "session";

/// The only value [`SESSION_FIELD`] reports.
const SESSION_ENDED: &str = "ended";

// ---------------------------------------------------------------------------
// Submissions and Server-core hooks
// ---------------------------------------------------------------------------

/// A validated login submission handed to the Server core.
///
/// The password is owned and cleared when dropped, so a rejected submission
/// leaves no plaintext password behind in this crate.
pub struct LoginSubmission {
    /// The submitted account name, still unresolved against any account.
    pub username: String,
    /// The submitted password, still unverified.
    pub password: Zeroizing<String>,
    /// The Client Module the session is requested for.
    pub client_module: String,
    /// The Server-generated correlation identifier for this attempt.
    pub correlation_id: String,
    /// The admitted request's extensions, which carry the Server core's own
    /// admission permit and pre-body grant.
    pub context: Extensions,
}

/// A validated presented session handed to the Server core.
pub struct SessionSubmission {
    /// The session bearer value read from the session cookie.
    pub session_token: Zeroizing<String>,
    /// The cross-site request forgery token echoed in the request header.
    pub csrf_token: Zeroizing<String>,
    /// The Server-generated correlation identifier for this request.
    pub correlation_id: String,
    /// The admitted request's extensions.
    pub context: Extensions,
}

/// The session the Server core established for a verified login.
pub struct SessionEstablished {
    /// The issued session bearer value.
    pub session_token: Zeroizing<String>,
    /// The issued cross-site request forgery token.
    pub csrf_token: Zeroizing<String>,
}

/// The identity a validated session authenticates.
///
/// It carries the account and the issuing Client Module and nothing else. No
/// Group, grant, or other authorization data is reported, because
/// authorization is evaluated live from application state by its own boundary.
pub struct SessionIdentity {
    /// The authenticated account, rendered as lowercase hexadecimal.
    pub account_id: String,
    /// The Client Module the session was issued to.
    pub client_module: String,
}

/// What the Server core decided a submitted password entitles the request to.
///
/// A login answers with exactly one of these, and the choice is made only
/// after a password has actually verified. A denied submission is not a
/// variant here: it is an [`AuthenticationRejection`], so nothing about second
/// factors is observable until a password is correct.
pub enum LoginOutcome {
    /// The password verified and no second factor applies, so a session is
    /// issued in this response.
    SessionEstablished(SessionEstablished),
    /// The password verified and an enrolled second factor must be presented
    /// before a session exists.
    SecondFactorRequired {
        /// The one-time value that resumes this login.
        continuation: Zeroizing<String>,
    },
    /// The password verified and the account must enroll a second factor
    /// before a session exists.
    EnrollmentRequired {
        /// The one-time value that opens the enrollment for this login.
        continuation: Zeroizing<String>,
    },
}

/// Server-core hook that verifies a submission and decides its outcome.
pub type LoginCommit = Arc<
    dyn Fn(
            LoginSubmission,
        )
            -> Pin<Box<dyn Future<Output = Result<LoginOutcome, AuthenticationRejection>> + Send>>
        + Send
        + Sync,
>;

/// Server-core hook that validates a presented session.
pub type SessionValidate = Arc<
    dyn Fn(
            SessionSubmission,
        )
            -> Pin<Box<dyn Future<Output = Result<SessionIdentity, AuthenticationRejection>> + Send>>
        + Send
        + Sync,
>;

/// Server-core hook that revokes a presented session.
pub type SessionRevoke = Arc<
    dyn Fn(
            SessionSubmission,
        ) -> Pin<Box<dyn Future<Output = Result<(), AuthenticationRejection>> + Send>>
        + Send
        + Sync,
>;

/// Server-core source of the correlation identifier every response carries.
///
/// A Client Module does not mint correlation identifiers; it asks for one so a
/// rejection detected here is relatable to Server-side records exactly as a
/// rejection detected in the Server core is. `None` means no identifier could
/// be produced.
pub type CorrelationSource = Arc<dyn Fn() -> Option<String> + Send + Sync>;

/// The runtime collaborators a Client Module declares authentication with.
pub struct AuthenticationCapability {
    /// The trusted authority every authentication request must target.
    pub expected_origin: ExpectedOrigin,
    /// The Server-owned correlation identifier source.
    pub correlate: CorrelationSource,
    /// The hook that verifies credentials and issues a session.
    pub login: LoginCommit,
    /// The hook that validates a presented session.
    pub validate_session: SessionValidate,
    /// The hook that revokes a presented session.
    pub logout: SessionRevoke,
}

/// A declared authentication capability, split into its three mountable routes.
///
/// The Server core mounts each route together with the transport registration
/// that admits it, so login's single admission permit is granted only to the
/// route that was mounted with it.
pub struct AuthenticationDeclaration {
    capability: Arc<AuthenticationCapability>,
}

impl AuthenticationDeclaration {
    /// Declares authentication over the supplied runtime collaborators.
    #[must_use]
    pub fn new(capability: AuthenticationCapability) -> Self {
        Self {
            capability: Arc::new(capability),
        }
    }

    /// Returns the login route mounted at [`AUTH_LOGIN_ROUTE`].
    pub fn login_route(&self) -> MethodRouter {
        let capability = Arc::clone(&self.capability);
        any(move |request| login_response(request, Arc::clone(&capability)))
    }

    /// Returns the session validation route mounted at [`AUTH_SESSION_ROUTE`].
    pub fn session_route(&self) -> MethodRouter {
        let capability = Arc::clone(&self.capability);
        any(move |request| session_response(request, Arc::clone(&capability)))
    }

    /// Returns the logout route mounted at [`AUTH_LOGOUT_ROUTE`].
    pub fn logout_route(&self) -> MethodRouter {
        let capability = Arc::clone(&self.capability);
        any(move |request| logout_response(request, Arc::clone(&capability)))
    }
}

// ---------------------------------------------------------------------------
// Rejection contract
// ---------------------------------------------------------------------------

/// The complete, payload-free rejection contract for all three routes.
///
/// There is exactly one denial code for authentication and exactly one for a
/// presented session. An unknown account, an inactive account, an account
/// without a usable verifier, and a wrong password all render
/// [`Self::AuthenticationFailed`], and no variant reports which validation
/// step failed beyond its stable code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthenticationRejection {
    /// `400` for a malformed body, media type, or `Accept` value.
    BadRequest,
    /// `401` for any denied credential submission, whatever its cause.
    AuthenticationFailed,
    /// `401` for a missing, malformed, unknown, expired, or mismatched session.
    SessionInvalid,
    /// `403` for a failed same-origin, `Host`, or CSRF header check.
    RequestOriginDenied,
    /// `405` for any method other than `PUT`.
    MethodNotAllowed,
    /// `503` for a persistence, randomness, or other internal failure.
    ServiceUnavailable,
}

impl AuthenticationRejection {
    /// Returns the documented status code.
    #[must_use]
    pub const fn status(self) -> StatusCode {
        match self {
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::AuthenticationFailed | Self::SessionInvalid => StatusCode::UNAUTHORIZED,
            Self::RequestOriginDenied => StatusCode::FORBIDDEN,
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::ServiceUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    /// Returns the documented stable error code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::BadRequest => "bad_request",
            Self::AuthenticationFailed => "authentication_failed",
            Self::SessionInvalid => "session_invalid",
            Self::RequestOriginDenied => "request_origin_denied",
            Self::MethodNotAllowed => "method_not_allowed",
            Self::ServiceUnavailable => "service_unavailable",
        }
    }

    /// Builds the typed rejection envelope, including `Allow: PUT` for `405`.
    ///
    /// A rejection never carries a cookie effect, so a denied request can
    /// neither establish nor clear a session.
    #[must_use]
    pub fn response(self, correlation_id: &str) -> Response {
        let (Some(error), Some(correlation)) = (
            StableCode::new(self.code()),
            ResponseCorrelation::new(correlation_id),
        ) else {
            return unrenderable_response();
        };
        let mut response = typed_json_response(
            self.status(),
            TypedJsonEnvelope::Error {
                error,
                correlation_id: correlation,
            },
        );
        if self == Self::MethodNotAllowed {
            response
                .headers_mut()
                .insert(ALLOW, HeaderValue::from_static("PUT"));
        }
        response
    }
}

impl fmt::Display for AuthenticationRejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

/// The payload-free response used when no typed envelope can be rendered.
///
/// It uses the fixed profile's already-approved unavailable body, so a failure
/// to render a correlation identifier still answers within the frozen fixed
/// allowlist rather than inventing a shape.
pub(crate) fn unrenderable_response() -> Response {
    json_response_body(
        StatusCode::SERVICE_UNAVAILABLE,
        "{\"error\":\"service_unavailable\"}",
    )
}

// ---------------------------------------------------------------------------
// Head validation
// ---------------------------------------------------------------------------

/// Validates every header precondition of a login submission.
///
/// Login is the bootstrap request: no session exists yet, so the request is
/// trusted by exact same-origin validation plus the literal CSRF header value
/// every other pre-session route already requires.
pub fn validate_login_request(
    method: &Method,
    headers: &HeaderMap,
    expected_origin: ExpectedOrigin,
) -> Result<(), AuthenticationRejection> {
    if method != Method::PUT {
        return Err(AuthenticationRejection::MethodNotAllowed);
    }
    if !expected_origin.is_trusted(headers) {
        return Err(AuthenticationRejection::RequestOriginDenied);
    }
    let content_type =
        single_header(headers, CONTENT_TYPE).ok_or(AuthenticationRejection::BadRequest)?;
    if content_type.as_bytes() != JSON_MEDIA_TYPE || !accepts_json(headers) {
        return Err(AuthenticationRejection::BadRequest);
    }
    Ok(())
}

/// Validates every header and cookie precondition of a session-bearing request.
///
/// Both session-bearing routes advance or end durable session state, so both
/// are mutating and both require exact same-origin validation together with a
/// CSRF token echoed from the readable cookie. The token's shape is checked
/// here; whether it matches the session is decided by the Server core, which
/// holds the only digest it could be compared against.
pub fn validate_session_request(
    method: &Method,
    headers: &HeaderMap,
    expected_origin: ExpectedOrigin,
) -> Result<(), AuthenticationRejection> {
    if method != Method::PUT {
        return Err(AuthenticationRejection::MethodNotAllowed);
    }
    if !expected_origin.is_same_origin(headers) {
        return Err(AuthenticationRejection::RequestOriginDenied);
    }
    if has_request_body(headers) || !accepts_json(headers) {
        return Err(AuthenticationRejection::BadRequest);
    }
    submitted_csrf_token(headers)?;
    submitted_session_token(headers)?;
    Ok(())
}

/// Returns the well-formed CSRF token the request echoed in its header.
///
/// Requires exactly one [`CSRF_HEADER_NAME`] header whose value has the shape
/// of an issued token. A missing, repeated, or malformed header is
/// indistinguishable from a token that does not match the session.
pub fn submitted_csrf_token(headers: &HeaderMap) -> Result<&str, AuthenticationRejection> {
    let invalid = AuthenticationRejection::SessionInvalid;
    let value = single_header(headers, CSRF_HEADER_NAME).ok_or(invalid)?;
    let token = value.to_str().map_err(|_| invalid)?;
    if CookieValue::new(token).is_none() {
        return Err(invalid);
    }
    Ok(token)
}

/// Returns the well-formed session value the request presented in its cookie.
///
/// The session is accepted only from the session cookie. It is never read from
/// a header, a query string, or a body, so it cannot be replayed from a URL.
pub fn submitted_session_token(headers: &HeaderMap) -> Result<&str, AuthenticationRejection> {
    let invalid = AuthenticationRejection::SessionInvalid;
    let token = cookie_value(headers, SESSION_COOKIE_NAME).ok_or(invalid)?;
    if CookieValue::new(token).is_none() {
        return Err(invalid);
    }
    Ok(token)
}

/// Returns the single value one named cookie carries, if it is unambiguous.
///
/// Requires exactly one `Cookie` header, every pair in it to be well formed,
/// and the named cookie to appear exactly once. Anything else is treated as no
/// cookie at all rather than resolved by preferring one occurrence.
fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let header = single_header(headers, COOKIE)?;
    let text = header.to_str().ok()?;
    let mut found = None;
    for pair in text.split(';') {
        let pair = pair.strip_prefix(' ').unwrap_or(pair);
        let (key, value) = pair.split_once('=')?;
        if key == name {
            if found.is_some() {
                return None;
            }
            found = Some(value);
        }
    }
    found
}

// ---------------------------------------------------------------------------
// Request schema
// ---------------------------------------------------------------------------

/// The strictly validated login submission body.
///
/// The implementation is written by hand rather than derived because a derived
/// struct also accepts its JSON array form, which would let a credential be
/// submitted through a shape the API contract does not document.
struct LoginBody {
    username: String,
    password: Zeroizing<String>,
    client_module: String,
}

const LOGIN_FIELDS: &[&str] = &["username", "password", "client_module"];

impl<'de> Deserialize<'de> for LoginBody {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BodyVisitor;

        impl<'de> Visitor<'de> for BodyVisitor {
            type Value = LoginBody;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a login submission object")
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut username: Option<String> = None;
                let mut password: Option<Zeroizing<String>> = None;
                let mut client_module: Option<String> = None;
                while let Some(field) = map.next_key::<String>()? {
                    match field.as_str() {
                        "username" => {
                            if username.is_some() {
                                return Err(de::Error::duplicate_field("username"));
                            }
                            username = Some(map.next_value()?);
                        }
                        "password" => {
                            if password.is_some() {
                                return Err(de::Error::duplicate_field("password"));
                            }
                            password = Some(Zeroizing::new(map.next_value()?));
                        }
                        "client_module" => {
                            if client_module.is_some() {
                                return Err(de::Error::duplicate_field("client_module"));
                            }
                            client_module = Some(map.next_value()?);
                        }
                        unknown => return Err(de::Error::unknown_field(unknown, LOGIN_FIELDS)),
                    }
                }
                Ok(LoginBody {
                    username: username.ok_or_else(|| de::Error::missing_field("username"))?,
                    password: password.ok_or_else(|| de::Error::missing_field("password"))?,
                    client_module: client_module
                        .ok_or_else(|| de::Error::missing_field("client_module"))?,
                })
            }
        }

        deserializer.deserialize_map(BodyVisitor)
    }
}

/// Parses the login body out of a buffer that is wiped when dropped.
///
/// The login body carries the plaintext password, so it is read through the
/// shared [`WipedBody`] guard: the buffer is cleared on the parsed path and on
/// every rejection path alike, because the guard owns it for the whole call.
fn parse_login_body_wiped<B: AsMut<[u8]>>(buffer: B) -> Result<LoginBody, AuthenticationRejection> {
    let mut body = WipedBody::new(buffer);
    parse_login_body(body.bytes())
}

/// Parses the exact accepted login body.
///
/// An unknown field, duplicate key, missing field, wrongly typed value, array
/// form, trailing content, empty body, or oversized body is rejected.
fn parse_login_body(body: &[u8]) -> Result<LoginBody, AuthenticationRejection> {
    if body.len() > MAX_LOGIN_BODY_BYTES {
        return Err(AuthenticationRejection::BadRequest);
    }
    let mut deserializer = serde_json::Deserializer::from_slice(body);
    let parsed = LoginBody::deserialize(&mut deserializer)
        .map_err(|_| AuthenticationRejection::BadRequest)?;
    deserializer
        .end()
        .map_err(|_| AuthenticationRejection::BadRequest)?;
    Ok(parsed)
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

async fn login_response(request: Request, capability: Arc<AuthenticationCapability>) -> Response {
    let Some(correlation_id) = (capability.correlate)() else {
        return unrenderable_response();
    };
    let (parts, body) = request.into_parts();
    if let Err(rejection) =
        validate_login_request(&parts.method, &parts.headers, capability.expected_origin)
    {
        return rejection.response(&correlation_id);
    }
    let Ok(body) = to_bytes(body, MAX_LOGIN_BODY_BYTES).await else {
        return AuthenticationRejection::BadRequest.response(&correlation_id);
    };
    // `Bytes` is shared and immutable, so the collected buffer can only be
    // wiped once this crate holds it uniquely, which is the ordinary outcome
    // for a collected request body. If a clone is outstanding the fallback
    // still wipes the copy this crate owns; the shared original is out of
    // reach and is left to its own owner.
    let parsed = match body.try_into_mut() {
        Ok(unique) => parse_login_body_wiped(unique),
        Err(shared) => parse_login_body_wiped(shared.to_vec()),
    };
    let submitted = match parsed {
        Ok(submitted) => submitted,
        Err(rejection) => return rejection.response(&correlation_id),
    };

    match (capability.login)(LoginSubmission {
        username: submitted.username,
        password: submitted.password,
        client_module: submitted.client_module,
        correlation_id: correlation_id.clone(),
        context: parts.extensions,
    })
    .await
    {
        Ok(LoginOutcome::SessionEstablished(established)) => {
            session_established_response(&established, &correlation_id)
        }
        Ok(LoginOutcome::SecondFactorRequired { continuation }) => {
            crate::mfa::continuation_response(
                crate::mfa::MFA_REQUIRED_CODE,
                &continuation,
                &correlation_id,
            )
        }
        Ok(LoginOutcome::EnrollmentRequired { continuation }) => crate::mfa::continuation_response(
            crate::mfa::MFA_ENROLLMENT_REQUIRED_CODE,
            &continuation,
            &correlation_id,
        ),
        Err(rejection) => rejection.response(&correlation_id),
    }
}

async fn session_response(request: Request, capability: Arc<AuthenticationCapability>) -> Response {
    let Some(correlation_id) = (capability.correlate)() else {
        return unrenderable_response();
    };
    let submission = match session_submission(request, &capability, &correlation_id) {
        Ok(submission) => submission,
        Err(rejection) => return rejection.response(&correlation_id),
    };

    match (capability.validate_session)(submission).await {
        Ok(identity) => session_identity_response(&identity, &correlation_id),
        Err(rejection) => rejection.response(&correlation_id),
    }
}

async fn logout_response(request: Request, capability: Arc<AuthenticationCapability>) -> Response {
    let Some(correlation_id) = (capability.correlate)() else {
        return unrenderable_response();
    };
    let submission = match session_submission(request, &capability, &correlation_id) {
        Ok(submission) => submission,
        Err(rejection) => return rejection.response(&correlation_id),
    };

    match (capability.logout)(submission).await {
        Ok(()) => session_cleared_response(&correlation_id),
        Err(rejection) => rejection.response(&correlation_id),
    }
}

/// Validates a session-bearing request and builds its submission.
fn session_submission(
    request: Request,
    capability: &AuthenticationCapability,
    correlation_id: &str,
) -> Result<SessionSubmission, AuthenticationRejection> {
    let (parts, _body) = request.into_parts();
    validate_session_request(&parts.method, &parts.headers, capability.expected_origin)?;
    let session_token = Zeroizing::new(submitted_session_token(&parts.headers)?.to_owned());
    let csrf_token = Zeroizing::new(submitted_csrf_token(&parts.headers)?.to_owned());

    Ok(SessionSubmission {
        session_token,
        csrf_token,
        correlation_id: correlation_id.to_owned(),
        context: parts.extensions,
    })
}

/// Renders the only response that may ever establish a session.
///
/// The session and CSRF values are returned in cookies alone. Neither is
/// placed in the envelope, so neither can be read from a response body, a
/// redirect target, or a browser history entry.
pub(crate) fn session_established_response(
    established: &SessionEstablished,
    correlation_id: &str,
) -> Response {
    let (Some(session), Some(csrf)) = (
        CookieValue::new(&established.session_token),
        CookieValue::new(&established.csrf_token),
    ) else {
        return AuthenticationRejection::ServiceUnavailable.response(correlation_id);
    };
    let Some(result) = typed_field(AUTHENTICATED_FIELD, TypedValue::Boolean(true)) else {
        return AuthenticationRejection::ServiceUnavailable.response(correlation_id);
    };
    let Some(correlation) = ResponseCorrelation::new(correlation_id) else {
        return AuthenticationRejection::ServiceUnavailable.response(correlation_id);
    };
    typed_json_response_with_cookies(
        StatusCode::OK,
        TypedJsonEnvelope::Result {
            result,
            correlation_id: correlation,
        },
        CookieEffect::IssueSession { session, csrf },
    )
}

/// Renders the identity a validated session authenticates.
fn session_identity_response(identity: &SessionIdentity, correlation_id: &str) -> Response {
    let unavailable = AuthenticationRejection::ServiceUnavailable;
    let (Some(account), Some(module)) = (
        StableCode::new(&identity.account_id),
        OpaqueToken::new(&identity.client_module),
    ) else {
        return unavailable.response(correlation_id);
    };
    let Some(result) = typed_field(ACCOUNT_FIELD, TypedValue::Code(account)).and_then(|result| {
        let name = StableCode::new(CLIENT_MODULE_FIELD)?;
        result.with_field(name, TypedValue::Token(module))
    }) else {
        return unavailable.response(correlation_id);
    };
    match ResponseCorrelation::new(correlation_id) {
        Some(correlation) => typed_json_response(
            StatusCode::OK,
            TypedJsonEnvelope::Result {
                result,
                correlation_id: correlation,
            },
        ),
        None => unavailable.response(correlation_id),
    }
}

/// Renders the completion envelope of a revoked session.
fn session_cleared_response(correlation_id: &str) -> Response {
    let unavailable = AuthenticationRejection::ServiceUnavailable;
    let Some(ended) = StableCode::new(SESSION_ENDED) else {
        return unavailable.response(correlation_id);
    };
    let Some(result) = typed_field(SESSION_FIELD, TypedValue::Code(ended)) else {
        return unavailable.response(correlation_id);
    };
    match ResponseCorrelation::new(correlation_id) {
        Some(correlation) => typed_json_response_with_cookies(
            StatusCode::OK,
            TypedJsonEnvelope::Result {
                result,
                correlation_id: correlation,
            },
            CookieEffect::ClearSession,
        ),
        None => unavailable.response(correlation_id),
    }
}

/// Builds a one-field typed result, or `None` when the field name is invalid.
fn typed_field(name: &str, value: TypedValue) -> Option<TypedResult> {
    TypedResult::new().with_field(StableCode::new(name)?, value)
}

/// Reports the cookie name the readable cross-site request forgery token uses.
///
/// Restated here so a caller reading this contract sees both cookie names
/// without reaching into the cookie module.
#[must_use]
pub const fn csrf_cookie_name() -> &'static str {
    CSRF_COOKIE_NAME
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Request, StatusCode, header::ALLOW},
        routing::MethodRouter,
    };
    use tower::ServiceExt;
    use zeroize::Zeroizing;

    use super::{
        AUTH_LOGIN_ROUTE, AUTH_LOGOUT_ROUTE, AUTH_SESSION_ROUTE, AuthenticationCapability,
        AuthenticationDeclaration, AuthenticationRejection, LoginOutcome, LoginSubmission,
        MAX_LOGIN_BODY_BYTES, SessionEstablished, SessionIdentity, SessionSubmission,
        parse_login_body, parse_login_body_wiped,
    };
    use crate::{
        ExpectedOrigin,
        cookie::CookieEffect,
        typed_json::{TypedJsonEnvelope, typed_json_response},
        wiped_body_support::parse_and_observe,
    };

    const LISTENER: &str = "127.0.0.1:8443";

    /// A response header a route on this surface must never set itself.
    ///
    /// The cookie effect is a response extension the listener renders, so a
    /// literal `Set-Cookie` header on a route response would be a bypass.
    const FORBIDDEN_RESPONSE_HEADERS: [&str; 3] =
        ["set-cookie", "access-control-allow-origin", "cache-control"];

    fn expected_origin() -> ExpectedOrigin {
        ExpectedOrigin::from_listener(LISTENER.parse().expect("the listener authority parses"))
    }

    /// Records how the Server-core hooks were reached.
    #[derive(Default)]
    struct Recorder {
        logins: AtomicUsize,
        validations: AtomicUsize,
        logouts: AtomicUsize,
    }

    struct Harness {
        recorder: Arc<Recorder>,
        router: Router,
    }

    fn harness(
        login: Result<(&'static str, &'static str), AuthenticationRejection>,
        identity: Result<(&'static str, &'static str), AuthenticationRejection>,
        logout: Result<(), AuthenticationRejection>,
    ) -> Harness {
        let recorder = Arc::new(Recorder::default());
        let login_recorder = Arc::clone(&recorder);
        let validate_recorder = Arc::clone(&recorder);
        let logout_recorder = Arc::clone(&recorder);

        let declaration = AuthenticationDeclaration::new(AuthenticationCapability {
            expected_origin: expected_origin(),
            correlate: Arc::new(|| Some("correlation-0123456789".to_owned())),
            login: Arc::new(move |submission: LoginSubmission| {
                let recorder = Arc::clone(&login_recorder);
                Box::pin(async move {
                    recorder.logins.fetch_add(1, Ordering::Relaxed);
                    drop(submission);
                    login.map(|(session, csrf)| {
                        LoginOutcome::SessionEstablished(SessionEstablished {
                            session_token: Zeroizing::new(session.to_owned()),
                            csrf_token: Zeroizing::new(csrf.to_owned()),
                        })
                    })
                })
            }),
            validate_session: Arc::new(move |submission: SessionSubmission| {
                let recorder = Arc::clone(&validate_recorder);
                Box::pin(async move {
                    recorder.validations.fetch_add(1, Ordering::Relaxed);
                    drop(submission);
                    identity.map(|(account_id, client_module)| SessionIdentity {
                        account_id: account_id.to_owned(),
                        client_module: client_module.to_owned(),
                    })
                })
            }),
            logout: Arc::new(move |submission: SessionSubmission| {
                let recorder = Arc::clone(&logout_recorder);
                Box::pin(async move {
                    recorder.logouts.fetch_add(1, Ordering::Relaxed);
                    drop(submission);
                    logout
                })
            }),
        });

        Harness {
            recorder,
            router: routed(AUTH_LOGIN_ROUTE, declaration.login_route())
                .merge(routed(AUTH_SESSION_ROUTE, declaration.session_route()))
                .merge(routed(AUTH_LOGOUT_ROUTE, declaration.logout_route())),
        }
    }

    fn routed(path: &str, route: MethodRouter) -> Router {
        Router::new().route(path, route)
    }

    fn login_request(body: &'static str) -> Request<Body> {
        Request::put(AUTH_LOGIN_ROUTE)
            .header("host", LISTENER)
            .header("origin", format!("https://{LISTENER}"))
            .header("x-weavelit-csrf", "1")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .expect("the login request must build")
    }

    fn session_request(target: &'static str) -> Request<Body> {
        Request::put(target)
            .header("host", LISTENER)
            .header("origin", format!("https://{LISTENER}"))
            .header("x-weavelit-csrf", "csrf-token-value")
            .header("cookie", "__Host-weavelit_session=session-token-value")
            .header("content-length", "0")
            .body(Body::empty())
            .expect("the session request must build")
    }

    async fn envelope(response: axum::response::Response) -> String {
        response
            .extensions()
            .get::<TypedJsonEnvelope>()
            .map_or_else(String::new, TypedJsonEnvelope::serialize)
    }

    fn cookie_effect(response: &axum::response::Response) -> Option<String> {
        response
            .extensions()
            .get::<CookieEffect>()
            .and_then(CookieEffect::render)
            .map(|lines| lines.as_str().to_owned())
    }

    fn assert_sets_no_header_itself(response: &axum::response::Response) {
        for forbidden in FORBIDDEN_RESPONSE_HEADERS {
            assert!(
                !response.headers().contains_key(forbidden),
                "{forbidden} must never be set by a route"
            );
        }
    }

    #[tokio::test]
    async fn a_verified_login_answers_with_the_session_cookie_effect() {
        let harness = harness(
            Ok(("session-token-value", "csrf-token-value")),
            Err(AuthenticationRejection::SessionInvalid),
            Err(AuthenticationRejection::SessionInvalid),
        );

        let response = harness
            .router
            .oneshot(login_request(
                "{\"username\":\"admin\",\"password\":\"secret\",\"client_module\":\"web-ui\"}",
            ))
            .await
            .expect("the route must answer");

        assert_eq!(response.status(), StatusCode::OK);
        assert_sets_no_header_itself(&response);
        assert_eq!(
            cookie_effect(&response).expect("a verified login must carry the cookie effect"),
            "Set-Cookie: __Host-weavelit_session=session-token-value; Secure; HttpOnly; \
             SameSite=Strict; Path=/\r\n\
             Set-Cookie: __Host-weavelit_csrf=csrf-token-value; Secure; SameSite=Strict; \
             Path=/\r\n"
        );
        assert_eq!(
            envelope(response).await,
            "{\"result\":{\"authenticated\":true},\"correlation_id\":\"correlation-0123456789\"}"
        );
        assert_eq!(harness.recorder.logins.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn a_denied_login_answers_one_code_and_no_cookie_effect() {
        let harness = harness(
            Err(AuthenticationRejection::AuthenticationFailed),
            Err(AuthenticationRejection::SessionInvalid),
            Err(AuthenticationRejection::SessionInvalid),
        );

        let response = harness
            .router
            .oneshot(login_request(
                "{\"username\":\"admin\",\"password\":\"secret\",\"client_module\":\"web-ui\"}",
            ))
            .await
            .expect("the route must answer");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(cookie_effect(&response).is_none());
        assert_eq!(
            envelope(response).await,
            "{\"error\":\"authentication_failed\",\
             \"correlation_id\":\"correlation-0123456789\"}"
        );
    }

    #[tokio::test]
    async fn a_login_that_is_not_same_origin_never_reaches_the_server_core() {
        let harness = harness(
            Ok(("session-token-value", "csrf-token-value")),
            Err(AuthenticationRejection::SessionInvalid),
            Err(AuthenticationRejection::SessionInvalid),
        );

        let response = harness
            .router
            .oneshot(
                Request::put(AUTH_LOGIN_ROUTE)
                    .header("host", LISTENER)
                    .header("origin", "https://127.0.0.1:9443")
                    .header("x-weavelit-csrf", "1")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .expect("the request must build"),
            )
            .await
            .expect("the route must answer");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            envelope(response).await,
            "{\"error\":\"request_origin_denied\",\
             \"correlation_id\":\"correlation-0123456789\"}"
        );
        assert_eq!(harness.recorder.logins.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn a_login_by_any_other_method_advertises_only_put() {
        let harness = harness(
            Ok(("session-token-value", "csrf-token-value")),
            Err(AuthenticationRejection::SessionInvalid),
            Err(AuthenticationRejection::SessionInvalid),
        );

        let response = harness
            .router
            .oneshot(
                Request::get(AUTH_LOGIN_ROUTE)
                    .body(Body::empty())
                    .expect("the request must build"),
            )
            .await
            .expect("the route must answer");

        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(response.headers().get(ALLOW).unwrap(), "PUT");
        assert_eq!(harness.recorder.logins.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn a_validated_session_reports_only_its_account_and_client_module() {
        let harness = harness(
            Err(AuthenticationRejection::AuthenticationFailed),
            Ok(("0123456789abcdef0123456789abcdef", "web-ui")),
            Err(AuthenticationRejection::SessionInvalid),
        );

        let response = harness
            .router
            .oneshot(session_request(AUTH_SESSION_ROUTE))
            .await
            .expect("the route must answer");

        assert_eq!(response.status(), StatusCode::OK);
        assert!(cookie_effect(&response).is_none());
        assert_eq!(
            envelope(response).await,
            "{\"result\":{\"account_id\":\"0123456789abcdef0123456789abcdef\",\
             \"client_module\":\"web-ui\"},\"correlation_id\":\"correlation-0123456789\"}"
        );
        assert_eq!(harness.recorder.validations.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn a_session_request_without_a_session_cookie_never_reaches_the_server_core() {
        let harness = harness(
            Err(AuthenticationRejection::AuthenticationFailed),
            Ok(("0123456789abcdef0123456789abcdef", "web-ui")),
            Ok(()),
        );

        for target in [AUTH_SESSION_ROUTE, AUTH_LOGOUT_ROUTE] {
            let response = harness
                .router
                .clone()
                .oneshot(
                    Request::put(target)
                        .header("host", LISTENER)
                        .header("origin", format!("https://{LISTENER}"))
                        .header("x-weavelit-csrf", "csrf-token-value")
                        .header("content-length", "0")
                        .body(Body::empty())
                        .expect("the request must build"),
                )
                .await
                .expect("the route must answer");

            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{target}");
            assert_eq!(
                envelope(response).await,
                "{\"error\":\"session_invalid\",\
                 \"correlation_id\":\"correlation-0123456789\"}",
                "{target}"
            );
        }
        assert_eq!(harness.recorder.validations.load(Ordering::Relaxed), 0);
        assert_eq!(harness.recorder.logouts.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn a_logout_clears_both_cookies() {
        let harness = harness(
            Err(AuthenticationRejection::AuthenticationFailed),
            Err(AuthenticationRejection::SessionInvalid),
            Ok(()),
        );

        let response = harness
            .router
            .oneshot(session_request(AUTH_LOGOUT_ROUTE))
            .await
            .expect("the route must answer");

        assert_eq!(response.status(), StatusCode::OK);
        assert_sets_no_header_itself(&response);
        assert_eq!(
            cookie_effect(&response).expect("a logout must carry the deletion effect"),
            "Set-Cookie: __Host-weavelit_session=; Secure; HttpOnly; SameSite=Strict; \
             Path=/; Max-Age=0\r\n\
             Set-Cookie: __Host-weavelit_csrf=; Secure; SameSite=Strict; Path=/; Max-Age=0\r\n"
        );
        assert_eq!(
            envelope(response).await,
            "{\"result\":{\"session\":\"ended\"},\"correlation_id\":\"correlation-0123456789\"}"
        );
        assert_eq!(harness.recorder.logouts.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn a_session_value_that_cannot_be_rendered_as_a_cookie_emits_no_cookie() {
        let harness = harness(
            Ok(("not a cookie value", "csrf-token-value")),
            Err(AuthenticationRejection::SessionInvalid),
            Err(AuthenticationRejection::SessionInvalid),
        );

        let response = harness
            .router
            .oneshot(login_request(
                "{\"username\":\"admin\",\"password\":\"secret\",\"client_module\":\"web-ui\"}",
            ))
            .await
            .expect("the route must answer");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(cookie_effect(&response).is_none());
        assert_eq!(
            envelope(response).await,
            "{\"error\":\"service_unavailable\",\
             \"correlation_id\":\"correlation-0123456789\"}"
        );
    }

    #[tokio::test]
    async fn a_response_that_cannot_carry_a_correlation_identifier_uses_the_fixed_profile() {
        let declaration = AuthenticationDeclaration::new(AuthenticationCapability {
            expected_origin: expected_origin(),
            correlate: Arc::new(|| None),
            login: Arc::new(|_| {
                Box::pin(async { Err(AuthenticationRejection::AuthenticationFailed) })
            }),
            validate_session: Arc::new(|_| {
                Box::pin(async { Err(AuthenticationRejection::SessionInvalid) })
            }),
            logout: Arc::new(|_| Box::pin(async { Err(AuthenticationRejection::SessionInvalid) })),
        });

        let response = routed(AUTH_LOGIN_ROUTE, declaration.login_route())
            .oneshot(login_request("{}"))
            .await
            .expect("the route must answer");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(response.extensions().get::<TypedJsonEnvelope>().is_none());
        let body = to_bytes(response.into_body(), 128).await.unwrap();
        assert_eq!(body.as_ref(), b"{\"error\":\"service_unavailable\"}");
    }

    #[test]
    fn the_login_body_accepts_exactly_its_documented_shape() {
        let accepted = parse_login_body(
            b"{\"username\":\"admin\",\"password\":\"secret\",\"client_module\":\"web-ui\"}",
        )
        .expect("the documented body must parse");
        assert_eq!(accepted.username, "admin");
        assert_eq!(accepted.password.as_str(), "secret");
        assert_eq!(accepted.client_module, "web-ui");

        for rejected in [
            b"".as_slice(),
            b"[\"admin\",\"secret\",\"web-ui\"]",
            b"{\"username\":\"admin\",\"password\":\"secret\"}",
            b"{\"username\":\"admin\",\"password\":\"secret\",\"client_module\":\"web-ui\",\
              \"extra\":1}",
            b"{\"username\":\"admin\",\"username\":\"other\",\"password\":\"secret\",\
              \"client_module\":\"web-ui\"}",
            b"{\"username\":1,\"password\":\"secret\",\"client_module\":\"web-ui\"}",
            b"{\"username\":\"admin\",\"password\":\"secret\",\"client_module\":\"web-ui\"} trail",
        ] {
            assert!(
                parse_login_body(rejected).is_err(),
                "{}",
                String::from_utf8_lossy(rejected)
            );
        }

        let oversized = vec![b'a'; MAX_LOGIN_BODY_BYTES + 1];
        assert!(parse_login_body(&oversized).is_err());
    }

    #[test]
    fn the_login_body_is_cleared_on_the_parsed_and_rejected_paths_alike() {
        let (parsed, released) = parse_and_observe(
            "{\"username\":\"admin\",\"password\":\"secret\",\"client_module\":\"web-ui\"}",
            parse_login_body_wiped,
        );
        assert_eq!(
            parsed.map(|body| body.password.as_str().to_owned()),
            Ok("secret".to_owned())
        );
        assert_eq!(
            released,
            vec![0u8; released.len()],
            "a parsed login body must leave no readable password bytes behind"
        );
        assert!(!released.is_empty());

        let (rejected, released) = parse_and_observe(
            "{\"username\":\"admin\",\"password\":\"secret\",\"client_module\":\"web-ui\"",
            parse_login_body_wiped,
        );
        assert_eq!(
            rejected.map(|body| body.password.as_str().to_owned()),
            Err(AuthenticationRejection::BadRequest)
        );
        assert_eq!(
            released,
            vec![0u8; released.len()],
            "a rejected login body must leave no readable password bytes behind"
        );
        assert!(!released.is_empty());
    }

    #[test]
    fn every_rejection_renders_a_typed_envelope_with_its_stable_code() {
        for rejection in [
            AuthenticationRejection::BadRequest,
            AuthenticationRejection::AuthenticationFailed,
            AuthenticationRejection::SessionInvalid,
            AuthenticationRejection::RequestOriginDenied,
            AuthenticationRejection::MethodNotAllowed,
            AuthenticationRejection::ServiceUnavailable,
        ] {
            let response = rejection.response("correlation-0123456789");
            assert_eq!(response.status(), rejection.status());
            assert!(response.extensions().get::<CookieEffect>().is_none());
            let envelope = response
                .extensions()
                .get::<TypedJsonEnvelope>()
                .expect("a rejection must carry a typed envelope")
                .serialize();
            assert_eq!(
                envelope,
                format!(
                    "{{\"error\":\"{}\",\"correlation_id\":\"correlation-0123456789\"}}",
                    rejection.code()
                )
            );
        }
    }

    #[test]
    fn a_typed_response_carries_a_cookie_effect_only_when_one_was_supplied() {
        let response = typed_json_response(
            StatusCode::OK,
            TypedJsonEnvelope::Error {
                error: crate::typed_json::StableCode::new("bad_request").unwrap(),
                correlation_id: crate::typed_json::ResponseCorrelation::new("c").unwrap(),
            },
        );
        assert!(response.extensions().get::<CookieEffect>().is_none());
    }
}
