//! Shared Client Module contract for restricted-session password replacement.

use std::{fmt, pin::Pin, sync::Arc};

use axum::{
    body::to_bytes,
    extract::Request,
    http::{HeaderMap, Method, header::CONTENT_TYPE},
    response::Response,
    routing::{MethodRouter, any},
};
use serde::de::{self, Deserialize, Deserializer, MapAccess, Visitor};
use zeroize::Zeroizing;

use crate::{
    ExpectedOrigin, JSON_MEDIA_TYPE, WipedBody, accepts_json,
    authentication::{
        AuthenticationRejection, CorrelationSource, SessionEstablished,
        session_established_response, submitted_csrf_token, submitted_session_token,
        unrenderable_response,
    },
    single_header,
};

/// The canonical route that replaces a temporary password.
pub const AUTH_PASSWORD_CHANGE_ROUTE: &str = "/api/v1/auth/password/change";

/// Largest decoded replacement password accepted by the version 1 contract.
pub const MAX_PASSWORD_CHANGE_PASSWORD_BYTES: usize = 1024;

/// Largest JSON body accepted by the password-change route.
///
/// This admits the maximum password even when every decoded byte uses JSON's
/// longest six-byte escape, together with the one documented field wrapper.
pub const MAX_PASSWORD_CHANGE_BODY_BYTES: usize = (MAX_PASSWORD_CHANGE_PASSWORD_BYTES * 6) + 32;

/// A validated password-change request handed to the Server core.
pub struct PasswordChangeSubmission {
    /// The session bearer value read from the session cookie.
    pub session_token: Zeroizing<String>,
    /// The cross-site request forgery token echoed in the request header.
    pub csrf_token: Zeroizing<String>,
    /// The replacement password, still unhashed.
    pub password: Zeroizing<String>,
    /// The Server-generated correlation identifier for this request.
    pub correlation_id: String,
    /// The admitted request's extensions.
    pub context: axum::http::Extensions,
}

/// Server-core hook that commits one restricted-session password change.
pub type PasswordChangeCommit = Arc<
    dyn Fn(
            PasswordChangeSubmission,
        ) -> Pin<
            Box<dyn Future<Output = Result<SessionEstablished, AuthenticationRejection>> + Send>,
        > + Send
        + Sync,
>;

/// Runtime collaborators required to declare password change.
pub struct PasswordChangeCapability {
    /// The trusted authority every request must target.
    pub expected_origin: ExpectedOrigin,
    /// The Server-owned correlation identifier source.
    pub correlate: CorrelationSource,
    /// The hook that commits the change and establishes a fresh session.
    pub change: PasswordChangeCommit,
}

/// A declared password-change capability.
pub struct PasswordChangeDeclaration {
    capability: Arc<PasswordChangeCapability>,
}

impl PasswordChangeDeclaration {
    /// Declares password change over the supplied runtime collaborators.
    #[must_use]
    pub fn new(capability: PasswordChangeCapability) -> Self {
        Self {
            capability: Arc::new(capability),
        }
    }

    /// Returns the route mounted at [`AUTH_PASSWORD_CHANGE_ROUTE`].
    pub fn route(&self) -> MethodRouter {
        let capability = Arc::clone(&self.capability);
        any(move |request| password_change_response(request, Arc::clone(&capability)))
    }
}

/// Validates every header, cookie, and media precondition before body parsing.
pub fn validate_password_change_request(
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
    submitted_csrf_token(headers)?;
    submitted_session_token(headers)?;
    Ok(())
}

struct PasswordChangeBody {
    password: Zeroizing<String>,
}

const PASSWORD_CHANGE_FIELDS: &[&str] = &["password"];

impl<'de> Deserialize<'de> for PasswordChangeBody {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BodyVisitor;

        impl<'de> Visitor<'de> for BodyVisitor {
            type Value = PasswordChangeBody;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a password-change submission object")
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
                            return Err(de::Error::unknown_field(unknown, PASSWORD_CHANGE_FIELDS));
                        }
                    }
                }
                let password = password.ok_or_else(|| de::Error::missing_field("password"))?;
                if password.is_empty()
                    || password.as_bytes().len() > MAX_PASSWORD_CHANGE_PASSWORD_BYTES
                {
                    return Err(de::Error::custom("password is outside the accepted bound"));
                }
                Ok(PasswordChangeBody { password })
            }
        }

        deserializer.deserialize_map(BodyVisitor)
    }
}

fn parse_password_change_body_wiped<B: AsMut<[u8]>>(
    buffer: B,
) -> Result<PasswordChangeBody, AuthenticationRejection> {
    let mut body = WipedBody::new(buffer);
    parse_password_change_body(body.bytes())
}

fn parse_password_change_body(body: &[u8]) -> Result<PasswordChangeBody, AuthenticationRejection> {
    if body.len() > MAX_PASSWORD_CHANGE_BODY_BYTES {
        return Err(AuthenticationRejection::BadRequest);
    }
    let mut deserializer = serde_json::Deserializer::from_slice(body);
    let parsed = PasswordChangeBody::deserialize(&mut deserializer)
        .map_err(|_| AuthenticationRejection::BadRequest)?;
    deserializer
        .end()
        .map_err(|_| AuthenticationRejection::BadRequest)?;
    Ok(parsed)
}

async fn password_change_response(
    request: Request,
    capability: Arc<PasswordChangeCapability>,
) -> Response {
    let Some(correlation_id) = (capability.correlate)() else {
        return unrenderable_response();
    };
    let (parts, body) = request.into_parts();
    if let Err(rejection) =
        validate_password_change_request(&parts.method, &parts.headers, capability.expected_origin)
    {
        return rejection.response(&correlation_id);
    }
    let session_token = Zeroizing::new(
        submitted_session_token(&parts.headers)
            .expect("validated session token must remain available")
            .to_owned(),
    );
    let csrf_token = Zeroizing::new(
        submitted_csrf_token(&parts.headers)
            .expect("validated CSRF token must remain available")
            .to_owned(),
    );
    let Ok(body) = to_bytes(body, MAX_PASSWORD_CHANGE_BODY_BYTES).await else {
        return AuthenticationRejection::BadRequest.response(&correlation_id);
    };
    let parsed = match body.try_into_mut() {
        Ok(unique) => parse_password_change_body_wiped(unique),
        Err(shared) => parse_password_change_body_wiped(shared.to_vec()),
    };
    let parsed = match parsed {
        Ok(parsed) => parsed,
        Err(rejection) => return rejection.response(&correlation_id),
    };

    match (capability.change)(PasswordChangeSubmission {
        session_token,
        csrf_token,
        password: parsed.password,
        correlation_id: correlation_id.clone(),
        context: parts.extensions,
    })
    .await
    {
        Ok(established) => session_established_response(&established, &correlation_id),
        Err(rejection) => rejection.response(&correlation_id),
    }
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
        http::{Request, StatusCode, header::ALLOW},
    };
    use tower::ServiceExt;

    use super::*;
    use crate::{
        cookie::CookieEffect, typed_json::TypedJsonEnvelope, wiped_body_support::parse_and_observe,
    };

    const LISTENER: &str = "127.0.0.1:8443";
    const PASSWORD: &str = "new ordinary password";

    fn expected_origin() -> ExpectedOrigin {
        ExpectedOrigin::from_listener(LISTENER.parse().unwrap())
    }

    fn request(body: impl Into<Body>) -> Request<Body> {
        Request::put(AUTH_PASSWORD_CHANGE_ROUTE)
            .header("host", LISTENER)
            .header("origin", format!("https://{LISTENER}"))
            .header("accept", "application/json")
            .header("content-type", "application/json")
            .header("x-weavelit-csrf", "csrf-token-value")
            .header("cookie", "__Host-weavelit_session=session-token-value")
            .body(body.into())
            .unwrap()
    }

    fn router(
        result: Result<SessionEstablished, AuthenticationRejection>,
        calls: Arc<AtomicUsize>,
    ) -> Router {
        let declaration = PasswordChangeDeclaration::new(PasswordChangeCapability {
            expected_origin: expected_origin(),
            correlate: Arc::new(|| Some("correlation-0123456789".to_owned())),
            change: Arc::new(move |submission| {
                let calls = Arc::clone(&calls);
                let result = result.as_ref().map_or_else(
                    |rejection| Err(*rejection),
                    |session| {
                        Ok(SessionEstablished {
                            session_token: Zeroizing::new(session.session_token.to_string()),
                            csrf_token: Zeroizing::new(session.csrf_token.to_string()),
                        })
                    },
                );
                Box::pin(async move {
                    calls.fetch_add(1, Ordering::Relaxed);
                    drop(submission);
                    result
                })
            }),
        });
        Router::new().route(AUTH_PASSWORD_CHANGE_ROUTE, declaration.route())
    }

    fn established() -> SessionEstablished {
        SessionEstablished {
            session_token: Zeroizing::new("fresh-session-token".to_owned()),
            csrf_token: Zeroizing::new("fresh-csrf-token".to_owned()),
        }
    }

    fn envelope(response: &Response) -> String {
        response
            .extensions()
            .get::<TypedJsonEnvelope>()
            .unwrap()
            .serialize()
            .to_string()
    }

    #[tokio::test]
    async fn a_changed_password_establishes_only_the_fresh_session_cookie_effect() {
        let calls = Arc::new(AtomicUsize::new(0));
        let response = router(Ok(established()), Arc::clone(&calls))
            .oneshot(request(format!("{{\"password\":\"{PASSWORD}\"}}")))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            envelope(&response),
            "{\"result\":{\"authenticated\":true},\"correlation_id\":\"correlation-0123456789\"}"
        );
        let cookies = response
            .extensions()
            .get::<CookieEffect>()
            .and_then(CookieEffect::render)
            .unwrap();
        assert!(cookies.as_str().contains("fresh-session-token"));
        assert!(cookies.as_str().contains("fresh-csrf-token"));
        assert!(!envelope(&response).contains(PASSWORD));
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn every_core_refusal_is_payload_free_and_sets_no_cookie() {
        for rejection in [
            AuthenticationRejection::SessionInvalid,
            AuthenticationRejection::ServiceUnavailable,
        ] {
            let response = router(Err(rejection), Arc::new(AtomicUsize::new(0)))
                .oneshot(request(format!("{{\"password\":\"{PASSWORD}\"}}")))
                .await
                .unwrap();

            assert_eq!(response.status(), rejection.status());
            assert_eq!(
                envelope(&response),
                format!(
                    "{{\"error\":\"{}\",\"correlation_id\":\"correlation-0123456789\"}}",
                    rejection.code()
                )
            );
            assert!(response.extensions().get::<CookieEffect>().is_none());
            assert!(!envelope(&response).contains(PASSWORD));
        }
    }

    #[tokio::test]
    async fn invalid_head_or_method_never_reaches_the_core() {
        let calls = Arc::new(AtomicUsize::new(0));
        let surface = router(Ok(established()), Arc::clone(&calls));
        let wrong_origin = Request::put(AUTH_PASSWORD_CHANGE_ROUTE)
            .header("host", LISTENER)
            .header("origin", "https://127.0.0.1:9443")
            .header("accept", "application/json")
            .header("content-type", "application/json")
            .header("x-weavelit-csrf", "csrf-token-value")
            .header("cookie", "__Host-weavelit_session=session-token-value")
            .body(Body::from(format!("{{\"password\":\"{PASSWORD}\"}}")))
            .unwrap();
        let denied = surface.clone().oneshot(wrong_origin).await.unwrap();
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);

        let missing_session = Request::put(AUTH_PASSWORD_CHANGE_ROUTE)
            .header("host", LISTENER)
            .header("origin", format!("https://{LISTENER}"))
            .header("accept", "application/json")
            .header("content-type", "application/json")
            .header("x-weavelit-csrf", "csrf-token-value")
            .body(Body::from(format!("{{\"password\":\"{PASSWORD}\"}}")))
            .unwrap();
        let invalid = surface.clone().oneshot(missing_session).await.unwrap();
        assert_eq!(invalid.status(), StatusCode::UNAUTHORIZED);

        let method = surface
            .oneshot(
                Request::get(AUTH_PASSWORD_CHANGE_ROUTE)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(method.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(method.headers().get(ALLOW).unwrap(), "PUT");
        assert_eq!(calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn the_body_accepts_exactly_one_bounded_replacement_password() {
        assert_eq!(
            parse_password_change_body(format!("{{\"password\":\"{PASSWORD}\"}}").as_bytes())
                .unwrap()
                .password
                .as_str(),
            PASSWORD
        );
        assert!(
            parse_password_change_body(
                format!("{{\"password\":\"{}\"}}", "a".repeat(1024)).as_bytes()
            )
            .is_ok()
        );

        for rejected in [
            "".to_owned(),
            "[]".to_owned(),
            "{}".to_owned(),
            "{\"password\":\"\"}".to_owned(),
            "{\"password\":1}".to_owned(),
            "{\"password\":\"new\",\"password\":\"other\"}".to_owned(),
            "{\"password\":\"new\",\"account_id\":\"target\"}".to_owned(),
            "{\"password\":\"new\",\"old_password\":\"old\"}".to_owned(),
            "{\"password\":\"new\",\"temporary_password\":\"old\"}".to_owned(),
            "{\"password\":\"new\",\"session\":\"value\"}".to_owned(),
            format!("{{\"password\":\"{}\"}}", "a".repeat(1025)),
        ] {
            assert!(parse_password_change_body(rejected.as_bytes()).is_err());
        }
    }

    #[test]
    fn the_body_buffer_is_cleared_on_success_and_rejection() {
        for body in [
            format!("{{\"password\":\"{PASSWORD}\"}}"),
            format!("{{\"password\":\"{PASSWORD}\",\"extra\":true}}"),
        ] {
            let (_, released) = parse_and_observe(&body, parse_password_change_body_wiped);
            assert_eq!(released, vec![0; released.len()]);
            assert!(!released.is_empty());
        }
    }
}
