# GitHub Project Standards

This document records the active GitHub planning metadata for
[JNTRE/weavelit](https://github.com/JNTRE/weavelit) and the
[Weavelit GitHub Project](https://github.com/orgs/JNTRE/projects/1/views/1).
GitHub remains the source of truth; update this reference when that configuration
changes.

## Metadata Scope

These standards apply to GitHub Issues and Pull Requests where GitHub supports
the metadata. Labels, GitHub Project fields, and milestones use the same
configured values for both work-item types. Native issue types,
organization-level `Priority`, and issue relationships apply only to Issues.

## Issue Types

Create issues from the matching Markdown template in
[`docs/plan/issues/templates/`](../issues/templates/). Copy the template sections beginning with
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

Apply the applicable component label to each Issue or Pull Request. Add the
general-purpose labels only when their defined condition applies; they do not
replace a native issue type.

### Component Labels

| Label | Apply when the Issue or Pull Request affects |
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
importance assigned to an Issue. Set one of these exact values on each Issue.

| Priority |
| --- |
| `urgent` |
| `high` |
| `medium` |
| `low` |
| `next` |

## Project Status

Set one of these exact `Status` values for each Issue or Pull Request Project
item.

| Status | Meaning |
| --- | --- |
| `backlog` | The item has not been started. |
| `ready` | The item is ready to be picked up. |
| `in progress` | The item is actively being worked on. |
| `In review` | The item is in review. |
| `done` | The item has been completed. |
| `blocked` | The item cannot proceed until its blocker is resolved. |

## Milestones

GitHub Milestones are the authoritative source for milestone titles, summaries,
goals, state, dates, progress, and assigned issues. The repository
[Milestone Index](../milestones/milestones.md) provides navigation and brief
summaries only.

Apply the relevant GitHub Milestone to an Issue or Pull Request when it belongs
to a defined delivery milestone. Leave the milestone unset only when no
milestone applies.

## Related Documents

- [Issue Templates and Open Issue Overview](../issues/issues.md)
- [Milestone Index](../milestones/milestones.md)
- [Contributing](../../../CONTRIBUTING.md)
