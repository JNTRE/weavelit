# Milestone 7: Build the Operations CLI

## Goals

- [ ] A **[Human User](../glossary.md#identities-and-access)** can sign in to the **[Operations CLI](../glossary.md#applications-and-interfaces)** only when a **[Group](../glossary.md#identities-and-access)** grants access to the Operations CLI **[Client Module](../glossary.md#applications-and-interfaces)**.
- [ ] A Human User can sign out of the Operations CLI; subsequent requests are not permitted through the Operations CLI Client Module until the user signs in again.
- [ ] The Operations CLI uses `/api/v1/` routes on the configured HTTPS listener to submit supported **[Operations](../glossary.md#applications-and-interfaces)**.
- [ ] An **[Administrator](../glossary.md#identities-and-access)** with Group grants to the Operations CLI Client Module and a named Operation can use the Operations CLI, but its **[Server Administration Permission](../glossary.md#identities-and-access)** does not provide Operations CLI access or administrative functions through the client.
- [ ] A Human User with Group grants to an enabled **[Service Module](../glossary.md#applications-and-interfaces)** and a named Operation can invoke that Operation through the Operations CLI when the applicable configured **[Service Connection](../glossary.md#applications-and-interfaces)** of that Service Module's one supported type is authenticated.
- [ ] A Human User with the required grants can invoke a supported Operation through the Operations CLI and receive the expected structured result.

## Related Documents

- [Roadmap](../roadmap.md)
- [Vision](../vision.md)
- [Core Statements](../core-statements.md)
- [Security Model](../security-model.md)
- [Glossary](../glossary.md)
- [Open Questions](../open-questions.md)
