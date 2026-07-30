# Milestone 9: Build an Additional Service Module

## GitHub Milestone

Implementation progress is tracked in [GitHub Milestone 9](https://github.com/JNTRE/weavelit/milestone/9). Keep that GitHub Milestone aligned with this canonical document when this milestone's title, goals, or scope changes.

## Goals

- [ ] The additional **[Service Module](../../glossary.md#applications-and-interfaces)** selected through a recorded decision defines and implements one supported **[Service Connection](../../glossary.md#applications-and-interfaces)** type and setup workflow, its named **[Operations](../../glossary.md#applications-and-interfaces)**, required permissions and configuration, retry and rate-limit behavior, error behavior, safety tests, and maintenance responsibility.
- [ ] A Service Connection determines the external provider identity used but does not grant caller access; a **[Human User](../../glossary.md#identities-and-access)** must have **[Group](../../glossary.md#identities-and-access)** grants to the additional Service Module and the named Operation.
- [ ] The **[Weavelit Server](../../glossary.md#applications-and-interfaces)** validates and authorizes each requested Operation before contacting the provider. A malformed, unsupported, unavailable, or unauthorized request sends no provider request.
- [ ] Unavailable or failed Service Connection authentication stops the requested Operation safely and never starts an interactive provider login from the **[Weavelit CLI](../../glossary.md#applications-and-interfaces)**.
- [ ] Each supported Operation sends the appropriate provider request and returns a structured success or failure result with a correlation identifier.
- [ ] Each write Operation is safe to retry or protected against duplicate or unintended provider changes, and defined rate-limit and provider failures fail safely.
- [ ] Provider credentials remain server-owned and are never exposed in client results, **[System Logs](../../glossary.md#applications-and-interfaces)**, or **[Audit Logs](../../glossary.md#applications-and-interfaces)**.
- [ ] Successful and failed Operations produce the required Server audit records.

## Related Documents

- [Vision](../../vision.md)
- [Core Statements](../../core-statements.md)
- [Security Model](../../security-model.md)
- [Glossary](../../glossary.md)
- [Open Questions](../../open-questions.md)
- [Testing and Validation Policy](../../testing.md)
