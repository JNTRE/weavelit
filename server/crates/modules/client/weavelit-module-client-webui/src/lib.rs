#![forbid(unsafe_code)]

//! Web UI translation for the restricted pre-operational status contract.

use axum::{
    body::Body,
    extract::Request,
    http::{
        HeaderMap, Method, StatusCode,
        header::{ACCEPT, ALLOW, CONTENT_LENGTH, CONTENT_TYPE, TRANSFER_ENCODING},
    },
    response::Response,
    routing::{MethodRouter, any},
};

/// Returns the Web UI Client Module route for the current status projection.
pub fn preoperational_status_route(database_selected: bool) -> MethodRouter {
    any(move |request| status_response(request, database_selected))
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
        .get(CONTENT_LENGTH)
        .is_some_and(|value| value.as_bytes() != b"0")
        || headers.contains_key(TRANSFER_ENCODING)
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

    use super::status_response;

    async fn response_body(response: axum::response::Response) -> String {
        String::from_utf8(to_bytes(response.into_body(), 128).await.unwrap().to_vec()).unwrap()
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
}
