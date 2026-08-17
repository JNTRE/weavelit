//! Approved age v1 parameter policy and header rejection.
//!
//! Weavelit's backup format defines exactly one X25519 recovery recipient, so
//! anything outside that profile is refused as `backup_incompatible` before any
//! key agreement, while a corrupted header of the supported profile stays
//! indistinguishable from every other authentication failure.

mod support;

use base64::{Engine as _, engine::general_purpose::STANDARD_NO_PAD};
use support::{committed, committed_text, envelope, split_stream, validate};

const INCOMPATIBLE: (&str, &str) = ("backup_incompatible", "backup_incompatible");
const INVALID: (&str, &str) = ("backup_invalid", "backup_invalid");

fn identity() -> String {
    committed_text("valid-identity.txt")
}

/// Rebuilds the committed artifact with `rewrite` applied to its age header.
fn rewritten(rewrite: impl FnOnce(String) -> String) -> Vec<u8> {
    let (header, payload) = split_stream(&committed("valid.wlitbackup"));
    let mut stream = rewrite(header).into_bytes();
    stream.extend_from_slice(&payload);
    envelope(&stream)
}

fn category_of(artifact: &[u8]) -> (&'static str, &'static str) {
    support::category(validate(artifact, &identity()).expect_err("the artifact must be rejected"))
}

#[test]
fn an_scrypt_stanza_is_out_of_policy() {
    let artifact = rewritten(|header| header.replace("-> X25519 ", "-> scrypt "));

    assert_eq!(category_of(&artifact), INCOMPATIBLE);
}

#[test]
fn an_unknown_stanza_type_is_out_of_policy() {
    for stanza_type in ["ssh-rsa", "ssh-ed25519", "X25519-extra", "grease"] {
        let artifact =
            rewritten(|header| header.replace("-> X25519 ", &format!("-> {stanza_type} ")));
        assert_eq!(category_of(&artifact), INCOMPATIBLE, "{stanza_type}");
    }
}

#[test]
fn a_second_recipient_stanza_is_out_of_policy() {
    let artifact = rewritten(|header| {
        let (stanza, authenticator) = header
            .split_once("---")
            .expect("the header carries an authenticator line");
        format!(
            "{stanza}{recipient}---{authenticator}",
            recipient = {
                let lines = stanza
                    .strip_prefix("age-encryption.org/v1\n")
                    .expect("the header begins with the version line");
                lines.to_owned()
            }
        )
    });

    assert_eq!(category_of(&artifact), INCOMPATIBLE);
}

#[test]
fn a_header_without_a_recipient_stanza_is_out_of_policy() {
    let artifact = rewritten(|header| {
        let (_, authenticator) = header
            .split_once("---")
            .expect("the header carries an authenticator line");
        format!("age-encryption.org/v1\n---{authenticator}")
    });

    assert_eq!(category_of(&artifact), INCOMPATIBLE);
}

#[test]
fn an_unsupported_age_version_is_out_of_policy() {
    for version in ["age-encryption.org/v2", "age-encryption.org/v1.1"] {
        let artifact = rewritten(|header| header.replace("age-encryption.org/v1", version));
        assert_eq!(category_of(&artifact), INCOMPATIBLE, "{version}");
    }
}

#[test]
fn a_version_line_carrying_a_control_character_is_an_invalid_artifact() {
    let artifact =
        rewritten(|header| header.replace("age-encryption.org/v1\n", "age-encryption.org/v1\r\n"));

    assert_eq!(category_of(&artifact), INVALID);
}

#[test]
fn a_foreign_first_line_is_an_invalid_artifact() {
    let artifact = rewritten(|header| header.replace("age-encryption.org/v1", "not-an-age-file"));

    assert_eq!(category_of(&artifact), INVALID);
}

#[test]
fn an_extra_stanza_argument_is_an_invalid_artifact() {
    let artifact = rewritten(|header| header.replacen("-> X25519 ", "-> X25519 extra ", 1));

    assert_eq!(category_of(&artifact), INVALID);
}

#[test]
fn an_altered_header_is_indistinguishable_from_any_other_failure() {
    // Flipping one ephemeral-share character both breaks key agreement and
    // invalidates the header authenticator.
    let artifact = rewritten(|header| {
        let mut bytes = header.into_bytes();
        let index = b"age-encryption.org/v1\n-> X25519 ".len();
        bytes[index] = if bytes[index] == b'A' { b'B' } else { b'A' };
        String::from_utf8(bytes).expect("the header stays ASCII")
    });

    assert_eq!(category_of(&artifact), INVALID);
}

#[test]
fn a_non_canonical_or_padded_base64_field_is_rejected() {
    let padded = rewritten(|header| {
        let (stanza, rest) = header
            .split_once('\n')
            .expect("the header carries a version line");
        let (recipient, rest) = rest.split_once('\n').expect("the header carries a stanza");
        let (body, rest) = rest
            .split_once('\n')
            .expect("the header carries a stanza body");
        format!("{stanza}\n{recipient}\n{body}=\n{rest}")
    });
    assert_eq!(category_of(&padded), INVALID);

    let short = rewritten(|header| header.replacen("-> X25519 ", "-> X25519 AAAA", 1));
    assert_eq!(category_of(&short), INVALID);
}

#[test]
fn a_low_order_ephemeral_share_is_rejected_before_it_can_agree_on_a_key() {
    let zero_share = STANDARD_NO_PAD.encode([0_u8; 32]);
    let artifact = rewritten(|header| {
        let (version, rest) = header
            .split_once('\n')
            .expect("the header carries a version line");
        let (_, rest) = rest.split_once('\n').expect("the header carries a stanza");
        format!("{version}\n-> X25519 {zero_share}\n{rest}")
    });

    assert_eq!(category_of(&artifact), INVALID);
}

#[test]
fn an_unbounded_header_is_rejected_without_scanning_the_artifact() {
    let mut stream = b"age-encryption.org/v1\n-> X25519 ".to_vec();
    stream.extend(std::iter::repeat_n(b'A', 4 * 1024));
    let artifact = envelope(&stream);

    assert_eq!(category_of(&artifact), INVALID);
}

#[test]
fn a_stream_without_a_payload_nonce_is_rejected() {
    let (header, _) = split_stream(&committed("valid.wlitbackup"));
    let artifact = envelope(header.as_bytes());

    assert_eq!(category_of(&artifact), INVALID);
}
