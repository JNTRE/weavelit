//! Typed JSON response envelopes for the typed response profile.
//!
//! The listener serializes these values itself instead of forwarding a route's
//! bytes, so the only shapes that can reach the wire are the two envelopes the
//! API contract approves. Every part of an envelope is drawn from a validated
//! closed character set, so no escaping is required and no request content, no
//! message, no field path, no trace, and no dependency name can appear.
//!
//! The envelopes live in the shared Client Module crate because they are part
//! of the API contract every Client Module route answers with, not part of any
//! one route's implementation. The Server core only serializes them and
//! enforces their derived byte bound at the transport boundary.

use std::fmt::Write;

use axum::{body::Body, http::StatusCode, response::Response};
use zeroize::Zeroizing;

use crate::cookie::CookieEffect;

/// Closed marker for a typed response that discloses a one-time secret.
///
/// Only this module can name or construct this type. The named secret-response
/// builders below attach it and the listener observes it through
/// [`has_secret_disclosure_effect`], so a route can supply no cache-header name,
/// value, or effect value.
#[derive(Clone)]
struct SecretDisclosureEffect {
    _private: (),
}

impl SecretDisclosureEffect {
    const fn new() -> Self {
        Self { _private: () }
    }
}

/// Reports whether a response carries the closed one-time-secret effect.
///
/// This is deliberately observation-only. The effect type is private, so a
/// caller outside this module cannot construct, clone, remove, or insert it.
///
/// ```compile_fail
/// use weavelit_module_client::typed_json::SecretDisclosureEffect;
/// ```
#[must_use]
pub fn has_secret_disclosure_effect(response: &Response) -> bool {
    response
        .extensions()
        .get::<SecretDisclosureEffect>()
        .is_some()
}

/// Longest stable code or result field name the typed profile serializes.
pub const MAX_STABLE_CODE_BYTES: usize = 48;

/// Longest opaque token the typed profile serializes.
///
/// A token is an independent, high-entropy, single-use bearer value a route
/// returns to the client that requested it, such as the Restore submission
/// ticket. It is bounded by the stable-code bound so a token field can never
/// cost more than a code field, and the derived envelope bound below therefore
/// does not change when a route returns one.
pub const MAX_OPAQUE_TOKEN_BYTES: usize = MAX_STABLE_CODE_BYTES;

/// Longest correlation identifier the typed profile serializes.
///
/// This matches the canonical correlation-identifier bound recorded by
/// `weavelit_server_database::MAX_LOG_CORRELATION_IDENTIFIER_LENGTH`, so an
/// envelope can carry any correlation value the Server already records. That
/// crate is not a dependency here, so the bound is restated rather than
/// imported.
pub const MAX_RESPONSE_CORRELATION_BYTES: usize = 64;

/// Most fields one typed result object may carry.
pub const MAX_TYPED_RESULT_FIELDS: usize = 4;

/// Longest provisioning URI the typed profile serializes.
///
/// A provisioning URI is the only result value that is neither a code nor a
/// bearer token: it is a structured `otpauth://` value an authenticator reads,
/// so it carries reserved URI punctuation a code or token may not. The bound is
/// what remains of the typed envelope's own byte bound once the largest
/// enrollment envelope's other parts are accounted for, so disclosing one can
/// never push a response past a bound the listener would redact it for. A URI
/// that would exceed it is refused rather than shortened.
pub const MAX_PROVISIONING_URI_BYTES: usize = 288;

/// Longest canonical recovery-key line the typed profile serializes.
///
/// This matches the canonical recovery-key bound recorded by
/// `weavelit_server_recovery_key::MAX_RECOVERY_KEY_LENGTH`, so the one Init
/// response that delivers a key can carry any line this Server can encode. That
/// crate is not a dependency here, so the bound is restated rather than
/// imported.
///
/// Like [`MAX_PROVISIONING_URI_BYTES`], this bound is what the single envelope
/// that discloses the value can afford rather than what four fields of the
/// largest result object could carry. The delivery envelope carries exactly one
/// recovery-key field and one token field, so disclosing one can never push a
/// response past the bound the listener would redact it for.
pub const MAX_RECOVERY_KEY_LINE_BYTES: usize = 128;

/// Capacity reserved before any typed envelope is serialized.
///
/// The closed envelope shapes below can construct at most 504 bytes, so this
/// 512-byte capacity is both the listener's enforced typed-body bound and
/// enough to prevent appending a secret result value from reallocating a prior
/// plaintext allocation.
pub const MAX_TYPED_JSON_BODY_BYTES: usize = 512;

/// A stable, lowercase, dependency-neutral code or result field name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StableCode(String);

impl StableCode {
    /// Accepts only `[a-z0-9_]` within the code bound.
    #[must_use]
    pub fn new(value: &str) -> Option<Self> {
        if value.is_empty() || value.len() > MAX_STABLE_CODE_BYTES {
            return None;
        }
        if !value
            .bytes()
            .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'_'))
        {
            return None;
        }
        Some(Self(value.to_owned()))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

/// An opaque single-use bearer token a route returns exactly once.
///
/// The accepted set is unpadded URL-safe Base64, so a token is serialized
/// without escaping and a value that is not a canonical token cannot be
/// returned at all.
///
/// The wrapped value is a bearer secret, so this type renders nothing: it
/// implements neither `Debug` nor `Display`, and no comparison.
#[derive(Clone)]
pub struct OpaqueToken(Zeroizing<String>);

impl OpaqueToken {
    /// Accepts only `[A-Za-z0-9_-]` within the token bound.
    #[must_use]
    pub fn new(value: &str) -> Option<Self> {
        if value.is_empty() || value.len() > MAX_OPAQUE_TOKEN_BYTES {
            return None;
        }
        if !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return None;
        }
        Some(Self(Zeroizing::new(value.to_owned())))
    }

    fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// A bounded `otpauth://` provisioning URI disclosed exactly once.
///
/// The accepted set is the unreserved URI characters plus the percent sign and
/// the delimiters this workspace's provisioning URI is built from. It excludes
/// the quote, the backslash, and every control character, so a URI is
/// serialized without escaping and no value outside the shape a provisioning
/// URI can take is returnable at all.
///
/// The wrapped value is one-time provisioning data, so this type renders
/// nothing: it implements neither `Debug` nor `Display`, and no comparison.
#[derive(Clone)]
pub struct ProvisioningUri(Zeroizing<String>);

impl ProvisioningUri {
    /// Accepts only `[A-Za-z0-9-._~%:/?&=]` within the provisioning bound.
    #[must_use]
    pub fn new(value: &str) -> Option<Self> {
        if value.is_empty() || value.len() > MAX_PROVISIONING_URI_BYTES {
            return None;
        }
        if !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'-' | b'.' | b'_' | b'~' | b'%' | b':' | b'/' | b'?' | b'&' | b'='
                )
        }) {
            return None;
        }
        Some(Self(Zeroizing::new(value.to_owned())))
    }

    fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// A canonical age recovery-key line delivered exactly once.
///
/// The accepted set is the canonical uppercase age identity alphabet: ASCII
/// uppercase letters, digits, and the Bech32 separator. It excludes the quote,
/// the backslash, and every control character, so a line is serialized without
/// escaping and no value outside the shape a canonical identity can take is
/// returnable at all.
///
/// The wrapped value is the delivered private recovery key, so this type
/// renders nothing: it implements neither `Debug` nor `Display`, and no
/// comparison. It is cleared when the last clone is dropped.
#[derive(Clone)]
pub struct RecoveryKeyLine(Zeroizing<String>);

impl RecoveryKeyLine {
    /// Accepts only `[A-Z0-9-]` within the recovery-key line bound.
    #[must_use]
    pub fn new(value: &str) -> Option<Self> {
        if value.is_empty() || value.len() > MAX_RECOVERY_KEY_LINE_BYTES {
            return None;
        }
        if !value
            .bytes()
            .all(|byte| matches!(byte, b'A'..=b'Z' | b'0'..=b'9' | b'-'))
        {
            return None;
        }
        Some(Self(Zeroizing::new(value.to_owned())))
    }

    fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// The response-envelope rendering of a correlation identifier.
///
/// The canonical correlation identifier permits any bounded printable text,
/// including characters JSON would have to escape. This type narrows that set
/// to `[a-z0-9-]` so an envelope is serialized without escaping and no request
/// content can change its shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResponseCorrelation(String);

impl ResponseCorrelation {
    /// Accepts only `[a-z0-9-]` within the correlation bound.
    #[must_use]
    pub fn new(value: &str) -> Option<Self> {
        if value.is_empty() || value.len() > MAX_RESPONSE_CORRELATION_BYTES {
            return None;
        }
        if !value
            .bytes()
            .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-'))
        {
            return None;
        }
        Some(Self(value.to_owned()))
    }

    /// Borrows the trusted Server-controlled value for correlation-header rendering.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Closed set of values a typed result field may carry.
#[derive(Clone)]
pub enum TypedValue {
    /// A JSON boolean.
    Boolean(bool),
    /// A JSON number restricted to unsigned integers.
    Unsigned(u64),
    /// A stable code emitted as a JSON string.
    Code(StableCode),
    /// An opaque single-use token emitted as a JSON string.
    Token(OpaqueToken),
    /// A one-time provisioning URI emitted as a JSON string.
    Uri(ProvisioningUri),
    /// A one-time recovery-key line emitted as a JSON string.
    RecoveryKey(RecoveryKeyLine),
}

/// The bounded object a typed success envelope carries as `result`.
#[derive(Clone, Default)]
pub struct TypedResult {
    fields: Vec<(StableCode, TypedValue)>,
}

impl TypedResult {
    /// Builds an empty result object.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one field, or returns `None` once the field bound is reached.
    #[must_use]
    pub fn with_field(mut self, name: StableCode, value: TypedValue) -> Option<Self> {
        if self.fields.len() == MAX_TYPED_RESULT_FIELDS {
            return None;
        }
        self.fields.push((name, value));
        Some(self)
    }
}

/// The only JSON shapes the typed response profile may emit.
#[derive(Clone)]
pub enum TypedJsonEnvelope {
    /// `{"result":{...},"correlation_id":"..."}`.
    Result {
        /// The structured result object.
        result: TypedResult,
        /// The Server-generated correlation identifier.
        correlation_id: ResponseCorrelation,
    },
    /// `{"error":"stable_code","correlation_id":"..."}`.
    Error {
        /// The stable, redacted, dependency-neutral error code.
        error: StableCode,
        /// The Server-generated correlation identifier.
        correlation_id: ResponseCorrelation,
    },
}

impl TypedJsonEnvelope {
    /// Serializes the envelope into its approved wire shape.
    #[must_use]
    pub fn serialize(&self) -> Zeroizing<String> {
        let mut text = Zeroizing::new(String::with_capacity(MAX_TYPED_JSON_BODY_BYTES));
        match self {
            Self::Result {
                result,
                correlation_id,
            } => {
                text.push_str("{\"result\":{");
                for (index, (name, value)) in result.fields.iter().enumerate() {
                    if index > 0 {
                        text.push(',');
                    }
                    let _ = write!(text, "\"{}\":", name.as_str());
                    match value {
                        TypedValue::Boolean(value) => {
                            let _ = write!(text, "{value}");
                        }
                        TypedValue::Unsigned(value) => {
                            let _ = write!(text, "{value}");
                        }
                        TypedValue::Code(code) => {
                            let _ = write!(text, "\"{}\"", code.as_str());
                        }
                        TypedValue::Token(token) => {
                            let _ = write!(text, "\"{}\"", token.as_str());
                        }
                        TypedValue::Uri(uri) => {
                            let _ = write!(text, "\"{}\"", uri.as_str());
                        }
                        TypedValue::RecoveryKey(line) => {
                            let _ = write!(text, "\"{}\"", line.as_str());
                        }
                    }
                }
                let _ = write!(
                    text,
                    "}},\"correlation_id\":\"{}\"}}",
                    correlation_id.as_str()
                );
                text
            }
            Self::Error {
                error,
                correlation_id,
            } => {
                let _ = write!(
                    text,
                    "{{\"error\":\"{}\",\"correlation_id\":\"{}\"}}",
                    error.as_str(),
                    correlation_id.as_str()
                );
                text
            }
        }
    }
}

/// Builds a route response the listener serializes under the typed profile.
///
/// The response carries no header of its own: the listener supplies the media
/// type from the response profile, so a route cannot emit a cross-origin
/// header, a cache directive, or any other header text through this path. The
/// closed cookie and secret-disclosure effects are typed values that the
/// listener renders itself; neither accepts header text from a route.
#[must_use]
pub fn typed_json_response(status: StatusCode, envelope: TypedJsonEnvelope) -> Response {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = status;
    response.extensions_mut().insert(envelope);
    response
}

/// Builds a typed response carrying the closed one-time-secret marker.
///
/// This is crate-private so external route implementations cannot opt an
/// arbitrary response into the effect. Callers still supply no header text.
#[must_use]
pub(crate) fn typed_json_secret_response(
    status: StatusCode,
    envelope: TypedJsonEnvelope,
) -> Response {
    let mut response = typed_json_response(status, envelope);
    response
        .extensions_mut()
        .insert(SecretDisclosureEffect::new());
    response
}

/// Marks a non-typed-envelope response owned by this crate as a secret disclosure.
pub(crate) fn mark_secret_disclosure(response: &mut Response) {
    response
        .extensions_mut()
        .insert(SecretDisclosureEffect::new());
}

/// Builds a typed response that also carries one closed cookie effect.
///
/// The effect is a value, not header text: the listener validates and renders
/// it, and replaces the whole response with its fixed redacted failure if it
/// does not render. A route therefore cannot emit a partially applied cookie
/// effect, and cannot emit one at all without also emitting a typed envelope.
#[must_use]
pub fn typed_json_response_with_cookies(
    status: StatusCode,
    envelope: TypedJsonEnvelope,
    cookies: CookieEffect,
) -> Response {
    let mut response = typed_json_response(status, envelope);
    response.extensions_mut().insert(cookies);
    response
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_OPAQUE_TOKEN_BYTES, MAX_PROVISIONING_URI_BYTES, MAX_RECOVERY_KEY_LINE_BYTES,
        MAX_RESPONSE_CORRELATION_BYTES, MAX_STABLE_CODE_BYTES, MAX_TYPED_JSON_BODY_BYTES,
        MAX_TYPED_RESULT_FIELDS, OpaqueToken, ProvisioningUri, RecoveryKeyLine,
        ResponseCorrelation, StableCode, TypedJsonEnvelope, TypedResult, TypedValue,
    };
    use zeroize::Zeroizing;

    fn correlation() -> ResponseCorrelation {
        ResponseCorrelation::new(&"c".repeat(MAX_RESPONSE_CORRELATION_BYTES)).unwrap()
    }

    fn longest_code() -> StableCode {
        StableCode::new(&"a".repeat(MAX_STABLE_CODE_BYTES)).unwrap()
    }

    fn longest_token() -> OpaqueToken {
        OpaqueToken::new(&"a".repeat(MAX_OPAQUE_TOKEN_BYTES)).unwrap()
    }

    fn assert_clearing_string(_: &Zeroizing<String>) {}

    #[test]
    fn stable_codes_accept_only_the_closed_character_set() {
        assert!(StableCode::new("restore_ticket_invalid").is_some());
        assert!(StableCode::new(&"a".repeat(MAX_STABLE_CODE_BYTES)).is_some());
        for rejected in [
            "",
            "Uppercase",
            "with space",
            "with-dash",
            "with\"quote",
            "with\\escape",
            "with{brace}",
            "with\nnewline",
        ] {
            assert!(StableCode::new(rejected).is_none(), "{rejected}");
        }
        assert!(StableCode::new(&"a".repeat(MAX_STABLE_CODE_BYTES + 1)).is_none());
    }

    #[test]
    fn opaque_tokens_accept_only_the_closed_character_set() {
        assert!(OpaqueToken::new("Ab0-_zZ9").is_some());
        assert!(OpaqueToken::new(&"a".repeat(MAX_OPAQUE_TOKEN_BYTES)).is_some());
        for rejected in [
            "",
            "with space",
            "with\"quote",
            "with\\escape",
            "with{brace}",
            "with\nnewline",
            "with+plus",
            "with/slash",
            "with=padding",
        ] {
            assert!(OpaqueToken::new(rejected).is_none(), "{rejected}");
        }
        assert!(OpaqueToken::new(&"a".repeat(MAX_OPAQUE_TOKEN_BYTES + 1)).is_none());
    }

    #[test]
    fn provisioning_uris_accept_only_the_closed_character_set() {
        assert!(
            ProvisioningUri::new(
                "otpauth://totp/Weavelit:first%2Dadmin?secret=GEZDGNBVGY3TQOJQ\
                 &issuer=Weavelit&algorithm=SHA1&digits=6&period=30"
            )
            .is_some()
        );
        assert!(ProvisioningUri::new(&"a".repeat(MAX_PROVISIONING_URI_BYTES)).is_some());
        for rejected in [
            "",
            "with space",
            "with\"quote",
            "with\\escape",
            "with{brace}",
            "with\nnewline",
            "with+plus",
            "with#fragment",
            &"a".repeat(MAX_PROVISIONING_URI_BYTES + 1),
        ] {
            assert!(ProvisioningUri::new(rejected).is_none(), "{rejected}");
        }
    }

    #[test]
    fn recovery_key_lines_accept_only_the_closed_character_set() {
        assert!(
            RecoveryKeyLine::new("AGE-SECRET-KEY-1QQPZRY9X8GF2TVDW0S3JN54KHCE6MUA7L").is_some()
        );
        assert!(RecoveryKeyLine::new(&"A".repeat(MAX_RECOVERY_KEY_LINE_BYTES)).is_some());
        for rejected in [
            "",
            "age1lowercase",
            "with space",
            "with\"quote",
            "with\\escape",
            "with{brace}",
            "with\nnewline",
            "WITH_UNDERSCORE",
            &"A".repeat(MAX_RECOVERY_KEY_LINE_BYTES + 1),
        ] {
            assert!(RecoveryKeyLine::new(rejected).is_none(), "{rejected}");
        }
    }

    #[test]
    fn correlation_identifiers_accept_only_the_closed_character_set() {
        assert!(ResponseCorrelation::new("restore-0123456789").is_some());
        assert_eq!(
            ResponseCorrelation::new("backup-create-0123456789")
                .unwrap()
                .as_str(),
            "backup-create-0123456789"
        );
        assert_eq!(correlation().as_str().len(), MAX_RESPONSE_CORRELATION_BYTES);
        for rejected in [
            "",
            "Uppercase",
            "with space",
            "with_underscore",
            "with\"quote",
            "with\\escape",
            &"c".repeat(MAX_RESPONSE_CORRELATION_BYTES + 1),
        ] {
            assert!(ResponseCorrelation::new(rejected).is_none(), "{rejected}");
        }
    }

    #[test]
    fn a_result_object_is_bounded_to_its_declared_field_count() {
        let mut result = TypedResult::new();
        for _ in 0..MAX_TYPED_RESULT_FIELDS {
            result = result
                .with_field(longest_code(), TypedValue::Boolean(false))
                .unwrap();
        }
        assert!(
            result
                .with_field(longest_code(), TypedValue::Boolean(false))
                .is_none()
        );
    }

    #[test]
    fn typed_secret_values_and_serialized_envelopes_use_clearing_owners() {
        let token = longest_token();
        let uri = ProvisioningUri::new(&"a".repeat(MAX_PROVISIONING_URI_BYTES)).unwrap();
        let recovery_key = RecoveryKeyLine::new(&"A".repeat(MAX_RECOVERY_KEY_LINE_BYTES)).unwrap();
        assert_clearing_string(&token.0);
        assert_clearing_string(&uri.0);
        assert_clearing_string(&recovery_key.0);

        let serialized = TypedJsonEnvelope::Error {
            error: longest_code(),
            correlation_id: correlation(),
        }
        .serialize();
        assert_clearing_string(&serialized);
        assert_eq!(serialized.capacity(), MAX_TYPED_JSON_BODY_BYTES);
    }

    #[test]
    fn both_envelopes_serialize_to_their_approved_shapes() {
        let correlation = ResponseCorrelation::new("restore-0123456789").unwrap();
        let error = TypedJsonEnvelope::Error {
            error: StableCode::new("restore_pending").unwrap(),
            correlation_id: correlation.clone(),
        };
        assert_eq!(
            error.serialize().as_str(),
            "{\"error\":\"restore_pending\",\"correlation_id\":\"restore-0123456789\"}"
        );

        let result = TypedJsonEnvelope::Result {
            result: TypedResult::new()
                .with_field(
                    StableCode::new("accepted").unwrap(),
                    TypedValue::Boolean(true),
                )
                .unwrap()
                .with_field(StableCode::new("bytes").unwrap(), TypedValue::Unsigned(0))
                .unwrap()
                .with_field(
                    StableCode::new("state").unwrap(),
                    TypedValue::Code(StableCode::new("uninitialized").unwrap()),
                )
                .unwrap()
                .with_field(
                    StableCode::new("restore_ticket").unwrap(),
                    TypedValue::Token(OpaqueToken::new("Ab0-_zZ9").unwrap()),
                )
                .unwrap(),
            correlation_id: correlation.clone(),
        };
        assert_eq!(
            result.serialize().as_str(),
            "{\"result\":{\"accepted\":true,\"bytes\":0,\"state\":\"uninitialized\",\
             \"restore_ticket\":\"Ab0-_zZ9\"},\"correlation_id\":\"restore-0123456789\"}"
        );

        let empty = TypedJsonEnvelope::Result {
            result: TypedResult::new(),
            correlation_id: correlation,
        };
        assert_eq!(
            empty.serialize().as_str(),
            "{\"result\":{},\"correlation_id\":\"restore-0123456789\"}"
        );
    }

    /// The typed bound is derived from these maxima, not from the fixed
    /// profile's 128-byte bound.
    ///
    /// The error envelope costs `{"error":"` (10) plus a 48-byte code, plus
    /// `","correlation_id":"` (20), plus a 64-byte correlation, plus `"}` (2):
    /// 144 bytes. The result envelope costs `{"result":{` (11), plus four
    /// fields of a 48-byte quoted name, a colon, and a 48-byte quoted code or
    /// token (101 each), plus three separating commas, plus
    /// `},"correlation_id":"` (20), plus a 64-byte correlation, plus `"}` (2):
    /// 504 bytes. A token field is bounded exactly as a code field is, so the
    /// derivation is the same whichever string value a field carries.
    #[test]
    fn the_largest_envelope_matches_the_derived_bound() {
        let error = TypedJsonEnvelope::Error {
            error: longest_code(),
            correlation_id: correlation(),
        };
        assert_eq!(error.serialize().len(), 144);

        for value in [
            TypedValue::Code(longest_code()),
            TypedValue::Token(longest_token()),
        ] {
            let mut result = TypedResult::new();
            for _ in 0..MAX_TYPED_RESULT_FIELDS {
                result = result.with_field(longest_code(), value.clone()).unwrap();
            }
            let largest = TypedJsonEnvelope::Result {
                result,
                correlation_id: correlation(),
            };
            assert_eq!(largest.serialize().len(), 504);
        }
    }

    /// The one envelope that discloses a recovery key stays inside the
    /// derivation above rather than extending it.
    ///
    /// The delivery envelope costs `{"result":{` (11), plus a 48-byte quoted
    /// name, a colon, and a 128-byte quoted line (181), plus a separating
    /// comma, plus a 48-byte quoted name, a colon, and a 48-byte quoted token
    /// (101), plus `},"correlation_id":"` (20), plus a 64-byte correlation,
    /// plus `"}` (2): 380 bytes. A recovery-key field is therefore affordable
    /// exactly where this envelope carries one, and the largest envelope the
    /// listener must accept is unchanged at 504 bytes.
    #[test]
    fn the_recovery_key_delivery_envelope_stays_inside_the_derived_bound() {
        let delivery = TypedJsonEnvelope::Result {
            result: TypedResult::new()
                .with_field(
                    longest_code(),
                    TypedValue::RecoveryKey(
                        RecoveryKeyLine::new(&"A".repeat(MAX_RECOVERY_KEY_LINE_BYTES)).unwrap(),
                    ),
                )
                .unwrap()
                .with_field(longest_code(), TypedValue::Token(longest_token()))
                .unwrap(),
            correlation_id: correlation(),
        };
        assert_eq!(delivery.serialize().len(), 380);
        assert!(delivery.serialize().len() <= 504);
    }
}
