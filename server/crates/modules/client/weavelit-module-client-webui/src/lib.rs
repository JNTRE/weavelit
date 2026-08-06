#![forbid(unsafe_code)]

//! Web UI translation for the restricted pre-operational status contract and
//! delivery of the compile-time Web UI asset allowlist.

use axum::{
    Router,
    body::{Body, Bytes},
    extract::Request,
    http::{
        HeaderMap, Method, StatusCode,
        header::{
            ACCEPT, ALLOW, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_SECURITY_POLICY, CONTENT_TYPE,
            TRANSFER_ENCODING, X_CONTENT_TYPE_OPTIONS,
        },
    },
    response::Response,
    routing::{MethodRouter, any},
};

/// Returns the Web UI Client Module route for the current status projection.
pub fn preoperational_status_route(database_selected: bool) -> MethodRouter {
    any(move |request| status_response(request, database_selected))
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
    "/../../../../web-ui/dist/assets/application.js"
));
const APPLICATION_CSS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../web-ui/dist/assets/application.css"
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
            Self::Script => "/assets/application.js",
            Self::Stylesheet => "/assets/application.css",
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

async fn status_response(request: Request, database_selected: bool) -> Response {
    let (parts, _body) = request.into_parts();
    if parts.method != Method::GET {
        return json_response_with_allow(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
    }
    if has_request_body(&parts.headers) || !accepts_json(&parts.headers) {
        return json_response(StatusCode::BAD_REQUEST, "bad_request");
    }

    let body = if database_selected {
        "{\"lifecycle\":\"uninitialized\",\"database_selected\":true}"
    } else {
        "{\"lifecycle\":\"uninitialized\",\"database_selected\":false}"
    };
    json_response_body(StatusCode::OK, body)
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
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    use super::{
        ASSET_CONTENT_SECURITY_POLICY, EMBEDDED_ASSETS, EmbeddedAsset, embedded_asset_routes,
        status_response,
    };

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
            EmbeddedAsset::Script => "assets/application.js",
            EmbeddedAsset::Stylesheet => "assets/application.css",
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
            ["/", "/assets/application.js", "/assets/application.css"]
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
            "/assets/application.js/",
            "/assets/Application.js",
            "/ASSETS/application.js",
            "/assets/%61pplication.js",
            "/assets/application%2Ejs",
            "/assets/../assets/application.js",
            "/../assets/application.js",
            "/assets/..%2Fapplication.js",
            "/%2E%2E/assets/application.js",
            "/assets/application.js.map",
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
