#![allow(dead_code)]

//! Deterministic generator for the committed Restore backup fixtures.
//!
//! This is an age v1 X25519 *writer* built from fixed key material. It is a
//! second implementation of the same profile that `src/crypto.rs` reads, and
//! the two are deliberately independent: the writer derives HKDF and HMAC by
//! hand from `sha2`, while the production reader uses the maintained `hkdf` and
//! `hmac` crates. Neither calls the other. The committed fixture bytes are the
//! known-answer vectors that bind them together — `tests/fixtures.rs` pins
//! those bytes against this generator, and `tests/validation.rs` decrypts them
//! with the production reader — so a change on either side that breaks the
//! format fails a test.
//!
//! Because both implementations live in this repository, the committed fixtures
//! are not external validation of the age format itself. Nothing here is
//! production code; the Server only ever decrypts.

use std::{collections::BTreeMap, fmt::Write as _, fs, path::Path, sync::OnceLock};

use base64::{Engine as _, engine::general_purpose::STANDARD_NO_PAD};
use chacha20poly1305::{
    ChaCha20Poly1305, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use sha2::{Digest, Sha256};
use weavelit_server_authentication::{
    Argon2Engine as _, CURRENT_ARGON2_PROFILE, PasswordPolicy, RustCryptoArgon2,
};
use weavelit_server_restore::{
    AvailableComponents, BackendIdentifier, DeploymentIdentifier, Name, RequestBudget,
    RestoreAuthority, RestoreError, RestoreRequest, RestoreTarget, RestoreValidator,
};
use x25519_dalek::{PublicKey, StaticSecret};

/// Fixed backup recovery secret used by every valid fixture.
const RECOVERY_SECRET: [u8; 32] = [
    0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20,
    0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f, 0x30,
];

/// Fixed unrelated recovery secret used by the wrong-key fixture.
const WRONG_SECRET: [u8; 32] = [
    0x71, 0x72, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x7b, 0x7c, 0x7d, 0x7e, 0x7f, 0x80,
    0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x8b, 0x8c, 0x8d, 0x8e, 0x8f, 0x90,
];

/// Fixed age ephemeral secret.
const EPHEMERAL_SECRET: [u8; 32] = [
    0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x4b, 0x4c, 0x4d, 0x4e, 0x4f, 0x50,
    0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x5b, 0x5c, 0x5d, 0x5e, 0x5f, 0x60,
];

/// Fixed age file key.
const FILE_KEY: [u8; 16] = [
    0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x6b, 0x6c, 0x6d, 0x6e, 0x6f, 0x70,
];

/// Fixed age payload nonce.
const PAYLOAD_NONCE: [u8; 16] = [
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
];

const IDENTITY_HRP: &str = "AGE-SECRET-KEY-";
const RECIPIENT_HRP: &str = "age";
const X25519_LABEL: &[u8] = b"age-encryption.org/v1/X25519";
const HEADER_LABEL: &[u8] = b"header";
const PAYLOAD_LABEL: &[u8] = b"payload";

/// The password the committed fixtures' `administrator` account is restored
/// with.
///
/// It is a fixture credential rather than a secret: the fixtures exist so a
/// test can restore a deployment and then sign in to it, which is only possible
/// if the fixture's stored verifier was derived from a password the test knows.
pub const FIXTURE_ADMINISTRATOR_PASSWORD: &str = "fixture-administrator-password";

/// Fixed salt the fixture verifier is derived with.
///
/// Argon2 salts are random in production. This one is fixed so the derived
/// verifier, and therefore every fixture that embeds it, stays byte
/// reproducible. Only the leading [`Argon2Profile::salt_bytes`] of it are used,
/// so a profile change carries the salt with it instead of failing to encode.
///
/// [`Argon2Profile::salt_bytes`]: weavelit_server_authentication::Argon2Profile::salt_bytes
const FIXTURE_VERIFIER_SALT: [u8; 48] = [
    0xb1, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xbb, 0xbc, 0xbd, 0xbe, 0xbf, 0xc0,
    0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xcb, 0xcc, 0xcd, 0xce, 0xcf, 0xd0,
    0xd1, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xdb, 0xdc, 0xdd, 0xde, 0xdf, 0xe0,
];

/// Returns the committed fixtures' administrator password verifier.
///
/// The verifier is derived, not written down: it is produced by the Server's own
/// hashing engine at [`CURRENT_ARGON2_PROFILE`], so it cannot drift away from
/// the approved profile or from the profile allowlist that decides whether a
/// stored verifier may be attempted at all. Deriving it costs the approved
/// profile's full memory and time once per test process, so the result is
/// cached.
pub fn administrator_verifier() -> &'static str {
    static VERIFIER: OnceLock<String> = OnceLock::new();
    VERIFIER.get_or_init(|| {
        let salt = &FIXTURE_VERIFIER_SALT[..CURRENT_ARGON2_PROFILE.salt_bytes()];
        RustCryptoArgon2::new(PasswordPolicy::approved())
            .hash(
                FIXTURE_ADMINISTRATOR_PASSWORD.as_bytes(),
                &CURRENT_ARGON2_PROFILE,
                salt,
            )
            .expect("the approved profile must produce a verifier")
    })
}

/// Fixed outer envelope constants, duplicated so a production change is caught.
const MAGIC: [u8; 8] = *b"WLBKUP\r\n";
const FORMAT_VERSION: u16 = 1;

/// One committed fixture file.
pub struct Fixture {
    /// File name inside the fixture directory.
    pub name: &'static str,
    /// Authoritative bytes.
    pub bytes: Vec<u8>,
}

/// Every committed fixture plus the canonical manifest that pins it.
pub struct Fixtures {
    /// Fixture files, excluding the manifest.
    pub files: Vec<Fixture>,
    /// Canonical JSON manifest bytes.
    pub manifest: Vec<u8>,
}

impl Fixtures {
    /// Returns one fixture's bytes by file name.
    pub fn bytes(&self, name: &str) -> &[u8] {
        &self
            .files
            .iter()
            .find(|fixture| fixture.name == name)
            .unwrap_or_else(|| panic!("fixture {name} is generated"))
            .bytes
    }

    /// Returns one fixture's UTF-8 text by file name.
    pub fn text(&self, name: &str) -> &str {
        std::str::from_utf8(self.bytes(name)).expect("fixture is UTF-8")
    }

    /// Writes every fixture and the manifest into `directory`.
    pub fn write(&self, directory: &Path) -> std::io::Result<()> {
        fs::create_dir_all(directory)?;
        for fixture in &self.files {
            fs::write(directory.join(fixture.name), &fixture.bytes)?;
        }
        fs::write(directory.join(MANIFEST_NAME), &self.manifest)
    }
}

/// Manifest file name inside the fixture directory.
pub const MANIFEST_NAME: &str = "fixtures.json";

/// Directory that holds the committed fixtures.
pub fn fixture_directory() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Generates every fixture deterministically from the fixed key material above.
pub fn generate() -> Fixtures {
    let identity_line = encode_identity(&RECOVERY_SECRET);
    let recipient_key = PublicKey::from(&StaticSecret::from(RECOVERY_SECRET));
    let recipient_line = encode_recipient(recipient_key.as_bytes());

    let plaintext = backup_plaintext(1, "sqlite", &recipient_line, Referenced::Full);
    let valid = artifact(&plaintext, &recipient_key);

    let compiled_in_plaintext =
        backup_plaintext(1, "sqlite", &recipient_line, Referenced::CompiledIn);
    let compiled_in = artifact(&compiled_in_plaintext, &recipient_key);

    let mut bad_magic = valid.clone();
    bad_magic[0] ^= 0x01;

    let mut wrong_outer_version = valid.clone();
    wrong_outer_version[8..10].copy_from_slice(&2_u16.to_be_bytes());

    let mut non_zero_flags = valid.clone();
    non_zero_flags[11] = 0x01;

    let mut wrong_declared_length = valid.clone();
    let declared = (valid.len() - 20) as u64;
    wrong_declared_length[12..20].copy_from_slice(&(declared - 1).to_be_bytes());

    let mut truncated_stream = valid.clone();
    truncated_stream.pop();
    truncated_stream[12..20].copy_from_slice(&(declared - 1).to_be_bytes());

    // The last 16 bytes are the final STREAM chunk's Poly1305 tag.
    let mut tampered_tag = valid.clone();
    let tag_index = tampered_tag.len() - 1;
    tampered_tag[tag_index] ^= 0x01;

    let mut tampered_ciphertext = valid.clone();
    let ciphertext_index = tampered_ciphertext.len() - 24;
    tampered_ciphertext[ciphertext_index] ^= 0x01;

    let wrong_inner_version = artifact(
        &backup_plaintext(2, "sqlite", &recipient_line, Referenced::Full),
        &recipient_key,
    );
    let wrong_source_backend = artifact(
        &backup_plaintext(1, "postgresql", &recipient_line, Referenced::Full),
        &recipient_key,
    );

    let files = vec![
        Fixture {
            name: "valid.wlitbackup",
            bytes: valid,
        },
        Fixture {
            name: "valid-plaintext.json",
            bytes: plaintext,
        },
        Fixture {
            name: "valid-web-ui-sqlite.wlitbackup",
            bytes: compiled_in,
        },
        Fixture {
            name: "valid-web-ui-sqlite-plaintext.json",
            bytes: compiled_in_plaintext,
        },
        Fixture {
            name: "valid-identity.txt",
            bytes: identity_line.into_bytes(),
        },
        Fixture {
            name: "valid-recipient.txt",
            bytes: recipient_line.into_bytes(),
        },
        Fixture {
            name: "wrong-identity.txt",
            bytes: encode_identity(&WRONG_SECRET).into_bytes(),
        },
        Fixture {
            name: "malformed-key.txt",
            bytes: malformed_identity(&RECOVERY_SECRET).into_bytes(),
        },
        Fixture {
            name: "multiline-key.txt",
            bytes: format!("{line}\n{line}", line = encode_identity(&RECOVERY_SECRET)).into_bytes(),
        },
        Fixture {
            name: "bad-magic.wlitbackup",
            bytes: bad_magic,
        },
        Fixture {
            name: "wrong-outer-version.wlitbackup",
            bytes: wrong_outer_version,
        },
        Fixture {
            name: "non-zero-flags.wlitbackup",
            bytes: non_zero_flags,
        },
        Fixture {
            name: "wrong-declared-length.wlitbackup",
            bytes: wrong_declared_length,
        },
        Fixture {
            name: "truncated-stream.wlitbackup",
            bytes: truncated_stream,
        },
        Fixture {
            name: "tampered-ciphertext.wlitbackup",
            bytes: tampered_ciphertext,
        },
        Fixture {
            name: "tampered-tag.wlitbackup",
            bytes: tampered_tag,
        },
        Fixture {
            name: "wrong-inner-version.wlitbackup",
            bytes: wrong_inner_version,
        },
        Fixture {
            name: "wrong-source-backend.wlitbackup",
            bytes: wrong_source_backend,
        },
    ];

    let manifest = manifest(&files);

    Fixtures { files, manifest }
}

/// Returns the canonical JSON manifest for the generated fixtures.
fn manifest(files: &[Fixture]) -> Vec<u8> {
    let entries = files
        .iter()
        .map(|fixture| (fixture.name, fixture))
        .collect::<BTreeMap<_, _>>();

    let mut manifest = String::from("{\"format_version\":1,\"fixtures\":{");
    for (index, (name, fixture)) in entries.iter().enumerate() {
        if index > 0 {
            manifest.push(',');
        }
        write!(
            manifest,
            "\"{name}\":{{\"length\":{length},\"sha256\":\"{digest}\"}}",
            length = fixture.bytes.len(),
            digest = digest(&fixture.bytes),
        )
        .expect("string writes cannot fail");
    }
    manifest.push_str("}}");
    manifest.into_bytes()
}

/// Returns the lowercase hexadecimal SHA-256 digest of `bytes`.
pub fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut text, byte| {
            write!(text, "{byte:02x}").expect("string writes cannot fail");
            text
        })
}

/// Which components a generated backup plaintext refers to.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Referenced {
    /// Also names the `totp` MFA Module and the `zendesk` Service Module, which
    /// no Server build in this repository compiles in.
    Full,
    /// Names only the `web-ui` Client Module and the `sqlite` Log Module, which
    /// is exactly what the Server binary compiles in.
    CompiledIn,
}

/// Builds one canonical version 1 backup plaintext.
fn backup_plaintext(
    format_version: u32,
    source_backend: &str,
    recipient_line: &str,
    referenced: Referenced,
) -> Vec<u8> {
    padded_backup_plaintext(
        format_version,
        source_backend,
        recipient_line,
        &[],
        referenced,
    )
}

/// Builds one canonical version 1 backup plaintext with extra padding entries.
///
/// Each entry in `padding` adds one configuration entry whose value is that many
/// `a` characters, which lets a test drive the plaintext to an exact length
/// without changing the shape of the document.
fn padded_backup_plaintext(
    format_version: u32,
    source_backend: &str,
    recipient_line: &str,
    padding: &[usize],
    referenced: Referenced,
) -> Vec<u8> {
    let account = encode_identifier(0x01);
    let group = encode_identifier(0x02);
    let factor = encode_identifier(0x03);
    let connection = encode_identifier(0x04);
    let log_configuration = encode_identifier(0x05);
    let secret = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"provider-token");
    let factor_data = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"totp-seed");
    let component_secret =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"at-rest-value");
    let (mfa_factors, service_connections) = match referenced {
        Referenced::Full => (
            format!(
                "{{\"identifier\":\"{factor}\",\"account\":\"{account}\",\"module\":\"totp\",\"factor_data\":\"{factor_data}\"}}"
            ),
            format!(
                "{{\"identifier\":\"{connection}\",\"service_module\":\"zendesk\",\"name\":\"Primary\",\"credential\":\"{secret}\"}}"
            ),
        ),
        Referenced::CompiledIn => (String::new(), String::new()),
    };
    let padding = padding.iter().enumerate().fold(
        String::new(),
        |mut entries, (index, length)| {
            write!(
                entries,
                ",{{\"component\":\"weavelit-server\",\"key\":\"pad-{index:04}\",\"value\":\"{value}\"}}",
                value = "a".repeat(*length),
            )
            .expect("string writes cannot fail");
            entries
        },
    );

    format!(
        concat!(
            "{{\"format_version\":{format_version},",
            "\"source_backend\":\"{source_backend}\",",
            "\"recovery_public_key\":\"{recipient}\",",
            "\"configuration\":[{{\"component\":\"weavelit-server\",\"key\":\"site-name\",\"value\":\"Example\"}}{padding}],",
            "\"protected_secrets\":[{{\"component\":\"weavelit-server\",\"key\":\"at-rest-probe\",\"value\":\"{component_secret}\"}}],",
            "\"accounts\":[{{\"identifier\":\"{account}\",\"username\":\"administrator\",\"display_name\":\"Site Administrator\",\"active\":true}}],",
            "\"password_verifiers\":[{{\"account\":\"{account}\",\"verifier\":\"{verifier}\"}}],",
            "\"groups\":[{{\"identifier\":\"{group}\",\"name\":\"Administrators\",\"description\":\"Full access\"}}],",
            "\"group_memberships\":[{{\"group\":\"{group}\",\"account\":\"{account}\"}}],",
            "\"group_grants\":[{{\"group\":\"{group}\",\"grant\":{{\"type\":\"server_administration\"}}}},",
            "{{\"group\":\"{group}\",\"grant\":{{\"type\":\"client_module\",\"value\":\"web-ui\"}}}}],",
            "\"mfa_factors\":[{mfa_factors}],",
            "\"service_connections\":[{service_connections}],",
            "\"log_module_configurations\":[{{\"identifier\":\"{log_configuration}\",\"module\":\"sqlite\",\"name\":\"Local\",\"enabled\":true,",
            "\"settings\":[{{\"key\":\"retention-days\",\"value\":\"30\"}}]}}],",
            "\"log_assignments\":[{{\"log_type\":\"system\",\"configuration\":\"{log_configuration}\"}},",
            "{{\"log_type\":\"audit\",\"configuration\":\"{log_configuration}\"}}]}}"
        ),
        format_version = format_version,
        source_backend = source_backend,
        recipient = recipient_line,
        padding = padding,
        component_secret = component_secret,
        account = account,
        group = group,
        verifier = administrator_verifier(),
        mfa_factors = mfa_factors,
        service_connections = service_connections,
        log_configuration = log_configuration,
    )
    .into_bytes()
}

fn encode_identifier(seed: u8) -> String {
    let mut bytes = [0_u8; 16];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = seed.wrapping_add(index as u8).wrapping_add(1);
    }
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

// ---------------------------------------------------------------------------
// age v1 X25519 recipient profile
// ---------------------------------------------------------------------------

/// Wraps one age v1 stream in the fixed Weavelit outer envelope.
fn artifact(plaintext: &[u8], recipient: &PublicKey) -> Vec<u8> {
    envelope(&age_stream(plaintext, recipient))
}

/// Builds one deterministic age v1 file encrypted to a single X25519 recipient.
fn age_stream(plaintext: &[u8], recipient: &PublicKey) -> Vec<u8> {
    age_stream_with(plaintext, recipient, true)
}

/// Builds one deterministic age v1 file, optionally leaving the last STREAM
/// chunk unflagged so a reader that ignores the final-chunk flag is caught.
fn age_stream_with(plaintext: &[u8], recipient: &PublicKey, flag_final: bool) -> Vec<u8> {
    let ephemeral = StaticSecret::from(EPHEMERAL_SECRET);
    let ephemeral_share = PublicKey::from(&ephemeral);
    let shared = ephemeral.diffie_hellman(recipient);

    let mut salt = [0_u8; 64];
    salt[..32].copy_from_slice(ephemeral_share.as_bytes());
    salt[32..].copy_from_slice(recipient.as_bytes());
    let wrap_key = hkdf_sha256(&salt, X25519_LABEL, shared.as_bytes());
    let wrapped = seal(&wrap_key, &[0; 12], &FILE_KEY);

    let mut header = Vec::new();
    header.extend_from_slice(b"age-encryption.org/v1\n-> X25519 ");
    header.extend_from_slice(
        STANDARD_NO_PAD
            .encode(ephemeral_share.as_bytes())
            .as_bytes(),
    );
    header.push(b'\n');
    header.extend_from_slice(STANDARD_NO_PAD.encode(&wrapped).as_bytes());
    header.extend_from_slice(b"\n---");

    let mac_key = hkdf_sha256(&[], HEADER_LABEL, &FILE_KEY);
    let mac = hmac_sha256(&mac_key, &header);
    header.push(b' ');
    header.extend_from_slice(STANDARD_NO_PAD.encode(mac).as_bytes());
    header.push(b'\n');

    let payload_key = hkdf_sha256(&PAYLOAD_NONCE, PAYLOAD_LABEL, &FILE_KEY);

    let mut stream = header;
    stream.extend_from_slice(&PAYLOAD_NONCE);
    // An empty plaintext still produces exactly one empty final chunk.
    let total = plaintext.len().div_ceil(CHUNK_PLAINTEXT_LENGTH).max(1);
    for counter in 0..total {
        let start = counter * CHUNK_PLAINTEXT_LENGTH;
        let chunk = &plaintext[start..(start + CHUNK_PLAINTEXT_LENGTH).min(plaintext.len())];
        let last = counter + 1 == total;
        let mut nonce = [0_u8; 12];
        nonce[3..11].copy_from_slice(&(counter as u64).to_be_bytes());
        nonce[11] = u8::from(last && flag_final);
        stream.extend_from_slice(&seal(&payload_key, &nonce, chunk));
    }
    stream
}

/// Plaintext bytes carried by one full age STREAM chunk.
pub const CHUNK_PLAINTEXT_LENGTH: usize = 64 * 1024;

/// One generated backup artifact and the plaintext it encrypts.
pub struct GeneratedBackup {
    /// Complete Weavelit artifact bytes.
    pub artifact: Vec<u8>,
    /// Exact authenticated plaintext the artifact carries.
    pub plaintext: Vec<u8>,
    /// Number of configuration entries the plaintext declares.
    pub configuration_entries: usize,
}

/// Generates a valid backup whose plaintext is exactly `plaintext_length` bytes.
///
/// The committed fixtures are all one STREAM chunk, so tests generate the
/// larger multi-chunk artifacts at run time instead of committing megabytes of
/// opaque binary.
pub fn generated_backup(plaintext_length: usize) -> GeneratedBackup {
    generate_backup(plaintext_length, true)
}

/// Generates the same backup with its last STREAM chunk left unflagged.
pub fn generated_backup_without_final_flag(plaintext_length: usize) -> Vec<u8> {
    generate_backup(plaintext_length, false).artifact
}

fn generate_backup(plaintext_length: usize, flag_final: bool) -> GeneratedBackup {
    /// Maximum accepted length of one configuration value.
    const MAX_VALUE_LENGTH: usize = 4 * 1024;

    let recipient_key = PublicKey::from(&StaticSecret::from(RECOVERY_SECRET));
    let recipient_line = encode_recipient(recipient_key.as_bytes());
    let build = |padding: &[usize]| {
        padded_backup_plaintext(1, "sqlite", &recipient_line, padding, Referenced::Full)
    };

    let mut padding = Vec::new();
    let plaintext = loop {
        let mut candidate = padding.clone();
        candidate.push(1);
        let length = build(&candidate).len();
        assert!(
            length <= plaintext_length,
            "the requested plaintext length is at least one padded document"
        );

        let deficit = plaintext_length - length;
        if deficit < MAX_VALUE_LENGTH {
            candidate.pop();
            candidate.push(1 + deficit);
            padding = candidate;
            break build(&padding);
        }
        padding.push(MAX_VALUE_LENGTH);
    };
    assert_eq!(plaintext.len(), plaintext_length);

    let stream = age_stream_with(&plaintext, &recipient_key, flag_final);

    GeneratedBackup {
        artifact: envelope(&stream),
        plaintext,
        configuration_entries: 1 + padding.len(),
    }
}

/// Wraps arbitrary age stream bytes in a well-formed outer Weavelit envelope.
pub fn envelope(stream: &[u8]) -> Vec<u8> {
    let mut artifact = Vec::with_capacity(20 + stream.len());
    artifact.extend_from_slice(&MAGIC);
    artifact.extend_from_slice(&FORMAT_VERSION.to_be_bytes());
    artifact.extend_from_slice(&[0, 0]);
    artifact.extend_from_slice(&(stream.len() as u64).to_be_bytes());
    artifact.extend_from_slice(stream);
    artifact
}

/// Splits one artifact into its age header text and the payload that follows.
///
/// The age v1 header is ASCII through its authenticator line, so a test can
/// rewrite one header property as text without disturbing the binary payload.
pub fn split_stream(artifact: &[u8]) -> (String, Vec<u8>) {
    let stream = &artifact[20..];
    let dashes = stream
        .windows(3)
        .position(|window| window == b"---")
        .expect("the age header carries an authenticator line");
    let end = dashes
        + stream[dashes..]
            .iter()
            .position(|byte| *byte == b'\n')
            .expect("the authenticator line is newline terminated")
        + 1;
    (
        String::from_utf8(stream[..end].to_vec()).expect("the age header is ASCII"),
        stream[end..].to_vec(),
    )
}

/// Returns `artifact` with its last byte removed and its declared length fixed.
pub fn truncate_artifact(artifact: &[u8]) -> Vec<u8> {
    let mut truncated = artifact.to_vec();
    truncated.pop();
    let declared = (truncated.len() - 20) as u64;
    truncated[12..20].copy_from_slice(&declared.to_be_bytes());
    truncated
}

fn seal(key: &[u8; 32], nonce: &[u8; 12], plaintext: &[u8]) -> Vec<u8> {
    ChaCha20Poly1305::new_from_slice(key)
        .expect("the key is exactly 32 bytes")
        .encrypt(
            &Nonce::from(*nonce),
            Payload {
                msg: plaintext,
                aad: &[],
            },
        )
        .expect("the fixture chunk is within the STREAM chunk size")
}

fn hkdf_sha256(salt: &[u8], label: &[u8], ikm: &[u8]) -> [u8; 32] {
    let pseudorandom_key = hmac_sha256(salt, ikm);
    let mut input = label.to_vec();
    input.push(0x01);
    hmac_sha256(&pseudorandom_key, &input)
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;

    let mut block = [0_u8; BLOCK];
    if key.len() > BLOCK {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }

    let mut inner_digest = [0_u8; 32];
    let mut inner = Sha256::new();
    inner.update(block.map(|byte| byte ^ 0x36));
    inner.update(message);
    inner_digest.copy_from_slice(&inner.finalize());

    let mut outer_digest = [0_u8; 32];
    let mut outer = Sha256::new();
    outer.update(block.map(|byte| byte ^ 0x5c));
    outer.update(inner_digest);
    outer_digest.copy_from_slice(&outer.finalize());
    outer_digest
}

// ---------------------------------------------------------------------------
// Canonical age Bech32 encoding
// ---------------------------------------------------------------------------

fn encode_identity(secret: &[u8; 32]) -> String {
    bech32::encode_lower::<bech32::Bech32>(
        bech32::Hrp::parse(IDENTITY_HRP).expect("the identity prefix is a valid HRP"),
        secret,
    )
    .expect("the identity payload is within the Bech32 length limit")
    .to_uppercase()
}

fn encode_recipient(public: &[u8; 32]) -> String {
    bech32::encode_lower::<bech32::Bech32>(
        bech32::Hrp::parse(RECIPIENT_HRP).expect("the recipient prefix is a valid HRP"),
        public,
    )
    .expect("the recipient payload is within the Bech32 length limit")
}

/// Returns a canonical-looking identity whose Bech32 checksum is wrong.
fn malformed_identity(secret: &[u8; 32]) -> String {
    let mut line = encode_identity(secret).into_bytes();
    let last = line.len() - 1;
    line[last] = if line[last] == b'Q' { b'P' } else { b'Q' };
    String::from_utf8(line).expect("the mutated identity stays ASCII")
}

// ---------------------------------------------------------------------------
// Shared validation harness
// ---------------------------------------------------------------------------

/// Reads one committed fixture file.
pub fn committed(name: &str) -> Vec<u8> {
    fs::read(fixture_directory().join(name))
        .unwrap_or_else(|error| panic!("fixture {name} is committed: {error}"))
}

/// Reads one committed fixture file as UTF-8 text.
pub fn committed_text(name: &str) -> String {
    String::from_utf8(committed(name)).expect("the fixture is UTF-8")
}

/// Replacement deployment identifier used by the test authority.
pub fn deployment() -> DeploymentIdentifier {
    DeploymentIdentifier::from_bytes([
        0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xab, 0xac, 0xad, 0xae, 0xaf,
        0xb0,
    ])
    .expect("the fixed identifier is non-zero")
}

/// Components the fixture backup references.
pub fn components() -> AvailableComponents {
    fn names(values: &[&str]) -> std::collections::BTreeSet<Name> {
        values
            .iter()
            .map(|value| Name::new(*value).expect("the component name is valid"))
            .collect()
    }

    AvailableComponents {
        client_modules: names(&["web-ui", "weavelit-cli", "mcp"]),
        mfa_modules: names(&["totp"]),
        service_modules: names(&["zendesk"]),
        log_modules: names(&["sqlite"]),
        operations: names(&["ticket-search"]),
    }
}

/// Lifecycle authority stub bound to the fixture backend.
pub struct TestAuthority {
    target: Result<RestoreTarget, RestoreError>,
}

impl TestAuthority {
    /// Authorizes Restore against the given Application Database backend.
    pub fn eligible(backend: &str) -> Self {
        Self {
            target: Ok(RestoreTarget::new(
                deployment(),
                BackendIdentifier::new(backend).expect("the backend identifier is valid"),
            )),
        }
    }

    /// Rejects Restore with the given lifecycle outcome.
    pub const fn rejecting(error: RestoreError) -> Self {
        Self { target: Err(error) }
    }
}

impl RestoreAuthority for TestAuthority {
    fn authorize(&self) -> Result<RestoreTarget, RestoreError> {
        self.target.clone()
    }
}

/// Validates one artifact and recovery key against the fixture components.
pub fn validate(
    artifact: &[u8],
    recovery_key: &str,
) -> Result<weavelit_server_restore::ValidatedBackup, RestoreError> {
    RestoreValidator::new(components()).validate(
        &TestAuthority::eligible("sqlite"),
        &RequestBudget::start(),
        RestoreRequest {
            artifact,
            recovery_key,
        },
    )
}

/// Returns the public category and reason a failed validation presents.
pub fn category(error: RestoreError) -> (&'static str, &'static str) {
    error.category_reason()
}
