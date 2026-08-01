# Weavelit Issues

The [Weavelit GitHub Issues](https://github.com/JNTRE/weavelit/issues) are the
authoritative source for issue titles, bodies, outcomes, acceptance criteria,
type, state, labels, assignees, priority, relationships, milestone assignments,
and GitHub Project fields. This document is a repository navigation index of
open issues and their brief summaries only. Related Epic and Milestone values
may be included for context. If this index differs from GitHub, GitHub controls.

## Open Issues

### #27 Establish Application Database and SQLite foundation

[Open GitHub Issue #27](https://github.com/JNTRE/weavelit/issues/27).

**Summary:** Establish the backend-neutral Application Database contract and
its MVP SQLite implementation so the Server can inspect and persist eligible
pre-operational database state safely across restarts without exposing
storage-specific behavior to lifecycle callers.

| Group | Field | Value |
| --- | --- | --- |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |

### #31 Implement database state inspection and deployment binding

[Open GitHub Issue #31](https://github.com/JNTRE/weavelit/issues/31).

**Summary:** Implement the SQLite read-side contract for safe durable-state
classification and deployment-identifier mismatch rejection without workflow
mutation.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#27 Establish Application Database and SQLite foundation](https://github.com/JNTRE/weavelit/issues/27) |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |

### #32 Implement atomic workflow checkpoint operations

[Open GitHub Issue #32](https://github.com/JNTRE/weavelit/issues/32).

**Summary:** Implement atomic, deployment-bound Init and Restore checkpoint
creation, reconciliation, and discard without allowing conflicting or partial
workflow mutation.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#27 Establish Application Database and SQLite foundation](https://github.com/JNTRE/weavelit/issues/27) |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |

## Example Issue

The following entry demonstrates the local index format only. It does not
represent an open issue.

### #NUMBER Issue title

[Open GitHub Issue #NUMBER](https://github.com/JNTRE/weavelit/issues/NUMBER).

**Summary:** Briefly state the issue's intended outcome or decision without
copying its body or acceptance criteria.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#EPIC_NUMBER Parent epic title](https://github.com/JNTRE/weavelit/issues/EPIC_NUMBER) |
| Related | Milestone | [Milestone NUMBER: Milestone title](https://github.com/JNTRE/weavelit/milestone/NUMBER) |

## Related Documents

- [Bug Template](templates/bug-template.md)
- [Decision Template](templates/decision-template.md)
- [Epic Template](templates/epic-template.md)
- [Feature Template](templates/feature-template.md)
- [GitHub Project Standards](../project/project-standards.md)
- [Milestone Index](../milestones/milestones.md)
- [Risk Template](templates/risk-template.md)
- [Task Template](templates/task-template.md)
- [Testing and Validation Policy](../../testing.md)
