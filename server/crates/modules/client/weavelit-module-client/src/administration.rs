//! Shared Client Module contract for account administration.

use std::{fmt, io, pin::Pin, sync::Arc};

use axum::{
    body::to_bytes,
    extract::Request,
    http::{
        Extensions, HeaderMap, HeaderValue, Method, StatusCode,
        header::{ALLOW, CONTENT_TYPE},
    },
    response::Response,
    routing::{MethodRouter, any},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{
    Serialize,
    de::{self, Deserialize, Deserializer, MapAccess, Visitor},
};
use zeroize::Zeroizing;

use crate::{
    ExpectedOrigin, JSON_MEDIA_TYPE, accepts_json,
    authentication::{
        CorrelationSource, submitted_csrf_token, submitted_session_token, unrenderable_response,
    },
    has_request_body, single_header,
    typed_json::{ResponseCorrelation, StableCode, TypedJsonEnvelope, typed_json_response},
};

/// Canonical route that lists bounded public account projections.
pub const ACCOUNTS_LIST_ROUTE: &str = "/api/v1/administration/accounts/list";

/// Canonical route that loads one public account projection.
pub const ACCOUNTS_VIEW_ROUTE: &str = "/api/v1/administration/accounts/view";

/// Canonical route that changes one account's active status.
pub const ACCOUNTS_STATUS_ROUTE: &str = "/api/v1/administration/accounts/status";

/// Default number of accounts returned by a list request.
pub const DEFAULT_ACCOUNTS_PAGE_LIMIT: usize = 50;

/// Largest number of accounts returned by a list request.
pub const MAX_ACCOUNTS_PAGE_LIMIT: usize = 100;

/// Largest request body accepted by either account administration route.
pub const MAX_ACCOUNT_ADMINISTRATION_BODY_BYTES: usize = 512;

const LIST_CURSOR_SCOPE: &[u8] = b"weavelit:/api/v1/administration/accounts/list:v1\0";
const MAX_CURSOR_POSITION_BYTES: usize = 256;
const MAX_LIST_CURSOR_BYTES: usize =
    ((LIST_CURSOR_SCOPE.len() + MAX_CURSOR_POSITION_BYTES) * 4).div_ceil(3);
const LIST_FIELDS: &[&str] = &["limit", "cursor"];
const VIEW_FIELDS: &[&str] = &["public_id"];
const STATUS_FIELDS: &[&str] = &["public_id", "active"];
const ACCOUNT_PUBLIC_ID_BYTES: usize = 16;
const ACCOUNT_PUBLIC_ID_BASE64URL_CHARS: usize = 22;
const MAX_ACCOUNT_NAME_BYTES: usize = 256;
const MAX_ACCOUNT_PROJECTION_JSON_BYTES: usize = 160 + (4 * MAX_ACCOUNT_NAME_BYTES);

/// Largest serialized typed account-administration envelope.
pub const MAX_ACCOUNT_ADMINISTRATION_RESPONSE_BYTES: usize = 192
    + (MAX_ACCOUNTS_PAGE_LIMIT * (MAX_ACCOUNT_PROJECTION_JSON_BYTES + 1))
    + MAX_LIST_CURSOR_BYTES;

/// Strictly validated paging request for the account collection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountsListRequest {
    limit: usize,
    after_username: Option<String>,
}

impl AccountsListRequest {
    /// Parses an absent body or the documented optional paging object.
    pub fn from_optional_json(body: &[u8]) -> Result<Self, AccountAdministrationInputRejected> {
        if body.len() > MAX_ACCOUNT_ADMINISTRATION_BODY_BYTES {
            return Err(AccountAdministrationInputRejected);
        }
        if body.is_empty() {
            return Ok(Self {
                limit: DEFAULT_ACCOUNTS_PAGE_LIMIT,
                after_username: None,
            });
        }

        let mut deserializer = serde_json::Deserializer::from_slice(body);
        let parsed = AccountsListBody::deserialize(&mut deserializer)
            .map_err(|_| AccountAdministrationInputRejected)?;
        deserializer
            .end()
            .map_err(|_| AccountAdministrationInputRejected)?;
        let limit = parsed.limit.unwrap_or(DEFAULT_ACCOUNTS_PAGE_LIMIT);
        if !(1..=MAX_ACCOUNTS_PAGE_LIMIT).contains(&limit) {
            return Err(AccountAdministrationInputRejected);
        }
        let after_username = parsed
            .cursor
            .map(|cursor| decode_list_cursor(&cursor))
            .transpose()?;
        Ok(Self {
            limit,
            after_username,
        })
    }

    /// Returns the requested page size after applying the default.
    #[must_use]
    pub const fn limit(&self) -> usize {
        self.limit
    }

    /// Returns the immutable username carried by a validated cursor.
    #[must_use]
    pub fn after_username(&self) -> Option<&str> {
        self.after_username.as_deref()
    }
}

struct AccountsListBody {
    limit: Option<usize>,
    cursor: Option<String>,
}

/// Strictly validated exact account-view request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountsViewRequest {
    public_id: String,
}

impl AccountsViewRequest {
    /// Parses exactly `{"public_id":"<22 character Base64url>"}`.
    pub fn from_json(body: &[u8]) -> Result<Self, AccountAdministrationInputRejected> {
        if body.is_empty() || body.len() > MAX_ACCOUNT_ADMINISTRATION_BODY_BYTES {
            return Err(AccountAdministrationInputRejected);
        }
        let mut deserializer = serde_json::Deserializer::from_slice(body);
        let parsed = AccountsViewBody::deserialize(&mut deserializer)
            .map_err(|_| AccountAdministrationInputRejected)?;
        deserializer
            .end()
            .map_err(|_| AccountAdministrationInputRejected)?;
        if !valid_public_id(&parsed.public_id) {
            return Err(AccountAdministrationInputRejected);
        }
        Ok(Self {
            public_id: parsed.public_id,
        })
    }

    /// Returns the canonical unpadded Base64url account identifier.
    #[must_use]
    pub fn public_id(&self) -> &str {
        &self.public_id
    }
}

struct AccountsViewBody {
    public_id: String,
}

/// Strictly validated account-status request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountsStatusRequest {
    public_id: String,
    active: bool,
}

impl AccountsStatusRequest {
    /// Parses exactly `{"public_id":"<22 character Base64url>","active":<boolean>}`.
    pub fn from_json(body: &[u8]) -> Result<Self, AccountAdministrationInputRejected> {
        if body.is_empty() || body.len() > MAX_ACCOUNT_ADMINISTRATION_BODY_BYTES {
            return Err(AccountAdministrationInputRejected);
        }
        let mut deserializer = serde_json::Deserializer::from_slice(body);
        let parsed = AccountsStatusBody::deserialize(&mut deserializer)
            .map_err(|_| AccountAdministrationInputRejected)?;
        deserializer
            .end()
            .map_err(|_| AccountAdministrationInputRejected)?;
        if !valid_public_id(&parsed.public_id) {
            return Err(AccountAdministrationInputRejected);
        }
        Ok(Self {
            public_id: parsed.public_id,
            active: parsed.active,
        })
    }

    /// Returns the canonical unpadded Base64url account identifier.
    #[must_use]
    pub fn public_id(&self) -> &str {
        &self.public_id
    }

    /// Returns the requested active state.
    #[must_use]
    pub const fn active(&self) -> bool {
        self.active
    }
}

struct AccountsStatusBody {
    public_id: String,
    active: bool,
}

impl<'de> Deserialize<'de> for AccountsStatusBody {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BodyVisitor;

        impl<'de> Visitor<'de> for BodyVisitor {
            type Value = AccountsStatusBody;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an exact account status object")
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut public_id = None;
                let mut active = None;
                while let Some(field) = map.next_key::<String>()? {
                    match field.as_str() {
                        "public_id" => {
                            if public_id.is_some() {
                                return Err(de::Error::duplicate_field("public_id"));
                            }
                            public_id = Some(map.next_value()?);
                        }
                        "active" => {
                            if active.is_some() {
                                return Err(de::Error::duplicate_field("active"));
                            }
                            active = Some(map.next_value()?);
                        }
                        unknown => return Err(de::Error::unknown_field(unknown, STATUS_FIELDS)),
                    }
                }
                Ok(AccountsStatusBody {
                    public_id: public_id.ok_or_else(|| de::Error::missing_field("public_id"))?,
                    active: active.ok_or_else(|| de::Error::missing_field("active"))?,
                })
            }
        }

        deserializer.deserialize_map(BodyVisitor)
    }
}

impl<'de> Deserialize<'de> for AccountsViewBody {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BodyVisitor;

        impl<'de> Visitor<'de> for BodyVisitor {
            type Value = AccountsViewBody;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an exact account view object")
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut public_id = None;
                while let Some(field) = map.next_key::<String>()? {
                    match field.as_str() {
                        "public_id" => {
                            if public_id.is_some() {
                                return Err(de::Error::duplicate_field("public_id"));
                            }
                            public_id = Some(map.next_value()?);
                        }
                        unknown => return Err(de::Error::unknown_field(unknown, VIEW_FIELDS)),
                    }
                }
                Ok(AccountsViewBody {
                    public_id: public_id.ok_or_else(|| de::Error::missing_field("public_id"))?,
                })
            }
        }

        deserializer.deserialize_map(BodyVisitor)
    }
}

impl<'de> Deserialize<'de> for AccountsListBody {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BodyVisitor;

        impl<'de> Visitor<'de> for BodyVisitor {
            type Value = AccountsListBody;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an account list paging object")
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut limit = None;
                let mut cursor = None;
                while let Some(field) = map.next_key::<String>()? {
                    match field.as_str() {
                        "limit" => {
                            if limit.is_some() {
                                return Err(de::Error::duplicate_field("limit"));
                            }
                            limit = Some(map.next_value()?);
                        }
                        "cursor" => {
                            if cursor.is_some() {
                                return Err(de::Error::duplicate_field("cursor"));
                            }
                            cursor = Some(map.next_value()?);
                        }
                        unknown => return Err(de::Error::unknown_field(unknown, LIST_FIELDS)),
                    }
                }
                Ok(AccountsListBody { limit, cursor })
            }
        }

        deserializer.deserialize_map(BodyVisitor)
    }
}

/// Payload-free refusal of an account administration request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccountAdministrationInputRejected;

/// One validated account administration read intent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccountAdministrationRequest {
    /// List one bounded page in canonical username order.
    List(AccountsListRequest),
    /// View one exact public identifier.
    View(AccountsViewRequest),
    /// Change one exact public identifier to the requested active state.
    Status(AccountsStatusRequest),
}

/// Validated session-bearing submission handed to trusted Server composition.
pub struct AccountAdministrationSubmission {
    /// The requested account read.
    pub request: AccountAdministrationRequest,
    /// Session bearer read only from the approved cookie.
    pub session_token: Zeroizing<String>,
    /// CSRF value echoed from the readable session cookie.
    pub csrf_token: Zeroizing<String>,
    /// Server-generated correlation identifier for this request.
    pub correlation_id: String,
    /// Listener-owned admitted request context.
    pub context: Extensions,
}

/// One account's complete public administration projection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AccountAdministrationProjection {
    public_id: String,
    username: String,
    display_name: Option<String>,
    active: bool,
    mfa_required: bool,
}

impl AccountAdministrationProjection {
    /// Builds the fixed safe projection accepted by the public envelope.
    pub fn new(
        public_id: String,
        username: String,
        display_name: Option<String>,
        active: bool,
        mfa_required: bool,
    ) -> Result<Self, AccountAdministrationInputRejected> {
        if !valid_public_id(&public_id)
            || !valid_account_name(&username)
            || display_name
                .as_deref()
                .is_some_and(|name| !valid_account_name(name))
        {
            return Err(AccountAdministrationInputRejected);
        }
        Ok(Self {
            public_id,
            username,
            display_name,
            active,
            mfa_required,
        })
    }

    /// Returns the immutable unique username used for page ordering.
    #[must_use]
    pub fn username(&self) -> &str {
        &self.username
    }

    /// Returns the stable public identifier used as the member-page tiebreaker.
    #[must_use]
    pub fn public_id(&self) -> &str {
        &self.public_id
    }
}

/// One deterministic bounded account collection page.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AccountsPage {
    items: Vec<AccountAdministrationProjection>,
    next_cursor: Option<String>,
}

impl AccountsPage {
    /// Slices strictly ordered projections after the request's exact keyset position.
    pub fn from_ordered(
        request: &AccountsListRequest,
        projections: Vec<AccountAdministrationProjection>,
    ) -> Result<Self, AccountAdministrationInputRejected> {
        if projections
            .windows(2)
            .any(|pair| pair[0].username() >= pair[1].username())
        {
            return Err(AccountAdministrationInputRejected);
        }
        let start = match request.after_username() {
            Some(after) => {
                projections
                    .binary_search_by(|projection| projection.username().cmp(after))
                    .map_err(|_| AccountAdministrationInputRejected)?
                    + 1
            }
            None => 0,
        };
        let end = start.saturating_add(request.limit()).min(projections.len());
        let next_cursor = if end < projections.len() {
            projections
                .get(end - 1)
                .map(|projection| encode_list_cursor(projection.username()))
                .transpose()?
        } else {
            None
        };
        Ok(Self {
            items: projections[start..end].to_vec(),
            next_cursor,
        })
    }
}

/// Successful result carried directly in the typed response envelope.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum AccountAdministrationResult {
    /// Account collection page.
    List(AccountsPage),
    /// One exact account projection.
    View(AccountAdministrationProjection),
    /// One account projection after a successful status request.
    Status(AccountAdministrationProjection),
}

/// Complete public account-administration rejection vocabulary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountAdministrationRejection {
    /// Malformed headers, body, schema, target identifier, or cursor.
    BadRequest,
    /// Missing, malformed, unknown, expired, or mismatched session.
    SessionInvalid,
    /// Failed exact `Origin` or `Host` check.
    RequestOriginDenied,
    /// Live Administration Plane authorization denied.
    AuthorizationDenied,
    /// Method other than `PUT`.
    MethodNotAllowed,
    /// Exact public account identifier was not found.
    NotFound,
    /// Persistence, composition, or response construction failed.
    ServiceUnavailable,
}

impl AccountAdministrationRejection {
    /// Returns the stable HTTP status.
    #[must_use]
    pub const fn status(self) -> StatusCode {
        match self {
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::SessionInvalid => StatusCode::UNAUTHORIZED,
            Self::RequestOriginDenied | Self::AuthorizationDenied => StatusCode::FORBIDDEN,
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::ServiceUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    /// Returns the stable payload-free code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::BadRequest => "bad_request",
            Self::SessionInvalid => "session_invalid",
            Self::RequestOriginDenied => "request_origin_denied",
            Self::AuthorizationDenied => "authorization_denied",
            Self::MethodNotAllowed => "method_not_allowed",
            Self::NotFound => "not_found",
            Self::ServiceUnavailable => "service_unavailable",
        }
    }

    /// Builds the typed rejection envelope.
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

/// Server-core hook that validates, authorizes, consumes, and executes one account request.
pub type AccountAdministrationCommit = Arc<
    dyn Fn(
            AccountAdministrationSubmission,
        ) -> Pin<
            Box<
                dyn Future<
                        Output = Result<
                            AccountAdministrationResult,
                            AccountAdministrationRejection,
                        >,
                    > + Send,
            >,
        > + Send
        + Sync,
>;

/// Runtime collaborators a Client Module declares account administration with.
pub struct AccountAdministrationCapability {
    /// Trusted listener authority.
    pub expected_origin: ExpectedOrigin,
    /// Server-owned correlation source.
    pub correlate: CorrelationSource,
    /// Trusted Server execution hook.
    pub execute: AccountAdministrationCommit,
}

/// Declared account-administration capability split into its canonical routes.
pub struct AccountAdministrationDeclaration {
    capability: Arc<AccountAdministrationCapability>,
}

impl AccountAdministrationDeclaration {
    /// Declares account administration over supplied Server collaborators.
    #[must_use]
    pub fn new(capability: AccountAdministrationCapability) -> Self {
        Self {
            capability: Arc::new(capability),
        }
    }

    /// Returns the canonical account-list route.
    pub fn list_route(&self) -> MethodRouter {
        let capability = Arc::clone(&self.capability);
        any(move |request| {
            account_administration_response(request, Arc::clone(&capability), AccountRoute::List)
        })
    }

    /// Returns the canonical account-view route.
    pub fn view_route(&self) -> MethodRouter {
        let capability = Arc::clone(&self.capability);
        any(move |request| {
            account_administration_response(request, Arc::clone(&capability), AccountRoute::View)
        })
    }

    /// Returns the canonical account-status route.
    pub fn status_route(&self) -> MethodRouter {
        let capability = Arc::clone(&self.capability);
        any(move |request| {
            account_administration_response(request, Arc::clone(&capability), AccountRoute::Status)
        })
    }
}

#[derive(Clone, Copy)]
enum AccountRoute {
    List,
    View,
    Status,
}

/// Route-specific typed envelope serialized by the listener under its own bound.
#[derive(Clone)]
pub struct AccountAdministrationEnvelope {
    result: AccountAdministrationResult,
    correlation_id: String,
}

impl AccountAdministrationEnvelope {
    /// Serializes only the documented result envelope into a fixed-capacity owner.
    #[must_use]
    pub fn serialize(&self) -> Option<Zeroizing<String>> {
        #[derive(Serialize)]
        struct WireEnvelope<'a> {
            result: &'a AccountAdministrationResult,
            correlation_id: &'a str,
        }

        let mut writer = BoundedJsonWriter::new();
        serde_json::to_writer(
            &mut writer,
            &WireEnvelope {
                result: &self.result,
                correlation_id: &self.correlation_id,
            },
        )
        .ok()?;
        let mut bytes = writer.into_bytes();
        String::from_utf8(std::mem::take(&mut *bytes))
            .ok()
            .map(Zeroizing::new)
    }
}

struct BoundedJsonWriter {
    bytes: Zeroizing<Vec<u8>>,
}

impl BoundedJsonWriter {
    fn new() -> Self {
        Self {
            bytes: Zeroizing::new(Vec::with_capacity(
                MAX_ACCOUNT_ADMINISTRATION_RESPONSE_BYTES,
            )),
        }
    }

    fn into_bytes(self) -> Zeroizing<Vec<u8>> {
        self.bytes
    }
}

impl io::Write for BoundedJsonWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self.bytes.len().saturating_add(buffer.len()) > MAX_ACCOUNT_ADMINISTRATION_RESPONSE_BYTES
        {
            return Err(io::Error::other("account response exceeds its bound"));
        }
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

async fn account_administration_response(
    request: Request,
    capability: Arc<AccountAdministrationCapability>,
    route: AccountRoute,
) -> Response {
    let Some(correlation_id) = (capability.correlate)() else {
        return unrenderable_response();
    };
    let (parts, body) = request.into_parts();
    if let Err(rejection) = validate_request_head(
        &parts.method,
        &parts.headers,
        capability.expected_origin,
        !matches!(route, AccountRoute::List),
    ) {
        return rejection.response(&correlation_id);
    }
    let Ok(body) = to_bytes(body, MAX_ACCOUNT_ADMINISTRATION_BODY_BYTES).await else {
        return AccountAdministrationRejection::BadRequest.response(&correlation_id);
    };
    let parsed = match route {
        AccountRoute::List => {
            AccountsListRequest::from_optional_json(&body).map(AccountAdministrationRequest::List)
        }
        AccountRoute::View => {
            AccountsViewRequest::from_json(&body).map(AccountAdministrationRequest::View)
        }
        AccountRoute::Status => {
            AccountsStatusRequest::from_json(&body).map(AccountAdministrationRequest::Status)
        }
    };
    let Ok(request) = parsed else {
        return AccountAdministrationRejection::BadRequest.response(&correlation_id);
    };
    let session_token = match submitted_session_token(&parts.headers) {
        Ok(value) => Zeroizing::new(value.to_owned()),
        Err(_) => {
            return AccountAdministrationRejection::SessionInvalid.response(&correlation_id);
        }
    };
    let csrf_token = match submitted_csrf_token(&parts.headers) {
        Ok(value) => Zeroizing::new(value.to_owned()),
        Err(_) => {
            return AccountAdministrationRejection::SessionInvalid.response(&correlation_id);
        }
    };

    match (capability.execute)(AccountAdministrationSubmission {
        request,
        session_token,
        csrf_token,
        correlation_id: correlation_id.clone(),
        context: parts.extensions,
    })
    .await
    {
        Ok(result)
            if matches!(
                (route, &result),
                (AccountRoute::List, AccountAdministrationResult::List(_))
                    | (AccountRoute::View, AccountAdministrationResult::View(_))
                    | (AccountRoute::Status, AccountAdministrationResult::Status(_))
            ) =>
        {
            account_administration_success(result, correlation_id)
        }
        Ok(_) => AccountAdministrationRejection::ServiceUnavailable.response(&correlation_id),
        Err(rejection) => rejection.response(&correlation_id),
    }
}

fn validate_request_head(
    method: &Method,
    headers: &HeaderMap,
    expected_origin: ExpectedOrigin,
    body_required: bool,
) -> Result<(), AccountAdministrationRejection> {
    if method != Method::PUT {
        return Err(AccountAdministrationRejection::MethodNotAllowed);
    }
    if !expected_origin.is_same_origin(headers) {
        return Err(AccountAdministrationRejection::RequestOriginDenied);
    }
    if !accepts_json(headers) {
        return Err(AccountAdministrationRejection::BadRequest);
    }
    if body_required || has_request_body(headers) {
        let content_type = single_header(headers, CONTENT_TYPE)
            .ok_or(AccountAdministrationRejection::BadRequest)?;
        if content_type.as_bytes() != JSON_MEDIA_TYPE {
            return Err(AccountAdministrationRejection::BadRequest);
        }
    } else if single_header(headers, CONTENT_TYPE).is_some() {
        return Err(AccountAdministrationRejection::BadRequest);
    }
    submitted_session_token(headers)
        .and_then(|_| submitted_csrf_token(headers))
        .map_err(|_| AccountAdministrationRejection::SessionInvalid)?;
    Ok(())
}

pub(crate) fn account_administration_success(
    result: AccountAdministrationResult,
    correlation_id: String,
) -> Response {
    if ResponseCorrelation::new(&correlation_id).is_none() {
        return unrenderable_response();
    }
    let mut response = Response::new(axum::body::Body::empty());
    *response.status_mut() = StatusCode::OK;
    response
        .extensions_mut()
        .insert(AccountAdministrationEnvelope {
            result,
            correlation_id,
        });
    response
}

/// Encodes the last immutable username in one route/version-scoped cursor.
pub(crate) fn encode_list_cursor(
    username: &str,
) -> Result<String, AccountAdministrationInputRejected> {
    if !valid_cursor_position(username) {
        return Err(AccountAdministrationInputRejected);
    }
    let mut scoped = Vec::with_capacity(LIST_CURSOR_SCOPE.len() + username.len());
    scoped.extend_from_slice(LIST_CURSOR_SCOPE);
    scoped.extend_from_slice(username.as_bytes());
    Ok(URL_SAFE_NO_PAD.encode(scoped))
}

fn decode_list_cursor(cursor: &str) -> Result<String, AccountAdministrationInputRejected> {
    if cursor.is_empty() || cursor.len() > MAX_LIST_CURSOR_BYTES {
        return Err(AccountAdministrationInputRejected);
    }
    let decoded = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| AccountAdministrationInputRejected)?;
    if URL_SAFE_NO_PAD.encode(&decoded) != cursor {
        return Err(AccountAdministrationInputRejected);
    }
    let position = decoded
        .strip_prefix(LIST_CURSOR_SCOPE)
        .ok_or(AccountAdministrationInputRejected)?;
    let position = std::str::from_utf8(position).map_err(|_| AccountAdministrationInputRejected)?;
    if !valid_cursor_position(position) {
        return Err(AccountAdministrationInputRejected);
    }
    Ok(position.to_owned())
}

fn valid_cursor_position(position: &str) -> bool {
    !position.is_empty()
        && position.len() <= MAX_CURSOR_POSITION_BYTES
        && !position.chars().any(char::is_control)
}

fn valid_public_id(value: &str) -> bool {
    if value.len() != ACCOUNT_PUBLIC_ID_BASE64URL_CHARS {
        return false;
    }
    URL_SAFE_NO_PAD.decode(value).is_ok_and(|decoded| {
        decoded.len() == ACCOUNT_PUBLIC_ID_BYTES
            && decoded.iter().any(|byte| *byte != 0)
            && URL_SAFE_NO_PAD.encode(decoded) == value
    })
}

fn valid_account_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ACCOUNT_NAME_BYTES
        && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::Body,
        http::{
            Request as HttpRequest,
            header::{ACCEPT, COOKIE, HOST, ORIGIN},
        },
        routing::any,
    };
    use tower::ServiceExt as _;

    use super::*;
    use crate::{CSRF_COOKIE_NAME, CSRF_HEADER_NAME, OperationalSurface, SESSION_COOKIE_NAME};

    const LISTENER: &str = "127.0.0.1:8443";
    const CORRELATION: &str = "correlation-0123456789";
    const SESSION: &str = "session-token";
    const CSRF: &str = "csrf-token";

    fn public_id(byte: u8) -> String {
        URL_SAFE_NO_PAD.encode([byte; ACCOUNT_PUBLIC_ID_BYTES])
    }

    fn projection(username: &str) -> AccountAdministrationProjection {
        AccountAdministrationProjection::new(
            public_id(username.as_bytes()[0]),
            username.to_owned(),
            Some(format!("Display {username}")),
            true,
            false,
        )
        .unwrap()
    }

    fn capability(
        outcome: Result<AccountAdministrationResult, AccountAdministrationRejection>,
    ) -> AccountAdministrationCapability {
        AccountAdministrationCapability {
            expected_origin: ExpectedOrigin::from_listener(LISTENER.parse().unwrap()),
            correlate: Arc::new(|| Some(CORRELATION.to_owned())),
            execute: Arc::new(move |_| {
                let outcome = outcome.clone();
                Box::pin(async move { outcome })
            }),
        }
    }

    fn request(method: Method, target: &str, body: &str) -> HttpRequest<Body> {
        let mut builder = HttpRequest::builder()
            .method(method)
            .uri(target)
            .header(HOST, LISTENER)
            .header(ORIGIN, format!("https://{LISTENER}"))
            .header(ACCEPT, "application/json")
            .header(CSRF_HEADER_NAME, CSRF)
            .header(
                COOKIE,
                format!("{SESSION_COOKIE_NAME}={SESSION}; {CSRF_COOKIE_NAME}={CSRF}"),
            );
        if !body.is_empty() {
            builder = builder
                .header(CONTENT_TYPE, "application/json")
                .header("content-length", body.len().to_string());
        }
        builder.body(Body::from(body.to_owned())).unwrap()
    }

    async fn rendered(response: Response) -> String {
        if let Some(envelope) = response.extensions().get::<AccountAdministrationEnvelope>() {
            return envelope.serialize().unwrap().to_string();
        }
        if let Some(envelope) = response.extensions().get::<TypedJsonEnvelope>() {
            return envelope.serialize().to_string();
        }
        String::from_utf8(
            to_bytes(
                response.into_body(),
                MAX_ACCOUNT_ADMINISTRATION_RESPONSE_BYTES,
            )
            .await
            .unwrap()
            .to_vec(),
        )
        .unwrap()
    }

    #[test]
    fn list_request_accepts_absent_and_optional_paging_members() {
        assert_eq!(
            AccountsListRequest::from_optional_json(b"").unwrap(),
            AccountsListRequest {
                limit: DEFAULT_ACCOUNTS_PAGE_LIMIT,
                after_username: None,
            }
        );
        assert_eq!(
            AccountsListRequest::from_optional_json(br#"{}"#).unwrap(),
            AccountsListRequest {
                limit: DEFAULT_ACCOUNTS_PAGE_LIMIT,
                after_username: None,
            }
        );
        assert_eq!(
            AccountsListRequest::from_optional_json(br#"{"limit":100}"#)
                .unwrap()
                .limit(),
            MAX_ACCOUNTS_PAGE_LIMIT
        );

        let cursor = encode_list_cursor("administrator").unwrap();
        let body = format!(r#"{{"cursor":"{cursor}","limit":1}}"#);
        let request = AccountsListRequest::from_optional_json(body.as_bytes()).unwrap();
        assert_eq!(request.limit(), 1);
        assert_eq!(request.after_username(), Some("administrator"));
    }

    #[test]
    fn list_request_rejects_every_undocumented_or_unbounded_shape() {
        for body in [
            br#"[]"#.as_slice(),
            br#"null"#,
            br#"{"limit":0}"#,
            br#"{"limit":101}"#,
            br#"{"limit":"1"}"#,
            br#"{"cursor":null}"#,
            br#"{"extra":true}"#,
            br#"{"limit":1,"limit":2}"#,
            br#"{} trailing"#,
        ] {
            assert_eq!(
                AccountsListRequest::from_optional_json(body),
                Err(AccountAdministrationInputRejected),
                "{}",
                String::from_utf8_lossy(body)
            );
        }
        assert_eq!(
            AccountsListRequest::from_optional_json(&vec![
                b' ';
                MAX_ACCOUNT_ADMINISTRATION_BODY_BYTES + 1
            ]),
            Err(AccountAdministrationInputRejected)
        );
    }

    #[test]
    fn list_cursor_is_canonical_scoped_and_bounded() {
        let cursor = encode_list_cursor("zoe").unwrap();
        assert_eq!(decode_list_cursor(&cursor).unwrap(), "zoe");
        assert!(!cursor.contains(['+', '/', '=']));

        let wrong_scope =
            URL_SAFE_NO_PAD.encode(b"weavelit:/api/v1/administration/groups/list:v1\0zoe");
        for rejected in [
            String::new(),
            "not*base64".to_owned(),
            format!("{cursor}="),
            wrong_scope,
            URL_SAFE_NO_PAD.encode(LIST_CURSOR_SCOPE),
            "a".repeat(MAX_LIST_CURSOR_BYTES + 1),
        ] {
            assert_eq!(
                decode_list_cursor(&rejected),
                Err(AccountAdministrationInputRejected),
                "{rejected}"
            );
        }
        assert_eq!(
            encode_list_cursor(&"a".repeat(MAX_CURSOR_POSITION_BYTES + 1)),
            Err(AccountAdministrationInputRejected)
        );
        assert!(
            encode_list_cursor(&"a".repeat(MAX_CURSOR_POSITION_BYTES))
                .unwrap()
                .len()
                <= MAX_LIST_CURSOR_BYTES
        );
    }

    #[test]
    fn view_request_accepts_only_one_canonical_nonzero_public_identifier() {
        let identifier = public_id(0x41);
        let request =
            AccountsViewRequest::from_json(format!(r#"{{"public_id":"{identifier}"}}"#).as_bytes())
                .unwrap();
        assert_eq!(request.public_id(), identifier);

        for body in [
            br#"{}"#.as_slice(),
            br#"[]"#,
            br#"{"public_id":null}"#,
            br#"{"public_id":"short"}"#,
            format!(r#"{{"public_id":"{}"}}"#, public_id(0)).as_bytes(),
            format!(r#"{{"public_id":"{identifier}","public_id":"{identifier}"}}"#).as_bytes(),
            format!(r#"{{"public_id":"{identifier}","account_id":"secret"}}"#).as_bytes(),
        ] {
            assert_eq!(
                AccountsViewRequest::from_json(body),
                Err(AccountAdministrationInputRejected)
            );
        }
    }

    #[test]
    fn status_request_accepts_only_an_exact_public_identifier_and_boolean() {
        let identifier = public_id(0x41);
        let request = AccountsStatusRequest::from_json(
            format!(r#"{{"public_id":"{identifier}","active":false}}"#).as_bytes(),
        )
        .unwrap();
        assert_eq!(request.public_id(), identifier);
        assert!(!request.active());

        for body in [
            br#"{}"#.as_slice(),
            br#"[]"#,
            br#"{"public_id":null,"active":false}"#,
            br#"{"public_id":"short","active":false}"#,
            format!(r#"{{"public_id":"{identifier}","active":"false"}}"#).as_bytes(),
            format!(r#"{{"public_id":"{identifier}","active":false,"active":true}}"#).as_bytes(),
            format!(r#"{{"public_id":"{identifier}","active":false,"confirmed":true}}"#).as_bytes(),
            format!(r#"{{"public_id":"{identifier}","active":false}} trailing"#).as_bytes(),
        ] {
            assert_eq!(
                AccountsStatusRequest::from_json(body),
                Err(AccountAdministrationInputRejected)
            );
        }
    }

    #[test]
    fn ordered_pages_round_trip_the_exact_last_username() {
        let projections = vec![projection("alice"), projection("bob"), projection("carol")];
        let first_request = AccountsListRequest::from_optional_json(br#"{"limit":2}"#).unwrap();
        let first = AccountsPage::from_ordered(&first_request, projections.clone()).unwrap();
        assert_eq!(first.items.len(), 2);
        let cursor = first.next_cursor.clone().unwrap();

        let next_request = AccountsListRequest::from_optional_json(
            format!(r#"{{"limit":2,"cursor":"{cursor}"}}"#).as_bytes(),
        )
        .unwrap();
        let next = AccountsPage::from_ordered(&next_request, projections.clone()).unwrap();
        assert_eq!(next.items, vec![projection("carol")]);
        assert_eq!(next.next_cursor, None);

        let stale = AccountsListRequest {
            limit: 2,
            after_username: Some("missing".to_owned()),
        };
        assert_eq!(
            AccountsPage::from_ordered(&stale, projections),
            Err(AccountAdministrationInputRejected)
        );
        assert_eq!(
            AccountsPage::from_ordered(
                &first_request,
                vec![projection("bob"), projection("alice")]
            ),
            Err(AccountAdministrationInputRejected)
        );
    }

    #[test]
    fn typed_results_serialize_only_the_safe_projection() {
        let result = AccountAdministrationResult::List(AccountsPage {
            items: vec![projection("alice")],
            next_cursor: None,
        });
        let serialized = AccountAdministrationEnvelope {
            result,
            correlation_id: "correlation-0123456789".to_owned(),
        }
        .serialize()
        .unwrap();
        assert_eq!(
            serialized.as_str(),
            format!(
                "{{\"result\":{{\"items\":[{{\"public_id\":\"{}\",\"username\":\"alice\",\"display_name\":\"Display alice\",\"active\":true,\"mfa_required\":false}}],\"next_cursor\":null}},\"correlation_id\":\"correlation-0123456789\"}}",
                public_id(b'a')
            )
        );
        for forbidden in [
            "password",
            "verifier",
            "session",
            "temporary",
            "account_id",
            "audit",
            "state_id",
        ] {
            assert!(!serialized.contains(forbidden), "{forbidden}");
        }
        assert!(serialized.len() <= MAX_ACCOUNT_ADMINISTRATION_RESPONSE_BYTES);
    }

    #[tokio::test]
    async fn list_view_and_status_routes_emit_the_exact_typed_success_shapes() {
        let page = AccountsPage {
            items: vec![projection("alice")],
            next_cursor: None,
        };
        let list = AccountAdministrationDeclaration::new(capability(Ok(
            AccountAdministrationResult::List(page),
        )))
        .list_route()
        .oneshot(request(Method::PUT, ACCOUNTS_LIST_ROUTE, ""))
        .await
        .unwrap();
        assert_eq!(list.status(), StatusCode::OK);
        assert!(rendered(list).await.contains(r#""items":[{"public_id":"#));

        let identifier = public_id(0x41);
        let view = AccountAdministrationDeclaration::new(capability(Ok(
            AccountAdministrationResult::View(projection("alice")),
        )))
        .view_route()
        .oneshot(request(
            Method::PUT,
            ACCOUNTS_VIEW_ROUTE,
            &format!(r#"{{"public_id":"{identifier}"}}"#),
        ))
        .await
        .unwrap();
        assert_eq!(view.status(), StatusCode::OK);
        let body = rendered(view).await;
        assert!(body.contains(r#""result":{"public_id":"#));
        assert!(!body.contains(r#""items"#));

        let status = AccountAdministrationDeclaration::new(capability(Ok(
            AccountAdministrationResult::Status(projection("alice")),
        )))
        .status_route()
        .oneshot(request(
            Method::PUT,
            ACCOUNTS_STATUS_ROUTE,
            &format!(r#"{{"public_id":"{identifier}","active":false}}"#),
        ))
        .await
        .unwrap();
        assert_eq!(status.status(), StatusCode::OK);
        let body = rendered(status).await;
        assert!(body.contains(r#""result":{"public_id":"#));
        assert!(!body.contains("confirmed"));
    }

    #[tokio::test]
    async fn status_route_rejects_wrong_method_origin_media_session_csrf_and_schema() {
        let identifier = public_id(0x41);
        let body = format!(r#"{{"public_id":"{identifier}","active":false}}"#);
        let declaration = AccountAdministrationDeclaration::new(capability(Ok(
            AccountAdministrationResult::Status(projection("alice")),
        )));

        let method = declaration
            .status_route()
            .oneshot(request(Method::POST, ACCOUNTS_STATUS_ROUTE, &body))
            .await
            .unwrap();
        assert_eq!(method.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(method.headers().get(ALLOW).unwrap(), "PUT");

        let mut origin = request(Method::PUT, ACCOUNTS_STATUS_ROUTE, &body);
        origin.headers_mut().insert(
            ORIGIN,
            HeaderValue::from_static("https://elsewhere.example"),
        );
        let origin = declaration.status_route().oneshot(origin).await.unwrap();
        assert_eq!(origin.status(), StatusCode::FORBIDDEN);
        assert!(rendered(origin).await.contains("request_origin_denied"));

        let mut host = request(Method::PUT, ACCOUNTS_STATUS_ROUTE, &body);
        host.headers_mut()
            .insert(HOST, HeaderValue::from_static("elsewhere.example"));
        let host = declaration.status_route().oneshot(host).await.unwrap();
        assert_eq!(host.status(), StatusCode::FORBIDDEN);

        let mut media = request(Method::PUT, ACCOUNTS_STATUS_ROUTE, &body);
        media.headers_mut().remove(CONTENT_TYPE);
        let media = declaration.status_route().oneshot(media).await.unwrap();
        assert_eq!(media.status(), StatusCode::BAD_REQUEST);

        let mut accept = request(Method::PUT, ACCOUNTS_STATUS_ROUTE, &body);
        accept
            .headers_mut()
            .insert(ACCEPT, HeaderValue::from_static("text/plain"));
        let accept = declaration.status_route().oneshot(accept).await.unwrap();
        assert_eq!(accept.status(), StatusCode::BAD_REQUEST);

        let mut session = request(Method::PUT, ACCOUNTS_STATUS_ROUTE, &body);
        session.headers_mut().remove(COOKIE);
        let session = declaration.status_route().oneshot(session).await.unwrap();
        assert_eq!(session.status(), StatusCode::UNAUTHORIZED);

        let mut csrf = request(Method::PUT, ACCOUNTS_STATUS_ROUTE, &body);
        csrf.headers_mut().remove(CSRF_HEADER_NAME);
        let csrf = declaration.status_route().oneshot(csrf).await.unwrap();
        assert_eq!(csrf.status(), StatusCode::UNAUTHORIZED);

        let schema = declaration
            .status_route()
            .oneshot(request(
                Method::PUT,
                ACCOUNTS_STATUS_ROUTE,
                &format!(r#"{{"public_id":"{identifier}","active":false,"confirmed":true}}"#),
            ))
            .await
            .unwrap();
        assert_eq!(schema.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn account_routes_render_every_stable_rejection() {
        for (rejection, status, code) in [
            (
                AccountAdministrationRejection::BadRequest,
                StatusCode::BAD_REQUEST,
                "bad_request",
            ),
            (
                AccountAdministrationRejection::SessionInvalid,
                StatusCode::UNAUTHORIZED,
                "session_invalid",
            ),
            (
                AccountAdministrationRejection::RequestOriginDenied,
                StatusCode::FORBIDDEN,
                "request_origin_denied",
            ),
            (
                AccountAdministrationRejection::AuthorizationDenied,
                StatusCode::FORBIDDEN,
                "authorization_denied",
            ),
            (
                AccountAdministrationRejection::NotFound,
                StatusCode::NOT_FOUND,
                "not_found",
            ),
            (
                AccountAdministrationRejection::ServiceUnavailable,
                StatusCode::SERVICE_UNAVAILABLE,
                "service_unavailable",
            ),
        ] {
            let response = AccountAdministrationDeclaration::new(capability(Err(rejection)))
                .list_route()
                .oneshot(request(Method::PUT, ACCOUNTS_LIST_ROUTE, ""))
                .await
                .unwrap();
            assert_eq!(response.status(), status, "{code}");
            assert_eq!(
                rendered(response).await,
                format!(r#"{{"error":"{code}","correlation_id":"{CORRELATION}"}}"#)
            );
        }

        let response = AccountAdministrationDeclaration::new(capability(Err(
            AccountAdministrationRejection::ServiceUnavailable,
        )))
        .list_route()
        .oneshot(request(Method::GET, ACCOUNTS_LIST_ROUTE, ""))
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(response.headers().get(ALLOW).unwrap(), "PUT");
        assert!(
            rendered(response)
                .await
                .contains(r#""error":"method_not_allowed""#)
        );
    }

    #[tokio::test]
    async fn account_routes_reject_origin_session_and_media_before_the_hook() {
        let hook = capability(Ok(AccountAdministrationResult::List(AccountsPage {
            items: Vec::new(),
            next_cursor: None,
        })));
        let declaration = AccountAdministrationDeclaration::new(hook);

        let mut wrong_origin = request(Method::PUT, ACCOUNTS_LIST_ROUTE, "");
        wrong_origin.headers_mut().insert(
            ORIGIN,
            HeaderValue::from_static("https://elsewhere.example"),
        );
        let response = declaration
            .list_route()
            .oneshot(wrong_origin)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(rendered(response).await.contains("request_origin_denied"));

        let mut no_session = request(Method::PUT, ACCOUNTS_LIST_ROUTE, "");
        no_session.headers_mut().remove(COOKIE);
        let response = declaration.list_route().oneshot(no_session).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(rendered(response).await.contains("session_invalid"));

        let mut wrong_accept = request(Method::PUT, ACCOUNTS_LIST_ROUTE, "");
        wrong_accept
            .headers_mut()
            .insert(ACCEPT, HeaderValue::from_static("text/plain"));
        let response = declaration
            .list_route()
            .oneshot(wrong_accept)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(rendered(response).await.contains("bad_request"));
    }

    #[tokio::test]
    async fn undeclared_operational_surface_mounts_no_account_route() {
        let router = OperationalSurface::default()
            .mount(Router::new().fallback(any(|| async { StatusCode::NOT_FOUND })));
        for target in [
            ACCOUNTS_LIST_ROUTE,
            ACCOUNTS_VIEW_ROUTE,
            ACCOUNTS_STATUS_ROUTE,
        ] {
            let response = router
                .clone()
                .oneshot(request(Method::PUT, target, ""))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{target}");
        }
    }
}
