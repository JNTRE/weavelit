//! Shared Client Module contract for account credential issuance.

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
    authentication::{
        CorrelationSource, submitted_csrf_token, submitted_session_token, unrenderable_response,
    },
    single_header,
    typed_json::{
        OpaqueToken, ResponseCorrelation, StableCode, TypedJsonEnvelope, TypedResult, TypedValue,
        typed_json_response, typed_json_secret_response,
    },
};

/// Exact route that proves fresh credential-issuance assurance.
pub const CREDENTIAL_ISSUANCE_STEP_UP_ROUTE: &str =
    "/api/v1/administration/step-up/credential-issuance";

/// Exact route that creates one local account with a temporary password.
pub const ACCOUNTS_CREATE_ROUTE: &str = "/api/v1/administration/accounts/create";

/// Exact route that resets one local account password.
pub const ACCOUNTS_RESET_PASSWORD_ROUTE: &str = "/api/v1/administration/accounts/reset-password";

/// Largest decoded current password accepted by credential assurance.
pub const MAX_CREDENTIAL_ISSUANCE_PASSWORD_BYTES: usize = 1024;

/// Largest secret-bearing request body accepted by these routes.
pub const MAX_CREDENTIAL_ISSUANCE_BODY_BYTES: usize =
    (MAX_CREDENTIAL_ISSUANCE_PASSWORD_BYTES * 6) + 768;

const TICKET_TEXT_BYTES: usize = 43;
const TICKET_ENTROPY_BYTES: usize = 32;
const ACCOUNT_PUBLIC_ID_TEXT_BYTES: usize = 22;
const ACCOUNT_PUBLIC_ID_BYTES: usize = 16;
const MAX_ACCOUNT_NAME_BYTES: usize = 256;
const STEP_UP_FIELDS: &[&str] = &["password", "totp_code"];
const CREATE_FIELDS: &[&str] = &["username", "display_name", "credential_issuance_ticket"];
const RESET_FIELDS: &[&str] = &["public_id", "credential_issuance_ticket"];

/// Validated credential-assurance submission handed to Server core.
pub struct CredentialIssuanceStepUpSubmission {
    pub session_token: Zeroizing<String>,
    pub csrf_token: Zeroizing<String>,
    pub password: Zeroizing<String>,
    pub totp_code: Option<Zeroizing<String>>,
    pub correlation_id: String,
    pub context: axum::http::Extensions,
}

/// Validated account-create submission handed to Server core.
pub struct AccountCreateSubmission {
    pub session_token: Zeroizing<String>,
    pub csrf_token: Zeroizing<String>,
    pub username: String,
    pub display_name: Option<String>,
    pub credential_issuance_ticket: Zeroizing<String>,
    pub correlation_id: String,
    pub context: axum::http::Extensions,
}

/// Validated account password-reset submission handed to Server core.
pub struct AccountPasswordResetSubmission {
    pub session_token: Zeroizing<String>,
    pub csrf_token: Zeroizing<String>,
    pub public_id: String,
    pub credential_issuance_ticket: Zeroizing<String>,
    pub correlation_id: String,
    pub context: axum::http::Extensions,
}

/// One opaque ticket disclosed by successful credential assurance.
pub struct CredentialIssuanceTicketIssued {
    pub credential_issuance_ticket: Zeroizing<String>,
}

/// One committed account credential and its sole plaintext disclosure.
pub struct AccountCredentialIssued {
    pub public_id: String,
    pub temporary_password: Zeroizing<String>,
}

/// Complete redacted rejection vocabulary for credential issuance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialIssuanceRejection {
    BadRequest,
    SessionInvalid,
    RequestOriginDenied,
    AuthorizationDenied,
    CredentialDenied,
    MethodNotAllowed,
    Conflict,
    NotFound,
    ServiceUnavailable,
}

impl CredentialIssuanceRejection {
    #[must_use]
    pub const fn status(self) -> StatusCode {
        match self {
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::SessionInvalid => StatusCode::UNAUTHORIZED,
            Self::RequestOriginDenied | Self::AuthorizationDenied | Self::CredentialDenied => {
                StatusCode::FORBIDDEN
            }
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::Conflict => StatusCode::CONFLICT,
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
            Self::CredentialDenied => "credential_issuance_denied",
            Self::MethodNotAllowed => "method_not_allowed",
            Self::Conflict => "conflict",
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

pub type CredentialIssuanceStepUpCommit = Arc<
    dyn Fn(
            CredentialIssuanceStepUpSubmission,
        ) -> Pin<
            Box<
                dyn Future<
                        Output = Result<
                            CredentialIssuanceTicketIssued,
                            CredentialIssuanceRejection,
                        >,
                    > + Send,
            >,
        > + Send
        + Sync,
>;

pub type AccountCreateCommit = Arc<
    dyn Fn(
            AccountCreateSubmission,
        ) -> Pin<
            Box<
                dyn Future<Output = Result<AccountCredentialIssued, CredentialIssuanceRejection>>
                    + Send,
            >,
        > + Send
        + Sync,
>;

pub type AccountPasswordResetCommit = Arc<
    dyn Fn(
            AccountPasswordResetSubmission,
        ) -> Pin<
            Box<
                dyn Future<Output = Result<AccountCredentialIssued, CredentialIssuanceRejection>>
                    + Send,
            >,
        > + Send
        + Sync,
>;

/// Collaborators required to declare the three credential-issuance routes.
pub struct CredentialIssuanceCapability {
    pub expected_origin: ExpectedOrigin,
    pub correlate: CorrelationSource,
    pub step_up: CredentialIssuanceStepUpCommit,
    pub create: AccountCreateCommit,
    pub reset_password: AccountPasswordResetCommit,
}

/// Declared credential-issuance routes.
pub struct CredentialIssuanceDeclaration {
    capability: Arc<CredentialIssuanceCapability>,
}

impl CredentialIssuanceDeclaration {
    #[must_use]
    pub fn new(capability: CredentialIssuanceCapability) -> Self {
        Self {
            capability: Arc::new(capability),
        }
    }

    pub fn step_up_route(&self) -> MethodRouter {
        let capability = Arc::clone(&self.capability);
        any(move |request| step_up_response(request, Arc::clone(&capability)))
    }

    pub fn create_route(&self) -> MethodRouter {
        let capability = Arc::clone(&self.capability);
        any(move |request| create_response(request, Arc::clone(&capability)))
    }

    pub fn reset_password_route(&self) -> MethodRouter {
        let capability = Arc::clone(&self.capability);
        any(move |request| reset_response(request, Arc::clone(&capability)))
    }
}

/// Validates method, origin, media, session, and CSRF before body allocation.
pub fn validate_credential_issuance_request(
    method: &Method,
    headers: &HeaderMap,
    expected_origin: ExpectedOrigin,
) -> Result<(), CredentialIssuanceRejection> {
    if method != Method::PUT {
        return Err(CredentialIssuanceRejection::MethodNotAllowed);
    }
    if !expected_origin.is_same_origin(headers) {
        return Err(CredentialIssuanceRejection::RequestOriginDenied);
    }
    let content_type =
        single_header(headers, CONTENT_TYPE).ok_or(CredentialIssuanceRejection::BadRequest)?;
    if content_type.as_bytes() != JSON_MEDIA_TYPE || !accepts_json(headers) {
        return Err(CredentialIssuanceRejection::BadRequest);
    }
    submitted_session_token(headers)
        .and_then(|_| submitted_csrf_token(headers))
        .map_err(|_| CredentialIssuanceRejection::SessionInvalid)?;
    Ok(())
}

struct StepUpBody {
    password: Zeroizing<String>,
    totp_code: Option<Zeroizing<String>>,
}

impl<'de> Deserialize<'de> for StepUpBody {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BodyVisitor;
        impl<'de> Visitor<'de> for BodyVisitor {
            type Value = StepUpBody;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an exact credential assurance object")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut password: Option<Zeroizing<String>> = None;
                let mut totp_code: Option<Zeroizing<String>> = None;
                while let Some(field) = map.next_key::<String>()? {
                    match field.as_str() {
                        "password" => {
                            if password.is_some() {
                                return Err(de::Error::duplicate_field("password"));
                            }
                            password = Some(Zeroizing::new(map.next_value::<String>()?));
                        }
                        "totp_code" => {
                            if totp_code.is_some() {
                                return Err(de::Error::duplicate_field("totp_code"));
                            }
                            totp_code = Some(Zeroizing::new(map.next_value::<String>()?));
                        }
                        unknown => return Err(de::Error::unknown_field(unknown, STEP_UP_FIELDS)),
                    }
                }
                let password = password.ok_or_else(|| de::Error::missing_field("password"))?;
                if password.is_empty() || password.len() > MAX_CREDENTIAL_ISSUANCE_PASSWORD_BYTES {
                    return Err(de::Error::custom("password outside bound"));
                }
                if totp_code.as_ref().is_some_and(|code| !valid_totp(code)) {
                    return Err(de::Error::custom("TOTP code outside shape"));
                }
                Ok(StepUpBody {
                    password,
                    totp_code,
                })
            }
        }
        deserializer.deserialize_map(BodyVisitor)
    }
}

struct CreateBody {
    username: String,
    display_name: Option<String>,
    ticket: Zeroizing<String>,
}

impl<'de> Deserialize<'de> for CreateBody {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BodyVisitor;
        impl<'de> Visitor<'de> for BodyVisitor {
            type Value = CreateBody;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an exact account create object")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut username: Option<String> = None;
                let mut display_name: Option<String> = None;
                let mut ticket: Option<Zeroizing<String>> = None;
                while let Some(field) = map.next_key::<String>()? {
                    match field.as_str() {
                        "username" => {
                            if username.is_some() {
                                return Err(de::Error::duplicate_field("username"));
                            }
                            username = Some(map.next_value::<String>()?);
                        }
                        "display_name" => {
                            if display_name.is_some() {
                                return Err(de::Error::duplicate_field("display_name"));
                            }
                            display_name = Some(map.next_value::<String>()?);
                        }
                        "credential_issuance_ticket" => {
                            if ticket.is_some() {
                                return Err(de::Error::duplicate_field(
                                    "credential_issuance_ticket",
                                ));
                            }
                            ticket = Some(Zeroizing::new(map.next_value::<String>()?));
                        }
                        unknown => return Err(de::Error::unknown_field(unknown, CREATE_FIELDS)),
                    }
                }
                let username = username.ok_or_else(|| de::Error::missing_field("username"))?;
                let ticket =
                    ticket.ok_or_else(|| de::Error::missing_field("credential_issuance_ticket"))?;
                if !valid_name(&username)
                    || display_name
                        .as_deref()
                        .is_some_and(|name| !valid_name(name))
                    || !valid_ticket(&ticket)
                {
                    return Err(de::Error::custom("account create value outside shape"));
                }
                Ok(CreateBody {
                    username,
                    display_name,
                    ticket,
                })
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
                formatter.write_str("an exact account password reset object")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut public_id: Option<String> = None;
                let mut ticket: Option<Zeroizing<String>> = None;
                while let Some(field) = map.next_key::<String>()? {
                    match field.as_str() {
                        "public_id" => {
                            if public_id.is_some() {
                                return Err(de::Error::duplicate_field("public_id"));
                            }
                            public_id = Some(map.next_value::<String>()?);
                        }
                        "credential_issuance_ticket" => {
                            if ticket.is_some() {
                                return Err(de::Error::duplicate_field(
                                    "credential_issuance_ticket",
                                ));
                            }
                            ticket = Some(Zeroizing::new(map.next_value::<String>()?));
                        }
                        unknown => return Err(de::Error::unknown_field(unknown, RESET_FIELDS)),
                    }
                }
                let public_id = public_id.ok_or_else(|| de::Error::missing_field("public_id"))?;
                let ticket =
                    ticket.ok_or_else(|| de::Error::missing_field("credential_issuance_ticket"))?;
                if !valid_public_id(&public_id) || !valid_ticket(&ticket) {
                    return Err(de::Error::custom("account reset value outside shape"));
                }
                Ok(ResetBody { public_id, ticket })
            }
        }
        deserializer.deserialize_map(BodyVisitor)
    }
}

async fn step_up_response(
    request: Request,
    capability: Arc<CredentialIssuanceCapability>,
) -> Response {
    let Some(correlation_id) = (capability.correlate)() else {
        return unrenderable_response();
    };
    let (parts, body) = request.into_parts();
    if let Err(rejection) = validate_credential_issuance_request(
        &parts.method,
        &parts.headers,
        capability.expected_origin,
    ) {
        return rejection.response(&correlation_id);
    }
    let Some((session_token, csrf_token)) = submitted_tokens(&parts.headers) else {
        return CredentialIssuanceRejection::SessionInvalid.response(&correlation_id);
    };
    let parsed = parse_wiped::<StepUpBody>(body).await;
    let Ok(parsed) = parsed else {
        return CredentialIssuanceRejection::BadRequest.response(&correlation_id);
    };
    match (capability.step_up)(CredentialIssuanceStepUpSubmission {
        session_token,
        csrf_token,
        password: parsed.password,
        totp_code: parsed.totp_code,
        correlation_id: correlation_id.clone(),
        context: parts.extensions,
    })
    .await
    {
        Ok(result) => ticket_success(result, correlation_id),
        Err(rejection) => rejection.response(&correlation_id),
    }
}

async fn create_response(
    request: Request,
    capability: Arc<CredentialIssuanceCapability>,
) -> Response {
    let Some(correlation_id) = (capability.correlate)() else {
        return unrenderable_response();
    };
    let (parts, body) = request.into_parts();
    if let Err(rejection) = validate_credential_issuance_request(
        &parts.method,
        &parts.headers,
        capability.expected_origin,
    ) {
        return rejection.response(&correlation_id);
    }
    let Some((session_token, csrf_token)) = submitted_tokens(&parts.headers) else {
        return CredentialIssuanceRejection::SessionInvalid.response(&correlation_id);
    };
    let parsed = parse_wiped::<CreateBody>(body).await;
    let Ok(parsed) = parsed else {
        return CredentialIssuanceRejection::BadRequest.response(&correlation_id);
    };
    match (capability.create)(AccountCreateSubmission {
        session_token,
        csrf_token,
        username: parsed.username,
        display_name: parsed.display_name,
        credential_issuance_ticket: parsed.ticket,
        correlation_id: correlation_id.clone(),
        context: parts.extensions,
    })
    .await
    {
        Ok(result) => credential_success(result, correlation_id),
        Err(rejection) => rejection.response(&correlation_id),
    }
}

async fn reset_response(
    request: Request,
    capability: Arc<CredentialIssuanceCapability>,
) -> Response {
    let Some(correlation_id) = (capability.correlate)() else {
        return unrenderable_response();
    };
    let (parts, body) = request.into_parts();
    if let Err(rejection) = validate_credential_issuance_request(
        &parts.method,
        &parts.headers,
        capability.expected_origin,
    ) {
        return rejection.response(&correlation_id);
    }
    let Some((session_token, csrf_token)) = submitted_tokens(&parts.headers) else {
        return CredentialIssuanceRejection::SessionInvalid.response(&correlation_id);
    };
    let parsed = parse_wiped::<ResetBody>(body).await;
    let Ok(parsed) = parsed else {
        return CredentialIssuanceRejection::BadRequest.response(&correlation_id);
    };
    match (capability.reset_password)(AccountPasswordResetSubmission {
        session_token,
        csrf_token,
        public_id: parsed.public_id,
        credential_issuance_ticket: parsed.ticket,
        correlation_id: correlation_id.clone(),
        context: parts.extensions,
    })
    .await
    {
        Ok(result) => credential_success(result, correlation_id),
        Err(rejection) => rejection.response(&correlation_id),
    }
}

async fn parse_wiped<T: for<'de> Deserialize<'de>>(body: axum::body::Body) -> Result<T, ()> {
    let body = to_bytes(body, MAX_CREDENTIAL_ISSUANCE_BODY_BYTES)
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

fn ticket_success(result: CredentialIssuanceTicketIssued, correlation_id: String) -> Response {
    let Some(result) = TypedResult::new().with_field(
        StableCode::new("credential_issuance_ticket").unwrap(),
        TypedValue::Token(match OpaqueToken::new(&result.credential_issuance_ticket) {
            Some(ticket) => ticket,
            None => return unrenderable_response(),
        }),
    ) else {
        return unrenderable_response();
    };
    success(result, correlation_id)
}

fn credential_success(result: AccountCredentialIssued, correlation_id: String) -> Response {
    let AccountCredentialIssued {
        public_id,
        temporary_password,
    } = result;
    if !valid_public_id(&public_id) {
        return unrenderable_response();
    }
    let Some(result) = TypedResult::new()
        .with_field(
            StableCode::new("public_id").unwrap(),
            TypedValue::Token(match OpaqueToken::new(&public_id) {
                Some(public_id) => public_id,
                None => return unrenderable_response(),
            }),
        )
        .and_then(|result| {
            result.with_field(
                StableCode::new("temporary_password").unwrap(),
                TypedValue::Token(OpaqueToken::new(&temporary_password)?),
            )
        })
    else {
        return unrenderable_response();
    };
    success(result, correlation_id)
}

fn success(result: TypedResult, correlation_id: String) -> Response {
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

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ACCOUNT_NAME_BYTES
        && !value.chars().any(char::is_control)
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
        body::{Body, to_bytes},
        http::{
            Method, Request, StatusCode,
            header::{ALLOW, CONTENT_TYPE, COOKIE, LOCATION, ORIGIN, SET_COOKIE},
        },
        routing::MethodRouter,
    };
    use tower::ServiceExt;
    use zeroize::Zeroizing;

    use super::{
        ACCOUNTS_CREATE_ROUTE, ACCOUNTS_RESET_PASSWORD_ROUTE, AccountCreateSubmission,
        AccountCredentialIssued, AccountPasswordResetSubmission, CREDENTIAL_ISSUANCE_STEP_UP_ROUTE,
        CredentialIssuanceCapability, CredentialIssuanceDeclaration, CredentialIssuanceRejection,
        CredentialIssuanceStepUpSubmission, CredentialIssuanceTicketIssued,
        MAX_CREDENTIAL_ISSUANCE_PASSWORD_BYTES,
    };
    use crate::{ExpectedOrigin, cookie::CookieEffect, typed_json::TypedJsonEnvelope};

    const LISTENER: &str = "127.0.0.1:8443";
    const CORRELATION: &str = "credential-issuance-correlation";
    const TICKET: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const PUBLIC_ID: &str = "MTExMTExMTExMTExMTExMQ";
    const TEMPORARY_PASSWORD: &str = "temporary-password-1234";

    #[derive(Default)]
    struct Recorder {
        step_up: AtomicUsize,
        create: AtomicUsize,
        reset: AtomicUsize,
    }

    struct Harness {
        recorder: Arc<Recorder>,
        router: Router,
    }

    fn expected_origin() -> ExpectedOrigin {
        ExpectedOrigin::from_listener(LISTENER.parse().expect("the listener authority parses"))
    }

    fn harness(
        step_up: Result<&'static str, CredentialIssuanceRejection>,
        create: Result<(&'static str, &'static str), CredentialIssuanceRejection>,
        reset: Result<(&'static str, &'static str), CredentialIssuanceRejection>,
    ) -> Harness {
        let recorder = Arc::new(Recorder::default());
        let step_up_recorder = Arc::clone(&recorder);
        let create_recorder = Arc::clone(&recorder);
        let reset_recorder = Arc::clone(&recorder);
        let declaration = CredentialIssuanceDeclaration::new(CredentialIssuanceCapability {
            expected_origin: expected_origin(),
            correlate: Arc::new(|| Some(CORRELATION.to_owned())),
            step_up: Arc::new(move |submission: CredentialIssuanceStepUpSubmission| {
                let recorder = Arc::clone(&step_up_recorder);
                Box::pin(async move {
                    recorder.step_up.fetch_add(1, Ordering::Relaxed);
                    drop(submission);
                    step_up.map(|ticket| CredentialIssuanceTicketIssued {
                        credential_issuance_ticket: Zeroizing::new(ticket.to_owned()),
                    })
                })
            }),
            create: Arc::new(move |submission: AccountCreateSubmission| {
                let recorder = Arc::clone(&create_recorder);
                Box::pin(async move {
                    recorder.create.fetch_add(1, Ordering::Relaxed);
                    drop(submission);
                    create.map(|(public_id, temporary_password)| AccountCredentialIssued {
                        public_id: public_id.to_owned(),
                        temporary_password: Zeroizing::new(temporary_password.to_owned()),
                    })
                })
            }),
            reset_password: Arc::new(move |submission: AccountPasswordResetSubmission| {
                let recorder = Arc::clone(&reset_recorder);
                Box::pin(async move {
                    recorder.reset.fetch_add(1, Ordering::Relaxed);
                    drop(submission);
                    reset.map(|(public_id, temporary_password)| AccountCredentialIssued {
                        public_id: public_id.to_owned(),
                        temporary_password: Zeroizing::new(temporary_password.to_owned()),
                    })
                })
            }),
        });

        Harness {
            recorder,
            router: routed(
                CREDENTIAL_ISSUANCE_STEP_UP_ROUTE,
                declaration.step_up_route(),
            )
            .merge(routed(ACCOUNTS_CREATE_ROUTE, declaration.create_route()))
            .merge(routed(
                ACCOUNTS_RESET_PASSWORD_ROUTE,
                declaration.reset_password_route(),
            )),
        }
    }

    fn routed(path: &str, route: MethodRouter) -> Router {
        Router::new().route(path, route)
    }

    fn request(method: Method, target: &str, body: impl Into<Body>) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(target)
            .header("host", LISTENER)
            .header(ORIGIN, format!("https://{LISTENER}"))
            .header("x-weavelit-csrf", "csrf-token-value")
            .header(COOKIE, "__Host-weavelit_session=session-token-value")
            .header(CONTENT_TYPE, "application/json")
            .header("accept", "application/json")
            .body(body.into())
            .expect("the credential issuance request must build")
    }

    fn envelope(response: &axum::response::Response) -> String {
        response
            .extensions()
            .get::<TypedJsonEnvelope>()
            .map_or_else(String::new, |envelope| envelope.serialize().to_string())
    }

    fn assert_no_secret_transport(response: &axum::response::Response) {
        assert!(!response.headers().contains_key(SET_COOKIE));
        assert!(!response.headers().contains_key(LOCATION));
        assert!(response.extensions().get::<CookieEffect>().is_none());
    }

    fn assert_not_called(recorder: &Recorder) {
        assert_eq!(recorder.step_up.load(Ordering::Relaxed), 0);
        assert_eq!(recorder.create.load(Ordering::Relaxed), 0);
        assert_eq!(recorder.reset.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn credential_issuance_routes_return_only_their_one_time_secret_results() {
        let harness = harness(
            Ok(TICKET),
            Ok((PUBLIC_ID, TEMPORARY_PASSWORD)),
            Ok((PUBLIC_ID, TEMPORARY_PASSWORD)),
        );
        let cases = [
            (
                CREDENTIAL_ISSUANCE_STEP_UP_ROUTE,
                "{\"password\":\"current-password\",\"totp_code\":\"123456\"}".to_owned(),
                format!(
                    "{{\"result\":{{\"credential_issuance_ticket\":\"{TICKET}\"}},\
                     \"correlation_id\":\"{CORRELATION}\"}}"
                ),
            ),
            (
                ACCOUNTS_CREATE_ROUTE,
                format!(
                    "{{\"username\":\"new-user\",\"display_name\":\"New User\",\
                     \"credential_issuance_ticket\":\"{TICKET}\"}}"
                ),
                format!(
                    "{{\"result\":{{\"public_id\":\"{PUBLIC_ID}\",\
                     \"temporary_password\":\"{TEMPORARY_PASSWORD}\"}},\
                     \"correlation_id\":\"{CORRELATION}\"}}"
                ),
            ),
            (
                ACCOUNTS_RESET_PASSWORD_ROUTE,
                format!(
                    "{{\"public_id\":\"{PUBLIC_ID}\",\
                     \"credential_issuance_ticket\":\"{TICKET}\"}}"
                ),
                format!(
                    "{{\"result\":{{\"public_id\":\"{PUBLIC_ID}\",\
                     \"temporary_password\":\"{TEMPORARY_PASSWORD}\"}},\
                     \"correlation_id\":\"{CORRELATION}\"}}"
                ),
            ),
        ];

        for (target, body, expected) in cases {
            let response = harness
                .router
                .clone()
                .oneshot(request(Method::PUT, target, body))
                .await
                .expect("the credential issuance route must answer");

            assert_eq!(response.status(), StatusCode::OK, "{target}");
            assert_no_secret_transport(&response);
            assert!(
                crate::typed_json::has_secret_disclosure_effect(&response),
                "{target}"
            );
            assert_eq!(envelope(&response), expected, "{target}");
        }
        assert_eq!(harness.recorder.step_up.load(Ordering::Relaxed), 1);
        assert_eq!(harness.recorder.create.load(Ordering::Relaxed), 1);
        assert_eq!(harness.recorder.reset.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn credential_issuance_parsers_reject_non_strict_or_malformed_bodies() {
        let cases = vec![
            (CREDENTIAL_ISSUANCE_STEP_UP_ROUTE, "{}".to_owned()),
            (
                CREDENTIAL_ISSUANCE_STEP_UP_ROUTE,
                "{\"password\":\"one\",\"password\":\"two\"}".to_owned(),
            ),
            (
                CREDENTIAL_ISSUANCE_STEP_UP_ROUTE,
                "{\"password\":\"value\",\"totp_code\":\"12345\"}".to_owned(),
            ),
            (
                CREDENTIAL_ISSUANCE_STEP_UP_ROUTE,
                "{\"password\":\"value\",\"unknown\":true}".to_owned(),
            ),
            (
                CREDENTIAL_ISSUANCE_STEP_UP_ROUTE,
                format!(
                    "{{\"password\":\"{}\"}}",
                    "p".repeat(MAX_CREDENTIAL_ISSUANCE_PASSWORD_BYTES + 1)
                ),
            ),
            (
                ACCOUNTS_CREATE_ROUTE,
                format!(
                    "{{\"username\":\"new-user\",\"extra\":1,\"credential_issuance_ticket\":\"{TICKET}\"}}"
                ),
            ),
            (
                ACCOUNTS_CREATE_ROUTE,
                format!(
                    "{{\"username\":\"first\",\"username\":\"second\",\"credential_issuance_ticket\":\"{TICKET}\"}}"
                ),
            ),
            (
                ACCOUNTS_CREATE_ROUTE,
                "{\"username\":\"new-user\",\"credential_issuance_ticket\":\"short\"}".to_owned(),
            ),
            (
                ACCOUNTS_CREATE_ROUTE,
                format!(
                    "{{\"username\":\"new-user\",\"credential_issuance_ticket\":\"{TICKET}\"}} trailing"
                ),
            ),
            (
                ACCOUNTS_RESET_PASSWORD_ROUTE,
                format!("{{\"credential_issuance_ticket\":\"{TICKET}\"}}"),
            ),
            (
                ACCOUNTS_RESET_PASSWORD_ROUTE,
                format!(
                    "{{\"public_id\":\"AAAAAAAAAAAAAAAAAAAAAA\",\"credential_issuance_ticket\":\"{TICKET}\"}}"
                ),
            ),
            (
                ACCOUNTS_RESET_PASSWORD_ROUTE,
                format!(
                    "{{\"public_id\":\"{PUBLIC_ID}\",\"credential_issuance_ticket\":\"{TICKET}\",\"unknown\":true}}"
                ),
            ),
        ];

        for (target, body) in cases {
            let harness = harness(
                Ok(TICKET),
                Ok((PUBLIC_ID, TEMPORARY_PASSWORD)),
                Ok((PUBLIC_ID, TEMPORARY_PASSWORD)),
            );
            let response = harness
                .router
                .oneshot(request(Method::PUT, target, body))
                .await
                .expect("the credential issuance route must answer");

            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{target}");
            assert_eq!(
                envelope(&response),
                format!("{{\"error\":\"bad_request\",\"correlation_id\":\"{CORRELATION}\"}}"),
                "{target}"
            );
            assert_no_secret_transport(&response);
            assert_not_called(&harness.recorder);
        }
    }

    #[tokio::test]
    async fn credential_issuance_preconditions_enforce_method_origin_session_and_headers() {
        let valid_body = "{\"password\":\"current-password\"}";
        let harness = harness(
            Ok(TICKET),
            Ok((PUBLIC_ID, TEMPORARY_PASSWORD)),
            Ok((PUBLIC_ID, TEMPORARY_PASSWORD)),
        );
        let method = harness
            .router
            .clone()
            .oneshot(request(
                Method::GET,
                CREDENTIAL_ISSUANCE_STEP_UP_ROUTE,
                valid_body,
            ))
            .await
            .unwrap();
        assert_eq!(method.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(method.headers().get(ALLOW).unwrap(), "PUT");

        let mut wrong_origin = request(Method::PUT, CREDENTIAL_ISSUANCE_STEP_UP_ROUTE, valid_body);
        wrong_origin
            .headers_mut()
            .insert(ORIGIN, "https://127.0.0.1:9443".parse().unwrap());
        let wrong_origin = harness.router.clone().oneshot(wrong_origin).await.unwrap();
        assert_eq!(wrong_origin.status(), StatusCode::FORBIDDEN);

        let mut no_session = request(Method::PUT, CREDENTIAL_ISSUANCE_STEP_UP_ROUTE, valid_body);
        no_session.headers_mut().remove(COOKIE);
        let no_session = harness.router.clone().oneshot(no_session).await.unwrap();
        assert_eq!(no_session.status(), StatusCode::UNAUTHORIZED);

        let mut no_csrf = request(Method::PUT, CREDENTIAL_ISSUANCE_STEP_UP_ROUTE, valid_body);
        no_csrf.headers_mut().remove("x-weavelit-csrf");
        let no_csrf = harness.router.clone().oneshot(no_csrf).await.unwrap();
        assert_eq!(no_csrf.status(), StatusCode::UNAUTHORIZED);

        let mut no_content_type =
            request(Method::PUT, CREDENTIAL_ISSUANCE_STEP_UP_ROUTE, valid_body);
        no_content_type.headers_mut().remove(CONTENT_TYPE);
        let no_content_type = harness
            .router
            .clone()
            .oneshot(no_content_type)
            .await
            .unwrap();
        assert_eq!(no_content_type.status(), StatusCode::BAD_REQUEST);

        for response in [method, wrong_origin, no_session, no_csrf, no_content_type] {
            assert_no_secret_transport(&response);
        }
        assert_not_called(&harness.recorder);
    }

    #[tokio::test]
    async fn credential_issuance_refusals_never_echo_submitted_secrets() {
        let password = "submitted-current-password";
        let totp = "654321";
        let submitted_ticket = TICKET;
        let cases = [
            (
                CREDENTIAL_ISSUANCE_STEP_UP_ROUTE,
                format!("{{\"password\":\"{password}\",\"totp_code\":\"{totp}\"}}"),
                CredentialIssuanceRejection::CredentialDenied,
            ),
            (
                ACCOUNTS_CREATE_ROUTE,
                format!(
                    "{{\"username\":\"new-user\",\"credential_issuance_ticket\":\"{submitted_ticket}\"}}"
                ),
                CredentialIssuanceRejection::Conflict,
            ),
            (
                ACCOUNTS_RESET_PASSWORD_ROUTE,
                format!(
                    "{{\"public_id\":\"{PUBLIC_ID}\",\"credential_issuance_ticket\":\"{submitted_ticket}\"}}"
                ),
                CredentialIssuanceRejection::NotFound,
            ),
        ];

        for (target, body, rejection) in cases {
            let harness = harness(Err(rejection), Err(rejection), Err(rejection));
            let response = harness
                .router
                .oneshot(request(Method::PUT, target, body))
                .await
                .unwrap();
            let rendered = envelope(&response);

            assert_eq!(response.status(), rejection.status(), "{target}");
            assert!(!rendered.contains(password));
            assert!(!rendered.contains(totp));
            assert!(!rendered.contains(submitted_ticket));
            assert_no_secret_transport(&response);
        }
    }

    #[tokio::test]
    async fn credential_issuance_refuses_unrenderable_ticket_and_temporary_secret() {
        let invalid_ticket = "ticket contains a secret but is not opaque";
        let invalid_temporary_password = "temporary password with spaces";
        let cases = [
            (
                CREDENTIAL_ISSUANCE_STEP_UP_ROUTE,
                "{\"password\":\"current-password\"}".to_owned(),
                harness(
                    Ok(invalid_ticket),
                    Ok((PUBLIC_ID, TEMPORARY_PASSWORD)),
                    Ok((PUBLIC_ID, TEMPORARY_PASSWORD)),
                ),
                invalid_ticket,
            ),
            (
                ACCOUNTS_CREATE_ROUTE,
                format!(
                    "{{\"username\":\"new-user\",\"credential_issuance_ticket\":\"{TICKET}\"}}"
                ),
                harness(
                    Ok(TICKET),
                    Ok((PUBLIC_ID, invalid_temporary_password)),
                    Ok((PUBLIC_ID, TEMPORARY_PASSWORD)),
                ),
                invalid_temporary_password,
            ),
            (
                ACCOUNTS_RESET_PASSWORD_ROUTE,
                format!(
                    "{{\"public_id\":\"{PUBLIC_ID}\",\"credential_issuance_ticket\":\"{TICKET}\"}}"
                ),
                harness(
                    Ok(TICKET),
                    Ok((PUBLIC_ID, TEMPORARY_PASSWORD)),
                    Ok((PUBLIC_ID, invalid_temporary_password)),
                ),
                invalid_temporary_password,
            ),
        ];

        for (target, body, harness, forbidden) in cases {
            let response = harness
                .router
                .oneshot(request(Method::PUT, target, body))
                .await
                .unwrap();

            assert_eq!(
                response.status(),
                StatusCode::SERVICE_UNAVAILABLE,
                "{target}"
            );
            assert!(response.extensions().get::<TypedJsonEnvelope>().is_none());
            assert_no_secret_transport(&response);
            let body = to_bytes(response.into_body(), 128).await.unwrap();
            assert_eq!(body.as_ref(), b"{\"error\":\"service_unavailable\"}");
            assert!(!String::from_utf8_lossy(&body).contains(forbidden));
        }
    }
}
