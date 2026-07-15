# Milestone 5: Web UI - Admin and User Self-Service

## GitHub Milestone

Implementation progress is tracked in [GitHub Milestone 5](https://github.com/JNTRE/weavelit/milestone/5). Keep that GitHub Milestone aligned with this canonical document when this milestone's title, goals, or scope changes.

## Goals

- [ ] The Web UI uses project-local, version-pinned ESLint and Prettier configurations for its TypeScript and React source, with npm scripts that let contributors lint, check formatting, and apply formatting locally.
- [ ] The Server Makefile and a GitHub Actions workflow run the same read-only Web UI linting and formatting checks, so local and continuous-integration quality gates remain consistent.
- [ ] The initial **[Administrator](../../glossary.md#identities-and-access)** can sign in using the local username and password established during **[Init](../../glossary.md#states-and-requests)** before enrolling in MFA.
- [ ] A **[Human User](../../glossary.md#identities-and-access)** with **[Group](../../glossary.md#identities-and-access)**-granted Web UI **[Client Module](../../glossary.md#applications-and-interfaces)** access can sign in to a self-service account area.
- [ ] **[Human Users](../../glossary.md#identities-and-access)** using **[Local Authentication](../../glossary.md#identities-and-access)** can change the password for their own account when granted Web UI Client Module access.
- [ ] A local Human User can enroll their own time-based one-time password (TOTP) MFA factor from the self-service account area by confirming their current password and a generated TOTP code. The Web UI never displays the TOTP secret after enrollment.
- [ ] The self-service account area shows a read-only summary of the Human User's **[Group](../../glossary.md#identities-and-access)** memberships and effective Client Module, Service Module, and named **[Operation](../../glossary.md#applications-and-interfaces)** grants, without exposing Service Module configuration, Service Connection details, provider identities, or credentials.
- [ ] A Human User without the **[Server Administration Permission](../../glossary.md#identities-and-access)** cannot see Web UI administration navigation or pages; the Server rejects a direct request for an administrative function.
- [ ] An Administrator can enable or disable a **[Client Module](../../glossary.md#applications-and-interfaces)**; its connection surface is unavailable while disabled.
- [ ] An Administrator can enable or disable a **[Service Module](../../glossary.md#applications-and-interfaces)**; its **[Operations](../../glossary.md#applications-and-interfaces)** are unavailable while disabled.
- [ ] An Administrator can enable or disable one or more **[Operations](../../glossary.md#applications-and-interfaces)**; the disabled Operation is unavailable to every human user and **[Automation Identity](../../glossary.md#identities-and-access)**.
- [ ] An Administrator can access account management.
- [ ] An Administrator can create a local human user account; there is no self-registration or email-based invitation, and the new user must change its password at the first Local Authentication login.
- [ ] An Administrator can enable or disable human user accounts. Disabled accounts are not deleted.
- [ ] An Administrator can reset any local human user's password, including their own, and require a password change at the user's next Local Authentication login.
- [ ] An Administrator can require MFA for, or reset the MFA enrollment of, any local Human User, including themselves, through the Web UI. An MFA reset clears the prior factor and forces re-enrollment when MFA remains required.
- [ ] An Administrator who has enrolled in MFA must complete TOTP verification for the current session before requiring MFA or resetting an MFA enrollment, including their own.
- [ ] A Host Administrator can use the Admin CLI without an application session to perform the same local-account administration functions available to an Administrator through the Web UI, including clearing the MFA enrollment of the sole Administrator after an MFA lockout.
- [ ] An Administrator can create **[Groups](../../glossary.md#identities-and-access)** and add Human Users to one or more Groups.
- [ ] An Administrator can configure a Group's grants to Client Modules, Service Modules, named Operations, and the **[Server Administration Permission](../../glossary.md#identities-and-access)**.
- [ ] An Administrator can configure the **[Weavelit Server](../../glossary.md#applications-and-interfaces)** web listener IP address and port through the Web UI.

## Related Documents

- [Roadmap](../../roadmap.md)
- [Vision](../../vision.md)
- [Core Statements](../../core-statements.md)
- [Security Model](../../security-model.md)
- [Glossary](../../glossary.md)
- [Open Questions](../../open-questions.md)
