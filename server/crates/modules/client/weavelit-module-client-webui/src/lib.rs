#![forbid(unsafe_code)]

//! Browser-specific Web UI Client Module surface.
//!
//! It declares the shared pre-operational capabilities that
//! [`weavelit_module_client`] implements, and owns the compile-time Web UI
//! asset allowlist and its delivery routes.

use axum::{
    Router,
    body::{Body, Bytes},
    extract::Request,
    http::{
        Method, StatusCode,
        header::{CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS},
    },
    response::Response,
    routing::any,
};
pub use weavelit_module_client::{
    ExpectedOrigin, OperationalSurface, PreoperationalSurface, ProjectionSource, SelectionCommit,
};
use weavelit_module_client::{has_request_body, json_response, json_response_with_allow};

/// Returns the pre-operational surface this Client Module declares.
///
/// Presence is the declaration: the Web UI declares the live status projection,
/// Application Database selection, and browser asset capabilities, so the
/// Server core mounts exactly those and nothing else.
pub fn preoperational_surface(
    projection: ProjectionSource,
    expected_origin: ExpectedOrigin,
    commit: SelectionCommit,
) -> PreoperationalSurface {
    PreoperationalSurface::default()
        .with_status(projection)
        .with_database_selection(expected_origin, commit)
        .with_assets(embedded_asset_routes())
}

/// Returns the operational surface this Client Module declares.
///
/// A sealed deployment's Web UI declares only browser asset delivery, so the
/// pre-operational status and Application Database routes are absent rather
/// than mounted and denied.
pub fn operational_surface() -> OperationalSurface {
    OperationalSurface::default().with_assets(embedded_asset_routes())
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
fn embedded_asset_routes() -> Router {
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

#[cfg(test)]
mod tests {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    use super::{
        ASSET_CONTENT_SECURITY_POLICY, EMBEDDED_ASSETS, EmbeddedAsset, embedded_asset_routes,
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
}
