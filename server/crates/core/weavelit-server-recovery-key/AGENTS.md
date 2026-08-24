# Server Recovery Key Crate Agent Guide

This crate defines the canonical age recovery key every Weavelit Server workflow
shares: the one accepted key syntax, the redacted secret types that carry a
private identity, the X25519 key agreement a backup reader needs, and the Init
delivery nonce and HMAC-SHA-256 proof of possession.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This crate owns canonical age Bech32 parsing and encoding of a lowercase
  `age1...` public recipient and an uppercase `AGE-SECRET-KEY-1...` private
  identity accepted as exactly one canonical line, the non-cloneable clearing
  private identity, the public recipient, key-pair generation, the age X25519
  key agreement, the unique delivery nonce, the expected HMAC-SHA-256 proof
  value, and its constant-time comparison.
- It exists so Init and Restore share one representation and one accepted
  spelling without depending on each other.
- It takes no workspace path dependency, so the transport, the Application
  Database, the lifecycle crate, and every Client Module are outside its reach.
- It does not own checkpoint persistence, request validation, backup envelope
  or payload handling, workflow orchestration, error presentation, or the
  response that delivers a private key.

## Asset Inventory

- `Cargo.toml`: Package metadata and the approved cryptographic dependencies the
  canonical encoding, key agreement, randomness, proof, and constant-time
  comparison are composed from, with their excluded feature surface.
- `src/lib.rs`: Crate boundary and public surface.
- `src/error.rs`: The redacted rejected-submission and preparation failures.
- `src/key.rs`: Canonical age Bech32 parsing and encoding, the private identity
  and public recipient, key generation, and the age X25519 key agreement.
- `src/proof.rs`: The delivery nonce, the HMAC-SHA-256 proof value, and its
  constant-time comparison.
- `src/delivery.rs`: The one-time preparation that fixes the required order of
  key generation, nonce generation, and proof computation.
- `tests/`: Canonical encoding round-trip and rejection tests (`tests/key.rs`)
  and delivery-nonce, proof, and redaction tests (`tests/proof.rs`).

## Working Rules

- MUST follow [Contribution Guidelines](../../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this guide, then each parent `AGENTS.md` through the
  repository root.
- MUST read the Server Init Design's `Recovery-Key Delivery And Finalization`
  section, the Server Restore Design, the Server Architecture Design, and the
  Security Model.
- Agents MUST NOT change the accepted key syntax. Restore's committed backup fixtures and
  the vendored C2SP CCTV age vectors are validated through this crate; a syntax
  change silently changes what an existing backup key means.
- MUST compute the expected proof over the delivery nonce alone, keyed by the private
  key's raw bytes. A client that retained the delivered key reproduces the value
  from the nonce, so any additional input to the message breaks finalization.
- MUST run the package tests during development and `make -C server check` before
  handoff.

- MUST update this inventory whenever crate assets are added, removed, renamed, or
  moved.
- Agents MUST NOT expose the private key's raw bytes outside this crate. Only the public
  recipient, the delivery nonce, and the expected proof value leave it as
  recordable values.
- MUST keep the private identity non-cloneable, redacted in `Debug`, and cleared on
  drop, and keep every delivered line and derived secret in a zeroizing buffer.
- Agents MUST NOT add a custom cryptographic primitive. Compose only approved maintained
  primitives (`x25519-dalek`, `hmac`, `sha2`, `bech32`, `subtle`, `getrandom`),
  and compare a proof only through `subtle`'s constant-time equality.
- MUST keep randomness free of a deterministic fallback: a randomness failure stops
  the operation rather than producing a predictable key or nonce.
- MUST keep every rejection payload-free and uniform in its display representation,
  and never include rejected text in an error.
- MUST keep every dependency exactly pinned with `default-features = false` and the
  minimum feature set the crate requires.
- MUST forbid `unsafe` code.
