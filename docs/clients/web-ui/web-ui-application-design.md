# Web UI Application Design

This document owns the **[Web UI](../../glossary.md#applications-and-interfaces)** browser application: its pinned build toolchain, the deterministic generated production outputs, the application shell, the pre-operational status presentation states, the first-launch **[Init](../../glossary.md#states-and-requests)** and **[Restore](../../glossary.md#states-and-requests)** choice, the Application Database selection control, the Init workflow, the Restore submission control, and the sign-in control. The [Web UI Pre-Operational Status Surface](../../client-modules/web-ui/pre-operational-status-design.md) owns the `GET /api/v1/status` transport contract this application consumes, the [Web UI Pre-Operational Database Selection Surface](../../client-modules/web-ui/pre-operational-database-selection-design.md) owns the `PUT /api/v1/application-database` route, request schema, headers, and rejection contract the selection control drives, the [Web UI Pre-Operational Init Surface](../../client-modules/web-ui/pre-operational-init-design.md) owns the two-request Init submission protocol, its recovery-key delivery, its browser-side proof-of-possession derivation, and its rejection contract the Init workflow drives, the [Web UI Pre-Operational Restore Surface](../../client-modules/web-ui/pre-operational-restore-design.md) owns the two-request Restore submission protocol, its ticket, and its rejection contract the Restore control drives, the [Embedded Asset Delivery Design](../../client-modules/web-ui/embedded-asset-delivery-design.md) owns how the Server delivers this application's generated output to the browser, and the [Server Authentication Design](../../server/authentication/authentication-design.md) and [Server API Contract](../../server/api/api-contract-design.md) own the shared session and sign-in route contract the sign-in control drives. This document does not restate any of those contracts.

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

## First-Launch Choice

Before an **[Application Database](../../glossary.md#applications-and-interfaces)**
is selected, the shell offers exactly one control: a choice between
**[Init](../../glossary.md#states-and-requests)** and
**[Restore](../../glossary.md#states-and-requests)**. The control is
presented in a titled region carrying a
`data-setup-choice` attribute for testability, with one labelled action per
path and a short description of what each does. The two paths are mutually
exclusive, matching the states-and-requests contract: choosing one hides the
other, and the shell offers no control to change a chosen path once made for
the page's lifetime.

Both paths need a selected Application Database before their own controls
appear, so they share the one [Application Database Selection Control](#application-database-selection-control)
below instead of each presenting a duplicate. Once a database is selected, the
chosen path's own control replaces the selection control.

## Application Database Selection Control

The shell offers exactly one control, shown once a path is chosen from the
[First-Launch Choice](#first-launch-choice) above and only in the Unselected
status state. The control selects SQLite, the single **[Application Database](../../glossary.md#applications-and-interfaces)**
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

## Settling Pre-Operational Outcomes

The Init workflow and the Restore submission control both drive a request that
can seal this deployment, and both are served by pre-operational routes a
sealed deployment withdraws. They therefore share one rule, stated once here
and applied by each control at its own step boundaries.

An outcome is settled only by evidence. A result that reports nothing — a lost
connection, an unreadable body, or the listener's timeout — is never presented
as a failure. Each control reconciles it by submitting the opaque
`reconciliation_capability` its own submission delivered to the submission-bound
lifecycle reconciliation route documented in the
[API Contract Design](../../server/api/api-contract-design.md#lifecycle-reconciliation),
which is served whether or not this deployment is sealed and does not depend
on or change the generic session route described in
[Sign-In Control](#sign-in-control) below. A confirmed result proves that this
exact submission is the one this deployment completed, so it settles the
attempt as a success. A non-matching or unavailable result is not evidence
either way: it is what a workflow still running, a workflow that never
committed, and an unrelated deployment all look like alike, so the attempt
stays unsettled, with a recheck offered, until a confirmed result proves what
happened.

Once an attempt has gone unsettled, a later attempt's rejection is read the
same way whenever the unsettled original could itself have caused it. A Server
that has left `Uninitialized` refuses the workflow that would take it there,
and a Server running normal operation no longer mounts the pre-operational
route at all; each answer is exactly what the unsettled original committing
would produce. Neither proves it, because both also have determinate causes,
so a response that could report the original workflow committed is reconciled
against the lifecycle reconciliation route rather than believed. A confirmed
reconciliation settles the attempt as a success; a non-matching or unavailable
reconciliation leaves it unsettled rather than failed. A rejection answering a
first attempt is never read this way, because it was answered about that
attempt itself.

A rejection the Server did report is determinate and is never reconciled. A
determinate rejection can carry the same fixed code a transport or read failure
presents as, so each control distinguishes the two by whether a response
carried a stable code at all, never by the code that reaches the presentation
layer. This keeps a determinate failure from being dressed up as an unsettled
one, and an unsettled attempt from being presented as a failure.

A delivered recovery key, and the reconciliation capability its own submission
delivered alongside it, are never discarded while an attempt is unsettled.
Both are dropped only on an outcome that is actually known: a completion,
because they have done their work, or a determinate permanent failure, because
no attempt against this Server can ever use either again. While an attempt is
unsettled both stay in the same transient component memory that held them
before, and are never written to a URL, a cookie, `localStorage`, or
`sessionStorage` to survive there.

Neither control re-requests the pre-operational status projection or the
Application Database selection to settle anything. Both are withdrawn once the
deployment is sealed, so both would report the same absence for a sealed
deployment and for a Server that never finished.

## Restore Submission Control

The shell offers the Restore control exactly when the status projection reports
a selected Application Database, which is exactly when the Server makes Restore
eligible. A completed Restore withdraws it, so no second submission is left on
the page. The completion is reported to the shell from the response the
submission already holds, exactly as a confirmed Init is, because the
pre-operational status projection is no longer served once the deployment is
sealed. The shell therefore withdraws the whole setup surface and offers the
sign-in control immediately, without a page reload and without a further status
request.

The control is presented in a titled region carrying a `data-restore-state`
attribute for testability, containing a heading, a short description, a file
input for the encrypted backup, a masked recovery-key input, and an action
button whose accessible name is fixed so it does not change between states. It
renders exactly five states:

| State | Condition | Presentation |
| --- | --- | --- |
| Idle | No submission is in flight and none has failed since the last attempt. | Both inputs are enabled, and the action is enabled once a file is chosen and a key is entered. |
| Submitting | A Restore submission is in flight. | Both inputs and the action are disabled, so a repeated activation cannot issue a second Restore. |
| Indeterminate | A submission reported no outcome, or a retry for an active unsettled submission failed before it received a valid ticket and reconciliation capability, and no probe has settled it. | A fixed message stating that no outcome was reported and that the submitted key is still this backup's key, the reported code, and either a checking notice or a recheck control. |
| Failed | A first submission was rejected determinately, or an issued retry's artifact route rejected determinately. | The inputs are enabled again and the Server's stable error code is presented in an assertive live region. |
| Completed | The Restore completed and the deployment now runs in normal operation. | The inputs and the action are replaced by a fixed completion message in a polite live region. |

The completed state is terminal and momentary: the shell adopts the same
completion and withdraws the whole region, so the deployment's new operational
state is reported by the shell's status region rather than by a control the
sealed deployment no longer offers.

A submission that reports no outcome is not presented as a failure. The
listener's `gateway_timeout`, a transport failure after the artifact upload was
accepted, and a completion body that never arrives intact all leave whether the
Restore committed unknown, and the commit chain any of them abandons is not
cancelled by them. The control settles such a result under
[Settling Pre-Operational Outcomes](#settling-pre-operational-outcomes) above:
it submits the `reconciliation_capability` the recovery-key submission
delivered to the lifecycle reconciliation route, and a confirmed result
reports completion and withdraws the whole setup surface; a non-matching or
unavailable result proves nothing, so the control holds the indeterminate
state, which names the reported code, states that whether the backup was
restored is not yet known, and tells the person to keep the submitted key
because it is still the key this backup is encrypted with. It offers a
recheck control and leaves the retry available only with the original
artifact and key. Those payload controls remain disabled throughout the
indeterminate state, so a retry cannot be mistaken for a different Restore;
the retry action is held disabled while a reconciliation request is in flight,
so no retry is issued against routes a committed Restore no longer serves.

The control retains exactly one active unsettled capability and its recovery
key. A retry that fails before the recovery-key route returns a valid `202`
ticket and reconciliation-capability envelope does not replace that active
pair. Transport loss, unreadable or malformed responses, and every reported
pre-ticket refusal, rate, admission, or timeout result in one reconciliation
request for the active capability; a confirmed result reports completion, while
a non-matching or unavailable result returns to the indeterminate state with the
same pair. The browser neither keeps a capability list nor retries
automatically.

A valid ticket and reconciliation capability for B is the sole succession
event for an active A. The Server's shared Restore mutation lane means B cannot
be issued while A can still commit: B waits until A has released the lane, at
which point A either committed and makes B ineligible or failed before its
checkpoint and cannot later commit. The control then replaces A with B. An
indeterminate B artifact response is reconciled with B's capability; a
determinate B artifact rejection settles B and clears its capability and
recovery key. This preserves the active key through every pre-ticket retry
outcome without treating a response code as proof that A settled.

A rejection the Server itself reported is never settled that way. Its
`restore_failed` code is also the code a transport or read failure presents as,
so the two are distinguished by whether the response carried a stable code at
all, never by the code that reaches the presentation layer.

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
any browser storage, and the key is cleared as soon as the active submission
settles, whether that attempt succeeded or failed. An unsettled submission keeps
its key because that key is still the one its backup is encrypted with and this
page holds no other copy of it. A reconciliation capability is also component
memory only: it is never rendered into the DOM or written to a URL, cookie, or
browser storage. The selected file's bytes are never read into a string, an
`ArrayBuffer`, or an array; the handle is passed to `fetch` as the request body,
so the approved 256 MiB artifact bound streams from the browser's file-backed
storage instead of being copied through the JavaScript heap.

The application performs no client-side validation of the artifact or the key
beyond requiring that both are present. It does not parse, preview, or inspect
backup content, and it does not claim that any client-side check establishes
validity.

## Init Workflow

The shell offers the Init workflow exactly when the person has chosen Init
from the [First-Launch Choice](#first-launch-choice) and the status projection
reports a selected Application Database, the same condition that makes Restore
eligible. The workflow is presented in a titled region carrying a
`data-init-state` attribute reporting its current state — details, preparing,
key, review, finalizing, indeterminate, or closed — for testability, and
renders one of three step regions, each carrying its own `data-init-step`
value: details (covering both the details and preparing states), key, and
review (covering both the review and finalizing states). The indeterminate and
closed states render their own region instead of a step.

The details step collects the System Log and the Audit Log assignments and
the first **[Administrator](../../glossary.md#identities-and-access)**
identity. Each log is assigned independently to the compiled-in SQLite
**[Log Module](../../glossary.md#applications-and-interfaces)** or left
unassigned; the workflow does not imply that a SQLite Log Module shares the
SQLite Application Database's file, schema, or connection. The Administrator
fields are a username, an optional display name, and a password; the password
is held in component state only and is cleared as soon as the attempt it
drives settles, whichever way it settles. Submitting these details requests
the one-time recovery key.

On delivery, the workflow moves to the key step and displays the private
recovery key exactly once, read-only, with a copy control and a fixed warning
that Weavelit cannot show the key again, recover it, or issue a replacement.
Progress past this step is gated behind an explicit acknowledgement checkbox;
the checkbox records the person's stated responsibility and does not verify
that the key was actually copied or stored durably. The workflow then presents
a review step summarizing the submitted username, display name, and log
assignments without redisplaying the password or the recovery key, and a
finalizing step while the completing request is in flight.

The recovery key's proof of possession is derived entirely in the browser
between these steps and is documented in full, including exactly what is sent
and what is never sent, by the
[Web UI Pre-Operational Init Surface](../../client-modules/web-ui/pre-operational-init-design.md#browser-side-proof-derivation).
No other route is available to this page between key delivery and
finalization: the shell has no router and issues no reload or status
re-request across that boundary, so the already-loaded page is the only thing
that can complete the workflow.

A rejected submission distinguishes an actionable failure from a permanent
one rather than presenting one fixed message. An actionable failure returns to
the details step with the Server's stable error code shown, so the person may
correct the details and try again; when a key has already been delivered, the
same delivered key remains the one this deployment expects, and the workflow
never implies that a new key is needed. A permanent failure moves to the
closed step, which presents a fixed message that this Server can no longer
complete Init, withdraws every retry control, and discards the delivered key
from memory. Because the finalization route's `initialization_failed` code
covers both an internal failure and cases the Server cannot yet distinguish
from one, the workflow treats it as actionable so an already-delivered key is
never abandoned on an ambiguous rejection.

A finalization that established no outcome at all is neither of those. The
listener writes `gateway_timeout` when it stops waiting for the route rather
than the route writing it, a rejected `fetch` may still have delivered the
request, and a completion body truncated before it arrives may still have been
written by a deployment that was already sealed. The Server's finalization work
is not cancelled by any of them, so the deployment may have gone on to become
operational after this page was answered. The workflow therefore adds a third
outcome, the indeterminate state, which states plainly that no outcome was
reported, that whether the deployment was initialized is not yet known, and
that the saved recovery key is still the only key this deployment can be
restored with. It offers a recheck control and a retry that returns to the
details step with the same delivered key and its original locked detail
payload.

A failure the Server reported, and a failure raised before any request was
issued such as a proof that could not be derived in the browser, are
determinate and are never presented this way. Both can carry the same fixed
code a transport or read failure presents as, so the workflow distinguishes
them by whether an answer was ever read from the route, never by the presented
code.

This follows from one rule the workflow applies without exception, stated in
full in [Settling Pre-Operational Outcomes](#settling-pre-operational-outcomes)
above: a delivered key is discarded only on an outcome that is actually known.
Completion drops it because it has done its work, and a permanent failure drops
it because no attempt against this Server can ever use it again. An outcome
that reported nothing establishes neither, so the key, its reconciliation
capability, and original password are retained in the same transient
component memory that held them before, and are never written to a URL, a
cookie, `localStorage`, or `sessionStorage` to survive there. Every detail
control stays locked while the outcome is indeterminate, so the retry submits
exactly the request that may already be completing. The password is cleared
as soon as the outcome becomes known.

The workflow settles an indeterminate outcome by submitting the
`reconciliation_capability` the recovery-key response delivered to the
lifecycle reconciliation route documented in the
[API Contract Design](../../server/api/api-contract-design.md#lifecycle-reconciliation),
on first entering the state and again on each recheck. It deliberately does not
re-request the pre-operational status projection or the Application Database
selection, which are withdrawn once the deployment is sealed and would
therefore report the same absence for a sealed deployment and a Server that
never finished. A confirmed result proves that normal operation was published
for this exact finalization, so the workflow releases the key and reports
completion exactly as a confirmed finalization does. A non-matching or
unavailable result proves nothing — finalization may still be running, or may
never have committed, or may have committed for a different attempt — so the
key is kept and the person decides whether to recheck or retry.

Once the workflow has gone unsettled, a retried finalization answered
`already_initialized` or `not_found` is reconciled through that same
reconciliation route rather than believed. A finalization that committed is
exactly what leaves this deployment initialized and withdraws the
pre-operational routes, but a lifecycle pending some other workflow and a
Server serving nothing at all answer identically. A confirmed reconciliation
settles the attempt as a completed Init; a non-matching or unavailable
reconciliation returns the workflow to the indeterminate state, still holding
the delivered key, its reconciliation capability, and original locked payload.
A first finalization answered by either code was answered about itself and is
presented as the rejection it is.

Once finalization is confirmed, the shell adopts that confirmation directly
rather than issuing a further status request, because the pre-operational
status projection is no longer served once the deployment is sealed. This is
the same signal, described in [Sign-In Control](#sign-in-control) below, that
offers the sign-in control next.

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
form that could never succeed. A deployment acquires its first account through
either **[Init](../../glossary.md#states-and-requests)** or
**[Restore](../../glossary.md#states-and-requests)**, so a real sign-in is
reachable only once one of them has completed.

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
| Indeterminate | A session-establishing submission reported no outcome and no session probe has yet authenticated this browser. | A fixed checking message while automatic probes run; afterwards, a fixed unresolved message and one `Check again` action. The submission controls and all attempt secrets are absent. |
| Attempt ended | A one-time value was spent by a reported refusal. | The credential inputs return and the one fixed attempt-ended message is presented in an assertive live region. |
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
it drove settles or indeterminate reconciliation begins. Neither is ever
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

The login response that establishes a session, second-factor verification, and
enrollment confirmation all treat a transport interruption, unreadable `200`,
invalid session-establishing `200` envelope, and the listener's stable
`504 gateway_timeout` envelope as indeterminate. The application immediately
clears the password, code, continuation, enrollment value, setup key, and setup
link, and never retries the mutating submission. It probes the ordinary session
route immediately, then after 10 seconds and 30 seconds. An authenticated
result reaches Authenticated; an unauthenticated or absent result is not proof
of refusal and leaves the control in Indeterminate after the bounded schedule.
`Check again` starts the same schedule only after the preceding one ended, so
automatic reconciliation makes at most six session probes per minute, below the
listener's 12-request burst and 20-requests-per-minute per-source budget.

A reported stable non-`200` rejection, including `400`, `401`, `403`, and
`503`, remains determinate and never starts a session probe. A reported refusal
of a one-time code or enrollment confirmation reaches Attempt ended; a reported
login refusal reaches Failed. Opening enrollment cannot establish a session, so
its rejection also remains terminal. No reconciliation presentation renders a
response detail, transport diagnostic, or secret.

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
that needs them, and dropped as soon as that attempt settles or indeterminate
reconciliation begins. None is written to a URL, a query string, a cookie,
`localStorage`, or `sessionStorage`. The setup key and the setup link are the
only two values this application renders at all, because an operator cannot
capture them otherwise, and they are removed from the rendered output as soon
as the enrollment they belong to settles or reconciliation begins. Nothing that
outlives that enrollment retains them.

## Same-Origin Requests

The application issues exactly eleven outbound request kinds, all same-origin,
all with `cache: no-store` and `redirect: error`:

- `GET /api/v1/status` with `Accept: application/json` and `credentials: omit`;
- `PUT /api/v1/application-database` with `Accept: application/json`, an
  unparameterized `Content-Type: application/json`, the required
  `X-Weavelit-CSRF` header, `credentials: omit`, and the fixed request body the
  [Web UI Pre-Operational Database Selection Surface](../../client-modules/web-ui/pre-operational-database-selection-design.md)
  defines;
- `PUT /api/v1/init/recovery-key` with the same JSON headers, `credentials: omit`,
  and a body carrying the log assignments and the first Administrator's
  identity and password;
- `PUT /api/v1/init` with the same JSON headers, `credentials: omit`, and a
  body carrying that same submission together with the browser-derived
  recovery-key proof of possession;
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

Only the last five requests use `credentials: same-origin`; the preceding six
use `credentials: omit` because no session exists yet to send or receive while
the pre-operational surface is in use.

The application never sets `Host` or `Origin`. Both are forbidden header names
that the browser populates itself on a same-origin request, and a same-origin
request satisfies the route's precondition without client involvement. The
application sends no other request and performs no cross-origin call.

## Related Documents

- [Web UI Pre-Operational Status Surface](../../client-modules/web-ui/pre-operational-status-design.md)
- [Web UI Pre-Operational Database Selection Surface](../../client-modules/web-ui/pre-operational-database-selection-design.md)
- [Web UI Pre-Operational Init Surface](../../client-modules/web-ui/pre-operational-init-design.md)
- [Web UI Pre-Operational Restore Surface](../../client-modules/web-ui/pre-operational-restore-design.md)
- [Embedded Asset Delivery Design](../../client-modules/web-ui/embedded-asset-delivery-design.md)
- [Server Authentication Design](../../server/authentication/authentication-design.md)
- [Server API Contract](../../server/api/api-contract-design.md)
- [Web UI Agent Guide](AGENTS.md)
- [Server Init Design](../../server/lifecycle/init/init-design.md)
- [Server Restore Design](../../server/lifecycle/restore/restore-design.md)
- [Testing and Validation Policy](../../testing.md)
- [Technical Specification](../../spec.md)
