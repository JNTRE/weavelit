# Web UI Application Design

This document owns the **[Web UI](../../glossary.md#applications-and-interfaces)** browser application: its pinned build toolchain, the deterministic generated production outputs, the application shell, and the pre-operational status presentation states. The [Web UI Pre-Operational Status Surface](../../client-modules/web-ui/pre-operational-status-design.md) owns the `GET /api/v1/status` transport contract this application consumes, and the [Embedded Asset Delivery Design](../../client-modules/web-ui/embedded-asset-delivery-design.md) owns how the Server delivers this application's generated output to the browser. This document does not restate either contract.

## Build Toolchain

The application builds under `server/web-ui` with an exactly pinned toolchain,
locked by a committed `package-lock.json`:

| Tool | Version |
| --- | --- |
| Node.js | 24.19.0 |
| npm | 11.17.0 |
| React and React DOM | 19.2.8 |
| Vite | 8.2.0 |
| TypeScript | 7.0.2 |
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
`dist/assets/application.js`, and `dist/assets/application.css`. The build
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
dependencies are `react` and `react-dom`, and `application.css` is
hand-authored. This reflects the current absence of any client-side route or
selection control; a later normal-operation experience revisits this shell.

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
default. No presentation state renders a selection control; introducing one is
tracked separately from this design.

## Same-Origin Status Request

The application issues exactly one outbound request kind: a same-origin
`GET /api/v1/status` with `Accept: application/json`, `credentials: omit`,
`cache: no-store`, and `redirect: error`. It sends no other request, uses no
credentials, and performs no cross-origin call.

## Related Documents

- [Web UI Pre-Operational Status Surface](../../client-modules/web-ui/pre-operational-status-design.md)
- [Embedded Asset Delivery Design](../../client-modules/web-ui/embedded-asset-delivery-design.md)
- [Web UI Agent Guide](AGENTS.md)
- [Testing and Validation Policy](../../testing.md)
- [Technical Specification](../../spec.md)
