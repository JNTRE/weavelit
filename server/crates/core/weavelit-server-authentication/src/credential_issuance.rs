//! Clearing input for exact-session account credential issuance.

use std::fmt;

use zeroize::Zeroizing;

/// Fresh credentials supplied for one account-create or password-reset action.
///
/// The value is not clonable and owns both fields in zeroizing storage. Its
/// diagnostics expose neither field, and callers receive only borrowed bytes
/// while the owning workflow is evaluating one issuance attempt.
pub struct AccountCredentialIssuanceInput {
    current_password: Zeroizing<Vec<u8>>,
    totp_code: Option<Zeroizing<Vec<u8>>>,
}

impl AccountCredentialIssuanceInput {
    /// Adopts clearing current-password bytes and an optional clearing TOTP code.
    #[must_use]
    pub fn new(
        current_password: Zeroizing<Vec<u8>>,
        totp_code: Option<Zeroizing<Vec<u8>>>,
    ) -> Self {
        Self {
            current_password,
            totp_code,
        }
    }

    /// Returns the current password while this input remains owned.
    #[must_use]
    pub fn current_password(&self) -> &[u8] {
        &self.current_password
    }

    /// Returns the optional TOTP code while this input remains owned.
    #[must_use]
    pub fn totp_code(&self) -> Option<&[u8]> {
        self.totp_code.as_ref().map(|code| code.as_slice())
    }
}

impl fmt::Debug for AccountCredentialIssuanceInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AccountCredentialIssuanceInput(REDACTED)")
    }
}

#[cfg(test)]
mod tests {
    use zeroize::Zeroizing;

    use super::AccountCredentialIssuanceInput;

    #[test]
    fn issuance_input_owns_only_clearing_bytes_and_renders_no_credentials() {
        let password = b"sensitive current password".to_vec();
        let code = b"123456".to_vec();
        let input = AccountCredentialIssuanceInput::new(
            Zeroizing::new(password.clone()),
            Some(Zeroizing::new(code.clone())),
        );

        assert_eq!(input.current_password(), password);
        assert_eq!(input.totp_code(), Some(code.as_slice()));
        let rendered = format!("{input:?}");
        assert_eq!(rendered, "AccountCredentialIssuanceInput(REDACTED)");
        assert!(!rendered.contains("sensitive"));
        assert!(!rendered.contains("123456"));
    }

    #[test]
    fn issuance_input_distinguishes_absent_from_present_totp() {
        let input =
            AccountCredentialIssuanceInput::new(Zeroizing::new(b"current password".to_vec()), None);

        assert_eq!(input.current_password(), b"current password");
        assert_eq!(input.totp_code(), None);
    }
}
