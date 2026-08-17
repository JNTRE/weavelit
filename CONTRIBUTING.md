# Contributing

Thanks for contributing to Weavelit.

## Branch and release workflow

- `main` is the release branch. Do not commit feature work directly to it.
- `dev` is the permanent integration branch. Never delete it.
- Topic branches are temporary and contain one focused change.

For normal development:

1. Update `dev` from `origin/dev`.
2. Create and push a topic branch from `dev`.
3. Make, verify, commit, and push the change on the topic branch.
4. Open a pull request targeting `dev` and merge it after checks pass.
5. Delete the topic branch when it is no longer needed.

For a release, verify the integrated work on `dev`, then open and merge a pull
request from `dev` into `main`. Keep `dev` and continue using it for subsequent
development.

## Validation

Run the applicable checks before opening a pull request. For Server Rust
changes, run `make -C server check`. Rust changes must meet the [Testing and
Validation Policy](docs/testing.md); the Rust Quality workflow repeats this
suite on non-draft pull requests targeting `dev` or `main`.

## Branch names

Use `<type>/<short-kebab-case-description>`, such as
`docs/conventional-commits` or `fix/missing-project-file`. Use the same types
listed for commits below.

## Agent-created issue titles

For every GitHub Issue created through a JNTRE workflow, use the same
Conventional Commit subject format required for commit messages:

```text
<type>(<scope>): <description>
```

This applies to every native issue type, including epics, features, tasks,
bugs, decisions, and risks. Choose the Conventional Commit type for the
intended outcome rather than copying the native issue type. Keep the
description short, imperative, and specific; put detailed context and
acceptance criteria in the issue body.

This requirement does not apply to user-submitted, imported, or otherwise
externally created issues. Do not automatically retitle those issues merely to
make them conform.

## Commit messages

Follow [Conventional Commits 1.0.0](https://www.conventionalcommits.org/en/v1.0.0/)
using this project format:

```text
<type>(<scope>): <description>
```

The scope is required. Use one of these project scopes:

- `cli` — the command-line interface
- `webui` — the web user interface
- `core` — the core application
- `module` — shared work on Rust service-integration libraries
- `module-<service>` — a Rust library for a specific service, such as
  `module-zendesk` or `module-azure`
- `repo` — repository-wide documentation, configuration, or maintenance

Use lowercase kebab-case for service names. Prefer the specific
`module-<service>` scope when a change affects only one integration.

Use these common types:

- `feat` — new functionality
- `fix` — bug fixes
- `docs` — documentation only
- `style` — formatting changes that do not affect behavior
- `refactor` — code changes that neither fix a bug nor add functionality
- `perf` — performance improvements
- `test` — test additions or corrections
- `build` — build system or dependency changes
- `ci` — continuous integration changes
- `chore` — maintenance not covered by another type
- `revert` — revert an earlier commit

Keep descriptions short, imperative, and specific. Examples:

```text
feat(cli): add project import command
fix(webui): preserve the active project after refresh
refactor(core): simplify project loading
feat(module-zendesk): add ticket search
chore(repo): update shared configuration
```
