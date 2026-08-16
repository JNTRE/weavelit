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

use zeroize::Zeroizing;

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
pub struct CookieValue(Zeroizing<String>);

impl CookieValue {
    /// Accepts only `[A-Za-z0-9_-]` within [`MAX_COOKIE_VALUE_BYTES`].
    #[must_use]
    pub fn new(value: &str) -> Option<Self> {
        Self::is_valid(value).then(|| Self(Zeroizing::new(value.to_owned())))
    }

    /// Checks whether a borrowed value has the shape of an issued cookie.
    #[must_use]
    pub fn is_valid(value: &str) -> bool {
        !value.is_empty()
            && value.len() <= MAX_COOKIE_VALUE_BYTES
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
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

        let parts = [
            (SESSION_COOKIE_NAME, session, SESSION_ATTRIBUTES),
            (CSRF_COOKIE_NAME, csrf, CSRF_ATTRIBUTES),
        ];
        let lines = parts.len();
        let bytes = parts
            .iter()
            .fold(0_usize, |total, (name, value, attributes)| {
                total
                    + "Set-Cookie: ".len()
                    + name.len()
                    + 1
                    + value.len()
                    + attributes.len()
                    + expiry.len()
                    + "\r\n".len()
            });
        if lines > MAX_COOKIE_LINES || bytes > MAX_COOKIE_HEADER_BYTES {
            return None;
        }

        let mut text = Zeroizing::new(String::with_capacity(bytes));
        for (name, value, attributes) in parts {
            text.push_str("Set-Cookie: ");
            text.push_str(name);
            text.push('=');
            text.push_str(value);
            text.push_str(attributes);
            text.push_str(expiry);
            text.push_str("\r\n");
        }
        debug_assert_eq!(text.len(), bytes);
        Some(CookieLines(text))
    }
}

/// Accepts rendered header text only within both declared cookie bounds.
///
/// Separated from [`CookieEffect::render`] so the fail-closed bound is
/// exercised directly. No constructible effect can breach it today, which is
/// the point: the bound is what keeps a future attribute or line change from
/// silently growing the response head.
#[cfg(test)]
fn bounded_lines(text: Zeroizing<String>, lines: usize) -> Option<CookieLines> {
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
pub struct CookieLines(Zeroizing<String>);

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
    use zeroize::Zeroizing;

    fn value(text: &str) -> CookieValue {
        CookieValue::new(text).expect("the test value must be accepted")
    }

    #[test]
    fn cookie_values_accept_only_the_closed_character_set() {
        for accepted in ["Ab0-_zZ9", &"a".repeat(MAX_COOKIE_VALUE_BYTES)] {
            assert!(CookieValue::is_valid(accepted), "{accepted}");
            assert!(CookieValue::new(accepted).is_some(), "{accepted}");
        }
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
            assert!(!CookieValue::is_valid(rejected), "{rejected}");
            assert!(CookieValue::new(rejected).is_none(), "{rejected}");
        }
        let over_bound = "a".repeat(MAX_COOKIE_VALUE_BYTES + 1);
        assert!(!CookieValue::is_valid(&over_bound));
        assert!(CookieValue::new(&over_bound).is_none());
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
        let at_bound = Zeroizing::new("a".repeat(MAX_COOKIE_HEADER_BYTES));
        assert!(bounded_lines(at_bound.clone(), MAX_COOKIE_LINES).is_some());

        let over_bytes = Zeroizing::new("a".repeat(MAX_COOKIE_HEADER_BYTES + 1));
        assert!(bounded_lines(over_bytes, MAX_COOKIE_LINES).is_none());
        assert!(bounded_lines(at_bound, MAX_COOKIE_LINES + 1).is_none());
    }
}
