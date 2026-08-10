# Web UI Application Design

This document owns the **[Web UI](../../glossary.md#applications-and-interfaces)** browser application: its pinned build toolchain, the deterministic generated production outputs, the application shell, the pre-operational status presentation states, the Application Database selection control, and the **[Restore](../../glossary.md#states-and-requests)** submission control. The [Web UI Pre-Operational Status Surface](../../client-modules/web-ui/pre-operational-status-design.md) owns the `GET /api/v1/status` transport contract this application consumes, the [Web UI Pre-Operational Database Selection Surface](../../client-modules/web-ui/pre-operational-database-selection-design.md) owns the `PUT /api/v1/application-database` route, request schema, headers, and rejection contract the selection control drives, the [Web UI Pre-Operational Restore Surface](../../client-modules/web-ui/pre-operational-restore-design.md) owns the two-request Restore submission protocol, its ticket, and its rejection contract the Restore control drives, and the [Embedded Asset Delivery Design](../../client-modules/web-ui/embedded-asset-delivery-design.md) owns how the Server delivers this application's generated output to the browser. This document does not restate any of those contracts.

## Build Toolchain

The application builds under `server/web-ui` with an exactly pinned toolchain,
locked by a committed `package-lock.json`:

| Tool | Version |
| --- | --- |
| Node.js | 24.19.0 |
| npm | 11.17.0 |
| React and React DOM | 19.2.8 |
| Vite | 8.2.0 |
| TypeScript | 6.0.3 (planned upgrade to 7 tracked in #117) |
| Vitest | 4.1.10 |
| `@playwright/test` | 1.62.1 |

`server/web-ui/.npmrc` sets `save-exact=true` so every dependency is pinned to
an exact version, and `engine-strict=true` so an install fails outright under
any other Node or npm release. `server/web-ui/.node-version` and
`package.json#engines` are the authoritative pinned versions; a toolchain
upgrade is a deliberate, reviewed change to those files rather than a floating
range.

## Generated Production Output

A clean production build emits exactly three unhashed files: `dist/index.html`,
`dist/assets/weavelit-application.js`, and `dist/assets/weavelit-application.css`. The build
produces no source maps, no code-split chunks, and no other emitted file.

Content hashing is deliberately disabled in the build configuration because the
Web UI **[Client Module](../../glossary.md#applications-and-interfaces)** embeds
these assets by an exact compile-time allowlist;
hashed, content-derived file names are incompatible with that allowlist. The
[Embedded Asset Delivery Design](../../client-modules/web-ui/embedded-asset-delivery-design.md)
owns the resulting `Cache-Control: no-store` response header that compensates
for the fixed names.

`server/web-ui/scripts/verify-build-output.mjs` runs after every production
build and fails on a missing, renamed, extra, or oversized output. It enforces
each asset's raw-byte budget and a combined budget, and reports raw and gzip
sizes for information only; the Server serves these assets without
compression, so raw bytes are the enforced budget. `make -C server check` runs
this validator as part of its Web UI gate before the Rust workspace gates.

## Build Content Manifest

The production build writes `dist/build-manifest.json` immediately after a clean
Vite build. It is build metadata, not a fourth generated asset: `dist/` is not
committed, the manifest is excluded from the generated-output inventory check,
and the [Embedded Asset Delivery Design](../../client-modules/web-ui/embedded-asset-delivery-design.md)
never embeds or serves it.

The manifest is strict JSON with exactly three fields: a `format_version`
integer, an `inputs` object, and an `assets` object. Each object maps a `/`-separated
relative path to that file's lowercase SHA-256 hex digest, with keys sorted so
the same tree always produces the same file. `format_version` is `1`; any other
value is rejected rather than interpreted.

A **bundle input** is a file that can change the production bundle. The
inventory is exactly:

- `index.html`, the Vite entry document;
- every file under `src/` except a test-only file, meaning any name ending in
  `.test.ts` or `.test.tsx` and `test-setup.ts`; and
- `vite.config.ts`, `tsconfig.json`, `package.json`, and `package-lock.json`,
  the build, compiler, and resolved-dependency configuration.

Test-only sources, `browser-tests/`, `playwright.config.ts`, and `scripts/` are
excluded because none of them reaches the production bundle; including them
would make editing a unit test fail an otherwise correct Rust build. A missing
configuration input is a failure rather than a silently shortened inventory.

The validator has two modes. `--write` validates the bundle and then writes the
manifest, and runs only as the second half of `npm run build`, immediately after
Vite empties and repopulates `dist/`. `--check` validates the bundle and
re-verifies the manifest against the current inputs and outputs, and is what
`npm run verify:build` runs. Check mode never writes, repairs, or refreshes the
manifest: if it could, re-running verification would bless a stale build and
defeat the freshness guarantee.

## Application Shell

The application has a single root component, `ApplicationShell`, mounted into
`#weavelit-root` by `main.tsx`. It deliberately has no router, no
state-management library, and no CSS framework: its only production
dependencies are `react` and `react-dom`, and `weavelit-application.css` is
hand-authored. This reflects the current absence of any client-side route and
the single control the pre-operational experience offers; a later
normal-operation experience revisits this shell.

## Status Presentation States

The shell requests the pre-operational status once on mount through
`useDeploymentStatus`, and re-requests it only on an explicit reload. It
renders exactly four presentation states from that request, each identified by
a `data-status-state` attribute on the status region for testability:

| State | Condition | Presentation |
| --- | --- | --- |
| Loading | The status request has not yet settled. | A fixed loading message. |
| Selected | The response reports `database_selected: true`. | A fixed message confirming a selected **[Application Database](../../glossary.md#applications-and-interfaces)**. |
| Unselected | The response reports `database_selected: false`. | A fixed message confirming no Application Database is selected. |
| Failure | A transport failure, a non-`200` response, or a payload missing a documented field or carrying a documented field with the wrong type. | A fixed, payload-free failure message. |

The failure state carries no response detail: it never renders a server
payload, status text, or transport diagnostic. The client ignores unknown
additive JSON fields consistent with the versioned `/api/v1/` compatibility
policy, but treats a missing or wrongly typed documented field
(`lifecycle` or `database_selected`) as a failure rather than guessing a
default.

## Application Database Selection Control

The shell offers exactly one control, and only in the Unselected status state.
The control selects SQLite, the single **[Application Database](../../glossary.md#applications-and-interfaces)**
backend this milestone supports, so it is a single labelled action rather than a
backend picker. It is presented in a titled region carrying a
`data-selection-state` attribute for testability, containing a heading, a short
description of what selecting SQLite does, and an action button whose accessible
name is fixed so it does not change between states.

The control renders exactly three states:

| State | Condition | Presentation |
| --- | --- | --- |
| Idle | No submission is in flight and none has failed since the last attempt. | The action is enabled and no failure message is present. |
| Submitting | A selection request is in flight. | The action is disabled, so a repeated activation cannot issue a second selection. |
| Failed | The selection request did not return a valid success projection. | The action is enabled again and a fixed failure message is presented in an assertive live region. |

The failure message is fixed and redacted. The selection route deliberately
returns no detail that distinguishes which check failed, so the application
surfaces no server error code, no HTTP status number, and no transport
diagnostic; every failure cause presents identically. This follows the same
precedent as the status failure state.

On success, the response body carries the authoritative status projection. The
application applies that projection directly to the displayed status and issues
no follow-up status request, because a second request would spend a shared
rate-limit budget to re-read a value the Server already returned. Applying the
projection moves the status to Selected, which withdraws the control: once a
database is selected the shell never offers to select again, and a repeat
database is selected the shell never offers to select again, and an exact replay
is accepted as a successful no-op; only a differing repeat is refused by the
Server.

## Restore Submission Control

The shell offers the Restore control exactly when the status projection reports
a selected Application Database, which is exactly when the Server makes Restore
eligible. The same condition withdraws it: the pre-operational status projection
is no longer served once the deployment is sealed, so a completed Restore
removes the control rather than leaving a second submission on the page.

The control is presented in a titled region carrying a `data-restore-state`
attribute for testability, containing a heading, a short description, a file
input for the encrypted backup, a masked recovery-key input, and an action
button whose accessible name is fixed so it does not change between states. It
renders exactly four states:

| State | Condition | Presentation |
| --- | --- | --- |
| Idle | No submission is in flight and none has failed since the last attempt. | Both inputs are enabled, and the action is enabled once a file is chosen and a key is entered. |
| Submitting | A Restore submission is in flight. | Both inputs and the action are disabled, so a repeated activation cannot issue a second Restore. |
| Failed | Either request of the submission was rejected. | The inputs are enabled again and the Server's stable error code is presented in an assertive live region. |
| Completed | The Restore completed and the deployment now runs in normal operation. | The inputs and the action are replaced by a fixed completion message in a polite live region. |

The failure presentation renders the Server's stable, lowercase error code and
nothing else: no server message, HTTP status number, field path, or transport
diagnostic. A code outside the closed stable-code shape, an unreadable body, a
response outside the contract, and a transport failure all present as the single
documented `restore_failed` code, so an unexpected response cannot become a
distinguishing signal or place arbitrary text on the page. Rendering the code
rather than a single fixed message is deliberate here and differs from the
selection control: the Restore rejection contract distinguishes causes the
person can act on, such as an invalid recovery key or an incompatible backup.

The recovery key is held in component state alone and the backup is held only as
the browser-provided `File` handle. Neither is written to a URL, a cookie, or
any browser storage, and the key is cleared as soon as the attempt it drove
settles, whether that attempt succeeded or failed. The selected file's bytes are
never read into a string, an `ArrayBuffer`, or an array; the handle is passed to
`fetch` as the request body, so the approved 256 MiB artifact bound streams from
the browser's file-backed storage instead of being copied through the JavaScript
heap.

The application performs no client-side validation of the artifact or the key
beyond requiring that both are present. It does not parse, preview, or inspect
backup content, and it does not claim that any client-side check establishes
validity.

## Same-Origin Requests

The application issues exactly four outbound request kinds, all same-origin, all
with `credentials: omit`, `cache: no-store`, and `redirect: error`:

- `GET /api/v1/status` with `Accept: application/json`;
- `PUT /api/v1/application-database` with `Accept: application/json`, an
  unparameterized `Content-Type: application/json`, the required
  `X-Weavelit-CSRF` header, and the fixed request body the
  [Web UI Pre-Operational Database Selection Surface](../../client-modules/web-ui/pre-operational-database-selection-design.md)
  defines;
- `PUT /api/v1/restore` with the same JSON headers and a body carrying only the
  recovery key; and
- `PUT /api/v1/restore/artifact` with `Accept: application/json`, an
  unparameterized `Content-Type: application/octet-stream`, the required
  `X-Weavelit-CSRF` header, the issued ticket in `X-Weavelit-Restore-Ticket`,
  and the selected file as the request body.

The application never sets `Host` or `Origin`. Both are forbidden header names
that the browser populates itself on a same-origin request, and a same-origin
request satisfies the route's precondition without client involvement. The
application sends no other request, uses no credentials, and performs no
cross-origin call.

## Related Documents

- [Web UI Pre-Operational Status Surface](../../client-modules/web-ui/pre-operational-status-design.md)
- [Web UI Pre-Operational Database Selection Surface](../../client-modules/web-ui/pre-operational-database-selection-design.md)
- [Web UI Pre-Operational Restore Surface](../../client-modules/web-ui/pre-operational-restore-design.md)
- [Embedded Asset Delivery Design](../../client-modules/web-ui/embedded-asset-delivery-design.md)
- [Web UI Agent Guide](AGENTS.md)
- [Server Restore Design](../../server/lifecycle/restore/restore-design.md)
- [Testing and Validation Policy](../../testing.md)
- [Technical Specification](../../spec.md)
