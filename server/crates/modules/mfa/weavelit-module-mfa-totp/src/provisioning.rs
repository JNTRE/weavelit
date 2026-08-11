//! One-time TOTP provisioning data and the `otpauth://` URI that carries it.

use std::fmt;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use zeroize::Zeroizing;

use crate::{ALGORITHM, DIGITS, STEP_SECONDS};

/// Maximum UTF-8 bytes accepted in an issuer or account label.
pub const MAX_LABEL_LENGTH: usize = 256;

/// Everything outside RFC 3986's unreserved set is percent-encoded.
const UNRESERVED: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

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
/// submitted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProvisioningError {
    /// The issuer is empty, oversized, unprintable, or contains a colon.
    InvalidIssuer,
    /// The account name is empty, oversized, unprintable, or contains a colon.
    InvalidAccount,
}

impl fmt::Display for ProvisioningError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidIssuer => "provisioning issuer is invalid",
            Self::InvalidAccount => "provisioning account name is invalid",
        };

        formatter.write_str(message)
    }
}

impl std::error::Error for ProvisioningError {}

/// Builds the provisioning URI for one enrollment.
///
/// The URI is formatted directly rather than parsed from or rebuilt by a URL
/// library, so the disclosed shape is fixed by this function alone.
pub(crate) fn provisioning_uri(
    issuer: &str,
    account: &str,
    secret: &ProvisioningText,
) -> Result<ProvisioningText, ProvisioningError> {
    let issuer = label(issuer).ok_or(ProvisioningError::InvalidIssuer)?;
    let account = label(account).ok_or(ProvisioningError::InvalidAccount)?;

    Ok(ProvisioningText::new(format!(
        "otpauth://totp/{issuer}:{account}?secret={secret}&issuer={issuer}\
         &algorithm={ALGORITHM}&digits={DIGITS}&period={STEP_SECONDS}",
        secret = secret.expose(),
    )))
}

/// Percent-encodes one label, refusing values that cannot appear in one.
///
/// A colon is refused rather than encoded: it separates the issuer from the
/// account name in the URI's label, so a value containing one has no
/// unambiguous representation even when escaped.
fn label(value: &str) -> Option<String> {
    let acceptable = !value.is_empty()
        && value.len() <= MAX_LABEL_LENGTH
        && !value.contains(':')
        && !value.chars().any(char::is_control);

    acceptable.then(|| utf8_percent_encode(value, UNRESERVED).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret() -> ProvisioningText {
        ProvisioningText::new("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ".to_owned())
    }

    #[test]
    fn a_uri_pins_the_exact_provisioning_shape() {
        let uri = provisioning_uri("Weavelit", "first-admin", &secret()).unwrap();

        assert_eq!(
            uri.expose(),
            "otpauth://totp/Weavelit:first-admin\
             ?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&issuer=Weavelit\
             &algorithm=SHA1&digits=6&period=30"
        );
    }

    #[test]
    fn a_label_character_outside_the_unreserved_set_is_percent_encoded() {
        let uri = provisioning_uri("Weavelit Ops", "ops+admin@example.com", &secret()).unwrap();

        assert_eq!(
            uri.expose(),
            "otpauth://totp/Weavelit%20Ops:ops%2Badmin%40example.com\
             ?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&issuer=Weavelit%20Ops\
             &algorithm=SHA1&digits=6&period=30"
        );
    }

    #[test]
    fn a_label_that_cannot_be_represented_is_refused_without_echoing_it() {
        let colon_issuer = provisioning_uri("Weave:lit", "first-admin", &secret()).unwrap_err();
        let colon_account = provisioning_uri("Weavelit", "first:admin", &secret()).unwrap_err();
        let empty_issuer = provisioning_uri("", "first-admin", &secret()).unwrap_err();
        let empty_account = provisioning_uri("Weavelit", "", &secret()).unwrap_err();
        let control_account = provisioning_uri("Weavelit", "first\nadmin", &secret()).unwrap_err();
        let long_account =
            provisioning_uri("Weavelit", &"a".repeat(MAX_LABEL_LENGTH + 1), &secret()).unwrap_err();

        assert_eq!(colon_issuer, ProvisioningError::InvalidIssuer);
        assert_eq!(empty_issuer, ProvisioningError::InvalidIssuer);
        assert_eq!(colon_account, ProvisioningError::InvalidAccount);
        assert_eq!(empty_account, ProvisioningError::InvalidAccount);
        assert_eq!(control_account, ProvisioningError::InvalidAccount);
        assert_eq!(long_account, ProvisioningError::InvalidAccount);
        assert_eq!(
            format!("{colon_issuer} {colon_account}"),
            "provisioning issuer is invalid provisioning account name is invalid"
        );
    }

    #[test]
    fn a_label_at_the_accepted_length_is_still_provisioned() {
        let account = "a".repeat(MAX_LABEL_LENGTH);

        let uri = provisioning_uri("Weavelit", &account, &secret()).unwrap();

        assert!(uri.expose().contains(&account));
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
