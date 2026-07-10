# Contributing

Thanks for contributing to WeaveLit.

Please base changes on `dev` and open pull requests against `dev`.

## Branch names

Use `<type>/<short-kebab-case-description>`, such as
`docs/conventional-commits` or `fix/missing-project-file`. Use the same types
listed for commits below.

## Commit messages

Follow [Conventional Commits 1.0.0](https://www.conventionalcommits.org/en/v1.0.0/)
using this project format:

```text
<type>(<scope>): <description>
```

The scope is required. Use a short noun for the affected area, such as `editor`,
`storage`, or `docs`. Use `repo` when a change affects the repository as a whole.

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
feat(editor): add chapter navigation
fix(storage): handle missing project files
docs(contributing): define commit naming rules
chore(repo): update shared configuration
```

Mark breaking changes with `!` before the colon, and explain the impact in the
commit body or a `BREAKING CHANGE:` footer:

```text
feat(storage)!: change the project file format
```
