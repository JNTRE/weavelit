# Authorization Design

This document is the canonical destination for implementation-specific
authorization design for the **[Weavelit Server](../../glossary.md#applications-and-interfaces)**.
Binding application requirements remain in the
[Technical Specification](../../spec.md), and cross-cutting authorization
invariants remain in the [Security Model](../../security-model.md). This
document owns how the Server implements those requirements and invariants.

## Scope

This document owns how the Server evaluates a **[Human User](../../glossary.md#identities-and-access)**
request against **[Group](../../glossary.md#identities-and-access)** grants:
the grant model, the additive union that produces effective grants, the
separation of the **[Server Administration Permission](../../glossary.md#identities-and-access)**
from operational grants, the precedence in which requirements are checked, how
default-deny is made structural, and what is recorded when a request is denied.

It does not own credential validation, which belongs to the
[Server Authentication Design](../authentication/authentication-design.md), and
it does not own **[Automation Identity](../../glossary.md#identities-and-access)**
authorization, which belongs to the
[Automation Identity Design](../automation-identities/automation-identity-design.md).

This document also records two structural properties that depend on crates it
does not otherwise own: that a decision cannot run before session validation,
enforced by the Weavelit Server crate's `ValidatedSession`, and that a
successful decision is spent exactly once, enforced by
`weavelit-server-operation`. Service Connection selection and provider
execution themselves belong to `weavelit-server-operation`, not to this
document.

## Grant Model

A Group confers exactly four kinds of grant:

| Grant | Confers |
| --- | --- |
| **[Client Module](../../glossary.md#applications-and-interfaces)** | Reachability of one named Client Module |
| **[Service Module](../../glossary.md#applications-and-interfaces)** | Reachability of one named Service Module |
| **[Operation](../../glossary.md#applications-and-interfaces)** | Reachability of one exactly named Operation |
| Server Administration Permission | Eligibility for **[Administration Plane](../../glossary.md#applications-and-interfaces)** functions |

Groups are the only source of these grants. An account holds no grant of its
own, and no grant is implied by another: holding the Server Administration
Permission confers no Client Module, Service Module, or Operation grant, and
holding a Service Module grant confers no Operation of that Service Module.

There is no wildcard, prefix, or Service-Module-wide Operation grant. An
Operation grant matches one whole Operation name, so registering a new Operation
leaves it unreachable until a Group grants that exact name.

## Effective Grants And The Additive Union

A Human User's effective grants are the additive union of the grants of every
Group the account belongs to. The Application Database returns a narrow
authorization projection carrying only the account's active flag and the grants
joined across the account's memberships; the Server folds that projection into
effective grants.

The fold is purely additive:

- A grant is effective when at least one membership Group confers it.
- No Group can remove a grant another Group confers; there is no deny grant.
- The same grant conferred by several Groups is indistinguishable from that
  grant conferred by one, so overlapping Groups compose without ordering.
- An account with no membership has no effective grant of any kind.

Because the union is over grants rather than over decisions, adding a Group to
an account can only widen reachability and removing one can only narrow it.

The projection does not report which Group conferred which grant, so a decision
cannot depend on Group identity and a denial cannot disclose it.

## Separation Of The Server Administration Permission

Effective grants are held in two parts: the operational grants, which name a
reachable Client Module, Service Module, or Operation, and the Server
Administration Permission, which is a two-state value.

The **[User Plane](../../glossary.md#applications-and-interfaces)** evaluator
receives only the operational grants. The Server Administration Permission is
not a value it can read, so "an **[Administrator](../../glossary.md#identities-and-access)**
implies Operation grants" is not a statement the implementation can express,
rather than a rule it merely declines to apply. An Administrator holding only a
Client Module grant and the Server Administration Permission is therefore denied
every named Operation.

The Administration Plane evaluator reads both parts, because reaching that plane
still requires a grant to the Client Module the request arrives through.

## Requirement Precedence

Each decision checks its requirements left to right and denies at the first
failure: an inactive account, then a disabled or uncatalogued component, then a
missing grant. A component the catalog does not declare is treated exactly as a
disabled one.

### User Plane Operation

| Active | Client Module enabled and declares the User Plane | Service Module enabled | Operation enabled and owned by that Service Module | Client Module grant | Service Module grant | Operation grant | Result |
| --- | --- | --- | --- | --- | --- | --- | --- |
| No | - | - | - | - | - | - | Deny |
| Yes | No | - | - | - | - | - | Deny |
| Yes | Yes | No | - | - | - | - | Deny |
| Yes | Yes | Yes | No | - | - | - | Deny |
| Yes | Yes | Yes | Yes | No | - | - | Deny |
| Yes | Yes | Yes | Yes | Yes | No | - | Deny |
| Yes | Yes | Yes | Yes | Yes | Yes | No | Deny |
| Yes | Yes | Yes | Yes | Yes | Yes | Yes | Allow |

### Administration Plane Function

| Active | Client Module enabled and declares the Administration Plane | Client Module grant | Server Administration Permission | Result |
| --- | --- | --- | --- | --- |
| No | - | - | - | Deny |
| Yes | No | - | - | Deny |
| Yes | Yes | No | - | Deny |
| Yes | Yes | Yes | No | Deny |
| Yes | Yes | Yes | Yes | Allow |

No operational Service Module or Operation grant participates in the
Administration Plane chain.

The Client Module a request is evaluated against is the one the authenticated
session was established for, not one the request names, so Client Modules that
share an API surface cannot be interchanged by a caller.

## Structural Default-Deny

Default-deny is enforced by what the implementation can represent rather than by
convention:

- `authorize_user_operation` returns `AuthorizedOperation`, and
  `authorize_administration` returns `AuthorizedAdministration`. Both proof
  types live in the `weavelit-server-authorization` crate; every field and the
  only constructor of each are private to that crate, so a value of either type
  can exist only where the single successful branch of its evaluator produced
  it. Holding a proof is therefore evidence that the whole chain succeeded, not
  a claim a caller can make on its own.
- This is a compile-time property, and `tests/proof_construction.rs` pins it as
  one rather than asserting only that some fixture fails to compile: an
  external fixture that calls `AuthorizedOperation`'s private constructor is
  required to fail with the exact rustc code `E0624`, and a fixture that writes
  a struct literal for `AuthorizedAdministration` is required to fail with
  `E0451`. The test also checks that the failing span is the fixture's own
  forgery attempt, so the fixture can only pass by being rejected for forging a
  proof, not by failing to compile for an unrelated reason.
- Every match over a grant kind, over a plane, and over the Server
  Administration Permission is exhaustive with no wildcard arm, so adding a
  requirement variant fails to compile until each decision states how it treats
  it.
- An absent catalog entry is neither an error nor a permissive case; it is
  handled identically to a disabled entry.
- The two decisions are separate functions with separate proof types, so an
  Administration Plane result cannot be presented where an Operation result is
  required.

## Enforcement Order With Session Validation

The Server-side composition point is `AuthorizationRuntime`, in
`weavelit-server/src/authorization.rs`. Its two entry points, `authorize_operation`
and `authorize_administration`, each take a `ValidatedSession`: a type whose
constructor is private to the authentication module and whose only producer is
`AuthenticationRuntime::validated_session`. A request path therefore cannot
reach either authorization decision without first passing session validation;
there is no second constructor of `ValidatedSession` a shortcut could use, so an
authorization that skipped session validation is not a mistake this
implementation can make, it is code that does not compile.

The session is also authoritative over which **[Client Module](../../glossary.md#applications-and-interfaces)**
the decision runs against. `AuthorizationRuntime` denies a request that names a
Client Module other than the one the session was established for, so two
Client Modules that share an API surface cannot be interchanged by a caller
naming the other one in the request.

## Live Inputs And Why Caching Is Prohibited

`AuthorizationRuntime::live_inputs` reads `load_human_authorization` and
`load_component_enablement` from the **[Application Database](../../glossary.md#applications-and-interfaces)**
on every call, inside one acquisition of the database lane, so a decision
cannot combine a grant set read before an administrator's change with an
enablement read after it. Both results are returned by value to the single
decision that uses them and are dropped when that decision returns.

Nothing derived from either read is stored: not on `AuthorizationRuntime`, not
on `ValidatedSession`, not at login, and not in any other structure that
outlives one call. Caching either value would reopen exactly the window this
design closes: a cached grant set would keep granting access a Group change had
already revoked, and a cached enablement flag would keep denying or allowing
past the moment an administrator changed it, in both cases until whatever
invalidated the cache caught up. Reading live on every call removes that window
entirely, so a Group change or a component enablement change takes effect on
the very next request, with no re-login and no cache invalidation to race.

## Proof Consumption Is Spent Exactly Once

An `AuthorizedOperation` proof is not itself an entry into Service Connection
selection or provider execution; that boundary belongs to the
`weavelit-server-operation` crate. Two of its properties are load-bearing for
authorization's own guarantee that a decision is used at most once, so they are
recorded here even though this document does not own that crate:

- `SelectedServiceConnection::select` takes the `AuthorizedOperation` proof by
  value. Selection is therefore the point at which an authorization is spent:
  once a connection has been selected, the proof is gone and cannot be moved
  into a second `select` call to justify a second Operation or a second
  connection. Selection also refuses a connection that the authorized
  Operation's own Service Module does not own, and it carries no provider
  credential.
- `SelectedServiceConnection::execute` takes the selection by value, so a
  provider runs at most once per selection and, transitively, at most once per
  authorization.

`tests/proof_consumption.rs` pins both properties at compile time, the same way
the forbidden-proof fixture above pins construction: a fixture that passes a
borrowed proof to `select` is required to fail with `E0308`; a fixture that
tries to reuse a moved proof is required to fail with `E0382`; a fixture that
tries to reuse a moved selection is also required to fail with `E0382`; and a
fixture that writes a struct literal for `SelectedServiceConnection` fails
without a numbered rustc code at all, so the test pins its exact message
instead: "cannot construct `SelectedServiceConnection` with struct literal
syntax due to private fields".

## Denial Reporting

Every unsuccessful authorization returns one `AuthorizationDenied` value. There
is exactly one such value, so no branch reports which check failed: an
inactive account, a disabled or uncatalogued Client Module, Service Module, or
Operation, an Operation owned by a different Service Module, and every missing
grant are indistinguishable to the caller and in every rendering of the denial.

### Denial Response Contract

`weavelit-module-client`'s `authorization` module renders every denial as HTTP
`403` with exactly:

```json
{"error":"authorization_denied","correlation_id":"<opaque>"}
```

The body is byte-identical across every denial cause; only the correlation
identifier varies, and it is the same opaque value the System Log denial
record below carries. This is deliberately distinct from authentication's `401`
`session_invalid` response, owned by the
[Server Authentication Design](../authentication/authentication-design.md): a
request whose session fails validation never reaches an authorization decision
at all, so the two response contracts are never alternatives for one request;
they answer two different failures at two different points on the request
path.

### Denial Record

A denied request produces one fixed **[System Log](../../glossary.md#applications-and-interfaces)**
record whose content is owned by the
[Authorization-Denial System Log Record](../observability/authorization-denial-record-design.md).
Delivery is attempted before the denial is returned, and every failure inside
it is absorbed, so an unconfigured System Log destination or a delivery
failure can change what is recorded but can never turn a denial into an allow.

## MFA Re-Enablement Exception

The Administration Plane decision consults only the Client Module the request
arrived through and the Server Administration Permission; it never takes a
target component. An **[MFA Module](../../glossary.md#applications-and-interfaces)**'s
own enablement is not represented in the catalog this decision evaluates
against at all, so a disabled MFA Module cannot deny the Administration Plane
function that re-enables it: that function is authorized the same way every
other Administration Plane function is, with no reference to any component's
enablement. This satisfies the Administration Plane requirement in the
[Technical Specification](../../spec.md#multifactor-authentication) that
Administrators be able to configure MFA Module enablement, and is consistent
with the module starting disabled after Init as described in
[MFA Module Enablement](../authentication/authentication-design.md#mfa-module-enablement).

## Related Documents

- [Authorization-Denial System Log Record](../observability/authorization-denial-record-design.md)
- [Server Authentication Design](../authentication/authentication-design.md)
- [Server API Contract](../api/api-contract-design.md)
- [Automation Identity Design](../automation-identities/automation-identity-design.md)
- [Application Database Design](../database/application-database-design.md)
- [Log Module Design](../../log-modules/log-module-design.md)
- [Security Model](../../security-model.md)
- [Technical Specification](../../spec.md)
- [Glossary](../../glossary.md)
