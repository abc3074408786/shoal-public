# Loading and handing off workflows

`S` is this package's `assets/bmad/skills` directory. `P` is the selected project,
not the directory holding the Skill. Keep the user's working directory in the
project while running its workflows. Never initialize the Velella source
repository because the Skill happens to be stored there.

Before invoking a project workflow, run the bundled, read-only compatibility check:

```text
uv run "B/scripts/check_project.py" --project-root "P"
```

This compares project-local shared and module scripts with this fixed bundle,
using the upstream setup code's own tree comparison. It does not contact GitHub
or change files. Require `ready: true`. If setup is missing, mismatched or invalid,
read `references/setup.md`; do not execute a different project runtime or repair
it without the appropriate setup/upgrade authorization. A matching script tree
does not imply a feature or business acceptance verdict.

| Request | Embedded entry |
| --- | --- |
| Understand BMAD or choose a route | `S/bmad/SKILL.md` |
| Research, brief, or business requirements | `S/bmad-agent-analyst/SKILL.md` |
| Product planning and requirements documents | `S/bmad-agent-pm/SKILL.md` |
| Architecture decisions | `S/bmad-architecture/SKILL.md` |
| Implement a defined change | `S/bmad-build/SKILL.md` |
| Review changes | `S/bmad-code-review/SKILL.md` |

Follow the selected entry's real activation procedure. Some entries resolve
customization/configuration; build and code review instead instruct a renderer.
Do not send every entry to a renderer or invent a single shared activation flow.

For example, the official build entry runs:

```text
uv run --no-cache "P/_bmad/scripts/render_skill.py" --project-root "P" --skill "S/bmad-build"
```

Honor the entry's user-selected route/review flags. The rendered workflow is the
instruction source; report `HALT` or failure instead of running raw templates as
a fallback. Only read a referenced template or guidance file when the current
step needs it. Do not render `B` (the outer bundle), because the upstream renderer
recurses into Markdown files and would collect unrelated embedded workflows.

For help, the official hub's local module query is:

```text
uv run "S/bmad/scripts/knowledge.py" --root "S" --content
```

An instruction to invoke a bundled canonical name, such as `bmad-facilitate`,
means read `S/bmad-facilitate/SKILL.md` and follow it with that directory as
`{skill-root}`. Preserve its input artifacts, ticket/initiative and resolved
configuration; do not start an unrelated task. The package's resource tree is the
active BMAD installation for these calls, even though the host exposes one outer
Skill. Optional founder, toolsmith, game-dev and test-architect modules are not
part of this bundle.

Project authority remains the existing repository contract. BMAD's ticket tree,
roles and review steps do not automatically supersede adopted Issue/PR scope or
authorize extra agent launches. Report generated plans, actual edits, executed
checks and independent acceptance separately. Loading or rendering a workflow is
not proof that the workflow completed or that a feature was deployed.

## Review-only requests

When the user asks only for findings, render code review with
`--set workflow.fixes=handoff`. Return the findings in chat; do not append to a
plan, ticket, or `deferred-work.md`, even if the upstream present step asks for
it. Do not edit code, run builds/tests, or execute quality gates. Retain paths
only as evidence references, not destinations to modify.

BMAD rendering creates local analysis artifacts under `_bmad/`. If the user
forbids every project write, do not initialize or render; use a suitable existing
rendered snapshot, or report the missing read-only preparation rather than
silently writing scratch files into that project.
