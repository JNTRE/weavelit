//! Shared Client Module contract for submission-bound lifecycle reconciliation.
//!
//! A completed Init or Restore delivers an opaque browser-held capability. This
//! route accepts that value only in its exact JSON body, delegates the lookup
//! to Server core, and renders fixed confirmation or refusal responses.

use std::fmt;
use std::pin::Pin;
use std::sync::Arc;

use axum::{
    body::to_bytes,
    extract::Request,
    http::{HeaderMap, HeaderValue, Method, StatusCode, header::ALLOW},
    response::Response,
    routing::{MethodRouter, any},
};
use serde::de::{self, Deserialize, Deserializer, MapAccess, Visitor};
use zeroize::Zeroizing;

use crate::{
    ExpectedOrigin, JSON_MEDIA_TYPE, WipedBody, accepts_json, json_response_body, single_header,
    typed_json::MAX_OPAQUE_TOKEN_BYTES,
};

/// The canonical route that confirms a completed Init or Restore submission.
pub const LIFECYCLE_RECONCILIATION_ROUTE: &str = "/api/v1/lifecycle/reconciliation";

/// Largest request body accepted by the reconciliation route.
pub const MAX_RECONCILIATION_BODY_BYTES: usize = 128;

/// The only accepted request member.
const RECONCILIATION_CAPABILITY_FIELD: &str = "reconciliation_capability";

/// A validated browser-held capability passed to Server core.
pub struct ReconciliationSubmission {
    /// The submitted capability, cleared when Server core finishes with it.
    pub capability: Zeroizing<String>,
}

impl fmt::Debug for ReconciliationSubmission {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ReconciliationSubmission(REDACTED)")
    }
}

/// The fixed outcomes Server core may report for one submitted capability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReconciliationOutcome {
    /// The capability proves the completed submission.
    Confirmed,
    /// The capability does not prove this deployment's completed submission.
    NotFound,
    /// The reconciliation store could not be read safely.
    Unavailable,
}

/// Server-core hook that checks a validated reconciliation capability.
pub type ReconciliationCommit = Arc<
    dyn Fn(ReconciliationSubmission) -> Pin<Box<dyn Future<Output = ReconciliationOutcome> + Send>>
        + Send
        + Sync,
>;

/// The collaborators needed to declare the reconciliation route.
pub struct ReconciliationCapability {
    /// The trusted authority every reconciliation request must target.
    pub expected_origin: ExpectedOrigin,
    /// The Server-core reconciliation lookup.
    pub reconcile: ReconciliationCommit,
}

impl ReconciliationCapability {
    /// Returns the reconciliation route at its canonical path.
    pub fn route(self) -> MethodRouter {
        let capability = Arc::new(self);
        any(move |request| reconciliation_response(request, Arc::clone(&capability)))
    }
}

/// The complete payload-free rejection contract for reconciliation requests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReconciliationRejection {
    /// `400` for a malformed body, media type, or `Accept` value.
    BadRequest,
    /// `403` for a failed same-origin, `Host`, or CSRF header check.
    RequestOriginDenied,
    /// `405` for any method other than `PUT`.
    MethodNotAllowed,
}

impl ReconciliationRejection {
    /// Every route-head rejection this contract can render.
    pub const ALL: &'static [Self] = &[
        Self::BadRequest,
        Self::RequestOriginDenied,
        Self::MethodNotAllowed,
    ];

    /// Returns the documented status code.
    #[must_use]
    pub const fn status(self) -> StatusCode {
        match self {
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::RequestOriginDenied => StatusCode::FORBIDDEN,
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
        }
    }

    /// Returns the documented fixed JSON body.
    #[must_use]
    pub const fn body(self) -> &'static str {
        match self {
            Self::BadRequest => "{\"error\":\"bad_request\"}",
            Self::RequestOriginDenied => "{\"error\":\"request_origin_denied\"}",
            Self::MethodNotAllowed => "{\"error\":\"method_not_allowed\"}",
        }
    }

    /// Builds the fixed response, including `Allow: PUT` for `405`.
    #[must_use]
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

/// Validates every header precondition of a reconciliation submission.
///
/// The same-origin and CSRF trust check runs before media-type validation so a
/// cross-site request is denied without revealing body or negotiation detail.
pub fn validate_reconciliation_request(
    method: &Method,
    headers: &HeaderMap,
    expected_origin: ExpectedOrigin,
) -> Result<(), ReconciliationRejection> {
    if method != Method::PUT {
        return Err(ReconciliationRejection::MethodNotAllowed);
    }
    if !expected_origin.is_trusted(headers) {
        return Err(ReconciliationRejection::RequestOriginDenied);
    }
    let content_type = single_header(headers, axum::http::header::CONTENT_TYPE)
        .ok_or(ReconciliationRejection::BadRequest)?;
    if content_type.as_bytes() != JSON_MEDIA_TYPE || !accepts_json(headers) {
        return Err(ReconciliationRejection::BadRequest);
    }
    Ok(())
}

/// Parses the exact reconciliation request object.
struct ReconciliationBody {
    capability: Zeroizing<String>,
}

impl<'de> Deserialize<'de> for ReconciliationBody {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BodyVisitor;

        impl<'de> Visitor<'de> for BodyVisitor {
            type Value = ReconciliationBody;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a lifecycle reconciliation submission object")
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut capability: Option<Zeroizing<String>> = None;
                while let Some(field) = map.next_key::<String>()? {
                    if field != RECONCILIATION_CAPABILITY_FIELD {
                        return Err(de::Error::unknown_field(
                            &field,
                            &[RECONCILIATION_CAPABILITY_FIELD],
                        ));
                    }
                    if capability.is_some() {
                        return Err(de::Error::duplicate_field(RECONCILIATION_CAPABILITY_FIELD));
                    }
                    capability = Some(Zeroizing::new(map.next_value()?));
                }
                Ok(ReconciliationBody {
                    capability: capability
                        .ok_or_else(|| de::Error::missing_field(RECONCILIATION_CAPABILITY_FIELD))?,
                })
            }
        }

        deserializer.deserialize_map(BodyVisitor)
    }
}

/// Parses a reconciliation body through a buffer that clears on release.
fn parse_reconciliation_body_wiped<B: AsMut<[u8]>>(
    buffer: B,
) -> Result<Zeroizing<String>, ReconciliationRejection> {
    let mut body = WipedBody::new(buffer);
    parse_reconciliation_body(body.bytes())
}

/// Parses the exact accepted request body.
fn parse_reconciliation_body(body: &[u8]) -> Result<Zeroizing<String>, ReconciliationRejection> {
    if body.len() > MAX_RECONCILIATION_BODY_BYTES {
        return Err(ReconciliationRejection::BadRequest);
    }
    let mut deserializer = serde_json::Deserializer::from_slice(body);
    let parsed = ReconciliationBody::deserialize(&mut deserializer)
        .map_err(|_| ReconciliationRejection::BadRequest)?;
    deserializer
        .end()
        .map_err(|_| ReconciliationRejection::BadRequest)?;
    if !is_opaque_capability(&parsed.capability) {
        return Err(ReconciliationRejection::BadRequest);
    }
    Ok(parsed.capability)
}

fn is_opaque_capability(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_OPAQUE_TOKEN_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

async fn reconciliation_response(
    request: Request,
    capability: Arc<ReconciliationCapability>,
) -> Response {
    let (parts, body) = request.into_parts();
    if let Err(rejection) =
        validate_reconciliation_request(&parts.method, &parts.headers, capability.expected_origin)
    {
        return rejection.response();
    }
    let Ok(body) = to_bytes(body, MAX_RECONCILIATION_BODY_BYTES).await else {
        return ReconciliationRejection::BadRequest.response();
    };
    // `Bytes` is shared and immutable, so the collected buffer can only be
    // wiped once this crate holds it uniquely. If a clone is outstanding, the
    // fallback wipes the copy this crate owns rather than rejecting a request.
    let parsed = match body.try_into_mut() {
        Ok(unique) => parse_reconciliation_body_wiped(unique),
        Err(shared) => parse_reconciliation_body_wiped(shared.to_vec()),
    };
    let submitted_capability = match parsed {
        Ok(capability) => capability,
        Err(rejection) => return rejection.response(),
    };

    reconciliation_outcome_response(
        (capability.reconcile)(ReconciliationSubmission {
            capability: submitted_capability,
        })
        .await,
    )
}

/// Renders one bounded lifecycle reconciliation outcome.
#[must_use]
pub fn reconciliation_outcome_response(outcome: ReconciliationOutcome) -> Response {
    match outcome {
        ReconciliationOutcome::Confirmed => {
            json_response_body(StatusCode::OK, "{\"result\":\"reconciliation_confirmed\"}")
        }
        ReconciliationOutcome::NotFound => {
            json_response_body(StatusCode::NOT_FOUND, "{\"error\":\"not_found\"}")
        }
        ReconciliationOutcome::Unavailable => json_response_body(
            StatusCode::SERVICE_UNAVAILABLE,
            "{\"error\":\"service_unavailable\"}",
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::{
        Router,
        body::{Body, to_bytes},
        extract::Request,
        http::{HeaderMap, HeaderValue, Method, StatusCode},
    };
    use tower::ServiceExt;

    use super::{
        LIFECYCLE_RECONCILIATION_ROUTE, MAX_RECONCILIATION_BODY_BYTES, ReconciliationCapability,
        ReconciliationOutcome, ReconciliationRejection, parse_reconciliation_body,
        parse_reconciliation_body_wiped, reconciliation_outcome_response,
        validate_reconciliation_request,
    };
    use crate::{CSRF_HEADER_NAME, ExpectedOrigin, wiped_body_support::parse_and_observe};

    const CAPABILITY: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghi";

    fn expected_origin() -> ExpectedOrigin {
        ExpectedOrigin::from_listener("127.0.0.1:8443".parse().unwrap())
    }

    fn trusted_headers(content_type: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CSRF_HEADER_NAME, HeaderValue::from_static("1"));
        headers.insert("origin", HeaderValue::from_static("https://127.0.0.1:8443"));
        headers.insert("host", HeaderValue::from_static("127.0.0.1:8443"));
        headers.insert("content-type", HeaderValue::from_str(content_type).unwrap());
        headers.insert("accept", HeaderValue::from_static("application/json"));
        headers
    }

    fn body(capability: &str) -> String {
        format!("{{\"reconciliation_capability\":\"{capability}\"}}")
    }

    fn harness(outcome: ReconciliationOutcome, submitted: Arc<Mutex<Vec<String>>>) -> Router {
        Router::new().route(
            LIFECYCLE_RECONCILIATION_ROUTE,
            ReconciliationCapability {
                expected_origin: expected_origin(),
                reconcile: Arc::new(move |submission| {
                    let submitted = Arc::clone(&submitted);
                    Box::pin(async move {
                        submitted
                            .lock()
                            .expect("the recorder must not be poisoned")
                            .push(submission.capability.as_str().to_owned());
                        outcome
                    })
                }),
            }
            .route(),
        )
    }

    fn request(body: String) -> Request<Body> {
        Request::put(LIFECYCLE_RECONCILIATION_ROUTE)
            .header("host", "127.0.0.1:8443")
            .header("origin", "https://127.0.0.1:8443")
            .header(CSRF_HEADER_NAME, "1")
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .body(Body::from(body))
            .expect("the reconciliation request must build")
    }

    #[test]
    fn the_head_requires_the_exact_method_origin_and_json_media_types() {
        assert_eq!(
            validate_reconciliation_request(
                &Method::PUT,
                &trusted_headers("application/json"),
                expected_origin(),
            ),
            Ok(())
        );
        assert_eq!(
            validate_reconciliation_request(
                &Method::POST,
                &trusted_headers("application/json"),
                expected_origin(),
            ),
            Err(ReconciliationRejection::MethodNotAllowed)
        );

        let mut untrusted = trusted_headers("application/json");
        untrusted.remove(CSRF_HEADER_NAME);
        assert_eq!(
            validate_reconciliation_request(&Method::PUT, &untrusted, expected_origin()),
            Err(ReconciliationRejection::RequestOriginDenied)
        );

        for media_type in [
            "application/json; charset=utf-8",
            "application/octet-stream",
        ] {
            assert_eq!(
                validate_reconciliation_request(
                    &Method::PUT,
                    &trusted_headers(media_type),
                    expected_origin(),
                ),
                Err(ReconciliationRejection::BadRequest),
                "{media_type}"
            );
        }

        let mut wrong_accept = trusted_headers("application/json");
        wrong_accept.insert(
            "accept",
            HeaderValue::from_static("application/json, text/plain"),
        );
        assert_eq!(
            validate_reconciliation_request(&Method::PUT, &wrong_accept, expected_origin()),
            Err(ReconciliationRejection::BadRequest)
        );
    }

    #[test]
    fn the_schema_accepts_only_one_bounded_opaque_capability_field() {
        assert_eq!(
            parse_reconciliation_body(body(CAPABILITY).as_bytes())
                .unwrap()
                .as_str(),
            CAPABILITY
        );
        for rejected in [
            String::new(),
            "{}".to_owned(),
            "[]".to_owned(),
            "{\"reconciliation_capability\":1}".to_owned(),
            "{\"reconciliation_capability\":\"\",\"extra\":1}".to_owned(),
            format!(
                "{{\"reconciliation_capability\":\"{CAPABILITY}\",\"reconciliation_capability\":\"{CAPABILITY}\"}}"
            ),
            format!("{} trailing", body(CAPABILITY)),
            body("has=padding"),
            body(&"a".repeat(49)),
        ] {
            assert_eq!(
                parse_reconciliation_body(rejected.as_bytes()),
                Err(ReconciliationRejection::BadRequest),
                "{rejected}"
            );
        }
        let oversized = format!(
            "{{\"reconciliation_capability\":\"{}\"}}",
            "a".repeat(MAX_RECONCILIATION_BODY_BYTES)
        );
        assert!(oversized.len() > MAX_RECONCILIATION_BODY_BYTES);
        assert_eq!(
            parse_reconciliation_body(oversized.as_bytes()),
            Err(ReconciliationRejection::BadRequest)
        );
    }

    #[test]
    fn the_reconciliation_body_is_cleared_on_parsed_and_rejected_paths() {
        let accepted = body(CAPABILITY);
        let (parsed, released) = parse_and_observe(&accepted, parse_reconciliation_body_wiped);
        assert_eq!(
            parsed.map(|value| value.as_str().to_owned()),
            Ok(CAPABILITY.to_owned())
        );
        assert_eq!(released, vec![0_u8; released.len()]);
        assert!(!released.is_empty());

        let rejected = format!("{accepted} trailing");
        let (parsed, released) = parse_and_observe(&rejected, parse_reconciliation_body_wiped);
        assert_eq!(parsed, Err(ReconciliationRejection::BadRequest));
        assert_eq!(released, vec![0_u8; released.len()]);
        assert!(!released.is_empty());
    }

    #[tokio::test]
    async fn the_route_only_calls_core_for_a_valid_submission_and_never_sets_cookie_or_cors_headers()
     {
        let submitted = Arc::new(Mutex::new(Vec::new()));
        let response = harness(ReconciliationOutcome::Confirmed, Arc::clone(&submitted))
            .oneshot(request(body(CAPABILITY)))
            .await
            .expect("the route must answer");
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get("set-cookie").is_none());
        assert!(
            !response
                .headers()
                .keys()
                .any(|name| name.as_str().starts_with("access-control-"))
        );
        assert_eq!(
            to_bytes(response.into_body(), 128).await.unwrap().as_ref(),
            b"{\"result\":\"reconciliation_confirmed\"}"
        );
        assert_eq!(
            submitted
                .lock()
                .expect("the recorder must not be poisoned")
                .as_slice(),
            [CAPABILITY.to_owned()]
        );

        let submitted = Arc::new(Mutex::new(Vec::new()));
        let response = harness(ReconciliationOutcome::Confirmed, Arc::clone(&submitted))
            .oneshot(request("{\"reconciliation_capability\":1}".to_owned()))
            .await
            .expect("the route must answer");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(
            submitted
                .lock()
                .expect("the recorder must not be poisoned")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn match_mismatch_and_unavailable_render_only_fixed_nonreflective_bodies() {
        for (outcome, status, expected) in [
            (
                ReconciliationOutcome::Confirmed,
                StatusCode::OK,
                "{\"result\":\"reconciliation_confirmed\"}",
            ),
            (
                ReconciliationOutcome::NotFound,
                StatusCode::NOT_FOUND,
                "{\"error\":\"not_found\"}",
            ),
            (
                ReconciliationOutcome::Unavailable,
                StatusCode::SERVICE_UNAVAILABLE,
                "{\"error\":\"service_unavailable\"}",
            ),
        ] {
            let response = reconciliation_outcome_response(outcome);
            assert_eq!(response.status(), status);
            assert!(response.headers().get("set-cookie").is_none());
            assert!(
                !response
                    .headers()
                    .keys()
                    .any(|name| name.as_str().starts_with("access-control-"))
            );
            let rendered = to_bytes(response.into_body(), 128).await.unwrap();
            assert_eq!(rendered.as_ref(), expected.as_bytes());
            assert!(
                !rendered
                    .windows(CAPABILITY.len())
                    .any(|bytes| bytes == CAPABILITY.as_bytes())
            );
        }
    }

    #[test]
    fn every_head_rejection_has_a_bounded_fixed_body() {
        for rejection in ReconciliationRejection::ALL {
            let rejection = *rejection;
            assert!(rejection.body().len() <= 128);
            let response = rejection.response();
            assert_eq!(response.status(), rejection.status());
            assert_eq!(
                response.headers().get(axum::http::header::ALLOW).is_some(),
                rejection == ReconciliationRejection::MethodNotAllowed
            );
        }
    }
}
