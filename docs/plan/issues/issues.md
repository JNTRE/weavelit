# Open Issue Overview

This document is a high-level snapshot of open issues in the
[Weavelit issue tracker](https://github.com/JNTRE/weavelit/issues) and their
planning metadata in the
[Weavelit GitHub Project](https://github.com/orgs/JNTRE/projects/1/views/1).
GitHub remains the source of truth for live issue state, relationships,
milestones, and project fields.

## Standard Record Format

Each open issue uses an issue-centered record instead of a wide table. This
keeps summaries readable, accommodates optional project fields, and limits a
routine metadata update to the affected issue's record.

- The linked heading records the issue number, name, and canonical issue link.
- `Summary` states the intended outcome or decision at a high level.
- `Related` records the related epic and GitHub Milestone.
- `Project` records GitHub Project board status and priority.
- `Issue` records type and label.

## Open Issues

### [#Example-issue-title](https://github.com/JNTRE/weavelit/issues/X)

**Summary:** Document a repeatable process for refreshing this overview from the issue tracker, GitHub Milestones, and the Weavelit GitHub Project without treating the snapshot as the live source of truth.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#4 Server architecture and dependency baseline](https://github.com/JNTRE/weavelit/issues/4) |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |
| Project | Board | `backlog` |
| Project | Priority | `next` |
| Issue | Type | `task` |
| Issue | Label | `documentation` |

### [#4 Server architecture and dependency baseline](https://github.com/JNTRE/weavelit/issues/4)

**Summary:** Establish the Server package boundaries and dependency policy needed to begin Milestone 1 implementation, coordinating six focused decision and implementation issues without prematurely selecting unrelated libraries.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | This issue is the epic |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |
| Project | Board | `backlog` |
| Project | Priority | `next` |
| Issue | Type | `epic` |
| Issue | Label | `server core` |

### [#5 Define Rust workspace dependency policy](https://github.com/JNTRE/weavelit/issues/5)

**Summary:** Decide how the Server Rust workspace centralizes dependency versions, constrains crate-specific features, maintains its lockfile, and justifies production dependencies before additional internal crates are introduced.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#4 Server architecture and dependency baseline](https://github.com/JNTRE/weavelit/issues/4) |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |
| Project | Board | `backlog` |
| Project | Priority | `next` |
| Issue | Type | `decision` |
| Issue | Label | `server core` |

### [#6 Define Application Database crate boundary and SQLite approach](https://github.com/JNTRE/weavelit/issues/6)

**Summary:** Decide the boundary between the Server, the Application Database contract, and the SQLite backend, including the driver, migrations, transactions, typed errors, and isolated-test approach.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#4 Server architecture and dependency baseline](https://github.com/JNTRE/weavelit/issues/4) |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |
| Project | Board | `backlog` |
| Project | Priority | `next` |
| Issue | Type | `decision` |
| Issue | Label | `database module` |

### [#7 Define Server-owned Init and Admin CLI boundary](https://github.com/JNTRE/weavelit/issues/7)

**Summary:** Define one Server-owned Init use case shared by interactive and non-interactive workflows, with explicit Admin CLI adapter responsibilities, secret-file handling, validation, and rejection behavior.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#4 Server architecture and dependency baseline](https://github.com/JNTRE/weavelit/issues/4) |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |
| Project | Board | `backlog` |
| Project | Priority | `next` |
| Issue | Type | `decision` |
| Issue | Label | `server core` |

### [#8 Add Application Database contract crate](https://github.com/JNTRE/weavelit/issues/8)

**Summary:** Add the backend-neutral Application Database contract crate with the minimum initialization and persistence APIs, typed errors, and focused contract tests needed by the first Server-owned Init workflow.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#4 Server architecture and dependency baseline](https://github.com/JNTRE/weavelit/issues/4) |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |
| Project | Board | `backlog` |
| Project | Priority | `next` |
| Issue | Type | `task` |
| Issue | Label | `database module` |

### [#9 Add SQLite Application Database backend crate](https://github.com/JNTRE/weavelit/issues/9)

**Summary:** Add the SQLite backend crate with the selected driver, explicit migrations, transactional writes, restart persistence, typed redacted errors, and isolated integration tests.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#4 Server architecture and dependency baseline](https://github.com/JNTRE/weavelit/issues/4) |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |
| Project | Board | `backlog` |
| Project | Priority | `next` |
| Issue | Type | `task` |
| Issue | Label | `database module` |

### [#10 Establish Server diagnostics and test-resource baseline](https://github.com/JNTRE/weavelit/issues/10)

**Summary:** Establish reusable typed-error, structured redacted diagnostics, and isolated temporary-resource conventions for the Server, Admin CLI, and deterministic tests.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#4 Server architecture and dependency baseline](https://github.com/JNTRE/weavelit/issues/4) |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |
| Project | Board | `backlog` |
| Project | Priority | `next` |
| Issue | Type | `task` |
| Issue | Label | `server core` |

## Maintenance

Refresh this snapshot from all three GitHub planning layers: repository issues,
GitHub Milestones, and the Weavelit GitHub Project. Add newly opened issues,
remove closed issues, and update changed metadata and summaries. Preserve
`Not assigned` for empty fields and do not infer project metadata from titles or
issue bodies.

## Related Documents

- [Milestone Index](../milestones.md)
- [Milestone 1 Outcomes](../milestones/milestone-1.md)
- [Testing and Validation Policy](../../testing.md)
