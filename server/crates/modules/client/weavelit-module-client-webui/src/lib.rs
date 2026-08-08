#![forbid(unsafe_code)]

//! Web UI translation for the restricted pre-operational status and Application
//! Database selection contracts, and delivery of the compile-time Web UI asset
//! allowlist.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    extract::Request,
    http::{
        HeaderMap, HeaderValue, Method, StatusCode,
        header::{
            ACCEPT, ALLOW, AsHeaderName, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_SECURITY_POLICY,
            CONTENT_TYPE, HOST, ORIGIN, TRANSFER_ENCODING, X_CONTENT_TYPE_OPTIONS,
        },
    },
    response::Response,
    routing::{MethodRouter, any},
};
use serde::Deserialize;
use weavelit_server_lifecycle::{LifecycleProjection, SelectionFailureKind};

/// Live lifecycle projection source the Server core supplies to a mounted route.
///
/// The module holds no lifecycle state of its own; it calls this once per
/// request, so a response never reports a value captured at startup. `None`
/// means the trusted lifecycle boundary could not be read.
pub type ProjectionSource = Arc<dyn Fn() -> Option<LifecycleProjection> + Send + Sync>;

/// Server-core commit hook for a validated Application Database selection.
///
/// The module is not the selection authority: it hands the validated backend to
/// this hook, which owns the lifecycle mutation and returns the projection
/// observed under the same mutation permit.
pub type SelectionCommit = Arc<
    dyn Fn(SelectedBackend) -> Result<LifecycleProjection, DatabaseSelectionRejection>
        + Send
        + Sync,
>;

/// Returns the Web UI Client Module route for the live status projection.
pub fn preoperational_status_route(projection: ProjectionSource) -> MethodRouter {
    any(move |request| status_response(request, Arc::clone(&projection)))
}

/// Returns the Web UI Client Module route for Application Database selection.
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
    match commit(selection.backend()) {
        Ok(projection) => database_selection_response(&projection),
        Err(rejection) => rejection.response(),
    }
}

// ---------------------------------------------------------------------------
// Application Database selection contract
// ---------------------------------------------------------------------------

/// Largest request body this Client Module accepts for database selection.
pub const MAX_DATABASE_SELECTION_BODY_BYTES: usize = 1024;

/// The exact request media type and the only negotiable response media type.
const JSON_MEDIA_TYPE: &[u8] = b"application/json";

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
        let denied = DatabaseSelectionRejection::RequestOriginDenied;

        let csrf = single_header(headers, CSRF_HEADER_NAME).ok_or(denied)?;
        if csrf.as_bytes() != CSRF_HEADER_VALUE {
            return Err(denied);
        }

        let origin = single_header(headers, ORIGIN).ok_or(denied)?;
        let host = single_header(headers, HOST).ok_or(denied)?;

        let origin_authority = origin
            .to_str()
            .ok()
            .and_then(|value| value.strip_prefix("https://"))
            .and_then(normalize_authority)
            .ok_or(denied)?;
        let host_authority = host
            .to_str()
            .ok()
            .and_then(normalize_authority)
            .ok_or(denied)?;

        let expected = (self.address, self.port);
        if origin_authority != expected || host_authority != expected {
            return Err(denied);
        }
        Ok(())
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

fn single_header<N: AsHeaderName>(headers: &HeaderMap, name: N) -> Option<&HeaderValue> {
    let mut values = headers.get_all(name).iter();
    match (values.next(), values.next()) {
        (Some(value), None) => Some(value),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Compile-time asset allowlist
// ---------------------------------------------------------------------------

const MAX_HTML_ASSET_BYTES: usize = 16 * 1024;
const MAX_JAVASCRIPT_ASSET_BYTES: usize = 256 * 1024;
const MAX_CSS_ASSET_BYTES: usize = 64 * 1024;

const INDEX_HTML: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../web-ui/dist/index.html"
));
const APPLICATION_JAVASCRIPT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../web-ui/dist/assets/weavelit-application.js"
));
const APPLICATION_CSS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../web-ui/dist/assets/weavelit-application.css"
));

const _: () = assert!(
    !INDEX_HTML.is_empty() && INDEX_HTML.len() <= MAX_HTML_ASSET_BYTES,
    "the embedded Web UI document must be present and within its 16 KiB bound"
);
const _: () = assert!(
    !APPLICATION_JAVASCRIPT.is_empty()
        && APPLICATION_JAVASCRIPT.len() <= MAX_JAVASCRIPT_ASSET_BYTES,
    "the embedded Web UI script must be present and within its 256 KiB bound"
);
const _: () = assert!(
    !APPLICATION_CSS.is_empty() && APPLICATION_CSS.len() <= MAX_CSS_ASSET_BYTES,
    "the embedded Web UI stylesheet must be present and within its 64 KiB bound"
);

const ASSET_CONTENT_SECURITY_POLICY: &str = concat!(
    "default-src 'none'; base-uri 'none'; object-src 'none'; frame-ancestors 'none'; ",
    "form-action 'none'; script-src 'self'; style-src 'self'; connect-src 'self'"
);

/// The exact set of browser assets this Client Module is permitted to deliver.
#[derive(Clone, Copy)]
enum EmbeddedAsset {
    Document,
    Script,
    Stylesheet,
}

impl EmbeddedAsset {
    const fn path(self) -> &'static str {
        match self {
            Self::Document => "/",
            Self::Script => "/assets/weavelit-application.js",
            Self::Stylesheet => "/assets/weavelit-application.css",
        }
    }

    const fn bytes(self) -> &'static [u8] {
        match self {
            Self::Document => INDEX_HTML,
            Self::Script => APPLICATION_JAVASCRIPT,
            Self::Stylesheet => APPLICATION_CSS,
        }
    }

    const fn media_type(self) -> &'static str {
        match self {
            Self::Document => "text/html; charset=utf-8",
            Self::Script => "text/javascript; charset=utf-8",
            Self::Stylesheet => "text/css; charset=utf-8",
        }
    }
}

const EMBEDDED_ASSETS: [EmbeddedAsset; 3] = [
    EmbeddedAsset::Document,
    EmbeddedAsset::Script,
    EmbeddedAsset::Stylesheet,
];

/// Returns the Web UI Client Module routes for its compile-time asset allowlist.
///
/// Every route is an exact path with no wildcard, prefix, or fallback, so no
/// other target is served and `/api/` routing is never captured.
pub fn embedded_asset_routes() -> Router {
    EMBEDDED_ASSETS.iter().fold(Router::new(), |router, asset| {
        let asset = *asset;
        router.route(
            asset.path(),
            any(move |request| asset_response(request, asset)),
        )
    })
}

async fn asset_response(request: Request, asset: EmbeddedAsset) -> Response {
    let (parts, _body) = request.into_parts();
    if parts.method != Method::GET {
        return json_response_with_allow(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
    }
    if has_request_body(&parts.headers) {
        return json_response(StatusCode::BAD_REQUEST, "bad_request");
    }

    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, asset.media_type())
        .header(CONTENT_SECURITY_POLICY, ASSET_CONTENT_SECURITY_POLICY)
        .header(X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(CACHE_CONTROL, "no-store")
        .body(Body::from(Bytes::from_static(asset.bytes())))
        .expect("fixed Web UI asset responses must be valid")
}

async fn status_response(request: Request, projection: ProjectionSource) -> Response {
    let (parts, _body) = request.into_parts();
    if parts.method != Method::GET {
        return json_response_with_allow(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
    }
    if has_request_body(&parts.headers) || !accepts_json(&parts.headers) {
        return json_response(StatusCode::BAD_REQUEST, "bad_request");
    }

    let Some(projection) = projection() else {
        return json_response(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable");
    };
    json_response_body(StatusCode::OK, projection_body(&projection))
}

fn has_request_body(headers: &HeaderMap) -> bool {
    headers
        .get_all(CONTENT_LENGTH)
        .iter()
        .any(|value| parse_content_length(value) != Some(0))
        || headers.contains_key(TRANSFER_ENCODING)
}

fn parse_content_length(value: &axum::http::HeaderValue) -> Option<u64> {
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

fn accepts_json(headers: &HeaderMap) -> bool {
    let mut values = headers.get_all(ACCEPT).iter();
    match (values.next(), values.next()) {
        (None, _) => true,
        (Some(value), None) => value.as_bytes() == b"application/json",
        _ => false,
    }
}

fn json_response(status: StatusCode, error: &'static str) -> Response {
    let body = match error {
        "bad_request" => "{\"error\":\"bad_request\"}",
        "method_not_allowed" => "{\"error\":\"method_not_allowed\"}",
        "service_unavailable" => "{\"error\":\"service_unavailable\"}",
        _ => unreachable!("all Web UI status errors use fixed codes"),
    };
    json_response_body(status, body)
}

fn json_response_with_allow(status: StatusCode, error: &'static str) -> Response {
    let mut response = json_response(status, error);
    response.headers_mut().insert(ALLOW, "GET".parse().unwrap());
    response
}

fn json_response_body(status: StatusCode, body: &'static str) -> Response {
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "application/json; charset=utf-8")
        .body(Body::from(body))
        .expect("fixed Web UI status responses must be valid")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
        response::Response,
    };
    use tower::ServiceExt;
    use weavelit_server_lifecycle::LifecycleProjection;

    use super::{
        ASSET_CONTENT_SECURITY_POLICY, EMBEDDED_ASSETS, EmbeddedAsset, ProjectionSource,
        embedded_asset_routes,
    };

    /// Builds a projection source; `None` models an unreadable lifecycle boundary.
    fn projection_source(database_selected: Option<bool>) -> ProjectionSource {
        Arc::new(move || database_selected.map(LifecycleProjection::new))
    }

    async fn status_response(request: Request<Body>, database_selected: bool) -> Response {
        super::status_response(request, projection_source(Some(database_selected))).await
    }

    const FORBIDDEN_RESPONSE_HEADERS: [&str; 7] = [
        "access-control-allow-origin",
        "access-control-allow-credentials",
        "access-control-expose-headers",
        "set-cookie",
        "content-encoding",
        "vary",
        "server",
    ];

    fn generated_asset_bytes(asset: EmbeddedAsset) -> Vec<u8> {
        let relative = match asset {
            EmbeddedAsset::Document => "index.html",
            EmbeddedAsset::Script => "assets/weavelit-application.js",
            EmbeddedAsset::Stylesheet => "assets/weavelit-application.css",
        };
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../web-ui/dist")
            .join(relative);
        std::fs::read(path).unwrap()
    }

    async fn asset_route_response(method: &str, target: &str) -> axum::response::Response {
        embedded_asset_routes()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(target)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn response_body(response: axum::response::Response) -> String {
        String::from_utf8(to_bytes(response.into_body(), 128).await.unwrap().to_vec()).unwrap()
    }

    #[tokio::test]
    async fn asset_routes_deliver_the_exact_generated_assets() {
        for asset in EMBEDDED_ASSETS {
            let path = asset.path();
            let response = asset_route_response("GET", path).await;
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            let headers = response.headers().clone();
            assert_eq!(
                headers.get("content-type").unwrap(),
                asset.media_type(),
                "{path}"
            );
            assert_eq!(
                headers.get("content-security-policy").unwrap(),
                ASSET_CONTENT_SECURITY_POLICY,
                "{path}"
            );
            assert_eq!(
                headers.get("x-content-type-options").unwrap(),
                "nosniff",
                "{path}"
            );
            assert_eq!(headers.get("cache-control").unwrap(), "no-store", "{path}");
            for forbidden in FORBIDDEN_RESPONSE_HEADERS {
                assert!(!headers.contains_key(forbidden), "{path}: {forbidden}");
            }

            let served = to_bytes(response.into_body(), 256 * 1024).await.unwrap();
            assert_eq!(served.as_ref(), asset.bytes(), "{path}");
            assert_eq!(served.as_ref(), generated_asset_bytes(asset), "{path}");
        }
    }

    #[tokio::test]
    async fn asset_routes_expose_exactly_the_compile_time_allowlist() {
        assert_eq!(
            EMBEDDED_ASSETS.map(EmbeddedAsset::path),
            [
                "/",
                "/assets/weavelit-application.js",
                "/assets/weavelit-application.css"
            ]
        );
    }

    #[tokio::test]
    async fn asset_routes_reject_unsupported_methods_and_request_bodies() {
        for asset in EMBEDDED_ASSETS {
            let path = asset.path();
            for method in ["POST", "PUT", "PATCH", "DELETE", "OPTIONS", "HEAD"] {
                let response = asset_route_response(method, path).await;
                assert_eq!(
                    response.status(),
                    StatusCode::METHOD_NOT_ALLOWED,
                    "{path} {method}"
                );
                assert_eq!(response.headers().get("allow").unwrap(), "GET");
                assert_eq!(
                    response.headers().get("content-type").unwrap(),
                    "application/json; charset=utf-8"
                );
                // Axum strips the body from a `HEAD` response; the transport
                // parser rejects the method before routing in any case.
                let expected_body = if method == "HEAD" {
                    ""
                } else {
                    "{\"error\":\"method_not_allowed\"}"
                };
                assert_eq!(
                    response_body(response).await,
                    expected_body,
                    "{path} {method}"
                );
            }

            for (name, value) in [("content-length", "1"), ("transfer-encoding", "chunked")] {
                let response = embedded_asset_routes()
                    .oneshot(
                        Request::builder()
                            .uri(path)
                            .header(name, value)
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path} {name}");
                assert_eq!(response_body(response).await, "{\"error\":\"bad_request\"}");
            }
        }
    }

    #[tokio::test]
    async fn asset_routes_serve_no_target_outside_the_allowlist() {
        for target in [
            "/index.html",
            "/assets/",
            "/assets/weavelit-application.js/",
            "/assets/Weavelit-Application.js",
            "/ASSETS/weavelit-application.js",
            "/assets/%77eavelit-application.js",
            "/assets/weavelit-application%2Ejs",
            "/assets/../assets/weavelit-application.js",
            "/../assets/weavelit-application.js",
            "/assets/..%2Fweavelit-application.js",
            "/%2E%2E/assets/weavelit-application.js",
            "/assets/weavelit-application.js.map",
            "/api/v1/status",
            "/api/",
            "//",
        ] {
            let response = asset_route_response("GET", target).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{target}");
            assert!(
                to_bytes(response.into_body(), 256 * 1024)
                    .await
                    .unwrap()
                    .is_empty(),
                "{target}"
            );
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
