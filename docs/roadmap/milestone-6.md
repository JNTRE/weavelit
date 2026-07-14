# Milestone 6: Client Module - Operations CLI

## Goals

- [ ] The **[Weavelit CLI](../glossary.md#applications-and-interfaces)** **[Client Module](../glossary.md#applications-and-interfaces)** is registered with the **[Weavelit Server](../glossary.md#applications-and-interfaces)** and mounts its authenticated request namespace under `/api/v1/` on the configured HTTPS listener.
- [ ] An **[Administrator](../glossary.md#identities-and-access)** can enable or disable the Operations CLI Client Module; when disabled, its API routes are unavailable.
- [ ] The Operations CLI Client Module authenticates the caller with Server-validated credentials, derives the caller identity from those credentials, and never trusts identity, group, or permission claims supplied by the Weavelit CLI.
- [ ] A **[Human User](../glossary.md#identities-and-access)** must have Operations CLI Client Module access through a **[Group](../glossary.md#identities-and-access)** before the module permits access.
- [ ] Every request entering through the Operations CLI Client Module is translated into a validated **[Operational Request](../glossary.md#states-and-requests)** for a supported **[Operation](../glossary.md#applications-and-interfaces)** and is passed to the Server's shared authorization policy.
- [ ] The Operations CLI Client Module permits operations-only access and does not accept Weavelit CLI credentials for administrative functions.
- [ ] The Operations CLI Client Module never exposes provider credentials, automation credentials, or internal error traces to the Weavelit CLI.

## Related Documents

- [Roadmap](../roadmap.md)
- [Vision](../vision.md)
- [Core Statements](../core-statements.md)
- [Security Model](../security-model.md)
- [Glossary](../glossary.md)
- [Open Questions](../open-questions.md)
