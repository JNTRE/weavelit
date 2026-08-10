//! The fixed outer backup envelope accepts exactly one shape.

mod support;

use support::committed;
use weavelit_server_restore::{BACKUP_FORMAT_VERSION, BACKUP_MAGIC, Envelope, EnvelopeError};

fn header(version: u16, flags: [u8; 2], declared: u64) -> Vec<u8> {
    let mut header = Vec::with_capacity(20);
    header.extend_from_slice(&BACKUP_MAGIC);
    header.extend_from_slice(&version.to_be_bytes());
    header.extend_from_slice(&flags);
    header.extend_from_slice(&declared.to_be_bytes());
    header
}

fn artifact(version: u16, flags: [u8; 2], declared: u64, payload: &[u8]) -> Vec<u8> {
    let mut artifact = header(version, flags, declared);
    artifact.extend_from_slice(payload);
    artifact
}

#[test]
fn the_magic_and_header_length_are_fixed() {
    assert_eq!(BACKUP_MAGIC, *b"WLBKUP\r\n");
    assert_eq!(BACKUP_FORMAT_VERSION, 1);
    assert_eq!(weavelit_server_restore::HEADER_LENGTH, 20);
}

#[test]
fn a_well_formed_envelope_exposes_only_the_declared_payload() {
    let payload = b"age-encryption.org/v1\n";
    let artifact = artifact(1, [0, 0], payload.len() as u64, payload);
    let envelope = Envelope::parse(&artifact).expect("the envelope is well formed");

    assert_eq!(envelope.format_version(), BACKUP_FORMAT_VERSION);
    assert_eq!(envelope.payload(), payload);
}

#[test]
fn the_committed_fixture_declares_its_exact_payload_length() {
    let artifact = committed("valid.wlitbackup");
    let envelope = Envelope::parse(&artifact).expect("the fixture envelope is well formed");

    assert_eq!(envelope.payload().len(), artifact.len() - 20);
    assert!(envelope.payload().starts_with(b"age-encryption.org/v1\n"));
}

#[test]
fn a_short_artifact_cannot_contain_a_header() {
    for length in 0..20 {
        let truncated = committed("valid.wlitbackup")[..length].to_vec();
        assert_eq!(
            Envelope::parse(&truncated),
            Err(EnvelopeError::TooShort),
            "length {length}"
        );
    }
}

#[test]
fn a_foreign_magic_is_rejected() {
    let payload = b"payload";
    let mut artifact = artifact(1, [0, 0], payload.len() as u64, payload);
    artifact[0] = b'X';

    assert_eq!(
        Envelope::parse(&artifact),
        Err(EnvelopeError::MagicMismatch)
    );
}

#[test]
fn only_format_version_one_is_accepted() {
    let payload = b"payload";
    for version in [0, 2, u16::MAX] {
        let artifact = artifact(version, [0, 0], payload.len() as u64, payload);
        assert_eq!(
            Envelope::parse(&artifact),
            Err(EnvelopeError::UnsupportedFormatVersion),
            "version {version}"
        );
    }
}

#[test]
fn reserved_flag_bytes_must_be_zero() {
    let payload = b"payload";
    for flags in [[1, 0], [0, 1], [0xff, 0xff]] {
        let artifact = artifact(1, flags, payload.len() as u64, payload);
        assert_eq!(
            Envelope::parse(&artifact),
            Err(EnvelopeError::FlagsNotZero),
            "flags {flags:?}"
        );
    }
}

#[test]
fn the_declared_length_must_match_the_remaining_stream_exactly() {
    let payload = b"payload";
    for declared in [0, 6, 8, u64::MAX] {
        let artifact = artifact(1, [0, 0], declared, payload);
        assert_eq!(
            Envelope::parse(&artifact),
            Err(EnvelopeError::DeclaredLengthMismatch),
            "declared {declared}"
        );
    }
}

#[test]
fn envelope_failures_render_without_disclosing_the_failed_property() {
    let rendered: Vec<String> = [
        EnvelopeError::TooShort,
        EnvelopeError::MagicMismatch,
        EnvelopeError::UnsupportedFormatVersion,
        EnvelopeError::FlagsNotZero,
        EnvelopeError::DeclaredLengthMismatch,
    ]
    .iter()
    .map(|error| error.to_string())
    .collect();

    assert!(
        rendered.iter().all(|text| text == &rendered[0]),
        "{rendered:?}"
    );
}
