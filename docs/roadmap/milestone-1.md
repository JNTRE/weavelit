# Milestone 1: Core Server Application

## GitHub Milestone

Implementation progress is tracked in [GitHub Milestone 1](https://github.com/JNTRE/weavelit/milestone/1). Keep that GitHub Milestone aligned with this canonical document when this milestone's title, goals, or scope changes.

## Goals

- [ ] A **[Host Administrator](../glossary.md#identities-and-access)** can use the **[Admin CLI](../glossary.md#applications-and-interfaces)** to complete **[Init](../glossary.md#states-and-requests)** interactively or from an explicit non-interactive bootstrap configuration file, create the **[Administrators Group](../glossary.md#identities-and-access)**, create the first local **[Human User](../glossary.md#identities-and-access)** without MFA enrollment, and add that user to the Administrators Group.
- [ ] The non-interactive Admin CLI bootstrap uses the same Server-owned Init logic as interactive Init, runs only against uninitialized Server state, and reads sensitive bootstrap values only from local files referenced by its configuration file. It does not accept sensitive bootstrap values through environment variables or log or persist those values or the configuration.
- [ ] The Administrators Group grants the Web UI **[Client Module](../glossary.md#applications-and-interfaces)** and the **[Server Administration Permission](../glossary.md#identities-and-access)**, but no named Operations.
- [ ] The **[Weavelit Server](../glossary.md#applications-and-interfaces)** applies default-deny authorization using additive Group grants and global availability gates. A disabled Human User, Client Module, **[Service Module](../glossary.md#applications-and-interfaces)**, or **[Operation](../glossary.md#applications-and-interfaces)** is unavailable regardless of otherwise effective Group grants.
- [ ] A Host Administrator can use the Admin CLI to set the Weavelit Server listening IP address.
- [ ] A Host Administrator can start and stop the Weavelit Server process successfully.
- [ ] Init selects and configures the Server's **[Application Database](../glossary.md#applications-and-interfaces)**. The MVP SQLite backend is implemented as a dedicated Rust crate behind the Server's internal Application Database backend contract, is separate from the Log Module destination, and persists required Server state across a restart, including accounts, Groups and their grants, Client Module and Service Module enablement, configuration, **[Service Connections](../glossary.md#applications-and-interfaces)**, and active server-managed user sessions.
- [ ] During Init, a Host Administrator selects, configures, and activates one or more **[Log Modules](../glossary.md#applications-and-interfaces)**, then assigns configured Log Modules separately to System Logs and Audit Logs. The same Log Module may receive both types. Init does not complete, and the Server does not start normal operation, until both assignments validate and the Audit Log assignment can durably record Audit Logs.
- [ ] Init creates a backup recovery key pair, retains only its public key in the Server, and presents the private key once for the **[Host Administrator](../glossary.md#identities-and-access)** to retain outside Weavelit. The private key is separate from Server-local at-rest key material and is never retained by the Server.
- [ ] An **[Administrator](../glossary.md#identities-and-access)** can create and download a versioned, encrypted Application Database backup through server-administration functions. The backup contains the application state needed for operational recovery, including password verifiers, protected MFA factor data, and Service Connection credentials, and is encrypted for the retained public recovery key.
- [ ] A Host Administrator can use the Admin CLI and the retained private recovery key to validate and import a compatible Application Database backup into a separately initialized Server after that Server's Application Database is selected and configured. Import replaces application state atomically, invalidates active sessions, and re-encrypts restored secret material with the replacement Server's local at-rest key. The workflow does not support in-place migration between Application Database technologies.
- [ ] A Host Administrator can use the Admin CLI without an application session to reset any local Human User's password and require a password change at the user's next **[Local Authentication](../glossary.md#identities-and-access)** login, including the sole Administrator after an account lockout.
- [ ] A Host Administrator can use the Admin CLI without an application session to reset MFA enrollment for any local Human User, including the sole Administrator after an MFA lockout. The reset immediately invalidates the prior enrollment, forces re-enrollment when MFA remains required, and the Admin CLI never displays or replaces the user's MFA secret.
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
