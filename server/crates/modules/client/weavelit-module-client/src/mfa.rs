//! Shared Client Module contract for the second-factor authentication surface.
//!
//! Four routes make up the surface: submitting a code against a continuation
//! issued by login, opening an enrollment from that same continuation, opening
//! an enrollment from a live session and a re-entered password, and confirming
//! an opened enrollment with a code from the new secret.
//!
//! This module owns the canonical routes, the request schemas, every header and
//! cookie precondition, and the response envelopes. It owns no secret store, no
//! clock, no code arithmetic, and no policy: it hands a validated submission to
//! a Server-core hook and renders exactly what that hook returns. It cannot
//! tell why a submission was refused, because the hook does not tell it.
//!
//! Every route here answers in the same closed
//! [`AuthenticationRejection`] vocabulary the login surface answers in, so a
//! wrong code, an unknown continuation, an expired continuation, a replayed
//! code, and a disabled Module are one indistinguishable refusal.

use std::fmt;
use std::pin::Pin;
use std::sync::Arc;

use axum::{
    body::to_bytes,
    extract::Request,
    http::{Extensions, HeaderMap, Method, StatusCode, header::CONTENT_TYPE},
    response::Response,
    routing::{MethodRouter, any},
};
use serde::de::{self, Deserialize, Deserializer, MapAccess, Visitor};
use zeroize::Zeroizing;

use crate::{
    CSRF_HEADER_NAME, ExpectedOrigin, JSON_MEDIA_TYPE, WipedBody, accepts_json,
    authentication::{
        AuthenticationRejection, CorrelationSource, SessionEstablished,
        session_established_response, submitted_csrf_token, submitted_session_token,
    },
    cookie::CookieValue,
    single_header,
    typed_json::{
        OpaqueToken, ProvisioningUri, ResponseCorrelation, StableCode, TypedJsonEnvelope,
        TypedResult, TypedValue, typed_json_response,
    },
};

/// The canonical route that submits a code against a login continuation.
pub const AUTH_MFA_VERIFY_ROUTE: &str = "/api/v1/auth/mfa/verify";

/// The canonical route that opens an enrollment from a login continuation.
pub const AUTH_MFA_ENROLLMENT_ROUTE: &str = "/api/v1/auth/mfa/enrollment";

/// The canonical route that opens an enrollment from a live session.
pub const AUTH_MFA_SELF_ENROLLMENT_ROUTE: &str = "/api/v1/auth/mfa/enrollment/session";

/// The canonical route that confirms an opened enrollment with a code.
pub const AUTH_MFA_ENROLLMENT_CONFIRM_ROUTE: &str = "/api/v1/auth/mfa/enrollment/confirm";

/// Largest request body accepted on any second-factor route.
///
/// Every accepted body carries at most one bearer value, one six-digit code,
/// and one password, so the surface stays inside the listener's default body
/// bound exactly as login does.
pub const MAX_MFA_BODY_BYTES: usize = 1024;

/// Decimal digits in one submitted code.
///
/// Restated here rather than imported so this contract can refuse a body that
/// is not shaped like a code without depending on the Module that verifies it.
pub const MFA_CODE_DIGITS: usize = 6;

/// The result field naming which second factor step a login stopped at.
const MFA_FIELD: &str = "mfa";

/// The value of [`MFA_FIELD`] when an enrolled factor must be presented.
pub const MFA_REQUIRED_CODE: &str = "mfa_required";

/// The value of [`MFA_FIELD`] when the account must enroll before proceeding.
pub const MFA_ENROLLMENT_REQUIRED_CODE: &str = "mfa_enrollment_required";

/// The result field carrying the one-time value that resumes the login.
const CONTINUATION_FIELD: &str = "continuation";

/// The result field carrying the one-time value that confirms an enrollment.
const ENROLLMENT_FIELD: &str = "enrollment";

/// The result field carrying the Base32 shared secret, disclosed exactly once.
const SECRET_FIELD: &str = "secret";

/// The result field carrying the provisioning URI, disclosed exactly once.
const PROVISIONING_URI_FIELD: &str = "provisioning_uri";

/// The result field reporting a confirmed enrollment.
const ENROLLED_FIELD: &str = "enrolled";

// ---------------------------------------------------------------------------
// Submissions and Server-core hooks
// ---------------------------------------------------------------------------

/// A validated code submission handed to the Server core.
pub struct MfaCodeSubmission {
    /// The one-time value the earlier response issued.
    pub continuation: Zeroizing<String>,
    /// The submitted code, still unverified.
    pub code: Zeroizing<String>,
    /// The Server-generated correlation identifier for this attempt.
    pub correlation_id: String,
    /// The admitted request's extensions.
    pub context: Extensions,
}

/// A validated request to open an enrollment from a login continuation.
pub struct MfaEnrollmentSubmission {
    /// The one-time value login issued.
    pub continuation: Zeroizing<String>,
    /// The Server-generated correlation identifier for this attempt.
    pub correlation_id: String,
    /// The admitted request's extensions.
    pub context: Extensions,
}

/// A validated request to open an enrollment from a live session.
pub struct MfaSelfEnrollmentSubmission {
    /// The session bearer value read from the session cookie.
    pub session_token: Zeroizing<String>,
    /// The cross-site request forgery token echoed in the request header.
    pub csrf_token: Zeroizing<String>,
    /// The re-entered current password, still unverified.
    pub password: Zeroizing<String>,
    /// The Server-generated correlation identifier for this attempt.
    pub correlation_id: String,
    /// The admitted request's extensions.
    pub context: Extensions,
}

/// The one-time provisioning data an opened enrollment discloses.
///
/// The three values are returned in exactly one response and are never
/// retrievable afterwards. Nothing in this crate stores, logs, or renders them
/// outside [`enrollment_opened_response`].
pub struct MfaEnrollmentOpened {
    /// The unpadded Base32 shared secret.
    pub secret: Zeroizing<String>,
    /// The `otpauth://` provisioning URI carrying that same secret.
    ///
    /// The typed value is built by the Server core before it issues the
    /// one-time confirmation ticket, so a URI this envelope could not carry is
    /// refused while the enrollment is still repeatable. Rendering it here is
    /// therefore infallible.
    pub provisioning_uri: ProvisioningUri,
    /// The one-time value that confirms this enrollment.
    pub enrollment: Zeroizing<String>,
}

/// Server-core hook that verifies a code against a login continuation.
pub type MfaCodeCommit = Arc<
    dyn Fn(
            MfaCodeSubmission,
        ) -> Pin<
            Box<dyn Future<Output = Result<SessionEstablished, AuthenticationRejection>> + Send>,
        > + Send
        + Sync,
>;

/// Server-core hook that opens an enrollment from a login continuation.
pub type MfaEnrollmentCommit = Arc<
    dyn Fn(
            MfaEnrollmentSubmission,
        ) -> Pin<
            Box<dyn Future<Output = Result<MfaEnrollmentOpened, AuthenticationRejection>> + Send>,
        > + Send
        + Sync,
>;

/// Server-core hook that opens an enrollment from a live session.
pub type MfaSelfEnrollmentCommit = Arc<
    dyn Fn(
            MfaSelfEnrollmentSubmission,
        ) -> Pin<
            Box<dyn Future<Output = Result<MfaEnrollmentOpened, AuthenticationRejection>> + Send>,
        > + Send
        + Sync,
>;

/// Server-core hook that confirms an opened enrollment with a code.
pub type MfaEnrollmentConfirmCommit = Arc<
    dyn Fn(
            MfaCodeSubmission,
        ) -> Pin<
            Box<dyn Future<Output = Result<SessionEstablished, AuthenticationRejection>> + Send>,
        > + Send
        + Sync,
>;

/// The runtime collaborators a Client Module declares the MFA surface with.
pub struct MfaCapability {
    /// The trusted authority every second-factor request must target.
    pub expected_origin: ExpectedOrigin,
    /// The Server-owned correlation identifier source.
    pub correlate: CorrelationSource,
    /// The hook that verifies a code against a login continuation.
    pub verify: MfaCodeCommit,
    /// The hook that opens an enrollment from a login continuation.
    pub open_enrollment: MfaEnrollmentCommit,
    /// The hook that opens an enrollment from a live session.
    pub open_self_enrollment: MfaSelfEnrollmentCommit,
    /// The hook that confirms an opened enrollment.
    pub confirm_enrollment: MfaEnrollmentConfirmCommit,
}

/// A declared second-factor capability, split into its four mountable routes.
pub struct MfaDeclaration {
    capability: Arc<MfaCapability>,
}

impl MfaDeclaration {
    /// Declares the second-factor surface over the supplied collaborators.
    #[must_use]
    pub fn new(capability: MfaCapability) -> Self {
        Self {
            capability: Arc::new(capability),
        }
    }

    /// Returns the route mounted at [`AUTH_MFA_VERIFY_ROUTE`].
    pub fn verify_route(&self) -> MethodRouter {
        let capability = Arc::clone(&self.capability);
        any(move |request| verify_response(request, Arc::clone(&capability)))
    }

    /// Returns the route mounted at [`AUTH_MFA_ENROLLMENT_ROUTE`].
    pub fn enrollment_route(&self) -> MethodRouter {
        let capability = Arc::clone(&self.capability);
        any(move |request| enrollment_response(request, Arc::clone(&capability)))
    }

    /// Returns the route mounted at [`AUTH_MFA_SELF_ENROLLMENT_ROUTE`].
    pub fn self_enrollment_route(&self) -> MethodRouter {
        let capability = Arc::clone(&self.capability);
        any(move |request| self_enrollment_response(request, Arc::clone(&capability)))
    }

    /// Returns the route mounted at [`AUTH_MFA_ENROLLMENT_CONFIRM_ROUTE`].
    pub fn enrollment_confirm_route(&self) -> MethodRouter {
        let capability = Arc::clone(&self.capability);
        any(move |request| enrollment_confirm_response(request, Arc::clone(&capability)))
    }
}

// ---------------------------------------------------------------------------
// Head validation
// ---------------------------------------------------------------------------

/// Validates every header precondition of a continuation-bearing request.
///
/// These requests carry no session, exactly as login does not: the continuation
/// in the body is the only thing binding them to an earlier verified password.
/// They are therefore trusted by exact same-origin validation plus the literal
/// CSRF header value every other pre-session route already requires.
pub fn validate_mfa_request(
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

/// Validates every precondition of the session-bearing enrollment request.
///
/// This request both carries a body and mutates durable state, so it requires
/// exact same-origin validation together with the per-session CSRF token
/// echoed from the readable cookie, and not the pre-session literal.
pub fn validate_mfa_session_request(
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
    let content_type =
        single_header(headers, CONTENT_TYPE).ok_or(AuthenticationRejection::BadRequest)?;
    if content_type.as_bytes() != JSON_MEDIA_TYPE || !accepts_json(headers) {
        return Err(AuthenticationRejection::BadRequest);
    }
    if single_header(headers, CSRF_HEADER_NAME).is_none() {
        return Err(AuthenticationRejection::SessionInvalid);
    }
    submitted_csrf_token(headers)?;
    submitted_session_token(headers)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Request schemas
// ---------------------------------------------------------------------------

/// Declares a strictly validated request body.
///
/// The visitor is generated rather than derived because a derived struct also
/// accepts its JSON array form, which would let a bearer value or a code be
/// submitted through a shape the API contract does not document. An unknown
/// field, a duplicate key, a missing field, a wrongly typed value, and the
/// array form are all rejected. Each secret is wrapped as it is read, so a
/// later rejection clears every decoded secret owner.
macro_rules! strict_body {
    ($name:ident, $expecting:literal, $($field:ident),+ $(,)?) => {
        struct $name {
            $($field: Zeroizing<String>,)+
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                const FIELDS: &[&str] = &[$(stringify!($field),)+];

                struct BodyVisitor;

                impl<'de> Visitor<'de> for BodyVisitor {
                    type Value = $name;

                    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                        formatter.write_str($expecting)
                    }

                    fn visit_map<M: MapAccess<'de>>(
                        self,
                        mut map: M,
                    ) -> Result<Self::Value, M::Error> {
                        $(let mut $field: Option<Zeroizing<String>> = None;)+
                        while let Some(key) = map.next_key::<String>()? {
                            match key.as_str() {
                                $(stringify!($field) => {
                                    if $field.is_some() {
                                        return Err(de::Error::duplicate_field(
                                            stringify!($field),
                                        ));
                                    }
                                    $field = Some(Zeroizing::new(map.next_value()?));
                                })+
                                unknown => {
                                    return Err(de::Error::unknown_field(unknown, FIELDS));
                                }
                            }
                        }
                        Ok($name {
                            $($field: $field.ok_or_else(|| {
                                de::Error::missing_field(stringify!($field))
                            })?,)+
                        })
                    }
                }

                deserializer.deserialize_map(BodyVisitor)
            }
        }
    };
}

strict_body!(
    VerifyBody,
    "a second-factor code submission object",
    continuation,
    code
);
strict_body!(EnrollmentBody, "an enrollment request object", continuation);
strict_body!(
    ConfirmBody,
    "an enrollment confirmation object",
    enrollment,
    code
);

/// The session-bearing enrollment body, whose password is cleared on drop.
///
/// It is written out rather than generated because the password is wrapped as
/// it is read, so no plaintext copy outlives the parse even when a later field
/// rejects the body.
struct SelfEnrollmentBody {
    password: Zeroizing<String>,
}

const SELF_ENROLLMENT_FIELDS: &[&str] = &["password"];

impl<'de> Deserialize<'de> for SelfEnrollmentBody {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BodyVisitor;

        impl<'de> Visitor<'de> for BodyVisitor {
            type Value = SelfEnrollmentBody;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a self-enrollment request object")
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut password: Option<Zeroizing<String>> = None;
                while let Some(field) = map.next_key::<String>()? {
                    match field.as_str() {
                        "password" => {
                            if password.is_some() {
                                return Err(de::Error::duplicate_field("password"));
                            }
                            password = Some(Zeroizing::new(map.next_value()?));
                        }
                        unknown => {
                            return Err(de::Error::unknown_field(unknown, SELF_ENROLLMENT_FIELDS));
                        }
                    }
                }
                Ok(SelfEnrollmentBody {
                    password: password.ok_or_else(|| de::Error::missing_field("password"))?,
                })
            }
        }

        deserializer.deserialize_map(BodyVisitor)
    }
}

/// Parses one accepted body out of a buffer that is wiped when dropped.
///
/// These bodies carry one-time codes, continuation bearers, and
/// self-enrollment credential material, so each is read through the shared
/// [`WipedBody`] guard: the buffer is cleared on the parsed path and on every
/// rejection path alike, because the guard owns it for the whole call.
fn parse_body_wiped<T: for<'de> Deserialize<'de>, B: AsMut<[u8]>>(
    buffer: B,
) -> Result<T, AuthenticationRejection> {
    let mut body = WipedBody::new(buffer);
    parse_body(body.bytes())
}

/// Parses one accepted body, rejecting anything outside its exact shape.
fn parse_body<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, AuthenticationRejection> {
    if body.len() > MAX_MFA_BODY_BYTES {
        return Err(AuthenticationRejection::BadRequest);
    }
    let mut deserializer = serde_json::Deserializer::from_slice(body);
    let parsed =
        T::deserialize(&mut deserializer).map_err(|_| AuthenticationRejection::BadRequest)?;
    deserializer
        .end()
        .map_err(|_| AuthenticationRejection::BadRequest)?;
    Ok(parsed)
}

/// Accepts only a value shaped like an issued one-time bearer value.
///
/// A value outside the issued shape is refused before it reaches the Server
/// core, so a malformed submission costs no store lookup. It is refused as a
/// denied submission rather than as a malformed request, so a client cannot
/// learn the accepted shape by watching which status it receives.
fn submitted_bearer(
    value: Zeroizing<String>,
) -> Result<Zeroizing<String>, AuthenticationRejection> {
    if CookieValue::new(&value).is_none() {
        return Err(AuthenticationRejection::AuthenticationFailed);
    }
    Ok(value)
}

/// Accepts only a value shaped like a code this profile issues.
///
/// Shape alone is checked here; whether the digits are the right ones is
/// decided by the Server core against a secret this crate never sees. A wrong
/// shape and a wrong code are the same refusal.
fn submitted_code(value: Zeroizing<String>) -> Result<Zeroizing<String>, AuthenticationRejection> {
    let accepted =
        value.len() == MFA_CODE_DIGITS && value.bytes().all(|byte| byte.is_ascii_digit());
    if !accepted {
        return Err(AuthenticationRejection::AuthenticationFailed);
    }
    Ok(value)
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

async fn verify_response(request: Request, capability: Arc<MfaCapability>) -> Response {
    let Some(correlation_id) = (capability.correlate)() else {
        return crate::authentication::unrenderable_response();
    };
    let submission = match code_submission(request, &capability, &correlation_id).await {
        Ok(submission) => submission,
        Err(rejection) => return rejection.response(&correlation_id),
    };

    match (capability.verify)(submission).await {
        Ok(established) => session_established_response(&established, &correlation_id),
        Err(rejection) => rejection.response(&correlation_id),
    }
}

async fn enrollment_confirm_response(request: Request, capability: Arc<MfaCapability>) -> Response {
    let Some(correlation_id) = (capability.correlate)() else {
        return crate::authentication::unrenderable_response();
    };
    let submission = match confirm_submission(request, &capability, &correlation_id).await {
        Ok(submission) => submission,
        Err(rejection) => return rejection.response(&correlation_id),
    };

    match (capability.confirm_enrollment)(submission).await {
        Ok(established) => session_established_response(&established, &correlation_id),
        Err(rejection) => rejection.response(&correlation_id),
    }
}

async fn enrollment_response(request: Request, capability: Arc<MfaCapability>) -> Response {
    let Some(correlation_id) = (capability.correlate)() else {
        return crate::authentication::unrenderable_response();
    };
    let (parts, body) = request.into_parts();
    if let Err(rejection) =
        validate_mfa_request(&parts.method, &parts.headers, capability.expected_origin)
    {
        return rejection.response(&correlation_id);
    }
    let Ok(bytes) = to_bytes(body, MAX_MFA_BODY_BYTES).await else {
        return AuthenticationRejection::BadRequest.response(&correlation_id);
    };
    // `Bytes` is shared and immutable, so the collected buffer can only be
    // wiped once this crate holds it uniquely, which is the ordinary outcome
    // for a collected request body. If a clone is outstanding the fallback
    // still wipes the copy this crate owns; the shared original is out of
    // reach and is left to its own owner.
    let parsed = match bytes.try_into_mut() {
        Ok(unique) => parse_body_wiped::<EnrollmentBody, _>(unique),
        Err(shared) => parse_body_wiped::<EnrollmentBody, _>(shared.to_vec()),
    };
    let submitted = match parsed.and_then(|parsed| submitted_bearer(parsed.continuation)) {
        Ok(submitted) => submitted,
        Err(rejection) => return rejection.response(&correlation_id),
    };

    match (capability.open_enrollment)(MfaEnrollmentSubmission {
        continuation: submitted,
        correlation_id: correlation_id.clone(),
        context: parts.extensions,
    })
    .await
    {
        Ok(opened) => enrollment_opened_response(&opened, &correlation_id),
        Err(rejection) => rejection.response(&correlation_id),
    }
}

async fn self_enrollment_response(request: Request, capability: Arc<MfaCapability>) -> Response {
    let Some(correlation_id) = (capability.correlate)() else {
        return crate::authentication::unrenderable_response();
    };
    let (parts, body) = request.into_parts();
    if let Err(rejection) =
        validate_mfa_session_request(&parts.method, &parts.headers, capability.expected_origin)
    {
        return rejection.response(&correlation_id);
    }
    let (session_token, csrf_token) = match (
        submitted_session_token(&parts.headers),
        submitted_csrf_token(&parts.headers),
    ) {
        (Ok(session), Ok(csrf)) => (
            Zeroizing::new(session.to_owned()),
            Zeroizing::new(csrf.to_owned()),
        ),
        _ => return AuthenticationRejection::SessionInvalid.response(&correlation_id),
    };
    let Ok(bytes) = to_bytes(body, MAX_MFA_BODY_BYTES).await else {
        return AuthenticationRejection::BadRequest.response(&correlation_id);
    };
    // The same unique-ownership reasoning as the enrollment request applies
    // here: the fallback wipes the copy this crate owns rather than rejecting.
    let parsed = match bytes.try_into_mut() {
        Ok(unique) => parse_body_wiped::<SelfEnrollmentBody, _>(unique),
        Err(shared) => parse_body_wiped::<SelfEnrollmentBody, _>(shared.to_vec()),
    };
    let submitted = match parsed {
        Ok(submitted) => submitted,
        Err(rejection) => return rejection.response(&correlation_id),
    };

    match (capability.open_self_enrollment)(MfaSelfEnrollmentSubmission {
        session_token,
        csrf_token,
        password: submitted.password,
        correlation_id: correlation_id.clone(),
        context: parts.extensions,
    })
    .await
    {
        Ok(opened) => enrollment_opened_response(&opened, &correlation_id),
        Err(rejection) => rejection.response(&correlation_id),
    }
}

/// Validates and builds one continuation-and-code submission.
async fn code_submission(
    request: Request,
    capability: &MfaCapability,
    correlation_id: &str,
) -> Result<MfaCodeSubmission, AuthenticationRejection> {
    let (parts, body) = request.into_parts();
    validate_mfa_request(&parts.method, &parts.headers, capability.expected_origin)?;
    let bytes = to_bytes(body, MAX_MFA_BODY_BYTES)
        .await
        .map_err(|_| AuthenticationRejection::BadRequest)?;
    // The same unique-ownership reasoning as the enrollment request applies
    // here: the fallback wipes the copy this crate owns rather than rejecting.
    let parsed = match bytes.try_into_mut() {
        Ok(unique) => parse_body_wiped::<VerifyBody, _>(unique),
        Err(shared) => parse_body_wiped::<VerifyBody, _>(shared.to_vec()),
    }?;

    Ok(MfaCodeSubmission {
        continuation: submitted_bearer(parsed.continuation)?,
        code: submitted_code(parsed.code)?,
        correlation_id: correlation_id.to_owned(),
        context: parts.extensions,
    })
}

/// Validates and builds one enrollment-and-code submission.
async fn confirm_submission(
    request: Request,
    capability: &MfaCapability,
    correlation_id: &str,
) -> Result<MfaCodeSubmission, AuthenticationRejection> {
    let (parts, body) = request.into_parts();
    validate_mfa_request(&parts.method, &parts.headers, capability.expected_origin)?;
    let bytes = to_bytes(body, MAX_MFA_BODY_BYTES)
        .await
        .map_err(|_| AuthenticationRejection::BadRequest)?;
    // The same unique-ownership reasoning as the enrollment request applies
    // here: the fallback wipes the copy this crate owns rather than rejecting.
    let parsed = match bytes.try_into_mut() {
        Ok(unique) => parse_body_wiped::<ConfirmBody, _>(unique),
        Err(shared) => parse_body_wiped::<ConfirmBody, _>(shared.to_vec()),
    }?;

    Ok(MfaCodeSubmission {
        continuation: submitted_bearer(parsed.enrollment)?,
        code: submitted_code(parsed.code)?,
        correlation_id: correlation_id.to_owned(),
        context: parts.extensions,
    })
}

// ---------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------

/// Renders the `202` a login that verified a password but issued no session
/// answers with.
///
/// The envelope carries no cookie effect at all, so a login that stopped at the
/// second factor cannot establish a session through this path.
#[must_use]
pub fn continuation_response(stage: &str, continuation: &str, correlation_id: &str) -> Response {
    let unavailable = AuthenticationRejection::ServiceUnavailable;
    let (Some(stage), Some(continuation), Some(correlation)) = (
        StableCode::new(stage),
        OpaqueToken::new(continuation),
        ResponseCorrelation::new(correlation_id),
    ) else {
        return unavailable.response(correlation_id);
    };
    let Some(result) = field(MFA_FIELD, TypedValue::Code(stage)).and_then(|result| {
        result.with_field(
            StableCode::new(CONTINUATION_FIELD)?,
            TypedValue::Token(continuation),
        )
    }) else {
        return unavailable.response(correlation_id);
    };
    typed_json_response(
        StatusCode::ACCEPTED,
        TypedJsonEnvelope::Result {
            result,
            correlation_id: correlation,
        },
    )
}

/// Renders the single response that discloses provisioning data.
///
/// The secret and the URI appear here and nowhere else in this crate. They are
/// not logged, not echoed into any rejection, and not retained: the borrowed
/// value is cleared by its owner as soon as this call returns.
fn enrollment_opened_response(opened: &MfaEnrollmentOpened, correlation_id: &str) -> Response {
    let unavailable = AuthenticationRejection::ServiceUnavailable;
    let (Some(secret), Some(enrollment), Some(correlation)) = (
        OpaqueToken::new(&opened.secret),
        OpaqueToken::new(&opened.enrollment),
        ResponseCorrelation::new(correlation_id),
    ) else {
        return unavailable.response(correlation_id);
    };
    let Some(result) = field(SECRET_FIELD, TypedValue::Token(secret))
        .and_then(|result| {
            result.with_field(
                StableCode::new(PROVISIONING_URI_FIELD)?,
                TypedValue::Uri(opened.provisioning_uri.clone()),
            )
        })
        .and_then(|result| {
            result.with_field(
                StableCode::new(ENROLLMENT_FIELD)?,
                TypedValue::Token(enrollment),
            )
        })
    else {
        return unavailable.response(correlation_id);
    };
    typed_json_response(
        StatusCode::OK,
        TypedJsonEnvelope::Result {
            result,
            correlation_id: correlation,
        },
    )
}

/// Reports the result field a confirmed enrollment sets.
#[must_use]
pub const fn enrolled_field_name() -> &'static str {
    ENROLLED_FIELD
}

/// Builds a one-field typed result, or `None` when the field name is invalid.
fn field(name: &str, value: TypedValue) -> Option<TypedResult> {
    TypedResult::new().with_field(StableCode::new(name)?, value)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
        response::Response,
        routing::MethodRouter,
    };
    use serde::de::Deserialize;
    use tower::ServiceExt as _;
    use zeroize::Zeroizing;

    use super::{
        AUTH_MFA_ENROLLMENT_CONFIRM_ROUTE, AUTH_MFA_ENROLLMENT_ROUTE,
        AUTH_MFA_SELF_ENROLLMENT_ROUTE, AUTH_MFA_VERIFY_ROUTE, ConfirmBody, EnrollmentBody,
        MFA_ENROLLMENT_REQUIRED_CODE, MFA_REQUIRED_CODE, MfaCapability, MfaDeclaration,
        MfaEnrollmentOpened, SelfEnrollmentBody, VerifyBody, continuation_response,
        parse_body_wiped,
    };
    use crate::{
        ExpectedOrigin,
        authentication::{AuthenticationRejection, SessionEstablished},
        cookie::CookieEffect,
        typed_json::{ProvisioningUri, TypedJsonEnvelope},
        wiped_body_support::parse_and_observe,
    };

    const LISTENER: &str = "127.0.0.1:8443";
    const CORRELATION: &str = "correlation-0123456789";
    const CONTINUATION: &str = "Y29udGludWF0aW9uLXZhbHVlLWZvci10ZXN0aW5nLTAx";
    const ENROLLMENT: &str = "ZW5yb2xsbWVudC12YWx1ZS1mb3ItdGVzdGluZy0wMDAwMDA";
    const SECRET: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
    const URI: &str = "otpauth://totp/Weavelit:first-admin\
                       ?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&issuer=Weavelit\
                       &algorithm=SHA1&digits=6&period=30";

    fn expected_origin() -> ExpectedOrigin {
        ExpectedOrigin::from_listener(LISTENER.parse().expect("the listener authority parses"))
    }

    fn opened() -> MfaEnrollmentOpened {
        MfaEnrollmentOpened {
            secret: Zeroizing::new(SECRET.to_owned()),
            provisioning_uri: ProvisioningUri::new(URI)
                .expect("the fixture provisioning uri must be accepted"),
            enrollment: Zeroizing::new(ENROLLMENT.to_owned()),
        }
    }

    fn established() -> SessionEstablished {
        SessionEstablished {
            session_token: Zeroizing::new("session-token-value".to_owned()),
            csrf_token: Zeroizing::new("csrf-token-value".to_owned()),
        }
    }

    /// Declares the surface with hooks that always succeed.
    ///
    /// The refusals proved here are the ones this contract decides on its own,
    /// so the hooks never refuse: a refused response can only have come from
    /// validation in this module.
    fn accepting_router() -> Router {
        let declaration = MfaDeclaration::new(MfaCapability {
            expected_origin: expected_origin(),
            correlate: Arc::new(|| Some(CORRELATION.to_owned())),
            verify: Arc::new(|_| Box::pin(async { Ok(established()) })),
            open_enrollment: Arc::new(|_| Box::pin(async { Ok(opened()) })),
            open_self_enrollment: Arc::new(|_| Box::pin(async { Ok(opened()) })),
            confirm_enrollment: Arc::new(|_| Box::pin(async { Ok(established()) })),
        });

        routed(AUTH_MFA_VERIFY_ROUTE, declaration.verify_route())
            .merge(routed(
                AUTH_MFA_ENROLLMENT_ROUTE,
                declaration.enrollment_route(),
            ))
            .merge(routed(
                AUTH_MFA_SELF_ENROLLMENT_ROUTE,
                declaration.self_enrollment_route(),
            ))
            .merge(routed(
                AUTH_MFA_ENROLLMENT_CONFIRM_ROUTE,
                declaration.enrollment_confirm_route(),
            ))
    }

    fn routed(path: &str, route: MethodRouter) -> Router {
        Router::new().route(path, route)
    }

    fn submission(route: &str, body: &str) -> Request<Body> {
        Request::put(route)
            .header("host", LISTENER)
            .header("origin", format!("https://{LISTENER}"))
            .header("x-weavelit-csrf", "1")
            .header("content-type", "application/json")
            .body(Body::from(body.to_owned()))
            .expect("the request must build")
    }

    fn envelope(response: &Response) -> String {
        response
            .extensions()
            .get::<TypedJsonEnvelope>()
            .map_or_else(String::new, |envelope| envelope.serialize().to_string())
    }

    async fn answered(request: Request<Body>) -> Response {
        accepting_router()
            .oneshot(request)
            .await
            .expect("the route must answer")
    }

    #[tokio::test]
    async fn an_opened_enrollment_discloses_its_secret_uri_and_confirmation_value() {
        let response = answered(submission(
            AUTH_MFA_ENROLLMENT_ROUTE,
            &format!("{{\"continuation\":\"{CONTINUATION}\"}}"),
        ))
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            envelope(&response),
            format!(
                "{{\"result\":{{\"secret\":\"{SECRET}\",\"provisioning_uri\":\"{URI}\",\
                 \"enrollment\":\"{ENROLLMENT}\"}},\"correlation_id\":\"{CORRELATION}\"}}"
            )
        );
    }

    #[tokio::test]
    async fn opening_an_enrollment_never_carries_a_cookie_effect() {
        let response = answered(submission(
            AUTH_MFA_ENROLLMENT_ROUTE,
            &format!("{{\"continuation\":\"{CONTINUATION}\"}}"),
        ))
        .await;

        assert!(response.extensions().get::<CookieEffect>().is_none());
    }

    #[tokio::test]
    async fn a_verified_code_renders_the_established_session_envelope() {
        for (route, body) in [
            (
                AUTH_MFA_VERIFY_ROUTE,
                format!("{{\"continuation\":\"{CONTINUATION}\",\"code\":\"287082\"}}"),
            ),
            (
                AUTH_MFA_ENROLLMENT_CONFIRM_ROUTE,
                format!("{{\"enrollment\":\"{ENROLLMENT}\",\"code\":\"287082\"}}"),
            ),
        ] {
            let response = answered(submission(route, &body)).await;

            assert_eq!(response.status(), StatusCode::OK, "{route}");
            assert_eq!(
                envelope(&response),
                format!(
                    "{{\"result\":{{\"authenticated\":true}},\
                     \"correlation_id\":\"{CORRELATION}\"}}"
                ),
                "{route}"
            );
            assert!(
                response.extensions().get::<CookieEffect>().is_some(),
                "{route}"
            );
        }
    }

    #[tokio::test]
    async fn a_body_outside_the_documented_shape_is_refused() {
        for (route, body) in [
            (AUTH_MFA_VERIFY_ROUTE, "{\"continuation\":\"x\"}"),
            (AUTH_MFA_VERIFY_ROUTE, "[]"),
            (
                AUTH_MFA_VERIFY_ROUTE,
                "{\"continuation\":\"a\",\"code\":\"123456\",\"extra\":1}",
            ),
            (AUTH_MFA_ENROLLMENT_ROUTE, "{}"),
            (AUTH_MFA_ENROLLMENT_CONFIRM_ROUTE, "{\"code\":\"123456\"}"),
        ] {
            let response = answered(submission(route, body)).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{route} {body}");
        }
    }

    /// Pins every generated strict-body secret field to the clearing owner.
    ///
    /// The explicit type annotations make a regression to `String` fail to
    /// compile, including on parser rejection paths where a returned body is
    /// unavailable for observation.
    #[test]
    fn strict_mfa_body_secret_fields_are_zeroizing_strings() {
        let parsed = parse_body_wiped::<VerifyBody, _>(
            format!("{{\"continuation\":\"{CONTINUATION}\",\"code\":\"287082\"}}").into_bytes(),
        )
        .expect("the verification body parses");
        let continuation: Zeroizing<String> = parsed.continuation;
        let code: Zeroizing<String> = parsed.code;
        assert_eq!(continuation.as_str(), CONTINUATION);
        assert_eq!(code.as_str(), "287082");

        let parsed = parse_body_wiped::<EnrollmentBody, _>(
            format!("{{\"continuation\":\"{CONTINUATION}\"}}").into_bytes(),
        )
        .expect("the enrollment body parses");
        let continuation: Zeroizing<String> = parsed.continuation;
        assert_eq!(continuation.as_str(), CONTINUATION);

        let parsed = parse_body_wiped::<ConfirmBody, _>(
            format!("{{\"enrollment\":\"{ENROLLMENT}\",\"code\":\"287082\"}}").into_bytes(),
        )
        .expect("the enrollment confirmation body parses");
        let enrollment: Zeroizing<String> = parsed.enrollment;
        let code: Zeroizing<String> = parsed.code;
        assert_eq!(enrollment.as_str(), ENROLLMENT);
        assert_eq!(code.as_str(), "287082");
    }

    #[test]
    fn every_second_factor_body_is_cleared_on_the_parsed_and_rejected_paths_alike() {
        assert_cleared::<VerifyBody>(&format!(
            "{{\"continuation\":\"{CONTINUATION}\",\"code\":\"287082\"}}"
        ));
        assert_cleared::<EnrollmentBody>(&format!("{{\"continuation\":\"{CONTINUATION}\"}}"));
        assert_cleared::<ConfirmBody>(&format!(
            "{{\"enrollment\":\"{ENROLLMENT}\",\"code\":\"287082\"}}"
        ));
        assert_cleared::<SelfEnrollmentBody>("{\"password\":\"correct horse battery staple\"}");
    }

    #[test]
    fn decoded_mfa_secrets_remain_in_clearing_owners_on_every_parse_rejection() {
        for body in [
            format!(
                "{{\"continuation\":\"{CONTINUATION}\",\"code\":\"287082\",\"code\":\"287082\"}}"
            ),
            format!("{{\"continuation\":\"{CONTINUATION}\",\"code\":\"287082\",\"unknown\":true}}"),
            format!("{{\"continuation\":\"{CONTINUATION}\"}}"),
            format!("{{\"continuation\":\"{CONTINUATION}\",\"code\":\"287082\"}} trailing"),
        ] {
            assert_rejected_and_cleared::<VerifyBody>(&body);
        }

        for body in [
            format!("{{\"continuation\":\"{CONTINUATION}\",\"continuation\":\"{CONTINUATION}\"}}"),
            format!("{{\"continuation\":\"{CONTINUATION}\",\"unknown\":true}}"),
            "{}".to_owned(),
            format!("{{\"continuation\":\"{CONTINUATION}\"}} trailing"),
        ] {
            assert_rejected_and_cleared::<EnrollmentBody>(&body);
        }

        for body in [
            format!("{{\"enrollment\":\"{ENROLLMENT}\",\"code\":\"287082\",\"code\":\"287082\"}}"),
            format!("{{\"enrollment\":\"{ENROLLMENT}\",\"code\":\"287082\",\"unknown\":true}}"),
            format!("{{\"enrollment\":\"{ENROLLMENT}\"}}"),
            format!("{{\"enrollment\":\"{ENROLLMENT}\",\"code\":\"287082\"}} trailing"),
        ] {
            assert_rejected_and_cleared::<ConfirmBody>(&body);
        }
    }

    /// Asserts that `body` and a malformed variant of it are both cleared.
    fn assert_cleared<T: for<'de> Deserialize<'de>>(body: &str) {
        let (parsed, released) = parse_and_observe(body, parse_body_wiped::<T, _>);
        assert!(parsed.is_ok(), "the fixture body must parse: {body}");
        assert_eq!(
            released,
            vec![0u8; released.len()],
            "a parsed body must leave no readable secret bytes behind: {body}"
        );
        assert!(!released.is_empty());

        let malformed = format!("{body} trail");
        let (parsed, released) = parse_and_observe(&malformed, parse_body_wiped::<T, _>);
        assert_eq!(
            parsed.err(),
            Some(AuthenticationRejection::BadRequest),
            "the malformed body must be refused: {malformed}"
        );
        assert_eq!(
            released,
            vec![0u8; released.len()],
            "a rejected body must leave no readable secret bytes behind: {malformed}"
        );
        assert!(!released.is_empty());
    }

    fn assert_rejected_and_cleared<T: for<'de> Deserialize<'de>>(body: &str) {
        let (parsed, released) = parse_and_observe(body, parse_body_wiped::<T, _>);
        assert_eq!(
            parsed.err(),
            Some(AuthenticationRejection::BadRequest),
            "the malformed body must be refused: {body}"
        );
        assert_eq!(
            released,
            vec![0u8; released.len()],
            "a rejected body must leave no readable secret bytes behind: {body}"
        );
        assert!(!released.is_empty());
    }

    #[tokio::test]
    async fn a_malformed_bearer_or_code_is_refused_as_a_denied_submission() {
        for (route, body) in [
            (
                AUTH_MFA_VERIFY_ROUTE,
                "{\"continuation\":\"not a token\",\"code\":\"287082\"}".to_owned(),
            ),
            (
                AUTH_MFA_ENROLLMENT_ROUTE,
                "{\"continuation\":\"not a token\"}".to_owned(),
            ),
            (
                AUTH_MFA_ENROLLMENT_CONFIRM_ROUTE,
                "{\"enrollment\":\"not a token\",\"code\":\"287082\"}".to_owned(),
            ),
            (
                AUTH_MFA_VERIFY_ROUTE,
                format!("{{\"continuation\":\"{CONTINUATION}\",\"code\":\"28708\"}}"),
            ),
            (
                AUTH_MFA_ENROLLMENT_CONFIRM_ROUTE,
                format!("{{\"enrollment\":\"{ENROLLMENT}\",\"code\":\"28708\"}}"),
            ),
        ] {
            let response = answered(submission(route, &body)).await;

            assert_eq!(
                response.status(),
                StatusCode::UNAUTHORIZED,
                "{route} {body}"
            );
            assert_eq!(
                envelope(&response),
                format!(
                    "{{\"error\":\"authentication_failed\",\
                     \"correlation_id\":\"{CORRELATION}\"}}"
                ),
                "{route} {body}"
            );
        }
    }

    #[tokio::test]
    async fn every_route_refuses_a_method_other_than_put() {
        for route in [
            AUTH_MFA_VERIFY_ROUTE,
            AUTH_MFA_ENROLLMENT_ROUTE,
            AUTH_MFA_SELF_ENROLLMENT_ROUTE,
            AUTH_MFA_ENROLLMENT_CONFIRM_ROUTE,
        ] {
            let request = Request::get(route)
                .header("host", LISTENER)
                .body(Body::empty())
                .expect("the request must build");
            let response = answered(request).await;

            assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED, "{route}");
        }
    }

    #[tokio::test]
    async fn every_route_refuses_an_untrusted_origin() {
        for route in [
            AUTH_MFA_VERIFY_ROUTE,
            AUTH_MFA_ENROLLMENT_ROUTE,
            AUTH_MFA_SELF_ENROLLMENT_ROUTE,
            AUTH_MFA_ENROLLMENT_CONFIRM_ROUTE,
        ] {
            let request = Request::put(route)
                .header("host", LISTENER)
                .header("origin", "https://attacker.example")
                .header("x-weavelit-csrf", "1")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .expect("the request must build");
            let response = answered(request).await;

            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{route}");
        }
    }

    #[tokio::test]
    async fn the_self_enrollment_route_requires_a_presented_session() {
        let response = answered(submission(
            AUTH_MFA_SELF_ENROLLMENT_ROUTE,
            "{\"password\":\"secret\"}",
        ))
        .await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            envelope(&response),
            format!("{{\"error\":\"session_invalid\",\"correlation_id\":\"{CORRELATION}\"}}")
        );
    }

    #[tokio::test]
    async fn the_self_enrollment_route_opens_an_enrollment_for_a_presented_session() {
        let request = Request::put(AUTH_MFA_SELF_ENROLLMENT_ROUTE)
            .header("host", LISTENER)
            .header("origin", format!("https://{LISTENER}"))
            .header("x-weavelit-csrf", "csrf-token-value")
            .header("cookie", "__Host-weavelit_session=session-token-value")
            .header("content-type", "application/json")
            .body(Body::from("{\"password\":\"secret\"}"))
            .expect("the request must build");
        let response = answered(request).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.extensions().get::<CookieEffect>().is_none());
    }

    #[test]
    fn a_continuation_response_names_its_stage_and_carries_no_cookie() {
        for stage in [MFA_REQUIRED_CODE, MFA_ENROLLMENT_REQUIRED_CODE] {
            let response = continuation_response(stage, CONTINUATION, CORRELATION);

            assert_eq!(response.status(), StatusCode::ACCEPTED);
            assert!(response.extensions().get::<CookieEffect>().is_none());
            assert_eq!(
                envelope(&response),
                format!(
                    "{{\"result\":{{\"mfa\":\"{stage}\",\"continuation\":\"{CONTINUATION}\"}},\
                     \"correlation_id\":\"{CORRELATION}\"}}"
                )
            );
        }
    }
}
