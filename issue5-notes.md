# Issue 5 Review Notes

Working notes for [issue #5: Define Rust workspace dependency policy](https://github.com/JNTRE/weavelit/issues/5).
This file records discussion and provisional decisions while the review is in
progress. Settled policy must be moved into the appropriate canonical document
under `docs/` before this work is merged.

## Review Scope

- Shared dependency versions in `server/Cargo.toml`.
- Crate-specific feature selection and constraints.
- `Cargo.lock` commit and update expectations.
- Evidence and ownership required for production dependencies.
- Initial cross-cutting dependencies appropriate before feature work begins.

## Notes

- 2026-07-16: Security is the default for dependency selection. Every
  production dependency must be carefully selected, justified, and enabled
  with only the required features.
- 2026-07-16: Cargo unifies enabled features for each resolved dependency
  version across the workspace. A feature enabled by any workspace crate is
  present in the effective build and must be reviewed accordingly.
- 2026-07-16: When a second crate needs the same dependency, promote its
  version, source, and shared security baseline from the first crate manifest
  to `[workspace.dependencies]` in the same change. Preserve each crate's
  minimal feature selection, then review the combined feature set and lockfile
  update.

## Decisions

| Topic | Decision | Rationale | Status |
| --- | --- | --- | --- |
| Workspace and crate feature ownership | The workspace manifest owns each approved dependency's identity, version, source, and any workspace-wide security baseline. Each consuming crate owns the minimal behavior-specific feature set. The combined workspace feature set is reviewed as a security-relevant dependency change. | Central version control prevents drift while crate-local feature declarations keep capability selection tied to an owning behavior. Cargo unifies features across workspace consumers, so the resulting set requires workspace-wide review. | Agreed |
| Workspace dependency placement | A dependency stays in its single owning crate until a second workspace crate uses the same package. The change adding that consumer promotes the dependency to `[workspace.dependencies]`, preserves each consumer's minimal features, and reviews the combined feature set and lockfile update. Do not add speculative workspace dependencies. | Local placement makes a single dependency's behavior owner clear. Promotion centralizes version and source governance when sharing becomes real, without merging product boundaries or preselecting future technology choices. | Agreed |
| Lockfile governance | Commit `server/Cargo.lock`; change it only through Cargo, never by hand. Normal updates are targeted and reviewed with their resolved dependency changes. Broad updates require a dedicated maintenance change. Required validation uses Cargo's `--locked` mode. Security updates remain focused and document their advisory, resolved version, affected behavior, lockfile impact, and validation. | The committed lockfile makes builds reproducible and reviewable. Targeted resolution updates keep supply-chain changes understandable while retaining an urgent, auditable path for security fixes. | Agreed |
| Production dependency evidence and registry | Every direct production dependency requires a documented record of its behavior, owner, purpose, source, version, minimal features, maintenance and license evidence, and validation. Security-sensitive packages add security-property, capability, advisory, and safe-failure-test evidence. The Server Architecture Design maintains the approved direct-dependency registry and is updated with every material change. | The record creates a durable review trail without duplicating Cargo's transitive resolution lockfile. | Agreed |
| Package sources and exceptions | Released crates.io packages are the default. Local paths are reserved for internal workspace members, and alternate registries are prohibited without approval. Git dependencies, unpublished forks, and any non-registry resolved package are temporary exceptions requiring an immutable full revision, source and replacement rationale, named owner, and removal condition or follow-on issue. | Restricting package sources reduces supply-chain trust boundaries while retaining a documented path for urgent compatibility or security exceptions. | Agreed |
| Initial cross-cutting dependencies | Approve no external production dependency before a named Milestone behavior requires it and an owning crate is known. | Deferring selection avoids speculative architecture, unnecessary supply-chain exposure, and unowned configuration. | Agreed |

## Documentation Changes

- `docs/server/server-architecture-design.md`: Add and maintain the approved
  direct production dependency registry and record the shared-dependency
  boundary.
- `docs/plan/issues/issues.md`: Update issue #5's summary with the settled
  policy outcome.

## Related Documents

- [Server Architecture Design](docs/server/server-architecture-design.md)
- [Milestone 1](docs/plan/milestones/milestone-1.md)
- [Testing and Validation Policy](docs/testing.md)
