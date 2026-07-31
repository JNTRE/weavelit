# Weavelit Testing and Validation Policy

This policy defines the evidence required to treat an implementation change as
ready to merge or deploy. Passing tests provides high confidence, not proof,
that a deployment will succeed; production-like packaging and deployment
verification provide the additional evidence that tests alone cannot.

## Principles

- Tests protect stated behavior and failure boundaries, not implementation
  details. A test should fail when a user-visible, operational, or security
  commitment regresses.
- Every behavior change includes automated tests in the same change. A change
  that cannot reasonably be automated must document why and provide a repeatable
  manual verification procedure in its owning documentation.
- Tests are deterministic, isolated, and safe to run repeatedly. They do not
  depend on production services, live provider credentials, real user data, or
  network timing.
- Negative and failure cases receive equal attention to the successful path.
  For security-relevant behavior, tests show that invalid, unauthorized,
  disabled, expired, replayed, malformed, and dependency-failure conditions
  fail safely.
- A test failure is addressed by correcting the behavior or updating an
  intentionally changed requirement and its tests together. Tests are not
  deleted, weakened, skipped, or made flaky merely to obtain a passing result.

## Required Test Design

Before implementation begins, the author records the behavior being added or
changed, its observable success result, its rejection or failure results, and
the smallest useful test layer. The feature's canonical documentation is the
source for this record when it exists.

Each change must include the tests appropriate to its risk and boundary:

| Change or boundary | Required evidence |
| --- | --- |
| Pure domain, validation, state, parsing, or transformation logic | Unit tests covering normal, boundary, and invalid inputs. |
| Database, filesystem, configuration, serialization, or process behavior | Integration tests using isolated temporary resources and real adapters. |
| Versioned API, **[Client Module](glossary.md#applications-and-interfaces)**, or **[Service Module](glossary.md#applications-and-interfaces)** contract | Contract tests for accepted requests and stable success and error responses. |
| Authentication, authorization, secret handling, audit logging, MFA, or destructive operations | Tests for every allowed and denied path, plus tests that sensitive values are absent from returned errors and logs. |
| Server-owned lifecycle and pre-operational database-selection contract | Direct tests for every startup classification, deployment-record creation and irreversible sealing, deployment-identifier matching, database selection and locator persistence, secret-reference rejection, workflow exclusivity, mutation serialization, every cross-store crash point, direct invocation after sealing, and fail-closed missing, malformed, mismatched, unavailable, or integrity-failing deployment state; contract and process tests for route gating, seal reconciliation, and transitions to normal operation. |
| Server-owned **[Init](glossary.md#states-and-requests)** use case and Init-capable **[Client Module](glossary.md#applications-and-interfaces)** surface | Direct workflow tests for normalized request validation, validation before later secret submission, recovery-key generation, one-time delivery, proof and reset, Init-checkpoint handling, atomic fresh-state creation, retry, concurrency, redaction, and rejection before secret reading or side effects; contract and process tests for lifecycle composition and the transition to normal operation. |
| Server-owned **[Restore](glossary.md#states-and-requests)** use case and Restore-capable Client Module surface | Direct workflow tests for every artifact and resource bound, malformed, unauthentic, integrity-failing, incompatible, and semantically invalid backups, wrong recovery keys, checkpoint handling, session invalidation, recovery-public-key preservation, protected-secret re-encryption, private-key and plaintext non-persistence, atomic rollback, retry and reset, durable Restore-result Audit Log recording with the **[System Principal](glossary.md#identities-and-access)** and without requester or recovery-key identity attribution, every Restore-specific crash point, concurrency with Init and Restore, redaction, and rejection before key or artifact processing; contract, process, and Web UI end-to-end tests for transfer, lifecycle gating, post-commit reconciliation, and normal sign-in after Restore. |
| Provider integration | Tests against controlled fakes or recorded fixtures for request construction, error mapping, retry, rate-limit, and duplicate-protection behavior. Live-provider checks are separately controlled smoke tests, never the default test suite. |
| Web UI, Weavelit CLI, packaging, or deployment workflow | Focused end-to-end or smoke tests of the user workflow and the failure condition most likely to cause an unusable release. |

Use table-driven tests for meaningful input combinations and property tests for
invariants with broad input spaces. Prefer assertions on public results,
persisted state, emitted audit events, and provider requests over assertions on
private functions or internal call order. Each defect fix includes a regression
test that fails before the fix and passes after it.

## Rust Quality Gates

All Rust code uses the repository's Rust 1.97 stable toolchain. Once the Cargo
workspace is introduced, it must commit a `rust-toolchain.toml` that pins the
toolchain and required components. Run the complete Server Rust quality-gate
suite locally and in CI with:

```sh
make -C server check
```

The Server `Makefile` runs the following required commands without warnings or
failures. Add a required default Rust quality gate there so local and CI
validation remain identical:

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --all-targets
cargo build --locked --workspace --release
```

Crate-local tests are included through their workspace member's Cargo targets.
A dedicated Server test crate must be a workspace member so `make -C server
check` runs it automatically.

`cargo fmt` and Clippy are quality gates, not substitutes for tests. The
repository's shared VS Code settings run rustfmt on Rust-file saves and cause
rust-analyzer to run Clippy diagnostics during editing. Contributors still run
the relevant commands before requesting review because editor feedback is not
CI evidence.

CI must run these gates on every pull request and protected branch. It must
also run affected integration, contract, and end-to-end suites. The first Rust
workspace change must add a CI workflow implementing these requirements; a
later change may add coverage reporting after its report format, exclusions,
and ratchet policy are documented. A global line-coverage percentage is not a
merge criterion: coverage is useful as a trend and gap signal but cannot prove
that the security and failure behavior above was exercised.

## Deployment Confidence

Before a release or deployment, CI builds the exact release artifact from a
clean checkout and verifies it in a production-like environment. For the
**[Weavelit Server](glossary.md#applications-and-interfaces)**, this includes
installation or image startup, configuration and secret-file handling, Init or
an equivalent controlled fixture, an authenticated request, an authorization
denial, durable state across a restart, and clean shutdown. The separately
packaged **[Weavelit CLI](glossary.md#applications-and-interfaces)** must be
tested on its supported macOS `arm64` platform against the versioned Server
interface.

Release evidence records the artifact version and digest, toolchain version,
validation command results, test-suite results, and the deployment smoke-test
result. A failed required gate blocks deployment unless an explicitly recorded
incident response or rollback procedure authorizes the exception.

## Agent and Review Workflow

An agent or contributor implementing a change must:

1. Read the owning documentation and identify success, rejection, and failure
   behavior before editing implementation code.
2. Add or update focused tests in the same change, including a regression test
   for a bug fix and security tests for any changed trust boundary.
3. Run the narrowest relevant test during development, then run all required
   Rust quality gates that the current workspace supports before handoff.
4. Report the commands actually run, their results, and any validation that
   could not be performed. Do not claim unrun tests or deployment checks
   passed.

Reviewers verify that the tests exercise the stated behavior, negative cases,
and observable contract rather than only raising coverage. A pull request is
incomplete when a behavior change lacks test evidence or a documented,
repeatable reason why automation is not feasible.

## Adoption Sequence

The policy applies immediately to all implementation work. The first Rust
workspace delivery must introduce the pinned toolchain, quality-gate CI, a
test-support pattern for isolated dependencies, and a documented command for
running the full suite locally. Later milestones add integration and
end-to-end suites with the capability they verify; they do not defer those
tests to a final hardening phase.

## Related Documents

- [Technical Specification](spec.md)
- [Security Model](security-model.md)
- [Glossary](glossary.md)
- [Server Init Design](server/lifecycle/init/init-design.md)
