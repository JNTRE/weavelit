//! Shared Client Module contract for the two-step Restore submission protocol.
//!
//! A Restore is submitted in two requests. The first submits the recovery key
//! alone and receives a short-lived one-time ticket. The second uploads the
//! encrypted artifact and carries that ticket in one exact custom header. The
//! recovery key therefore never travels with the artifact, and the artifact is
//! never admitted without a ticket the Server itself issued.
//!
//! This module owns the canonical routes, the request schemas, every header
//! precondition, the payload-free rejection contract, and the two typed
//! success envelopes. It owns no lifecycle authority, no ticket store, and no
//! orchestration: it hands a validated submission to a Server-core hook and
//! renders exactly what that hook returns.

use std::fmt;
use std::pin::Pin;
use std::sync::Arc;

use axum::{
    body::{Bytes, to_bytes},
    extract::Request,
    http::{Extensions, HeaderMap, HeaderValue, Method, StatusCode, header::ALLOW},
    response::Response,
    routing::{MethodRouter, any},
};
use serde::de::{self, Deserialize, Deserializer, MapAccess, Visitor};
use zeroize::Zeroizing;

use crate::{
    ExpectedOrigin, JSON_MEDIA_TYPE, accepts_json, json_response_body, single_header,
    typed_json::{
        OpaqueToken, ResponseCorrelation, StableCode, TypedJsonEnvelope, TypedResult, TypedValue,
        typed_json_response,
    },
};

/// The canonical route that submits a Restore recovery key and issues a ticket.
pub const RESTORE_ROUTE: &str = "/api/v1/restore";

/// The canonical route that uploads the encrypted artifact against a ticket.
pub const RESTORE_ARTIFACT_ROUTE: &str = "/api/v1/restore/artifact";

/// Non-simple header that carries the issued ticket back to the Server.
///
/// It is named consistently with [`crate::CSRF_HEADER_NAME`] and, like it, a
/// browser cannot send it cross-site without a preflight. The ticket is never
/// accepted from a URL, a query string, or a cookie.
pub const RESTORE_TICKET_HEADER_NAME: &str = "x-weavelit-restore-ticket";

/// The result field name that carries an issued ticket.
const RESTORE_TICKET_FIELD: &str = "restore_ticket";

/// The result field name that reports the activated lifecycle state.
const LIFECYCLE_FIELD: &str = "lifecycle";

/// The only lifecycle value a completed Restore reports.
const LIFECYCLE_INITIALIZED: &str = "initialized";

/// Largest request body accepted for the recovery-key submission.
///
/// The key submission carries one canonical age identity line and nothing
/// else, so it stays inside the listener's default body bound and is never
/// given the artifact route's admitted profile.
pub const MAX_RESTORE_KEY_BODY_BYTES: usize = 1024;

/// The exact request media type of an encrypted artifact upload.
const ARTIFACT_MEDIA_TYPE: &[u8] = b"application/octet-stream";

/// A validated recovery-key submission handed to the Server core.
///
/// The key is owned and cleared when dropped, so a rejected or abandoned
/// submission leaves no plaintext key behind in this crate.
pub struct RestoreKeySubmission {
    /// The submitted recovery key, still unvalidated as an age identity.
    pub recovery_key: Zeroizing<String>,
    /// The admitted request's extensions, which carry the Server core's own
    /// admission permit and pre-body grant.
    pub context: Extensions,
}

/// A validated artifact upload handed to the Server core.
pub struct RestoreArtifactSubmission {
    /// The exact declared artifact bytes the listener already admitted.
    pub artifact: Bytes,
    /// The admitted request's extensions, which carry the Server core's own
    /// admission permit, claimed ticket, and inherited request budget.
    pub context: Extensions,
}

/// What the Server core returns after retaining a submitted recovery key.
pub struct RestoreTicketIssued {
    /// The one-time ticket the artifact upload must present.
    pub ticket: String,
    /// The Server-generated correlation identifier for this Restore.
    pub correlation_id: String,
}

/// What the Server core returns after a completed Restore.
pub struct RestoreCompleted {
    /// The Server-generated correlation identifier for this Restore.
    pub correlation_id: String,
}

/// Server-core hook that retains a submitted recovery key and issues a ticket.
pub type RestoreKeyCommit = Arc<
    dyn Fn(
            RestoreKeySubmission,
        )
            -> Pin<Box<dyn Future<Output = Result<RestoreTicketIssued, RestoreRejection>> + Send>>
        + Send
        + Sync,
>;

/// Server-core hook that claims a ticket and runs one Restore to completion.
pub type RestoreArtifactCommit = Arc<
    dyn Fn(
            RestoreArtifactSubmission,
        )
            -> Pin<Box<dyn Future<Output = Result<RestoreCompleted, RestoreRejection>> + Send>>
        + Send
        + Sync,
>;

/// The runtime collaborators a Client Module declares Restore with.
pub struct RestoreCapability {
    /// The trusted authority every Restore request must target.
    pub expected_origin: ExpectedOrigin,
    /// The approved encrypted-artifact bound the Server core admitted.
    pub max_artifact_bytes: usize,
    /// The hook that retains a recovery key and issues its ticket.
    pub submit_key: RestoreKeyCommit,
    /// The hook that claims a ticket and completes the Restore.
    pub upload_artifact: RestoreArtifactCommit,
}

/// A declared Restore capability, split into its two mountable routes.
///
/// The Server core mounts each route together with the transport registration
/// that admits its body, so the artifact route's larger bound and its own read
/// budget can never be granted to a route that was not mounted.
pub struct RestoreDeclaration {
    capability: Arc<RestoreCapability>,
}

impl RestoreDeclaration {
    /// Declares Restore over the supplied runtime collaborators.
    #[must_use]
    pub fn new(capability: RestoreCapability) -> Self {
        Self {
            capability: Arc::new(capability),
        }
    }

    /// Returns the recovery-key route mounted at [`RESTORE_ROUTE`].
    pub fn key_route(&self) -> MethodRouter {
        let capability = Arc::clone(&self.capability);
        any(move |request| restore_key_response(request, Arc::clone(&capability)))
    }

    /// Returns the artifact route mounted at [`RESTORE_ARTIFACT_ROUTE`].
    pub fn artifact_route(&self) -> MethodRouter {
        let capability = Arc::clone(&self.capability);
        any(move |request| restore_artifact_response(request, Arc::clone(&capability)))
    }
}

// ---------------------------------------------------------------------------
// Rejection contract
// ---------------------------------------------------------------------------

/// The complete, payload-free rejection contract for both Restore routes.
///
/// Every variant carries a fixed body and nothing else. No variant can report
/// which validation step failed beyond its stable code, and none can carry a
/// recovery key, a ticket, an artifact byte, or any backup content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestoreRejection {
    /// `400` for a malformed body, media type, or `Accept` value.
    BadRequest,
    /// `400` for a submitted value that is not one canonical recovery key.
    RecoveryKeyInvalid,
    /// `400` for a malformed, unauthentic, altered, or invalid artifact.
    BackupInvalid,
    /// `400` for an artifact this Server's compiled-in components cannot serve.
    BackupIncompatible,
    /// `403` for a failed same-origin, `Host`, or CSRF header check.
    RequestOriginDenied,
    /// `403` for a missing, malformed, replayed, expired, or unknown ticket.
    RestoreTicketInvalid,
    /// `405` for any method other than `PUT`.
    MethodNotAllowed,
    /// `409` for a lifecycle state that no longer permits a Restore.
    RestoreNotAllowed,
    /// `409` for a Restore that is already outstanding or in progress.
    RestorePending,
    /// `500` for a deadline, storage, or other internal Restore failure.
    RestoreFailed,
    /// `503` for a backend, persistence, or integrity failure.
    ServiceUnavailable,
}

impl RestoreRejection {
    /// Returns the documented status code.
    #[must_use]
    pub const fn status(self) -> StatusCode {
        match self {
            Self::BadRequest
            | Self::RecoveryKeyInvalid
            | Self::BackupInvalid
            | Self::BackupIncompatible => StatusCode::BAD_REQUEST,
            Self::RequestOriginDenied | Self::RestoreTicketInvalid => StatusCode::FORBIDDEN,
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::RestoreNotAllowed | Self::RestorePending => StatusCode::CONFLICT,
            Self::RestoreFailed => StatusCode::INTERNAL_SERVER_ERROR,
            Self::ServiceUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    /// Returns the documented fixed JSON body.
    #[must_use]
    pub const fn body(self) -> &'static str {
        match self {
            Self::BadRequest => "{\"error\":\"bad_request\"}",
            Self::RecoveryKeyInvalid => "{\"error\":\"recovery_key_invalid\"}",
            Self::BackupInvalid => "{\"error\":\"backup_invalid\"}",
            Self::BackupIncompatible => "{\"error\":\"backup_incompatible\"}",
            Self::RequestOriginDenied => "{\"error\":\"request_origin_denied\"}",
            Self::RestoreTicketInvalid => "{\"error\":\"restore_ticket_invalid\"}",
            Self::MethodNotAllowed => "{\"error\":\"method_not_allowed\"}",
            Self::RestoreNotAllowed => "{\"error\":\"restore_not_allowed\"}",
            Self::RestorePending => "{\"error\":\"restore_pending\"}",
            Self::RestoreFailed => "{\"error\":\"restore_failed\"}",
            Self::ServiceUnavailable => "{\"error\":\"service_unavailable\"}",
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

// ---------------------------------------------------------------------------
// Head validation
// ---------------------------------------------------------------------------

/// Validates every header precondition of a recovery-key submission.
///
/// The same-origin and CSRF trust check runs before media-type validation so a
/// cross-site request is denied without revealing negotiation detail.
pub fn validate_restore_key_request(
    method: &Method,
    headers: &HeaderMap,
    expected_origin: ExpectedOrigin,
) -> Result<(), RestoreRejection> {
    if method != Method::PUT {
        return Err(RestoreRejection::MethodNotAllowed);
    }
    if !expected_origin.is_trusted(headers) {
        return Err(RestoreRejection::RequestOriginDenied);
    }
    validate_media(headers, JSON_MEDIA_TYPE)
}

/// Validates every header precondition of an artifact upload.
///
/// The ticket header is checked for shape only. Whether the ticket is known,
/// unexpired, and unclaimed is decided by the Server core, which holds the
/// only digest it could be compared against.
pub fn validate_restore_artifact_request(
    method: &Method,
    headers: &HeaderMap,
    expected_origin: ExpectedOrigin,
) -> Result<(), RestoreRejection> {
    if method != Method::PUT {
        return Err(RestoreRejection::MethodNotAllowed);
    }
    if !expected_origin.is_trusted(headers) {
        return Err(RestoreRejection::RequestOriginDenied);
    }
    validate_media(headers, ARTIFACT_MEDIA_TYPE)?;
    submitted_restore_ticket(headers).map(|_| ())
}

/// Returns the well-formed ticket the request presented.
///
/// Requires exactly one [`RESTORE_TICKET_HEADER_NAME`] header whose value is a
/// canonical opaque token. A missing, repeated, or malformed header is
/// indistinguishable from an unknown ticket.
pub fn submitted_restore_ticket(headers: &HeaderMap) -> Result<&str, RestoreRejection> {
    let invalid = RestoreRejection::RestoreTicketInvalid;
    let value = single_header(headers, RESTORE_TICKET_HEADER_NAME).ok_or(invalid)?;
    let ticket = value.to_str().map_err(|_| invalid)?;
    if OpaqueToken::new(ticket).is_none() {
        return Err(invalid);
    }
    Ok(ticket)
}

/// Requires exactly one exact `Content-Type` and a JSON-or-absent `Accept`.
fn validate_media(headers: &HeaderMap, media_type: &[u8]) -> Result<(), RestoreRejection> {
    let content_type = single_header(headers, axum::http::header::CONTENT_TYPE)
        .ok_or(RestoreRejection::BadRequest)?;
    if content_type.as_bytes() != media_type || !accepts_json(headers) {
        return Err(RestoreRejection::BadRequest);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Request schema
// ---------------------------------------------------------------------------

/// The strictly validated recovery-key submission body.
///
/// The implementation is written by hand rather than derived because a derived
/// struct also accepts its JSON array form, which would let `["<key>"]` submit
/// a recovery key through a shape the API contract does not document.
struct RecoveryKeyBody {
    recovery_key: Zeroizing<String>,
}

impl<'de> Deserialize<'de> for RecoveryKeyBody {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BodyVisitor;

        impl<'de> Visitor<'de> for BodyVisitor {
            type Value = RecoveryKeyBody;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a recovery key submission object")
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut recovery_key: Option<Zeroizing<String>> = None;
                while let Some(field) = map.next_key::<String>()? {
                    if field != "recovery_key" {
                        return Err(de::Error::unknown_field(&field, &["recovery_key"]));
                    }
                    if recovery_key.is_some() {
                        return Err(de::Error::duplicate_field("recovery_key"));
                    }
                    recovery_key = Some(Zeroizing::new(map.next_value()?));
                }
                Ok(RecoveryKeyBody {
                    recovery_key: recovery_key
                        .ok_or_else(|| de::Error::missing_field("recovery_key"))?,
                })
            }
        }

        deserializer.deserialize_map(BodyVisitor)
    }
}

/// Parses the exact accepted body `{"recovery_key":"<canonical age identity>"}`.
///
/// An unknown field, duplicate key, missing field, wrongly typed value, array
/// form, trailing content, empty body, or oversized body is rejected. The
/// parsed key is moved straight into a clearing wrapper and is never copied
/// again here.
fn parse_recovery_key(body: &[u8]) -> Result<Zeroizing<String>, RestoreRejection> {
    if body.len() > MAX_RESTORE_KEY_BODY_BYTES {
        return Err(RestoreRejection::BadRequest);
    }
    let mut deserializer = serde_json::Deserializer::from_slice(body);
    let parsed = RecoveryKeyBody::deserialize(&mut deserializer)
        .map_err(|_| RestoreRejection::BadRequest)?;
    deserializer
        .end()
        .map_err(|_| RestoreRejection::BadRequest)?;
    Ok(parsed.recovery_key)
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

async fn restore_key_response(request: Request, capability: Arc<RestoreCapability>) -> Response {
    let (parts, body) = request.into_parts();
    if let Err(rejection) =
        validate_restore_key_request(&parts.method, &parts.headers, capability.expected_origin)
    {
        return rejection.response();
    }
    let Ok(body) = to_bytes(body, MAX_RESTORE_KEY_BODY_BYTES).await else {
        return RestoreRejection::BadRequest.response();
    };
    let recovery_key = match parse_recovery_key(&body) {
        Ok(recovery_key) => recovery_key,
        Err(rejection) => return rejection.response(),
    };
    drop(body);

    match (capability.submit_key)(RestoreKeySubmission {
        recovery_key,
        context: parts.extensions,
    })
    .await
    {
        Ok(issued) => restore_ticket_response(&issued),
        Err(rejection) => rejection.response(),
    }
}

async fn restore_artifact_response(
    request: Request,
    capability: Arc<RestoreCapability>,
) -> Response {
    let (parts, body) = request.into_parts();
    if let Err(rejection) =
        validate_restore_artifact_request(&parts.method, &parts.headers, capability.expected_origin)
    {
        return rejection.response();
    }
    let Ok(artifact) = to_bytes(body, capability.max_artifact_bytes).await else {
        return RestoreRejection::BadRequest.response();
    };

    match (capability.upload_artifact)(RestoreArtifactSubmission {
        artifact,
        context: parts.extensions,
    })
    .await
    {
        Ok(completed) => restore_completed_response(&completed),
        Err(rejection) => rejection.response(),
    }
}

/// Renders the only response that may ever carry an issued ticket.
///
/// The ticket is returned in the typed envelope alone. It is never placed in a
/// header, a redirect target, or a cookie, so it cannot be logged by an
/// intermediary or replayed from a browser history entry.
fn restore_ticket_response(issued: &RestoreTicketIssued) -> Response {
    let Some(ticket) = OpaqueToken::new(&issued.ticket) else {
        return RestoreRejection::RestoreFailed.response();
    };
    let Some(result) = typed_field(RESTORE_TICKET_FIELD, TypedValue::Token(ticket)) else {
        return RestoreRejection::RestoreFailed.response();
    };
    match ResponseCorrelation::new(&issued.correlation_id) {
        Some(correlation_id) => typed_json_response(
            StatusCode::ACCEPTED,
            TypedJsonEnvelope::Result {
                result,
                correlation_id,
            },
        ),
        None => RestoreRejection::RestoreFailed.response(),
    }
}

/// Renders the completion envelope of an activated Restore.
fn restore_completed_response(completed: &RestoreCompleted) -> Response {
    let Some(state) = StableCode::new(LIFECYCLE_INITIALIZED) else {
        return RestoreRejection::RestoreFailed.response();
    };
    let Some(result) = typed_field(LIFECYCLE_FIELD, TypedValue::Code(state)) else {
        return RestoreRejection::RestoreFailed.response();
    };
    match ResponseCorrelation::new(&completed.correlation_id) {
        Some(correlation_id) => typed_json_response(
            StatusCode::OK,
            TypedJsonEnvelope::Result {
                result,
                correlation_id,
            },
        ),
        None => RestoreRejection::RestoreFailed.response(),
    }
}

fn typed_field(name: &str, value: TypedValue) -> Option<TypedResult> {
    TypedResult::new().with_field(StableCode::new(name)?, value)
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};

    use super::{
        MAX_RESTORE_KEY_BODY_BYTES, RESTORE_TICKET_HEADER_NAME, RestoreCompleted, RestoreRejection,
        RestoreTicketIssued, parse_recovery_key, restore_completed_response,
        restore_ticket_response, submitted_restore_ticket, validate_restore_artifact_request,
        validate_restore_key_request,
    };
    use crate::{CSRF_HEADER_NAME, ExpectedOrigin, typed_json::TypedJsonEnvelope};

    const TICKET: &str = "0123456789abcdefghijklmnopqrstuvwxyzABCDEFG";
    const CORRELATION: &str = "0123456789abcdef0123456789abcdef";

    fn expected_origin() -> ExpectedOrigin {
        ExpectedOrigin::from_listener("127.0.0.1:8443".parse().unwrap())
    }

    fn trusted_headers(content_type: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CSRF_HEADER_NAME, HeaderValue::from_static("1"));
        headers.insert("origin", HeaderValue::from_static("https://127.0.0.1:8443"));
        headers.insert("host", HeaderValue::from_static("127.0.0.1:8443"));
        headers.insert("content-type", HeaderValue::from_str(content_type).unwrap());
        headers
    }

    fn artifact_headers() -> HeaderMap {
        let mut headers = trusted_headers("application/octet-stream");
        headers.insert(RESTORE_TICKET_HEADER_NAME, HeaderValue::from_static(TICKET));
        headers
    }

    fn envelope(response: &axum::response::Response) -> String {
        response
            .extensions()
            .get::<TypedJsonEnvelope>()
            .expect("a Restore success renders a typed envelope")
            .serialize()
    }

    #[test]
    fn a_key_submission_requires_its_method_origin_and_media_type() {
        assert_eq!(
            validate_restore_key_request(
                &Method::PUT,
                &trusted_headers("application/json"),
                expected_origin()
            ),
            Ok(())
        );
        assert_eq!(
            validate_restore_key_request(
                &Method::GET,
                &trusted_headers("application/json"),
                expected_origin()
            ),
            Err(RestoreRejection::MethodNotAllowed)
        );

        let mut untrusted = trusted_headers("application/json");
        untrusted.remove(CSRF_HEADER_NAME);
        assert_eq!(
            validate_restore_key_request(&Method::PUT, &untrusted, expected_origin()),
            Err(RestoreRejection::RequestOriginDenied)
        );

        assert_eq!(
            validate_restore_key_request(
                &Method::PUT,
                &trusted_headers("application/octet-stream"),
                expected_origin()
            ),
            Err(RestoreRejection::BadRequest)
        );
    }

    #[test]
    fn an_artifact_upload_requires_its_media_type_and_a_well_formed_ticket() {
        assert_eq!(
            validate_restore_artifact_request(&Method::PUT, &artifact_headers(), expected_origin()),
            Ok(())
        );

        let mut wrong_media = artifact_headers();
        wrong_media.insert("content-type", HeaderValue::from_static("application/json"));
        assert_eq!(
            validate_restore_artifact_request(&Method::PUT, &wrong_media, expected_origin()),
            Err(RestoreRejection::BadRequest)
        );

        let mut missing = artifact_headers();
        missing.remove(RESTORE_TICKET_HEADER_NAME);
        assert_eq!(
            validate_restore_artifact_request(&Method::PUT, &missing, expected_origin()),
            Err(RestoreRejection::RestoreTicketInvalid)
        );

        let mut repeated = artifact_headers();
        repeated.append(RESTORE_TICKET_HEADER_NAME, HeaderValue::from_static(TICKET));
        assert_eq!(
            validate_restore_artifact_request(&Method::PUT, &repeated, expected_origin()),
            Err(RestoreRejection::RestoreTicketInvalid)
        );

        for malformed in ["", "with=padding", "with/slash", &"a".repeat(49)] {
            let mut headers = artifact_headers();
            headers.insert(
                RESTORE_TICKET_HEADER_NAME,
                HeaderValue::from_str(malformed).unwrap(),
            );
            assert_eq!(
                submitted_restore_ticket(&headers),
                Err(RestoreRejection::RestoreTicketInvalid),
                "{malformed}"
            );
        }
    }

    #[test]
    fn the_recovery_key_schema_accepts_only_its_exact_shape() {
        assert_eq!(
            parse_recovery_key(b"{\"recovery_key\":\"AGE-SECRET-KEY-1TEST\"}")
                .unwrap()
                .as_str(),
            "AGE-SECRET-KEY-1TEST"
        );
        for rejected in [
            &b""[..],
            b"{}",
            b"{\"recovery_key\":1}",
            b"{\"recovery_key\":\"a\",\"extra\":1}",
            b"{\"recovery_key\":\"a\"}{}",
            b"[\"a\"]",
        ] {
            assert_eq!(
                parse_recovery_key(rejected),
                Err(RestoreRejection::BadRequest)
            );
        }
        let oversized = format!(
            "{{\"recovery_key\":\"{}\"}}",
            "a".repeat(MAX_RESTORE_KEY_BODY_BYTES)
        );
        assert_eq!(
            parse_recovery_key(oversized.as_bytes()),
            Err(RestoreRejection::BadRequest)
        );
    }

    #[test]
    fn both_successes_render_their_typed_envelopes() {
        let issued = restore_ticket_response(&RestoreTicketIssued {
            ticket: TICKET.to_owned(),
            correlation_id: CORRELATION.to_owned(),
        });
        assert_eq!(issued.status(), StatusCode::ACCEPTED);
        assert_eq!(
            envelope(&issued),
            format!(
                "{{\"result\":{{\"restore_ticket\":\"{TICKET}\"}},\"correlation_id\":\"{CORRELATION}\"}}"
            )
        );

        let completed = restore_completed_response(&RestoreCompleted {
            correlation_id: CORRELATION.to_owned(),
        });
        assert_eq!(completed.status(), StatusCode::OK);
        assert_eq!(
            envelope(&completed),
            format!(
                "{{\"result\":{{\"lifecycle\":\"initialized\"}},\"correlation_id\":\"{CORRELATION}\"}}"
            )
        );
    }

    #[test]
    fn an_unrenderable_success_falls_back_to_a_payload_free_failure() {
        let response = restore_ticket_response(&RestoreTicketIssued {
            ticket: "not a token".to_owned(),
            correlation_id: CORRELATION.to_owned(),
        });
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(response.extensions().get::<TypedJsonEnvelope>().is_none());

        let response = restore_completed_response(&RestoreCompleted {
            correlation_id: "NOT VALID".to_owned(),
        });
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn every_rejection_has_a_distinct_bounded_fixed_body() {
        for rejection in [
            RestoreRejection::BadRequest,
            RestoreRejection::RecoveryKeyInvalid,
            RestoreRejection::BackupInvalid,
            RestoreRejection::BackupIncompatible,
            RestoreRejection::RequestOriginDenied,
            RestoreRejection::RestoreTicketInvalid,
            RestoreRejection::MethodNotAllowed,
            RestoreRejection::RestoreNotAllowed,
            RestoreRejection::RestorePending,
            RestoreRejection::RestoreFailed,
            RestoreRejection::ServiceUnavailable,
        ] {
            assert!(rejection.body().len() <= 128, "{rejection:?}");
            let response = rejection.response();
            assert_eq!(response.status(), rejection.status());
            assert_eq!(
                response.headers().get(axum::http::header::ALLOW).is_some(),
                rejection == RestoreRejection::MethodNotAllowed
            );
        }
    }
}
