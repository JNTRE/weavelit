# Server Restore Crate Agent Guide

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This crate owns the fixed outer backup envelope, canonical recovery-key
  syntax, the age recipient-profile policy, authenticated decryption, transfer
  bounds and deadlines, the single-operation Restore permit, backup content
  parsing, compatibility checking, reference and component resolution, and
  redacted Restore errors.
- It receives a validated lifecycle authority and the selected Application
  Database binding as inbound values through `RestoreAuthority`; it does not
  reach into the lifecycle crate's internals.
- It does not own startup classification, workflow arbitration, checkpoint
  creation, atomic state replacement, System Log acknowledgement, lifecycle
  sealing, in-process activation, transport, or client presentation.

## Asset Inventory

- `Cargo.toml`: Package metadata and approved cryptographic, authentication, and Argon2 dependencies.
- `src/`: Transfer bounds, the Restore permit, envelope and recovery-key parsing, authenticated decryption, content normalization, redacted errors, and the validation entry point. `src/state.rs` re-seals recovered secrets and assembles state; `src/ticket.rs` mints the single-use ticket; `src/vectors.rs` is test-only vendored-vector coverage.
- `examples/`: Development-only fixture generator; it is never linked into the
  Server binary.
- `tests/`: Bounds, envelope, recovery-key, content, age policy, multi-chunk STREAM, fixture reproducibility, secret re-sealing, end-to-end validation, fixture-credential authentication, and the shared fixture generator and harness.
- `tests/fixtures/`: Immutable committed backup fixtures, their canonical
  recovery keys, the expected decrypted plaintext for each valid fixture, and
  the `fixtures.json` manifest pinning every fixture's byte length and SHA-256
  digest. Two valid fixtures are committed: `valid.wlitbackup`, which names a
  fuller component inventory than any build in this repository compiles in, and
  `valid-web-ui-sqlite.wlitbackup`, which names exactly the `web-ui` Client
  Module and the `sqlite` Log Module the Server binary compiles in.
- `tests/vectors/`: Vendored C2SP CCTV age test vectors, their `README.md`
  provenance and license record, and the `vectors.json` manifest pinning every
  vendored file's byte length and SHA-256 digest.

## Working Rules

- MUST follow [Contribution Guidelines](../../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this guide, then each parent `AGENTS.md` through the
  repository root.
- MUST read the Server Restore, Lifecycle, Architecture, and Application Database Designs, Security Model, and Testing and Validation Policy.
- MUST preserve the committed fixture bytes and the `fixtures.json` manifest; a
  fixture change requires regenerating with
  `cargo run --example generate-restore-fixtures -p weavelit-server-restore`
  and an explicit format decision recorded in the Server Restore Design.
- MUST keep `tests/support/mod.rs`'s `FIXTURE_TOTP_SECRET` the exact length the TOTP
  Module declares. Content validation refuses factor data a named MFA Module
  could not open, so a shorter placeholder would make the canonical valid
  fixture invalid.
- Agents MUST NOT edit a vendored vector under `tests/vectors/`. Refresh the whole set
  from the pinned upstream commit instead, and update `tests/vectors/README.md`,
  `tests/vectors/vectors.json`, and the pinned expectation table in
  `src/vectors.rs` together.
- MUST run the package tests during development and `make -C server check` before
  handoff.

- MUST update this inventory whenever crate assets are added, removed, renamed, or
  moved.
- MUST keep validation in the Server Restore Design's fixed order and reject before
  reading sensitive input; never mutate deployment state from this crate.
- MUST keep every recovery key, unwrapped data key, and decrypted plaintext in
  bounded transient memory under maintained zeroization, and never write backup
  material to disk or logs.
- Agents MUST NOT add custom cryptographic primitives to production code. This crate
  implements the age v1 X25519 recipient profile itself, but composes it only
  from approved maintained primitives (`x25519-dalek`, `hkdf`, `hmac`, `sha2`,
  `chacha20poly1305`, `bech32`); never hand-roll a construction one of those
  provides. The test-only fixture generator is a deliberately independent
  second implementation of the same profile, used solely to pin known-answer
  vectors.
- MUST accept exactly one X25519 recipient stanza. Reject `scrypt`, any other stanza
  type, an absent stanza, an additional stanza, and an unsupported version line
  as `backup_incompatible` before key agreement.
- MUST keep every `backup_invalid` cause mutually indistinguishable in public
  presentation, and keep public errors payload-free with redacted diagnostic
  formatting.
- MUST reject unknown, duplicate, missing, wrongly typed, non-canonically encoded, or
  oversized fields before constructing restored state.
- MUST reject every password verifier the backup carries that falls outside the
  closed Argon2 profile allowlist `weavelit-server-authentication` owns, before
  constructing restored state. Resolve it through `PasswordPolicy::approved` and
  that crate's PHC reader; never restate the allowlist or parse PHC here.
- MUST keep that rejection to supplied entries only. Never require a verifier to
  exist for an account, and never add an administrator-topology or
  verifier-presence check: an absent verifier is a modeled credential state, and
  the Technical Specification's Multifactor Authentication section accepts a
  fail-closed deployment and forbids Restore from claiming to guarantee renewed
  administrative access. `tests/content.rs` pins that acceptance; changing it
  requires changing the specification first.
- MUST enforce the approved transfer bounds, deadlines, and single-operation permit
  in the crate rather than relying on callers.
