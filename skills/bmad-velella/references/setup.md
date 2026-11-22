# Project setup and the fixed bundle

Use `B` and `S` from the entrypoint. All paths must resolve from the current
Velella snapshot or installed package. This package includes the official
core-tools and method modules with their dependency skills and original resources.
It is a preview snapshot: both module records say `6.13.0-next`; the exact commit
is in `assets/bmad/PROVENANCE.json`. It is not the v6.12 stable installer.

Use `uv` for the shipped Python scripts (Python 3.11+). If unavailable, report
the missing runtime; do not manually recreate `_bmad/`. No MCP service or
background process is needed. Set `PYTHONDONTWRITEBYTECODE=1` in the child
environment so Python imports do not mutate the fixed skill package.

## Choose a project

Use the project explicitly selected by the user. If it already has `_bmad/`,
inspect that setup and its version first. Do not replace it, migrate it, rename
custom files, or adopt a parent project's setup merely to make a command work.
With no `_bmad/`, initialize only when the user requested BMAD setup or authorized
the workflow's project setup. A help or status request does not authorize setup.

Read `S/bmad/references/setup.md` for the official setup contract. Here the active
BMAD root is the bundled `S`; this integration is managed by Velella, not npm
Skills CLI or an upstream plugin marketplace. The setup script's local setup
branch is sufficient; do not execute its unrelated install/update/removal offers.

Before a first setup, obtain the questions without writing files:

```text
uv run --no-cache "S/bmad/scripts/setup.py" --project-root "P" --skill "S/bmad" --root "S" --list-config-questions
```

Replace `P` with the selected project and `S` with its absolute bundled path.
Ask any emitted configuration questions, retaining their scope and defaults as
the official setup reference requires. If the list is empty, no answer file is
needed. Then initialize using the same pinned scripts:

```text
uv run --no-cache "S/bmad/scripts/setup.py" --project-root "P" --skill "S/bmad" --root "S"
```

Setup creates project-local `_bmad/` configuration/runtime files and
`_bmad-output/`. It does not run a model or implement the requested feature. Check
the returned JSON and preserve the user's existing files. An unknown response or
failure is not a successful initialization; inspect before retrying and stop after
one retry with a changed prerequisite or new evidence.

## Status and upgrades

The same command with `--status` is read-only for project files, but can contact
GitHub to check upstream module versions. Distinguish local readiness from remote
update availability; `could-not-check` is an unknown update check. Report the
pinned commit even when newer upstream code exists. Do not claim the installation
is current unless the official result says `current: true`.

For an upgrade request, first review a new fixed source revision and its diff,
validate the bundle in a separate project, then use Velella's preview/update flow.
Updating the Skill does not implicitly migrate a project's `_bmad/` runtime.
Present any required project migration separately before executing it.
