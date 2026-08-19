# Server Authentication Crate Agent Guide

This crate owns the Server's local password authentication core: the approved
Argon2id profile, the closed allowlist of profiles a stored verifier may be
attempted against, the equal-work password decision, preparation of temporary
password credentials, and generation and hashing of session and CSRF bearer
values.

## Purpose and Scope

- This crate owns the current Argon2id profile, the closed accepted-profile
  allowlist and its verification-memory ceiling, PHC parsing and profile
  matching, the Argon2 execution seam, the decoy-backed equal-work denial,
  rehash-on-profile-drift, redacted authentication errors, session and CSRF
  token generation, encoding, digesting, and constant-time digest comparison,
  preparation of a non-recoverable temporary password and approved verifier,
  and the opaque single-use continuation ticket that binds a verified password
  to a later second-factor or enrollment step.
- It does not own account lookup, persistence, session lifetime, cookies, route
  contracts, transport, MFA method behavior, or client presentation. A caller
  supplies the stored credential as an inbound value and persists what this
  crate returns.
- It takes no workspace path dependency, so it cannot reach the transport, the
  listener, the Application Database, or a Client Module.
- It has no child paths.

## Asset Inventory

- `AGENTS.md`: Local routing, inventory, and authentication-boundary rules.
- `Cargo.toml`: Package metadata and the approved password-hashing, encoding,
  digest, randomness, constant-time, and zeroization dependencies with their
  excluded feature surface.
- `src/lib.rs`: Crate boundary and public surface.
- `src/profile.rs`: The current Argon2id profile, the closed allowlist, the
  verification-memory ceiling, PHC profile matching, and the validated policy.
- `src/phc.rs`: PHC encoding of a salt and output at a known profile.
- `src/engine.rs`: The `Argon2Engine` seam and its RustCrypto implementation.
- `src/password.rs`: The equal-work password decision and rehash-on-drift.
- `src/session.rs`: Session and CSRF tokens, their digests, and redaction.
- `src/temporary_password.rs`: Temporary-password generation, approved verifier
  preparation, one-response disclosure ownership, and the fixed lifetime.
- `src/continuation.rs`: The opaque, single-use, short-lived continuation
  ticket and its stored digest.
- `src/error.rs`: Payload-free authentication errors.
- `src/random.rs`: Operating-system randomness with no fallback.

## Usage Guidance

- Before editing, read this guide, then each parent `AGENTS.md` through the
  repository root.
- Read the Server Authentication Design, Security Model, Server Architecture
  Design, and Testing and Validation Policy before changing behavior here.
- Treat `ACCEPTED_ARGON2_PROFILES` as policy: adding an entry accepts stored
  verifiers at that profile forever, and removing one immediately refuses them.
  Every entry must stay within `MAX_VERIFICATION_MEMORY_KIB`, and the same
  change must update the Server Authentication Design.
- Never verify a password against a stored profile that the allowlist did not
  resolve; the hashing library takes its cost parameters from the encoded value
  it is given.
- Prove equal-work denial by counting operations through an injected
  `Argon2Engine`, never by comparing elapsed time.
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
