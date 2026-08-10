//! Weavelit's own reader for the approved age v1 X25519 recipient profile.
//!
//! The Server only ever decrypts, so this module implements exactly the
//! Security Model's approved profile — one X25519 recipient stanza, an
//! HMAC-SHA-256 header authenticator, and the ChaCha20-Poly1305 STREAM payload
//! — and rejects every other age capability as out of policy. Every size is
//! bounded before it is used to allocate, every chunk is authenticated before
//! the next one is read, and every authentication outcome collapses to one
//! indistinguishable failure.

use base64::{Engine as _, engine::general_purpose::STANDARD_NO_PAD};
use chacha20poly1305::{
    ChaCha20Poly1305, Nonce, Tag,
    aead::{AeadInOut, KeyInit},
};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use x25519_dalek::PublicKey;
use zeroize::Zeroizing;

use crate::{ContentError, RecoveryIdentity, RestoreError, TransferBounds, key::KEY_LENGTH};

/// Fixed first line of an age v1 file.
const VERSION_LINE: &[u8] = b"age-encryption.org/v1\n";

/// Prefix shared by every age version line.
const VERSION_PREFIX: &[u8] = b"age-encryption.org/v";

/// The only recipient stanza type in the approved parameter policy.
const X25519_STANZA_TYPE: &[u8] = b"X25519";

/// HKDF label binding the wrap key to the X25519 recipient stanza.
const X25519_LABEL: &[u8] = b"age-encryption.org/v1/X25519";

/// HKDF label binding the header authenticator key to the file key.
const HEADER_LABEL: &[u8] = b"header";

/// HKDF label binding the payload key to the file key and payload nonce.
const PAYLOAD_LABEL: &[u8] = b"payload";

/// Maximum header bytes scanned before the header is rejected as invalid.
///
/// The approved profile's header is one X25519 stanza and one authenticator
/// line, roughly 130 bytes; the bound leaves room for canonical whitespace
/// without letting an artifact drive an unbounded search.
const MAX_HEADER_BYTES: usize = 1024;

/// Byte length of an age file key.
const FILE_KEY_LENGTH: usize = 16;

/// Byte length of a ChaCha20-Poly1305 authentication tag.
const TAG_LENGTH: usize = 16;

/// Byte length of the age STREAM payload nonce.
const PAYLOAD_NONCE_LENGTH: usize = 16;

/// Plaintext bytes carried by one full age STREAM chunk.
const CHUNK_PLAINTEXT_LENGTH: usize = 64 * 1024;

/// Ciphertext bytes carried by one full age STREAM chunk.
const CHUNK_CIPHERTEXT_LENGTH: usize = CHUNK_PLAINTEXT_LENGTH + TAG_LENGTH;

/// Decrypts and authenticates the framed age v1 stream into bounded memory.
///
/// Every authentication outcome — a wrong recovery key, an altered header, an
/// altered ciphertext, an altered tag, or a truncated stream — collapses to
/// [`RestoreError::BackupInvalid`] with no distinguishing data. Only an
/// out-of-policy age parameter produces [`RestoreError::BackupIncompatible`],
/// and it is rejected before any key agreement.
pub(crate) fn decrypt_payload(
    payload: &[u8],
    identity: &RecoveryIdentity,
    bounds: TransferBounds,
) -> Result<Zeroizing<Vec<u8>>, RestoreError> {
    let header = Header::parse(payload)?;

    let share = PublicKey::from(header.ephemeral_share);
    let shared = identity.diffie_hellman(&share);
    // An all-zero shared secret means the share had small order and contributed
    // nothing to the agreement; the age specification requires rejecting it.
    if !shared.was_contributory() {
        return Err(RestoreError::BackupInvalid);
    }

    let mut salt = [0_u8; KEY_LENGTH * 2];
    salt[..KEY_LENGTH].copy_from_slice(share.as_bytes());
    salt[KEY_LENGTH..].copy_from_slice(&identity.public_key());
    let wrap_key = derive(&salt, X25519_LABEL, shared.as_bytes());

    let mut file_key = Zeroizing::new([0_u8; FILE_KEY_LENGTH]);
    file_key.copy_from_slice(&header.wrapped_file_key[..FILE_KEY_LENGTH]);
    open(
        &ChaCha20Poly1305::new_from_slice(&wrap_key[..])
            .map_err(|_| RestoreError::BackupInvalid)?,
        [0_u8; 12],
        &mut file_key[..],
        &header.wrapped_file_key[FILE_KEY_LENGTH..],
    )?;

    let authenticator = derive(&[], HEADER_LABEL, &file_key[..]);
    let mut mac = Hmac::<Sha256>::new_from_slice(&authenticator[..])
        .map_err(|_| RestoreError::BackupInvalid)?;
    mac.update(header.authenticated);
    mac.verify_slice(&header.mac[..])
        .map_err(|_| RestoreError::BackupInvalid)?;

    let (nonce, chunks) = header
        .payload
        .split_at_checked(PAYLOAD_NONCE_LENGTH)
        .ok_or(RestoreError::BackupInvalid)?;
    let payload_key = derive(nonce, PAYLOAD_LABEL, &file_key[..]);

    decrypt_stream(
        &payload_key,
        chunks,
        bounds.max_authenticated_plaintext_bytes,
    )
}

/// Authenticates and decrypts the age STREAM chunk by chunk.
///
/// The output buffer is reserved once from the already bounded ciphertext
/// length so no sensitive plaintext is copied into a reallocated allocation,
/// and no chunk is admitted to it before its tag verifies.
fn decrypt_stream(
    payload_key: &Zeroizing<[u8; KEY_LENGTH]>,
    chunks: &[u8],
    limit: usize,
) -> Result<Zeroizing<Vec<u8>>, RestoreError> {
    let cipher = ChaCha20Poly1305::new_from_slice(&payload_key[..])
        .map_err(|_| RestoreError::BackupInvalid)?;

    let mut plaintext = Zeroizing::new(Vec::with_capacity(chunks.len().min(limit)));
    let mut remaining = chunks;
    let mut counter: u64 = 0;

    loop {
        // An age stream always carries at least one chunk, and every chunk
        // carries at least its tag.
        if remaining.len() < TAG_LENGTH {
            return Err(RestoreError::BackupInvalid);
        }

        let final_chunk = remaining.len() <= CHUNK_CIPHERTEXT_LENGTH;
        let taken = if final_chunk {
            remaining.len()
        } else {
            CHUNK_CIPHERTEXT_LENGTH
        };
        let (chunk, rest) = remaining.split_at(taken);
        let chunk_length = chunk.len() - TAG_LENGTH;

        // Only a stream whose whole plaintext is empty may end in an empty
        // chunk, so a trailing empty chunk cannot silently truncate a stream.
        if chunk_length == 0 && counter > 0 {
            return Err(RestoreError::BackupInvalid);
        }
        if plaintext.len() + chunk_length > limit {
            return Err(ContentError::PlaintextTooLarge.into());
        }

        let start = plaintext.len();
        plaintext.extend_from_slice(&chunk[..chunk_length]);
        open(
            &cipher,
            chunk_nonce(counter, final_chunk),
            &mut plaintext[start..],
            &chunk[chunk_length..],
        )?;

        if final_chunk {
            return Ok(plaintext);
        }

        remaining = rest;
        counter = counter.checked_add(1).ok_or(RestoreError::BackupInvalid)?;
    }
}

/// Returns the age STREAM nonce for one chunk: an 11-byte big-endian counter
/// followed by the final-chunk flag byte.
fn chunk_nonce(counter: u64, final_chunk: bool) -> [u8; 12] {
    let mut nonce = [0_u8; 12];
    nonce[3..11].copy_from_slice(&counter.to_be_bytes());
    nonce[11] = u8::from(final_chunk);
    nonce
}

/// Authenticates and decrypts `buffer` in place with a detached tag.
fn open(
    cipher: &ChaCha20Poly1305,
    nonce: [u8; 12],
    buffer: &mut [u8],
    tag: &[u8],
) -> Result<(), RestoreError> {
    let tag = Tag::try_from(tag).map_err(|_| RestoreError::BackupInvalid)?;
    cipher
        .decrypt_inout_detached(&Nonce::from(nonce), &[], buffer.into(), &tag)
        .map_err(|_| RestoreError::BackupInvalid)
}

/// Derives 32 bytes with the age HKDF-SHA-256 construction.
fn derive(salt: &[u8], label: &[u8], ikm: &[u8]) -> Zeroizing<[u8; KEY_LENGTH]> {
    let mut derived = Zeroizing::new([0_u8; KEY_LENGTH]);
    Hkdf::<Sha256>::new(Some(salt), ikm)
        .expand(label, &mut derived[..])
        .expect("thirty-two bytes is within the HKDF-SHA-256 output limit");
    derived
}

/// Parsed age v1 header restricted to the approved recipient profile.
struct Header<'stream> {
    /// Ephemeral X25519 share carried by the single recipient stanza.
    ephemeral_share: [u8; KEY_LENGTH],
    /// Wrapped file key and its detached tag.
    wrapped_file_key: [u8; FILE_KEY_LENGTH + TAG_LENGTH],
    /// Header authenticator claimed by the artifact.
    mac: [u8; KEY_LENGTH],
    /// Header bytes the authenticator covers, up to and including `---`.
    authenticated: &'stream [u8],
    /// Payload nonce and STREAM chunks that follow the header.
    payload: &'stream [u8],
}

impl<'stream> Header<'stream> {
    /// Parses the bounded header and enforces the approved parameter policy.
    ///
    /// Weavelit's backup format defines exactly one recovery recipient, so a
    /// missing stanza, a second stanza, and any non-X25519 stanza type —
    /// including `scrypt` — are rejected as out of policy before key agreement.
    /// A structurally malformed header of the supported profile is an invalid
    /// artifact instead.
    fn parse(stream: &'stream [u8]) -> Result<Self, RestoreError> {
        let end = stream.len().min(MAX_HEADER_BYTES);
        let mut cursor = 0;

        let version = read_line(stream, end, &mut cursor)?;
        if version != &VERSION_LINE[..VERSION_LINE.len() - 1] {
            // Only a printable label after the prefix names a different age
            // version. A stray control character, such as the carriage return
            // of a CRLF line ending, is a malformed artifact instead.
            let other_version = version
                .strip_prefix(VERSION_PREFIX)
                .is_some_and(|label| !label.is_empty() && label.iter().all(u8::is_ascii_graphic));
            return Err(if other_version {
                RestoreError::BackupIncompatible
            } else {
                RestoreError::BackupInvalid
            });
        }

        let stanza = read_line(stream, end, &mut cursor)?;
        // A header that reaches its authenticator without a recipient stanza
        // carries no recipient this Server can be, which is a policy outcome
        // rather than a corrupted artifact.
        if stanza.starts_with(b"---") {
            return Err(RestoreError::BackupIncompatible);
        }
        let arguments = stanza
            .strip_prefix(b"-> ")
            .ok_or(RestoreError::BackupInvalid)?;
        let mut arguments = arguments.split(|byte| *byte == b' ');
        let stanza_type = arguments.next().unwrap_or_default();
        if stanza_type.is_empty() {
            return Err(RestoreError::BackupInvalid);
        }
        if stanza_type != X25519_STANZA_TYPE {
            return Err(RestoreError::BackupIncompatible);
        }
        let share = arguments.next().ok_or(RestoreError::BackupInvalid)?;
        if arguments.next().is_some() {
            return Err(RestoreError::BackupInvalid);
        }
        let ephemeral_share = decode_exact::<KEY_LENGTH>(share)?;
        let wrapped_file_key =
            decode_exact::<{ FILE_KEY_LENGTH + TAG_LENGTH }>(read_line(stream, end, &mut cursor)?)?;

        let authenticator_start = cursor;
        let authenticator = read_line(stream, end, &mut cursor)?;
        if authenticator.starts_with(b"-> ") {
            return Err(RestoreError::BackupIncompatible);
        }
        let mac = authenticator
            .strip_prefix(b"--- ")
            .ok_or(RestoreError::BackupInvalid)?;

        Ok(Self {
            ephemeral_share,
            wrapped_file_key,
            mac: decode_exact::<KEY_LENGTH>(mac)?,
            authenticated: &stream[..authenticator_start + 3],
            payload: &stream[cursor..],
        })
    }
}

/// Reads one newline-terminated line from the bounded header region.
fn read_line<'stream>(
    stream: &'stream [u8],
    end: usize,
    cursor: &mut usize,
) -> Result<&'stream [u8], RestoreError> {
    let remaining = stream
        .get(*cursor..end)
        .ok_or(RestoreError::BackupInvalid)?;
    let length = remaining
        .iter()
        .position(|byte| *byte == b'\n')
        .ok_or(RestoreError::BackupInvalid)?;
    *cursor += length + 1;
    Ok(&remaining[..length])
}

/// Decodes canonical unpadded standard Base64 of an exact decoded length.
fn decode_exact<const LENGTH: usize>(text: &[u8]) -> Result<[u8; LENGTH], RestoreError> {
    if text.len() != (LENGTH * 4).div_ceil(3) {
        return Err(RestoreError::BackupInvalid);
    }

    let mut decoded = [0_u8; LENGTH];
    let written = STANDARD_NO_PAD
        .decode_slice(text, &mut decoded)
        .map_err(|_| RestoreError::BackupInvalid)?;
    if written != LENGTH || STANDARD_NO_PAD.encode(decoded).as_bytes() != text {
        return Err(RestoreError::BackupInvalid);
    }

    Ok(decoded)
}
