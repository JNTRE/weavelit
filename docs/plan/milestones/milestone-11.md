# Milestone 11: Build Support for Automation Identities

## GitHub Milestone

Implementation progress is tracked in [GitHub Milestone 11](https://github.com/JNTRE/weavelit/milestone/11). Keep that GitHub Milestone aligned with this canonical document when this milestone's title, goals, or scope changes.

## Goals

- [ ] An **[Administrator](../../glossary.md#identities-and-access)** can create and manage an **[Automation Identity](../../glossary.md#identities-and-access)**, including its credentials and named **[Operation](../../glossary.md#applications-and-interfaces)** scopes.
- [ ] Each Automation Identity has an active **[Responsible Owner](../../glossary.md#identities-and-access)** who is a **[Human User](../../glossary.md#identities-and-access)**. Responsibility does not grant authority to change the Automation Identity's permissions or credentials.
- [ ] Automation credentials grant only the explicitly assigned named Operations. An Administrator can revoke a credential or configure its expiration; after revocation or expiration, it cannot authenticate or authorize an **[Operational Request](../../glossary.md#states-and-requests)**.
- [ ] **[Audit Logs](../../glossary.md#applications-and-interfaces)** for an Automation Identity's actions identify both the authenticated Automation Identity and its Responsible Owner.

## Related Documents

- [Vision](../../vision.md)
- [Core Statements](../../core-statements.md)
- [Security Model](../../security-model.md)
- [Glossary](../../glossary.md)
- [Open Questions](../../open-questions.md)
- [Automation Identity Design](../../server/automation-identities/automation-identity-design.md)
- [Testing and Validation Policy](../../testing.md)
