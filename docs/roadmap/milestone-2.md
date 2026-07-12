# Milestone 2: Build the TOTP MFA Module

## Goals

- [ ] The TOTP **[MFA Module](../glossary.md#applications-and-interfaces)** is compiled into the Weavelit Server package, registered with the Server, and enabled by default after Init. An Administrator can enable or disable it through server-administration functions.
- [ ] The TOTP MFA Module uses maintained, standards-compliant TOTP libraries to generate and verify TOTP factors without exposing its implementation library directly to Client Modules or client applications.
- [ ] The TOTP MFA Module generates a unique TOTP secret and provisioning value for a local **[Human User](../glossary.md#identities-and-access)** enrollment. The provisioning value is available only during that Human User's enrollment and is not returned after enrollment completes.
- [ ] The TOTP MFA Module activates an enrollment only after the enrolling Human User confirms a valid generated TOTP code, and it securely stores the resulting factor data in the Server's trusted environment.
- [ ] The TOTP MFA Module verifies valid TOTP codes and rejects invalid, expired, or replayed codes. It returns a typed verification result to the Server without disclosing the TOTP secret or raw implementation errors.
- [ ] Disabling the TOTP MFA Module immediately prevents new TOTP enrollment and verification. The Server applies the defined affected-user reporting, session termination, and MFA-policy behavior.

## Related Documents

- [Roadmap](../roadmap.md)
- [Vision](../vision.md)
- [Core Statements](../core-statements.md)
- [Security Model](../security-model.md)
- [Glossary](../glossary.md)
- [Open Questions](../open-questions.md)
