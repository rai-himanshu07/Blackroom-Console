# Agent Context Exclusions

Search matches consume context even when the agent never opens the matched file.
Exclude generated, vendored, and high-volume paths from both version control and
agent search while keeping source, tests, schemas, migrations, and lockfiles
visible.

## Git Ignore

Merge only entries that match the project. At minimum, data-science projects
using this template should ignore run artifacts:

```gitignore
artifacts/
```

Common additional candidates:

```gitignore
.venv/
node_modules/
dist/
build/
.next/
coverage/
.codebase-memory/
```

Do not ignore dependency lockfiles, database migrations, fixtures required by
tests, or generated clients that the project intentionally commits.

## VS Code Search

The default install includes a conservative `.vscode/settings.json`; use
`--without-context-settings` to skip it, or merge its `search.exclude` object into existing
workspace settings. `search.exclude` keeps files visible in Explorer while
removing them from text search and grep. Use `files.exclude` only when hiding
them from Explorer is also desirable.

The semantic index also respects `.gitignore`, `files.exclude`, and common
generated-file exclusions. If an ignored file is open or selected, it can still
enter context intentionally.

## Validation

Run the project doctor. It reports separately when experiment artifacts can be
committed and when they can enter agent search context.
