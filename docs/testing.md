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
| Web UI Client Module pre-operational status surface | Direct-TLS process and contract tests for both database-selection results; every lifecycle availability boundary; accepted and rejected media negotiation; every fixed method, body, malformed-request, target, header, capacity, rate, and timeout rejection; connection, handler, handshake, rate, and response-size limits; redaction; and absence of CORS, cookies, normal routes, and a cleartext listener. |
| Authentication, authorization, secret handling, audit logging, MFA, or destructive operations | Tests for every allowed and denied path, plus tests that sensitive values are absent from returned errors and logs. |
| Audit terminal recovery, binding retention, and supersession | Contract, projection, and import tests proving ordinary transitions retain the exact prior identity/version and resolved handle; changed-binding delivery is rejected before destination access; only matching authority, exact confirmation, and successful replacement preflight evidence can create the fixed disposition; malformed, repeated, mismatched, non-oldest, and out-of-order input fails closed; a same-identifier but byte-different original projection or retained-binding mismatch is rejected before mutation; the original remains immutable in an oldest-first late-delivery sequence; the replacement action records degraded completeness without secret or raw-error content; and only exact delivery through each obligation's own binding permits acknowledgement. Restore and System Logs are tested as non-substitutes. Concrete backend work adds restart and transactional rollback evidence for exact-original comparison, disposition, assignment, and replacement-obligation persistence. |
| Server-owned lifecycle and pre-operational database-selection contract | Direct tests for every startup classification; the published anchor known-answer vector; strict version, canonical JSON, Base64, schema, size, setting, and cryptographic validation; deployment-record creation and irreversible sealing; deployment-identifier and locator-generation matching; database selection and generation-pointer locator persistence; rejection of client-supplied paths and file references; Server-derived local paths; encrypted secret connection persistence and restart reopening; workflow exclusivity and process-lifetime root locking; mutation serialization; valid-run failure handling for file or directory synchronization, locator commit, cleanup, and cross-store operations; direct invocation after sealing; exact redacted category/reason output; and fail-closed missing, malformed, mismatched, unavailable, or integrity-failing deployment state. Isolated real-filesystem tests cover non-root ownership, exact modes, every path-component and child symlink position, hard links, non-regular and unknown entries, closed-inventory bounds, unavailable filesystem behavior, retained temporary and orphan classification, interrupted-bootstrap classification, SQLite recovery sidecars, and restart. Contract and process tests verify nonzero exit before HTTPS bind, route gating, stable redacted interruption action-class diagnostics, absence of post-interruption completion-log delivery, reconciliation, cleanup, or sealing, and transitions to normal operation only after valid completion. They must not assert survival across power loss or abrupt process termination as an application guarantee; where the Server can start and classify state, they must assert the fail-closed result and stable redacted error reporting. |
| Server-owned **[Init](glossary.md#states-and-requests)** use case and Init-capable **[Pre-Operational Surface](glossary.md#applications-and-interfaces)** | Direct workflow tests for normalized request validation, validation before later secret submission, recovery-key generation, one-time delivery, proof, Init-checkpoint handling, atomic fresh-state creation, durable Init-result System Log recording during a valid run, retained-partial-state classification, absence of post-interruption reconciliation, retry, reset, automatic deletion, recreation, and sealing, concurrency, redaction, and rejection before secret reading or side effects; contract and process tests for lifecycle composition, fail-closed route removal, stable redacted action-class diagnostics, and the transition to normal operation only after valid completion. |
| Server-owned **[Restore](glossary.md#states-and-requests)** use case and Restore-capable Pre-Operational Surface | Direct workflow tests for every artifact and resource bound, malformed, unauthentic, integrity-failing, incompatible, and semantically invalid backups, wrong recovery keys, checkpoint handling, session invalidation, recovery-public-key preservation, protected-secret re-encryption, private-key and plaintext non-persistence, atomic rollback, durable Restore-result System Log recording during a valid run, retained-partial-state classification, absence of post-interruption reconciliation, retry, reset, automatic cleanup, recreation, and sealing, Restore-specific valid-run failure classification, concurrency with Init and Restore, redaction, and rejection before key or artifact processing; runtime tests that judge at least one complete submission against the Server's own compiled-in component inventory rather than a supplied one, and that a backup naming a component the build lacks is refused as `backup_incompatible` before any state changes; external known-answer vectors from the C2SP Community Cryptography Test Vectors for age, vendored at a pinned upstream commit, run against the age v1 reader with each vector's outcome pinned by category; a fixture-credential test that reads the committed fixtures' administrator password verifier back through the production Restore reader and authenticates it against the documented fixture password through the real password authenticator, pinning it to the current approved Argon2 profile rather than merely an accepted one, and denies every other tried password; content tests that a backup carrying a password verifier outside the approved profile allowlist is refused as `backup_invalid` before restored state is constructed, that a verifier at the approved profile still normalizes, and that the refusal is indistinguishable from every other invalid-backup cause; and contract, process, and Web UI end-to-end tests for transfer, the two-request submission protocol and its one-time ticket, lifecycle gating, stable redacted action-class diagnostics, fail-closed route removal, and a real sign-in, only reachable after valid Restore completion, whose session persists across a Server restart. |
| Provider integration | Tests against controlled fakes or recorded fixtures for request construction, error mapping, retry, rate-limit, and duplicate-protection behavior. Live-provider checks are separately controlled smoke tests, never the default test suite. |
| Web UI, Weavelit CLI, packaging, or deployment workflow | Focused end-to-end or smoke tests of the user workflow and the failure condition most likely to cause an unusable release. |

Use table-driven tests for meaningful input combinations and property tests for
invariants with broad input spaces. Prefer assertions on public results,
persisted state, emitted audit events, and provider requests over assertions on
private functions or internal call order. Each defect fix includes a regression
test that fails before the fix and passes after it.

## Server Quality Gates

All Rust code uses the repository's Rust 1.97 stable toolchain. Once the Cargo
workspace is introduced, it must commit a `rust-toolchain.toml` that pins the
toolchain and required components. Run the complete Server Rust quality-gate
suite in the documented development container before integration into `dev` and
in CI before integration into `main` with:

```sh
make -C server check
```

The Server `Makefile` runs the following required commands without warnings or
failures. Add a required default Rust quality gate there so development-container
and CI validation remain identical:

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --doc
cargo test --locked --workspace --all-targets
cargo build --locked --workspace --release
```

Crate-local tests are included through their workspace member's Cargo targets.
A dedicated Server test crate must be a workspace member so `make -C server
check` runs it automatically.

`make -C server check` runs the Web UI gate before the Rust commands above, in
this order: a locked `npm ci` install, a TypeScript typecheck, the Web UI unit
tests, the Node test-runner suite for the build-output validator scripts, a
clean production build that also writes the build content manifest, and a
build-output check that fails on a missing, renamed, extra, or oversized
generated asset, re-verifies the manifest against the current bundle inputs and
generated assets, and reports raw and gzip bundle sizes. Raw sizes are the
enforced budget because the Server serves these assets without compression. The
Node and npm releases are pinned by `server/web-ui/.node-version` and
`server/web-ui/package.json`, and every frontend dependency is pinned to an
exact version and locked by `server/web-ui/package-lock.json`.

After the Rust commands, `make -C server check` installs the pinned Chromium
build and runs the Playwright suite in `server/web-ui/browser-tests/` against
the release Server binary over its real direct-TLS listener. That suite covers
the pre-operational status page load, the complete Application Database
selection outcome, the complete Restore submission outcome, and a sign-in that
survives a Server restart.

The selection scenario has an operator select SQLite through the Web UI control,
the displayed status change to selected within the same process, the Server
terminated with `SIGTERM` and its exit awaited and asserted rather than assumed,
and a second Server generation started against the identical state root and
listener port, where the reloaded page still reports the selected database. The
Server stops on that signal through its own bounded shutdown, so this exercises
an orderly stop and restart end to end; a successful restart is also evidence
that the stopped process released the state-root lock and the listening socket.
The Rust suite proves the shutdown itself, including a `SIGTERM` to the built
binary that must exit with status `0` and no terminating signal, so this
scenario is browser evidence of the restart rather than the primary evidence of
the shutdown.

The Restore scenario drives the two-request submission protocol through the
browser against the committed backup fixture whose referenced components match
what the release binary compiles in. It asserts a rejected attempt rendered as
the Server's stable code alone, then a completed Restore that activates normal
operation in the same process, both Restore routes absent from the sealed
surface, and no recovery key or ticket in any request URL, cookie, browser
storage, rendered page, or Server output. Because it drives the real binary, it
would fail if the fixture named a component the binary does not compile in; the
`weavelit-server` suite proves the same pairing first, so that failure is caught
before the browser layer.

The sign-in scenario runs four Server generations against one restored
deployment: the first restores the committed `valid-web-ui-sqlite.wlitbackup`
fixture, the deployment is sealed, the pre-operational status route becomes
absent, and the shell falls through to the sign-in control. The second
generation submits an unknown account and the fixture account with a wrong
password and asserts the two denials are indistinguishable field for field
beside their correlation identifier, both render the identical fixed failure
message, and neither sets a cookie; it then submits the documented fixture
password and asserts a session is established with exactly the two approved
cookies at their documented attributes. The third generation restarts the
Server and reloads the page with no credential re-entered, asserting the
persisted session, not remembered client state, is what authenticates. It loads
the Groups chunk only after selection and exercises its safe access-detail
reads. The fourth generation again adopts the persisted session, proves neither
lazy chunk loads with Accounts, loads only the fixed Configuration chunk after
selection, exercises real Log configuration list and view projections, and
receives a real TOTP enablement preview without applying it. The scenario also
asserts that a pinned set of real, non-empty observed secret values, namely both
passwords, the issued session and CSRF tokens, and the single-claim TOTP
enablement preview, is
absent from every request URL, browser storage, rendered page, and captured
Server stdout and stderr; pinning each secret to a value the run actually
produced keeps that absence check from passing vacuously against an unobserved
or empty string.

Each generation asserts the exact set of requests it served, which keeps every
scenario inside the listener's per-source request-rate budget.

The build content manifest has two test surfaces, one per consumer, because a
silent mismatch between the writer and the verifier would reintroduce the stale
embedded asset it exists to prevent. The Node suite covers manifest write mode,
check mode, and the requirement that check mode never repairs a stale manifest.
The Web UI Client Module's `tests/build_manifest.rs` compiles the build script's
verification module directly and covers a valid manifest, a missing, malformed,
non-object, or wrongly versioned manifest, an unrecognized field, a non-digest
entry, a source-hash mismatch, an asset-hash mismatch, an added or removed
bundle input, and the rebuild-trigger inventory.

`cargo fmt` and Clippy are quality gates, not substitutes for tests. The
repository's shared VS Code settings run rustfmt on Rust-file saves and cause
rust-analyzer to run Clippy diagnostics during editing. Contributors still run
the relevant commands before requesting review because editor feedback is not
CI evidence.

Before a feature pull request is merged into `dev`, its author MUST run
`make -C server container-check` against the commit to be merged. This is the
required integration evidence for `dev`.

The Rust Quality workflow MUST run the same gate from a clean Ubuntu checkout
for every non-draft pull request targeting `main`, including each subsequent
update to that pull request. A passing result for the current pull-request head
is required before merging into `main`. It must also run affected integration,
contract, and end-to-end suites. A later change may add coverage reporting
after its report format, exclusions, and ratchet policy are documented. A
global line-coverage percentage is not a merge criterion: coverage is useful
as a trend and gap signal but cannot prove that the security and failure
behavior above was exercised.

## Deployment Confidence

Before a release or deployment, CI builds the exact release artifact from a
clean checkout and verifies it in a production-like environment. For the
**[Weavelit Server](glossary.md#applications-and-interfaces)**, this includes
installation or image startup, configuration and protected credential handling,
Init or an equivalent controlled fixture, an authenticated request, an
authorization denial, application state across a controlled restart, and clean shutdown. The
separately packaged **[Weavelit CLI](glossary.md#applications-and-interfaces)**
must be tested on its supported macOS `arm64` platform against the versioned
Server interface.

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
3. Run the narrowest relevant test during development. Before merging a change
  into `dev`, run `make -C server container-check`. Before merging into `main`,
  obtain a passing Rust Quality result for the current non-draft pull-request
  head.
4. Report the commands actually run, their results, the tested commit SHA, and
  any validation that could not be performed. Do not claim unrun tests or
  deployment checks passed.

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
- [Audit Terminal Binding Retention And Supersession Decision](log-modules/audit-terminal-binding-retention-decision.md)
