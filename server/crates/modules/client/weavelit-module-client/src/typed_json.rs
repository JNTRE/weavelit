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
pub struct OpaqueToken(String);

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
        Some(Self(value.to_owned()))
    }

    fn as_str(&self) -> &str {
        &self.0
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

    fn as_str(&self) -> &str {
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
    pub fn serialize(&self) -> String {
        match self {
            Self::Result {
                result,
                correlation_id,
            } => {
                let mut text = String::from("{\"result\":{");
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
            } => format!(
                "{{\"error\":\"{}\",\"correlation_id\":\"{}\"}}",
                error.as_str(),
                correlation_id.as_str()
            ),
        }
    }
}

/// Builds a route response the listener serializes under the typed profile.
///
/// The response carries no header of its own: the listener supplies the media
/// type from the response profile, so a route cannot emit a cookie, a
/// cross-origin header, or any other header through this path.
#[must_use]
pub fn typed_json_response(status: StatusCode, envelope: TypedJsonEnvelope) -> Response {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = status;
    response.extensions_mut().insert(envelope);
    response
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_OPAQUE_TOKEN_BYTES, MAX_RESPONSE_CORRELATION_BYTES, MAX_STABLE_CODE_BYTES,
        MAX_TYPED_RESULT_FIELDS, OpaqueToken, ResponseCorrelation, StableCode, TypedJsonEnvelope,
        TypedResult, TypedValue,
    };

    fn correlation() -> ResponseCorrelation {
        ResponseCorrelation::new(&"c".repeat(MAX_RESPONSE_CORRELATION_BYTES)).unwrap()
    }

    fn longest_code() -> StableCode {
        StableCode::new(&"a".repeat(MAX_STABLE_CODE_BYTES)).unwrap()
    }

    fn longest_token() -> OpaqueToken {
        OpaqueToken::new(&"a".repeat(MAX_OPAQUE_TOKEN_BYTES)).unwrap()
    }

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
    fn correlation_identifiers_accept_only_the_closed_character_set() {
        assert!(ResponseCorrelation::new("restore-0123456789").is_some());
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
    fn both_envelopes_serialize_to_their_approved_shapes() {
        let correlation = ResponseCorrelation::new("restore-0123456789").unwrap();
        let error = TypedJsonEnvelope::Error {
            error: StableCode::new("restore_pending").unwrap(),
            correlation_id: correlation.clone(),
        };
        assert_eq!(
            error.serialize(),
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
            result.serialize(),
            "{\"result\":{\"accepted\":true,\"bytes\":0,\"state\":\"uninitialized\",\
             \"restore_ticket\":\"Ab0-_zZ9\"},\"correlation_id\":\"restore-0123456789\"}"
        );

        let empty = TypedJsonEnvelope::Result {
            result: TypedResult::new(),
            correlation_id: correlation,
        };
        assert_eq!(
            empty.serialize(),
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
}
