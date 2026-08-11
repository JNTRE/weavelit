//! Canonical age recovery-key parsing, encoding, and round-trip tests.

use weavelit_server_recovery_key::{
    IDENTITY_PREFIX, KEY_LENGTH, MAX_RECOVERY_KEY_LENGTH, RECIPIENT_PREFIX, RecoveryIdentity,
    RecoveryKey, RecoveryKeyError,
};

/// A fixed identity so encoding assertions are exact rather than incidental.
const FIXED_SECRET: [u8; KEY_LENGTH] = [
    0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20,
    0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f, 0x30,
];

fn encode(hrp: &str, payload: &[u8]) -> String {
    bech32::encode_lower::<bech32::Bech32>(
        bech32::Hrp::parse(hrp).expect("the test prefix is a valid HRP"),
        payload,
    )
    .expect("the test payload is within the Bech32 length limit")
}

fn fixed_identity() -> RecoveryIdentity {
    RecoveryKey::parse(&encode("AGE-SECRET-KEY-", &FIXED_SECRET).to_uppercase())
        .expect("the fixed identity is canonical")
        .into_identity()
        .expect("the fixed value is an identity")
}

/// Returns an identity's canonical delivery line as owned test text.
fn delivery_line(identity: &RecoveryIdentity) -> String {
    identity
        .encode()
        .expect("an identity encodes")
        .as_str()
        .to_owned()
}

#[test]
fn a_generated_identity_round_trips_through_its_canonical_encoding() {
    let identity = RecoveryIdentity::generate().expect("host randomness must be available");
    let line = delivery_line(&identity);

    assert!(line.starts_with(IDENTITY_PREFIX));
    assert!(line.len() <= MAX_RECOVERY_KEY_LENGTH);

    let parsed = RecoveryKey::parse(&line)
        .expect("a generated identity is canonical")
        .into_identity()
        .expect("a generated identity parses back as an identity");
    assert_eq!(parsed.public_key(), identity.public_key());
}

#[test]
fn a_generated_recipient_round_trips_through_its_canonical_encoding() {
    let identity = RecoveryIdentity::generate().expect("host randomness must be available");
    let recipient = identity.recipient();
    let line = recipient.encode().expect("a generated recipient encodes");

    assert!(line.starts_with(RECIPIENT_PREFIX));
    assert!(line.len() <= MAX_RECOVERY_KEY_LENGTH);

    match RecoveryKey::parse(&line).expect("a generated recipient is canonical") {
        RecoveryKey::Recipient(parsed) => assert_eq!(parsed, recipient),
        RecoveryKey::Identity(_) => panic!("a recipient line must not parse as an identity"),
    }
}

#[test]
fn a_generated_recipient_line_is_not_accepted_where_an_identity_is_required() {
    let identity = RecoveryIdentity::generate().expect("host randomness must be available");
    let line = identity
        .recipient()
        .encode()
        .expect("a generated recipient encodes");

    assert_eq!(
        RecoveryKey::parse(&line)
            .expect("the recipient is canonical")
            .into_identity()
            .err(),
        Some(RecoveryKeyError::IdentityRequired)
    );
}

#[test]
fn generation_produces_a_distinct_key_every_time() {
    let first = RecoveryIdentity::generate().expect("host randomness must be available");
    let second = RecoveryIdentity::generate().expect("host randomness must be available");
    assert_ne!(first.public_key(), second.public_key());
    assert_ne!(first.public_key(), [0_u8; KEY_LENGTH]);
}

#[test]
fn surrounding_content_is_rejected() {
    let line = delivery_line(&fixed_identity());

    for candidate in [
        format!(" {line}"),
        format!("{line} "),
        format!("\t{line}"),
        format!("{line}\t"),
        format!("{line}\r"),
        format!("recovery: {line}"),
    ] {
        assert!(
            matches!(
                RecoveryKey::parse(&candidate).err(),
                Some(RecoveryKeyError::SurroundingContent | RecoveryKeyError::NotSingleLine)
            ),
            "surrounding content must be rejected: {candidate:?}"
        );
    }
}

#[test]
fn a_single_trailing_newline_is_accepted_and_any_further_line_is_rejected() {
    let line = delivery_line(&fixed_identity());

    RecoveryKey::parse(&format!("{line}\n")).expect("a single trailing newline is tolerated");

    assert_eq!(
        RecoveryKey::parse(&format!("{line}\n\n")).err(),
        Some(RecoveryKeyError::NotSingleLine)
    );
    assert_eq!(
        RecoveryKey::parse(&format!("{line}\n{line}")).err(),
        Some(RecoveryKeyError::TooLong)
    );
    assert_eq!(
        RecoveryKey::parse(&format!("\n{line}")).err(),
        Some(RecoveryKeyError::NotSingleLine)
    );
}

#[test]
fn the_wrong_case_is_rejected_for_both_key_kinds() {
    let identity = fixed_identity();
    let identity_line = delivery_line(&identity);
    let recipient_line = identity
        .recipient()
        .encode()
        .expect("the fixed recipient encodes");

    assert_eq!(
        RecoveryKey::parse(&identity_line.to_lowercase()).err(),
        Some(RecoveryKeyError::NotCanonical),
        "a lowercase identity must be rejected"
    );
    assert_eq!(
        RecoveryKey::parse(&recipient_line.to_uppercase()).err(),
        Some(RecoveryKeyError::NotCanonical),
        "an uppercase recipient must be rejected"
    );

    let mixed = format!(
        "{}{}",
        &identity_line[..IDENTITY_PREFIX.len()],
        identity_line[IDENTITY_PREFIX.len()..].to_lowercase()
    );
    assert_eq!(
        RecoveryKey::parse(&mixed).err(),
        Some(RecoveryKeyError::NotCanonical),
        "a mixed-case identity must be rejected"
    );
}

#[test]
fn a_non_canonical_encoding_is_rejected() {
    let identity = fixed_identity();
    let recipient_line = identity
        .recipient()
        .encode()
        .expect("the fixed recipient encodes");

    // A Bech32m checksum over the same payload and human-readable part.
    let bech32m = bech32::encode_lower::<bech32::Bech32m>(
        bech32::Hrp::parse("age").expect("the recipient prefix is a valid HRP"),
        &identity.public_key(),
    )
    .expect("the recipient payload encodes");
    assert_ne!(bech32m, recipient_line);
    assert_eq!(
        RecoveryKey::parse(&bech32m).err(),
        Some(RecoveryKeyError::NotCanonical)
    );

    // A correct prefix over a payload that is not exactly 32 bytes.
    assert_eq!(
        RecoveryKey::parse(&encode("age", &[7_u8; KEY_LENGTH - 1])).err(),
        Some(RecoveryKeyError::NotCanonical)
    );

    // A broken checksum.
    let mut broken = recipient_line.as_bytes().to_vec();
    let last = broken.len() - 1;
    broken[last] = if broken[last] == b'q' { b'p' } else { b'q' };
    let broken = String::from_utf8(broken).expect("the mutated recipient stays ASCII");
    assert_eq!(
        RecoveryKey::parse(&broken).err(),
        Some(RecoveryKeyError::NotCanonical)
    );

    // An unknown human-readable part.
    assert_eq!(
        RecoveryKey::parse(&encode("wlit", &identity.public_key())).err(),
        Some(RecoveryKeyError::NotCanonical)
    );
}

#[test]
fn an_empty_or_oversize_submission_is_rejected_before_decoding() {
    assert_eq!(RecoveryKey::parse("").err(), Some(RecoveryKeyError::Empty));
    assert_eq!(
        RecoveryKey::parse("\n").err(),
        Some(RecoveryKeyError::Empty)
    );
    assert_eq!(
        RecoveryKey::parse(&"A".repeat(MAX_RECOVERY_KEY_LENGTH + 1)).err(),
        Some(RecoveryKeyError::TooLong)
    );
}
