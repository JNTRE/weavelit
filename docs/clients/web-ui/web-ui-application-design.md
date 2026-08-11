# Web UI Application Design

This document owns the **[Web UI](../../glossary.md#applications-and-interfaces)** browser application: its pinned build toolchain, the deterministic generated production outputs, the application shell, the pre-operational status presentation states, the Application Database selection control, the **[Restore](../../glossary.md#states-and-requests)** submission control, and the sign-in control. The [Web UI Pre-Operational Status Surface](../../client-modules/web-ui/pre-operational-status-design.md) owns the `GET /api/v1/status` transport contract this application consumes, the [Web UI Pre-Operational Database Selection Surface](../../client-modules/web-ui/pre-operational-database-selection-design.md) owns the `PUT /api/v1/application-database` route, request schema, headers, and rejection contract the selection control drives, the [Web UI Pre-Operational Restore Surface](../../client-modules/web-ui/pre-operational-restore-design.md) owns the two-request Restore submission protocol, its ticket, and its rejection contract the Restore control drives, the [Embedded Asset Delivery Design](../../client-modules/web-ui/embedded-asset-delivery-design.md) owns how the Server delivers this application's generated output to the browser, and the [Server Authentication Design](../../server/authentication/authentication-design.md) and [Server API Contract](../../server/api/api-contract-design.md) own the shared session and sign-in route contract the sign-in control drives. This document does not restate any of those contracts.

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

## Authentication

This application implements the browser sign-in surface for the shared session
the [Server Authentication Design](../../server/authentication/authentication-design.md)
and the [Server API Contract](../../server/api/api-contract-design.md) own; the
Web UI **[Client Module](../../glossary.md#applications-and-interfaces)** is the
only registered `client_module` value the login route accepts, so this
application is the sole eligible client for a session. This document owns only
the control's presentation, gating, and confidentiality behavior in the
browser; it does not restate the route contract or the session and
cross-site request forgery (CSRF) cookie shapes those documents define.

### Sign-In Control

The shell offers the sign-in control exactly when the pre-operational status
projection described in [Status Presentation States](#status-presentation-states)
is no longer served, which is the only externally observable signal that a
deployment may now be operational: the status route is withdrawn once the
deployment is sealed, and an unreachable Server produces the identical
absence. The control does not render on that signal alone. It first probes the
Server's own session route and renders nothing while that probe is in flight or
if the probed surface is also absent, so an unreachable Server never presents a
form that could never succeed. Because **[Restore](../../glossary.md#states-and-requests)**
is currently the only way a deployment acquires an account, a real sign-in is
reachable only once a Restore has completed.

The control is presented in a titled region carrying a
`data-authentication-state` attribute for testability. Its initial probe and an
absent authentication surface both render nothing; the remaining states are:

| State | Condition | Presentation |
| --- | --- | --- |
| Unauthenticated | The session probe reports no session authenticates. | A username input, a masked password input, and an action; the action is enabled once both are non-empty. |
| Submitting | A sign-in request is in flight. | Both inputs and the action are disabled, so a repeated activation cannot issue a second sign-in. |
| Failed | The submitted credential was rejected. | The inputs and action are re-enabled and the one fixed failure message is presented in an assertive live region. |
| Second factor | A verified password was admitted only to an enrolled second factor. | The credential inputs are replaced by a single code input and a verification action; the action is enabled only for a submission the route's code shape accepts. |
| Second factor submitting | A code submission is in flight. | The input and the action are disabled, so a repeated activation cannot spend a second continuation. |
| Enrollment | A verified password was admitted only to enrolling a factor, and the enrollment has been opened. | The one disclosure, a code input, and a confirmation action, described in [Second-Factor Steps](#second-factor-steps). |
| Enrollment submitting | An enrollment confirmation is in flight. | The inputs and the action are disabled. |
| Attempt ended | A one-time value was spent by a refused request. | The credential inputs return and the one fixed attempt-ended message is presented in an assertive live region. |
| Authenticated | The session probe, or a completed sign-in, reports an established session. | The inputs and action are replaced by a fixed confirmation message in a polite live region. |

The failure message is fixed and redacted: `Sign-in failed.` The login route
deliberately reports no cause for a denial and returns the identical response
for an unknown account, an inactive account, and a wrong password; the
application discards whatever it received rather than inspecting it, so it has
nothing to render even if a future response tried to distinguish causes. This
follows the same precedent as the status and selection failure states.

Every second-factor state is reachable only from a continuation the Server
issued, so no rendered state widens what a denied credential discloses: a
denial reaches only the failed and attempt-ended states.

The username is held in component state for the duration of the panel. The
password is held in component state only and is cleared as soon as the attempt
it drove settles, whether that attempt succeeded or failed. Neither is ever
rendered, and neither is written to a URL, a cookie, or any browser storage.

### Second-Factor Steps

A login that verified a password without issuing a session answers with a
continuation and a stage naming which step is owed. The application presents
one step per stage and never invents a third.

For `mfa_required` the control presents a single code input and submits it once
with the continuation it was issued.

For `mfa_enrollment_required` the control opens an enrollment with that same
continuation and presents what the response discloses: the Base32 setup key,
the `otpauth://` setup link, and a code input confirming that an authenticator
app now holds the key. Both disclosed values are presented as read-only text
controls so they can be selected and copied, and neither is rendered as a
navigation target. The step carries a fixed warning that the two values are
shown once and cannot be shown again, because the Server discloses them in
exactly one response and can never return them. The enrollment is opened from
the settled login submission rather than from a render effect, so the single
disclosure is requested exactly once per attempt.

Every one-time value is spent by the first request that presents it, whether or
not that request was accepted. A refused code therefore ends the attempt rather
than inviting another code against the same value, and the control says so
plainly: `That code was not accepted. This sign-in attempt has ended, so sign in
again to start a new one.` The credential inputs return with the password
cleared, which is the only thing that can follow a spent continuation. The same
message and the same state are presented when an enrollment cannot be opened
and when an enrollment confirmation is refused, because those refusals spend a
one-time value identically and the Server reports no cause for any of them.

### CSRF Cookie Handling

The application reads the Server's readable `__Host-weavelit_csrf` cookie and
echoes its value in the `X-Weavelit-CSRF` header of the session probe, the one
mutating request this application issues while an existing session may already
be present. A cookie value outside the issued opaque-token shape is discarded
rather than echoed, so nothing else a cookie jar happens to hold can reach a
request header. The login request instead carries the fixed pre-session literal
the route requires, because signing in is the bootstrap request: no session,
and therefore no per-session CSRF token, exists yet to echo. The session cookie
itself is never read by this application; the browser attaches it
automatically, and the application never inspects, renders, or stores its
value.

### Confidentiality

No credential or session-related value is ever placed in a URL, a query
string, `localStorage`, `sessionStorage`, or rendered output. The username and
password travel only in the login request body; the session and CSRF values
are only ever carried in the cookies the Server sets and reads, and this
application never reads either back from a response body, renders either, or
persists either itself.

The continuation, the enrollment value, the submitted code, the setup key, and
the setup link are held the same way: in component state for the one attempt
that needs them, and dropped as soon as that attempt settles. None is written
to a URL, a query string, a cookie, `localStorage`, or `sessionStorage`. The
setup key and the setup link are the only two values this application renders
at all, because an operator cannot capture them otherwise, and they are removed
from the rendered output as soon as the enrollment they belong to settles.
Nothing that outlives that enrollment retains them.

## Same-Origin Requests

The application issues exactly nine outbound request kinds, all same-origin,
all with `cache: no-store` and `redirect: error`:

- `GET /api/v1/status` with `Accept: application/json` and `credentials: omit`;
- `PUT /api/v1/application-database` with `Accept: application/json`, an
  unparameterized `Content-Type: application/json`, the required
  `X-Weavelit-CSRF` header, `credentials: omit`, and the fixed request body the
  [Web UI Pre-Operational Database Selection Surface](../../client-modules/web-ui/pre-operational-database-selection-design.md)
  defines;
- `PUT /api/v1/restore` with the same JSON headers, `credentials: omit`, and a
  body carrying only the recovery key;
- `PUT /api/v1/restore/artifact` with `Accept: application/json`, an
  unparameterized `Content-Type: application/octet-stream`, the required
  `X-Weavelit-CSRF` header, `credentials: omit`, the issued ticket in
  `X-Weavelit-Restore-Ticket`, and the selected file as the request body;
- `PUT /api/v1/auth/login` with `Accept: application/json`, an unparameterized
  `Content-Type: application/json`, the fixed pre-session `X-Weavelit-CSRF`
  literal the route requires, `credentials: same-origin` so the Server's
  issued cookies are stored, and a body carrying the username, the password,
  and this application's Client Module identifier; and
- `PUT /api/v1/auth/session` with `Accept: application/json`, the
  `X-Weavelit-CSRF` header carrying the readable cookie's value when one is
  present, and `credentials: same-origin` so an already-issued session cookie
  is sent;
- `PUT /api/v1/auth/mfa/verify` with the same JSON headers, the same fixed
  pre-session `X-Weavelit-CSRF` literal, `credentials: same-origin`, and a body
  carrying the continuation and the submitted code;
- `PUT /api/v1/auth/mfa/enrollment` with the same headers and credentials mode
  and a body carrying the continuation alone; and
- `PUT /api/v1/auth/mfa/enrollment/confirm` with the same headers and
  credentials mode and a body carrying the enrollment value and the submitted
  code.

The three second-factor requests carry the pre-session literal rather than a
per-session token, because they carry no session either: the one-time value in
the body is the only thing binding them to an earlier verified password. They
use `credentials: same-origin` so the cookies a completed step issues are
stored. This application does not issue the session-bearing self-enrollment
request the Server also serves.

Only the last five requests use `credentials: same-origin`; the preceding four
use `credentials: omit` because no session exists yet to send or receive while
the pre-operational surface is in use.

The application never sets `Host` or `Origin`. Both are forbidden header names
that the browser populates itself on a same-origin request, and a same-origin
request satisfies the route's precondition without client involvement. The
application sends no other request and performs no cross-origin call.

## Related Documents

- [Web UI Pre-Operational Status Surface](../../client-modules/web-ui/pre-operational-status-design.md)
- [Web UI Pre-Operational Database Selection Surface](../../client-modules/web-ui/pre-operational-database-selection-design.md)
- [Web UI Pre-Operational Restore Surface](../../client-modules/web-ui/pre-operational-restore-design.md)
- [Embedded Asset Delivery Design](../../client-modules/web-ui/embedded-asset-delivery-design.md)
- [Server Authentication Design](../../server/authentication/authentication-design.md)
- [Server API Contract](../../server/api/api-contract-design.md)
- [Web UI Agent Guide](AGENTS.md)
- [Server Restore Design](../../server/lifecycle/restore/restore-design.md)
- [Testing and Validation Policy](../../testing.md)
- [Technical Specification](../../spec.md)
