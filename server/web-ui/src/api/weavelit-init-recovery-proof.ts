/**
 * Recovery-key proof of possession for the Web UI Client Module Init workflow.
 *
 * The proof is an `HMAC-SHA-256` over the raw delivery nonce, keyed by the raw
 * secret the delivered age identity encodes. Derivation uses the browser's
 * native `crypto.subtle` only. Weavelit deliberately carries no JavaScript
 * cryptography dependency, so the Bech32 decoding the age identity requires is
 * implemented here rather than taken from a package.
 *
 * The contract is owned by
 * `docs/client-modules/web-ui/pre-operational-init-design.md`.
 */

/** The exact length of an accepted proof: 32 bytes as unpadded URL-safe Base64. */
export const RECOVERY_PROOF_CHARACTERS = 43;

/** The human-readable part every recovery key carries, in its decoded case. */
const IDENTITY_HRP = "age-secret-key-";

/** The closed shape of one canonical uppercase age identity line. */
const IDENTITY_PATTERN = /^AGE-SECRET-KEY-1[0-9A-Z]+$/;

/** The closed shape of unpadded URL-safe Base64. */
const BASE64URL_PATTERN = /^[A-Za-z0-9_-]+$/;

/** The Bech32 data alphabet, indexed by five-bit value. */
const BECH32_CHARSET = "qpzry9x8gf2tvdw0s3jn54khce6mua7l";

/** The Bech32 checksum generator constants. */
const BECH32_GENERATOR = [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3];

/** The residue a valid Bech32 checksum leaves. Bech32m would leave another. */
const BECH32_RESIDUE = 1;

/** The number of trailing data characters that carry the checksum. */
const BECH32_CHECKSUM_CHARACTERS = 6;

/** The exact secret size an age identity encodes. */
const RECOVERY_KEY_BYTES = 32;

/**
 * A proof could not be derived from the delivered key and nonce.
 *
 * The message is fixed and carries no key material, no nonce, and no
 * cryptographic diagnostic.
 */
export class RecoveryProofUnavailableError extends Error {
  constructor() {
    super("recovery_proof_unavailable");
    this.name = "RecoveryProofUnavailableError";
  }
}

function unavailable(): RecoveryProofUnavailableError {
  return new RecoveryProofUnavailableError();
}

function bech32Polymod(values: readonly number[]): number {
  let checksum = 1;
  for (const value of values) {
    const top = checksum >>> 25;
    checksum = ((checksum & 0x1ffffff) << 5) ^ value;
    for (const [bit, generator] of BECH32_GENERATOR.entries()) {
      if (((top >>> bit) & 1) === 1) {
        checksum ^= generator;
      }
    }
  }
  return checksum >>> 0;
}

function expandHumanReadablePart(part: string): number[] {
  const high: number[] = [];
  const low: number[] = [];
  for (const character of part) {
    const code = character.charCodeAt(0);
    high.push(code >>> 5);
    low.push(code & 31);
  }
  return [...high, 0, ...low];
}

/**
 * Regroups five-bit Bech32 data into bytes, rejecting non-canonical padding.
 *
 * Leftover bits must number fewer than five and must all be zero, so an
 * encoding that is not the canonical one for its payload is refused rather than
 * silently truncated to a different secret.
 */
function fiveBitToBytes(values: readonly number[]): Uint8Array<ArrayBuffer> {
  const bytes: number[] = [];
  let accumulator = 0;
  let bits = 0;
  for (const value of values) {
    accumulator = (accumulator << 5) | value;
    bits += 5;
    while (bits >= 8) {
      bits -= 8;
      bytes.push((accumulator >>> bits) & 0xff);
    }
  }
  if (bits >= 5 || ((accumulator << (8 - bits)) & 0xff) !== 0) {
    throw unavailable();
  }
  return Uint8Array.from(bytes);
}

/**
 * Decodes the raw secret one canonical recovery key encodes.
 *
 * The human-readable part, the character set, the Bech32 checksum, the padding,
 * and the decoded size are all verified, so a mistyped or corrupted key fails
 * here rather than producing a proof the Server will reject.
 */
export function decodeRecoveryKeySecret(recoveryKey: string): Uint8Array<ArrayBuffer> {
  if (!IDENTITY_PATTERN.test(recoveryKey)) {
    throw unavailable();
  }
  const encoded = recoveryKey.toLowerCase().slice(IDENTITY_HRP.length + 1);
  const data: number[] = [];
  for (const character of encoded) {
    const value = BECH32_CHARSET.indexOf(character);
    if (value < 0) {
      throw unavailable();
    }
    data.push(value);
  }
  if (data.length <= BECH32_CHECKSUM_CHARACTERS) {
    throw unavailable();
  }
  if (bech32Polymod([...expandHumanReadablePart(IDENTITY_HRP), ...data]) !== BECH32_RESIDUE) {
    throw unavailable();
  }
  const secret = fiveBitToBytes(data.slice(0, -BECH32_CHECKSUM_CHARACTERS));
  if (secret.length !== RECOVERY_KEY_BYTES) {
    throw unavailable();
  }
  return secret;
}

function decodeBase64Url(value: string): Uint8Array<ArrayBuffer> {
  if (!BASE64URL_PATTERN.test(value)) {
    throw unavailable();
  }
  const standard = value.replaceAll("-", "+").replaceAll("_", "/");
  const padding = "=".repeat((4 - (standard.length % 4)) % 4);
  let binary: string;
  try {
    binary = globalThis.atob(standard + padding);
  } catch {
    throw unavailable();
  }
  return Uint8Array.from(binary, (character) => character.charCodeAt(0));
}

function encodeBase64Url(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }
  return globalThis.btoa(binary).replaceAll("+", "-").replaceAll("/", "_").replaceAll("=", "");
}

/**
 * Derives the proof of possession for one delivered recovery key and nonce.
 *
 * The decoded secret is overwritten once the key material has been imported, so
 * the raw secret does not outlive the single derivation that needs it. Neither
 * the key nor the derived proof is stored, logged, or returned to any caller
 * other than the one submission that consumes it.
 *
 * Rejects with {@link RecoveryProofUnavailableError} when the delivered values
 * are not the documented shapes or the browser cannot perform the derivation.
 */
export async function deriveRecoveryKeyProof(
  recoveryKey: string,
  deliveryNonce: string,
): Promise<string> {
  const secret = decodeRecoveryKeySecret(recoveryKey);
  const nonce = decodeBase64Url(deliveryNonce);

  let signature: ArrayBuffer;
  try {
    const key = await globalThis.crypto.subtle.importKey(
      "raw",
      secret,
      { name: "HMAC", hash: "SHA-256" },
      false,
      ["sign"],
    );
    secret.fill(0);
    signature = await globalThis.crypto.subtle.sign("HMAC", key, nonce);
  } catch {
    secret.fill(0);
    throw unavailable();
  }

  const proof = encodeBase64Url(new Uint8Array(signature));
  if (proof.length !== RECOVERY_PROOF_CHARACTERS) {
    throw unavailable();
  }
  return proof;
}
