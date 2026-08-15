//! One-time TOTP provisioning data and the `otpauth://` URI that carries it.

use std::fmt;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use zeroize::Zeroizing;

use crate::{ALGORITHM, DIGITS, STEP_SECONDS};

/// Maximum UTF-8 bytes accepted in an issuer label.
pub const MAX_LABEL_LENGTH: usize = 256;

/// Everything outside RFC 3986's unreserved set is percent-encoded.
const UNRESERVED: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// The character that separates the issuer from the account in a URI label.
const LABEL_DELIMITER: char = ':';

/// The unreserved character shown in place of a label delimiter.
const DELIMITER_SUBSTITUTE: char = '.';

/// The unreserved character appended to a display label that was shortened.
const TRUNCATION_MARKER: char = '~';

/// The display label shown when no part of the account name fits.
const FALLBACK_LABEL: &str = "account";

/// Provisioning data disclosed exactly once and never rendered.
///
/// The value is zeroized on drop and implements neither [`fmt::Display`] nor
/// `PartialEq`. [`ProvisioningText::expose`] is the only way to read it, so a
/// secret cannot reach a log, an error, or a response body through formatting,
/// through a derived `Debug` on a containing type, or through a failed
/// assertion.
pub struct ProvisioningText(Zeroizing<String>);

impl ProvisioningText {
    pub(crate) fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }

    /// Returns the provisioning text for the single disclosure it exists for.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ProvisioningText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ProvisioningText(REDACTED)")
    }
}

/// Why a provisioning URI could not be built.
///
/// No variant carries the rejected text, so a refusal cannot disclose what was
/// submitted. Neither variant is decided by the account name: an account label
/// is cosmetic and is always representable, so no account name can cost its
/// owner an enrollment it could never complete.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProvisioningError {
    /// The issuer is empty, oversized, unprintable, or contains a colon.
    InvalidIssuer,
    /// The caller's maximum cannot hold this issuer's URI at any label length.
    MaximumTooSmall,
}

impl fmt::Display for ProvisioningError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidIssuer => "provisioning issuer is invalid",
            Self::MaximumTooSmall => "provisioning uri maximum is too small",
        };

        formatter.write_str(message)
    }
}

impl std::error::Error for ProvisioningError {}

/// Builds the provisioning URI for one enrollment, within `maximum_bytes`.
///
/// The URI is formatted directly rather than parsed from or rebuilt by a URL
/// library, so the disclosed shape is fixed by this function alone.
///
/// `maximum_bytes` is the caller's own bound on the URI it can carry. The
/// account portion of the label is fitted to whatever that bound leaves, so a
/// caller that supplies the bound its response profile enforces receives a URI
/// that profile accepts rather than a refusal it cannot act on.
pub(crate) fn provisioning_uri(
    issuer: &str,
    account: &str,
    secret: &ProvisioningText,
    maximum_bytes: usize,
) -> Result<ProvisioningText, ProvisioningError> {
    let issuer = label(issuer).ok_or(ProvisioningError::InvalidIssuer)?;
    let prefix = format!("otpauth://totp/{issuer}{LABEL_DELIMITER}");
    // Zeroized rather than plain, because this intermediate carries the secret.
    let suffix = Zeroizing::new(format!(
        "?secret={secret}&issuer={issuer}\
         &algorithm={ALGORITHM}&digits={DIGITS}&period={STEP_SECONDS}",
        secret = secret.expose(),
    ));
    let budget = maximum_bytes
        .checked_sub(prefix.len() + suffix.len())
        .ok_or(ProvisioningError::MaximumTooSmall)?;
    let account = display_label(account, budget);

    Ok(ProvisioningText::new(format!(
        "{prefix}{account}{}",
        suffix.as_str()
    )))
}

/// Percent-encodes one issuer label, refusing values that cannot appear in one.
///
/// A colon is refused rather than encoded: it separates the issuer from the
/// account name in the URI's label, so a value containing one has no
/// unambiguous representation even when escaped. The issuer is a compiled-in
/// deployment constant rather than user-supplied text, so refusing it is a
/// defect in the caller and cannot lock an account out.
fn label(value: &str) -> Option<String> {
    let acceptable = !value.is_empty()
        && value.len() <= MAX_LABEL_LENGTH
        && !value.contains(LABEL_DELIMITER)
        && !value.chars().any(char::is_control);

    acceptable.then(|| utf8_percent_encode(value, UNRESERVED).to_string())
}

/// Builds the cosmetic account label that fits `budget` encoded bytes.
///
/// The account portion of a provisioning label is decorative: an authenticator
/// displays it, while the secret and the profile parameters are what enroll the
/// factor. It is therefore fitted rather than refused. A label that fits is
/// returned exactly as it encodes, so an ordinary account name produces the
/// same bytes it always has; a label that does not fit is shortened and marked
/// with [`TRUNCATION_MARKER`]; a name that leaves nothing to show falls back to
/// [`FALLBACK_LABEL`]. The result is defined for every input, so opening an
/// enrollment never fails on the account's name.
fn display_label(account: &str, budget: usize) -> String {
    let (encoded, complete) = encode_within(account, budget);
    if complete && !encoded.is_empty() {
        return encoded;
    }

    // One byte is held back so the marker itself stays inside the budget.
    let (shortened, _) = encode_within(account, budget.saturating_sub(1));
    if shortened.is_empty() {
        return FALLBACK_LABEL.chars().take(budget).collect();
    }

    format!("{shortened}{TRUNCATION_MARKER}")
}

/// Percent-encodes `value` scalar by scalar, stopping within `budget` bytes.
///
/// Each Unicode scalar's escape is appended whole or not at all, so the result
/// never ends inside a multi-byte character or a partial `%XX` escape. The
/// returned flag reports whether every scalar was encoded.
fn encode_within(value: &str, budget: usize) -> (String, bool) {
    let mut encoded = String::new();
    for character in value.chars() {
        let mut buffer = [0_u8; 4];
        let shown = if character == LABEL_DELIMITER {
            DELIMITER_SUBSTITUTE
        } else {
            character
        };
        let escape: String =
            utf8_percent_encode(shown.encode_utf8(&mut buffer), UNRESERVED).collect();
        if encoded.len() + escape.len() > budget {
            return (encoded, false);
        }
        encoded.push_str(&escape);
    }

    (encoded, true)
}

#[cfg(test)]
mod tests {
    use percent_encoding::percent_decode_str;

    use super::*;

    /// The maximum a caller's response profile enforces on a disclosed URI.
    ///
    /// This restates `weavelit_module_client::typed_json`'s own bound. That
    /// crate is not a dependency here, so the value is stated rather than
    /// imported, and every case below is decided against it.
    const MAXIMUM: usize = 288;

    /// The encoded account bytes [`MAXIMUM`] leaves for the fixture issuer.
    const BUDGET: usize = 174;

    fn secret() -> ProvisioningText {
        ProvisioningText::new("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ".to_owned())
    }

    fn built(account: &str) -> String {
        provisioning_uri("Weavelit", account, &secret(), MAXIMUM)
            .unwrap()
            .expose()
            .to_owned()
    }

    /// Returns the account portion of one built URI's label.
    fn account_label(uri: &str) -> &str {
        uri.strip_prefix("otpauth://totp/Weavelit:")
            .expect("the fixture issuer prefixes every built URI")
            .split('?')
            .next()
            .expect("splitting always yields a first part")
    }

    /// Asserts a label ends on whole escapes and whole Unicode scalars.
    fn assert_whole_escapes_and_scalars(label: &str) {
        let bytes = label.as_bytes();
        for (index, byte) in bytes.iter().enumerate() {
            if *byte == b'%' {
                assert!(
                    index + 2 < bytes.len()
                        && bytes[index + 1].is_ascii_hexdigit()
                        && bytes[index + 2].is_ascii_hexdigit(),
                    "a percent escape must be whole: {label}"
                );
            }
        }
        String::from_utf8(percent_decode_str(label).collect::<Vec<u8>>())
            .expect("a label must decode to whole Unicode scalars");
    }

    #[test]
    fn a_uri_pins_the_exact_provisioning_shape() {
        let uri = built("first-admin");

        assert_eq!(
            uri,
            "otpauth://totp/Weavelit:first-admin\
             ?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&issuer=Weavelit\
             &algorithm=SHA1&digits=6&period=30"
        );
    }

    #[test]
    fn a_label_character_outside_the_unreserved_set_is_percent_encoded() {
        let uri =
            provisioning_uri("Weavelit Ops", "ops+admin@example.com", &secret(), MAXIMUM).unwrap();

        assert_eq!(
            uri.expose(),
            "otpauth://totp/Weavelit%20Ops:ops%2Badmin%40example.com\
             ?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&issuer=Weavelit%20Ops\
             &algorithm=SHA1&digits=6&period=30"
        );
    }

    #[test]
    fn an_issuer_that_cannot_be_represented_is_refused_without_echoing_it() {
        let colon = provisioning_uri("Weave:lit", "first-admin", &secret(), MAXIMUM).unwrap_err();
        let empty = provisioning_uri("", "first-admin", &secret(), MAXIMUM).unwrap_err();
        let control =
            provisioning_uri("Weave\nlit", "first-admin", &secret(), MAXIMUM).unwrap_err();
        let long = provisioning_uri(
            &"a".repeat(MAX_LABEL_LENGTH + 1),
            "first-admin",
            &secret(),
            MAXIMUM,
        )
        .unwrap_err();

        for refusal in [colon, empty, control, long] {
            assert_eq!(refusal, ProvisioningError::InvalidIssuer);
        }
        assert_eq!(format!("{colon}"), "provisioning issuer is invalid");
    }

    #[test]
    fn a_maximum_too_small_for_the_issuer_is_refused_without_echoing_it() {
        let refusal = provisioning_uri("Weavelit", "first-admin", &secret(), 10).unwrap_err();

        assert_eq!(refusal, ProvisioningError::MaximumTooSmall);
        assert_eq!(
            format!("{refusal}"),
            "provisioning uri maximum is too small"
        );
    }

    #[test]
    fn an_account_name_that_fits_is_encoded_byte_for_byte() {
        for account in [
            "first-admin",
            "alice",
            "ops+admin@example.com",
            &"a".repeat(BUDGET),
        ] {
            let expected: String = utf8_percent_encode(account, UNRESERVED).collect::<String>();
            let uri = built(account);

            assert_eq!(account_label(&uri), expected, "{account}");
            assert!(
                !account_label(&uri).ends_with(TRUNCATION_MARKER),
                "{account}"
            );
            assert!(uri.len() <= MAXIMUM, "{account}");
        }
    }

    #[test]
    fn a_maximal_ascii_account_name_is_shortened_to_a_marked_label() {
        let account = "a".repeat(MAX_LABEL_LENGTH);

        let uri = built(&account);
        let label = account_label(&uri);

        assert_eq!(uri.len(), MAXIMUM);
        assert_eq!(label.len(), BUDGET);
        assert_eq!(
            label,
            format!("{}{TRUNCATION_MARKER}", "a".repeat(BUDGET - 1))
        );
        assert!(account.starts_with(label.trim_end_matches(TRUNCATION_MARKER)));
        assert_whole_escapes_and_scalars(label);
    }

    #[test]
    fn a_multibyte_account_name_is_truncated_only_on_scalar_boundaries() {
        // Eighty-five three-byte scalars: 255 UTF-8 bytes, nine encoded bytes
        // each, so no whole number of them lands on the budget exactly.
        let account = "\u{2602}".repeat(85);
        assert_eq!(account.len(), MAX_LABEL_LENGTH - 1);

        let uri = built(&account);
        let label = account_label(&uri);

        assert!(uri.len() <= MAXIMUM);
        assert!(label.ends_with(TRUNCATION_MARKER));
        assert_eq!(
            label,
            format!("{}{TRUNCATION_MARKER}", "%E2%98%82".repeat(19))
        );
        assert_whole_escapes_and_scalars(label);
    }

    #[test]
    fn an_account_name_carrying_the_label_delimiter_is_substituted_not_refused() {
        let uri = built("first:admin");

        assert_eq!(account_label(&uri), "first.admin");
        assert_eq!(
            uri,
            "otpauth://totp/Weavelit:first.admin\
             ?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&issuer=Weavelit\
             &algorithm=SHA1&digits=6&period=30"
        );

        let long = built(&format!("{}:{}", "b".repeat(200), "c".repeat(55)));
        assert!(!account_label(&long).contains(LABEL_DELIMITER));
        assert!(long.len() <= MAXIMUM);
    }

    #[test]
    fn an_account_name_that_shows_nothing_falls_back_to_a_fixed_label() {
        assert_eq!(account_label(&built("")), FALLBACK_LABEL);
    }

    #[test]
    fn every_account_name_builds_a_uri_within_the_caller_maximum() {
        for account in [
            String::new(),
            "first-admin".to_owned(),
            "ops+admin@example.com".to_owned(),
            "first:admin".to_owned(),
            "first\nadmin".to_owned(),
            "a".repeat(MAX_LABEL_LENGTH),
            "\u{2602}".repeat(85),
            "\u{1F510}".repeat(64),
            format!("{}\u{00E9}", "d".repeat(MAX_LABEL_LENGTH - 2)),
        ] {
            let uri = built(&account);

            assert!(uri.len() <= MAXIMUM, "{}", uri.len());
            assert_whole_escapes_and_scalars(account_label(&uri));
        }
    }

    #[test]
    fn provisioning_text_never_renders_its_value() {
        let text = ProvisioningText::new("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ".to_owned());

        assert_eq!(format!("{text:?}"), "ProvisioningText(REDACTED)");
        assert_eq!(
            format!("{:?}", Some(&text)),
            "Some(ProvisioningText(REDACTED))"
        );
    }
}
