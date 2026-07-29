# Milestone 14: Build Support for a Server OCI Image

## Goals

- [ ] A supported OCI-compliant production image contains and runs the same versioned, prebuilt **[Weavelit Server](../../glossary.md#applications-and-interfaces)** release output used to assemble the `.deb` package, without installing the `.deb` at any image-build or runtime stage and without compiling the Server at container startup.
- [ ] The production image runs the Server as one application process and preserves the Server-owned **[Init](../../glossary.md#states-and-requests)** and application-configuration boundaries rather than introducing independently deployed services or a container-specific configuration surface.
- [ ] The production OCI image deployment has documented and tested boundaries for host administration, persistent Server state and backups, TLS termination, non-secret configuration, secret injection, image provenance, and upgrade and rollback.

## Related Documents

- [Vision](../../vision.md)
- [Core Statements](../../core-statements.md)
- [Security Model](../../security-model.md)
- [Glossary](../../glossary.md)
- [Open Questions](../../open-questions.md)
