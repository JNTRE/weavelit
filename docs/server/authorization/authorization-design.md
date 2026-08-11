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

- Each decision returns a proof value whose fields and constructor are private
  to the authorization crate. Only the single successful branch of an evaluator
  constructs one, so a caller holding a proof holds evidence that the whole
  chain succeeded and cannot mint one. A compile-fixture test asserts that an
  external crate fails to compile when it calls the private constructor or
  writes a struct literal for a proof.
- Every match over a grant kind, over a plane, and over the Server
  Administration Permission is exhaustive with no wildcard arm, so adding a
  requirement variant fails to compile until each decision states how it treats
  it.
- An absent catalog entry is neither an error nor a permissive case; it is
  handled identically to a disabled entry.
- The two decisions are separate functions with separate proof types, so an
  Administration Plane result cannot be presented where an Operation result is
  required.

## Denial Reporting

Every unsuccessful authorization returns one denial value. There is exactly one
such value, so no branch reports which check failed: an inactive account, a
disabled or uncatalogued Client Module, Service Module, or Operation, an
Operation owned by a different Service Module, and every missing grant are
indistinguishable to the caller and in every rendering of the denial.

A denied request produces one fixed **[System Log](../../glossary.md#applications-and-interfaces)**
record whose content is owned by the
[Authorization-Denial System Log Record](../observability/authorization-denial-record-design.md).

## Related Documents

- [Authorization-Denial System Log Record](../observability/authorization-denial-record-design.md)
- [Server Authentication Design](../authentication/authentication-design.md)
- [Automation Identity Design](../automation-identities/automation-identity-design.md)
- [Application Database Design](../database/application-database-design.md)
- [Log Module Design](../../log-modules/log-module-design.md)
- [Security Model](../../security-model.md)
- [Technical Specification](../../spec.md)
- [Glossary](../../glossary.md)
