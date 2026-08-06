# Embedded Asset Delivery Design

This document owns the server-side **[Client Module](../../glossary.md#applications-and-interfaces)** contract for delivering the Web UI's generated browser assets: the compile-time asset allowlist, exact MIME types, security headers, body-size bounds, and path-rejection behavior. The [Web UI Application Design](../../clients/web-ui/web-ui-application-design.md) owns the build toolchain and the browser application that produces and consumes these assets. The [Web UI Pre-Operational Status Surface](pre-operational-status-design.md) owns the separate `GET /api/v1/status` transport contract served by the same module. This document does not restate either contract.

## Asset Allowlist And Routes

The `weavelit-module-client-webui` crate embeds exactly three generated files
with `include_bytes!` at compile time and mounts each at exactly one path, with
no wildcard, prefix, manifest-generated, or fallback route:

| Route | Embedded file | Media type |
| --- | --- | --- |
| `/` | `dist/index.html` | `text/html; charset=utf-8` |
| `/assets/application.js` | `dist/assets/application.js` | `text/javascript; charset=utf-8` |
| `/assets/application.css` | `dist/assets/application.css` | `text/css; charset=utf-8` |

The module performs no filesystem read at runtime; every byte it can ever serve
is compiled into the binary. A request for any other path, including a
traversal-like or `/api/`-prefixed path, never reaches this allowlist and
cannot expose an arbitrary file.

## Build-Time Availability And Freshness

`build.rs` fails the Rust build with an actionable diagnostic when
`server/web-ui/dist/` or any of its three expected files is absent. It invokes
no package manager and performs no network access; it only reports the failing
files and the command that produces them. Generated build output is
deliberately not committed to version control, so a fresh checkout must build
the Web UI application before the Rust workspace can compile this crate.

Presence alone is not sufficient. A developer who builds the Web UI once, edits
its source, and then runs `cargo build` directly would otherwise embed stale
bytes with no signal. To close that path, the Web UI production build writes
`dist/build-manifest.json` recording the SHA-256 hash of every declared bundle
input and of each generated asset, and `build.rs` re-hashes both sets at compile
time. It fails closed on a missing, malformed, or non-object manifest, an
unrecognized format version or field, an added or removed bundle input, or any
hash mismatch. It also emits a `cargo:rerun-if-changed` entry for every bundle
input, the `src/` directory, the manifest, and the three generated assets, so
Cargo re-runs the check after a source edit rather than reusing a cached build.

The manifest is build metadata only. It is never embedded, never added to the
asset allowlist, and never reachable by any route, and this crate never writes
it. The [Web UI Application Design](../../clients/web-ui/web-ui-application-design.md)
owns the manifest format, the bundle-input inventory rule, and its own output
validation. `make -C server check` builds the frontend and writes the manifest
before it runs the Rust gates.

## Size Bounds

Every embedded asset is bounded twice: once at compile time and once at
runtime.

A `const _: () = assert!(...)` per asset fails compilation if the embedded file
is empty or exceeds its bound. Independently, the Server runtime's
`ResponseProfile` enforces the same bound as a maximum response body size when
serving the asset, so a bound violation cannot reach a client even if a future
change bypassed the compile-time assertion.

| Asset | Bound | Built size |
| --- | --- | --- |
| `index.html` | 16 KiB | 452 B |
| `assets/application.js` | 256 KiB | 191,481 B |
| `assets/application.css` | 64 KiB | 488 B |

Built sizes vary by change and are reported by the build's bundle-size
validator; the bounds above are fixed.

## Media Type And Security Headers

The Server runtime's `ResponseProfile` has exactly four variants: JSON, HTML,
JavaScript, and CSS. The profile alone determines the response media type,
security header block, and maximum body size; no header is forwarded from the
module's own response, and nothing is inferred from the request, a file
extension, or the body content.

Every asset response carries:

- `Content-Type` set to the asset's exact media type from the table above;
- `Content-Security-Policy: default-src 'none'; base-uri 'none'; object-src 'none'; frame-ancestors 'none'; form-action 'none'; script-src 'self'; style-src 'self'; connect-src 'self'`;
- `X-Content-Type-Options: nosniff`; and
- `Cache-Control: no-store`.

`Cache-Control: no-store` compensates for the build's fixed, unhashed asset
names: because the allowlist requires stable file names rather than
content-derived ones, the response disables caching instead of relying on a
changing name to invalidate a stale copy. Asset responses send no CORS header,
support no credentials or cookies, and are never compressed.

## Response Framing

Asset responses use the same connection-close framing as the
[Web UI Pre-Operational Status Surface](pre-operational-status-design.md#rejections-and-bounds):
no `Content-Length` header and no HTTP reason phrase. The direct TLS listener
sends `close_notify` after each asset response, and the response write and TLS
close use a bounded timeout, identically to the status and fixed error
responses served by the same listener.

## Lifecycle Availability

The Server runtime mounts these asset routes only after trusted lifecycle
classification reports an uninitialized deployment, with or without a selected
**[Application Database](../../glossary.md#applications-and-interfaces)**, the
same condition that exposes `/api/v1/status`. They are absent for an
Init-pending or Restore-pending deployment, after the deployment is sealed,
during normal operation, or after a failed startup classification. The
[Web UI Pre-Operational Status Surface](pre-operational-status-design.md#lifecycle-availability)
and the [Server Lifecycle Design](../../server/lifecycle/lifecycle-design.md)
own the authority for when these lifecycle outcomes apply.

## Path Rejection And No SPA Fallback

An unknown non-API path receives the Server's fixed JSON `404`
(`{"error":"not_found"}`); it is never served an asset and never reads a file.
A path under `/api/` always receives the fixed JSON `404` and is never served
as an asset, regardless of whether it matches an asset route pattern.

This module deliberately provides no single-page-application fallback: it does
not serve `index.html` for an unmatched path. No client-side route exists yet
in the application this module serves, so a fallback would only broaden the
set of paths that return `200` without a corresponding capability. A future
change that introduces client-side routing must revisit this behavior
deliberately; it must not be added as an incidental fix for an unmatched path
returning `404`.

## Related Documents

- [Web UI Pre-Operational Status Surface](pre-operational-status-design.md)
- [Web UI Application Design](../../clients/web-ui/web-ui-application-design.md)
- [Server Architecture Design](../../server/server-architecture-design.md)
- [Server Lifecycle Design](../../server/lifecycle/lifecycle-design.md)
- [Security Model](../../security-model.md)
- [Testing and Validation Policy](../../testing.md)
