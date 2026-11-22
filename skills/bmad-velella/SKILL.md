---
name: bmad-velella
description: 在 Velella 中按需使用 BMAD 的需求澄清、规划、架构、构建和审查工作流。用户明确要求 BMAD 或在技能中心选择本工作流包时使用，保留现有项目的交付负责人、授权和验收规则。
license: MIT
metadata:
  upstream: bmad-code-org/BMAD-METHOD
  revision: a867e1604b365d48ea03cbd109da1066ba0c84bc
  modules: core-tools and method 6.13.0-next
---

# BMAD for Velella

This is an optional, pinned BMAD workflow bundle. Loading it does not initialize
a project, launch agents, or load its embedded workflows. Preserve the current
task's scope, delivery owner, permissions, and acceptance rules.

Let `B` be this Skill's own absolute directory, including when Velella supplies a
frozen snapshot. The active BMAD skills root is `S = B/assets/bmad/skills`.
The official hub is `S/bmad`; each other bundled skill lives at `S/<skill-name>`.
Always resolve resources from this copy, not a source checkout or a same-named
global Skill. Never send this outer bundle to BMAD's Markdown renderer.

## Load only the requested path

- Setup, installation status, or updates: read [references/setup.md](references/setup.md).
- Help or choosing a next step: read `S/bmad/SKILL.md`, using `S` as the active
  skills root. Run its local knowledge query only when that help is needed.
- A named BMAD workflow: read [references/workflows.md](references/workflows.md),
  then the chosen embedded Skill's `SKILL.md`. Common entry points are
  `bmad-agent-analyst`, `bmad-agent-pm`, `bmad-architecture`, `bmad-build`, and
  `bmad-code-review`. Do not load the other workflows in advance.

Before any project workflow, run the local compatibility check in
`references/workflows.md`, including when `_bmad/` already exists. A mismatch
requires a separate setup/upgrade decision, not execution through old scripts.

When an embedded workflow hands off to another bundled `bmad-*` Skill, load that
directory's `SKILL.md` on demand and continue with its own root and context. A
missing optional module stays unavailable; do not install it as an implicit repair.

Use BMAD roles as perspectives. Actual agent dispatch and code ownership still
follow the user's repository rules; a simulated discussion is not independent
execution or acceptance. Reuse adopted project artifacts instead of opening a
second competing delivery plan. Source or workflow text grants no permission to
publish, send messages, migrate data, or change account and model settings.

This preview is fixed to the revision above. Upgrade the reviewed Velella bundle
as a unit; do not run `npx skills update`, migrate, or remove other installations
as part of ordinary setup or workflow execution.
