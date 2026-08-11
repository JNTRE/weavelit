//! The closed cookie effect the typed response profile may carry.
//!
//! The typed profile discards every header a route sets, so a route cannot
//! reach the wire with header text of its own. Session establishment
//! nonetheless has to set two cookies. This module is the only way that
//! happens, and it is deliberately not a header passthrough: a route selects
//! one of two named effects, supplies only validated opaque values, and the
//! listener renders the fixed attribute text itself.
//!
//! Every rendered line is therefore compile-time text plus a value drawn from
//! a closed character set. A cookie name, an attribute, a `Domain`, an expiry,
//! or a second `Path` cannot be introduced by a route, by a request, or by
//! anything a caller can express.

use std::fmt::Write;

/// The session cookie name, `__Host-` prefixed so a sibling host cannot set it.
pub const SESSION_COOKIE_NAME: &str = "__Host-weavelit_session";

/// The cross-site request forgery cookie name, `__Host-` prefixed likewise.
pub const CSRF_COOKIE_NAME: &str = "__Host-weavelit_csrf";

/// Longest opaque cookie value the effect accepts.
///
/// A session and a CSRF token are 32 random bytes as unpadded Base64url, which
/// is 43 characters. The bound is set at the typed profile's stable-code bound
/// so the two bounds cannot drift apart.
pub const MAX_COOKIE_VALUE_BYTES: usize = crate::typed_json::MAX_STABLE_CODE_BYTES;

/// Most `Set-Cookie` lines one effect may emit.
pub const MAX_COOKIE_LINES: usize = 2;

/// Largest aggregate `Set-Cookie` header text one effect may emit, in bytes.
///
/// This counts every rendered line including its header name and its trailing
/// CRLF. An effect that would exceed it renders nothing, so the response head
/// stays bounded even if an attribute set is ever widened.
pub const MAX_COOKIE_HEADER_BYTES: usize = 512;

/// Fixed attribute text of the session cookie.
///
/// `Secure`, `HttpOnly`, `SameSite=Strict`, and `Path=/` with no `Domain` are
/// the approved session cookie profile. The cookie carries no `Max-Age` and no
/// `Expires`, so it is a browser-session cookie.
const SESSION_ATTRIBUTES: &str = "; Secure; HttpOnly; SameSite=Strict; Path=/";

/// Fixed attribute text of the cross-site request forgery cookie.
///
/// It is deliberately not `HttpOnly`: the client application reads it to echo
/// it in the CSRF request header. It is never a bearer credential on its own,
/// because the session cookie it accompanies is `HttpOnly`.
const CSRF_ATTRIBUTES: &str = "; Secure; SameSite=Strict; Path=/";

/// The sole expiry attribute, used only to delete a cookie at logout.
const DELETION_ATTRIBUTE: &str = "; Max-Age=0";

/// An opaque cookie value drawn from a closed character set.
///
/// The accepted set is unpadded URL-safe Base64, which contains no `;`, no
/// `,`, no whitespace, no quote, and no control byte, so a value can neither
/// terminate its own cookie nor introduce an attribute.
///
/// The wrapped value is a bearer secret, so this type renders nothing: it
/// implements neither `Debug` nor `Display`, and no comparison.
#[derive(Clone)]
pub struct CookieValue(String);

impl CookieValue {
    /// Accepts only `[A-Za-z0-9_-]` within [`MAX_COOKIE_VALUE_BYTES`].
    #[must_use]
    pub fn new(value: &str) -> Option<Self> {
        if value.is_empty() || value.len() > MAX_COOKIE_VALUE_BYTES {
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

/// The closed set of cookie effects a typed response may carry.
///
/// There is no variant that sets an arbitrary cookie, and no variant that sets
/// one cookie alone: a session and its CSRF token are issued together and
/// deleted together, so the two can never disagree about whether a session
/// exists.
#[derive(Clone)]
pub enum CookieEffect {
    /// Establishes a session and its paired cross-site request forgery token.
    IssueSession {
        /// The session bearer value.
        session: CookieValue,
        /// The paired cross-site request forgery token.
        csrf: CookieValue,
    },
    /// Deletes both cookies, which is the only effect that may carry an expiry.
    ClearSession,
}

impl CookieEffect {
    /// Renders the effect, or returns `None` when it does not fit its bounds.
    ///
    /// A `None` result is not a partial emission: the listener replaces the
    /// whole response with its fixed redacted failure and emits no cookie at
    /// all, so a bound that a future attribute change would breach fails
    /// closed instead of truncating a `Set-Cookie` line.
    #[must_use]
    pub fn render(&self) -> Option<CookieLines> {
        let (session, csrf, expiry) = match self {
            Self::IssueSession { session, csrf } => (session.as_str(), csrf.as_str(), ""),
            Self::ClearSession => ("", "", DELETION_ATTRIBUTE),
        };

        let mut text = String::new();
        let mut lines = 0_usize;
        for (name, value, attributes) in [
            (SESSION_COOKIE_NAME, session, SESSION_ATTRIBUTES),
            (CSRF_COOKIE_NAME, csrf, CSRF_ATTRIBUTES),
        ] {
            write!(text, "Set-Cookie: {name}={value}{attributes}{expiry}\r\n").ok()?;
            lines += 1;
        }

        bounded_lines(text, lines)
    }
}

/// Accepts rendered header text only within both declared cookie bounds.
///
/// Separated from [`CookieEffect::render`] so the fail-closed bound is
/// exercised directly. No constructible effect can breach it today, which is
/// the point: the bound is what keeps a future attribute or line change from
/// silently growing the response head.
fn bounded_lines(text: String, lines: usize) -> Option<CookieLines> {
    if lines > MAX_COOKIE_LINES || text.len() > MAX_COOKIE_HEADER_BYTES {
        return None;
    }
    Some(CookieLines(text))
}

/// Rendered, bounded `Set-Cookie` header lines ready for the response head.
///
/// The only constructor is [`CookieEffect::render`], so no caller can build
/// this value from text of its own.
#[derive(Clone)]
pub struct CookieLines(String);

impl CookieLines {
    /// Returns the complete header lines, each already CRLF terminated.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CookieEffect, CookieValue, MAX_COOKIE_HEADER_BYTES, MAX_COOKIE_LINES,
        MAX_COOKIE_VALUE_BYTES, bounded_lines,
    };

    fn value(text: &str) -> CookieValue {
        CookieValue::new(text).expect("the test value must be accepted")
    }

    #[test]
    fn cookie_values_accept_only_the_closed_character_set() {
        assert!(CookieValue::new("Ab0-_zZ9").is_some());
        assert!(CookieValue::new(&"a".repeat(MAX_COOKIE_VALUE_BYTES)).is_some());
        for rejected in [
            "",
            "with space",
            "with;semicolon",
            "with,comma",
            "with=equals",
            "with\"quote",
            "with\\escape",
            "with\rcarriage",
            "with\nnewline",
            "with/slash",
            "with+plus",
        ] {
            assert!(CookieValue::new(rejected).is_none(), "{rejected}");
        }
        assert!(CookieValue::new(&"a".repeat(MAX_COOKIE_VALUE_BYTES + 1)).is_none());
    }

    #[test]
    fn issuing_a_session_renders_the_two_approved_cookie_lines() {
        let rendered = CookieEffect::IssueSession {
            session: value("session-value"),
            csrf: value("csrf-value"),
        }
        .render()
        .expect("the approved effect must render");

        assert_eq!(
            rendered.as_str(),
            "Set-Cookie: __Host-weavelit_session=session-value; Secure; HttpOnly; \
             SameSite=Strict; Path=/\r\n\
             Set-Cookie: __Host-weavelit_csrf=csrf-value; Secure; SameSite=Strict; Path=/\r\n"
        );
    }

    #[test]
    fn clearing_a_session_is_the_only_effect_that_carries_an_expiry() {
        let issued = CookieEffect::IssueSession {
            session: value("session-value"),
            csrf: value("csrf-value"),
        }
        .render()
        .expect("the approved effect must render");
        assert!(!issued.as_str().contains("Max-Age"));
        assert!(!issued.as_str().contains("Expires"));

        let cleared = CookieEffect::ClearSession
            .render()
            .expect("the deletion effect must render");
        assert_eq!(
            cleared.as_str(),
            "Set-Cookie: __Host-weavelit_session=; Secure; HttpOnly; SameSite=Strict; \
             Path=/; Max-Age=0\r\n\
             Set-Cookie: __Host-weavelit_csrf=; Secure; SameSite=Strict; Path=/; Max-Age=0\r\n"
        );
        assert!(!cleared.as_str().contains("Expires"));
    }

    #[test]
    fn every_rendered_effect_stays_within_its_declared_bounds() {
        let longest = value(&"a".repeat(MAX_COOKIE_VALUE_BYTES));
        for effect in [
            CookieEffect::IssueSession {
                session: longest.clone(),
                csrf: longest,
            },
            CookieEffect::ClearSession,
        ] {
            let rendered = effect.render().expect("an approved effect must render");
            let lines: Vec<&str> = rendered
                .as_str()
                .split_terminator("\r\n")
                .filter(|line| !line.is_empty())
                .collect();

            assert_eq!(lines.len(), MAX_COOKIE_LINES);
            assert!(rendered.as_str().len() <= MAX_COOKIE_HEADER_BYTES);
            assert!(rendered.as_str().ends_with("\r\n"));
            for line in lines {
                assert!(line.starts_with("Set-Cookie: "), "{line}");
                assert!(line.contains("; Secure"), "{line}");
                assert!(line.contains("; SameSite=Strict"), "{line}");
                assert!(line.contains("; Path=/"), "{line}");
                assert!(!line.contains("Domain"), "{line}");
            }
        }
    }

    #[test]
    fn only_the_session_cookie_is_withheld_from_the_client_application() {
        let rendered = CookieEffect::IssueSession {
            session: value("session-value"),
            csrf: value("csrf-value"),
        }
        .render()
        .expect("the approved effect must render");
        let mut lines = rendered.as_str().split_terminator("\r\n");

        let session = lines.next().expect("the session line must be present");
        assert!(session.contains("__Host-weavelit_session="));
        assert!(session.contains("; HttpOnly"));

        let csrf = lines.next().expect("the CSRF line must be present");
        assert!(csrf.contains("__Host-weavelit_csrf="));
        assert!(!csrf.contains("HttpOnly"));
    }

    /// The bound fails closed rather than truncating a `Set-Cookie` line.
    ///
    /// No effect this crate can construct breaches either bound, so the
    /// predicate is exercised directly. A future attribute or line change that
    /// did breach one would render nothing, and the listener replaces such a
    /// response with its fixed failure and emits no cookie at all.
    #[test]
    fn rendered_text_outside_either_bound_renders_nothing() {
        let at_bound = "a".repeat(MAX_COOKIE_HEADER_BYTES);
        assert!(bounded_lines(at_bound.clone(), MAX_COOKIE_LINES).is_some());

        let over_bytes = "a".repeat(MAX_COOKIE_HEADER_BYTES + 1);
        assert!(bounded_lines(over_bytes, MAX_COOKIE_LINES).is_none());
        assert!(bounded_lines(at_bound, MAX_COOKIE_LINES + 1).is_none());
    }
}
