//! One-time Restore submission ticket and the digest a Server stores for it.
//!
//! A Restore is submitted in two steps: the recovery key is submitted first and
//! the encrypted artifact is uploaded afterwards. The ticket is the only value
//! that binds the two requests together, so it is an independent
//! cryptographically random bearer value and never a correlation identifier, a
//! session identifier, or anything derived from either.
//!
//! A Server retains only [`RestoreTicketDigest`], never the ticket itself, and
//! compares a submitted ticket against that digest in constant time.

use std::fmt;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// Entropy one Restore ticket carries, in bytes.
///
/// Thirty-two bytes is 256 bits, which is the approved minimum for a bearer
/// value that authorizes an artifact upload against a retained recovery key.
pub const RESTORE_TICKET_ENTROPY_BYTES: usize = 32;

/// Encoded length of a Restore ticket in ASCII bytes.
///
/// Unpadded URL-safe Base64 of [`RESTORE_TICKET_ENTROPY_BYTES`] is exactly this
/// many characters, all drawn from `[A-Za-z0-9_-]`.
pub const RESTORE_TICKET_TEXT_BYTES: usize = 43;

const _: () = assert!(
    RESTORE_TICKET_TEXT_BYTES == RESTORE_TICKET_ENTROPY_BYTES.div_ceil(3) * 4 - 1,
    "the encoded ticket length must match unpadded Base64 of the approved entropy"
);

/// Domain separator so a ticket digest can never collide with another digest
/// this workspace computes over unrelated bytes.
const TICKET_DIGEST_DOMAIN: &[u8] = b"weavelit.restore.ticket.v1";

/// A minted one-time Restore ticket.
///
/// The value exists only long enough to be returned to the submitting client.
/// It is cleared when dropped and never rendered by `Debug`.
pub struct RestoreTicket {
    text: Zeroizing<String>,
}

impl RestoreTicket {
    /// Mints a ticket from protected operating-system entropy supplied by the caller.
    ///
    /// The caller supplies the entropy because randomness is a Server runtime
    /// capability; this type owns only the encoding and the digest.
    #[must_use]
    pub fn from_zeroizing_entropy(entropy: Zeroizing<[u8; RESTORE_TICKET_ENTROPY_BYTES]>) -> Self {
        Self {
            text: Zeroizing::new(URL_SAFE_NO_PAD.encode(&entropy[..])),
        }
    }

    /// Returns the encoded ticket for the one response that carries it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Returns the digest a Server retains instead of the ticket.
    #[must_use]
    pub fn digest(&self) -> RestoreTicketDigest {
        RestoreTicketDigest::of(&self.text)
    }
}

impl fmt::Debug for RestoreTicket {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RestoreTicket(redacted)")
    }
}

/// The stored digest of one Restore ticket.
///
/// This type deliberately implements neither `PartialEq` nor `Display`, so the
/// only available comparison is the constant-time [`RestoreTicketDigest::matches`]
/// and no code path can render it.
#[derive(Clone, Copy)]
pub struct RestoreTicketDigest([u8; 32]);

impl RestoreTicketDigest {
    /// Computes the domain-separated SHA-256 digest of a submitted ticket.
    #[must_use]
    pub fn of(ticket: &str) -> Self {
        let mut digest = Sha256::new();
        digest.update(TICKET_DIGEST_DOMAIN);
        digest.update(ticket.as_bytes());
        Self(digest.finalize().into())
    }

    /// Compares two digests without a data-dependent branch or early return.
    #[must_use]
    pub fn matches(&self, other: &Self) -> bool {
        let mut difference = 0_u8;
        for (stored, submitted) in self.0.iter().zip(other.0.iter()) {
            difference |= stored ^ submitted;
        }
        difference == 0
    }
}

impl fmt::Debug for RestoreTicketDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RestoreTicketDigest(redacted)")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        RESTORE_TICKET_ENTROPY_BYTES, RESTORE_TICKET_TEXT_BYTES, RestoreTicket, RestoreTicketDigest,
    };
    use zeroize::Zeroizing;

    fn seeded_ticket(seed: u8) -> RestoreTicket {
        let mut entropy = [0_u8; RESTORE_TICKET_ENTROPY_BYTES];
        for (index, byte) in entropy.iter_mut().enumerate() {
            *byte = seed
                .wrapping_mul(31)
                .wrapping_add(u8::try_from(index).unwrap_or(u8::MAX));
        }
        RestoreTicket::from_zeroizing_entropy(Zeroizing::new(entropy))
    }

    #[test]
    fn a_ticket_encodes_its_full_entropy_in_a_closed_character_set() {
        let ticket = seeded_ticket(7);
        assert_eq!(ticket.as_str().len(), RESTORE_TICKET_TEXT_BYTES);
        assert!(
            ticket
                .as_str()
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'),
            "a ticket must never require JSON escaping"
        );
    }

    #[test]
    fn distinct_entropy_produces_distinct_tickets() {
        let tickets: BTreeSet<String> = (0..32_u8)
            .map(|seed| seeded_ticket(seed).as_str().to_owned())
            .collect();
        assert_eq!(tickets.len(), 32);
    }

    #[test]
    fn a_digest_matches_only_its_own_ticket() {
        let ticket = seeded_ticket(3);
        let digest = ticket.digest();
        assert!(digest.matches(&RestoreTicketDigest::of(ticket.as_str())));
        assert!(!digest.matches(&RestoreTicketDigest::of("")));
        assert!(!digest.matches(&seeded_ticket(4).digest()));

        // One flipped character must not match.
        let mut altered = ticket.as_str().to_owned();
        altered.replace_range(0..1, if altered.starts_with('A') { "B" } else { "A" });
        assert!(!digest.matches(&RestoreTicketDigest::of(&altered)));
    }

    #[test]
    fn neither_the_ticket_nor_its_digest_renders_its_value() {
        let ticket = seeded_ticket(9);
        let rendered = format!("{ticket:?} {:?}", ticket.digest());
        assert!(!rendered.contains(ticket.as_str()));
        assert_eq!(
            rendered,
            "RestoreTicket(redacted) RestoreTicketDigest(redacted)"
        );
    }
}
