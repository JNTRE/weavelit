//! Shared Client Module contract for the two-step Init submission protocol.
//!
//! Init is submitted in two requests. The first carries the complete
//! initialization request and receives the one-time private recovery key and
//! the delivery nonce that key must be proved against. The second carries the
//! same request again together with the proof of possession computed from the
//! delivered key, and completes the deployment.
//!
//! Splitting the submission is what makes the key one-time without making the
//! deployment initialized before the requesting client has demonstrably kept
//! it: the first request creates only a non-operational checkpoint, and the
//! second is the only request that can replace it. `PUT` is used for both,
//! because only `PUT` may carry a body and both requests carry one.
//!
//! This module owns the canonical routes, the request schemas, every header
//! precondition, the payload-free rejection contract, and the two typed
//! success envelopes. It owns no lifecycle authority, no key material, and no
//! orchestration: it hands a validated submission to a Server-core hook and
//! renders exactly what that hook returns.

use std::fmt;
use std::pin::Pin;
use std::sync::Arc;

use axum::{
    body::to_bytes,
    extract::Request,
    http::{Extensions, HeaderMap, HeaderValue, Method, StatusCode, header::ALLOW},
    response::Response,
    routing::{MethodRouter, any},
};
use serde::de::{self, Deserialize, DeserializeSeed, Deserializer, MapAccess, Visitor};
use zeroize::Zeroizing;

use crate::{
    ExpectedOrigin, JSON_MEDIA_TYPE, SelectedBackend, accepts_json, json_response_body,
    single_header,
    typed_json::{
        OpaqueToken, RecoveryKeyLine, ResponseCorrelation, StableCode, TypedJsonEnvelope,
        TypedResult, TypedValue, typed_json_response,
    },
};

/// The canonical route that prepares and delivers the one-time recovery key.
pub const INIT_RECOVERY_KEY_ROUTE: &str = "/api/v1/init/recovery-key";

/// The canonical route that finalizes Init against the delivered key.
pub const INIT_ROUTE: &str = "/api/v1/init";

/// The result field name that carries the delivered private recovery key.
const RECOVERY_KEY_FIELD: &str = "recovery_key";

/// The result field name that carries the delivery nonce.
const DELIVERY_NONCE_FIELD: &str = "delivery_nonce";

/// The result field name that reports the activated lifecycle state.
const LIFECYCLE_FIELD: &str = "lifecycle";

/// The only lifecycle value a completed Init reports.
const LIFECYCLE_INITIALIZED: &str = "initialized";

/// Largest request body accepted on either Init route.
///
/// Both bodies carry only bounded text and two bounded collections, so they
/// stay inside the listener's default body bound and are never given an
/// admitted transport profile of their own.
pub const MAX_INIT_BODY_BYTES: usize = 1024;

/// Most initial Log Module configurations accepted in one request.
///
/// This restates the Server-owned Init bound so the transport rejects an
/// oversized collection while rejecting is still free. The Init crate is not a
/// dependency here, so the bound is restated rather than imported, and the
/// Server revalidates it regardless.
pub const MAX_INIT_LOG_MODULES: usize = 16;

/// Most non-secret settings accepted in one initial Log Module configuration.
pub const MAX_INIT_LOG_MODULE_SETTINGS: usize = 64;

/// Most protected settings accepted in one initial Log Module configuration.
pub const MAX_INIT_PROTECTED_LOG_MODULE_SETTINGS: usize = 16;

/// Exact characters of unpadded URL-safe Base64 one proof of possession has.
///
/// The proof is an untruncated HMAC-SHA-256 value, so it is always 32 bytes and
/// always encodes to exactly this many characters.
pub const RECOVERY_PROOF_BASE64_CHARS: usize = 43;

// ---------------------------------------------------------------------------
// Submitted values
// ---------------------------------------------------------------------------

/// The first local Human User a submitted Init request describes.
///
/// The password is owned and cleared when dropped, so a rejected or abandoned
/// submission leaves no plaintext password behind in this crate.
pub struct InitAdministratorSubmission {
    /// Submitted username, still unvalidated as a bounded name.
    pub username: String,
    /// Submitted display name, absent when the request omitted the member.
    pub display_name: Option<String>,
    /// Submitted password, cleared when this value is dropped.
    pub password: Zeroizing<String>,
}

impl fmt::Debug for InitAdministratorSubmission {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("InitAdministratorSubmission(REDACTED)")
    }
}

/// One submitted non-secret Log Module setting.
#[derive(Debug)]
pub struct InitLogModuleSettingSubmission {
    /// Submitted setting key.
    pub key: String,
    /// Submitted non-secret setting value.
    pub value: String,
}

/// One submitted Log Module setting whose value requires at-rest protection.
///
/// The value is owned and cleared when dropped.
pub struct InitProtectedSettingSubmission {
    /// Submitted setting key.
    pub key: String,
    /// Submitted secret value, cleared when this value is dropped.
    pub value: Zeroizing<String>,
}

impl fmt::Debug for InitProtectedSettingSubmission {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("InitProtectedSettingSubmission(REDACTED)")
    }
}

/// One submitted initial Log Module configuration.
#[derive(Debug)]
pub struct InitLogModuleSubmission {
    /// Submitted Log Module identifier.
    pub module: String,
    /// Submitted configuration name, unique across the request.
    pub name: String,
    /// Whether the configuration is submitted as enabled.
    pub enabled: bool,
    /// Submitted non-secret settings.
    pub settings: Vec<InitLogModuleSettingSubmission>,
    /// Submitted secret settings.
    pub protected_settings: Vec<InitProtectedSettingSubmission>,
}

/// The complete initialization request both Init routes carry.
///
/// The transport decides only that this is well formed. Whether the names are
/// acceptable, the modules are compiled in, the assignments resolve, and the
/// confirmed backend is the one actually selected are all decided by the
/// Server-owned Init contract.
#[derive(Debug)]
pub struct InitRequestSubmission {
    /// The Application Database backend the client believes is selected.
    ///
    /// This confirms the client's view of the selection made through
    /// [`crate::APPLICATION_DATABASE_ROUTE`]. It carries no connection
    /// configuration, and it never selects a database.
    pub backend: SelectedBackend,
    /// The first local Human User.
    pub administrator: InitAdministratorSubmission,
    /// The initial Log Module configurations.
    pub log_modules: Vec<InitLogModuleSubmission>,
    /// The configuration name assigned to the System Log.
    pub system_log: String,
    /// The configuration name assigned to the Audit Log.
    pub audit_log: String,
}

/// A validated recovery-key preparation request handed to the Server core.
#[derive(Debug)]
pub struct InitRecoveryKeySubmission {
    /// The submitted initialization request.
    pub request: InitRequestSubmission,
    /// The admitted request's extensions, which carry the Server core's own
    /// admission permit.
    pub context: Extensions,
}

/// A validated finalization request handed to the Server core.
#[derive(Debug)]
pub struct InitFinalizeSubmission {
    /// The submitted initialization request.
    pub request: InitRequestSubmission,
    /// The submitted proof of possession, already checked for shape only.
    ///
    /// Whether it matches the checkpoint's expected proof is decided by the
    /// Server core, which holds the only value it could be compared against.
    pub recovery_key_proof: String,
    /// The admitted request's extensions, which carry the Server core's own
    /// admission permit.
    pub context: Extensions,
}

/// What the Server core returns after preparing the one-time key delivery.
///
/// The key is owned and cleared when dropped, so an unrenderable delivery
/// leaves no plaintext key behind in this crate.
pub struct InitRecoveryKeyPrepared {
    /// The canonical private recovery-key line, delivered exactly once.
    pub recovery_key: Zeroizing<String>,
    /// The delivery nonce the proof of possession is computed over.
    pub delivery_nonce: String,
    /// The Server-generated correlation identifier for this Init.
    pub correlation_id: String,
}

impl fmt::Debug for InitRecoveryKeyPrepared {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("InitRecoveryKeyPrepared(REDACTED)")
    }
}

/// What the Server core returns after a completed Init.
#[derive(Debug)]
pub struct InitCompleted {
    /// The Server-generated correlation identifier for this Init.
    pub correlation_id: String,
}

/// Server-core hook that prepares and returns the one-time recovery key.
pub type InitRecoveryKeyCommit = Arc<
    dyn Fn(
            InitRecoveryKeySubmission,
        )
            -> Pin<Box<dyn Future<Output = Result<InitRecoveryKeyPrepared, InitRejection>> + Send>>
        + Send
        + Sync,
>;

/// Server-core hook that verifies the proof and runs one Init to completion.
pub type InitFinalizeCommit = Arc<
    dyn Fn(
            InitFinalizeSubmission,
        ) -> Pin<Box<dyn Future<Output = Result<InitCompleted, InitRejection>> + Send>>
        + Send
        + Sync,
>;

/// The runtime collaborators a Client Module declares Init with.
pub struct InitCapability {
    /// The trusted authority every Init request must target.
    pub expected_origin: ExpectedOrigin,
    /// The hook that prepares and delivers the one-time recovery key.
    pub prepare_recovery_key: InitRecoveryKeyCommit,
    /// The hook that verifies the proof and completes the Init.
    pub finalize: InitFinalizeCommit,
}

/// A declared Init capability, split into its two mountable routes.
///
/// The Server core mounts each route on its own, because only a successfully
/// written key response may publish the finalization route. Handing the two
/// routes back separately is what lets the core publish one without the other.
pub struct InitDeclaration {
    capability: Arc<InitCapability>,
}

impl InitDeclaration {
    /// Declares Init over the supplied runtime collaborators.
    #[must_use]
    pub fn new(capability: InitCapability) -> Self {
        Self {
            capability: Arc::new(capability),
        }
    }

    /// Returns the preparation route mounted at [`INIT_RECOVERY_KEY_ROUTE`].
    pub fn recovery_key_route(&self) -> MethodRouter {
        let capability = Arc::clone(&self.capability);
        any(move |request| init_recovery_key_response(request, Arc::clone(&capability)))
    }

    /// Returns the finalization route mounted at [`INIT_ROUTE`].
    pub fn finalize_route(&self) -> MethodRouter {
        let capability = Arc::clone(&self.capability);
        any(move |request| init_finalize_response(request, Arc::clone(&capability)))
    }
}

// ---------------------------------------------------------------------------
// Rejection contract
// ---------------------------------------------------------------------------

/// The complete, payload-free rejection contract for both Init routes.
///
/// Every variant carries a fixed body and nothing else. No variant can report
/// which validation step failed beyond its stable code, and none can carry a
/// password, a Log Module secret, a recovery key, a delivery nonce, or a proof.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InitRejection {
    /// `400` for a malformed body, media type, or `Accept` value.
    BadRequest,
    /// `400` for a finalization that carried no proof of possession.
    RecoveryKeyConfirmationRequired,
    /// `400` for a proof that is malformed or does not match the checkpoint.
    RecoveryKeyConfirmationInvalid,
    /// `403` for a failed same-origin, `Host`, or CSRF header check.
    RequestOriginDenied,
    /// `405` for any method other than `PUT`.
    MethodNotAllowed,
    /// `409` for a lifecycle state that no longer permits this Init operation.
    AlreadyInitialized,
    /// `500` for request validation, persistence, logging, or sealing failure.
    InitializationFailed,
    /// `503` for a backend, persistence, or integrity failure.
    ServiceUnavailable,
}

impl InitRejection {
    /// Every variant this contract can render.
    ///
    /// The listener bounds a fixed-profile body against a compile-time
    /// allowlist and redacts anything absent from it, so a variant that is
    /// never walked through that step can reach the wire as a generic redacted
    /// body without any test noticing. This slice exists so that walk is over
    /// the whole contract rather than over a restatement of it.
    pub const ALL: &'static [Self] = &[
        Self::BadRequest,
        Self::RecoveryKeyConfirmationRequired,
        Self::RecoveryKeyConfirmationInvalid,
        Self::RequestOriginDenied,
        Self::MethodNotAllowed,
        Self::AlreadyInitialized,
        Self::InitializationFailed,
        Self::ServiceUnavailable,
    ];

    /// Returns the documented status code.
    #[must_use]
    pub const fn status(self) -> StatusCode {
        match self {
            Self::BadRequest
            | Self::RecoveryKeyConfirmationRequired
            | Self::RecoveryKeyConfirmationInvalid => StatusCode::BAD_REQUEST,
            Self::RequestOriginDenied => StatusCode::FORBIDDEN,
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::AlreadyInitialized => StatusCode::CONFLICT,
            Self::InitializationFailed => StatusCode::INTERNAL_SERVER_ERROR,
            Self::ServiceUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    /// Returns the documented fixed JSON body.
    #[must_use]
    pub const fn body(self) -> &'static str {
        match self {
            Self::BadRequest => "{\"error\":\"bad_request\"}",
            Self::RecoveryKeyConfirmationRequired => {
                "{\"error\":\"recovery_key_confirmation_required\"}"
            }
            Self::RecoveryKeyConfirmationInvalid => {
                "{\"error\":\"recovery_key_confirmation_invalid\"}"
            }
            Self::RequestOriginDenied => "{\"error\":\"request_origin_denied\"}",
            Self::MethodNotAllowed => "{\"error\":\"method_not_allowed\"}",
            Self::AlreadyInitialized => "{\"error\":\"already_initialized\"}",
            Self::InitializationFailed => "{\"error\":\"initialization_failed\"}",
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

/// Validates every header precondition of either Init request.
///
/// The same-origin and CSRF trust check runs before media-type validation so a
/// cross-site request is denied without revealing negotiation detail. Both
/// routes share one predicate, so they cannot disagree about what a trusted
/// Init request looks like.
pub fn validate_init_request(
    method: &Method,
    headers: &HeaderMap,
    expected_origin: ExpectedOrigin,
) -> Result<(), InitRejection> {
    if method != Method::PUT {
        return Err(InitRejection::MethodNotAllowed);
    }
    if !expected_origin.is_trusted(headers) {
        return Err(InitRejection::RequestOriginDenied);
    }
    let content_type = single_header(headers, axum::http::header::CONTENT_TYPE)
        .ok_or(InitRejection::BadRequest)?;
    if content_type.as_bytes() != JSON_MEDIA_TYPE || !accepts_json(headers) {
        return Err(InitRejection::BadRequest);
    }
    Ok(())
}

/// Returns the well-formed proof the finalization request presented.
///
/// The proof is checked for shape only. Whether it matches the checkpoint's
/// expected value is decided by the Server core. An absent member is its own
/// category, so a client that never submitted a proof is told what to do rather
/// than told its proof was wrong.
fn submitted_recovery_proof(submitted: Option<String>) -> Result<String, InitRejection> {
    let proof = submitted.ok_or(InitRejection::RecoveryKeyConfirmationRequired)?;
    if proof.is_empty() {
        return Err(InitRejection::RecoveryKeyConfirmationRequired);
    }
    if proof.len() != RECOVERY_PROOF_BASE64_CHARS
        || !proof
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(InitRejection::RecoveryKeyConfirmationInvalid);
    }
    Ok(proof)
}

// ---------------------------------------------------------------------------
// Request schema
// ---------------------------------------------------------------------------

/// Whether the parsed body may carry a proof of possession.
#[derive(Clone, Copy, Eq, PartialEq)]
enum ProofPolicy {
    /// The preparation route, where the member is an unknown field.
    Forbidden,
    /// The finalization route, where the member is expected.
    Required,
}

/// The strictly validated body both Init routes accept.
///
/// Every visitor here is written by hand rather than derived because a derived
/// struct also accepts its JSON array form, which would let a password or a
/// Log Module secret be submitted through a shape the API contract does not
/// document. An unknown field, a duplicate key, a missing field, a wrongly
/// typed value, an oversized collection, the array form, trailing content, and
/// an oversized body are all rejected.
struct InitBody {
    request: InitRequestSubmission,
    recovery_key_proof: Option<String>,
}

const INIT_FIELDS: &[&str] = &[
    "database",
    "administrator",
    "log_modules",
    "system_log",
    "audit_log",
    "recovery_key_proof",
];

const PREPARATION_FIELDS: &[&str] = &[
    "database",
    "administrator",
    "log_modules",
    "system_log",
    "audit_log",
];

struct InitBodySeed {
    proof: ProofPolicy,
}

impl<'de> DeserializeSeed<'de> for InitBodySeed {
    type Value = InitBody;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_map(InitBodyVisitor { proof: self.proof })
    }
}

struct InitBodyVisitor {
    proof: ProofPolicy,
}

impl<'de> Visitor<'de> for InitBodyVisitor {
    type Value = InitBody;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an initialization request object")
    }

    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
        let known = match self.proof {
            ProofPolicy::Forbidden => PREPARATION_FIELDS,
            ProofPolicy::Required => INIT_FIELDS,
        };
        let mut database: Option<DatabaseConfirmation> = None;
        let mut administrator: Option<AdministratorBody> = None;
        let mut log_modules: Option<Vec<LogModuleBody>> = None;
        let mut system_log: Option<String> = None;
        let mut audit_log: Option<String> = None;
        let mut recovery_key_proof: Option<String> = None;

        while let Some(field) = map.next_key::<String>()? {
            match field.as_str() {
                "database" => assign(&mut database, map.next_value()?, "database")?,
                "administrator" => {
                    assign(&mut administrator, map.next_value()?, "administrator")?;
                }
                "log_modules" => {
                    let modules: Vec<LogModuleBody> = map.next_value()?;
                    if modules.is_empty() || modules.len() > MAX_INIT_LOG_MODULES {
                        return Err(de::Error::invalid_length(modules.len(), &self));
                    }
                    assign(&mut log_modules, modules, "log_modules")?;
                }
                "system_log" => assign(&mut system_log, map.next_value()?, "system_log")?,
                "audit_log" => assign(&mut audit_log, map.next_value()?, "audit_log")?,
                "recovery_key_proof" if self.proof == ProofPolicy::Required => {
                    assign(
                        &mut recovery_key_proof,
                        map.next_value()?,
                        "recovery_key_proof",
                    )?;
                }
                unknown => return Err(de::Error::unknown_field(unknown, known)),
            }
        }

        let administrator =
            administrator.ok_or_else(|| de::Error::missing_field("administrator"))?;
        Ok(InitBody {
            request: InitRequestSubmission {
                backend: database
                    .ok_or_else(|| de::Error::missing_field("database"))?
                    .backend,
                administrator: InitAdministratorSubmission {
                    username: administrator.username,
                    display_name: administrator.display_name,
                    password: administrator.password,
                },
                log_modules: log_modules
                    .ok_or_else(|| de::Error::missing_field("log_modules"))?
                    .into_iter()
                    .map(LogModuleBody::into_submission)
                    .collect(),
                system_log: system_log.ok_or_else(|| de::Error::missing_field("system_log"))?,
                audit_log: audit_log.ok_or_else(|| de::Error::missing_field("audit_log"))?,
            },
            recovery_key_proof,
        })
    }
}

fn assign<T, E: de::Error>(slot: &mut Option<T>, value: T, field: &'static str) -> Result<(), E> {
    if slot.is_some() {
        return Err(de::Error::duplicate_field(field));
    }
    *slot = Some(value);
    Ok(())
}

/// The client's confirmation of the Application Database already selected.
struct DatabaseConfirmation {
    backend: SelectedBackend,
}

impl<'de> Deserialize<'de> for DatabaseConfirmation {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BodyVisitor;

        impl<'de> Visitor<'de> for BodyVisitor {
            type Value = DatabaseConfirmation;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a selected database confirmation object")
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut backend: Option<SelectedBackend> = None;
                while let Some(field) = map.next_key::<String>()? {
                    if field != "backend" {
                        return Err(de::Error::unknown_field(&field, &["backend"]));
                    }
                    assign(&mut backend, map.next_value()?, "backend")?;
                }
                Ok(DatabaseConfirmation {
                    backend: backend.ok_or_else(|| de::Error::missing_field("backend"))?,
                })
            }
        }

        deserializer.deserialize_map(BodyVisitor)
    }
}

/// The submitted first-administrator object, whose password clears on drop.
///
/// The password is wrapped as it is read, so no plaintext copy outlives the
/// parse even when a later field rejects the body.
struct AdministratorBody {
    username: String,
    display_name: Option<String>,
    password: Zeroizing<String>,
}

const ADMINISTRATOR_FIELDS: &[&str] = &["username", "display_name", "password"];

impl<'de> Deserialize<'de> for AdministratorBody {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BodyVisitor;

        impl<'de> Visitor<'de> for BodyVisitor {
            type Value = AdministratorBody;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a first administrator object")
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut username: Option<String> = None;
                let mut display_name: Option<String> = None;
                let mut password: Option<Zeroizing<String>> = None;
                while let Some(field) = map.next_key::<String>()? {
                    match field.as_str() {
                        "username" => assign(&mut username, map.next_value()?, "username")?,
                        "display_name" => {
                            assign(&mut display_name, map.next_value()?, "display_name")?;
                        }
                        "password" => {
                            assign(&mut password, Zeroizing::new(map.next_value()?), "password")?;
                        }
                        unknown => {
                            return Err(de::Error::unknown_field(unknown, ADMINISTRATOR_FIELDS));
                        }
                    }
                }
                Ok(AdministratorBody {
                    username: username.ok_or_else(|| de::Error::missing_field("username"))?,
                    display_name,
                    password: password.ok_or_else(|| de::Error::missing_field("password"))?,
                })
            }
        }

        deserializer.deserialize_map(BodyVisitor)
    }
}

/// One submitted Log Module configuration object.
struct LogModuleBody {
    module: String,
    name: String,
    enabled: bool,
    settings: Vec<SettingBody>,
    protected_settings: Vec<ProtectedSettingBody>,
}

const LOG_MODULE_FIELDS: &[&str] = &[
    "module",
    "name",
    "enabled",
    "settings",
    "protected_settings",
];

impl LogModuleBody {
    fn into_submission(self) -> InitLogModuleSubmission {
        InitLogModuleSubmission {
            module: self.module,
            name: self.name,
            enabled: self.enabled,
            settings: self
                .settings
                .into_iter()
                .map(|setting| InitLogModuleSettingSubmission {
                    key: setting.key,
                    value: setting.value,
                })
                .collect(),
            protected_settings: self
                .protected_settings
                .into_iter()
                .map(|setting| InitProtectedSettingSubmission {
                    key: setting.key,
                    value: setting.value,
                })
                .collect(),
        }
    }
}

impl<'de> Deserialize<'de> for LogModuleBody {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BodyVisitor;

        impl<'de> Visitor<'de> for BodyVisitor {
            type Value = LogModuleBody;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an initial Log Module configuration object")
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut module: Option<String> = None;
                let mut name: Option<String> = None;
                let mut enabled: Option<bool> = None;
                let mut settings: Option<Vec<SettingBody>> = None;
                let mut protected_settings: Option<Vec<ProtectedSettingBody>> = None;
                while let Some(field) = map.next_key::<String>()? {
                    match field.as_str() {
                        "module" => assign(&mut module, map.next_value()?, "module")?,
                        "name" => assign(&mut name, map.next_value()?, "name")?,
                        "enabled" => assign(&mut enabled, map.next_value()?, "enabled")?,
                        "settings" => {
                            let values: Vec<SettingBody> = map.next_value()?;
                            if values.len() > MAX_INIT_LOG_MODULE_SETTINGS {
                                return Err(de::Error::invalid_length(values.len(), &self));
                            }
                            assign(&mut settings, values, "settings")?;
                        }
                        "protected_settings" => {
                            let values: Vec<ProtectedSettingBody> = map.next_value()?;
                            if values.len() > MAX_INIT_PROTECTED_LOG_MODULE_SETTINGS {
                                return Err(de::Error::invalid_length(values.len(), &self));
                            }
                            assign(&mut protected_settings, values, "protected_settings")?;
                        }
                        unknown => {
                            return Err(de::Error::unknown_field(unknown, LOG_MODULE_FIELDS));
                        }
                    }
                }
                Ok(LogModuleBody {
                    module: module.ok_or_else(|| de::Error::missing_field("module"))?,
                    name: name.ok_or_else(|| de::Error::missing_field("name"))?,
                    enabled: enabled.ok_or_else(|| de::Error::missing_field("enabled"))?,
                    settings: settings.ok_or_else(|| de::Error::missing_field("settings"))?,
                    protected_settings: protected_settings
                        .ok_or_else(|| de::Error::missing_field("protected_settings"))?,
                })
            }
        }

        deserializer.deserialize_map(BodyVisitor)
    }
}

/// One submitted non-secret setting object.
struct SettingBody {
    key: String,
    value: String,
}

impl<'de> Deserialize<'de> for SettingBody {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let (key, value) = deserialize_setting(deserializer, "a setting object")?;
        Ok(SettingBody { key, value })
    }
}

/// One submitted protected setting object, whose value clears on drop.
struct ProtectedSettingBody {
    key: String,
    value: Zeroizing<String>,
}

impl<'de> Deserialize<'de> for ProtectedSettingBody {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let (key, value) = deserialize_setting(deserializer, "a protected setting object")?;
        Ok(ProtectedSettingBody {
            key,
            // The parsed allocation is moved rather than copied, so the only
            // plaintext copy of the value is the one that clears on drop.
            value: Zeroizing::new(value),
        })
    }
}

/// Parses the `{"key":...,"value":...}` shape both setting objects share.
fn deserialize_setting<'de, D: Deserializer<'de>>(
    deserializer: D,
    expecting: &'static str,
) -> Result<(String, String), D::Error> {
    struct BodyVisitor {
        expecting: &'static str,
    }

    impl<'de> Visitor<'de> for BodyVisitor {
        type Value = (String, String);

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(self.expecting)
        }

        fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
            let mut key: Option<String> = None;
            let mut value: Option<String> = None;
            while let Some(field) = map.next_key::<String>()? {
                match field.as_str() {
                    "key" => assign(&mut key, map.next_value()?, "key")?,
                    "value" => assign(&mut value, map.next_value()?, "value")?,
                    unknown => return Err(de::Error::unknown_field(unknown, &["key", "value"])),
                }
            }
            Ok((
                key.ok_or_else(|| de::Error::missing_field("key"))?,
                value.ok_or_else(|| de::Error::missing_field("value"))?,
            ))
        }
    }

    deserializer.deserialize_map(BodyVisitor { expecting })
}

/// Parses one exact accepted Init body.
///
/// The body bound is checked before parsing, so an oversized body is refused
/// without allocating anything it contains.
fn parse_init_body(body: &[u8], proof: ProofPolicy) -> Result<InitBody, InitRejection> {
    if body.len() > MAX_INIT_BODY_BYTES {
        return Err(InitRejection::BadRequest);
    }
    let mut deserializer = serde_json::Deserializer::from_slice(body);
    let parsed = InitBodySeed { proof }
        .deserialize(&mut deserializer)
        .map_err(|_| InitRejection::BadRequest)?;
    deserializer.end().map_err(|_| InitRejection::BadRequest)?;
    Ok(parsed)
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

async fn init_recovery_key_response(request: Request, capability: Arc<InitCapability>) -> Response {
    let (parts, body) = request.into_parts();
    if let Err(rejection) =
        validate_init_request(&parts.method, &parts.headers, capability.expected_origin)
    {
        return rejection.response();
    }
    let Ok(body) = to_bytes(body, MAX_INIT_BODY_BYTES).await else {
        return InitRejection::BadRequest.response();
    };
    let parsed = match parse_init_body(&body, ProofPolicy::Forbidden) {
        Ok(parsed) => parsed,
        Err(rejection) => return rejection.response(),
    };
    drop(body);

    match (capability.prepare_recovery_key)(InitRecoveryKeySubmission {
        request: parsed.request,
        context: parts.extensions,
    })
    .await
    {
        Ok(prepared) => init_recovery_key_prepared_response(&prepared),
        Err(rejection) => rejection.response(),
    }
}

async fn init_finalize_response(request: Request, capability: Arc<InitCapability>) -> Response {
    let (parts, body) = request.into_parts();
    if let Err(rejection) =
        validate_init_request(&parts.method, &parts.headers, capability.expected_origin)
    {
        return rejection.response();
    }
    let Ok(body) = to_bytes(body, MAX_INIT_BODY_BYTES).await else {
        return InitRejection::BadRequest.response();
    };
    let parsed = match parse_init_body(&body, ProofPolicy::Required) {
        Ok(parsed) => parsed,
        Err(rejection) => return rejection.response(),
    };
    drop(body);
    let recovery_key_proof = match submitted_recovery_proof(parsed.recovery_key_proof) {
        Ok(proof) => proof,
        Err(rejection) => return rejection.response(),
    };

    match (capability.finalize)(InitFinalizeSubmission {
        request: parsed.request,
        recovery_key_proof,
        context: parts.extensions,
    })
    .await
    {
        Ok(completed) => init_completed_response(&completed),
        Err(rejection) => rejection.response(),
    }
}

/// Renders the only response that may ever carry a private recovery key.
///
/// The key is returned in the typed envelope alone. It is never placed in a
/// header, a redirect target, or a cookie, so it cannot be logged by an
/// intermediary or replayed from a browser history entry. A value that is not
/// one canonical line falls back to the payload-free failure rather than being
/// rendered through a looser shape.
fn init_recovery_key_prepared_response(prepared: &InitRecoveryKeyPrepared) -> Response {
    let Some(line) = RecoveryKeyLine::new(&prepared.recovery_key) else {
        return InitRejection::InitializationFailed.response();
    };
    let Some(nonce) = OpaqueToken::new(&prepared.delivery_nonce) else {
        return InitRejection::InitializationFailed.response();
    };
    let Some(result) = typed_field(RECOVERY_KEY_FIELD, TypedValue::RecoveryKey(line)) else {
        return InitRejection::InitializationFailed.response();
    };
    let Some(nonce_field) = StableCode::new(DELIVERY_NONCE_FIELD) else {
        return InitRejection::InitializationFailed.response();
    };
    let Some(result) = result.with_field(nonce_field, TypedValue::Token(nonce)) else {
        return InitRejection::InitializationFailed.response();
    };
    match ResponseCorrelation::new(&prepared.correlation_id) {
        Some(correlation_id) => typed_json_response(
            StatusCode::OK,
            TypedJsonEnvelope::Result {
                result,
                correlation_id,
            },
        ),
        None => InitRejection::InitializationFailed.response(),
    }
}

/// Renders the completion envelope of an activated Init.
fn init_completed_response(completed: &InitCompleted) -> Response {
    let Some(state) = StableCode::new(LIFECYCLE_INITIALIZED) else {
        return InitRejection::InitializationFailed.response();
    };
    let Some(result) = typed_field(LIFECYCLE_FIELD, TypedValue::Code(state)) else {
        return InitRejection::InitializationFailed.response();
    };
    match ResponseCorrelation::new(&completed.correlation_id) {
        Some(correlation_id) => typed_json_response(
            StatusCode::OK,
            TypedJsonEnvelope::Result {
                result,
                correlation_id,
            },
        ),
        None => InitRejection::InitializationFailed.response(),
    }
}

fn typed_field(name: &str, value: TypedValue) -> Option<TypedResult> {
    TypedResult::new().with_field(StableCode::new(name)?, value)
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};

    use super::{
        INIT_ROUTE, InitBodySeed, InitCompleted, InitRecoveryKeyPrepared, InitRejection,
        MAX_INIT_BODY_BYTES, MAX_INIT_LOG_MODULES, ProofPolicy, RECOVERY_PROOF_BASE64_CHARS,
        init_completed_response, init_recovery_key_prepared_response, parse_init_body,
        submitted_recovery_proof, validate_init_request,
    };
    use crate::{CSRF_HEADER_NAME, ExpectedOrigin, SelectedBackend, typed_json::TypedJsonEnvelope};
    use serde::de::DeserializeSeed;
    use zeroize::Zeroizing;

    const PASSWORD: &str = "correct horse battery staple";
    const USERNAME: &str = "administrator";
    const DISPLAY_NAME: &str = "Site Administrator";
    const SECRET_SETTING: &str = "provider-token";
    const PROOF: &str = "0123456789abcdefghijklmnopqrstuvwxyzABCDEFG";
    const NONCE: &str = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG";
    const RECOVERY_KEY: &str = "AGE-SECRET-KEY-1QQPZRY9X8GF2TVDW0S3JN54KHCE6MUA7L";
    const CORRELATION: &str = "0123456789abcdef";

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

    /// The exact body a Web UI would submit, with the proof member optional.
    fn body(proof: Option<&str>) -> String {
        let proof = proof.map_or_else(String::new, |proof| {
            format!(",\"recovery_key_proof\":\"{proof}\"")
        });
        format!(
            "{{\"database\":{{\"backend\":\"sqlite\"}},\
             \"administrator\":{{\"username\":\"{USERNAME}\",\
             \"display_name\":\"{DISPLAY_NAME}\",\"password\":\"{PASSWORD}\"}},\
             \"log_modules\":[{{\"module\":\"log-sqlite\",\"name\":\"system\",\
             \"enabled\":true,\"settings\":[{{\"key\":\"path\",\"value\":\"system.db\"}}],\
             \"protected_settings\":[{{\"key\":\"token\",\"value\":\"{SECRET_SETTING}\"}}]}}],\
             \"system_log\":\"system\",\"audit_log\":\"audit\"{proof}}}"
        )
    }

    fn envelope(response: &axum::response::Response) -> String {
        response
            .extensions()
            .get::<TypedJsonEnvelope>()
            .expect("an Init success renders a typed envelope")
            .serialize()
    }

    /// Returns only the rejection side, so a schema expectation never depends
    /// on the accepted body being comparable.
    fn refused(body: &str, proof: ProofPolicy) -> Option<InitRejection> {
        parse_init_body(body.as_bytes(), proof).err()
    }

    #[test]
    fn both_routes_require_their_method_origin_and_media_type() {
        assert_eq!(
            validate_init_request(
                &Method::PUT,
                &trusted_headers("application/json"),
                expected_origin()
            ),
            Ok(())
        );
        assert_eq!(
            validate_init_request(
                &Method::POST,
                &trusted_headers("application/json"),
                expected_origin()
            ),
            Err(InitRejection::MethodNotAllowed)
        );

        let mut untrusted = trusted_headers("application/json");
        untrusted.remove(CSRF_HEADER_NAME);
        assert_eq!(
            validate_init_request(&Method::PUT, &untrusted, expected_origin()),
            Err(InitRejection::RequestOriginDenied)
        );

        assert_eq!(
            validate_init_request(
                &Method::PUT,
                &trusted_headers("application/octet-stream"),
                expected_origin()
            ),
            Err(InitRejection::BadRequest)
        );
    }

    #[test]
    fn the_accepted_body_carries_every_submitted_value_through_its_accessors() {
        let parsed = parse_init_body(body(Some(PROOF)).as_bytes(), ProofPolicy::Required)
            .expect("the fixture body must be accepted");

        // Each expectation is a value the fixture body actually carries, so a
        // silently emptied field would fail these rather than pass them.
        assert!(!PASSWORD.is_empty() && !SECRET_SETTING.is_empty());
        assert_eq!(parsed.request.backend, SelectedBackend::Sqlite);
        assert_eq!(parsed.request.administrator.username, USERNAME);
        assert_eq!(
            parsed.request.administrator.display_name.as_deref(),
            Some(DISPLAY_NAME)
        );
        assert_eq!(parsed.request.administrator.password.as_str(), PASSWORD);
        assert_eq!(parsed.request.system_log, "system");
        assert_eq!(parsed.request.audit_log, "audit");
        assert_eq!(parsed.request.log_modules.len(), 1);

        let module = &parsed.request.log_modules[0];
        assert_eq!(module.module, "log-sqlite");
        assert_eq!(module.name, "system");
        assert!(module.enabled);
        assert_eq!(module.settings[0].key, "path");
        assert_eq!(module.settings[0].value, "system.db");
        assert_eq!(module.protected_settings[0].key, "token");
        assert_eq!(module.protected_settings[0].value.as_str(), SECRET_SETTING);
        assert_eq!(parsed.recovery_key_proof.as_deref(), Some(PROOF));
    }

    #[test]
    fn an_absent_display_name_is_accepted_and_reported_as_absent() {
        let without = body(None).replace(&format!(",\"display_name\":\"{DISPLAY_NAME}\""), "");
        let parsed = parse_init_body(without.as_bytes(), ProofPolicy::Forbidden)
            .expect("an omitted display name must be accepted");
        assert_eq!(parsed.request.administrator.display_name, None);
    }

    #[test]
    fn the_schema_accepts_only_its_exact_shape() {
        for rejected in [
            String::new(),
            "{}".to_owned(),
            "[]".to_owned(),
            format!("{}{{}}", body(None)),
            body(None).replace("\"system_log\":\"system\",", ""),
            body(None).replace("\"enabled\":true", "\"enabled\":\"true\""),
            body(None).replace("\"backend\":\"sqlite\"", "\"backend\":\"postgres\""),
            body(None).replace("\"audit_log\"", "\"audit_logs\""),
            body(None).replace("\"log_modules\":[", "\"log_modules\":[[],"),
            format!("{},\"extra\":1}}", body(None).trim_end_matches('}')),
        ] {
            assert_eq!(
                refused(&rejected, ProofPolicy::Forbidden),
                Some(InitRejection::BadRequest),
                "{rejected}"
            );
        }
    }

    #[test]
    fn the_preparation_route_treats_a_submitted_proof_as_an_unknown_field() {
        assert_eq!(
            refused(&body(Some(PROOF)), ProofPolicy::Forbidden),
            Some(InitRejection::BadRequest)
        );
        assert!(parse_init_body(body(Some(PROOF)).as_bytes(), ProofPolicy::Required).is_ok());
    }

    #[test]
    fn an_oversized_body_or_collection_is_rejected() {
        let oversized = format!(
            "{{\"database\":{{\"backend\":\"sqlite\"}},\"padding\":\"{}\"}}",
            "a".repeat(MAX_INIT_BODY_BYTES)
        );
        assert!(oversized.len() > MAX_INIT_BODY_BYTES);
        assert_eq!(
            refused(&oversized, ProofPolicy::Forbidden),
            Some(InitRejection::BadRequest)
        );

        // More configurations than the collection bound cannot fit inside the
        // body bound, so the count check is exercised past the size check to
        // prove it is the collection bound rejecting and not the body bound.
        let module = "{\"module\":\"m\",\"name\":\"n\",\"enabled\":true,\
                      \"settings\":[],\"protected_settings\":[]}";
        let modules = std::iter::repeat_n(module, MAX_INIT_LOG_MODULES + 1)
            .collect::<Vec<_>>()
            .join(",");
        let over_limit = format!(
            "{{\"database\":{{\"backend\":\"sqlite\"}},\
             \"administrator\":{{\"username\":\"a\",\"password\":\"b\"}},\
             \"log_modules\":[{modules}],\"system_log\":\"n\",\"audit_log\":\"o\"}}"
        );
        let mut deserializer = serde_json::Deserializer::from_slice(over_limit.as_bytes());
        assert!(
            InitBodySeed {
                proof: ProofPolicy::Forbidden,
            }
            .deserialize(&mut deserializer)
            .is_err()
        );

        // The same body one configuration shorter parses, so the rejection
        // above is the count and not the shape.
        let within = over_limit.replacen(&format!("{module},"), "", 1);
        let mut deserializer = serde_json::Deserializer::from_slice(within.as_bytes());
        assert!(
            InitBodySeed {
                proof: ProofPolicy::Forbidden,
            }
            .deserialize(&mut deserializer)
            .is_ok()
        );
    }

    #[test]
    fn a_proof_is_required_when_absent_and_invalid_when_misshapen() {
        assert_eq!(
            submitted_recovery_proof(None),
            Err(InitRejection::RecoveryKeyConfirmationRequired)
        );
        assert_eq!(
            submitted_recovery_proof(Some(String::new())),
            Err(InitRejection::RecoveryKeyConfirmationRequired)
        );
        for misshapen in [
            "short",
            &"a".repeat(RECOVERY_PROOF_BASE64_CHARS + 1),
            &"+".repeat(RECOVERY_PROOF_BASE64_CHARS),
            &"=".repeat(RECOVERY_PROOF_BASE64_CHARS),
        ] {
            assert_eq!(
                submitted_recovery_proof(Some(misshapen.to_owned())),
                Err(InitRejection::RecoveryKeyConfirmationInvalid),
                "{misshapen}"
            );
        }
        assert_eq!(
            submitted_recovery_proof(Some(PROOF.to_owned())),
            Ok(PROOF.to_owned())
        );
    }

    #[test]
    fn both_successes_render_their_typed_envelopes() {
        let prepared = init_recovery_key_prepared_response(&InitRecoveryKeyPrepared {
            recovery_key: Zeroizing::new(RECOVERY_KEY.to_owned()),
            delivery_nonce: NONCE.to_owned(),
            correlation_id: CORRELATION.to_owned(),
        });
        assert_eq!(prepared.status(), StatusCode::OK);
        assert_eq!(
            envelope(&prepared),
            format!(
                "{{\"result\":{{\"recovery_key\":\"{RECOVERY_KEY}\",\
                 \"delivery_nonce\":\"{NONCE}\"}},\"correlation_id\":\"{CORRELATION}\"}}"
            )
        );

        let completed = init_completed_response(&InitCompleted {
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
        for unrenderable in [
            InitRecoveryKeyPrepared {
                recovery_key: Zeroizing::new("age1lowercase".to_owned()),
                delivery_nonce: NONCE.to_owned(),
                correlation_id: CORRELATION.to_owned(),
            },
            InitRecoveryKeyPrepared {
                recovery_key: Zeroizing::new(RECOVERY_KEY.to_owned()),
                delivery_nonce: "not a token".to_owned(),
                correlation_id: CORRELATION.to_owned(),
            },
            InitRecoveryKeyPrepared {
                recovery_key: Zeroizing::new(RECOVERY_KEY.to_owned()),
                delivery_nonce: NONCE.to_owned(),
                correlation_id: "NOT VALID".to_owned(),
            },
        ] {
            let response = init_recovery_key_prepared_response(&unrenderable);
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
            assert!(response.extensions().get::<TypedJsonEnvelope>().is_none());
        }

        let response = init_completed_response(&InitCompleted {
            correlation_id: "NOT VALID".to_owned(),
        });
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn every_rejection_has_a_distinct_bounded_fixed_body() {
        let mut seen: Vec<&'static str> = Vec::new();
        for rejection in InitRejection::ALL {
            let rejection = *rejection;
            assert!(rejection.body().len() <= 128, "{rejection:?}");
            assert!(!seen.contains(&rejection.body()), "{rejection:?}");
            seen.push(rejection.body());

            let response = rejection.response();
            assert_eq!(response.status(), rejection.status());
            assert_eq!(
                response.headers().get(axum::http::header::ALLOW).is_some(),
                rejection == InitRejection::MethodNotAllowed
            );
        }
        assert_eq!(seen.len(), 8);
    }

    /// No submitted secret and no delivered key reaches a rendered form.
    ///
    /// The submitted values are read back through their accessors rather than
    /// through a `Debug` rendering, because the bounded and clearing types
    /// deliberately redact in `Debug` and an assertion against that rendering
    /// would pass whether or not the value was ever carried.
    #[test]
    fn no_rendered_form_discloses_a_password_secret_or_delivered_key() {
        let parsed = parse_init_body(body(Some(PROOF)).as_bytes(), ProofPolicy::Required)
            .expect("the fixture body must be accepted");

        // The needles are present in the parsed request by construction, so the
        // absence assertions below cannot pass vacuously.
        assert_eq!(parsed.request.administrator.password.as_str(), PASSWORD);
        assert_eq!(
            parsed.request.log_modules[0].protected_settings[0]
                .value
                .as_str(),
            SECRET_SETTING
        );

        let rendered = format!("{:?} {}", parsed.request, INIT_ROUTE);
        for secret in [PASSWORD, SECRET_SETTING] {
            assert!(!secret.is_empty());
            assert!(!rendered.contains(secret), "{rendered}");
        }

        let prepared = InitRecoveryKeyPrepared {
            recovery_key: Zeroizing::new(RECOVERY_KEY.to_owned()),
            delivery_nonce: NONCE.to_owned(),
            correlation_id: CORRELATION.to_owned(),
        };
        assert_eq!(prepared.recovery_key.as_str(), RECOVERY_KEY);
        assert!(!format!("{prepared:?}").contains(RECOVERY_KEY));

        for rejection in InitRejection::ALL {
            let body = rejection.body();
            for secret in [PASSWORD, SECRET_SETTING, RECOVERY_KEY, NONCE, PROOF] {
                assert!(!body.contains(secret), "{rejection:?}: {body}");
            }
        }
    }
}
