# Milestone 4: Build the Zendesk Service Module

## Goals

- [ ] The Zendesk **[Service Module](../glossary.md#applications-and-interfaces)** declares one supported **[Service Connection](../glossary.md#applications-and-interfaces)** type and its setup workflow, and can use a configured connection of that type with Zendesk.
- [ ] A Service Connection determines the external Zendesk identity used but does not grant caller access; a **[Human User](../glossary.md#identities-and-access)** must have **[Group](../glossary.md#identities-and-access)** grants to the Zendesk Service Module and the named **[Operation](../glossary.md#applications-and-interfaces)**.
- [ ] The Server validates and authorizes each requested Zendesk Operation, including applicable Client Module, Human User, Zendesk Service Module, and named Operation availability and required Group grants, before contacting Zendesk. A malformed, unsupported, unavailable, or unauthorized request sends no Zendesk API request.
- [ ] Unavailable or failed Zendesk Service Connection authentication stops the requested Operation safely and never starts an interactive provider login from the Operations CLI.
- [ ] The Zendesk Service Module exposes named, validated Operations to create tickets, add comments to existing tickets, and close tickets.
- [ ] Each Zendesk write Operation has defined retry and duplicate-protection behavior and is safe to retry or protected against creating duplicate tickets, duplicate comments, or unintended ticket state changes.
- [ ] Each supported Zendesk Operation sends the appropriate Zendesk API request and returns a structured success or failure result.
- [ ] Zendesk credentials remain server-owned and are never exposed in client results or audit records.
- [ ] Successful and failed Zendesk Operations produce the required Server audit records.

## Related Documents

- [Roadmap](../roadmap.md)
- [Vision](../vision.md)
- [Core Statements](../core-statements.md)
- [Security Model](../security-model.md)
- [Glossary](../glossary.md)
- [Open Questions](../open-questions.md)
