//! Strict public Client Module contract for Group administration.

use std::{pin::Pin, sync::Arc};

use axum::{
    body::{Body, to_bytes},
    extract::Request,
    http::{
        Extensions, HeaderMap, HeaderValue, Method, StatusCode,
        header::{ALLOW, CONTENT_TYPE},
    },
    response::Response,
    routing::{MethodRouter, any},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::{
    ExpectedOrigin, JSON_MEDIA_TYPE, accepts_json,
    authentication::{
        CorrelationSource, submitted_csrf_token, submitted_session_token, unrenderable_response,
    },
    has_request_body, single_header,
    typed_json::{ResponseCorrelation, StableCode, TypedJsonEnvelope, typed_json_response},
};

pub const GROUPS_LIST_ROUTE: &str = "/api/v1/administration/groups/list";
pub const GROUPS_VIEW_ROUTE: &str = "/api/v1/administration/groups/view";
pub const GROUPS_CREATE_ROUTE: &str = "/api/v1/administration/groups/create";
pub const GROUPS_UPDATE_ROUTE: &str = "/api/v1/administration/groups/update";
pub const GROUPS_DELETE_ROUTE: &str = "/api/v1/administration/groups/delete";
pub const DEFAULT_GROUPS_PAGE_LIMIT: usize = 50;
pub const MAX_GROUPS_PAGE_LIMIT: usize = 100;
pub const MAX_GROUP_ADMINISTRATION_BODY_BYTES: usize = 2 * 1024;
pub const MAX_GROUP_ADMINISTRATION_RESPONSE_BYTES: usize = 160 * 1024;

const CURSOR_SCOPE: &[u8] = b"weavelit:/api/v1/administration/groups/list:v1\0";
const MAX_NAME_BYTES: usize = 256;
const MAX_DESCRIPTION_BYTES: usize = 1024;
const PUBLIC_ID_BYTES: usize = 16;
const PUBLIC_ID_CHARS: usize = 22;
const TICKET_BYTES: usize = 32;
const TICKET_CHARS: usize = 43;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GroupAdministrationInputRejected;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListBody {
    limit: Option<usize>,
    cursor: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupsListRequest {
    limit: usize,
    after_name: Option<String>,
}

impl GroupsListRequest {
    pub fn from_optional_json(body: &[u8]) -> Result<Self, GroupAdministrationInputRejected> {
        if body.len() > MAX_GROUP_ADMINISTRATION_BODY_BYTES {
            return Err(GroupAdministrationInputRejected);
        }
        let parsed = if body.is_empty() {
            ListBody {
                limit: None,
                cursor: None,
            }
        } else {
            strict_json(body)?
        };
        let limit = parsed.limit.unwrap_or(DEFAULT_GROUPS_PAGE_LIMIT);
        if !(1..=MAX_GROUPS_PAGE_LIMIT).contains(&limit) {
            return Err(GroupAdministrationInputRejected);
        }
        Ok(Self {
            limit,
            after_name: parsed
                .cursor
                .map(|value| decode_cursor(&value))
                .transpose()?,
        })
    }

    pub const fn limit(&self) -> usize {
        self.limit
    }
    pub fn after_name(&self) -> Option<&str> {
        self.after_name.as_deref()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PublicIdBody {
    public_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupsViewRequest {
    public_id: String,
}

impl GroupsViewRequest {
    pub fn from_json(body: &[u8]) -> Result<Self, GroupAdministrationInputRejected> {
        let parsed: PublicIdBody = required_json(body)?;
        if !valid_public_id(&parsed.public_id) {
            return Err(GroupAdministrationInputRejected);
        }
        Ok(Self {
            public_id: parsed.public_id,
        })
    }
    pub fn public_id(&self) -> &str {
        &self.public_id
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateBody {
    name: String,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupsCreateRequest {
    name: String,
    description: Option<String>,
}

impl GroupsCreateRequest {
    pub fn from_json(body: &[u8]) -> Result<Self, GroupAdministrationInputRejected> {
        let parsed: CreateBody = required_json(body)?;
        validate_text(&parsed.name, MAX_NAME_BYTES)?;
        if let Some(value) = parsed.description.as_deref() {
            validate_text(value, MAX_DESCRIPTION_BYTES)?;
        }
        Ok(Self {
            name: parsed.name,
            description: parsed.description,
        })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateBody {
    public_id: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupsUpdateRequest {
    public_id: String,
    name: String,
    description: Option<String>,
}

impl GroupsUpdateRequest {
    pub fn from_json(body: &[u8]) -> Result<Self, GroupAdministrationInputRejected> {
        let parsed: UpdateBody = required_json(body)?;
        if !valid_public_id(&parsed.public_id) {
            return Err(GroupAdministrationInputRejected);
        }
        validate_text(&parsed.name, MAX_NAME_BYTES)?;
        if let Some(value) = parsed.description.as_deref() {
            validate_text(value, MAX_DESCRIPTION_BYTES)?;
        }
        Ok(Self {
            public_id: parsed.public_id,
            name: parsed.name,
            description: parsed.description,
        })
    }
    pub fn public_id(&self) -> &str {
        &self.public_id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteBody {
    public_id: String,
    grant_mutation_step_up_ticket: String,
}

#[derive(Debug)]
pub struct GroupsDeleteRequest {
    public_id: String,
    ticket: Zeroizing<String>,
}

impl GroupsDeleteRequest {
    pub fn from_json(body: &[u8]) -> Result<Self, GroupAdministrationInputRejected> {
        let parsed: DeleteBody = required_json(body)?;
        if !valid_public_id(&parsed.public_id)
            || !valid_ticket(&parsed.grant_mutation_step_up_ticket)
        {
            return Err(GroupAdministrationInputRejected);
        }
        Ok(Self {
            public_id: parsed.public_id,
            ticket: Zeroizing::new(parsed.grant_mutation_step_up_ticket),
        })
    }
    pub fn public_id(&self) -> &str {
        &self.public_id
    }
    pub fn ticket(&self) -> &str {
        &self.ticket
    }
}

#[derive(Debug)]
pub enum GroupAdministrationRequest {
    List(GroupsListRequest),
    View(GroupsViewRequest),
    Create(GroupsCreateRequest),
    Update(GroupsUpdateRequest),
    Delete(GroupsDeleteRequest),
}

pub struct GroupAdministrationSubmission {
    pub request: GroupAdministrationRequest,
    pub session_token: Zeroizing<String>,
    pub csrf_token: Zeroizing<String>,
    pub correlation_id: String,
    pub context: Extensions,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GroupAdministrationProjection {
    public_id: String,
    name: String,
    description: Option<String>,
}

impl GroupAdministrationProjection {
    pub fn new(
        public_id: String,
        name: String,
        description: Option<String>,
    ) -> Result<Self, GroupAdministrationInputRejected> {
        if !valid_public_id(&public_id) {
            return Err(GroupAdministrationInputRejected);
        }
        validate_text(&name, MAX_NAME_BYTES)?;
        if let Some(value) = description.as_deref() {
            validate_text(value, MAX_DESCRIPTION_BYTES)?;
        }
        Ok(Self {
            public_id,
            name,
            description,
        })
    }
    pub fn public_id(&self) -> &str {
        &self.public_id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GroupsPage {
    items: Vec<GroupAdministrationProjection>,
    next_cursor: Option<String>,
}

impl GroupsPage {
    pub fn from_ordered(
        request: &GroupsListRequest,
        projections: Vec<GroupAdministrationProjection>,
    ) -> Result<Self, GroupAdministrationInputRejected> {
        if projections
            .windows(2)
            .any(|pair| pair[0].name() >= pair[1].name())
        {
            return Err(GroupAdministrationInputRejected);
        }
        let start = match request.after_name() {
            Some(after) => {
                projections
                    .binary_search_by(|value| value.name().cmp(after))
                    .map_err(|_| GroupAdministrationInputRejected)?
                    + 1
            }
            None => 0,
        };
        let end = start.saturating_add(request.limit()).min(projections.len());
        let next_cursor = if end < projections.len() {
            projections
                .get(end - 1)
                .map(|value| encode_cursor(value.name()))
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GroupDeleted {
    public_id: String,
}
impl GroupDeleted {
    pub fn new(public_id: String) -> Result<Self, GroupAdministrationInputRejected> {
        if !valid_public_id(&public_id) {
            return Err(GroupAdministrationInputRejected);
        }
        Ok(Self { public_id })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum GroupAdministrationResult {
    List(GroupsPage),
    Projection(GroupAdministrationProjection),
    Deleted(GroupDeleted),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GroupAdministrationRejection {
    BadRequest,
    SessionInvalid,
    RequestOriginDenied,
    AuthorizationDenied,
    GrantMutationDenied,
    MethodNotAllowed,
    NotFound,
    Conflict,
    ServiceUnavailable,
}

impl GroupAdministrationRejection {
    pub const fn status(self) -> StatusCode {
        match self {
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::SessionInvalid => StatusCode::UNAUTHORIZED,
            Self::RequestOriginDenied | Self::AuthorizationDenied | Self::GrantMutationDenied => {
                StatusCode::FORBIDDEN
            }
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Conflict => StatusCode::CONFLICT,
            Self::ServiceUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        }
    }
    pub const fn code(self) -> &'static str {
        match self {
            Self::BadRequest => "bad_request",
            Self::SessionInvalid => "session_invalid",
            Self::RequestOriginDenied => "request_origin_denied",
            Self::AuthorizationDenied => "authorization_denied",
            Self::GrantMutationDenied => "grant_mutation_denied",
            Self::MethodNotAllowed => "method_not_allowed",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::ServiceUnavailable => "service_unavailable",
        }
    }
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

pub type GroupAdministrationCommit = Arc<
    dyn Fn(
            GroupAdministrationSubmission,
        ) -> Pin<
            Box<
                dyn Future<Output = Result<GroupAdministrationResult, GroupAdministrationRejection>>
                    + Send,
            >,
        > + Send
        + Sync,
>;

pub struct GroupAdministrationCapability {
    pub expected_origin: ExpectedOrigin,
    pub correlate: CorrelationSource,
    pub execute: GroupAdministrationCommit,
}
pub struct GroupAdministrationDeclaration {
    capability: Arc<GroupAdministrationCapability>,
}

impl GroupAdministrationDeclaration {
    pub fn new(capability: GroupAdministrationCapability) -> Self {
        Self {
            capability: Arc::new(capability),
        }
    }
    pub fn list_route(&self) -> MethodRouter {
        self.route(Route::List)
    }
    pub fn view_route(&self) -> MethodRouter {
        self.route(Route::View)
    }
    pub fn create_route(&self) -> MethodRouter {
        self.route(Route::Create)
    }
    pub fn update_route(&self) -> MethodRouter {
        self.route(Route::Update)
    }
    pub fn delete_route(&self) -> MethodRouter {
        self.route(Route::Delete)
    }
    fn route(&self, route: Route) -> MethodRouter {
        let capability = Arc::clone(&self.capability);
        any(move |request| response(request, Arc::clone(&capability), route))
    }
}

#[derive(Clone, Copy)]
enum Route {
    List,
    View,
    Create,
    Update,
    Delete,
}

async fn response(
    request: Request,
    capability: Arc<GroupAdministrationCapability>,
    route: Route,
) -> Response {
    let Some(correlation_id) = (capability.correlate)() else {
        return unrenderable_response();
    };
    let (parts, body) = request.into_parts();
    if let Err(rejection) = validate_head(
        &parts.method,
        &parts.headers,
        capability.expected_origin,
        !matches!(route, Route::List),
    ) {
        return rejection.response(&correlation_id);
    }
    let Ok(body) = to_bytes(body, MAX_GROUP_ADMINISTRATION_BODY_BYTES).await else {
        return GroupAdministrationRejection::BadRequest.response(&correlation_id);
    };
    let parsed = match route {
        Route::List => {
            GroupsListRequest::from_optional_json(&body).map(GroupAdministrationRequest::List)
        }
        Route::View => GroupsViewRequest::from_json(&body).map(GroupAdministrationRequest::View),
        Route::Create => {
            GroupsCreateRequest::from_json(&body).map(GroupAdministrationRequest::Create)
        }
        Route::Update => {
            GroupsUpdateRequest::from_json(&body).map(GroupAdministrationRequest::Update)
        }
        Route::Delete => {
            GroupsDeleteRequest::from_json(&body).map(GroupAdministrationRequest::Delete)
        }
    };
    let Ok(request) = parsed else {
        return GroupAdministrationRejection::BadRequest.response(&correlation_id);
    };
    let Ok(session) = submitted_session_token(&parts.headers) else {
        return GroupAdministrationRejection::SessionInvalid.response(&correlation_id);
    };
    let Ok(csrf) = submitted_csrf_token(&parts.headers) else {
        return GroupAdministrationRejection::SessionInvalid.response(&correlation_id);
    };
    match (capability.execute)(GroupAdministrationSubmission {
        request,
        session_token: Zeroizing::new(session.to_owned()),
        csrf_token: Zeroizing::new(csrf.to_owned()),
        correlation_id: correlation_id.clone(),
        context: parts.extensions,
    })
    .await
    {
        Ok(result) => success(result, correlation_id),
        Err(rejection) => rejection.response(&correlation_id),
    }
}

fn success(result: GroupAdministrationResult, correlation_id: String) -> Response {
    #[derive(Serialize)]
    struct Envelope<'a> {
        result: &'a GroupAdministrationResult,
        correlation_id: &'a str,
    }
    if ResponseCorrelation::new(&correlation_id).is_none() {
        return unrenderable_response();
    }
    let Ok(body) = serde_json::to_vec(&Envelope {
        result: &result,
        correlation_id: &correlation_id,
    }) else {
        return unrenderable_response();
    };
    if body.len() > MAX_GROUP_ADMINISTRATION_RESPONSE_BYTES {
        return unrenderable_response();
    }
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = StatusCode::OK;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
}

fn validate_head(
    method: &Method,
    headers: &HeaderMap,
    expected_origin: ExpectedOrigin,
    body_required: bool,
) -> Result<(), GroupAdministrationRejection> {
    if method != Method::PUT {
        return Err(GroupAdministrationRejection::MethodNotAllowed);
    }
    if !expected_origin.is_same_origin(headers) {
        return Err(GroupAdministrationRejection::RequestOriginDenied);
    }
    if !accepts_json(headers) {
        return Err(GroupAdministrationRejection::BadRequest);
    }
    if body_required || has_request_body(headers) {
        let value =
            single_header(headers, CONTENT_TYPE).ok_or(GroupAdministrationRejection::BadRequest)?;
        if value.as_bytes() != JSON_MEDIA_TYPE {
            return Err(GroupAdministrationRejection::BadRequest);
        }
    } else if single_header(headers, CONTENT_TYPE).is_some() {
        return Err(GroupAdministrationRejection::BadRequest);
    }
    submitted_session_token(headers)
        .and_then(|_| submitted_csrf_token(headers))
        .map_err(|_| GroupAdministrationRejection::SessionInvalid)?;
    Ok(())
}

fn required_json<T: for<'de> Deserialize<'de>>(
    body: &[u8],
) -> Result<T, GroupAdministrationInputRejected> {
    if body.is_empty() || body.len() > MAX_GROUP_ADMINISTRATION_BODY_BYTES {
        return Err(GroupAdministrationInputRejected);
    }
    strict_json(body)
}
fn strict_json<T: for<'de> Deserialize<'de>>(
    body: &[u8],
) -> Result<T, GroupAdministrationInputRejected> {
    let mut deserializer = serde_json::Deserializer::from_slice(body);
    let value = T::deserialize(&mut deserializer).map_err(|_| GroupAdministrationInputRejected)?;
    deserializer
        .end()
        .map_err(|_| GroupAdministrationInputRejected)?;
    Ok(value)
}
fn validate_text(value: &str, max: usize) -> Result<(), GroupAdministrationInputRejected> {
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        Err(GroupAdministrationInputRejected)
    } else {
        Ok(())
    }
}
fn valid_public_id(value: &str) -> bool {
    value.len() == PUBLIC_ID_CHARS
        && URL_SAFE_NO_PAD.decode(value).is_ok_and(|bytes| {
            bytes.len() == PUBLIC_ID_BYTES
                && bytes.iter().any(|byte| *byte != 0)
                && URL_SAFE_NO_PAD.encode(bytes) == value
        })
}
fn valid_ticket(value: &str) -> bool {
    value.len() == TICKET_CHARS
        && URL_SAFE_NO_PAD.decode(value).is_ok_and(|bytes| {
            bytes.len() == TICKET_BYTES && URL_SAFE_NO_PAD.encode(bytes) == value
        })
}
fn encode_cursor(name: &str) -> Result<String, GroupAdministrationInputRejected> {
    validate_text(name, MAX_NAME_BYTES)?;
    let mut value = Vec::with_capacity(CURSOR_SCOPE.len() + name.len());
    value.extend_from_slice(CURSOR_SCOPE);
    value.extend_from_slice(name.as_bytes());
    Ok(URL_SAFE_NO_PAD.encode(value))
}
fn decode_cursor(cursor: &str) -> Result<String, GroupAdministrationInputRejected> {
    let decoded = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| GroupAdministrationInputRejected)?;
    if URL_SAFE_NO_PAD.encode(&decoded) != cursor {
        return Err(GroupAdministrationInputRejected);
    }
    let value = std::str::from_utf8(
        decoded
            .strip_prefix(CURSOR_SCOPE)
            .ok_or(GroupAdministrationInputRejected)?,
    )
    .map_err(|_| GroupAdministrationInputRejected)?;
    validate_text(value, MAX_NAME_BYTES)?;
    Ok(value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "MTExMTExMTExMTExMTExMQ";
    const TICKET: &str = "MTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTE";

    #[test]
    fn strict_requests_accept_only_documented_values() {
        assert!(GroupsListRequest::from_optional_json(b"").is_ok());
        assert!(GroupsListRequest::from_optional_json(br#"{"limit":100}"#).is_ok());
        assert!(GroupsListRequest::from_optional_json(br#"{"limit":0}"#).is_err());
        assert!(
            GroupsViewRequest::from_json(format!(r#"{{"public_id":"{ID}"}}"#).as_bytes()).is_ok()
        );
        assert!(
            GroupsCreateRequest::from_json(br#"{"name":"Operators","description":null}"#).is_ok()
        );
        assert!(
            GroupsUpdateRequest::from_json(
                format!(r#"{{"public_id":"{ID}","name":"Support"}}"#).as_bytes()
            )
            .is_ok()
        );
        assert!(
            GroupsDeleteRequest::from_json(
                format!(r#"{{"public_id":"{ID}","grant_mutation_step_up_ticket":"{TICKET}"}}"#)
                    .as_bytes()
            )
            .is_ok()
        );
        for invalid in [
            br#"{"name":"Operators","extra":true}"#.as_slice(),
            br#"{"name":"Operators","name":"Other"}"#.as_slice(),
            br#"{"name":""}"#.as_slice(),
        ] {
            assert!(GroupsCreateRequest::from_json(invalid).is_err());
        }
    }

    #[test]
    fn pagination_is_route_scoped_and_requires_current_exact_position() {
        let items = ["Alpha", "Beta", "Gamma"]
            .into_iter()
            .map(|name| {
                GroupAdministrationProjection::new(ID.to_owned(), name.to_owned(), None).unwrap()
            })
            .collect::<Vec<_>>();
        let first = GroupsPage::from_ordered(
            &GroupsListRequest::from_optional_json(br#"{"limit":2}"#).unwrap(),
            items.clone(),
        )
        .unwrap();
        let cursor = first.next_cursor.unwrap();
        let second = GroupsPage::from_ordered(
            &GroupsListRequest::from_optional_json(
                format!(r#"{{"cursor":"{cursor}"}}"#).as_bytes(),
            )
            .unwrap(),
            items,
        )
        .unwrap();
        assert_eq!(second.items.len(), 1);
        assert!(GroupsListRequest::from_optional_json(br#"{"cursor":"YmFk"}"#).is_err());
    }
}
