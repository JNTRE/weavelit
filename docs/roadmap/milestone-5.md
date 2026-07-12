# Milestone 5: Build the Web UI

## Goals

- [ ] The initial **[Administrator](../glossary.md#identities-and-access)** can
  sign in using the local username and password established during
  **[Init](../glossary.md#states-and-requests)** before enrolling in MFA.
- [ ] A **[Human User](../glossary.md#identities-and-access)** with
  **[Group](../glossary.md#identities-and-access)**-granted Web UI
  **[Client Module](../glossary.md#applications-and-interfaces)** access can
  sign in to a self-service account area.
- [ ] **[Human Users](../glossary.md#identities-and-access)** using
  **[Local Authentication](../glossary.md#identities-and-access)** can change
  the password for their own account when granted Web UI Client Module access.
- [ ] A local Human User can enroll their own time-based one-time password
  (TOTP) MFA factor from the self-service account area by confirming their
  current password and a generated TOTP code. The Web UI never displays the
  TOTP secret after enrollment.
- [ ] The self-service account area shows a read-only summary of the Human
  User's **[Group](../glossary.md#identities-and-access)** memberships and
  effective Client Module, Service Module, and named
  **[Operation](../glossary.md#applications-and-interfaces)** grants, without
  exposing Service Module configuration, Service Connection details, provider
  identities, or credentials.
- [ ] A Human User without the
  **[Server Administration Permission](../glossary.md#identities-and-access)**
  cannot see Web UI administration navigation or pages; the Server rejects a
  direct request for an administrative function.
- [ ] An Administrator can enable or disable a
  **[Client Module](../glossary.md#applications-and-interfaces)**; its
  connection surface is unavailable while disabled.
- [ ] An Administrator can enable or disable a
  **[Service Module](../glossary.md#applications-and-interfaces)**; its
  **[Operations](../glossary.md#applications-and-interfaces)** are unavailable
  while disabled.
- [ ] An Administrator can enable or disable one or more
  **[Operations](../glossary.md#applications-and-interfaces)**; the disabled
  Operation is unavailable to every human user and
  **[Automation Identity](../glossary.md#identities-and-access)**.
- [ ] An Administrator can access account management.
- [ ] An Administrator can create a local human user account; the new user must
  change its password at the first Local Authentication login.
- [ ] An Administrator can enable or disable human user accounts.
- [ ] An Administrator can reset another local human user's password and require
  a password change at the user's next Local Authentication login.
- [ ] An Administrator can require MFA for another local Human User or reset
  that user's MFA enrollment through the Web UI. An Administrator cannot reset
  their own MFA enrollment through the Web UI.
- [ ] An Administrator who has enrolled in MFA must complete TOTP verification
  for the current session before requiring MFA for, or resetting MFA enrollment
  for, another local Human User.
- [ ] An Administrator can create **[Groups](../glossary.md#identities-and-access)**
  and add Human Users to one or more Groups.
- [ ] An Administrator can configure a Group's grants to Client Modules,
  Service Modules, named Operations, and the
  **[Server Administration Permission](../glossary.md#identities-and-access)**.
- [ ] An Administrator can configure the
  **[Weavelit Server](../glossary.md#applications-and-interfaces)** web listener
  IP address and port through the Web UI.

## Related Documents

- [Roadmap](../roadmap.md)
- [Vision](../vision.md)
- [Core Statements](../core-statements.md)
- [Security Model](../security-model.md)
- [Glossary](../glossary.md)
- [Open Questions](../open-questions.md)
