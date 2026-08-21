//! Strict public contract for TOTP enablement and Log configuration administration.

use std::{fmt, io, pin::Pin, sync::Arc};

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
use serde::{Deserialize, Serialize, Serializer, ser::SerializeStruct};
use zeroize::Zeroizing;

use crate::{
    ExpectedOrigin, JSON_MEDIA_TYPE, WipedBody, accepts_json,
    authentication::{
        CorrelationSource, submitted_csrf_token, submitted_session_token, unrenderable_response,
    },
    has_request_body, single_header,
    typed_json::{ResponseCorrelation, StableCode, TypedJsonEnvelope, typed_json_response},
};

pub const TOTP_ENABLEMENT_PREVIEW_ROUTE: &str =
    "/api/v1/administration/mfa-modules/totp/enablement/preview";
pub const TOTP_ENABLEMENT_APPLY_ROUTE: &str =
    "/api/v1/administration/mfa-modules/totp/enablement/apply";
pub const LOG_CONFIGURATIONS_LIST_ROUTE: &str = "/api/v1/administration/log-configurations/list";
pub const LOG_CONFIGURATIONS_VIEW_ROUTE: &str = "/api/v1/administration/log-configurations/view";
pub const LOG_CONFIGURATIONS_CHANGE_ROUTE: &str =
    "/api/v1/administration/log-configurations/change";

pub const DEFAULT_LOG_CONFIGURATIONS_PAGE_LIMIT: usize = 50;
pub const MAX_LOG_CONFIGURATIONS_PAGE_LIMIT: usize = 100;
pub const MAX_CONFIGURATION_ADMINISTRATION_BODY_BYTES: usize = 256 * 1024;
pub const MAX_CONFIGURATION_ADMINISTRATION_RESPONSE_BYTES: usize = 512 * 1024;

const CURSOR_SCOPE: &[u8] = b"weavelit:/api/v1/administration/log-configurations/list:v1\0";
const MAX_NAME_BYTES: usize = 256;
const MAX_SETTING_KEY_BYTES: usize = 256;
const MAX_SETTING_VALUE_BYTES: usize = 4 * 1024;
const MAX_SETTINGS: usize = 64;
const TOKEN_BYTES: usize = 32;
const TOKEN_CHARS: usize = 43;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigurationAdministrationInputRejected;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigurationAdministrationRejection {
    BadRequest,
    SessionInvalid,
    RequestOriginDenied,
    AuthorizationDenied,
    MethodNotAllowed,
    NotFound,
    Conflict,
    ServiceUnavailable,
}

impl ConfigurationAdministrationRejection {
    pub const fn status(self) -> StatusCode {
        match self {
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::SessionInvalid => StatusCode::UNAUTHORIZED,
            Self::RequestOriginDenied | Self::AuthorizationDenied => StatusCode::FORBIDDEN,
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TotpPreviewBody {
    enabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TotpEnablementPreviewRequest {
    enabled: bool,
}

impl TotpEnablementPreviewRequest {
    fn from_json(body: &[u8]) -> Result<Self, ConfigurationAdministrationInputRejected> {
        Ok(Self {
            enabled: required_json::<TotpPreviewBody>(body)?.enabled,
        })
    }

    pub const fn enabled(self) -> bool {
        self.enabled
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TotpApplyBody {
    enabled: bool,
    totp_enablement_preview: Option<String>,
}

pub struct TotpEnablementApplyRequest {
    enabled: bool,
    preview: Option<Zeroizing<String>>,
}

impl fmt::Debug for TotpEnablementApplyRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TotpEnablementApplyRequest")
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

impl TotpEnablementApplyRequest {
    fn from_json(body: &[u8]) -> Result<Self, ConfigurationAdministrationInputRejected> {
        let parsed = required_json::<TotpApplyBody>(body)?;
        if parsed
            .totp_enablement_preview
            .as_deref()
            .is_some_and(|preview| !valid_token(preview))
        {
            return Err(ConfigurationAdministrationInputRejected);
        }
        Ok(Self {
            enabled: parsed.enabled,
            preview: parsed.totp_enablement_preview.map(Zeroizing::new),
        })
    }

    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn preview(&self) -> Option<&str> {
        self.preview.as_ref().map(|preview| preview.as_str())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListBody {
    limit: Option<usize>,
    cursor: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogConfigurationsListRequest {
    limit: usize,
    after_name: Option<String>,
}

impl LogConfigurationsListRequest {
    fn from_optional_json(body: &[u8]) -> Result<Self, ConfigurationAdministrationInputRejected> {
        let parsed = if body.is_empty() {
            ListBody {
                limit: None,
                cursor: None,
            }
        } else {
            strict_json(body)?
        };
        let limit = parsed
            .limit
            .unwrap_or(DEFAULT_LOG_CONFIGURATIONS_PAGE_LIMIT);
        if !(1..=MAX_LOG_CONFIGURATIONS_PAGE_LIMIT).contains(&limit) {
            return Err(ConfigurationAdministrationInputRejected);
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
struct ViewBody {
    configuration_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogConfigurationViewRequest {
    configuration_name: String,
}

impl LogConfigurationViewRequest {
    fn from_json(body: &[u8]) -> Result<Self, ConfigurationAdministrationInputRejected> {
        let parsed = required_json::<ViewBody>(body)?;
        validate_text(&parsed.configuration_name, MAX_NAME_BYTES)?;
        Ok(Self {
            configuration_name: parsed.configuration_name,
        })
    }

    pub fn configuration_name(&self) -> &str {
        &self.configuration_name
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LogSettingProjection {
    key: String,
    value: String,
}

impl LogSettingProjection {
    pub fn new(
        key: String,
        value: String,
    ) -> Result<Self, ConfigurationAdministrationInputRejected> {
        validate_text(&key, MAX_SETTING_KEY_BYTES)?;
        validate_text(&value, MAX_SETTING_VALUE_BYTES)?;
        Ok(Self { key, value })
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogTypeProjection {
    System,
    Audit,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LogAssignmentRequest {
    log_type: LogTypeProjection,
    configuration_name: String,
}

impl LogAssignmentRequest {
    pub const fn log_type(&self) -> LogTypeProjection {
        self.log_type
    }

    pub fn configuration_name(&self) -> &str {
        &self.configuration_name
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChangeBody {
    configuration_name: String,
    enabled: Option<bool>,
    settings: Option<Vec<LogSettingProjection>>,
    assignments: Option<Vec<LogAssignmentRequest>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogConfigurationChangeRequest {
    configuration_name: String,
    enabled: Option<bool>,
    settings: Option<Vec<LogSettingProjection>>,
    assignments: Vec<LogAssignmentRequest>,
}

impl LogConfigurationChangeRequest {
    fn from_json(body: &[u8]) -> Result<Self, ConfigurationAdministrationInputRejected> {
        let parsed = required_json::<ChangeBody>(body)?;
        validate_text(&parsed.configuration_name, MAX_NAME_BYTES)?;
        if parsed.enabled.is_none() && parsed.settings.is_none() && parsed.assignments.is_none() {
            return Err(ConfigurationAdministrationInputRejected);
        }
        let mut settings = parsed.settings;
        if let Some(values) = settings.as_mut() {
            if values.len() > MAX_SETTINGS
                || values.iter().any(|value| {
                    validate_text(value.key(), MAX_SETTING_KEY_BYTES).is_err()
                        || validate_text(value.value(), MAX_SETTING_VALUE_BYTES).is_err()
                })
            {
                return Err(ConfigurationAdministrationInputRejected);
            }
            values.sort_by(|left, right| left.key.cmp(&right.key));
            if values.windows(2).any(|pair| pair[0].key == pair[1].key) {
                return Err(ConfigurationAdministrationInputRejected);
            }
        }
        let assignments_supplied = parsed.assignments.is_some();
        let mut assignments = parsed.assignments.unwrap_or_default();
        for assignment in &assignments {
            validate_text(&assignment.configuration_name, MAX_NAME_BYTES)?;
        }
        assignments.sort_by_key(LogAssignmentRequest::log_type);
        if assignments
            .windows(2)
            .any(|pair| pair[0].log_type == pair[1].log_type)
        {
            return Err(ConfigurationAdministrationInputRejected);
        }
        if assignments_supplied
            && (assignments.len() != 2
                || assignments[0].log_type != LogTypeProjection::System
                || assignments[1].log_type != LogTypeProjection::Audit)
        {
            return Err(ConfigurationAdministrationInputRejected);
        }
        Ok(Self {
            configuration_name: parsed.configuration_name,
            enabled: parsed.enabled,
            settings,
            assignments,
        })
    }

    pub fn configuration_name(&self) -> &str {
        &self.configuration_name
    }

    pub const fn enabled(&self) -> Option<bool> {
        self.enabled
    }

    pub fn settings(&self) -> Option<&[LogSettingProjection]> {
        self.settings.as_deref()
    }

    pub fn assignments(&self) -> &[LogAssignmentRequest] {
        &self.assignments
    }
}

pub enum ConfigurationAdministrationRequest {
    TotpPreview(TotpEnablementPreviewRequest),
    TotpApply(TotpEnablementApplyRequest),
    LogList(LogConfigurationsListRequest),
    LogView(LogConfigurationViewRequest),
    LogChange(LogConfigurationChangeRequest),
}

pub struct ConfigurationAdministrationSubmission {
    pub request: ConfigurationAdministrationRequest,
    pub session_token: Zeroizing<String>,
    pub csrf_token: Zeroizing<String>,
    pub correlation_id: String,
    pub context: Extensions,
}

#[derive(Clone)]
pub struct TotpEnablementPreviewProjection {
    module: String,
    current_enabled: bool,
    desired_enabled: bool,
    affected_users: usize,
    preview: Zeroizing<String>,
}

impl TotpEnablementPreviewProjection {
    pub fn new(
        current_enabled: bool,
        desired_enabled: bool,
        affected_users: usize,
        preview: Zeroizing<String>,
    ) -> Result<Self, ConfigurationAdministrationInputRejected> {
        if !valid_token(&preview) {
            return Err(ConfigurationAdministrationInputRejected);
        }
        Ok(Self {
            module: "totp".to_owned(),
            current_enabled,
            desired_enabled,
            affected_users,
            preview,
        })
    }

    pub const fn current_enabled(&self) -> bool {
        self.current_enabled
    }

    pub const fn desired_enabled(&self) -> bool {
        self.desired_enabled
    }

    pub const fn affected_users(&self) -> usize {
        self.affected_users
    }
}

impl Serialize for TotpEnablementPreviewProjection {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("TotpEnablementPreviewProjection", 5)?;
        state.serialize_field("module", &self.module)?;
        state.serialize_field("current_enabled", &self.current_enabled)?;
        state.serialize_field("desired_enabled", &self.desired_enabled)?;
        state.serialize_field("affected_users", &self.affected_users)?;
        state.serialize_field("totp_enablement_preview", self.preview.as_str())?;
        state.end()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TotpEnablementApplied {
    module: String,
    current_enabled: bool,
    affected_users: usize,
}

impl TotpEnablementApplied {
    #[must_use]
    pub fn new(current_enabled: bool, affected_users: usize) -> Self {
        Self {
            module: "totp".to_owned(),
            current_enabled,
            affected_users,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LogConfigurationProjection {
    configuration_name: String,
    module: String,
    enabled: bool,
    settings: Vec<LogSettingProjection>,
    assigned_log_types: Vec<LogTypeProjection>,
}

impl LogConfigurationProjection {
    pub fn new(
        configuration_name: String,
        module: String,
        enabled: bool,
        settings: Vec<LogSettingProjection>,
        assigned_log_types: Vec<LogTypeProjection>,
    ) -> Result<Self, ConfigurationAdministrationInputRejected> {
        validate_text(&configuration_name, MAX_NAME_BYTES)?;
        validate_text(&module, MAX_NAME_BYTES)?;
        if settings.len() > MAX_SETTINGS
            || settings.windows(2).any(|pair| pair[0].key >= pair[1].key)
            || assigned_log_types.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(ConfigurationAdministrationInputRejected);
        }
        Ok(Self {
            configuration_name,
            module,
            enabled,
            settings,
            assigned_log_types,
        })
    }

    pub fn configuration_name(&self) -> &str {
        &self.configuration_name
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LogConfigurationsPage {
    items: Vec<LogConfigurationProjection>,
    next_cursor: Option<String>,
}

impl LogConfigurationsPage {
    pub fn from_ordered(
        request: &LogConfigurationsListRequest,
        projections: Vec<LogConfigurationProjection>,
    ) -> Result<Self, ConfigurationAdministrationInputRejected> {
        if projections
            .windows(2)
            .any(|pair| pair[0].configuration_name() >= pair[1].configuration_name())
        {
            return Err(ConfigurationAdministrationInputRejected);
        }
        let start = match request.after_name() {
            Some(after) => {
                projections
                    .binary_search_by(|value| value.configuration_name().cmp(after))
                    .map_err(|_| ConfigurationAdministrationInputRejected)?
                    + 1
            }
            None => 0,
        };
        let end = start.saturating_add(request.limit()).min(projections.len());
        let next_cursor = if end < projections.len() {
            projections
                .get(end - 1)
                .map(|value| encode_cursor(value.configuration_name()))
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

#[derive(Clone, Serialize)]
#[serde(untagged)]
pub enum ConfigurationAdministrationResult {
    TotpPreview(TotpEnablementPreviewProjection),
    TotpApplied(TotpEnablementApplied),
    LogList(LogConfigurationsPage),
    LogProjection(LogConfigurationProjection),
}

#[derive(Clone)]
pub struct ConfigurationAdministrationEnvelope {
    result: ConfigurationAdministrationResult,
    correlation_id: String,
}

impl ConfigurationAdministrationEnvelope {
    pub fn serialize(&self) -> Option<Zeroizing<String>> {
        #[derive(Serialize)]
        struct WireEnvelope<'a> {
            result: &'a ConfigurationAdministrationResult,
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
                MAX_CONFIGURATION_ADMINISTRATION_RESPONSE_BYTES,
            )),
        }
    }

    fn into_bytes(self) -> Zeroizing<Vec<u8>> {
        self.bytes
    }
}

impl io::Write for BoundedJsonWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self.bytes.len().saturating_add(buffer.len())
            > MAX_CONFIGURATION_ADMINISTRATION_RESPONSE_BYTES
        {
            return Err(io::Error::other("Configuration response exceeds its bound"));
        }
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub type ConfigurationAdministrationCommit = Arc<
    dyn Fn(
            ConfigurationAdministrationSubmission,
        ) -> Pin<
            Box<
                dyn Future<
                        Output = Result<
                            ConfigurationAdministrationResult,
                            ConfigurationAdministrationRejection,
                        >,
                    > + Send,
            >,
        > + Send
        + Sync,
>;

pub struct ConfigurationAdministrationCapability {
    pub expected_origin: ExpectedOrigin,
    pub correlate: CorrelationSource,
    pub execute: ConfigurationAdministrationCommit,
}

pub struct ConfigurationAdministrationDeclaration {
    capability: Arc<ConfigurationAdministrationCapability>,
}

impl ConfigurationAdministrationDeclaration {
    pub fn new(capability: ConfigurationAdministrationCapability) -> Self {
        Self {
            capability: Arc::new(capability),
        }
    }

    pub fn totp_preview_route(&self) -> MethodRouter {
        self.route(Route::TotpPreview)
    }

    pub fn totp_apply_route(&self) -> MethodRouter {
        self.route(Route::TotpApply)
    }

    pub fn log_list_route(&self) -> MethodRouter {
        self.route(Route::LogList)
    }

    pub fn log_view_route(&self) -> MethodRouter {
        self.route(Route::LogView)
    }

    pub fn log_change_route(&self) -> MethodRouter {
        self.route(Route::LogChange)
    }

    fn route(&self, route: Route) -> MethodRouter {
        let capability = Arc::clone(&self.capability);
        any(move |request| response(request, Arc::clone(&capability), route))
    }
}

#[derive(Clone, Copy)]
enum Route {
    TotpPreview,
    TotpApply,
    LogList,
    LogView,
    LogChange,
}

async fn response(
    request: Request,
    capability: Arc<ConfigurationAdministrationCapability>,
    route: Route,
) -> Response {
    let Some(correlation_id) = (capability.correlate)() else {
        return unrenderable_response();
    };
    let (parts, body) = request.into_parts();
    if let Err(rejection) = validate_configuration_administration_request(
        &parts.method,
        &parts.headers,
        capability.expected_origin,
        !matches!(route, Route::LogList),
    ) {
        return rejection.response(&correlation_id);
    }
    let Ok(body) = to_bytes(body, MAX_CONFIGURATION_ADMINISTRATION_BODY_BYTES).await else {
        return ConfigurationAdministrationRejection::BadRequest.response(&correlation_id);
    };
    let mut body = WipedBody::new(body.to_vec());
    let parsed = match route {
        Route::TotpPreview => TotpEnablementPreviewRequest::from_json(body.bytes())
            .map(ConfigurationAdministrationRequest::TotpPreview),
        Route::TotpApply => TotpEnablementApplyRequest::from_json(body.bytes())
            .map(ConfigurationAdministrationRequest::TotpApply),
        Route::LogList => LogConfigurationsListRequest::from_optional_json(body.bytes())
            .map(ConfigurationAdministrationRequest::LogList),
        Route::LogView => LogConfigurationViewRequest::from_json(body.bytes())
            .map(ConfigurationAdministrationRequest::LogView),
        Route::LogChange => LogConfigurationChangeRequest::from_json(body.bytes())
            .map(ConfigurationAdministrationRequest::LogChange),
    };
    let Ok(request) = parsed else {
        return ConfigurationAdministrationRejection::BadRequest.response(&correlation_id);
    };
    let Ok(session) = submitted_session_token(&parts.headers) else {
        return ConfigurationAdministrationRejection::SessionInvalid.response(&correlation_id);
    };
    let Ok(csrf) = submitted_csrf_token(&parts.headers) else {
        return ConfigurationAdministrationRejection::SessionInvalid.response(&correlation_id);
    };
    match (capability.execute)(ConfigurationAdministrationSubmission {
        request,
        session_token: Zeroizing::new(session.to_owned()),
        csrf_token: Zeroizing::new(csrf.to_owned()),
        correlation_id: correlation_id.clone(),
        context: parts.extensions,
    })
    .await
    {
        Ok(result) if route_matches_result(route, &result) => success(result, correlation_id),
        Ok(_) => ConfigurationAdministrationRejection::ServiceUnavailable.response(&correlation_id),
        Err(rejection) => rejection.response(&correlation_id),
    }
}

fn route_matches_result(route: Route, result: &ConfigurationAdministrationResult) -> bool {
    matches!(
        (route, result),
        (
            Route::TotpPreview,
            ConfigurationAdministrationResult::TotpPreview(_)
        ) | (
            Route::TotpApply,
            ConfigurationAdministrationResult::TotpApplied(_)
        ) | (
            Route::LogList,
            ConfigurationAdministrationResult::LogList(_)
        ) | (
            Route::LogView | Route::LogChange,
            ConfigurationAdministrationResult::LogProjection(_)
        )
    )
}

fn success(result: ConfigurationAdministrationResult, correlation_id: String) -> Response {
    if ResponseCorrelation::new(&correlation_id).is_none() {
        return unrenderable_response();
    }
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::OK;
    response
        .extensions_mut()
        .insert(ConfigurationAdministrationEnvelope {
            result,
            correlation_id,
        });
    response
}

pub fn validate_configuration_administration_request(
    method: &Method,
    headers: &HeaderMap,
    expected_origin: ExpectedOrigin,
    body_required: bool,
) -> Result<(), ConfigurationAdministrationRejection> {
    if method != Method::PUT {
        return Err(ConfigurationAdministrationRejection::MethodNotAllowed);
    }
    if !expected_origin.is_same_origin(headers) {
        return Err(ConfigurationAdministrationRejection::RequestOriginDenied);
    }
    if !accepts_json(headers) {
        return Err(ConfigurationAdministrationRejection::BadRequest);
    }
    if body_required || has_request_body(headers) {
        let value = single_header(headers, CONTENT_TYPE)
            .ok_or(ConfigurationAdministrationRejection::BadRequest)?;
        if value.as_bytes() != JSON_MEDIA_TYPE {
            return Err(ConfigurationAdministrationRejection::BadRequest);
        }
    } else if single_header(headers, CONTENT_TYPE).is_some() {
        return Err(ConfigurationAdministrationRejection::BadRequest);
    }
    submitted_session_token(headers)
        .and_then(|_| submitted_csrf_token(headers))
        .map_err(|_| ConfigurationAdministrationRejection::SessionInvalid)?;
    Ok(())
}

fn required_json<T: for<'de> Deserialize<'de>>(
    body: &[u8],
) -> Result<T, ConfigurationAdministrationInputRejected> {
    if body.is_empty() {
        return Err(ConfigurationAdministrationInputRejected);
    }
    strict_json(body)
}

fn strict_json<T: for<'de> Deserialize<'de>>(
    body: &[u8],
) -> Result<T, ConfigurationAdministrationInputRejected> {
    if body.len() > MAX_CONFIGURATION_ADMINISTRATION_BODY_BYTES {
        return Err(ConfigurationAdministrationInputRejected);
    }
    let mut deserializer = serde_json::Deserializer::from_slice(body);
    let value =
        T::deserialize(&mut deserializer).map_err(|_| ConfigurationAdministrationInputRejected)?;
    deserializer
        .end()
        .map_err(|_| ConfigurationAdministrationInputRejected)?;
    Ok(value)
}

fn validate_text(
    value: &str,
    maximum: usize,
) -> Result<(), ConfigurationAdministrationInputRejected> {
    if value.is_empty() || value.len() > maximum || value.chars().any(char::is_control) {
        return Err(ConfigurationAdministrationInputRejected);
    }
    Ok(())
}

fn valid_token(value: &str) -> bool {
    value.len() == TOKEN_CHARS
        && URL_SAFE_NO_PAD.decode(value).is_ok_and(|decoded| {
            decoded.len() == TOKEN_BYTES && URL_SAFE_NO_PAD.encode(decoded) == value
        })
}

fn encode_cursor(name: &str) -> Result<String, ConfigurationAdministrationInputRejected> {
    validate_text(name, MAX_NAME_BYTES)?;
    let mut bytes = Vec::with_capacity(CURSOR_SCOPE.len() + name.len());
    bytes.extend_from_slice(CURSOR_SCOPE);
    bytes.extend_from_slice(name.as_bytes());
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn decode_cursor(value: &str) -> Result<String, ConfigurationAdministrationInputRejected> {
    let decoded = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| ConfigurationAdministrationInputRejected)?;
    if URL_SAFE_NO_PAD.encode(&decoded) != value || !decoded.starts_with(CURSOR_SCOPE) {
        return Err(ConfigurationAdministrationInputRejected);
    }
    let name = std::str::from_utf8(&decoded[CURSOR_SCOPE.len()..])
        .map_err(|_| ConfigurationAdministrationInputRejected)?;
    validate_text(name, MAX_NAME_BYTES)?;
    Ok(name.to_owned())
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
    use crate::{CSRF_COOKIE_NAME, CSRF_HEADER_NAME, SESSION_COOKIE_NAME};

    const TOKEN: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const LISTENER: &str = "127.0.0.1:8443";
    const CORRELATION: &str = "configuration-correlation";

    fn projection(name: &str) -> LogConfigurationProjection {
        LogConfigurationProjection::new(
            name.to_owned(),
            "sqlite".to_owned(),
            true,
            Vec::new(),
            vec![LogTypeProjection::System, LogTypeProjection::Audit],
        )
        .unwrap()
    }

    fn capability(
        outcome: Result<ConfigurationAdministrationResult, ConfigurationAdministrationRejection>,
    ) -> ConfigurationAdministrationCapability {
        ConfigurationAdministrationCapability {
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

    async fn rendered(response: Response) -> String {
        if let Some(envelope) = response
            .extensions()
            .get::<ConfigurationAdministrationEnvelope>()
        {
            return envelope.serialize().unwrap().to_string();
        }
        if let Some(envelope) = response.extensions().get::<TypedJsonEnvelope>() {
            return envelope.serialize().to_string();
        }
        String::new()
    }

    #[test]
    fn request_schemas_are_exact_and_tokens_are_canonical() {
        assert!(TotpEnablementPreviewRequest::from_json(br#"{"enabled":false}"#).is_ok());
        assert!(
            TotpEnablementPreviewRequest::from_json(br#"{"enabled":false,"extra":1}"#).is_err()
        );
        assert!(
            TotpEnablementApplyRequest::from_json(
                format!(r#"{{"enabled":false,"totp_enablement_preview":"{TOKEN}"}}"#).as_bytes()
            )
            .is_ok()
        );
        assert!(
            TotpEnablementApplyRequest::from_json(
                br#"{"enabled":false,"totp_enablement_preview":"short"}"#
            )
            .is_err()
        );
        assert!(TotpEnablementApplyRequest::from_json(br#"{"enabled":false}"#).is_ok());
    }

    #[test]
    fn log_change_requires_a_complete_change_surface() {
        assert!(
            LogConfigurationChangeRequest::from_json(br#"{"configuration_name":"primary"}"#)
                .is_err()
        );
        assert!(
            LogConfigurationChangeRequest::from_json(
                br#"{"configuration_name":"primary","settings":[]}"#
            )
            .is_ok()
        );
        assert!(
            LogConfigurationChangeRequest::from_json(
                br#"{"configuration_name":"primary","assignments":[{"log_type":"audit","configuration_name":"primary"}]}"#
            )
            .is_err()
        );
        assert!(
            LogConfigurationChangeRequest::from_json(
                br#"{"configuration_name":"primary","enabled":true,"assignments":[{"log_type":"audit","configuration_name":"primary"},{"log_type":"audit","configuration_name":"other"}]}"#
            )
            .is_err()
        );
    }

    #[test]
    fn list_cursor_is_route_scoped_and_requires_an_exact_boundary() {
        let first = LogConfigurationsListRequest::from_optional_json(br#"{"limit":1}"#).unwrap();
        let page = LogConfigurationsPage::from_ordered(
            &first,
            vec![
                LogConfigurationProjection::new(
                    "first".to_owned(),
                    "sqlite".to_owned(),
                    true,
                    Vec::new(),
                    vec![LogTypeProjection::System],
                )
                .unwrap(),
                LogConfigurationProjection::new(
                    "second".to_owned(),
                    "sqlite".to_owned(),
                    true,
                    Vec::new(),
                    vec![LogTypeProjection::Audit],
                )
                .unwrap(),
            ],
        )
        .unwrap();
        let cursor = page.next_cursor.unwrap();
        let next = LogConfigurationsListRequest::from_optional_json(
            format!(r#"{{"cursor":"{cursor}"}}"#).as_bytes(),
        )
        .unwrap();
        assert_eq!(next.after_name(), Some("first"));
        assert!(decode_cursor("Zmlyc3Q").is_err());
    }

    #[test]
    fn projections_serialize_only_the_public_configuration_fields() {
        let projection = LogConfigurationProjection::new(
            "primary".to_owned(),
            "sqlite".to_owned(),
            true,
            Vec::new(),
            vec![LogTypeProjection::System, LogTypeProjection::Audit],
        )
        .unwrap();
        let rendered = serde_json::to_string(&projection).unwrap();
        assert_eq!(
            rendered,
            r#"{"configuration_name":"primary","module":"sqlite","enabled":true,"settings":[],"assigned_log_types":["system","audit"]}"#
        );
        for forbidden in [
            "generation",
            "path",
            "credential",
            "terminal",
            "record_id",
            "configuration_id",
        ] {
            assert!(!rendered.contains(forbidden));
        }
    }

    #[tokio::test]
    async fn list_route_accepts_only_its_exact_authenticated_same_origin_profile() {
        let list = LogConfigurationsPage::from_ordered(
            &LogConfigurationsListRequest::from_optional_json(b"").unwrap(),
            vec![projection("primary")],
        )
        .unwrap();
        let declaration = ConfigurationAdministrationDeclaration::new(capability(Ok(
            ConfigurationAdministrationResult::LogList(list),
        )));

        let accepted = declaration
            .log_list_route()
            .oneshot(request(Method::PUT, LOG_CONFIGURATIONS_LIST_ROUTE, ""))
            .await
            .unwrap();
        assert_eq!(accepted.status(), StatusCode::OK);
        assert!(
            rendered(accepted)
                .await
                .contains("\"configuration_name\":\"primary\"")
        );

        let method = declaration
            .log_list_route()
            .oneshot(request(Method::POST, LOG_CONFIGURATIONS_LIST_ROUTE, ""))
            .await
            .unwrap();
        assert_eq!(method.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(method.headers().get(ALLOW).unwrap(), "PUT");

        let mut origin = request(Method::PUT, LOG_CONFIGURATIONS_LIST_ROUTE, "");
        origin
            .headers_mut()
            .insert(ORIGIN, HeaderValue::from_static("https://other.example"));
        let origin = declaration.log_list_route().oneshot(origin).await.unwrap();
        assert_eq!(origin.status(), StatusCode::FORBIDDEN);
        assert!(rendered(origin).await.contains("request_origin_denied"));

        let mut csrf = request(Method::PUT, LOG_CONFIGURATIONS_LIST_ROUTE, "");
        csrf.headers_mut().remove(CSRF_HEADER_NAME);
        let csrf = declaration.log_list_route().oneshot(csrf).await.unwrap();
        assert_eq!(csrf.status(), StatusCode::UNAUTHORIZED);
        assert!(rendered(csrf).await.contains("session_invalid"));

        let mut unexpected_media = request(Method::PUT, LOG_CONFIGURATIONS_LIST_ROUTE, "");
        unexpected_media
            .headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let unexpected_media = declaration
            .log_list_route()
            .oneshot(unexpected_media)
            .await
            .unwrap();
        assert_eq!(unexpected_media.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn preview_route_requires_json_and_never_accepts_a_bodyless_request() {
        let declaration = ConfigurationAdministrationDeclaration::new(capability(Ok(
            ConfigurationAdministrationResult::TotpPreview(
                TotpEnablementPreviewProjection::new(
                    true,
                    false,
                    1,
                    Zeroizing::new(TOKEN.to_owned()),
                )
                .unwrap(),
            ),
        )));

        let accepted = declaration
            .totp_preview_route()
            .oneshot(request(
                Method::PUT,
                TOTP_ENABLEMENT_PREVIEW_ROUTE,
                r#"{"enabled":false}"#,
            ))
            .await
            .unwrap();
        assert_eq!(accepted.status(), StatusCode::OK);
        assert!(rendered(accepted).await.contains("totp_enablement_preview"));

        let missing = declaration
            .totp_preview_route()
            .oneshot(request(Method::PUT, TOTP_ENABLEMENT_PREVIEW_ROUTE, ""))
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::BAD_REQUEST);
    }
}
