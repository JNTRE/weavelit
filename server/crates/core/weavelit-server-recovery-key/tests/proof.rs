//! Delivery-nonce, proof-of-possession, and redaction tests.

use weavelit_server_recovery_key::{
    DELIVERY_NONCE_BYTES, DeliveryNonce, PreparedRecoveryKey, RECOVERY_PROOF_BYTES,
    RecoveryIdentity, RecoveryKey, RecoveryProof,
};

fn identity() -> RecoveryIdentity {
    RecoveryIdentity::generate().expect("host randomness must be available")
}

fn nonce() -> DeliveryNonce {
    DeliveryNonce::generate().expect("host randomness must be available")
}

#[test]
fn a_delivery_nonce_is_unique_and_fully_populated() {
    let first = nonce();
    let second = nonce();
    assert_ne!(first.as_bytes(), second.as_bytes());
    assert_ne!(first.as_bytes(), &[0_u8; DELIVERY_NONCE_BYTES]);
    assert_eq!(
        first.as_bytes(),
        DeliveryNonce::from_bytes(*first.as_bytes()).as_bytes(),
        "a nonce read back from a checkpoint must be the nonce that was written"
    );
}

#[test]
fn a_correct_proof_verifies_against_the_expected_proof() {
    let identity = identity();
    let nonce = nonce();

    let expected = RecoveryProof::compute(&identity, &nonce).expect("the expected proof computes");
    let submitted = RecoveryProof::compute(&identity, &nonce).expect("the client proof computes");

    assert!(expected.matches(&submitted));
    assert!(
        expected.matches(&RecoveryProof::from_bytes(*submitted.as_bytes())),
        "a proof read back from a checkpoint must still verify"
    );
}

#[test]
fn a_proof_computed_from_a_different_key_does_not_verify() {
    let nonce = nonce();
    let expected =
        RecoveryProof::compute(&identity(), &nonce).expect("the expected proof computes");
    let wrong = RecoveryProof::compute(&identity(), &nonce).expect("the wrong proof computes");

    assert!(!expected.matches(&wrong));
}

#[test]
fn a_proof_computed_for_a_different_nonce_does_not_verify() {
    let identity = identity();
    let expected =
        RecoveryProof::compute(&identity, &nonce()).expect("the expected proof computes");
    let other = RecoveryProof::compute(&identity, &nonce()).expect("the other proof computes");

    assert!(!expected.matches(&other));
}

#[test]
fn an_altered_proof_does_not_verify() {
    let expected =
        RecoveryProof::compute(&identity(), &nonce()).expect("the expected proof computes");

    for index in [0, RECOVERY_PROOF_BYTES / 2, RECOVERY_PROOF_BYTES - 1] {
        let mut altered = *expected.as_bytes();
        altered[index] ^= 0x01;
        assert!(
            !expected.matches(&RecoveryProof::from_bytes(altered)),
            "a proof differing in byte {index} must be rejected"
        );
    }

    assert!(!expected.matches(&RecoveryProof::from_bytes([0_u8; RECOVERY_PROOF_BYTES])));
}

#[test]
fn a_proof_is_the_hmac_of_the_delivery_nonce_keyed_by_the_delivered_private_key() {
    use hmac::{Hmac, KeyInit as _, Mac as _};
    use sha2::Sha256;

    let prepared = PreparedRecoveryKey::prepare().expect("host randomness must be available");
    let recipient = prepared.recipient();
    let delivery_nonce = *prepared.delivery_nonce().as_bytes();
    let expected = *prepared.expected_proof().as_bytes();

    // Recover the delivered private key exactly as a client would: parse the
    // canonical line it received, then key HMAC-SHA-256 with its raw bytes.
    let line = prepared
        .into_delivery_line()
        .expect("the prepared key encodes");
    let delivered = RecoveryKey::parse(line.as_str())
        .expect("the delivered line is canonical")
        .into_identity()
        .expect("the delivered line is an identity");
    assert_eq!(
        delivered.public_key(),
        recipient.public_key(),
        "the delivered line must carry the key whose recipient was recorded"
    );

    let secret = decode_identity(line.as_str());
    let mut mac = Hmac::<Sha256>::new_from_slice(&secret).expect("HMAC accepts a 32-byte key");
    mac.update(&delivery_nonce);
    let independent: [u8; RECOVERY_PROOF_BYTES] = mac.finalize().into_bytes().into();

    assert_eq!(independent, expected);
}

#[test]
fn preparation_records_only_the_recipient_the_nonce_and_the_expected_proof() {
    let prepared = PreparedRecoveryKey::prepare().expect("host randomness must be available");

    let recipient_line = prepared
        .recipient()
        .encode()
        .expect("the recorded recipient encodes");
    let delivery_nonce = *prepared.delivery_nonce().as_bytes();
    let expected_proof = *prepared.expected_proof().as_bytes();

    let line = prepared
        .into_delivery_line()
        .expect("the prepared key encodes");
    let secret = decode_identity(line.as_str());

    // The recorded values are real, so the following non-containment assertions
    // are not vacuous.
    assert!(recipient_line.starts_with("age1"));
    assert_ne!(delivery_nonce, [0_u8; DELIVERY_NONCE_BYTES]);
    assert_ne!(expected_proof, [0_u8; RECOVERY_PROOF_BYTES]);
    assert_ne!(secret, [0_u8; 32]);

    // None of the recorded values is the private key, in any form.
    assert_ne!(expected_proof, secret, "the proof must not be the key");
    assert_ne!(delivery_nonce, secret, "the nonce must not be the key");
    assert!(
        !recipient_line.contains(line.as_str()),
        "the recorded recipient must not carry the delivery line"
    );
}

#[test]
fn no_recovery_key_value_renders_itself_in_debug() {
    let prepared = PreparedRecoveryKey::prepare().expect("host randomness must be available");

    let nonce_bytes = *prepared.delivery_nonce().as_bytes();
    let proof_bytes = *prepared.expected_proof().as_bytes();
    let nonce_debug = format!("{:?}", prepared.delivery_nonce());
    let proof_debug = format!("{:?}", prepared.expected_proof());
    let prepared_debug = format!("{prepared:?}");

    let line = prepared
        .into_delivery_line()
        .expect("the prepared key encodes");
    let identity = RecoveryKey::parse(line.as_str()).expect("the delivered line is canonical");
    let identity_debug = format!("{identity:?}");
    let inner_debug = format!(
        "{:?}",
        identity
            .into_identity()
            .expect("the delivered line is an identity")
    );

    // Each rendering is the fixed redacted literal, so nothing about the value
    // reaches a debug rendering.
    assert_eq!(nonce_debug, "DeliveryNonce(REDACTED)");
    assert_eq!(proof_debug, "RecoveryProof(REDACTED)");
    assert_eq!(prepared_debug, "PreparedRecoveryKey(REDACTED)");
    assert_eq!(identity_debug, "RecoveryKey(REDACTED)");
    assert_eq!(inner_debug, "RecoveryIdentity(REDACTED)");

    // The values these renderings withhold are real, so the assertions above
    // are not vacuous.
    assert!(line.as_str().starts_with("AGE-SECRET-KEY-1"));
    assert_ne!(nonce_bytes, [0_u8; DELIVERY_NONCE_BYTES]);
    assert_ne!(proof_bytes, [0_u8; RECOVERY_PROOF_BYTES]);
    for rendering in [
        nonce_debug.as_str(),
        proof_debug.as_str(),
        prepared_debug.as_str(),
        identity_debug.as_str(),
        inner_debug.as_str(),
    ] {
        assert!(!rendering.contains(line.as_str()));
        assert!(!rendering.contains(&hex(&nonce_bytes)));
        assert!(!rendering.contains(&hex(&proof_bytes)));
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        use std::fmt::Write as _;
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// Decodes the raw 32-byte scalar out of a canonical identity line.
///
/// The crate deliberately exposes no accessor for the private bytes, so a test
/// that needs them reconstructs them from the delivered canonical line.
fn decode_identity(line: &str) -> [u8; 32] {
    let checked = bech32::primitives::decode::CheckedHrpstring::new::<bech32::Bech32>(line)
        .expect("the delivered line is canonical Bech32");
    checked
        .byte_iter()
        .collect::<Vec<u8>>()
        .try_into()
        .expect("an identity payload is exactly 32 bytes")
}
