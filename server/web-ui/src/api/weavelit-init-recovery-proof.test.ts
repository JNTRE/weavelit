import { describe, expect, it, vi } from "vitest";

import {
  RECOVERY_PROOF_CHARACTERS,
  RecoveryProofUnavailableError,
  decodeRecoveryKeySecret,
  deriveRecoveryKeyProof,
} from "./weavelit-init-recovery-proof";

/**
 * A canonical age identity encoding the bytes `0x00..0x1f`, its matching
 * delivery nonce encoding the bytes `0x20..0x3f`, and the proof both produce.
 *
 * The vector was produced by an independent reference implementation, and the
 * same reference decoded the Server's own committed identity fixture and
 * re-encoded it byte-for-byte, so the Bech32 form agreed with the Server's
 * encoder before this vector was fixed here. The secret is a synthetic counting
 * sequence and is not key material of any deployment.
 */
const RECOVERY_KEY = "AGE-SECRET-KEY-1QQQSYQCYQ5RQWZQFPG9SCRGWPUGPZYSNZS23V9CCRYDPK8QARC0SWRYDWG";
const DELIVERY_NONCE = "ICEiIyQlJicoKSorLC0uLzAxMjM0NTY3ODk6Ozw9Pj8";
const EXPECTED_PROOF = "YiFd573c6n4sQEf_a7lPjRgmL8iz82SBNLt9RBWP-E0";

/** The same identity with a single data character changed, breaking the checksum. */
const CORRUPTED_KEY = "AGE-SECRET-KEY-1QQQSYQCYQ5RQWZQFPG9SCRGWPUGPZYSNZS23V9CCRYDPK8QARC0SWRYDWH";

function hex(bytes: Uint8Array): string {
  return [...bytes].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

describe("decodeRecoveryKeySecret", () => {
  it("decodes the exact secret a canonical recovery key encodes", () => {
    const secret = decodeRecoveryKeySecret(RECOVERY_KEY);

    expect(secret).toHaveLength(32);
    expect(hex(secret)).toBe("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f");
  });

  it.each([
    ["an empty value", ""],
    ["a lowercase key", RECOVERY_KEY.toLowerCase()],
    [
      "a mixed-case key",
      "AGE-SECRET-KEY-1qqqsyqcyq5rqwzqfpg9scrgwpugpzysnzs23v9ccrydpk8qarc0swrydwg",
    ],
    ["another human-readable part", RECOVERY_KEY.replace("AGE-SECRET-KEY-", "AGE-PUBLIC-KEY-")],
    ["a missing separator", RECOVERY_KEY.replace("KEY-1", "KEY-")],
    ["a broken checksum", CORRUPTED_KEY],
    ["a truncated key", RECOVERY_KEY.slice(0, RECOVERY_KEY.length - 4)],
    ["a key carrying a character outside the alphabet", RECOVERY_KEY.replace("Q", "B")],
    ["a key carrying whitespace", `${RECOVERY_KEY} `],
    ["a key carrying a trailing newline", `${RECOVERY_KEY}\n`],
    ["a data-only value", "QQQSYQCYQ5RQWZQ"],
  ])("refuses %s", (_label, value) => {
    expect(() => decodeRecoveryKeySecret(value)).toThrow(RecoveryProofUnavailableError);
  });
});

describe("deriveRecoveryKeyProof", () => {
  it("derives the reference proof for the reference key and nonce", async () => {
    await expect(deriveRecoveryKeyProof(RECOVERY_KEY, DELIVERY_NONCE)).resolves.toBe(
      EXPECTED_PROOF,
    );
  });

  it("derives a proof of exactly the accepted length", async () => {
    const proof = await deriveRecoveryKeyProof(RECOVERY_KEY, DELIVERY_NONCE);

    expect(proof).toHaveLength(RECOVERY_PROOF_CHARACTERS);
    expect(proof).toMatch(/^[A-Za-z0-9_-]+$/);
  });

  it("binds the proof to the nonce", async () => {
    const other = `${DELIVERY_NONCE.slice(0, DELIVERY_NONCE.length - 1)}A`;

    await expect(deriveRecoveryKeyProof(RECOVERY_KEY, other)).resolves.not.toBe(EXPECTED_PROOF);
  });

  it.each([
    ["a padded nonce", `${DELIVERY_NONCE}==`],
    ["a standard-alphabet nonce", "ICEiIyQlJicoKSorLC0uLzAxMjM0NTY3ODk6Ozw9Pj/+"],
    ["an empty nonce", ""],
    ["a nonce carrying whitespace", `${DELIVERY_NONCE} `],
  ])("refuses %s", async (_label, nonce) => {
    await expect(deriveRecoveryKeyProof(RECOVERY_KEY, nonce)).rejects.toBeInstanceOf(
      RecoveryProofUnavailableError,
    );
  });

  it("refuses a key whose checksum does not verify", async () => {
    await expect(deriveRecoveryKeyProof(CORRUPTED_KEY, DELIVERY_NONCE)).rejects.toBeInstanceOf(
      RecoveryProofUnavailableError,
    );
  });

  it("reports an unavailable derivation without disclosing the key", async () => {
    vi.spyOn(globalThis.crypto.subtle, "sign").mockRejectedValue(new Error("unsupported"));

    const failure = await deriveRecoveryKeyProof(RECOVERY_KEY, DELIVERY_NONCE).catch(
      (reason: unknown) => reason,
    );

    expect(failure).toBeInstanceOf(RecoveryProofUnavailableError);
    expect(JSON.stringify(failure instanceof Error ? failure.message : failure)).not.toContain(
      "AGE-SECRET-KEY",
    );
  });

  it("uses the browser's native subtle crypto rather than a bundled implementation", async () => {
    const importKey = vi.spyOn(globalThis.crypto.subtle, "importKey");
    const sign = vi.spyOn(globalThis.crypto.subtle, "sign");

    await deriveRecoveryKeyProof(RECOVERY_KEY, DELIVERY_NONCE);

    expect(importKey).toHaveBeenCalledTimes(1);
    expect(importKey.mock.calls[0]?.[2]).toStrictEqual({ name: "HMAC", hash: "SHA-256" });
    // The key is imported as non-extractable and for signing alone, so the
    // derivation cannot read the secret back out of the imported key.
    expect(importKey.mock.calls[0]?.[3]).toBe(false);
    expect(importKey.mock.calls[0]?.[4]).toStrictEqual(["sign"]);
    expect(sign).toHaveBeenCalledTimes(1);
    expect(sign.mock.calls[0]?.[0]).toBe("HMAC");
  });
});
