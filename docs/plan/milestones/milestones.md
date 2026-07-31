# Weavelit Milestones

The [Weavelit GitHub Milestones](https://github.com/JNTRE/weavelit/milestones)
are the authoritative source for milestone titles, summaries, goals, state,
dates, progress, and assigned issues. This document is a repository navigation
index only. If this index differs from GitHub, GitHub controls.

## Milestones

### Milestone Example: Example Milestone title

[Open GitHub Milestone NUMBER](https://github.com/JNTRE/weavelit/milestone/NUMBER).

This example demonstrates the local index format only and does not represent a
live milestone.

Briefly summarize the milestone's intended delivery outcome without copying its
goals, completion state, dates, progress, or assigned issues.

### Milestone 1: Core Server Application

[Open GitHub Milestone 1](https://github.com/JNTRE/weavelit/milestone/1).

This milestone establishes the **[Weavelit Server](../../glossary.md#applications-and-interfaces)**
foundation: one-time **[Init](../../glossary.md#states-and-requests)**, durable
application and log state, default-deny access, authenticated account
administration, encrypted backup and
**[Restore](../../glossary.md#states-and-requests)**, HTTPS operation, and a
reproducible development environment.

### Milestone 2: MFA Module - TOTP

[Open GitHub Milestone 2](https://github.com/JNTRE/weavelit/milestone/2).

This milestone delivers the compiled-in TOTP
**[MFA Module](../../glossary.md#applications-and-interfaces)** for protected
factor enrollment and verification, including replay rejection and safe
enablement and disablement under Server-owned MFA policy.

### Milestone 3: Client Module - Web UI

[Open GitHub Milestone 3](https://github.com/JNTRE/weavelit/milestone/3).

This milestone establishes the
**[Web UI](../../glossary.md#applications-and-interfaces)**
**[Client Module](../../glossary.md#applications-and-interfaces)** as the
Server-owned browser connection surface, including its restricted
**[Init](../../glossary.md#states-and-requests)** and
**[Restore](../../glossary.md#states-and-requests)** contracts and its normal
authenticated functions, with secure sessions, validated identity, shared
authorization enforcement, and credential-safe responses.

### Milestone 4: Service Module - Zendesk

[Open GitHub Milestone 4](https://github.com/JNTRE/weavelit/milestone/4).

This milestone delivers the Zendesk
**[Service Module](../../glossary.md#applications-and-interfaces)** reference
integration with one supported connection type, deliberate ticket operations,
server-side authorization, safe retry and failure behavior, credential
isolation, and auditable structured results.

### Milestone 5: Web UI - Admin and User Self-Service

[Open GitHub Milestone 5](https://github.com/JNTRE/weavelit/milestone/5).

This milestone delivers the
**[Web UI](../../glossary.md#applications-and-interfaces)** administration and
self-service experience, including sign-in, password and
**[Multifactor Authentication (MFA)](../../glossary.md#identities-and-access)**
workflows, access visibility, account and
**[Group](../../glossary.md#identities-and-access)** management, module and
**[Operation](../../glossary.md#applications-and-interfaces)** controls,
listener configuration, and repeatable frontend quality gates.

### Milestone 6: Client Module - Weavelit CLI

[Open GitHub Milestone 6](https://github.com/JNTRE/weavelit/milestone/6).

This milestone establishes the
**[Weavelit CLI](../../glossary.md#applications-and-interfaces)**
**[Client Module](../../glossary.md#applications-and-interfaces)** as an
authenticated, operations-only `/api/v1/` surface that translates validated
requests while preserving Server-owned identity, authorization, and credential
boundaries.

### Milestone 7: Weavelit CLI

[Open GitHub Milestone 7](https://github.com/JNTRE/weavelit/milestone/7).

This milestone delivers the separately packaged
**[Weavelit CLI](../../glossary.md#applications-and-interfaces)** experience for
signing in, signing out, invoking permitted
**[Operations](../../glossary.md#applications-and-interfaces)** through the CLI
**[Client Module](../../glossary.md#applications-and-interfaces)**, and receiving
structured results without exposing administrative functions.

### Milestone 8: MVP Package Build and Validation

[Open GitHub Milestone 8](https://github.com/JNTRE/weavelit/milestone/8).

This milestone produces and validates the MVP
**[Weavelit Server](../../glossary.md#applications-and-interfaces)** `.deb`
package and native
**[Weavelit CLI](../../glossary.md#applications-and-interfaces)** artifact,
including clean installation, **[Init](../../glossary.md#states-and-requests)**,
**[Restore](../../glossary.md#states-and-requests)**, service startup,
authenticated operation, and operator documentation on their supported
platforms.

### Milestone 9: Build an Additional Service Module

[Open GitHub Milestone 9](https://github.com/JNTRE/weavelit/milestone/9).

This milestone delivers an additional
**[Service Module](../../glossary.md#applications-and-interfaces)** selected
through a recorded decision, with one connection type, deliberate
**[Operations](../../glossary.md#applications-and-interfaces)**, server-side
authorization, safe provider behavior, credential isolation, and auditable
structured results.

### Milestone 10: Build Support for User-Associated Service Connections

[Open GitHub Milestone 10](https://github.com/JNTRE/weavelit/milestone/10).

This milestone lets a
**[Human User](../../glossary.md#identities-and-access)** establish a
user-associated
**[Service Connection](../../glossary.md#applications-and-interfaces)** through
the **[Web UI](../../glossary.md#applications-and-interfaces)** self-service area
while the Server retains authentication material and independently enforces
every applicable access grant.

### Milestone 11: Build Support for Automation Identities

[Open GitHub Milestone 11](https://github.com/JNTRE/weavelit/milestone/11).

This milestone introduces
**[Automation Identities](../../glossary.md#identities-and-access)** with
explicitly scoped, revocable, and expiring credentials, an active
**[Responsible Owner](../../glossary.md#identities-and-access)**, and
**[Audit Logs](../../glossary.md#applications-and-interfaces)** that preserve
both authenticated-principal and human-accountability context.

### Milestone 12: Build Support for External Authentication

[Open GitHub Milestone 12](https://github.com/JNTRE/weavelit/milestone/12).

This milestone adds optional OpenID Connect
**[External Authentication](../../glossary.md#identities-and-access)** while
preserving Server-validated identity,
**[Client Module](../../glossary.md#applications-and-interfaces)** and
**[Group](../../glossary.md#identities-and-access)** authorization, safe
rejection of invalid provider credentials, and continued
**[Local Authentication](../../glossary.md#identities-and-access)** support.

### Milestone 13: Build the Passkey MFA Module

[Open GitHub Milestone 13](https://github.com/JNTRE/weavelit/milestone/13).

This milestone delivers a passkey
**[MFA Module](../../glossary.md#applications-and-interfaces)** that enrolls and
verifies passkeys while the Server retains control of MFA policy, session
usability, authorization, and audit records.

### Milestone 14: Build Support for a Server OCI Image

[Open GitHub Milestone 14](https://github.com/JNTRE/weavelit/milestone/14).

This milestone packages the verified, prebuilt
**[Weavelit Server](../../glossary.md#applications-and-interfaces)** release
output as a production OCI image while preserving the single-process
application, shared pre-operational lifecycle,
**[Init](../../glossary.md#states-and-requests)**, and
**[Restore](../../glossary.md#states-and-requests)** boundaries and defining
secure deployment, persistence, provenance, upgrade, and rollback behavior.

## Related Documents

- [Technical Specification](../../spec.md)
- [Open Questions](../../open-questions.md)
- [Testing and Validation Policy](../../testing.md)
- [Issue Index](../issues/issues.md)
- [GitHub Project Standards](../project/project-standards.md)
