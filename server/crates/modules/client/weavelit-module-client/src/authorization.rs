//! Shared Client Module contract for the operational authorization denial.
//!
//! Every Client Module renders an authorization denial the same way, because a
//! Human User who is denied must not be able to tell one denial cause from
//! another. An inactive account, a disabled Client Module, a disabled Service
//! Module, a disabled or unowned Operation, and each missing grant all produce
//! the same status, the same headers, and the same body bytes.
//!
//! The only part that varies is the correlation identifier, which is the same
//! opaque identifier the Server records in the System Log denial record. It
//! lets an operator join a user's report to a record; it encodes no cause, and
//! nothing in the response reveals which requirement failed.
//!
//! This contract is deliberately distinct from the authentication contract in
//! [`crate::authentication`]. Authentication answers `401` with
//! `session_invalid` when it cannot tell who is asking; authorization answers
//! `403` with `authorization_denied` when it knows who is asking and that the
//! answer is no.

use std::fmt;

use axum::{http::StatusCode, response::Response};

use crate::json_response_body;
use crate::typed_json::{ResponseCorrelation, StableCode, TypedJsonEnvelope, typed_json_response};

/// The single stable error code every authorization denial reports.
pub const AUTHORIZATION_DENIED_CODE: &str = "authorization_denied";

/// The single status code every authorization denial reports.
pub const AUTHORIZATION_DENIED_STATUS: StatusCode = StatusCode::FORBIDDEN;

/// The one authorization denial a Client Module can render.
///
/// This is a unit type on purpose. It carries no cause and offers no
/// constructor that could accept one, so no caller can widen the contract into
/// a set of distinguishable denials, and no future denial cause can leak
/// through it by being added to a variant list.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AuthorizationRejection;

impl AuthorizationRejection {
    /// Returns the documented status code.
    #[must_use]
    pub const fn status(self) -> StatusCode {
        AUTHORIZATION_DENIED_STATUS
    }

    /// Returns the documented stable error code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        AUTHORIZATION_DENIED_CODE
    }

    /// Builds the typed denial envelope.
    ///
    /// A denial never carries a cookie effect, an `Allow` header, or any other
    /// varying header, so the response bytes differ across two denials only in
    /// the correlation identifier. The session the request presented stays
    /// exactly as it was: being denied one Operation neither ends the session
    /// nor re-establishes it.
    #[must_use]
    pub fn response(self, correlation_id: &str) -> Response {
        let Some(correlation) = ResponseCorrelation::new(correlation_id) else {
            return unrenderable_response();
        };
        let Some(error) = StableCode::new(self.code()) else {
            return unrenderable_response();
        };
        typed_json_response(
            self.status(),
            TypedJsonEnvelope::Error {
                error,
                correlation_id: correlation,
            },
        )
    }
}

impl fmt::Display for AuthorizationRejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

/// The payload-free response used when no typed envelope can be rendered.
///
/// A correlation identifier the Server could not produce, or produced outside
/// its accepted shape, must not become an allow and must not become a
/// differently shaped denial. It answers with the already-approved unavailable
/// body instead of inventing a shape.
fn unrenderable_response() -> Response {
    json_response_body(
        StatusCode::SERVICE_UNAVAILABLE,
        "{\"error\":\"service_unavailable\"}",
    )
}

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;
    use axum::http::header::ALLOW;
    use weavelit_server_authorization::{
        AuthorizationCatalog, ClientModuleDeclaration, OperationDeclaration, Plane,
        ServiceModuleDeclaration, UserOperationRequest, authorize_user_operation,
    };
    use weavelit_server_database::{GroupGrant, HumanAuthorizationSnapshot, Name};

    use super::*;
    use crate::authentication::AuthenticationRejection;
    use crate::cookie::CookieEffect;

    const CORRELATION: &str = "0123456789abcdef";

    fn name(value: &str) -> Name {
        Name::new(value).expect("the test name must be accepted")
    }

    fn account(
        active: bool,
        grants: impl IntoIterator<Item = GroupGrant>,
    ) -> HumanAuthorizationSnapshot {
        HumanAuthorizationSnapshot::new(active, grants.into_iter().collect())
    }

    /// The complete set of grants and enablement an allowed request needs.
    fn allowed() -> (HumanAuthorizationSnapshot, AuthorizationCatalog) {
        let snapshot = account(
            true,
            [
                GroupGrant::ClientModule(name("web-ui")),
                GroupGrant::ServiceModule(name("zendesk")),
                GroupGrant::Operation(name("zendesk.ticket.create")),
            ],
        );
        let catalog = AuthorizationCatalog::new(
            vec![ClientModuleDeclaration::new(
                name("web-ui"),
                true,
                &[Plane::User],
            )],
            vec![ServiceModuleDeclaration::new(name("zendesk"), true)],
            vec![OperationDeclaration::new(
                name("zendesk.ticket.create"),
                name("zendesk"),
                true,
            )],
        )
        .expect("the test catalog must be accepted");

        (snapshot, catalog)
    }

    /// Renders one response into everything the listener can put on the wire.
    ///
    /// The listener derives the wire response from the status, the `Allow`
    /// header, the typed envelope, and the cookie effect, so comparing those
    /// compares the emitted bytes. The raw body is included too, because a
    /// response carrying no envelope is emitted from its body instead.
    async fn rendered(response: Response) -> Emitted {
        let status = response.status();
        let allow = response
            .headers()
            .get(ALLOW)
            .map(|value| value.as_bytes().to_vec());
        let envelope = response
            .extensions()
            .get::<TypedJsonEnvelope>()
            .map(TypedJsonEnvelope::serialize);
        let cookies = response.extensions().get::<CookieEffect>().is_some();
        let mut headers = response
            .headers()
            .iter()
            .map(|(name, value)| (name.as_str().to_owned(), value.as_bytes().to_vec()))
            .collect::<Vec<_>>();
        headers.sort();
        let body = to_bytes(response.into_body(), 4096)
            .await
            .expect("the denial body must render")
            .to_vec();

        Emitted {
            status,
            headers,
            allow,
            envelope,
            cookies,
            body,
        }
    }

    /// Everything one rendered response can put on the wire.
    #[derive(Debug, Eq, PartialEq)]
    struct Emitted {
        status: StatusCode,
        headers: Vec<(String, Vec<u8>)>,
        allow: Option<Vec<u8>>,
        envelope: Option<String>,
        cookies: bool,
        body: Vec<u8>,
    }

    #[tokio::test]
    async fn the_denial_response_is_the_documented_status_and_body() {
        let emitted = rendered(AuthorizationRejection.response(CORRELATION)).await;

        assert_eq!(emitted.status, StatusCode::FORBIDDEN);
        assert_eq!(
            emitted.envelope.as_deref(),
            Some(r#"{"error":"authorization_denied","correlation_id":"0123456789abcdef"}"#)
        );
        // A denial sets no header of its own and carries no cookie effect, so
        // being denied one Operation cannot end or re-establish the session.
        assert_eq!(emitted.allow, None);
        assert!(!emitted.cookies);
        assert!(emitted.headers.is_empty());
    }

    /// Drives every real denial cause through the evaluator and compares the
    /// rendered responses byte for byte, rather than field by field, so a
    /// header, a status, or a body difference of any kind fails.
    #[tokio::test]
    async fn every_denial_cause_renders_byte_identical_bytes() {
        let request = |catalog: &AuthorizationCatalog, snapshot: &HumanAuthorizationSnapshot| {
            authorize_user_operation(
                snapshot,
                catalog,
                UserOperationRequest {
                    client_module: &name("web-ui"),
                    service_module: &name("zendesk"),
                    operation: &name("zendesk.ticket.create"),
                },
            )
        };

        let (snapshot, catalog) = allowed();
        request(&catalog, &snapshot).expect("the fully granted request must be allowed");

        let inactive = account(
            false,
            [
                GroupGrant::ClientModule(name("web-ui")),
                GroupGrant::ServiceModule(name("zendesk")),
                GroupGrant::Operation(name("zendesk.ticket.create")),
            ],
        );
        let without_client_module = account(
            true,
            [
                GroupGrant::ServiceModule(name("zendesk")),
                GroupGrant::Operation(name("zendesk.ticket.create")),
            ],
        );
        let without_service_module = account(
            true,
            [
                GroupGrant::ClientModule(name("web-ui")),
                GroupGrant::Operation(name("zendesk.ticket.create")),
            ],
        );
        let without_operation = account(
            true,
            [
                GroupGrant::ClientModule(name("web-ui")),
                GroupGrant::ServiceModule(name("zendesk")),
            ],
        );

        let disabled_client_module = AuthorizationCatalog::new(
            vec![ClientModuleDeclaration::new(
                name("web-ui"),
                false,
                &[Plane::User],
            )],
            vec![ServiceModuleDeclaration::new(name("zendesk"), true)],
            vec![OperationDeclaration::new(
                name("zendesk.ticket.create"),
                name("zendesk"),
                true,
            )],
        )
        .expect("the test catalog must be accepted");
        let disabled_service_module = AuthorizationCatalog::new(
            vec![ClientModuleDeclaration::new(
                name("web-ui"),
                true,
                &[Plane::User],
            )],
            vec![ServiceModuleDeclaration::new(name("zendesk"), false)],
            vec![OperationDeclaration::new(
                name("zendesk.ticket.create"),
                name("zendesk"),
                true,
            )],
        )
        .expect("the test catalog must be accepted");
        let disabled_operation = AuthorizationCatalog::new(
            vec![ClientModuleDeclaration::new(
                name("web-ui"),
                true,
                &[Plane::User],
            )],
            vec![ServiceModuleDeclaration::new(name("zendesk"), true)],
            vec![OperationDeclaration::new(
                name("zendesk.ticket.create"),
                name("zendesk"),
                false,
            )],
        )
        .expect("the test catalog must be accepted");
        let unowned_operation = AuthorizationCatalog::new(
            vec![ClientModuleDeclaration::new(
                name("web-ui"),
                true,
                &[Plane::User],
            )],
            vec![
                ServiceModuleDeclaration::new(name("zendesk"), true),
                ServiceModuleDeclaration::new(name("other"), true),
            ],
            vec![OperationDeclaration::new(
                name("zendesk.ticket.create"),
                name("other"),
                true,
            )],
        )
        .expect("the test catalog must be accepted");

        let causes: [(&str, &HumanAuthorizationSnapshot, &AuthorizationCatalog); 8] = [
            ("inactive account", &inactive, &catalog),
            (
                "missing Client Module grant",
                &without_client_module,
                &catalog,
            ),
            (
                "missing Service Module grant",
                &without_service_module,
                &catalog,
            ),
            ("missing Operation grant", &without_operation, &catalog),
            ("disabled Client Module", &snapshot, &disabled_client_module),
            (
                "disabled Service Module",
                &snapshot,
                &disabled_service_module,
            ),
            ("disabled Operation", &snapshot, &disabled_operation),
            ("unowned Operation", &snapshot, &unowned_operation),
        ];

        let mut baseline = None;
        for (cause, account, declared) in causes {
            request(declared, account)
                .expect_err(&format!("{cause} must be denied by the evaluator"));
            let rendered = rendered(AuthorizationRejection.response(CORRELATION)).await;
            match &baseline {
                None => baseline = Some(rendered),
                Some(expected) => assert_eq!(
                    &rendered, expected,
                    "{cause} rendered a distinguishable response"
                ),
            }
        }
        assert!(baseline.is_some());
    }

    /// Only the correlation identifier varies, and it carries no cause.
    #[tokio::test]
    async fn only_the_correlation_identifier_varies_between_two_denials() {
        let first = rendered(AuthorizationRejection.response("aaaaaaaaaaaaaaaa")).await;
        let second = rendered(AuthorizationRejection.response("bbbbbbbbbbbbbbbb")).await;

        assert_eq!(first.status, second.status);
        assert_eq!(first.headers, second.headers);
        assert_eq!(first.allow, second.allow);
        assert_eq!(first.cookies, second.cookies);
        assert_ne!(first.envelope, second.envelope);

        let first_envelope = first.envelope.unwrap();
        let second_envelope = second.envelope.unwrap();
        assert_eq!(
            first_envelope.replace("aaaaaaaaaaaaaaaa", "bbbbbbbbbbbbbbbb"),
            second_envelope
        );
    }

    /// The authorization and authentication contracts must stay distinct, so a
    /// client cannot read one as the other and retry a login it does not need.
    #[tokio::test]
    async fn the_authorization_contract_does_not_collide_with_authentication() {
        assert_eq!(AuthorizationRejection.status(), StatusCode::FORBIDDEN);
        assert_eq!(AuthorizationRejection.code(), "authorization_denied");
        assert_eq!(
            AuthenticationRejection::SessionInvalid.status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            AuthenticationRejection::SessionInvalid.code(),
            "session_invalid"
        );

        for rejection in [
            AuthenticationRejection::BadRequest,
            AuthenticationRejection::AuthenticationFailed,
            AuthenticationRejection::SessionInvalid,
            AuthenticationRejection::RequestOriginDenied,
            AuthenticationRejection::MethodNotAllowed,
            AuthenticationRejection::ServiceUnavailable,
        ] {
            assert_ne!(
                rejection.code(),
                AuthorizationRejection.code(),
                "{rejection} reuses the authorization code"
            );
            let authentication = rendered(rejection.response(CORRELATION)).await;
            let authorization = rendered(AuthorizationRejection.response(CORRELATION)).await;
            assert_ne!(
                authentication, authorization,
                "{rejection} renders the authorization denial"
            );
        }
    }

    /// A correlation identifier the Server could not render must not turn a
    /// denial into an allow.
    #[tokio::test]
    async fn an_unrenderable_correlation_identifier_still_denies() {
        for identifier in ["", "NOT LOWERCASE", &"a".repeat(65)] {
            let emitted = rendered(AuthorizationRejection.response(identifier)).await;

            assert_eq!(emitted.status, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(emitted.envelope, None);
            assert_eq!(emitted.body, br#"{"error":"service_unavailable"}"#);
        }
    }
}
