# GitHub Project and Issue Standards

This document records the active GitHub planning metadata for
[JNTRE/weavelit](https://github.com/JNTRE/weavelit) and the
[Weavelit GitHub Project](https://github.com/orgs/JNTRE/projects/1/views/1).
GitHub remains the source of truth; update this reference when that configuration
changes.

## Issue Types

Create issues from the matching Markdown template in
[`docs/plan/issues/`](../issues/). Copy the template sections beginning with
`##` into the body file, then pass the completed file to
`gh issue create --body-file` and set the matching native type with `--type`.
After creation, assign the labels, `Priority`, GitHub Milestone, Project status,
and issue relationships required for the work.

| Type | Use |
| --- | --- |
| `task` | A specific piece of work. |
| `bug` | An unexpected problem or behavior. |
| `feature` | A request, idea, or new functionality. |
| `epic` | A cohesive outcome that organizes related issues. |
| `decision` | A bounded question requiring a documented conclusion. |
| `risk` | An identified uncertainty requiring monitoring or mitigation. |

## Labels

Apply the applicable component label to each issue. Add the general-purpose
labels only when their defined condition applies; they do not replace the native
issue type.

### Component Labels

| Label | Apply when the issue affects |
| --- | --- |
| `server core` | The Weavelit Server core. |
| `database module` | An Application Database component. |
| `service module` | A Service Module component. |
| `client module` | A Client Module component. |
| `log module` | A Log Module component. |

### General-Purpose Labels

| Label | Use |
| --- | --- |
| `documentation` | Improvements or additions to documentation. |
| `duplicate` | The issue or pull request already exists. |
| `enhancement` | A new feature or request. |
| `good first issue` | Work suitable for a newcomer. |
| `help wanted` | The work needs extra attention. |
| `invalid` | The report does not appear valid. |
| `question` | Further information is requested. |
| `wontfix` | The work will not be undertaken. |

## Priority

`Priority` is an organization-level GitHub Issue Field that records the current
importance level assigned to an issue. Set one of these exact values on each
issue.

| Priority |
| --- |
| `urgent` |
| `high` |
| `medium` |
| `low` |
| `next` |

## Project Status

Set one of these exact `Status` values for each Project item.

| Status | Meaning |
| --- | --- |
| `backlog` | The item has not been started. |
| `ready` | The item is ready to be picked up. |
| `in progress` | The item is actively being worked on. |
| `In review` | The item is in review. |
| `done` | The item has been completed. |
| `blocked` | The item cannot proceed until its blocker is resolved. |

## Related Documents

- [Issue Templates and Open Issue Overview](../issues/issues.md)
- [Contributing](../../../CONTRIBUTING.md)
