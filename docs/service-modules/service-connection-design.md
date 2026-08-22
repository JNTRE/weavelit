# Service Connection Design

This document defines the shared authentication boundary for **[Service
Connections](../glossary.md#applications-and-interfaces)**. It records accepted
connection-authentication methods without defining provider-specific contracts
or client presentation workflows.

## Represented Areas

| Type | Link |
| --- | --- |
| Folder | [Service Module documentation](.) |
| Canonical document | [Technical Specification](../spec.md) |
| Canonical document | [Glossary](../glossary.md) |

## Scope And Ownership

This document owns authentication methods shared across Service Modules. A
provider-specific Service Module design owns the provider's requested scopes,
connection lifecycle, polling behavior, and supported Operations. Client
applications and Client Modules own the presentation of any interactive
connection-establishment workflow.

## Accepted Authentication Methods

A **[Service Connection](../glossary.md#applications-and-interfaces)** MAY use
an API key or OAuth authorization. An OAuth Service Connection MAY be
established through the OAuth 2.0 Device Authorization Grant ([RFC
8628](https://www.rfc-editor.org/rfc/rfc8628)).

The Device Authorization Grant is an OAuth authorization-establishment method,
not a distinct persisted Service Connection type. The resulting connection
remains either shared or associated with one **[Human
User](../glossary.md#identities-and-access)**, as its Service Module declares.

## Related Documents

- [Technical Specification](../spec.md)
- [Glossary](../glossary.md)
- [Security Model](../security-model.md)
