//! Opaque process-memory handle for an MFA policy step-up proof.

use std::fmt;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use weavelit_server_administration::StepUpActionFamily;
use zeroize::Zeroizing;

pub(crate) const MFA_POLICY_TICKET_ENTROPY_BYTES: usize = 32;
pub(crate) const MFA_POLICY_TICKET_TEXT_BYTES: usize = 43;

const _: () = assert!(
    MFA_POLICY_TICKET_TEXT_BYTES == MFA_POLICY_TICKET_ENTROPY_BYTES.div_ceil(3) * 4 - 1,
    "the MFA policy ticket length must match its approved entropy"
);

const MFA_POLICY_TICKET_DIGEST_DOMAIN: &[u8] =
    b"weavelit.administration.mfa-policy-step-up-ticket.v1";
const GRANT_MUTATION_TICKET_DIGEST_DOMAIN: &[u8] =
    b"weavelit.administration.grant-mutation-step-up-ticket.v1";

/// One opaque reusable bearer returned after a current-session TOTP step-up.
pub(crate) struct MfaPolicyStepUpTicket {
    text: Zeroizing<String>,
}

impl MfaPolicyStepUpTicket {
    pub(crate) fn generate() -> Option<Self> {
        let mut entropy = Zeroizing::new([0_u8; MFA_POLICY_TICKET_ENTROPY_BYTES]);
        getrandom::fill(&mut *entropy).ok()?;
        Some(Self::from_zeroizing_entropy(entropy))
    }

    #[cfg(test)]
    fn from_zeroizing_entropy(entropy: Zeroizing<[u8; MFA_POLICY_TICKET_ENTROPY_BYTES]>) -> Self {
        Self {
            text: Zeroizing::new(URL_SAFE_NO_PAD.encode(&entropy[..])),
        }
    }

    #[cfg(not(test))]
    fn from_zeroizing_entropy(entropy: Zeroizing<[u8; MFA_POLICY_TICKET_ENTROPY_BYTES]>) -> Self {
        Self {
            text: Zeroizing::new(URL_SAFE_NO_PAD.encode(&entropy[..])),
        }
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.text
    }

    pub(crate) fn digest(&self, family: StepUpActionFamily) -> MfaPolicyStepUpTicketDigest {
        MfaPolicyStepUpTicketDigest::of_canonical(&self.text, family)
            .expect("a generated step-up ticket is canonical")
    }
}

impl fmt::Debug for MfaPolicyStepUpTicket {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MfaPolicyStepUpTicket(REDACTED)")
    }
}

/// Domain-separated digest retained instead of an MFA policy ticket.
#[derive(Clone, Copy)]
pub(crate) struct MfaPolicyStepUpTicketDigest([u8; 32]);

impl MfaPolicyStepUpTicketDigest {
    pub(crate) fn of_canonical(ticket: &str, family: StepUpActionFamily) -> Option<Self> {
        if ticket.len() != MFA_POLICY_TICKET_TEXT_BYTES {
            return None;
        }
        let decoded = URL_SAFE_NO_PAD.decode(ticket).ok()?;
        if decoded.len() != MFA_POLICY_TICKET_ENTROPY_BYTES
            || URL_SAFE_NO_PAD.encode(&decoded) != ticket
        {
            return None;
        }
        let mut digest = Sha256::new();
        digest.update(match family {
            StepUpActionFamily::MfaPolicy => MFA_POLICY_TICKET_DIGEST_DOMAIN,
            StepUpActionFamily::GrantMutation => GRANT_MUTATION_TICKET_DIGEST_DOMAIN,
        });
        digest.update(ticket.as_bytes());
        Some(Self(digest.finalize().into()))
    }

    pub(crate) fn matches(&self, other: &Self) -> bool {
        let mut difference = 0_u8;
        for (stored, submitted) in self.0.iter().zip(other.0.iter()) {
            difference |= stored ^ submitted;
        }
        difference == 0
    }
}

impl fmt::Debug for MfaPolicyStepUpTicketDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MfaPolicyStepUpTicketDigest(REDACTED)")
    }
}

#[cfg(test)]
mod tests {
    use sha2::{Digest as _, Sha256};

    use super::*;

    fn seeded(seed: u8) -> MfaPolicyStepUpTicket {
        let mut entropy = [0_u8; MFA_POLICY_TICKET_ENTROPY_BYTES];
        entropy[0] = seed;
        MfaPolicyStepUpTicket::from_zeroizing_entropy(Zeroizing::new(entropy))
    }

    #[test]
    fn ticket_is_canonical_and_digest_is_domain_separated() {
        let ticket = seeded(1);
        let other = seeded(2);
        let plain: [u8; 32] = Sha256::digest(ticket.as_str().as_bytes()).into();

        assert_eq!(ticket.as_str().len(), MFA_POLICY_TICKET_TEXT_BYTES);
        assert!(
            MfaPolicyStepUpTicketDigest::of_canonical(
                ticket.as_str(),
                StepUpActionFamily::MfaPolicy
            )
            .is_some()
        );
        assert!(
            ticket
                .digest(StepUpActionFamily::MfaPolicy)
                .matches(&ticket.digest(StepUpActionFamily::MfaPolicy))
        );
        assert!(
            !ticket
                .digest(StepUpActionFamily::MfaPolicy)
                .matches(&other.digest(StepUpActionFamily::MfaPolicy))
        );
        assert!(
            !ticket
                .digest(StepUpActionFamily::MfaPolicy)
                .matches(&MfaPolicyStepUpTicketDigest(plain))
        );
        assert!(
            !ticket
                .digest(StepUpActionFamily::MfaPolicy)
                .matches(&ticket.digest(StepUpActionFamily::GrantMutation))
        );
    }

    #[test]
    fn ticket_rejects_noncanonical_input_and_never_renders() {
        let ticket = seeded(3);
        assert!(
            MfaPolicyStepUpTicketDigest::of_canonical("short", StepUpActionFamily::MfaPolicy)
                .is_none()
        );
        assert!(
            MfaPolicyStepUpTicketDigest::of_canonical(
                &format!("{}=", ticket.as_str()),
                StepUpActionFamily::MfaPolicy,
            )
            .is_none()
        );
        assert_eq!(format!("{ticket:?}"), "MfaPolicyStepUpTicket(REDACTED)");
        assert_eq!(
            format!("{:?}", ticket.digest(StepUpActionFamily::MfaPolicy)),
            "MfaPolicyStepUpTicketDigest(REDACTED)"
        );
    }

    #[test]
    fn generated_tickets_differ() {
        let first = MfaPolicyStepUpTicket::generate().unwrap();
        let second = MfaPolicyStepUpTicket::generate().unwrap();
        assert_ne!(first.as_str(), second.as_str());
    }
}
