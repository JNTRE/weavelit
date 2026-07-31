# Production Container Design

## Purpose

The production image will provide the supported OCI deployment artifact for the
**[Weavelit Server](../../glossary.md#applications-and-interfaces)** after its
packaged release workflow is verified. It is a sibling delivery wrapper around
the same versioned, prebuilt Server release output used to assemble the `.deb`
package, not a separate Server build or application architecture.

## Current Boundary

The production image is reserved for Milestone 14 and must not be implemented
as a development-image mode. Its Containerfile remains a non-runnable
placeholder until the packaged release output, image provenance, and production
deployment contract are defined.

The placeholder's `org.opencontainers.image.description` label points to this
document.

When implemented, the production image must:

- contain and run the same verified, prebuilt Server release output used to
  assemble the `.deb` package, without installing the `.deb` at any image-build
  or runtime stage and without compiling source code at container startup;
- preserve the Server-owned
  **[Init](../../glossary.md#states-and-requests)** and
  **[Restore](../../glossary.md#states-and-requests)** boundaries and their
  shared lifecycle; container inputs supply only the host and process settings
  needed to start in restricted uninitialized mode and must not select the
  **[Application Database](../../glossary.md#applications-and-interfaces)** or
  create an alternative Init or Restore path;
- run the Server as a dedicated non-root service user, persist its protected
  deployment record and Application Database locator across container
  replacement, and make mounted secret files readable only by that user;
- exclude Rust, Cargo, source code, test tooling, and build dependencies;
- document and test host administration, persistent state and backups, TLS
  termination, non-secret configuration, secret injection, provenance, upgrade,
  and rollback boundaries; and
- follow the supported OCI image deployment requirements established for the
  release.

## Related Documents

- [Milestone 14](../../plan/milestones/milestones.md#milestone-14-build-support-for-a-server-oci-image)
- [Development Container Design](../dev/development-container-design.md)
- [Server Lifecycle Design](../../server/lifecycle-design.md)
- [Server Init Design](../../server/init-design.md)
- [Server Restore Design](../../server/restore-design.md)
- [Open Questions](../../open-questions.md)
