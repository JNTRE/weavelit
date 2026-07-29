# Open Issue Overview

> [!INFO]
> This document is a high-level snapshot of open issues in the
> [Weavelit issue tracker](https://github.com/JNTRE/weavelit/issues) and their
> planning metadata in the
> [Weavelit GitHub Project](https://github.com/orgs/JNTRE/projects/1/views/1).
> GitHub remains the source of truth for live issue state, relationships,
> milestones, and project fields.
>
> Refresh this snapshot from all three GitHub planning layers: repository
> issues, GitHub Milestones, and the Weavelit GitHub Project. Add newly opened
> issues, remove closed issues, and update changed metadata and summaries.
> Preserve `Not assigned` for empty fields and do not infer project metadata
> from titles or issue bodies.
>
> Create every new issue from the Markdown template in this directory that
> matches its native GitHub issue type. Copy the template in `templates/` beginning
> with `##` to a temporary body file, replace every bracketed placeholder, and pass the file to
> `gh issue create --body-file` with the matching `--type` value. After creation,
> assign the applicable repository component labels, `Priority` value, GitHub
> Milestone, Project status, and issue relationships. Do not recreate retired
> Project fields for type, area, delivery priority, size, or estimate. See the
> [GitHub Project Standards](../project/project-standards.md).

## Standard Record Format

Each open issue uses an issue-centered record instead of a wide table. This
keeps summaries readable, accommodates optional project fields, and limits a
routine metadata update to the affected issue's record. Every issue must retain
the example's heading, summary, and grouped-table layout. Include only rows for
applicable metadata fields; do not add or restructure record fields.

- The linked heading records the issue number, name, and canonical issue link.
- `Summary` states the intended outcome or decision at a high level.
- `Related` rows record the related epic and GitHub Milestone when applicable.
- `Project` rows record GitHub Project board status when applicable.
- `Issue` rows record type, label, and priority when applicable.

## Open Issues

### [#Example-issue-title](https://github.com/JNTRE/weavelit/issues/X)

**Summary:** Document a repeatable process for refreshing this overview from the issue tracker, GitHub Milestones, and the Weavelit GitHub Project without treating the snapshot as the live source of truth.

| Group | Field | Value |
| --- | --- | --- |
| Related | Related epic | [#4 Server architecture and dependency baseline](https://github.com/JNTRE/weavelit/issues/4) |
| Related | Milestone | [Milestone 1: Core Server Application](https://github.com/JNTRE/weavelit/milestone/1) |
| Project | Board | `backlog` |
| Issue | Priority | `next` |
| Issue | Type | `task` |
| Issue | Label | `documentation` |

## Related Documents

- [Bug Template](templates/bug-template.md)
- [Decision Template](templates/decision-template.md)
- [Epic Template](templates/epic-template.md)
- [Feature Template](templates/feature-template.md)
- [GitHub Project Standards](../project/project-standards.md)
- [Milestone 1 Outcomes](../milestones/milestone-1.md)
- [Risk Template](templates/risk-template.md)
- [Task Template](templates/task-template.md)
- [Testing and Validation Policy](../../testing.md)
