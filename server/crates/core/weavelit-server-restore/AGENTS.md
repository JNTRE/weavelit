# Server Restore Crate Agent Guide

This crate defines the Server-owned validation of an encrypted Weavelit backup:
outer envelope parsing, age v1 decryption, compatibility checking, and restored
state normalization performed before any deployment state is replaced.

## Purpose and Scope

- This crate owns the fixed outer backup envelope, canonical recovery-key
  syntax, the age recipient-profile policy, authenticated decryption, transfer
  bounds and deadlines, the single-operation Restore permit, backup content
  parsing, compatibility checking, reference and component resolution, and
  redacted Restore errors.
- It reuses the Application Database contract state types, the deployment
  identifier, and lifecycle backend identifiers and errors.
- It receives a validated lifecycle authority and the selected Application
  Database binding as inbound values through `RestoreAuthority`; it does not
  reach into the lifecycle crate's internals.
- It does not own startup classification, workflow arbitration, checkpoint
  creation, atomic state replacement, System Log acknowledgement, lifecycle
  sealing, in-process activation, transport, or client presentation.

## Asset Inventory

- `AGENTS.md`: Local Restore validation contract and fixture rules.
- `Cargo.toml`: Package metadata and the approved cryptographic dependencies
  that compose the in-house age v1 X25519 reader, with their excluded feature
  surface.
- `src/`: Transfer bounds and the Restore permit, outer envelope parsing,
  canonical recovery-key handling, authenticated decryption, backup content
  normalization, redacted errors, and the validation entry point. `src/state.rs`
  re-seals every recovered secret under the replacement deployment's at-rest key
  and assembles the replacement application state. `src/vectors.rs`
  is compiled only under `cfg(test)` and runs the reader against the vendored
  external age vectors.
- `examples/`: Development-only fixture generator; it is never linked into the
  Server binary.
- `tests/`: Bounds, envelope, recovery-key, content, age parameter policy,
  multi-chunk STREAM, fixture-reproducibility, secret re-sealing, and end-to-end
  validation tests, plus the shared deterministic fixture generator and harness
  in `tests/support/`.
- `tests/fixtures/`: Immutable committed backup fixtures, their canonical
  recovery keys, the expected decrypted plaintext, and the `fixtures.json`
  manifest pinning every fixture's byte length and SHA-256 digest.
- `tests/vectors/`: Vendored C2SP CCTV age test vectors, their `README.md`
  provenance and license record, and the `vectors.json` manifest pinning every
  vendored file's byte length and SHA-256 digest.

## Usage Guidance

- Before editing, read this guide, then each parent `AGENTS.md` through the
  repository root.
- Read the Server Restore Design, Server Lifecycle Design, Server Architecture
  Design, Application Database Design, Security Model, and Testing and
  Validation Policy.
- Preserve the committed fixture bytes and the `fixtures.json` manifest; a
  fixture change requires regenerating with
  `cargo run --example generate-restore-fixtures -p weavelit-server-restore`
  and an explicit format decision recorded in the Server Restore Design.
- Never edit a vendored vector under `tests/vectors/`. Refresh the whole set
  from the pinned upstream commit instead, and update `tests/vectors/README.md`,
  `tests/vectors/vectors.json`, and the pinned expectation table in
  `src/vectors.rs` together.
- Run the package tests during development and `make -C server check` before
  handoff.

## Standards and Conventions

- Update this inventory whenever crate assets are added, removed, renamed, or
  moved.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Keep validation in the Server Restore Design's fixed order and reject before
  reading sensitive input; never mutate deployment state from this crate.
- Keep every recovery key, unwrapped data key, and decrypted plaintext in
  bounded transient memory under maintained zeroization, and never write backup
  material to disk or logs.
- Never add custom cryptographic primitives to production code. This crate
  implements the age v1 X25519 recipient profile itself, but composes it only
  from approved maintained primitives (`x25519-dalek`, `hkdf`, `hmac`, `sha2`,
  `chacha20poly1305`, `bech32`); never hand-roll a construction one of those
  provides. The test-only fixture generator is a deliberately independent
  second implementation of the same profile, used solely to pin known-answer
  vectors.
- Accept exactly one X25519 recipient stanza. Reject `scrypt`, any other stanza
  type, an absent stanza, an additional stanza, and an unsupported version line
  as `backup_incompatible` before key agreement.
- Keep every `backup_invalid` cause mutually indistinguishable in public
  presentation, and keep public errors payload-free with redacted diagnostic
  formatting.
- Reject unknown, duplicate, missing, wrongly typed, non-canonically encoded, or
  oversized fields before constructing restored state.
- Enforce the approved transfer bounds, deadlines, and single-operation permit
  in the crate rather than relying on callers.
