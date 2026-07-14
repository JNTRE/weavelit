# Milestone 3: Build the Web UI Client Module

## Goals

- [ ] The **[Web UI](../glossary.md#applications-and-interfaces)** **[Client Module](../glossary.md#applications-and-interfaces)** is registered with the Weavelit Server and mounts its browser-facing route namespace on the configured HTTPS listener.
- [ ] An **[Administrator](../glossary.md#identities-and-access)** can enable or disable the Web UI Client Module; when disabled, its browser routes and sessions are unavailable.
- [ ] The Web UI Client Module uses secure, server-managed browser sessions and supports session termination.
- [ ] The Web UI Client Module derives the **[Human User](../glossary.md#identities-and-access)** identity from the Server-managed session and never trusts identity, group, or permission claims supplied by the browser.
- [ ] A Human User must have Web UI Client Module access through a **[Group](../glossary.md#identities-and-access)** before the module permits access.
- [ ] Every request entering through the Web UI Client Module is passed to the Server's shared authorization policy, including self-service, group-scoped, and server-administration access classes.
- [ ] The Web UI Client Module never exposes provider credentials, automation credentials, or internal error traces to the browser.

## Related Documents

- [Roadmap](../roadmap.md)
- [Vision](../vision.md)
- [Core Statements](../core-statements.md)
- [Security Model](../security-model.md)
- [Glossary](../glossary.md)
- [Open Questions](../open-questions.md)
