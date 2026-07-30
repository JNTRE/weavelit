# Milestone 10: Build Support for User-Associated Service Connections

## GitHub Milestone

Implementation progress is tracked in [GitHub Milestone 10](https://github.com/JNTRE/weavelit/milestone/10). Keep that GitHub Milestone aligned with this canonical document when this milestone's title, goals, or scope changes.

## Goals

- [ ] A **[Human User](../../glossary.md#identities-and-access)** with **[Web UI](../../glossary.md#applications-and-interfaces)** **[Client Module](../../glossary.md#applications-and-interfaces)** access and a **[Group](../../glossary.md#identities-and-access)** grant to a **[Service Module](../../glossary.md#applications-and-interfaces)** can identify from the self-service account area when that Service Module uses a user-associated **[Service Connection](../../glossary.md#applications-and-interfaces)** type.
- [ ] The self-service account area provides the Service Module's declared user-associated Service Connection setup workflow, such as API-key entry or OAuth authorization, only to the associated Human User. It does not provide setup for shared Service Connections.
- [ ] The **[Weavelit Server](../../glossary.md#applications-and-interfaces)** receives, stores, and uses the resulting authentication material. The material is never returned to, retained by, or otherwise disclosed to the Web UI or another Human User, and is never recorded in **[System Logs](../../glossary.md#applications-and-interfaces)** or **[Audit Logs](../../glossary.md#applications-and-interfaces)**.
- [ ] A user-associated Service Connection determines which external identity performs an approved **[Operation](../../glossary.md#applications-and-interfaces)** but does not grant access; the Server still enforces the caller's applicable Client Module, Service Module, and named Operation grants.

## Related Documents

- [Vision](../../vision.md)
- [Core Statements](../../core-statements.md)
- [Security Model](../../security-model.md)
- [Glossary](../../glossary.md)
- [Open Questions](../../open-questions.md)
- [Testing and Validation Policy](../../testing.md)
