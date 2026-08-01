use std::fmt;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use zeroize::Zeroizing;

use crate::{
    BackendIdentifier, ConnectionFieldIdentifier, ConnectionValue, DatabaseLocator,
    DeploymentIdentifier, DeploymentRecord, LIFECYCLE_FORMAT_VERSION, LifecycleError,
    LifecycleState, LocatorConnectionField, LocatorConnectionSettings, LocatorGeneration,
    MAX_CONNECTION_FIELDS,
};

pub(crate) const KEY_FILE_NAME: &str = "lifecycle-key.json";
pub(crate) const RECORD_FILE_NAME: &str = "deployment-record.json";
pub(crate) const LOCK_FILE_NAME: &str = "lifecycle.lock";
pub(crate) const KEY_FILE_LIMIT: usize = 512;
pub(crate) const RECORD_ENVELOPE_LIMIT: usize = 4 * 1024;
pub(crate) const RECORD_PLAINTEXT_LIMIT: usize = 1024;
pub(crate) const LOCATOR_ENVELOPE_LIMIT: usize = 64 * 1024;
pub(crate) const LOCATOR_PLAINTEXT_LIMIT: usize = 32 * 1024;

const KEY_LENGTH: usize = 32;
const NONCE_LENGTH: usize = 24;
const TAG_LENGTH: usize = 16;
const ALGORITHM: &str = "xchacha20-poly1305";
const RECORD_AAD: &[u8] = b"weavelit:lifecycle:deployment-record:v1";
const LOCATOR_AAD_PREFIX: &[u8] = b"weavelit:lifecycle:database-locator:v1:";

pub(crate) struct AnchorKey(Zeroizing<[u8; KEY_LENGTH]>);

impl AnchorKey {
    pub(crate) fn from_bytes(bytes: [u8; KEY_LENGTH]) -> Result<Self, LifecycleError> {
        if bytes == [0; KEY_LENGTH] {
            return Err(LifecycleError::IntegrityFailure);
        }
        Ok(Self(Zeroizing::new(bytes)))
    }

    pub(crate) fn as_bytes(&self) -> &[u8; KEY_LENGTH] {
        &self.0
    }
}

impl fmt::Debug for AnchorKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AnchorKey(REDACTED)")
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct KeyFileV1 {
    format_version: u32,
    key_algorithm: String,
    key: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EnvelopeV1 {
    format_version: u32,
    aead_algorithm: String,
    nonce: String,
    ciphertext: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RecordPayloadV1 {
    deployment_identifier: String,
    lifecycle_state: String,
    locator_generation: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LocatorPayloadV1 {
    deployment_identifier: String,
    locator_generation: String,
    backend_identifier: String,
    settings: Vec<LocatorSettingV1>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LocatorSettingV1 {
    field_identifier: String,
    value: ConnectionValueV1,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "lowercase")]
enum ConnectionValueV1 {
    String(String),
    Integer(i64),
    Boolean(bool),
    Bytes(String),
}

pub(crate) fn generate_key() -> Result<AnchorKey, LifecycleError> {
    AnchorKey::from_bytes(random_nonzero_bytes()?)
}

pub(crate) fn generate_deployment_identifier() -> Result<DeploymentIdentifier, LifecycleError> {
    DeploymentIdentifier::from_bytes(random_nonzero_bytes()?)
        .map_err(|_| LifecycleError::IntegrityFailure)
}

pub(crate) fn generate_locator_generation() -> Result<LocatorGeneration, LifecycleError> {
    LocatorGeneration::from_bytes(random_nonzero_bytes()?)
        .map_err(|_| LifecycleError::IntegrityFailure)
}

pub(crate) fn generate_nonce() -> Result<[u8; NONCE_LENGTH], LifecycleError> {
    random_nonzero_bytes()
}

pub(crate) fn locator_file_name(generation: LocatorGeneration) -> String {
    format!("database-locator-{}.json", generation_token(generation))
}

pub(crate) fn generation_token(generation: LocatorGeneration) -> String {
    encode(generation.as_bytes())
}

pub(crate) fn parse_generation_token(value: &str) -> Result<LocatorGeneration, LifecycleError> {
    LocatorGeneration::from_bytes(decode_array(value)?)
        .map_err(|_| LifecycleError::IntegrityFailure)
}

pub(crate) fn parse_locator_file_name(
    file_name: &str,
) -> Result<Option<LocatorGeneration>, LifecycleError> {
    let Some(token) = file_name
        .strip_prefix("database-locator-")
        .and_then(|name| name.strip_suffix(".json"))
    else {
        return Ok(None);
    };
    parse_generation_token(token).map(Some)
}

pub(crate) fn temporary_file_name(final_name: &str) -> Result<String, LifecycleError> {
    let token = generation_token(generate_locator_generation()?);
    Ok(format!("{final_name}.tmp-{token}"))
}

pub(crate) fn serialize_key(key: &AnchorKey) -> Result<Vec<u8>, LifecycleError> {
    serialize_canonical(&KeyFileV1 {
        format_version: LIFECYCLE_FORMAT_VERSION,
        key_algorithm: ALGORITHM.to_owned(),
        key: encode(key.as_bytes()),
    })
}

pub(crate) fn parse_key(bytes: &[u8]) -> Result<AnchorKey, LifecycleError> {
    let value: KeyFileV1 = parse_canonical(bytes, KEY_FILE_LIMIT)?;
    validate_version(value.format_version)?;
    if value.key_algorithm != ALGORITHM {
        return Err(LifecycleError::UnsupportedVersion);
    }
    AnchorKey::from_bytes(decode_array(&value.key)?)
}

pub(crate) fn encrypt_record(
    key: &AnchorKey,
    record: &DeploymentRecord,
    nonce: [u8; NONCE_LENGTH],
) -> Result<Vec<u8>, LifecycleError> {
    let plaintext = serialize_canonical(&record_to_payload(record))?;
    if plaintext.len() > RECORD_PLAINTEXT_LIMIT {
        return Err(LifecycleError::IntegrityFailure);
    }
    encrypt_envelope(key, nonce, RECORD_AAD, &plaintext)
}

pub(crate) fn decrypt_record(
    key: &AnchorKey,
    bytes: &[u8],
) -> Result<DeploymentRecord, LifecycleError> {
    let plaintext = decrypt_envelope(
        key,
        bytes,
        RECORD_AAD,
        RECORD_ENVELOPE_LIMIT,
        RECORD_PLAINTEXT_LIMIT,
    )?;
    let payload: RecordPayloadV1 = parse_canonical(&plaintext, RECORD_PLAINTEXT_LIMIT)?;
    payload_to_record(payload)
}

pub(crate) fn encrypt_locator(
    key: &AnchorKey,
    locator: &DatabaseLocator,
    nonce: [u8; NONCE_LENGTH],
) -> Result<Vec<u8>, LifecycleError> {
    let plaintext = serialize_canonical(&locator_to_payload(locator))?;
    if plaintext.len() > LOCATOR_PLAINTEXT_LIMIT {
        return Err(LifecycleError::IntegrityFailure);
    }
    let aad = locator_aad(locator.generation());
    encrypt_envelope(key, nonce, &aad, &plaintext)
}

pub(crate) fn decrypt_locator(
    key: &AnchorKey,
    generation: LocatorGeneration,
    bytes: &[u8],
) -> Result<DatabaseLocator, LifecycleError> {
    let aad = locator_aad(generation);
    let plaintext = decrypt_envelope(
        key,
        bytes,
        &aad,
        LOCATOR_ENVELOPE_LIMIT,
        LOCATOR_PLAINTEXT_LIMIT,
    )?;
    let payload: LocatorPayloadV1 = parse_canonical(&plaintext, LOCATOR_PLAINTEXT_LIMIT)?;
    payload_to_locator(payload, generation)
}

fn encrypt_envelope(
    key: &AnchorKey,
    nonce: [u8; NONCE_LENGTH],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, LifecycleError> {
    let cipher = XChaCha20Poly1305::new_from_slice(key.as_bytes())
        .map_err(|_| LifecycleError::IntegrityFailure)?;
    let ciphertext = cipher
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| LifecycleError::IntegrityFailure)?;
    serialize_canonical(&EnvelopeV1 {
        format_version: LIFECYCLE_FORMAT_VERSION,
        aead_algorithm: ALGORITHM.to_owned(),
        nonce: encode(&nonce),
        ciphertext: encode(&ciphertext),
    })
}

fn decrypt_envelope(
    key: &AnchorKey,
    bytes: &[u8],
    aad: &[u8],
    envelope_limit: usize,
    plaintext_limit: usize,
) -> Result<Zeroizing<Vec<u8>>, LifecycleError> {
    let envelope: EnvelopeV1 = parse_canonical(bytes, envelope_limit)?;
    validate_version(envelope.format_version)?;
    if envelope.aead_algorithm != ALGORITHM {
        return Err(LifecycleError::UnsupportedVersion);
    }
    let nonce = decode_array(&envelope.nonce)?;
    let ciphertext = decode(&envelope.ciphertext)?;
    if ciphertext.len() < TAG_LENGTH || ciphertext.len() > plaintext_limit + TAG_LENGTH {
        return Err(LifecycleError::IntegrityFailure);
    }
    let cipher = XChaCha20Poly1305::new_from_slice(key.as_bytes())
        .map_err(|_| LifecycleError::IntegrityFailure)?;
    let plaintext = cipher
        .decrypt(
            &XNonce::from(nonce),
            Payload {
                msg: &ciphertext,
                aad,
            },
        )
        .map_err(|_| LifecycleError::IntegrityFailure)?;
    if plaintext.len() > plaintext_limit {
        return Err(LifecycleError::IntegrityFailure);
    }
    Ok(Zeroizing::new(plaintext))
}

fn record_to_payload(record: &DeploymentRecord) -> RecordPayloadV1 {
    RecordPayloadV1 {
        deployment_identifier: encode(record.deployment_identifier().as_bytes()),
        lifecycle_state: record.state().as_str().to_owned(),
        locator_generation: record
            .locator_generation()
            .map(|generation| encode(generation.as_bytes())),
    }
}

fn payload_to_record(payload: RecordPayloadV1) -> Result<DeploymentRecord, LifecycleError> {
    let deployment_identifier =
        DeploymentIdentifier::from_bytes(decode_array(&payload.deployment_identifier)?)
            .map_err(|_| LifecycleError::IntegrityFailure)?;
    let state = match payload.lifecycle_state.as_str() {
        "uninitialized" => LifecycleState::Uninitialized,
        "initialization_pending" => LifecycleState::InitializationPending,
        "initialized" => LifecycleState::Initialized,
        _ => return Err(LifecycleError::IntegrityFailure),
    };
    let generation = payload
        .locator_generation
        .map(|value| {
            LocatorGeneration::from_bytes(decode_array(&value)?)
                .map_err(|_| LifecycleError::IntegrityFailure)
        })
        .transpose()?;
    DeploymentRecord::new(deployment_identifier, state, generation)
        .map_err(|_| LifecycleError::IntegrityFailure)
}

fn locator_to_payload(locator: &DatabaseLocator) -> LocatorPayloadV1 {
    LocatorPayloadV1 {
        deployment_identifier: encode(locator.deployment_identifier().as_bytes()),
        locator_generation: encode(locator.generation().as_bytes()),
        backend_identifier: locator.backend_identifier().as_str().to_owned(),
        settings: locator
            .settings()
            .iter()
            .map(|field| LocatorSettingV1 {
                field_identifier: field.identifier().as_str().to_owned(),
                value: value_to_payload(field.value()),
            })
            .collect(),
    }
}

fn payload_to_locator(
    payload: LocatorPayloadV1,
    expected_generation: LocatorGeneration,
) -> Result<DatabaseLocator, LifecycleError> {
    if payload.settings.len() > MAX_CONNECTION_FIELDS {
        return Err(LifecycleError::IntegrityFailure);
    }
    let deployment_identifier =
        DeploymentIdentifier::from_bytes(decode_array(&payload.deployment_identifier)?)
            .map_err(|_| LifecycleError::IntegrityFailure)?;
    let generation = LocatorGeneration::from_bytes(decode_array(&payload.locator_generation)?)
        .map_err(|_| LifecycleError::IntegrityFailure)?;
    if generation != expected_generation {
        return Err(LifecycleError::DeploymentMismatch);
    }
    let backend_identifier = BackendIdentifier::new(payload.backend_identifier)
        .map_err(|_| LifecycleError::IntegrityFailure)?;
    let mut settings = Vec::with_capacity(payload.settings.len());
    for setting in payload.settings {
        let identifier = ConnectionFieldIdentifier::new(setting.field_identifier)
            .map_err(|_| LifecycleError::IntegrityFailure)?;
        let value = payload_to_value(setting.value)?;
        if value.exceeds_bound() {
            return Err(LifecycleError::IntegrityFailure);
        }
        settings.push(LocatorConnectionField::new(identifier, value));
    }
    if settings
        .windows(2)
        .any(|pair| pair[0].identifier() >= pair[1].identifier())
    {
        return Err(LifecycleError::IntegrityFailure);
    }
    Ok(DatabaseLocator::from_persisted(
        deployment_identifier,
        generation,
        LocatorConnectionSettings::new(backend_identifier, settings),
    ))
}

fn value_to_payload(value: &ConnectionValue) -> ConnectionValueV1 {
    match value {
        ConnectionValue::String(value) => ConnectionValueV1::String(value.to_string()),
        ConnectionValue::Integer(value) => ConnectionValueV1::Integer(*value),
        ConnectionValue::Boolean(value) => ConnectionValueV1::Boolean(*value),
        ConnectionValue::Bytes(value) => ConnectionValueV1::Bytes(encode(value)),
    }
}

fn payload_to_value(value: ConnectionValueV1) -> Result<ConnectionValue, LifecycleError> {
    let value = match value {
        ConnectionValueV1::String(value) => ConnectionValue::string(value),
        ConnectionValueV1::Integer(value) => ConnectionValue::integer(value),
        ConnectionValueV1::Boolean(value) => ConnectionValue::boolean(value),
        ConnectionValueV1::Bytes(value) => ConnectionValue::bytes(decode(&value)?),
    };
    if value.exceeds_bound() {
        return Err(LifecycleError::IntegrityFailure);
    }
    Ok(value)
}

fn locator_aad(generation: LocatorGeneration) -> Vec<u8> {
    let mut aad = Vec::with_capacity(LOCATOR_AAD_PREFIX.len() + generation.as_bytes().len());
    aad.extend_from_slice(LOCATOR_AAD_PREFIX);
    aad.extend_from_slice(generation.as_bytes());
    aad
}

fn validate_version(version: u32) -> Result<(), LifecycleError> {
    if version == LIFECYCLE_FORMAT_VERSION {
        Ok(())
    } else {
        Err(LifecycleError::UnsupportedVersion)
    }
}

fn serialize_canonical<T: Serialize>(value: &T) -> Result<Vec<u8>, LifecycleError> {
    serde_json::to_vec(value).map_err(|_| LifecycleError::IntegrityFailure)
}

fn parse_canonical<T>(bytes: &[u8], limit: usize) -> Result<T, LifecycleError>
where
    T: DeserializeOwned + Serialize,
{
    if bytes.len() > limit {
        return Err(LifecycleError::IntegrityFailure);
    }
    let value = serde_json::from_slice(bytes).map_err(|_| LifecycleError::IntegrityFailure)?;
    if serialize_canonical(&value)? != bytes {
        return Err(LifecycleError::IntegrityFailure);
    }
    Ok(value)
}

fn encode(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn decode(value: &str) -> Result<Vec<u8>, LifecycleError> {
    let decoded = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| LifecycleError::IntegrityFailure)?;
    if encode(&decoded) != value {
        return Err(LifecycleError::IntegrityFailure);
    }
    Ok(decoded)
}

fn decode_array<const LENGTH: usize>(value: &str) -> Result<[u8; LENGTH], LifecycleError> {
    decode(value)?
        .try_into()
        .map_err(|_| LifecycleError::IntegrityFailure)
}

fn random_nonzero_bytes<const LENGTH: usize>() -> Result<[u8; LENGTH], LifecycleError> {
    random_nonzero_bytes_with(|bytes| {
        getrandom::fill(bytes).map_err(|_| LifecycleError::DependencyUnavailable)
    })
}

fn random_nonzero_bytes_with<const LENGTH: usize>(
    mut fill: impl FnMut(&mut [u8]) -> Result<(), LifecycleError>,
) -> Result<[u8; LENGTH], LifecycleError> {
    loop {
        let mut bytes = [0; LENGTH];
        fill(&mut bytes)?;
        if bytes != [0; LENGTH] {
            return Ok(bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_FILE: &str = "{\"format_version\":1,\"key_algorithm\":\"xchacha20-poly1305\",\"key\":\"AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8\"}";
    const RECORD_FILE: &str = "{\"format_version\":1,\"aead_algorithm\":\"xchacha20-poly1305\",\"nonce\":\"MDEyMzQ1Njc4OTo7PD0-P0BBQkNERUZH\",\"ciphertext\":\"KXtLw6q2Ome3-ygxlSSXJSiRNynqfL5O04B4V2dKH2IZa8LGrcGrA8a2IjClZJx91Wc-oEgHSgpc4cEdRLewapKA2qQ2o7dD4dFNylFv0HwI3Xb1lHgELZudtptc3WHuaYXxh9kUcpHtTkyUR6mG6a9eycbrUwPCskiKwOGlQaeA8RKh0st0bMvGv0V-iodvxMc\"}";
    const LOCATOR_FILE: &str = "{\"format_version\":1,\"aead_algorithm\":\"xchacha20-poly1305\",\"nonce\":\"SElKS0xNTk9QUVJTVFVWV1hZWltcXV5f\",\"ciphertext\":\"21kZgoNEofuzgJGHc6_0lIRa4mrLiIQL-BtJQRlgAOcsbd8-Tm8kRWmNoaYIozjD2ZbFMXi1h4mQH3XJcTb4YQLOirlrDg5EpDRMcNSWd6ap14D7IJBUcs5nKEGyEv_eHjcyYQYGWrgfUNuTULwZcKjuDoo4boTrGlrhH0BpXSuecCxurdY7mAxrqJG0RGu01ROMfIKY5tusH_rj\"}";

    fn sequential_bytes<const LENGTH: usize>(start: u8) -> [u8; LENGTH] {
        std::array::from_fn(|index| start.wrapping_add(index as u8))
    }

    #[test]
    fn version_one_known_answer_vector_matches_exact_bytes() {
        let key = AnchorKey::from_bytes(sequential_bytes(0x00)).unwrap();
        let deployment_identifier =
            DeploymentIdentifier::from_bytes(sequential_bytes(0x10)).unwrap();
        let generation = LocatorGeneration::from_bytes(sequential_bytes(0x20)).unwrap();
        let record = DeploymentRecord::new(
            deployment_identifier,
            LifecycleState::Uninitialized,
            Some(generation),
        )
        .unwrap();
        let locator = DatabaseLocator::from_persisted(
            deployment_identifier,
            generation,
            LocatorConnectionSettings::new(BackendIdentifier::new("sqlite").unwrap(), vec![]),
        );

        assert_eq!(serialize_key(&key).unwrap(), KEY_FILE.as_bytes());
        assert_eq!(
            encrypt_record(&key, &record, sequential_bytes(0x30)).unwrap(),
            RECORD_FILE.as_bytes()
        );
        assert_eq!(
            encrypt_locator(&key, &locator, sequential_bytes(0x48)).unwrap(),
            LOCATOR_FILE.as_bytes()
        );
        assert_eq!(
            parse_key(KEY_FILE.as_bytes()).unwrap().as_bytes(),
            key.as_bytes()
        );
        assert_eq!(
            decrypt_record(&key, RECORD_FILE.as_bytes()).unwrap(),
            record
        );
        assert_eq!(
            decrypt_locator(&key, generation, LOCATOR_FILE.as_bytes()).unwrap(),
            locator
        );
    }

    #[test]
    fn malformed_noncanonical_and_unauthentic_files_fail_closed() {
        let key = AnchorKey::from_bytes(sequential_bytes(0x00)).unwrap();
        let wrong_key = AnchorKey::from_bytes([9; KEY_LENGTH]).unwrap();
        let generation = LocatorGeneration::from_bytes(sequential_bytes(0x20)).unwrap();

        for invalid in [
            format!(" {KEY_FILE}"),
            KEY_FILE.replace("\"key\":", "\"unknown\":0,\"key\":"),
            KEY_FILE.replace(
                "\"format_version\":1,",
                "\"format_version\":1,\"format_version\":1,",
            ),
            KEY_FILE.replace(
                "\"format_version\":1,\"key_algorithm\":\"xchacha20-poly1305\",",
                "\"key_algorithm\":\"xchacha20-poly1305\",\"format_version\":1,",
            ),
            KEY_FILE.replace(",\"key\":\"", ",\"missing\":\""),
            KEY_FILE.replace("AAECAw", "=AECAw"),
            format!("{KEY_FILE}x"),
        ] {
            assert_eq!(
                parse_key(invalid.as_bytes()).unwrap_err(),
                LifecycleError::IntegrityFailure
            );
        }
        assert_eq!(
            parse_key(
                KEY_FILE
                    .replace("\"format_version\":1", "\"format_version\":2")
                    .as_bytes()
            )
            .unwrap_err(),
            LifecycleError::UnsupportedVersion
        );
        assert_eq!(
            parse_key(&[0xff, 0xfe]).unwrap_err(),
            LifecycleError::IntegrityFailure
        );
        assert_eq!(
            decrypt_record(&wrong_key, RECORD_FILE.as_bytes()).unwrap_err(),
            LifecycleError::IntegrityFailure
        );
        let wrong_generation = LocatorGeneration::from_bytes([9; 16]).unwrap();
        assert_eq!(
            decrypt_locator(&key, wrong_generation, LOCATOR_FILE.as_bytes()).unwrap_err(),
            LifecycleError::IntegrityFailure
        );
        let wrong_nonce = LOCATOR_FILE.replace(
            "SElKS0xNTk9QUVJTVFVWV1hZWltcXV5f",
            "MDEyMzQ1Njc4OTo7PD0-P0BBQkNERUZH",
        );
        assert_eq!(
            decrypt_locator(&key, generation, wrong_nonce.as_bytes()).unwrap_err(),
            LifecycleError::IntegrityFailure
        );
        let mut tampered = LOCATOR_FILE.as_bytes().to_vec();
        let last = tampered.len() - 3;
        tampered[last] = if tampered[last] == b'A' { b'B' } else { b'A' };
        assert_eq!(
            decrypt_locator(&key, generation, &tampered).unwrap_err(),
            LifecycleError::IntegrityFailure
        );
    }

    #[test]
    fn randomness_failure_has_no_fallback() {
        assert_eq!(
            random_nonzero_bytes_with::<16>(|_| Err(LifecycleError::DependencyUnavailable))
                .unwrap_err(),
            LifecycleError::DependencyUnavailable
        );

        let mut calls = 0;
        let bytes = random_nonzero_bytes_with::<16>(|output| {
            calls += 1;
            if calls == 2 {
                output[0] = 1;
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(calls, 2);
        assert_eq!(bytes[0], 1);
    }
}
