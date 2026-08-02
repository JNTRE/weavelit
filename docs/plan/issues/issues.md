# Weavelit Issues

The [Weavelit GitHub Issues](https://github.com/JNTRE/weavelit/issues) are the
authoritative source for issue titles, bodies, outcomes, acceptance criteria,
type, state, labels, assignees, priority, relationships, milestone assignments,
and GitHub Project fields. This document is a repository navigation index of
open issues and their brief summaries only. Related Epic and Milestone values
may be included for context. If this index differs from GitHub, GitHub controls.

## Open Issues

### #47 Establish HTTPS listener and Client Module API foundation

[Open GitHub Issue #47](https://github.com/JNTRE/weavelit/issues/47).

**Summary:** Start one configured HTTPS listener in restricted pre-operational
mode, register versioned Client Module API routes, and deliver bundled client
assets without an alternate lifecycle or configuration path.

| Group | Field | Value |
| --- | --- | --- |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |

### #56 Validate trusted HTTPS listener configuration

[Open GitHub Issue #56](https://github.com/JNTRE/weavelit/issues/56).

**Summary:** Validate only trusted host-supplied HTTPS listener and TLS
configuration, rejecting invalid material before any listener can bind.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#47 Establish HTTPS listener and Client Module API foundation](https://github.com/JNTRE/weavelit/issues/47) |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |

### #57 Compose restricted HTTPS listener and Client Module route foundation

[Open GitHub Issue #57](https://github.com/JNTRE/weavelit/issues/57).

**Summary:** Compose the lifecycle-gated HTTPS runtime and restricted
`/api/v1/` Client Module route foundation without normal-operation handlers.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#47 Establish HTTPS listener and Client Module API foundation](https://github.com/JNTRE/weavelit/issues/47) |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |

### #58 Deliver bundled Client Module assets over HTTPS

[Open GitHub Issue #58](https://github.com/JNTRE/weavelit/issues/58).

**Summary:** Deliver the Server-packaged Client Module asset bundle through the
restricted HTTPS listener while preserving the versioned API namespace.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#47 Establish HTTPS listener and Client Module API foundation](https://github.com/JNTRE/weavelit/issues/47) |
| Related | Milestone | [Milestone 3: Client Module - Web UI](https://github.com/JNTRE/weavelit/milestone/3) |

### #48 Compose Log Module contract and MVP SQLite Log Module

[Open GitHub Issue #48](https://github.com/JNTRE/weavelit/issues/48).

**Summary:** Compose the Log Module contract and compiled-in MVP SQLite Log
Module with redacted, separately stored System Logs and Audit Logs.

| Group | Field | Value |
| --- | --- | --- |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |

### #69 Define Log Module delivery and durable completion semantics

[Open GitHub Issue #69](https://github.com/JNTRE/weavelit/issues/69).

**Summary:** Define the delivery and durable completion semantics for Log
Module records.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#48 Compose Log Module contract and MVP SQLite Log Module](https://github.com/JNTRE/weavelit/issues/48) |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |

### #70 Define SQLite Log Module destination lifecycle

[Open GitHub Issue #70](https://github.com/JNTRE/weavelit/issues/70).

**Summary:** Define the lifecycle for an SQLite Log Module destination.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#48 Compose Log Module contract and MVP SQLite Log Module](https://github.com/JNTRE/weavelit/issues/48) |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |

### #71 Define MVP Log Module contract and structured record boundary

[Open GitHub Issue #71](https://github.com/JNTRE/weavelit/issues/71).

**Summary:** Define the MVP Log Module contract and structured record boundary.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#48 Compose Log Module contract and MVP SQLite Log Module](https://github.com/JNTRE/weavelit/issues/48) |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |

### #72 Implement isolated MVP SQLite Log Module

[Open GitHub Issue #72](https://github.com/JNTRE/weavelit/issues/72).

**Summary:** Implement the isolated MVP SQLite Log Module.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#48 Compose Log Module contract and MVP SQLite Log Module](https://github.com/JNTRE/weavelit/issues/48) |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |

### #73 Implement Server Log Module contract and compiled-in catalog

[Open GitHub Issue #73](https://github.com/JNTRE/weavelit/issues/73).

**Summary:** Implement the Server Log Module contract and compiled-in catalog.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#48 Compose Log Module contract and MVP SQLite Log Module](https://github.com/JNTRE/weavelit/issues/48) |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |

### #75 Compose compiled SQLite Log Module into Server startup

[Open GitHub Issue #75](https://github.com/JNTRE/weavelit/issues/75).

**Summary:** Compose the compiled SQLite Log Module into Server startup.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#48 Compose Log Module contract and MVP SQLite Log Module](https://github.com/JNTRE/weavelit/issues/48) |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |

### #76 Monitor SQLite Log Module destination availability risk

[Open GitHub Issue #76](https://github.com/JNTRE/weavelit/issues/76).

**Summary:** Monitor availability risk for the SQLite Log Module destination.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#48 Compose Log Module contract and MVP SQLite Log Module](https://github.com/JNTRE/weavelit/issues/48) |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |

### #77 Define independent Log Module destination recovery policy

[Open GitHub Issue #77](https://github.com/JNTRE/weavelit/issues/77).

**Summary:** Define the recovery policy for independent Log Module destinations.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#48 Compose Log Module contract and MVP SQLite Log Module](https://github.com/JNTRE/weavelit/issues/48) |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |

### #78 Define Log Module retention and purge policy

[Open GitHub Issue #78](https://github.com/JNTRE/weavelit/issues/78).

**Summary:** Define the retention and purge policy for Log Module records.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#48 Compose Log Module contract and MVP SQLite Log Module](https://github.com/JNTRE/weavelit/issues/48) |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |

### #49 Implement Server Init workflow

[Open GitHub Issue #49](https://github.com/JNTRE/weavelit/issues/49).

**Summary:** Implement the Server-owned Init workflow that creates initial
state, establishes log assignments and recovery material, and seals deployment.

| Group | Field | Value |
| --- | --- | --- |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |

### #50 Implement local authentication and session management

[Open GitHub Issue #50](https://github.com/JNTRE/weavelit/issues/50).

**Summary:** Implement Server-owned local password authentication and sessions
that remain usable only while account and MFA policy permit them.

| Group | Field | Value |
| --- | --- | --- |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |

### #51 Implement Server-owned MFA policy

[Open GitHub Issue #51](https://github.com/JNTRE/weavelit/issues/51).

**Summary:** Implement Server-owned MFA policy, enforcement, recovery, and
audit behavior while retaining method-specific logic in compiled MFA Modules.

| Group | Field | Value |
| --- | --- | --- |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |

### #52 Implement default-deny authorization

[Open GitHub Issue #52](https://github.com/JNTRE/weavelit/issues/52).

**Summary:** Enforce default-deny, additive Group grants, access-class
separation, and availability gates for users, modules, and Operations.

| Group | Field | Value |
| --- | --- | --- |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |

### #53 Implement Administration Plane contracts

[Open GitHub Issue #53](https://github.com/JNTRE/weavelit/issues/53).

**Summary:** Implement Server-owned Administration Plane contracts for account,
grant, module, log, and backup administration with independent authorization.

| Group | Field | Value |
| --- | --- | --- |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |

### #54 Implement encrypted backup and Restore

[Open GitHub Issue #54](https://github.com/JNTRE/weavelit/issues/54).

**Summary:** Implement encrypted Application Database backup and eligible
replacement Restore with validation, atomic state restoration, and sealing.

| Group | Field | Value |
| --- | --- | --- |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |

### #59 Decide HTTPS edge and unauthenticated-surface policy

[Open GitHub Issue #59](https://github.com/JNTRE/weavelit/issues/59).

**Summary:** Decide the supported HTTPS termination, trusted host
configuration, certificate custody and renewal, listener, and unauthenticated
surface policy that #56 needs to validate configuration objectively.

| Group | Field | Value |
| --- | --- | --- |
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
