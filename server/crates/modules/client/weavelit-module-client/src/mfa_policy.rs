//! Shared Client Module contract for MFA policy administration.

use std::{fmt, pin::Pin, sync::Arc};

use axum::{
    body::to_bytes,
    extract::Request,
    http::{
        HeaderMap, HeaderValue, Method, StatusCode,
        header::{ALLOW, CONTENT_TYPE},
    },
    response::Response,
    routing::{MethodRouter, any},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::de::{self, Deserialize, Deserializer, MapAccess, Visitor};
use zeroize::Zeroizing;

use crate::{
    ExpectedOrigin, JSON_MEDIA_TYPE, WipedBody, accepts_json,
    administration::{
        AccountAdministrationProjection, AccountAdministrationResult,
        account_administration_success,
    },
    authentication::{
        CorrelationSource, submitted_csrf_token, submitted_session_token, unrenderable_response,
    },
    single_header,
    typed_json::{
        OpaqueToken, ResponseCorrelation, StableCode, TypedJsonEnvelope, TypedResult, TypedValue,
        typed_json_response, typed_json_secret_response,
    },
};

/// Exact route that proves current-session TOTP for an MFA policy action.
pub const MFA_POLICY_STEP_UP_ROUTE: &str = "/api/v1/administration/step-up/totp";

/// Exact route that changes one account's MFA requirement.
pub const ACCOUNTS_MFA_REQUIREMENT_ROUTE: &str = "/api/v1/administration/accounts/mfa-requirement";

/// Exact route that resets one account's TOTP enrollment.
pub const ACCOUNTS_MFA_RESET_ROUTE: &str = "/api/v1/administration/accounts/mfa-reset";

/// Largest secret-bearing request body accepted by these routes.
pub const MAX_MFA_POLICY_BODY_BYTES: usize = 1024;

const TICKET_TEXT_BYTES: usize = 43;
const TICKET_ENTROPY_BYTES: usize = 32;
const ACCOUNT_PUBLIC_ID_TEXT_BYTES: usize = 22;
const ACCOUNT_PUBLIC_ID_BYTES: usize = 16;
const STEP_UP_FIELDS: &[&str] = &["family", "code"];
const REQUIREMENT_FIELDS: &[&str] = &["public_id", "required", "totp_step_up_ticket"];
const RESET_FIELDS: &[&str] = &["public_id", "totp_step_up_ticket"];

/// Publicly supported TOTP step-up families.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MfaPolicyStepUpFamily {
    /// MFA requirement and enrollment-reset administration.
    MfaPolicy,
    /// Group membership, grant, and deletion administration.
    GrantMutation,
}

/// Validated TOTP step-up submission handed to Server core.
pub struct MfaPolicyStepUpSubmission {
    pub family: MfaPolicyStepUpFamily,
    pub session_token: Zeroizing<String>,
    pub csrf_token: Zeroizing<String>,
    pub code: Zeroizing<String>,
    pub correlation_id: String,
    pub context: axum::http::Extensions,
}

/// Validated MFA requirement submission handed to Server core.
pub struct MfaRequirementSubmission {
    pub session_token: Zeroizing<String>,
    pub csrf_token: Zeroizing<String>,
    pub public_id: String,
    pub required: bool,
    pub totp_step_up_ticket: Zeroizing<String>,
    pub correlation_id: String,
    pub context: axum::http::Extensions,
}

/// Validated MFA enrollment-reset submission handed to Server core.
pub struct MfaResetSubmission {
    pub session_token: Zeroizing<String>,
    pub csrf_token: Zeroizing<String>,
    pub public_id: String,
    pub totp_step_up_ticket: Zeroizing<String>,
    pub correlation_id: String,
    pub context: axum::http::Extensions,
}

/// One opaque reusable ticket disclosed by successful TOTP step-up.
pub struct MfaPolicyTicketIssued {
    pub totp_step_up_ticket: Zeroizing<String>,
}

/// Complete redacted rejection vocabulary for MFA policy administration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MfaPolicyRejection {
    BadRequest,
    SessionInvalid,
    RequestOriginDenied,
    AuthorizationDenied,
    MfaPolicyDenied,
    MethodNotAllowed,
    NotFound,
    ServiceUnavailable,
}

impl MfaPolicyRejection {
    #[must_use]
    pub const fn status(self) -> StatusCode {
        match self {
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::SessionInvalid => StatusCode::UNAUTHORIZED,
            Self::RequestOriginDenied | Self::AuthorizationDenied | Self::MfaPolicyDenied => {
                StatusCode::FORBIDDEN
            }
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::ServiceUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::BadRequest => "bad_request",
            Self::SessionInvalid => "session_invalid",
            Self::RequestOriginDenied => "request_origin_denied",
            Self::AuthorizationDenied => "authorization_denied",
            Self::MfaPolicyDenied => "mfa_policy_denied",
            Self::MethodNotAllowed => "method_not_allowed",
            Self::NotFound => "not_found",
            Self::ServiceUnavailable => "service_unavailable",
        }
    }

    #[must_use]
    pub fn response(self, correlation_id: &str) -> Response {
        let (Some(error), Some(correlation_id)) = (
            StableCode::new(self.code()),
            ResponseCorrelation::new(correlation_id),
        ) else {
            return unrenderable_response();
        };
        let mut response = typed_json_response(
            self.status(),
            TypedJsonEnvelope::Error {
                error,
                correlation_id,
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

pub type MfaPolicyStepUpCommit = Arc<
    dyn Fn(
            MfaPolicyStepUpSubmission,
        ) -> Pin<
            Box<dyn Future<Output = Result<MfaPolicyTicketIssued, MfaPolicyRejection>> + Send>,
        > + Send
        + Sync,
>;

pub type MfaRequirementCommit = Arc<
    dyn Fn(
            MfaRequirementSubmission,
        ) -> Pin<
            Box<
                dyn Future<Output = Result<AccountAdministrationProjection, MfaPolicyRejection>>
                    + Send,
            >,
        > + Send
        + Sync,
>;

pub type MfaResetCommit = Arc<
    dyn Fn(
            MfaResetSubmission,
        ) -> Pin<
            Box<
                dyn Future<Output = Result<AccountAdministrationProjection, MfaPolicyRejection>>
                    + Send,
            >,
        > + Send
        + Sync,
>;

/// Collaborators required to declare the three MFA policy routes.
pub struct MfaPolicyCapability {
    pub expected_origin: ExpectedOrigin,
    pub correlate: CorrelationSource,
    pub step_up: MfaPolicyStepUpCommit,
    pub requirement: MfaRequirementCommit,
    pub reset: MfaResetCommit,
}

/// Declared MFA policy routes.
pub struct MfaPolicyDeclaration {
    capability: Arc<MfaPolicyCapability>,
}

impl MfaPolicyDeclaration {
    #[must_use]
    pub fn new(capability: MfaPolicyCapability) -> Self {
        Self {
            capability: Arc::new(capability),
        }
    }

    pub fn step_up_route(&self) -> MethodRouter {
        let capability = Arc::clone(&self.capability);
        any(move |request| step_up_response(request, Arc::clone(&capability)))
    }

    pub fn requirement_route(&self) -> MethodRouter {
        let capability = Arc::clone(&self.capability);
        any(move |request| requirement_response(request, Arc::clone(&capability)))
    }

    pub fn reset_route(&self) -> MethodRouter {
        let capability = Arc::clone(&self.capability);
        any(move |request| reset_response(request, Arc::clone(&capability)))
    }
}

/// Validates method, origin, media, session, and CSRF before body allocation.
pub fn validate_mfa_policy_request(
    method: &Method,
    headers: &HeaderMap,
    expected_origin: ExpectedOrigin,
) -> Result<(), MfaPolicyRejection> {
    if method != Method::PUT {
        return Err(MfaPolicyRejection::MethodNotAllowed);
    }
    if !expected_origin.is_same_origin(headers) {
        return Err(MfaPolicyRejection::RequestOriginDenied);
    }
    let content_type =
        single_header(headers, CONTENT_TYPE).ok_or(MfaPolicyRejection::BadRequest)?;
    if content_type.as_bytes() != JSON_MEDIA_TYPE || !accepts_json(headers) {
        return Err(MfaPolicyRejection::BadRequest);
    }
    submitted_session_token(headers)
        .and_then(|_| submitted_csrf_token(headers))
        .map_err(|_| MfaPolicyRejection::SessionInvalid)?;
    Ok(())
}

struct StepUpBody {
    family: MfaPolicyStepUpFamily,
    code: Zeroizing<String>,
}

impl<'de> Deserialize<'de> for StepUpBody {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BodyVisitor;
        impl<'de> Visitor<'de> for BodyVisitor {
            type Value = StepUpBody;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an exact MFA policy step-up object")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut family: Option<String> = None;
                let mut code: Option<Zeroizing<String>> = None;
                while let Some(field) = map.next_key::<String>()? {
                    match field.as_str() {
                        "family" => {
                            if family.is_some() {
                                return Err(de::Error::duplicate_field("family"));
                            }
                            family = Some(map.next_value()?);
                        }
                        "code" => {
                            if code.is_some() {
                                return Err(de::Error::duplicate_field("code"));
                            }
                            code = Some(Zeroizing::new(map.next_value()?));
                        }
                        unknown => return Err(de::Error::unknown_field(unknown, STEP_UP_FIELDS)),
                    }
                }
                let family = match family.as_deref() {
                    Some("mfa_policy") => MfaPolicyStepUpFamily::MfaPolicy,
                    Some("grant_mutation") => MfaPolicyStepUpFamily::GrantMutation,
                    _ => return Err(de::Error::custom("unsupported step-up family")),
                };
                let code = code.ok_or_else(|| de::Error::missing_field("code"))?;
                if !valid_totp(&code) {
                    return Err(de::Error::custom("TOTP code outside shape"));
                }
                Ok(StepUpBody { family, code })
            }
        }
        deserializer.deserialize_map(BodyVisitor)
    }
}

struct RequirementBody {
    public_id: String,
    required: bool,
    ticket: Zeroizing<String>,
}

impl<'de> Deserialize<'de> for RequirementBody {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BodyVisitor;
        impl<'de> Visitor<'de> for BodyVisitor {
            type Value = RequirementBody;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an exact MFA requirement object")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut public_id = None;
                let mut required = None;
                let mut ticket: Option<Zeroizing<String>> = None;
                while let Some(field) = map.next_key::<String>()? {
                    match field.as_str() {
                        "public_id" => {
                            if public_id.is_some() {
                                return Err(de::Error::duplicate_field("public_id"));
                            }
                            public_id = Some(map.next_value()?);
                        }
                        "required" => {
                            if required.is_some() {
                                return Err(de::Error::duplicate_field("required"));
                            }
                            required = Some(map.next_value()?);
                        }
                        "totp_step_up_ticket" => {
                            if ticket.is_some() {
                                return Err(de::Error::duplicate_field("totp_step_up_ticket"));
                            }
                            ticket = Some(Zeroizing::new(map.next_value()?));
                        }
                        unknown => {
                            return Err(de::Error::unknown_field(unknown, REQUIREMENT_FIELDS));
                        }
                    }
                }
                let body = RequirementBody {
                    public_id: public_id.ok_or_else(|| de::Error::missing_field("public_id"))?,
                    required: required.ok_or_else(|| de::Error::missing_field("required"))?,
                    ticket: ticket
                        .ok_or_else(|| de::Error::missing_field("totp_step_up_ticket"))?,
                };
                if !valid_public_id(&body.public_id) || !valid_ticket(&body.ticket) {
                    return Err(de::Error::custom("MFA requirement value outside shape"));
                }
                Ok(body)
            }
        }
        deserializer.deserialize_map(BodyVisitor)
    }
}

struct ResetBody {
    public_id: String,
    ticket: Zeroizing<String>,
}

impl<'de> Deserialize<'de> for ResetBody {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BodyVisitor;
        impl<'de> Visitor<'de> for BodyVisitor {
            type Value = ResetBody;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an exact MFA enrollment-reset object")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut public_id = None;
                let mut ticket: Option<Zeroizing<String>> = None;
                while let Some(field) = map.next_key::<String>()? {
                    match field.as_str() {
                        "public_id" => {
                            if public_id.is_some() {
                                return Err(de::Error::duplicate_field("public_id"));
                            }
                            public_id = Some(map.next_value()?);
                        }
                        "totp_step_up_ticket" => {
                            if ticket.is_some() {
                                return Err(de::Error::duplicate_field("totp_step_up_ticket"));
                            }
                            ticket = Some(Zeroizing::new(map.next_value()?));
                        }
                        unknown => return Err(de::Error::unknown_field(unknown, RESET_FIELDS)),
                    }
                }
                let body = ResetBody {
                    public_id: public_id.ok_or_else(|| de::Error::missing_field("public_id"))?,
                    ticket: ticket
                        .ok_or_else(|| de::Error::missing_field("totp_step_up_ticket"))?,
                };
                if !valid_public_id(&body.public_id) || !valid_ticket(&body.ticket) {
                    return Err(de::Error::custom("MFA reset value outside shape"));
                }
                Ok(body)
            }
        }
        deserializer.deserialize_map(BodyVisitor)
    }
}

async fn step_up_response(request: Request, capability: Arc<MfaPolicyCapability>) -> Response {
    let Some(correlation_id) = (capability.correlate)() else {
        return unrenderable_response();
    };
    let (parts, body) = request.into_parts();
    if let Err(rejection) =
        validate_mfa_policy_request(&parts.method, &parts.headers, capability.expected_origin)
    {
        return rejection.response(&correlation_id);
    }
    let Some((session_token, csrf_token)) = submitted_tokens(&parts.headers) else {
        return MfaPolicyRejection::SessionInvalid.response(&correlation_id);
    };
    let Ok(parsed) = parse_wiped::<StepUpBody>(body).await else {
        return MfaPolicyRejection::BadRequest.response(&correlation_id);
    };
    match (capability.step_up)(MfaPolicyStepUpSubmission {
        family: parsed.family,
        session_token,
        csrf_token,
        code: parsed.code,
        correlation_id: correlation_id.clone(),
        context: parts.extensions,
    })
    .await
    {
        Ok(result) => ticket_success(result, correlation_id),
        Err(rejection) => rejection.response(&correlation_id),
    }
}

async fn requirement_response(request: Request, capability: Arc<MfaPolicyCapability>) -> Response {
    let Some(correlation_id) = (capability.correlate)() else {
        return unrenderable_response();
    };
    let (parts, body) = request.into_parts();
    if let Err(rejection) =
        validate_mfa_policy_request(&parts.method, &parts.headers, capability.expected_origin)
    {
        return rejection.response(&correlation_id);
    }
    let Some((session_token, csrf_token)) = submitted_tokens(&parts.headers) else {
        return MfaPolicyRejection::SessionInvalid.response(&correlation_id);
    };
    let Ok(parsed) = parse_wiped::<RequirementBody>(body).await else {
        return MfaPolicyRejection::BadRequest.response(&correlation_id);
    };
    match (capability.requirement)(MfaRequirementSubmission {
        session_token,
        csrf_token,
        public_id: parsed.public_id,
        required: parsed.required,
        totp_step_up_ticket: parsed.ticket,
        correlation_id: correlation_id.clone(),
        context: parts.extensions,
    })
    .await
    {
        Ok(result) => account_administration_success(
            AccountAdministrationResult::Status(result),
            correlation_id,
        ),
        Err(rejection) => rejection.response(&correlation_id),
    }
}

async fn reset_response(request: Request, capability: Arc<MfaPolicyCapability>) -> Response {
    let Some(correlation_id) = (capability.correlate)() else {
        return unrenderable_response();
    };
    let (parts, body) = request.into_parts();
    if let Err(rejection) =
        validate_mfa_policy_request(&parts.method, &parts.headers, capability.expected_origin)
    {
        return rejection.response(&correlation_id);
    }
    let Some((session_token, csrf_token)) = submitted_tokens(&parts.headers) else {
        return MfaPolicyRejection::SessionInvalid.response(&correlation_id);
    };
    let Ok(parsed) = parse_wiped::<ResetBody>(body).await else {
        return MfaPolicyRejection::BadRequest.response(&correlation_id);
    };
    match (capability.reset)(MfaResetSubmission {
        session_token,
        csrf_token,
        public_id: parsed.public_id,
        totp_step_up_ticket: parsed.ticket,
        correlation_id: correlation_id.clone(),
        context: parts.extensions,
    })
    .await
    {
        Ok(result) => account_administration_success(
            AccountAdministrationResult::Status(result),
            correlation_id,
        ),
        Err(rejection) => rejection.response(&correlation_id),
    }
}

async fn parse_wiped<T: for<'de> Deserialize<'de>>(body: axum::body::Body) -> Result<T, ()> {
    let body = to_bytes(body, MAX_MFA_POLICY_BODY_BYTES)
        .await
        .map_err(|_| ())?;
    let parsed = match body.try_into_mut() {
        Ok(unique) => parse_json::<T>(WipedBody::new(unique).bytes()),
        Err(shared) => parse_json::<T>(WipedBody::new(shared.to_vec()).bytes()),
    };
    parsed.ok_or(())
}

fn parse_json<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Option<T> {
    let mut deserializer = serde_json::Deserializer::from_slice(body);
    let parsed = T::deserialize(&mut deserializer).ok()?;
    deserializer.end().ok()?;
    Some(parsed)
}

fn submitted_tokens(headers: &HeaderMap) -> Option<(Zeroizing<String>, Zeroizing<String>)> {
    Some((
        Zeroizing::new(submitted_session_token(headers).ok()?.to_owned()),
        Zeroizing::new(submitted_csrf_token(headers).ok()?.to_owned()),
    ))
}

fn ticket_success(result: MfaPolicyTicketIssued, correlation_id: String) -> Response {
    let Some(result) = TypedResult::new().with_field(
        StableCode::new("totp_step_up_ticket").unwrap(),
        TypedValue::Token(match OpaqueToken::new(&result.totp_step_up_ticket) {
            Some(ticket) => ticket,
            None => return unrenderable_response(),
        }),
    ) else {
        return unrenderable_response();
    };
    let Some(correlation_id) = ResponseCorrelation::new(&correlation_id) else {
        return unrenderable_response();
    };
    typed_json_secret_response(
        StatusCode::OK,
        TypedJsonEnvelope::Result {
            result,
            correlation_id,
        },
    )
}

fn valid_totp(code: &str) -> bool {
    code.len() == 6 && code.bytes().all(|byte| byte.is_ascii_digit())
}

fn valid_ticket(value: &str) -> bool {
    value.len() == TICKET_TEXT_BYTES
        && URL_SAFE_NO_PAD.decode(value).is_ok_and(|decoded| {
            decoded.len() == TICKET_ENTROPY_BYTES && URL_SAFE_NO_PAD.encode(decoded) == value
        })
}

fn valid_public_id(value: &str) -> bool {
    value.len() == ACCOUNT_PUBLIC_ID_TEXT_BYTES
        && URL_SAFE_NO_PAD.decode(value).is_ok_and(|decoded| {
            decoded.len() == ACCOUNT_PUBLIC_ID_BYTES
                && decoded.iter().any(|byte| *byte != 0)
                && URL_SAFE_NO_PAD.encode(decoded) == value
        })
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use axum::{
        Router,
        body::Body,
        http::{
            Request, StatusCode,
            header::{ALLOW, CONTENT_TYPE, COOKIE, LOCATION, ORIGIN, SET_COOKIE},
        },
    };
    use tower::ServiceExt;

    use super::*;
    use crate::{administration::AccountAdministrationEnvelope, typed_json::TypedJsonEnvelope};

    const LISTENER: &str = "127.0.0.1:8443";
    const CORRELATION: &str = "mfa-policy-correlation";
    const TICKET: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const PUBLIC_ID: &str = "MTExMTExMTExMTExMTExMQ";

    #[derive(Default)]
    struct Recorder {
        step_up: AtomicUsize,
        requirement: AtomicUsize,
        reset: AtomicUsize,
    }

    fn projection(required: bool) -> AccountAdministrationProjection {
        AccountAdministrationProjection::new(
            PUBLIC_ID.to_owned(),
            "administrator".to_owned(),
            Some("Administrator".to_owned()),
            true,
            required,
        )
        .unwrap()
    }

    fn harness(rejection: Option<MfaPolicyRejection>) -> (Arc<Recorder>, Router) {
        let recorder = Arc::new(Recorder::default());
        let step_up_recorder = Arc::clone(&recorder);
        let requirement_recorder = Arc::clone(&recorder);
        let reset_recorder = Arc::clone(&recorder);
        let capability = MfaPolicyCapability {
            expected_origin: ExpectedOrigin::from_listener(LISTENER.parse().unwrap()),
            correlate: Arc::new(|| Some(CORRELATION.to_owned())),
            step_up: Arc::new(move |submission| {
                let recorder = Arc::clone(&step_up_recorder);
                Box::pin(async move {
                    recorder.step_up.fetch_add(1, Ordering::Relaxed);
                    assert!(matches!(
                        submission.family,
                        MfaPolicyStepUpFamily::MfaPolicy | MfaPolicyStepUpFamily::GrantMutation
                    ));
                    assert_eq!(&*submission.code, "123456");
                    drop(submission);
                    match rejection {
                        Some(rejection) => Err(rejection),
                        None => Ok(MfaPolicyTicketIssued {
                            totp_step_up_ticket: Zeroizing::new(TICKET.to_owned()),
                        }),
                    }
                })
            }),
            requirement: Arc::new(move |submission| {
                let recorder = Arc::clone(&requirement_recorder);
                Box::pin(async move {
                    recorder.requirement.fetch_add(1, Ordering::Relaxed);
                    let required = submission.required;
                    drop(submission);
                    match rejection {
                        Some(rejection) => Err(rejection),
                        None => Ok(projection(required)),
                    }
                })
            }),
            reset: Arc::new(move |submission| {
                let recorder = Arc::clone(&reset_recorder);
                Box::pin(async move {
                    recorder.reset.fetch_add(1, Ordering::Relaxed);
                    drop(submission);
                    match rejection {
                        Some(rejection) => Err(rejection),
                        None => Ok(projection(true)),
                    }
                })
            }),
        };
        let declaration = MfaPolicyDeclaration::new(capability);
        let router = Router::new()
            .route(MFA_POLICY_STEP_UP_ROUTE, declaration.step_up_route())
            .route(
                ACCOUNTS_MFA_REQUIREMENT_ROUTE,
                declaration.requirement_route(),
            )
            .route(ACCOUNTS_MFA_RESET_ROUTE, declaration.reset_route());
        (recorder, router)
    }

    fn request(target: &str, body: impl Into<Body>) -> Request<Body> {
        Request::builder()
            .method("PUT")
            .uri(target)
            .header("host", LISTENER)
            .header(ORIGIN, format!("https://{LISTENER}"))
            .header("x-weavelit-csrf", "csrf-token-value")
            .header(COOKIE, "__Host-weavelit_session=session-token-value")
            .header(CONTENT_TYPE, "application/json")
            .header("accept", "application/json")
            .body(body.into())
            .unwrap()
    }

    fn typed(response: &Response) -> String {
        response
            .extensions()
            .get::<TypedJsonEnvelope>()
            .unwrap()
            .serialize()
            .to_string()
    }

    fn account(response: &Response) -> String {
        response
            .extensions()
            .get::<AccountAdministrationEnvelope>()
            .unwrap()
            .serialize()
            .unwrap()
            .to_string()
    }

    fn assert_no_sensitive_headers(response: &Response) {
        assert!(!response.headers().contains_key(SET_COOKIE));
        assert!(!response.headers().contains_key(LOCATION));
        assert!(
            !response
                .headers()
                .contains_key("access-control-allow-origin")
        );
    }

    #[tokio::test]
    async fn accepted_routes_emit_only_the_ticket_or_safe_account_projection() {
        let (recorder, router) = harness(None);
        let step_up = router
            .clone()
            .oneshot(request(
                MFA_POLICY_STEP_UP_ROUTE,
                r#"{"family":"mfa_policy","code":"123456"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(step_up.status(), StatusCode::OK);
        assert_eq!(
            typed(&step_up),
            format!(
                "{{\"result\":{{\"totp_step_up_ticket\":\"{TICKET}\"}},\"correlation_id\":\"{CORRELATION}\"}}"
            )
        );
        assert_no_sensitive_headers(&step_up);
        assert!(crate::typed_json::has_secret_disclosure_effect(&step_up));

        let requirement = router
            .clone()
            .oneshot(request(
                ACCOUNTS_MFA_REQUIREMENT_ROUTE,
                format!(
                    "{{\"public_id\":\"{PUBLIC_ID}\",\"required\":true,\"totp_step_up_ticket\":\"{TICKET}\"}}"
                ),
            ))
            .await
            .unwrap();
        assert_eq!(requirement.status(), StatusCode::OK);
        assert!(!crate::typed_json::has_secret_disclosure_effect(
            &requirement
        ));
        let requirement_body = account(&requirement);
        assert!(requirement_body.contains("\"mfa_required\":true"));
        assert!(!requirement_body.contains(TICKET));
        assert_no_sensitive_headers(&requirement);

        let reset = router
            .oneshot(request(
                ACCOUNTS_MFA_RESET_ROUTE,
                format!("{{\"public_id\":\"{PUBLIC_ID}\",\"totp_step_up_ticket\":\"{TICKET}\"}}"),
            ))
            .await
            .unwrap();
        assert_eq!(reset.status(), StatusCode::OK);
        assert!(!account(&reset).contains(TICKET));
        assert_eq!(recorder.step_up.load(Ordering::Relaxed), 1);
        assert_eq!(recorder.requirement.load(Ordering::Relaxed), 1);
        assert_eq!(recorder.reset.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn strict_bodies_reject_unsupported_family_and_malformed_values() {
        let (recorder, router) = harness(None);
        let grant_mutation = router
            .clone()
            .oneshot(request(
                MFA_POLICY_STEP_UP_ROUTE,
                r#"{"family":"grant_mutation","code":"123456"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(grant_mutation.status(), StatusCode::OK);
        let cases = [
            (
                MFA_POLICY_STEP_UP_ROUTE,
                r#"{"family":"mfa_policy","code":"12345"}"#.to_owned(),
            ),
            (
                MFA_POLICY_STEP_UP_ROUTE,
                r#"{"family":"mfa_policy","code":"123456","extra":true}"#.to_owned(),
            ),
            (
                MFA_POLICY_STEP_UP_ROUTE,
                r#"{"family":"mfa_policy","family":"mfa_policy","code":"123456"}"#.to_owned(),
            ),
            (
                ACCOUNTS_MFA_REQUIREMENT_ROUTE,
                format!(
                    "{{\"public_id\":\"bad\",\"required\":true,\"totp_step_up_ticket\":\"{TICKET}\"}}"
                ),
            ),
            (
                ACCOUNTS_MFA_REQUIREMENT_ROUTE,
                format!(
                    "{{\"public_id\":\"{PUBLIC_ID}\",\"required\":\"yes\",\"totp_step_up_ticket\":\"{TICKET}\"}}"
                ),
            ),
            (
                ACCOUNTS_MFA_REQUIREMENT_ROUTE,
                format!(
                    "{{\"public_id\":\"{PUBLIC_ID}\",\"required\":true,\"totp_step_up_ticket\":\"short\"}}"
                ),
            ),
            (
                ACCOUNTS_MFA_RESET_ROUTE,
                format!(
                    "{{\"public_id\":\"{PUBLIC_ID}\",\"totp_step_up_ticket\":\"{TICKET}\",\"extra\":true}}"
                ),
            ),
            (
                ACCOUNTS_MFA_RESET_ROUTE,
                format!(
                    "{{\"public_id\":\"{PUBLIC_ID}\",\"public_id\":\"{PUBLIC_ID}\",\"totp_step_up_ticket\":\"{TICKET}\"}}"
                ),
            ),
        ];
        for (target, body) in cases {
            let response = router.clone().oneshot(request(target, body)).await.unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{target}");
            assert_eq!(
                typed(&response),
                format!("{{\"error\":\"bad_request\",\"correlation_id\":\"{CORRELATION}\"}}")
            );
        }
        let oversized = "x".repeat(MAX_MFA_POLICY_BODY_BYTES + 1);
        let response = router
            .oneshot(request(MFA_POLICY_STEP_UP_ROUTE, oversized))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(recorder.step_up.load(Ordering::Relaxed), 1);
        assert_eq!(recorder.requirement.load(Ordering::Relaxed), 0);
        assert_eq!(recorder.reset.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn request_head_enforces_method_origin_media_session_and_csrf() {
        let (recorder, router) = harness(None);
        let body = r#"{"family":"mfa_policy","code":"123456"}"#;
        let mut cases = Vec::new();

        let mut method = request(MFA_POLICY_STEP_UP_ROUTE, body);
        *method.method_mut() = Method::POST;
        cases.push((method, StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed"));

        let mut origin = request(MFA_POLICY_STEP_UP_ROUTE, body);
        origin
            .headers_mut()
            .insert(ORIGIN, "https://elsewhere.test".parse().unwrap());
        cases.push((origin, StatusCode::FORBIDDEN, "request_origin_denied"));

        let mut media = request(MFA_POLICY_STEP_UP_ROUTE, body);
        media.headers_mut().insert(
            CONTENT_TYPE,
            "application/json; charset=utf-8".parse().unwrap(),
        );
        cases.push((media, StatusCode::BAD_REQUEST, "bad_request"));

        let mut accept = request(MFA_POLICY_STEP_UP_ROUTE, body);
        accept
            .headers_mut()
            .insert("accept", "text/plain".parse().unwrap());
        cases.push((accept, StatusCode::BAD_REQUEST, "bad_request"));

        let mut session = request(MFA_POLICY_STEP_UP_ROUTE, body);
        session.headers_mut().remove(COOKIE);
        cases.push((session, StatusCode::UNAUTHORIZED, "session_invalid"));

        let mut csrf = request(MFA_POLICY_STEP_UP_ROUTE, body);
        csrf.headers_mut().remove("x-weavelit-csrf");
        cases.push((csrf, StatusCode::UNAUTHORIZED, "session_invalid"));

        for (request, status, code) in cases {
            let response = router.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), status);
            assert!(typed(&response).contains(&format!("\"error\":\"{code}\"")));
            if status == StatusCode::METHOD_NOT_ALLOWED {
                assert_eq!(response.headers().get(ALLOW).unwrap(), "PUT");
            }
            assert_no_sensitive_headers(&response);
        }
        assert_eq!(recorder.step_up.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn every_execution_rejection_is_stable_and_payload_free() {
        for rejection in [
            MfaPolicyRejection::AuthorizationDenied,
            MfaPolicyRejection::MfaPolicyDenied,
            MfaPolicyRejection::NotFound,
            MfaPolicyRejection::ServiceUnavailable,
        ] {
            let (_, router) = harness(Some(rejection));
            let response = router
                .oneshot(request(
                    MFA_POLICY_STEP_UP_ROUTE,
                    r#"{"family":"mfa_policy","code":"123456"}"#,
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), rejection.status());
            let body = typed(&response);
            assert_eq!(
                body,
                format!(
                    "{{\"error\":\"{}\",\"correlation_id\":\"{CORRELATION}\"}}",
                    rejection.code()
                )
            );
            assert!(!body.contains("123456"));
            assert!(!body.contains(TICKET));
        }
    }
}
