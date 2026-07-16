# Production Container Design

## Purpose

The production image will provide the supported OCI deployment artifact for the
**[Weavelit Server](../../glossary.md#applications-and-interfaces)** after its
packaged release workflow is verified.

## Current Boundary

The production image is reserved for Milestone 14 and must not be implemented
as a development-image mode. Its Containerfile remains a non-runnable
placeholder until the release package, image provenance, and production
deployment contract are defined.

The placeholder's `org.opencontainers.image.description` label points to this
document.

When implemented, the production image must:

- run a verified packaged Server artifact without compiling source code at
  container startup;
- exclude Rust, Cargo, source code, test tooling, and build dependencies;
- document and test host administration, persistent state and backups, TLS
  termination, non-secret configuration, secret injection, provenance, upgrade,
  and rollback boundaries; and
- follow the supported OCI image deployment requirements established for the
  release.

## Related Documents

- [Milestone 14](../../plan/milestones/milestone-14.md)
- [Development Container Design](../dev/development-container-design.md)
- [Open Questions](../../open-questions.md)
