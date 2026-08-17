//! Canonical age Bech32 recovery-key handling.

mod support;

use support::committed_text;
use weavelit_server_restore::{
    IDENTITY_PREFIX, MAX_RECOVERY_KEY_LENGTH, RECIPIENT_PREFIX, RecoveryKey, RecoveryKeyError,
};

#[test]
fn the_canonical_prefixes_and_bound_are_fixed() {
    assert_eq!(IDENTITY_PREFIX, "AGE-SECRET-KEY-1");
    assert_eq!(RECIPIENT_PREFIX, "age1");
    assert_eq!(MAX_RECOVERY_KEY_LENGTH, 128);
}

#[test]
fn a_canonical_identity_line_parses_and_yields_an_identity() {
    let line = committed_text("valid-identity.txt");
    assert!(line.starts_with(IDENTITY_PREFIX));

    let key = RecoveryKey::parse(&line).expect("the committed identity is canonical");
    key.into_identity()
        .expect("an identity line yields an identity");
}

#[test]
fn one_optional_trailing_newline_is_accepted() {
    let line = format!("{}\n", committed_text("valid-identity.txt"));
    RecoveryKey::parse(&line).expect("a single trailing newline is tolerated");
}

#[test]
fn a_recipient_line_parses_but_cannot_decrypt() {
    let line = committed_text("valid-recipient.txt");
    assert!(line.starts_with(RECIPIENT_PREFIX));

    let key = RecoveryKey::parse(&line).expect("the committed recipient is canonical");
    assert_eq!(
        key.into_identity().err(),
        Some(RecoveryKeyError::IdentityRequired)
    );
}

#[test]
fn an_empty_submission_is_rejected() {
    assert_eq!(RecoveryKey::parse("").err(), Some(RecoveryKeyError::Empty));
    assert_eq!(
        RecoveryKey::parse("\n").err(),
        Some(RecoveryKeyError::Empty)
    );
}

#[test]
fn an_oversize_submission_is_rejected_before_decoding() {
    let line = "A".repeat(MAX_RECOVERY_KEY_LENGTH + 1);
    assert_eq!(
        RecoveryKey::parse(&line).err(),
        Some(RecoveryKeyError::TooLong)
    );
}

#[test]
fn a_multi_line_submission_is_rejected() {
    // The committed multi-line fixture is two canonical lines, so the length
    // bound fires first; both outcomes present the same public failure.
    let line = committed_text("multiline-key.txt");
    assert!(line.len() > MAX_RECOVERY_KEY_LENGTH);
    assert_eq!(
        RecoveryKey::parse(&line).err(),
        Some(RecoveryKeyError::TooLong)
    );

    for candidate in ["age1\nage1", "age1\rage1", "age1\n\n"] {
        assert_eq!(
            RecoveryKey::parse(candidate).err(),
            Some(RecoveryKeyError::NotSingleLine),
            "{candidate:?}"
        );
    }
}

#[test]
fn surrounding_whitespace_is_not_silently_accepted() {
    let line = committed_text("valid-identity.txt");
    for candidate in [format!(" {line}"), format!("{line} "), format!("\t{line}")] {
        assert_eq!(
            RecoveryKey::parse(&candidate).err(),
            Some(RecoveryKeyError::SurroundingContent),
            "{candidate:?}"
        );
    }
}

#[test]
fn a_non_canonical_key_is_rejected() {
    for candidate in [
        committed_text("malformed-key.txt"),
        committed_text("valid-identity.txt").to_lowercase(),
        committed_text("valid-recipient.txt").to_uppercase(),
        "AGE-SECRET-KEY-1".to_owned(),
        "age1".to_owned(),
        "not-a-recovery-key".to_owned(),
    ] {
        assert_eq!(
            RecoveryKey::parse(&candidate).err(),
            Some(RecoveryKeyError::NotCanonical),
            "{candidate:?}"
        );
    }
}

#[test]
fn recovery_key_failures_render_uniformly_and_redact_the_key() {
    let rendered: Vec<String> = [
        RecoveryKeyError::Empty,
        RecoveryKeyError::TooLong,
        RecoveryKeyError::NotSingleLine,
        RecoveryKeyError::SurroundingContent,
        RecoveryKeyError::NotCanonical,
        RecoveryKeyError::IdentityRequired,
    ]
    .iter()
    .map(|error| error.to_string())
    .collect();
    assert!(
        rendered.iter().all(|text| text == &rendered[0]),
        "{rendered:?}"
    );

    let line = committed_text("valid-identity.txt");
    let key = RecoveryKey::parse(&line).expect("the committed identity is canonical");
    let debug = format!("{key:?}");
    assert!(debug.contains("REDACTED"), "{debug}");
    assert!(!debug.contains(&line), "{debug}");
}
