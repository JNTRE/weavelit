//! External known-answer validation against the C2SP CCTV age test vectors.
//!
//! `tests/fixtures/` is produced by a second age v1 writer that also lives in
//! this repository, so it binds Weavelit's reader and writer together but does
//! not validate either against the age format itself. The vectors vendored in
//! `tests/vectors/` come from the C2SP Community Cryptography Test Vectors
//! project and are external: they were produced by the reference age
//! implementation and pin this reader's behavior against it.
//!
//! Weavelit's approved profile is deliberately narrower than age. Only one
//! X25519 recipient stanza is accepted, so `scrypt`, hybrid post-quantum, and
//! multi-recipient vectors are rejected by policy rather than decrypted. Those
//! rejections are pinned here by exact category so the narrowing stays
//! deliberate, and the vendored vectors' own `expect` values are pinned so an
//! upstream refresh that changes a vector cannot pass silently.

use std::{collections::BTreeMap, fmt::Write as _, fs, io::Read as _, path::PathBuf};

use flate2::read::ZlibDecoder;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    BACKUP_FORMAT_VERSION, BACKUP_MAGIC, Envelope, MAX_AUTHENTICATED_PLAINTEXT_BYTES, RecoveryKey,
    RestoreError, TransferBounds,
};

/// Manifest file name inside the vendored vector directory.
const MANIFEST_NAME: &str = "vectors.json";

/// Provenance record inside the vendored vector directory.
const README_NAME: &str = "README.md";

/// Plaintext bytes carried by one full age STREAM chunk.
const CHUNK_PLAINTEXT_LENGTH: usize = 64 * 1024;

/// Canonical CCTV X25519 identity.
///
/// A vector that carries no identity this Server's canonical recovery-key
/// syntax accepts — every `scrypt` vector, the hybrid post-quantum vectors, and
/// the empty vector — is still run so the reader itself is exercised. This
/// identity stands in for the missing one; every such vector is rejected by the
/// recipient-stanza policy before any key agreement, so the substitution cannot
/// turn a rejection into a success.
const SUBSTITUTE_IDENTITY: &str =
    "AGE-SECRET-KEY-1EGTZVFFV20835NWYV6270LXYVK2VKNX2MMDKWYKLMGR48UAWX40Q2P2LM0";

/// Outcome this Server's reader must produce for one vendored vector.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Outcome {
    /// Decrypts, and the recovered plaintext matches the vector's payload hash.
    Decrypted,
    /// Rejected as malformed, unauthentic, altered, or truncated.
    Invalid,
    /// Rejected by the narrower single-X25519-recipient parameter policy.
    Incompatible,
}

impl Outcome {
    /// Returns the public Restore category this outcome presents.
    const fn category(self) -> &'static str {
        match self {
            Self::Decrypted => "decrypted",
            Self::Invalid => "backup_invalid",
            Self::Incompatible => "backup_incompatible",
        }
    }
}

/// One vendored vector: its upstream header and its raw age file body.
struct Vector {
    name: String,
    expect: String,
    payload: Option<String>,
    identities: Vec<String>,
    body: Vec<u8>,
}

impl Vector {
    /// Returns the identity to decrypt with, substituting when none applies.
    fn identity(&self) -> &str {
        self.identities
            .iter()
            .find(|line| RecoveryKey::parse(line).is_ok_and(|key| key.into_identity().is_ok()))
            .map_or(SUBSTITUTE_IDENTITY, String::as_str)
    }

    /// Wraps the age file body in the fixed Weavelit outer envelope.
    fn artifact(&self) -> Vec<u8> {
        let mut artifact = Vec::with_capacity(20 + self.body.len());
        artifact.extend_from_slice(&BACKUP_MAGIC);
        artifact.extend_from_slice(&BACKUP_FORMAT_VERSION.to_be_bytes());
        artifact.extend_from_slice(&[0, 0]);
        artifact.extend_from_slice(&(self.body.len() as u64).to_be_bytes());
        artifact.extend_from_slice(&self.body);
        artifact
    }

    /// Runs the production reader over this vector through the outer envelope.
    fn read(&self) -> Result<Vec<u8>, RestoreError> {
        let artifact = self.artifact();
        let envelope = Envelope::parse(&artifact)?;
        let identity = RecoveryKey::parse(self.identity())?.into_identity()?;
        crate::crypto::decrypt_payload(envelope.payload(), &identity, TransferBounds::APPROVED)
            .map(|plaintext| plaintext.to_vec())
    }

    /// Returns the reader's outcome as a comparable category.
    fn outcome(&self) -> Result<Outcome, RestoreError> {
        match self.read() {
            Ok(plaintext) => {
                assert_eq!(
                    Some(digest(&plaintext)),
                    self.payload,
                    "vector {} decrypted to an unexpected payload",
                    self.name
                );
                Ok(Outcome::Decrypted)
            }
            Err(RestoreError::BackupInvalid) => Ok(Outcome::Invalid),
            Err(RestoreError::BackupIncompatible) => Ok(Outcome::Incompatible),
            Err(error) => Err(error),
        }
    }
}

#[derive(Deserialize)]
struct Manifest {
    format_version: u32,
    vectors: BTreeMap<String, ManifestEntry>,
}

#[derive(Deserialize)]
struct ManifestEntry {
    length: usize,
    sha256: String,
}

/// Directory that holds the vendored vectors.
fn directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/vectors")
}

/// Returns the lowercase hexadecimal SHA-256 digest of `bytes`.
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut text, byte| {
            write!(text, "{byte:02x}").expect("string writes cannot fail");
            text
        })
}

/// Returns every vendored file name, excluding the manifest and the README.
fn vendored_names() -> Vec<String> {
    let mut names = fs::read_dir(directory())
        .expect("the vendored vector directory is committed")
        .map(|entry| {
            entry
                .expect("the vendored vector directory is readable")
                .file_name()
                .into_string()
                .expect("every vendored vector name is UTF-8")
        })
        .filter(|name| name != MANIFEST_NAME && name != README_NAME)
        .collect::<Vec<_>>();
    names.sort();
    assert!(
        !names.is_empty(),
        "the vendored vector directory is not empty"
    );
    names
}

fn manifest() -> Manifest {
    serde_json::from_slice(
        &fs::read(directory().join(MANIFEST_NAME)).expect("the vector manifest is committed"),
    )
    .expect("the vector manifest is canonical JSON")
}

/// Parses one vendored vector, decompressing its body when upstream compressed it.
fn parse(name: &str, raw: &[u8]) -> Vector {
    let split = raw
        .windows(2)
        .position(|window| window == b"\n\n")
        .unwrap_or_else(|| panic!("vector {name} separates its header from its body"));
    let header = std::str::from_utf8(&raw[..split])
        .unwrap_or_else(|_| panic!("vector {name} carries a textual header"));

    let mut expect = None;
    let mut payload = None;
    let mut compressed = None;
    let mut identities = Vec::new();
    for line in header.split('\n') {
        let (key, value) = line
            .split_once(": ")
            .or_else(|| line.strip_suffix(':').map(|key| (key, "")))
            .unwrap_or_else(|| panic!("vector {name} header line {line:?} is a key-value pair"));
        match key {
            "expect" => expect = Some(value.to_owned()),
            "payload" => payload = Some(value.to_owned()),
            "compressed" => compressed = Some(value.to_owned()),
            "identity" => identities.push(value.to_owned()),
            // `file key`, `passphrase`, `comment`, and `armored` do not change
            // how this Server reads the file.
            _ => {}
        }
    }

    let body = raw[split + 2..].to_vec();
    let body = match compressed.as_deref() {
        None => body,
        Some("zlib") => {
            let mut decoded = Vec::new();
            ZlibDecoder::new(body.as_slice())
                .read_to_end(&mut decoded)
                .unwrap_or_else(|error| panic!("vector {name} body decompresses: {error}"));
            decoded
        }
        Some(other) => panic!("vector {name} uses unsupported compression {other:?}"),
    };

    Vector {
        name: name.to_owned(),
        expect: expect.unwrap_or_else(|| panic!("vector {name} declares an expectation")),
        payload,
        identities,
        body,
    }
}

/// Loads and validates every vendored vector against the manifest.
fn vectors() -> Vec<Vector> {
    let manifest = manifest();
    assert_eq!(manifest.format_version, 1);

    let names = vendored_names();
    assert_eq!(
        names,
        manifest.vectors.keys().cloned().collect::<Vec<_>>(),
        "the manifest records exactly the vendored vector files"
    );

    names
        .iter()
        .map(|name| {
            let raw = fs::read(directory().join(name))
                .unwrap_or_else(|error| panic!("vector {name} is committed: {error}"));
            let entry = &manifest.vectors[name];
            assert_eq!(entry.length, raw.len(), "{name} length");
            assert_eq!(entry.sha256, digest(&raw), "{name} digest");
            parse(name, &raw)
        })
        .collect()
}

#[test]
fn the_manifest_pins_every_vendored_vector() {
    // `vectors` verifies the manifest set, every length, and every digest.
    assert_eq!(vectors().len(), EXPECTED.len());
}

#[test]
fn the_expectation_table_covers_every_vendored_vector() {
    let mut pinned = EXPECTED.iter().map(|(name, ..)| *name).collect::<Vec<_>>();
    pinned.sort_unstable();
    pinned.dedup();
    assert_eq!(
        pinned.len(),
        EXPECTED.len(),
        "the expectation table names each vector once"
    );
    assert_eq!(
        pinned,
        vendored_names(),
        "the expectation table covers exactly the vendored vectors"
    );
}

#[test]
fn every_vendored_vector_matches_its_pinned_outcome() {
    let pinned = EXPECTED
        .iter()
        .map(|(name, expect, outcome)| (*name, (*expect, *outcome)))
        .collect::<BTreeMap<_, _>>();

    let mut executed = 0_usize;
    let mut by_expectation: BTreeMap<&str, usize> = BTreeMap::new();
    let mut by_outcome: BTreeMap<&str, usize> = BTreeMap::new();

    for vector in vectors() {
        let (expect, expected) = *pinned
            .get(vector.name.as_str())
            .unwrap_or_else(|| panic!("vector {} is unclassified", vector.name));
        assert_eq!(
            expect, vector.expect,
            "vector {} no longer declares the pinned upstream expectation",
            vector.name
        );

        let outcome = vector
            .outcome()
            .unwrap_or_else(|error| panic!("vector {} produced {error:?}", vector.name));
        assert_eq!(
            expected.category(),
            outcome.category(),
            "vector {} ({expect})",
            vector.name
        );

        // A vector upstream expects to fail must never decrypt here.
        if expect != "success" {
            assert_ne!(
                outcome,
                Outcome::Decrypted,
                "vector {} decrypted",
                vector.name
            );
        }

        executed += 1;
        *by_expectation.entry(expect).or_default() += 1;
        *by_outcome.entry(outcome.category()).or_default() += 1;
    }

    assert_eq!(
        executed, VENDORED_VECTOR_COUNT,
        "every vendored vector executed"
    );
    assert_eq!(
        by_expectation,
        BTreeMap::from([
            ("HMAC failure", 1),
            ("header failure", 60),
            ("no match", 12),
            ("payload failure", 18),
            ("success", 19),
        ]),
        "upstream expectation partition"
    );
    assert_eq!(
        by_outcome,
        BTreeMap::from([
            ("backup_incompatible", 42),
            ("backup_invalid", 59),
            ("decrypted", 9),
        ]),
        "reader outcome partition"
    );
}

#[test]
fn every_stream_framing_vector_executes() {
    // These externally validate the STREAM framing, final-chunk flag, and
    // truncation handling that the in-repository fixture generator cannot
    // reach, because it only ever emits a single chunk. `stream_no_final` is
    // the one single-chunk member: it carries a whole stream whose only chunk
    // is not flagged final.
    const REQUIRED: [&str; 12] = [
        "stream_257_chunks",
        "stream_258_chunks",
        "stream_two_chunks",
        "stream_three_chunks",
        "stream_no_final",
        "stream_no_final_two_chunks",
        "stream_two_final_chunks",
        "stream_last_chunk_empty",
        "stream_last_chunk_full",
        "stream_short_second_chunk",
        "stream_trailing_garbage_short",
        "stream_trailing_garbage_long",
    ];

    let vectors = vectors()
        .into_iter()
        .map(|vector| (vector.name.clone(), vector))
        .collect::<BTreeMap<_, _>>();

    for name in REQUIRED {
        let vector = vectors
            .get(name)
            .unwrap_or_else(|| panic!("vector {name} is vendored"));
        if name != "stream_no_final" {
            assert!(
                vector.body.len() > CHUNK_PLAINTEXT_LENGTH,
                "vector {name} spans more than one STREAM chunk"
            );
        }
        vector
            .outcome()
            .unwrap_or_else(|error| panic!("vector {name} produced {error:?}"));
    }
}

#[test]
fn no_vendored_vector_exceeds_the_approved_plaintext_bound() {
    for vector in vectors() {
        assert!(
            vector.body.len() <= MAX_AUTHENTICATED_PLAINTEXT_BYTES,
            "vector {} exceeds the approved plaintext bound",
            vector.name
        );
    }
}

/// Number of vendored vectors, pinned so a lost file fails instead of skipping.
const VENDORED_VECTOR_COUNT: usize = 110;

/// Every vendored vector, its upstream `expect` value, and the outcome this
/// Server's reader must produce for it.
///
/// `Decrypted` additionally asserts the recovered plaintext against the
/// vector's `payload` SHA-256. `Incompatible` marks a vector the approved
/// parameter policy rejects before key agreement: an `scrypt` passphrase
/// recipient, an additional or unknown recipient stanza, a non-canonical
/// stanza type, or an unsupported version line. The three hybrid
/// post-quantum vectors upstream expects to succeed are `Invalid` instead,
/// because their single recipient stanza line is longer than the reader's
/// bounded header scan, so it is rejected as malformed before the stanza type
/// is examined.
const EXPECTED: [(&str, &str, Outcome); VENDORED_VECTOR_COUNT] = [
    ("empty", "header failure", Outcome::Invalid),
    ("header_crlf", "header failure", Outcome::Invalid),
    ("hmac_bad", "HMAC failure", Outcome::Invalid),
    ("hmac_extra_space", "header failure", Outcome::Invalid),
    ("hmac_garbage", "header failure", Outcome::Invalid),
    ("hmac_missing", "header failure", Outcome::Invalid),
    ("hmac_no_space", "header failure", Outcome::Invalid),
    ("hmac_not_canonical", "header failure", Outcome::Invalid),
    ("hmac_trailing_space", "header failure", Outcome::Invalid),
    ("hmac_truncated", "header failure", Outcome::Invalid),
    ("hybrid", "success", Outcome::Invalid),
    ("hybrid_and_x25519", "success", Outcome::Invalid),
    ("hybrid_bad_tag", "no match", Outcome::Invalid),
    ("hybrid_currupted_enc_mlkem", "no match", Outcome::Invalid),
    ("hybrid_currupted_enc_x25519", "no match", Outcome::Invalid),
    ("hybrid_extra_argument", "header failure", Outcome::Invalid),
    ("hybrid_grease", "success", Outcome::Incompatible),
    ("hybrid_identity", "header failure", Outcome::Invalid),
    ("hybrid_long_file_key", "header failure", Outcome::Invalid),
    ("hybrid_long_share", "header failure", Outcome::Invalid),
    ("hybrid_low_order", "header failure", Outcome::Invalid),
    ("hybrid_multiple_recipients", "success", Outcome::Invalid),
    ("hybrid_no_match", "no match", Outcome::Invalid),
    (
        "hybrid_not_canonical_body",
        "header failure",
        Outcome::Invalid,
    ),
    (
        "hybrid_not_canonical_enc",
        "header failure",
        Outcome::Invalid,
    ),
    ("hybrid_short_share", "header failure", Outcome::Invalid),
    ("hybrid_uppercase", "no match", Outcome::Invalid),
    ("hybrid_x25519_arg", "header failure", Outcome::Invalid),
    ("scrypt", "success", Outcome::Incompatible),
    ("scrypt_and_x25519", "header failure", Outcome::Incompatible),
    ("scrypt_bad_tag", "no match", Outcome::Incompatible),
    ("scrypt_double", "header failure", Outcome::Incompatible),
    (
        "scrypt_extra_argument",
        "header failure",
        Outcome::Incompatible,
    ),
    (
        "scrypt_long_file_key",
        "header failure",
        Outcome::Incompatible,
    ),
    ("scrypt_no_match", "no match", Outcome::Incompatible),
    (
        "scrypt_not_canonical_body",
        "header failure",
        Outcome::Incompatible,
    ),
    (
        "scrypt_not_canonical_salt",
        "header failure",
        Outcome::Incompatible,
    ),
    ("scrypt_salt_long", "header failure", Outcome::Incompatible),
    (
        "scrypt_salt_missing",
        "header failure",
        Outcome::Incompatible,
    ),
    ("scrypt_salt_short", "header failure", Outcome::Incompatible),
    ("scrypt_uppercase", "no match", Outcome::Incompatible),
    (
        "scrypt_work_factor_23",
        "header failure",
        Outcome::Incompatible,
    ),
    (
        "scrypt_work_factor_hex",
        "header failure",
        Outcome::Incompatible,
    ),
    (
        "scrypt_work_factor_leading_garbage",
        "header failure",
        Outcome::Incompatible,
    ),
    (
        "scrypt_work_factor_leading_plus",
        "header failure",
        Outcome::Incompatible,
    ),
    (
        "scrypt_work_factor_leading_zero_decimal",
        "header failure",
        Outcome::Incompatible,
    ),
    (
        "scrypt_work_factor_leading_zero_octal",
        "header failure",
        Outcome::Incompatible,
    ),
    (
        "scrypt_work_factor_missing",
        "header failure",
        Outcome::Incompatible,
    ),
    (
        "scrypt_work_factor_negative",
        "header failure",
        Outcome::Incompatible,
    ),
    (
        "scrypt_work_factor_overflow",
        "header failure",
        Outcome::Incompatible,
    ),
    (
        "scrypt_work_factor_trailing_garbage",
        "header failure",
        Outcome::Incompatible,
    ),
    (
        "scrypt_work_factor_wrong",
        "no match",
        Outcome::Incompatible,
    ),
    (
        "scrypt_work_factor_zero",
        "header failure",
        Outcome::Incompatible,
    ),
    ("stanza_bad_start", "header failure", Outcome::Invalid),
    (
        "stanza_base64_padding",
        "header failure",
        Outcome::Incompatible,
    ),
    (
        "stanza_empty_argument",
        "header failure",
        Outcome::Incompatible,
    ),
    ("stanza_empty_body", "success", Outcome::Incompatible),
    ("stanza_empty_last_line", "success", Outcome::Incompatible),
    (
        "stanza_invalid_character",
        "header failure",
        Outcome::Incompatible,
    ),
    ("stanza_long_line", "header failure", Outcome::Incompatible),
    (
        "stanza_missing_body",
        "header failure",
        Outcome::Incompatible,
    ),
    (
        "stanza_missing_final_line",
        "header failure",
        Outcome::Incompatible,
    ),
    (
        "stanza_multiple_short_lines",
        "header failure",
        Outcome::Incompatible,
    ),
    ("stanza_no_arguments", "header failure", Outcome::Invalid),
    (
        "stanza_not_canonical",
        "header failure",
        Outcome::Incompatible,
    ),
    (
        "stanza_spurious_cr",
        "header failure",
        Outcome::Incompatible,
    ),
    ("stanza_valid_characters", "success", Outcome::Incompatible),
    ("stream_257_chunks", "success", Outcome::Decrypted),
    ("stream_257_chunks_full", "success", Outcome::Decrypted),
    ("stream_258_chunks", "success", Outcome::Decrypted),
    ("stream_bad_tag", "payload failure", Outcome::Invalid),
    (
        "stream_bad_tag_second_chunk",
        "payload failure",
        Outcome::Invalid,
    ),
    (
        "stream_bad_tag_second_chunk_full",
        "payload failure",
        Outcome::Invalid,
    ),
    ("stream_empty_payload", "success", Outcome::Decrypted),
    (
        "stream_last_chunk_empty",
        "payload failure",
        Outcome::Invalid,
    ),
    ("stream_last_chunk_full", "success", Outcome::Decrypted),
    (
        "stream_last_chunk_full_second",
        "success",
        Outcome::Decrypted,
    ),
    ("stream_missing_tag", "payload failure", Outcome::Invalid),
    ("stream_no_chunks", "payload failure", Outcome::Invalid),
    ("stream_no_final", "payload failure", Outcome::Invalid),
    ("stream_no_final_full", "payload failure", Outcome::Invalid),
    (
        "stream_no_final_two_chunks",
        "payload failure",
        Outcome::Invalid,
    ),
    (
        "stream_no_final_two_chunks_full",
        "payload failure",
        Outcome::Invalid,
    ),
    ("stream_no_nonce", "header failure", Outcome::Invalid),
    ("stream_short_chunk", "payload failure", Outcome::Invalid),
    ("stream_short_nonce", "header failure", Outcome::Invalid),
    (
        "stream_short_second_chunk",
        "payload failure",
        Outcome::Invalid,
    ),
    ("stream_three_chunks", "success", Outcome::Decrypted),
    (
        "stream_trailing_garbage_long",
        "payload failure",
        Outcome::Invalid,
    ),
    (
        "stream_trailing_garbage_short",
        "payload failure",
        Outcome::Invalid,
    ),
    ("stream_two_chunks", "success", Outcome::Decrypted),
    (
        "stream_two_final_chunks",
        "payload failure",
        Outcome::Invalid,
    ),
    (
        "stream_two_final_chunks_full",
        "payload failure",
        Outcome::Invalid,
    ),
    (
        "stream_two_final_chunks_second",
        "payload failure",
        Outcome::Invalid,
    ),
    (
        "stream_two_final_chunks_short",
        "payload failure",
        Outcome::Invalid,
    ),
    (
        "version_unsupported",
        "header failure",
        Outcome::Incompatible,
    ),
    ("x25519", "success", Outcome::Decrypted),
    ("x25519_bad_tag", "no match", Outcome::Invalid),
    ("x25519_extra_argument", "header failure", Outcome::Invalid),
    ("x25519_grease", "success", Outcome::Incompatible),
    ("x25519_identity", "header failure", Outcome::Invalid),
    ("x25519_long_file_key", "header failure", Outcome::Invalid),
    ("x25519_long_share", "header failure", Outcome::Invalid),
    ("x25519_low_order", "header failure", Outcome::Invalid),
    ("x25519_lowercase", "no match", Outcome::Incompatible),
    (
        "x25519_multiple_recipients",
        "success",
        Outcome::Incompatible,
    ),
    ("x25519_no_match", "no match", Outcome::Invalid),
    (
        "x25519_not_canonical_body",
        "header failure",
        Outcome::Invalid,
    ),
    (
        "x25519_not_canonical_share",
        "header failure",
        Outcome::Invalid,
    ),
    ("x25519_short_share", "header failure", Outcome::Invalid),
];
