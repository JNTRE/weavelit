//! Strict public Client Module contract for Group administration.

use std::{io, pin::Pin, sync::Arc};

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
    ExpectedOrigin, JSON_MEDIA_TYPE, WipedBody, accepts_json,
    administration::AccountAdministrationProjection,
    authentication::{
        CorrelationSource, submitted_csrf_token, submitted_session_token, unrenderable_response,
    },
    deserialize_present_optional, has_request_body, single_header,
    typed_json::{ResponseCorrelation, StableCode, TypedJsonEnvelope, typed_json_response},
};

pub const GROUPS_LIST_ROUTE: &str = "/api/v1/administration/groups/list";
pub const GROUPS_VIEW_ROUTE: &str = "/api/v1/administration/groups/view";
pub const GROUPS_CREATE_ROUTE: &str = "/api/v1/administration/groups/create";
pub const GROUPS_UPDATE_ROUTE: &str = "/api/v1/administration/groups/update";
pub const GROUPS_DELETE_ROUTE: &str = "/api/v1/administration/groups/delete";
pub const GROUP_MEMBERS_LIST_ROUTE: &str = "/api/v1/administration/groups/members/list";
pub const GROUP_MEMBERS_CHANGE_ROUTE: &str = "/api/v1/administration/groups/members/change";
pub const GROUP_GRANTS_LIST_ROUTE: &str = "/api/v1/administration/groups/grants/list";
pub const GROUP_GRANTS_CHANGE_ROUTE: &str = "/api/v1/administration/groups/grants/change";
pub const ADMINISTRATION_CATALOG_ROUTE: &str = "/api/v1/administration/catalog";
pub const DEFAULT_GROUPS_PAGE_LIMIT: usize = 50;
pub const MAX_GROUPS_PAGE_LIMIT: usize = 100;
pub const MAX_GROUP_ADMINISTRATION_BODY_BYTES: usize = 2 * 1024;
pub const MAX_GROUP_ADMINISTRATION_RESPONSE_BYTES: usize = 160 * 1024;
pub const MAX_ADMINISTRATION_CATALOG_ENTRIES: usize = 256;

const CURSOR_SCOPE: &[u8] = b"weavelit:/api/v1/administration/groups/list:v1\0";
const MEMBER_CURSOR_SCOPE: &[u8] = b"weavelit:/api/v1/administration/groups/members/list:v1\0";
const GRANT_CURSOR_SCOPE: &[u8] = b"weavelit:/api/v1/administration/groups/grants/list:v1\0";
const MAX_NAME_BYTES: usize = 256;
const MAX_DESCRIPTION_BYTES: usize = 1024;
const PUBLIC_ID_BYTES: usize = 16;
const PUBLIC_ID_CHARS: usize = 22;
const TICKET_BYTES: usize = 32;
const TICKET_CHARS: usize = 43;

fn deserialize_zeroizing_string<'de, D>(deserializer: D) -> Result<Zeroizing<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    String::deserialize(deserializer).map(Zeroizing::new)
}

fn parse_group_delete_body_wiped<B: AsMut<[u8]>>(
    buffer: B,
) -> Result<GroupsDeleteRequest, GroupAdministrationInputRejected> {
    let mut body = WipedBody::new(buffer);
    GroupsDeleteRequest::from_json(body.bytes())
}

fn parse_group_member_change_body_wiped<B: AsMut<[u8]>>(
    buffer: B,
) -> Result<GroupMemberChangeRequest, GroupAdministrationInputRejected> {
    let mut body = WipedBody::new(buffer);
    GroupMemberChangeRequest::from_json(body.bytes())
}

fn parse_group_grant_change_body_wiped<B: AsMut<[u8]>>(
    buffer: B,
) -> Result<GroupGrantChangeRequest, GroupAdministrationInputRejected> {
    let mut body = WipedBody::new(buffer);
    GroupGrantChangeRequest::from_json(body.bytes())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GroupAdministrationInputRejected;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListBody {
    #[serde(default, deserialize_with = "deserialize_present_optional")]
    limit: Option<usize>,
    #[serde(default, deserialize_with = "deserialize_present_optional")]
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
    #[serde(deserialize_with = "deserialize_zeroizing_string")]
    grant_mutation_step_up_ticket: Zeroizing<String>,
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
            ticket: parsed.grant_mutation_step_up_ticket,
        })
    }
    pub fn public_id(&self) -> &str {
        &self.public_id
    }
    pub fn ticket(&self) -> &str {
        &self.ticket
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssociationListBody {
    group_public_id: String,
    #[serde(default, deserialize_with = "deserialize_present_optional")]
    limit: Option<usize>,
    #[serde(default, deserialize_with = "deserialize_present_optional")]
    cursor: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupMembersListRequest {
    group_public_id: String,
    limit: usize,
    after: Option<(String, String)>,
}

impl GroupMembersListRequest {
    pub fn from_json(body: &[u8]) -> Result<Self, GroupAdministrationInputRejected> {
        let parsed: AssociationListBody = required_json(body)?;
        if !valid_public_id(&parsed.group_public_id) {
            return Err(GroupAdministrationInputRejected);
        }
        let limit = page_limit(parsed.limit)?;
        Ok(Self {
            group_public_id: parsed.group_public_id,
            limit,
            after: parsed
                .cursor
                .map(|cursor| decode_member_cursor(&cursor))
                .transpose()?,
        })
    }

    pub fn group_public_id(&self) -> &str {
        &self.group_public_id
    }

    pub const fn limit(&self) -> usize {
        self.limit
    }

    pub fn after(&self) -> Option<(&str, &str)> {
        self.after
            .as_ref()
            .map(|(username, public_id)| (username.as_str(), public_id.as_str()))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MemberChangeBody {
    group_public_id: String,
    account_public_id: String,
    present: bool,
    #[serde(deserialize_with = "deserialize_zeroizing_string")]
    grant_mutation_step_up_ticket: Zeroizing<String>,
}

#[derive(Debug)]
pub struct GroupMemberChangeRequest {
    group_public_id: String,
    account_public_id: String,
    present: bool,
    ticket: Zeroizing<String>,
}

impl GroupMemberChangeRequest {
    pub fn from_json(body: &[u8]) -> Result<Self, GroupAdministrationInputRejected> {
        let parsed: MemberChangeBody = required_json(body)?;
        if !valid_public_id(&parsed.group_public_id)
            || !valid_public_id(&parsed.account_public_id)
            || !valid_ticket(&parsed.grant_mutation_step_up_ticket)
        {
            return Err(GroupAdministrationInputRejected);
        }
        Ok(Self {
            group_public_id: parsed.group_public_id,
            account_public_id: parsed.account_public_id,
            present: parsed.present,
            ticket: parsed.grant_mutation_step_up_ticket,
        })
    }

    pub fn group_public_id(&self) -> &str {
        &self.group_public_id
    }

    pub fn account_public_id(&self) -> &str {
        &self.account_public_id
    }

    pub const fn present(&self) -> bool {
        self.present
    }

    pub fn ticket(&self) -> &str {
        &self.ticket
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum GroupGrantProjection {
    ClientModule { value: String },
    ServiceModule { value: String },
    Operation { value: String },
    ServerAdministration,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGroupGrant {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    value: RawGrantValue,
}

#[derive(Default)]
enum RawGrantValue {
    #[default]
    Missing,
    Text(String),
}

impl<'de> Deserialize<'de> for RawGrantValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        String::deserialize(deserializer).map(Self::Text)
    }
}

impl<'de> Deserialize<'de> for GroupGrantProjection {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawGroupGrant::deserialize(deserializer)?;
        match (raw.kind.as_str(), raw.value) {
            ("client_module", RawGrantValue::Text(value)) => Ok(Self::ClientModule { value }),
            ("service_module", RawGrantValue::Text(value)) => Ok(Self::ServiceModule { value }),
            ("operation", RawGrantValue::Text(value)) => Ok(Self::Operation { value }),
            ("server_administration", RawGrantValue::Missing) => Ok(Self::ServerAdministration),
            _ => Err(serde::de::Error::custom("invalid Group grant")),
        }
    }
}

impl GroupGrantProjection {
    fn validate(&self) -> Result<(), GroupAdministrationInputRejected> {
        match self {
            Self::ClientModule { value }
            | Self::ServiceModule { value }
            | Self::Operation { value } => validate_text(value, MAX_NAME_BYTES),
            Self::ServerAdministration => Ok(()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupGrantsListRequest {
    group_public_id: String,
    limit: usize,
    after: Option<GroupGrantProjection>,
}

impl GroupGrantsListRequest {
    pub fn from_json(body: &[u8]) -> Result<Self, GroupAdministrationInputRejected> {
        let parsed: AssociationListBody = required_json(body)?;
        if !valid_public_id(&parsed.group_public_id) {
            return Err(GroupAdministrationInputRejected);
        }
        let limit = page_limit(parsed.limit)?;
        Ok(Self {
            group_public_id: parsed.group_public_id,
            limit,
            after: parsed
                .cursor
                .map(|cursor| decode_grant_cursor(&cursor))
                .transpose()?,
        })
    }

    pub fn group_public_id(&self) -> &str {
        &self.group_public_id
    }

    pub const fn limit(&self) -> usize {
        self.limit
    }

    pub const fn after(&self) -> Option<&GroupGrantProjection> {
        self.after.as_ref()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantChangeBody {
    group_public_id: String,
    grant: GroupGrantProjection,
    present: bool,
    #[serde(deserialize_with = "deserialize_zeroizing_string")]
    grant_mutation_step_up_ticket: Zeroizing<String>,
}

#[derive(Debug)]
pub struct GroupGrantChangeRequest {
    group_public_id: String,
    grant: GroupGrantProjection,
    present: bool,
    ticket: Zeroizing<String>,
}

impl GroupGrantChangeRequest {
    pub fn from_json(body: &[u8]) -> Result<Self, GroupAdministrationInputRejected> {
        let parsed: GrantChangeBody = required_json(body)?;
        if !valid_public_id(&parsed.group_public_id)
            || !valid_ticket(&parsed.grant_mutation_step_up_ticket)
        {
            return Err(GroupAdministrationInputRejected);
        }
        parsed.grant.validate()?;
        Ok(Self {
            group_public_id: parsed.group_public_id,
            grant: parsed.grant,
            present: parsed.present,
            ticket: parsed.grant_mutation_step_up_ticket,
        })
    }

    pub fn group_public_id(&self) -> &str {
        &self.group_public_id
    }

    pub const fn grant(&self) -> &GroupGrantProjection {
        &self.grant
    }

    pub const fn present(&self) -> bool {
        self.present
    }

    pub fn ticket(&self) -> &str {
        &self.ticket
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdministrationCatalogRequest;

impl AdministrationCatalogRequest {
    pub fn from_optional_json(body: &[u8]) -> Result<Self, GroupAdministrationInputRejected> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Empty {}

        if body.len() > MAX_GROUP_ADMINISTRATION_BODY_BYTES {
            return Err(GroupAdministrationInputRejected);
        }
        if !body.is_empty() {
            let _: Empty = strict_json(body)?;
        }
        Ok(Self)
    }
}

#[derive(Debug)]
pub enum GroupAdministrationRequest {
    List(GroupsListRequest),
    View(GroupsViewRequest),
    Create(GroupsCreateRequest),
    Update(GroupsUpdateRequest),
    Delete(GroupsDeleteRequest),
    MembersList(GroupMembersListRequest),
    MemberChange(GroupMemberChangeRequest),
    GrantsList(GroupGrantsListRequest),
    GrantChange(GroupGrantChangeRequest),
    Catalog(AdministrationCatalogRequest),
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
pub struct GroupMembersPage {
    items: Vec<AccountAdministrationProjection>,
    next_cursor: Option<String>,
}

impl GroupMembersPage {
    pub fn from_ordered(
        request: &GroupMembersListRequest,
        projections: Vec<AccountAdministrationProjection>,
    ) -> Result<Self, GroupAdministrationInputRejected> {
        if projections
            .windows(2)
            .any(|pair| member_key(&pair[0]) >= member_key(&pair[1]))
        {
            return Err(GroupAdministrationInputRejected);
        }
        let start = match request.after() {
            Some(after) => {
                projections
                    .binary_search_by(|projection| member_key(projection).cmp(&after))
                    .map_err(|_| GroupAdministrationInputRejected)?
                    + 1
            }
            None => 0,
        };
        let end = start.saturating_add(request.limit()).min(projections.len());
        let next_cursor = if end < projections.len() {
            projections
                .get(end - 1)
                .map(|projection| {
                    encode_member_cursor(projection.username(), projection.public_id())
                })
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

fn member_key(projection: &AccountAdministrationProjection) -> (&str, &str) {
    (projection.username(), projection.public_id())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GroupGrantsPage {
    items: Vec<GroupGrantProjection>,
    next_cursor: Option<String>,
}

impl GroupGrantsPage {
    pub fn from_ordered(
        request: &GroupGrantsListRequest,
        projections: Vec<GroupGrantProjection>,
    ) -> Result<Self, GroupAdministrationInputRejected> {
        if projections.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(GroupAdministrationInputRejected);
        }
        let start = match request.after() {
            Some(after) => {
                projections
                    .binary_search(after)
                    .map_err(|_| GroupAdministrationInputRejected)?
                    + 1
            }
            None => 0,
        };
        let end = start.saturating_add(request.limit()).min(projections.len());
        let next_cursor = if end < projections.len() {
            projections
                .get(end - 1)
                .map(encode_grant_cursor)
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
pub struct GroupMemberChanged {
    account: AccountAdministrationProjection,
    present: bool,
}

impl GroupMemberChanged {
    #[must_use]
    pub const fn new(account: AccountAdministrationProjection, present: bool) -> Self {
        Self { account, present }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GroupGrantChanged {
    grant: GroupGrantProjection,
    present: bool,
}

impl GroupGrantChanged {
    pub fn new(
        grant: GroupGrantProjection,
        present: bool,
    ) -> Result<Self, GroupAdministrationInputRejected> {
        grant.validate()?;
        Ok(Self { grant, present })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AdministrationCatalog {
    client_modules: Vec<String>,
    service_modules: Vec<String>,
    operations: Vec<String>,
}

impl AdministrationCatalog {
    pub fn new(
        client_modules: Vec<String>,
        service_modules: Vec<String>,
        operations: Vec<String>,
    ) -> Result<Self, GroupAdministrationInputRejected> {
        for values in [&client_modules, &service_modules, &operations] {
            if values.len() > MAX_ADMINISTRATION_CATALOG_ENTRIES
                || values
                    .iter()
                    .any(|value| validate_text(value, MAX_NAME_BYTES).is_err())
                || values.windows(2).any(|pair| pair[0] >= pair[1])
            {
                return Err(GroupAdministrationInputRejected);
            }
        }
        Ok(Self {
            client_modules,
            service_modules,
            operations,
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
    Members(GroupMembersPage),
    MemberChanged(GroupMemberChanged),
    Grants(GroupGrantsPage),
    GrantChanged(GroupGrantChanged),
    Catalog(AdministrationCatalog),
}

/// Route-specific typed envelope serialized only by the Server listener.
#[derive(Clone)]
pub struct GroupAdministrationEnvelope {
    result: GroupAdministrationResult,
    correlation_id: String,
}

impl GroupAdministrationEnvelope {
    #[must_use]
    pub fn serialize(&self) -> Option<Zeroizing<String>> {
        #[derive(Serialize)]
        struct WireEnvelope<'a> {
            result: &'a GroupAdministrationResult,
            correlation_id: &'a str,
        }

        let mut writer = GroupBoundedJsonWriter::new();
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

struct GroupBoundedJsonWriter {
    bytes: Zeroizing<Vec<u8>>,
}

impl GroupBoundedJsonWriter {
    fn new() -> Self {
        Self {
            bytes: Zeroizing::new(Vec::with_capacity(MAX_GROUP_ADMINISTRATION_RESPONSE_BYTES)),
        }
    }

    fn into_bytes(self) -> Zeroizing<Vec<u8>> {
        self.bytes
    }
}

impl io::Write for GroupBoundedJsonWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self.bytes.len().saturating_add(buffer.len()) > MAX_GROUP_ADMINISTRATION_RESPONSE_BYTES {
            return Err(io::Error::other("Group response exceeds its bound"));
        }
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
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
    pub fn members_list_route(&self) -> MethodRouter {
        self.route(Route::MembersList)
    }
    pub fn member_change_route(&self) -> MethodRouter {
        self.route(Route::MemberChange)
    }
    pub fn grants_list_route(&self) -> MethodRouter {
        self.route(Route::GrantsList)
    }
    pub fn grant_change_route(&self) -> MethodRouter {
        self.route(Route::GrantChange)
    }
    pub fn catalog_route(&self) -> MethodRouter {
        self.route(Route::Catalog)
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
    MembersList,
    MemberChange,
    GrantsList,
    GrantChange,
    Catalog,
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
        !matches!(route, Route::List | Route::Catalog),
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
        Route::Delete => match body.try_into_mut() {
            Ok(unique) => parse_group_delete_body_wiped(unique),
            Err(shared) => parse_group_delete_body_wiped(shared.to_vec()),
        }
        .map(GroupAdministrationRequest::Delete),
        Route::MembersList => {
            GroupMembersListRequest::from_json(&body).map(GroupAdministrationRequest::MembersList)
        }
        Route::MemberChange => match body.try_into_mut() {
            Ok(unique) => parse_group_member_change_body_wiped(unique),
            Err(shared) => parse_group_member_change_body_wiped(shared.to_vec()),
        }
        .map(GroupAdministrationRequest::MemberChange),
        Route::GrantsList => {
            GroupGrantsListRequest::from_json(&body).map(GroupAdministrationRequest::GrantsList)
        }
        Route::GrantChange => match body.try_into_mut() {
            Ok(unique) => parse_group_grant_change_body_wiped(unique),
            Err(shared) => parse_group_grant_change_body_wiped(shared.to_vec()),
        }
        .map(GroupAdministrationRequest::GrantChange),
        Route::Catalog => AdministrationCatalogRequest::from_optional_json(&body)
            .map(GroupAdministrationRequest::Catalog),
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
        Ok(result) if route_matches_result(route, &result) => success(result, correlation_id),
        Ok(_) => GroupAdministrationRejection::ServiceUnavailable.response(&correlation_id),
        Err(rejection) => rejection.response(&correlation_id),
    }
}

fn route_matches_result(route: Route, result: &GroupAdministrationResult) -> bool {
    matches!(
        (route, result),
        (Route::List, GroupAdministrationResult::List(_))
            | (Route::View, GroupAdministrationResult::Projection(_))
            | (Route::Create, GroupAdministrationResult::Projection(_))
            | (Route::Update, GroupAdministrationResult::Projection(_))
            | (Route::Delete, GroupAdministrationResult::Deleted(_))
            | (Route::MembersList, GroupAdministrationResult::Members(_))
            | (
                Route::MemberChange,
                GroupAdministrationResult::MemberChanged(_)
            )
            | (Route::GrantsList, GroupAdministrationResult::Grants(_))
            | (
                Route::GrantChange,
                GroupAdministrationResult::GrantChanged(_)
            )
            | (Route::Catalog, GroupAdministrationResult::Catalog(_))
    )
}

fn success(result: GroupAdministrationResult, correlation_id: String) -> Response {
    if ResponseCorrelation::new(&correlation_id).is_none() {
        return unrenderable_response();
    }
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::OK;
    response
        .extensions_mut()
        .insert(GroupAdministrationEnvelope {
            result,
            correlation_id,
        });
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
fn page_limit(value: Option<usize>) -> Result<usize, GroupAdministrationInputRejected> {
    let limit = value.unwrap_or(DEFAULT_GROUPS_PAGE_LIMIT);
    if (1..=MAX_GROUPS_PAGE_LIMIT).contains(&limit) {
        Ok(limit)
    } else {
        Err(GroupAdministrationInputRejected)
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
fn encode_member_cursor(
    username: &str,
    public_id: &str,
) -> Result<String, GroupAdministrationInputRejected> {
    validate_text(username, MAX_NAME_BYTES)?;
    if !valid_public_id(public_id) {
        return Err(GroupAdministrationInputRejected);
    }
    let mut value =
        Vec::with_capacity(MEMBER_CURSOR_SCOPE.len() + username.len() + 1 + public_id.len());
    value.extend_from_slice(MEMBER_CURSOR_SCOPE);
    value.extend_from_slice(username.as_bytes());
    value.push(0);
    value.extend_from_slice(public_id.as_bytes());
    Ok(URL_SAFE_NO_PAD.encode(value))
}
fn decode_member_cursor(
    cursor: &str,
) -> Result<(String, String), GroupAdministrationInputRejected> {
    let decoded = canonical_cursor(cursor, MEMBER_CURSOR_SCOPE)?;
    let (username, public_id) = decoded
        .split_once('\0')
        .ok_or(GroupAdministrationInputRejected)?;
    validate_text(username, MAX_NAME_BYTES)?;
    if !valid_public_id(public_id) {
        return Err(GroupAdministrationInputRejected);
    }
    Ok((username.to_owned(), public_id.to_owned()))
}
fn encode_grant_cursor(
    grant: &GroupGrantProjection,
) -> Result<String, GroupAdministrationInputRejected> {
    grant.validate()?;
    let encoded = serde_json::to_vec(grant).map_err(|_| GroupAdministrationInputRejected)?;
    let mut value = Vec::with_capacity(GRANT_CURSOR_SCOPE.len() + encoded.len());
    value.extend_from_slice(GRANT_CURSOR_SCOPE);
    value.extend_from_slice(&encoded);
    Ok(URL_SAFE_NO_PAD.encode(value))
}
fn decode_grant_cursor(
    cursor: &str,
) -> Result<GroupGrantProjection, GroupAdministrationInputRejected> {
    let decoded = canonical_cursor(cursor, GRANT_CURSOR_SCOPE)?;
    let grant = strict_json(decoded.as_bytes())?;
    GroupGrantProjection::validate(&grant)?;
    Ok(grant)
}
fn canonical_cursor(
    cursor: &str,
    scope: &[u8],
) -> Result<String, GroupAdministrationInputRejected> {
    let decoded = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| GroupAdministrationInputRejected)?;
    if URL_SAFE_NO_PAD.encode(&decoded) != cursor {
        return Err(GroupAdministrationInputRejected);
    }
    let value = decoded
        .strip_prefix(scope)
        .ok_or(GroupAdministrationInputRejected)?;
    let value = std::str::from_utf8(value).map_err(|_| GroupAdministrationInputRejected)?;
    Ok(value.to_owned())
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{
            Request as HttpRequest,
            header::{ACCEPT, COOKIE, HOST, ORIGIN},
        },
    };
    use tower::ServiceExt as _;

    use super::*;
    use crate::{
        CSRF_COOKIE_NAME, CSRF_HEADER_NAME, SESSION_COOKIE_NAME,
        wiped_body_support::{SpyBuffer, parse_and_observe},
    };

    const ID: &str = "MTExMTExMTExMTExMTExMQ";
    const TICKET: &str = "MTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTE";
    const LISTENER: &str = "127.0.0.1:8443";
    const CORRELATION: &str = "group-correlation";

    fn capability(
        outcome: Result<GroupAdministrationResult, GroupAdministrationRejection>,
    ) -> GroupAdministrationCapability {
        GroupAdministrationCapability {
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
            .header(CSRF_HEADER_NAME, "csrf-token")
            .header(
                COOKIE,
                format!("{SESSION_COOKIE_NAME}=session-token; {CSRF_COOKIE_NAME}=csrf-token"),
            );
        if !body.is_empty() {
            builder = builder
                .header(CONTENT_TYPE, "application/json")
                .header("content-length", body.len().to_string());
        }
        builder.body(Body::from(body.to_owned())).unwrap()
    }

    fn with_extra_field(body: &str, field: &str) -> String {
        let mut extended = body
            .strip_suffix('}')
            .expect("the accepted request body must be an object")
            .to_owned();
        extended.push(',');
        extended.push_str(field);
        extended.push('}');
        extended
    }

    fn assert_ticket_request_body_wiped(
        request_type: &str,
        accepted: String,
        invalid_ticket: String,
        parse: fn(SpyBuffer) -> Result<(), GroupAdministrationInputRejected>,
    ) {
        let ticket_start = accepted
            .find(TICKET)
            .expect("the accepted request body must carry the ticket");
        let malformed = accepted[..ticket_start + TICKET.len() / 2].to_owned();
        let duplicate = format!(r#""grant_mutation_step_up_ticket":"{TICKET}""#);
        let padding = "x".repeat(MAX_GROUP_ADMINISTRATION_BODY_BYTES);
        let oversized = format!(r#""padding":"{padding}""#);
        let cases = [
            ("accepted", accepted.clone(), true),
            (
                "unknown-field",
                with_extra_field(&accepted, r#""extra":true"#),
                false,
            ),
            (
                "duplicate-field",
                with_extra_field(&accepted, &duplicate),
                false,
            ),
            ("malformed-ticket-string", malformed, false),
            ("invalid-ticket", invalid_ticket, false),
            ("oversized", with_extra_field(&accepted, &oversized), false),
        ];

        for (path, body, accepted) in cases {
            let (parsed, released) = parse_and_observe(&body, parse);
            assert_eq!(
                parsed.is_ok(),
                accepted,
                "{request_type} {path} path returned the wrong parse result"
            );
            assert_eq!(
                released,
                vec![0u8; released.len()],
                "{request_type} {path} path left readable ticket bytes behind"
            );
            assert!(!released.is_empty(), "{request_type} {path} body was empty");
        }
    }

    #[test]
    fn group_ticket_request_bodies_are_cleared_on_all_parse_paths() {
        assert_ticket_request_body_wiped(
            "delete",
            format!(r#"{{"public_id":"{ID}","grant_mutation_step_up_ticket":"{TICKET}"}}"#),
            format!(r#"{{"public_id":"{ID}","grant_mutation_step_up_ticket":"short"}}"#),
            |buffer| parse_group_delete_body_wiped(buffer).map(drop),
        );
        assert_ticket_request_body_wiped(
            "member change",
            format!(
                r#"{{"group_public_id":"{ID}","account_public_id":"{ID}","present":true,"grant_mutation_step_up_ticket":"{TICKET}"}}"#
            ),
            format!(
                r#"{{"group_public_id":"{ID}","account_public_id":"{ID}","present":true,"grant_mutation_step_up_ticket":"short"}}"#
            ),
            |buffer| parse_group_member_change_body_wiped(buffer).map(drop),
        );
        assert_ticket_request_body_wiped(
            "grant change",
            format!(
                r#"{{"group_public_id":"{ID}","grant":{{"type":"server_administration"}},"present":true,"grant_mutation_step_up_ticket":"{TICKET}"}}"#
            ),
            format!(
                r#"{{"group_public_id":"{ID}","grant":{{"type":"server_administration"}},"present":true,"grant_mutation_step_up_ticket":"short"}}"#
            ),
            |buffer| parse_group_grant_change_body_wiped(buffer).map(drop),
        );
    }

    #[test]
    fn group_ticket_schemas_retain_zeroizing_owners_before_validation() {
        fn assert_zeroizing(_: &Zeroizing<String>) {}

        let accepted_delete: DeleteBody = required_json(
            format!(r#"{{"public_id":"{ID}","grant_mutation_step_up_ticket":"{TICKET}"}}"#)
                .as_bytes(),
        )
        .unwrap();
        let rejected_delete: DeleteBody = required_json(
            format!(r#"{{"public_id":"invalid","grant_mutation_step_up_ticket":"{TICKET}"}}"#)
                .as_bytes(),
        )
        .unwrap();
        assert_zeroizing(&accepted_delete.grant_mutation_step_up_ticket);
        assert_zeroizing(&rejected_delete.grant_mutation_step_up_ticket);

        let accepted_member: MemberChangeBody = required_json(
            format!(
                r#"{{"group_public_id":"{ID}","account_public_id":"{ID}","present":true,"grant_mutation_step_up_ticket":"{TICKET}"}}"#
            )
            .as_bytes(),
        )
        .unwrap();
        let rejected_member: MemberChangeBody = required_json(
            format!(
                r#"{{"group_public_id":"{ID}","account_public_id":"invalid","present":true,"grant_mutation_step_up_ticket":"{TICKET}"}}"#
            )
            .as_bytes(),
        )
        .unwrap();
        assert_zeroizing(&accepted_member.grant_mutation_step_up_ticket);
        assert_zeroizing(&rejected_member.grant_mutation_step_up_ticket);

        let accepted_grant: GrantChangeBody = required_json(
            format!(
                r#"{{"group_public_id":"{ID}","grant":{{"type":"server_administration"}},"present":true,"grant_mutation_step_up_ticket":"{TICKET}"}}"#
            )
            .as_bytes(),
        )
        .unwrap();
        let rejected_grant: GrantChangeBody = required_json(
            format!(
                r#"{{"group_public_id":"{ID}","grant":{{"type":"operation","value":""}},"present":true,"grant_mutation_step_up_ticket":"{TICKET}"}}"#
            )
            .as_bytes(),
        )
        .unwrap();
        assert_zeroizing(&accepted_grant.grant_mutation_step_up_ticket);
        assert_zeroizing(&rejected_grant.grant_mutation_step_up_ticket);

        assert!(
            GroupsDeleteRequest::from_json(
                format!(r#"{{"public_id":"invalid","grant_mutation_step_up_ticket":"{TICKET}"}}"#)
                    .as_bytes()
            )
            .is_err()
        );
        assert!(
            GroupMemberChangeRequest::from_json(
                format!(
                    r#"{{"group_public_id":"{ID}","account_public_id":"invalid","present":true,"grant_mutation_step_up_ticket":"{TICKET}"}}"#
                )
                .as_bytes()
            )
            .is_err()
        );
        assert!(
            GroupGrantChangeRequest::from_json(
                format!(
                    r#"{{"group_public_id":"{ID}","grant":{{"type":"operation","value":""}},"present":true,"grant_mutation_step_up_ticket":"{TICKET}"}}"#
                )
                .as_bytes()
            )
            .is_err()
        );
    }

    async fn rendered(response: Response) -> String {
        if let Some(envelope) = response.extensions().get::<GroupAdministrationEnvelope>() {
            return envelope.serialize().unwrap().to_string();
        }
        if let Some(envelope) = response.extensions().get::<TypedJsonEnvelope>() {
            return envelope.serialize().to_string();
        }
        String::new()
    }

    #[test]
    fn strict_requests_accept_only_documented_values() {
        let omitted = GroupsListRequest::from_optional_json(b"").unwrap();
        assert_eq!(omitted.limit(), DEFAULT_GROUPS_PAGE_LIMIT);
        assert_eq!(omitted.after_name(), None);
        assert!(GroupsListRequest::from_optional_json(br#"{"limit":100}"#).is_ok());
        assert!(GroupsListRequest::from_optional_json(br#"{"limit":0}"#).is_err());
        for body in [
            br#"{"limit":null}"#.as_slice(),
            br#"{"limit":1,"cursor":null}"#.as_slice(),
        ] {
            assert!(GroupsListRequest::from_optional_json(body).is_err());
        }
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

    #[test]
    fn association_requests_are_strict_and_route_scoped() {
        let omitted = format!(r#"{{"group_public_id":"{ID}"}}"#);
        let members = GroupMembersListRequest::from_json(omitted.as_bytes()).unwrap();
        assert_eq!(members.limit(), DEFAULT_GROUPS_PAGE_LIMIT);
        assert_eq!(members.after(), None);
        let grants = GroupGrantsListRequest::from_json(omitted.as_bytes()).unwrap();
        assert_eq!(grants.limit(), DEFAULT_GROUPS_PAGE_LIMIT);
        assert_eq!(grants.after(), None);

        for body in [
            format!(r#"{{"group_public_id":"{ID}","limit":null}}"#),
            format!(r#"{{"group_public_id":"{ID}","limit":1,"cursor":null}}"#),
        ] {
            assert!(GroupMembersListRequest::from_json(body.as_bytes()).is_err());
            assert!(GroupGrantsListRequest::from_json(body.as_bytes()).is_err());
        }

        let member_cursor = encode_member_cursor("administrator", ID).unwrap();
        assert!(
            GroupMembersListRequest::from_json(
                format!(r#"{{"group_public_id":"{ID}","limit":1,"cursor":"{member_cursor}"}}"#)
                    .as_bytes()
            )
            .is_ok()
        );
        assert!(
            GroupGrantsListRequest::from_json(
                format!(r#"{{"group_public_id":"{ID}","cursor":"{member_cursor}"}}"#).as_bytes()
            )
            .is_err()
        );
        assert!(
            GroupMemberChangeRequest::from_json(
                format!(
                    r#"{{"group_public_id":"{ID}","account_public_id":"{ID}","present":true,"grant_mutation_step_up_ticket":"{TICKET}"}}"#
                )
                .as_bytes()
            )
            .is_ok()
        );
    }

    #[test]
    fn direct_grants_are_a_closed_strict_union() {
        for grant in [
            r#"{"type":"client_module","value":"web-ui"}"#,
            r#"{"type":"service_module","value":"zendesk"}"#,
            r#"{"type":"operation","value":"zendesk.ticket.read"}"#,
            r#"{"type":"server_administration"}"#,
        ] {
            let body = format!(
                r#"{{"group_public_id":"{ID}","grant":{grant},"present":true,"grant_mutation_step_up_ticket":"{TICKET}"}}"#
            );
            assert!(GroupGrantChangeRequest::from_json(body.as_bytes()).is_ok());
        }
        for grant in [
            r#"{"type":"server_administration","value":"unsafe"}"#,
            r#"{"type":"operation"}"#,
            r#"{"type":"unknown","value":"unsafe"}"#,
            r#"{"type":"client_module","value":"web-ui","extra":true}"#,
        ] {
            let body = format!(
                r#"{{"group_public_id":"{ID}","grant":{grant},"present":true,"grant_mutation_step_up_ticket":"{TICKET}"}}"#
            );
            assert!(GroupGrantChangeRequest::from_json(body.as_bytes()).is_err());
        }

        let grant = GroupGrantProjection::Operation {
            value: "zendesk.ticket.read".to_owned(),
        };
        let cursor = encode_grant_cursor(&grant).unwrap();
        let request = GroupGrantsListRequest::from_json(
            format!(r#"{{"group_public_id":"{ID}","cursor":"{cursor}"}}"#).as_bytes(),
        )
        .unwrap();
        assert_eq!(request.after(), Some(&grant));
        assert!(AdministrationCatalogRequest::from_optional_json(b"").is_ok());
        assert!(AdministrationCatalogRequest::from_optional_json(br#"{}"#).is_ok());
        assert!(AdministrationCatalogRequest::from_optional_json(br#"{"extra":true}"#).is_err());
    }

    #[test]
    fn catalog_and_typed_results_are_bounded_sorted_and_safe() {
        let catalog =
            AdministrationCatalog::new(vec!["web-ui".to_owned()], Vec::new(), Vec::new()).unwrap();
        let serialized = GroupAdministrationEnvelope {
            result: GroupAdministrationResult::Catalog(catalog),
            correlation_id: CORRELATION.to_owned(),
        }
        .serialize()
        .unwrap();
        assert_eq!(
            serialized.as_str(),
            r#"{"result":{"client_modules":["web-ui"],"service_modules":[],"operations":[]},"correlation_id":"group-correlation"}"#
        );
        for forbidden in ["account_id", "group_id", "audit_reference", "state_id"] {
            assert!(!serialized.contains(forbidden));
        }
        assert!(serialized.len() <= MAX_GROUP_ADMINISTRATION_RESPONSE_BYTES);

        assert!(
            AdministrationCatalog::new(
                vec!["zeta".to_owned(), "alpha".to_owned()],
                Vec::new(),
                Vec::new(),
            )
            .is_err()
        );
        assert!(
            AdministrationCatalog::new(
                (0..=MAX_ADMINISTRATION_CATALOG_ENTRIES)
                    .map(|index| format!("component-{index:03}"))
                    .collect(),
                Vec::new(),
                Vec::new(),
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn association_routes_reject_method_origin_session_csrf_schema_and_result_mismatch() {
        let page = GroupMembersPage {
            items: Vec::new(),
            next_cursor: None,
        };
        let declaration = GroupAdministrationDeclaration::new(capability(Ok(
            GroupAdministrationResult::Members(page),
        )));
        let body = format!(r#"{{"group_public_id":"{ID}"}}"#);

        let method = declaration
            .members_list_route()
            .oneshot(request(Method::POST, GROUP_MEMBERS_LIST_ROUTE, &body))
            .await
            .unwrap();
        assert_eq!(method.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(method.headers().get(ALLOW).unwrap(), "PUT");

        let mut origin = request(Method::PUT, GROUP_MEMBERS_LIST_ROUTE, &body);
        origin.headers_mut().insert(
            ORIGIN,
            HeaderValue::from_static("https://elsewhere.example"),
        );
        let origin = declaration
            .members_list_route()
            .oneshot(origin)
            .await
            .unwrap();
        assert_eq!(origin.status(), StatusCode::FORBIDDEN);
        assert!(rendered(origin).await.contains("request_origin_denied"));

        let mut session = request(Method::PUT, GROUP_MEMBERS_LIST_ROUTE, &body);
        session.headers_mut().remove(COOKIE);
        let session = declaration
            .members_list_route()
            .oneshot(session)
            .await
            .unwrap();
        assert_eq!(session.status(), StatusCode::UNAUTHORIZED);

        let mut csrf = request(Method::PUT, GROUP_MEMBERS_LIST_ROUTE, &body);
        csrf.headers_mut().remove(CSRF_HEADER_NAME);
        let csrf = declaration
            .members_list_route()
            .oneshot(csrf)
            .await
            .unwrap();
        assert_eq!(csrf.status(), StatusCode::UNAUTHORIZED);

        let schema = declaration
            .members_list_route()
            .oneshot(request(
                Method::PUT,
                GROUP_MEMBERS_LIST_ROUTE,
                &format!(r#"{{"group_public_id":"{ID}","confirmed":true}}"#),
            ))
            .await
            .unwrap();
        assert_eq!(schema.status(), StatusCode::BAD_REQUEST);

        let mismatch = GroupAdministrationDeclaration::new(capability(Ok(
            GroupAdministrationResult::Catalog(
                AdministrationCatalog::new(Vec::new(), Vec::new(), Vec::new()).unwrap(),
            ),
        )))
        .members_list_route()
        .oneshot(request(Method::PUT, GROUP_MEMBERS_LIST_ROUTE, &body))
        .await
        .unwrap();
        assert_eq!(mismatch.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn optional_fields_reject_present_null_at_routes() {
        let declaration = GroupAdministrationDeclaration::new(capability(Err(
            GroupAdministrationRejection::Conflict,
        )));
        let limit_null = format!(r#"{{"group_public_id":"{ID}","limit":null}}"#);
        let cursor_null = format!(r#"{{"group_public_id":"{ID}","limit":1,"cursor":null}}"#);

        for (route, path, body) in [
            (
                declaration.list_route(),
                GROUPS_LIST_ROUTE,
                r#"{"limit":null}"#,
            ),
            (
                declaration.list_route(),
                GROUPS_LIST_ROUTE,
                r#"{"limit":1,"cursor":null}"#,
            ),
            (
                declaration.members_list_route(),
                GROUP_MEMBERS_LIST_ROUTE,
                limit_null.as_str(),
            ),
            (
                declaration.members_list_route(),
                GROUP_MEMBERS_LIST_ROUTE,
                cursor_null.as_str(),
            ),
            (
                declaration.grants_list_route(),
                GROUP_GRANTS_LIST_ROUTE,
                limit_null.as_str(),
            ),
            (
                declaration.grants_list_route(),
                GROUP_GRANTS_LIST_ROUTE,
                cursor_null.as_str(),
            ),
        ] {
            let response = route
                .oneshot(request(Method::PUT, path, body))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert!(rendered(response).await.contains("bad_request"));
        }
    }
}
