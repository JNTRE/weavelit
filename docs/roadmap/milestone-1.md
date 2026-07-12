# Milestone 1: Build the Core Server Application

## Goals

- [ ] A **[Host Administrator](../glossary.md#identities-and-access)** can use the **[Admin CLI](../glossary.md#applications-and-interfaces)** to complete **[Init](../glossary.md#states-and-requests)** interactively or from an explicit non-interactive bootstrap configuration file, create the **[Administrators Group](../glossary.md#identities-and-access)**, create the first local **[Human User](../glossary.md#identities-and-access)** without MFA enrollment, and add that user to the Administrators Group.
- [ ] The non-interactive Admin CLI bootstrap uses the same Server-owned Init logic as interactive Init, runs only against uninitialized Server state, and reads sensitive bootstrap values only from local files referenced by its configuration file. It does not accept sensitive bootstrap values through environment variables or log or persist those values or the configuration.
- [ ] The Administrators Group grants the Web UI **[Client Module](../glossary.md#applications-and-interfaces)** and the **[Server Administration Permission](../glossary.md#identities-and-access)**, but no named Operations.
- [ ] The **[Weavelit Server](../glossary.md#applications-and-interfaces)** applies default-deny authorization using additive Group grants and global availability gates. A disabled Human User, Client Module, **[Service Module](../glossary.md#applications-and-interfaces)**, or **[Operation](../glossary.md#applications-and-interfaces)** is unavailable regardless of otherwise effective Group grants.
- [ ] A Host Administrator can use the Admin CLI to set the Weavelit Server listening IP address.
- [ ] A Host Administrator can start and stop the Weavelit Server process successfully.
- [ ] Init selects and configures the Server's **[Application Database](../glossary.md#applications-and-interfaces)**. The MVP Application Database uses SQLite, is separate from the Log Module destination, and persists required Server state across a restart, including accounts, Groups and their grants, Client Module and Service Module enablement, configuration, **[Service Connections](../glossary.md#applications-and-interfaces)**, and active server-managed user sessions.
- [ ] A Host Administrator can use the Admin CLI to export a configuration backup and import it into a separately initialized Server after that Server's Application Database is selected and configured. The workflow does not support in-place migration between Application Database technologies.
- [ ] A Host Administrator can use the Admin CLI to reset a local human user's password and require a password change at the user's next **[Local Authentication](../glossary.md#identities-and-access)** login.
- [ ] A Host Administrator can use the Admin CLI to reset MFA enrollment for any local Human User, including themselves. The reset immediately invalidates the prior enrollment, and the Admin CLI never displays or replaces the user's MFA secret.
- [ ] The Server stores every local human password set or reset through Init, the Admin CLI, or the Web UI in accordance with the [Security Model](../security-model.md#authentication): using a modern adaptive password-hashing algorithm from a maintained library, never in plaintext or reversibly encrypted.
- [ ] The Server evaluates each local Human User's MFA policy, enabled **[MFA Modules](../glossary.md#applications-and-interfaces)**, and verified MFA factors before granting a usable session. A Human User whose account requires MFA cannot obtain a usable session until they verify an enabled MFA method.
- [ ] **[Init](../glossary.md#states-and-requests)** generates and uses a self-signed TLS certificate by default.
- [ ] One configured HTTPS listener serves both the **[Web UI](../glossary.md#applications-and-interfaces)** browser routes and authenticated `/api/v1/` routes.
- [ ] A developer can use a documented process to build and use an OCI-compliant development image to build, run, test, and restart the Weavelit Server without installing Rust on the host. The development environment receives non-secret configuration through environment variables, mounts bootstrap secrets as local files, and preserves Server state in an explicitly managed development volume.

## Related Documents

- [Roadmap](../roadmap.md)
- [Vision](../vision.md)
- [Core Statements](../core-statements.md)
- [Security Model](../security-model.md)
- [Glossary](../glossary.md)
- [Open Questions](../open-questions.md)
