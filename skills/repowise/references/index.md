# Project indexes, loaded only for graph-backed queries

Use the selected checkout as cwd. Check `git rev-parse --show-toplevel`, HEAD, and
the working-tree state before indexing; do not jump to a linked worktree's main
checkout. Index-free operations in `cli.md` do not need this step.

## Reuse first

If `.repowise/wiki.db` and `.repowise/state.json` exist, inspect:

```text
repowise status --format json --no-workspace
```

Check the stored revision, available analysis, and any warnings against the
current checkout and dirty files. If status omits freshness information, inspect
only the relevant revision/sync fields in `.repowise/state.json`; do not print its
entire contents or `.repowise/.env`. Unknown freshness stays unknown. Do not
re-index merely because a new Agent session started.

## Create only when the question needs an index

Set the child environment described in `runtime.md`, then run:

```text
repowise init . --yes --no-prose --no-workspace --no-editor-setup --no-save-key --no-claude-md --no-agents --no-codex --no-hook --no-distill-hook --commit-limit 200 --progress json
```

This creates local `.repowise/` data. It must not rewrite agent instructions,
register MCP, install hooks, save keys, or change `.gitignore`. Do not add the
index to a commit. The 200-commit cap limits initial history work; disclose that
scope rather than claiming complete historical ownership or co-change evidence.
Do not use `--force` on an existing index as a routine repair.

## Refresh only when stale

For graph/risk queries about current staged, unstaged, or untracked source:

```text
repowise update . --no-workspace --index-only --defer-docs --working-tree --no-agents --progress json
```

Set the same child environment before updating. `--index-only --defer-docs`
refreshes facts without rendering or embedding documentation. If current template
wiki text is necessary for full-text search, omit `--defer-docs` for that update;
keep `--index-only`. Deferred or stale prose must not be described as current.

Check the final JSON event and the next status read, including degraded/skipped
analyses. Exit zero alone does not prove a complete index. Do not allow concurrent
updates of the same checkout; respect an existing update lock instead of deleting
it. Never use a watcher or post-commit hook to keep this Skill's index current.

If a reused index has editor integration enabled from another workflow, verify
the no-editor environment is honored before updating. Stop on unexpected writes;
preserve unrelated files and report the exact affected paths.
