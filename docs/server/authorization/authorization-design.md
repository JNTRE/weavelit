# Authorization Design

This document is the canonical destination for implementation-specific
authorization design for the **[Weavelit Server](../../glossary.md#applications-and-interfaces)**.
Binding application requirements remain in the
[Technical Specification](../../spec.md), and cross-cutting authorization
invariants remain in the [Security Model](../../security-model.md). This
document owns how the Server implements those requirements and invariants.

## Current Scope

This document will cover the implementation design for permission evaluation,
**[Group](../../glossary.md#identities-and-access)** grants, access classes, and
default-deny enforcement. No implementation-specific decisions have moved here
yet.

## Related Documents

- [Security Model](../../security-model.md)
- [Technical Specification](../../spec.md)
- [Glossary](../../glossary.md)
