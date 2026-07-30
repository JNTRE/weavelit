# Milestone 12: Build Support for External Authentication

## GitHub Milestone

Implementation progress is tracked in [GitHub Milestone 12](https://github.com/JNTRE/weavelit/milestone/12). Keep that GitHub Milestone aligned with this canonical document when this milestone's title, goals, or scope changes.

## Goals

- [ ] An **[Administrator](../../glossary.md#identities-and-access)** can configure an external OpenID Connect identity provider for **[External Authentication](../../glossary.md#identities-and-access)**.
- [ ] A **[Human User](../../glossary.md#identities-and-access)** authenticated through the configured identity provider can access the **[Weavelit Server](../../glossary.md#applications-and-interfaces)** only through the same **[Client Module](../../glossary.md#applications-and-interfaces)** availability and **[Group](../../glossary.md#identities-and-access)** grant rules that apply to locally authenticated Human Users.
- [ ] The Server derives an externally authenticated Human User's identity from credentials validated against the configured identity provider and never accepts client-supplied identity, Group, or permission claims as authorization.
- [ ] External Authentication remains optional. An absent or unavailable provider, or invalid, expired, or otherwise rejected external credentials, does not produce a usable session or prevent supported **[Local Authentication](../../glossary.md#identities-and-access)**.

## Related Documents

- [Vision](../../vision.md)
- [Core Statements](../../core-statements.md)
- [Security Model](../../security-model.md)
- [Glossary](../../glossary.md)
- [Open Questions](../../open-questions.md)
- [Authentication Design](../../server/authentication/authentication-design.md)
- [Testing and Validation Policy](../../testing.md)
