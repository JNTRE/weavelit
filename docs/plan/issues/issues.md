# Weavelit Issues

The [Weavelit GitHub Issues](https://github.com/JNTRE/weavelit/issues) are the
authoritative source for issue titles, bodies, outcomes, acceptance criteria,
type, state, labels, assignees, priority, relationships, milestone assignments,
and GitHub Project fields. This document is a repository navigation index of
open issues and their brief summaries only. Related Epic and Milestone values
may be included for context. If this index differs from GitHub, GitHub controls.

## Open Issues

### #34 Establish trusted pre-operational lifecycle foundation

[Open GitHub Issue #34](https://github.com/JNTRE/weavelit/issues/34).

**Summary:** Establish the trusted Server-owned pre-operational lifecycle
authority so one deployment can persist and validate its identity and
Application Database selection, reopen safely after restart, classify startup
state, serialize Init and Restore checkpoint ownership, and compose restricted
startup without exposing normal operation.

| Group | Field | Value |
| --- | --- | --- |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |

### #36 Create the Server lifecycle contract and backend catalog

[Open GitHub Issue #36](https://github.com/JNTRE/weavelit/issues/36).

**Summary:** Introduce a backend-neutral `weavelit-server-lifecycle` crate that
defines the trusted lifecycle domain and runtime-supplied Application Database
backend catalog needed by later persistence and orchestration work.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#34 Establish trusted pre-operational lifecycle foundation](https://github.com/JNTRE/weavelit/issues/34) |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |

### #37 Persist protected deployment records and database locators

[Open GitHub Issue #37](https://github.com/JNTRE/weavelit/issues/37).

**Summary:** Persist and reopen the protected deployment record, Application
Database locator, and Server-local at-rest key material so lifecycle identity
and selected-backend configuration survive restart with deterministic
fail-closed behavior.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#34 Establish trusted pre-operational lifecycle foundation](https://github.com/JNTRE/weavelit/issues/34) |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |

### #38 Implement Application Database selection and restart reopening

[Open GitHub Issue #38](https://github.com/JNTRE/weavelit/issues/38).

**Summary:** Implement Application Database selection, eligibility preflight,
protected locator persistence, eligible replacement, and restart reopening
without allowing client influence over local storage paths.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#34 Establish trusted pre-operational lifecycle foundation](https://github.com/JNTRE/weavelit/issues/34) |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |

### #39 Implement pre-operational startup classification

[Open GitHub Issue #39](https://github.com/JNTRE/weavelit/issues/39).

**Summary:** Implement deterministic startup classification for every
deployment-record, locator, and selected Application Database state combination
so the Server remains restricted and fails closed before exposing a workflow or
normal capability.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#34 Establish trusted pre-operational lifecycle foundation](https://github.com/JNTRE/weavelit/issues/34) |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |

### #40 Implement lifecycle workflow arbitration and crash reconciliation

[Open GitHub Issue #40](https://github.com/JNTRE/weavelit/issues/40).

**Summary:** Implement the shared lifecycle mutation authority that serializes
Init and Restore checkpoint ownership, advances and reconciles the deployment
record in crash-safe order, and supports explicit matching-workflow reset.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#34 Establish trusted pre-operational lifecycle foundation](https://github.com/JNTRE/weavelit/issues/34) |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |

### #41 Compose restricted lifecycle startup in the Server runtime

[Open GitHub Issue #41](https://github.com/JNTRE/weavelit/issues/41).

**Summary:** Replace the Server executable placeholder with restricted lifecycle
startup that consumes trusted host configuration, supplies the real SQLite
backend catalog, classifies retained state before exposing a capability, and
fails closed for states deferred to later work.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#34 Establish trusted pre-operational lifecycle foundation](https://github.com/JNTRE/weavelit/issues/34) |
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
