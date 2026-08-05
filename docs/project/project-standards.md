# GitHub Planning Compatibility Index

**Deprecated.** This transitional index preserves a repository compatibility
path while JNTRE planning workflow authority moves to focused agent skills and
live GitHub discovery. It does not define Issue, Pull Request, epic, or Project
workflow policy, and new policy must not be added here.

## Current Authorities

- GitHub is authoritative for live Issue and Pull Request state, native types,
  labels, assignees, priorities, relationships, milestones, Project fields,
  option identifiers, and configured values. Read the
  [JNTRE/weavelit repository](https://github.com/JNTRE/weavelit) and
  [Weavelit GitHub Project](https://github.com/orgs/JNTRE/projects/1/views/1)
  before an operation that depends on them.
- `docs/project/project.yaml` binds automation to the expected
  repository and Project identity. It is not a snapshot of live metadata or
  workflow policy.
- The Markdown files under `docs/project/issue_templates/` define the body
  shape for agent-created Issues. The resulting GitHub Issue is authoritative
  after creation.
- [Contributing](../../CONTRIBUTING.md) defines branch roles, branch naming,
  commit messages, and contribution flow.
- `.github/pull_request_template.md` defines the required Pull Request body and
  final checklist shape.
- GitHub Milestones are the sole live and navigation authority for milestone
  records.

## Compatibility Rules

Existing references to this path use it only to locate the current authorities
above. Agents must discover live GitHub configuration, bind it to
`docs/project/project.yaml`, and apply the active focused workflow
before mutating GitHub. They must stop on missing, stale, or ambiguous binding
or configuration instead of inferring a value from this index.

Do not add metadata tables, Project option identifiers, issue lifecycle rules,
epic membership or seal policy, Pull Request lifecycle rules, or Git operation
policy to this document.

## Removal Condition

Remove this compatibility index after repository and external workflow
references no longer depend on `docs/project/project-standards.md`. Until then,
changes may update authority routing or the removal condition only.

## Related Documents

- [Contributing](../../CONTRIBUTING.md)