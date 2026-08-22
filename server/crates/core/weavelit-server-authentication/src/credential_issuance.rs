//! Clearing input and opaque proof material for account credential issuance.

use std::fmt;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::{error::AuthenticationError, random::random_zeroizing_bytes};

/// Entropy carried by one credential-issuance ticket.
pub const CREDENTIAL_ISSUANCE_TICKET_ENTROPY_BYTES: usize = 32;

/// Canonical unpadded Base64url length of one credential-issuance ticket.
pub const CREDENTIAL_ISSUANCE_TICKET_TEXT_BYTES: usize = 43;

const _: () = assert!(
    CREDENTIAL_ISSUANCE_TICKET_TEXT_BYTES
        == CREDENTIAL_ISSUANCE_TICKET_ENTROPY_BYTES.div_ceil(3) * 4 - 1,
    "the credential-issuance ticket length must match its approved entropy"
);

const CREDENTIAL_ISSUANCE_TICKET_DIGEST_DOMAIN: &[u8] =
    b"weavelit.authentication.credential-issuance-ticket.v1";

/// One opaque single-use credential-issuance bearer returned to a client.
pub struct CredentialIssuanceTicket {
    text: Zeroizing<String>,
}

impl CredentialIssuanceTicket {
    /// Mints a ticket from operating-system randomness.
    pub fn generate() -> Result<Self, AuthenticationError> {
        Ok(Self::from_zeroizing_entropy(random_zeroizing_bytes::<
            CREDENTIAL_ISSUANCE_TICKET_ENTROPY_BYTES,
        >()?))
    }

    /// Encodes caller-supplied protected entropy for deterministic tests.
    #[must_use]
    pub fn from_zeroizing_entropy(
        entropy: Zeroizing<[u8; CREDENTIAL_ISSUANCE_TICKET_ENTROPY_BYTES]>,
    ) -> Self {
        Self {
            text: Zeroizing::new(URL_SAFE_NO_PAD.encode(&entropy[..])),
        }
    }

    /// Borrows the ticket for its one permitted response.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Returns the digest retained by the process-memory ticket store.
    #[must_use]
    pub fn digest(&self) -> CredentialIssuanceTicketDigest {
        CredentialIssuanceTicketDigest::of_canonical(&self.text)
            .expect("a generated ticket is canonical")
    }
}

impl fmt::Debug for CredentialIssuanceTicket {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CredentialIssuanceTicket(redacted)")
    }
}

impl fmt::Display for CredentialIssuanceTicket {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CredentialIssuanceTicket(redacted)")
    }
}

/// Domain-separated digest retained instead of a credential-issuance ticket.
#[derive(Clone, Copy)]
pub struct CredentialIssuanceTicketDigest([u8; 32]);

impl CredentialIssuanceTicketDigest {
    /// Digests only the exact canonical 256-bit ticket representation.
    #[must_use]
    pub fn of_canonical(ticket: &str) -> Option<Self> {
        if ticket.len() != CREDENTIAL_ISSUANCE_TICKET_TEXT_BYTES {
            return None;
        }
        let decoded = URL_SAFE_NO_PAD.decode(ticket).ok()?;
        if decoded.len() != CREDENTIAL_ISSUANCE_TICKET_ENTROPY_BYTES
            || URL_SAFE_NO_PAD.encode(&decoded) != ticket
        {
            return None;
        }

        let mut digest = Sha256::new();
        digest.update(CREDENTIAL_ISSUANCE_TICKET_DIGEST_DOMAIN);
        digest.update(ticket.as_bytes());
        Some(Self(digest.finalize().into()))
    }

    /// Compares ticket digests without an early return.
    #[must_use]
    pub fn matches(&self, other: &Self) -> bool {
        let mut difference = 0_u8;
        for (stored, submitted) in self.0.iter().zip(other.0.iter()) {
            difference |= stored ^ submitted;
        }
        difference == 0
    }
}

impl fmt::Debug for CredentialIssuanceTicketDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CredentialIssuanceTicketDigest(redacted)")
    }
}

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
    use sha2::{Digest as _, Sha256};
    use zeroize::Zeroizing;

    use super::{
        AccountCredentialIssuanceInput, CREDENTIAL_ISSUANCE_TICKET_ENTROPY_BYTES,
        CREDENTIAL_ISSUANCE_TICKET_TEXT_BYTES, CredentialIssuanceTicket,
        CredentialIssuanceTicketDigest,
    };

    fn seeded_ticket(seed: u8) -> CredentialIssuanceTicket {
        let mut entropy = [0_u8; CREDENTIAL_ISSUANCE_TICKET_ENTROPY_BYTES];
        entropy[0] = seed;
        CredentialIssuanceTicket::from_zeroizing_entropy(Zeroizing::new(entropy))
    }

    #[test]
    fn ticket_is_exact_canonical_unpadded_base64url() {
        let ticket = seeded_ticket(1);

        assert_eq!(ticket.as_str().len(), CREDENTIAL_ISSUANCE_TICKET_TEXT_BYTES);
        assert!(
            ticket
                .as_str()
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        );
        assert!(CredentialIssuanceTicketDigest::of_canonical(ticket.as_str()).is_some());
    }

    #[test]
    fn ticket_digest_rejects_noncanonical_or_wrong_length_input() {
        let ticket = seeded_ticket(2);

        assert!(CredentialIssuanceTicketDigest::of_canonical("short").is_none());
        assert!(
            CredentialIssuanceTicketDigest::of_canonical(&format!("{}=", ticket.as_str()))
                .is_none()
        );
        assert!(
            CredentialIssuanceTicketDigest::of_canonical(&ticket.as_str().to_ascii_lowercase())
                .is_none()
        );
    }

    #[test]
    fn ticket_digest_matches_only_the_exact_ticket_and_is_domain_separated() {
        let ticket = seeded_ticket(3);
        let other = seeded_ticket(4);
        let plain: [u8; 32] = Sha256::digest(ticket.as_str().as_bytes()).into();

        assert!(ticket.digest().matches(&ticket.digest()));
        assert!(!ticket.digest().matches(&other.digest()));
        assert!(
            !ticket
                .digest()
                .matches(&CredentialIssuanceTicketDigest(plain))
        );
    }

    #[test]
    fn generated_tickets_differ_and_ticket_material_never_renders() {
        let first = CredentialIssuanceTicket::generate().unwrap();
        let second = CredentialIssuanceTicket::generate().unwrap();

        assert_ne!(first.as_str(), second.as_str());
        assert_eq!(format!("{first:?}"), "CredentialIssuanceTicket(redacted)");
        assert_eq!(format!("{first}"), "CredentialIssuanceTicket(redacted)");
        assert_eq!(
            format!("{:?}", first.digest()),
            "CredentialIssuanceTicketDigest(redacted)"
        );
    }

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
