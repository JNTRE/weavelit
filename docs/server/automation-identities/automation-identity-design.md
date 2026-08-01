# Automation Identity Design

This document is the canonical destination for implementation-specific design
for **[Automation Identities](../../glossary.md#identities-and-access)** in the
**[Weavelit Server](../../glossary.md#applications-and-interfaces)**. Binding
application requirements remain in the [Technical Specification](../../spec.md),
and cross-cutting credential and accountability invariants remain in the
[Security Model](../../security-model.md). This document owns how the Server
implements those requirements and invariants.

## Current Scope

This document will cover Automation Identity lifecycle, credential management,
**[Responsible Owner](../../glossary.md#identities-and-access)** enforcement,
named **[Operation](../../glossary.md#applications-and-interfaces)** scopes,
revocation, expiration, and accountability integration. No
implementation-specific decisions have moved here yet.

## Related Documents

- [Security Model](../../security-model.md)
- [Technical Specification](../../spec.md)
- [Glossary](../../glossary.md)
- [Authentication Design](../authentication/authentication-design.md)
- [Authorization Design](../authorization/authorization-design.md)
