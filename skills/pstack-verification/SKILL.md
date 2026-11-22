---
name: pstack-verification
description: 为已有项目创建或维护可执行的用户路径验证 harness，或设计并运行技能、提示词的 Agent 行为对照评测。适用于缺少真实运行验证、验证流程漂移或需要比较 Agent 行为的任务。
license: MIT
metadata:
  upstream: cursor/plugins/pstack
  revision: a5539b15a3d56e04992565aafee2b37051329ce3
  upstream-version: 0.15.36
---

# Pstack verification for Velella

Use this package for verification work beside the project's existing BMAD,
OpenSpec, or delivery workflow. It does not create a second product plan, install
the full pstack plugin, or change agent/model settings when loaded.

Let `B` be this skill's own directory, including a Velella frozen snapshot.
Let `P` be the user's selected checkout. Generated verification belongs in `P`,
not in `B`, the source package, a sibling worktree, or the Velella installation.

## Choose one path

- No usable way to launch and verify real app behavior: read
  [create.md](references/create.md).
- An existing verification skill, script, or feature map has drifted: read
  [maintain.md](references/maintain.md).
- Compare a skill/prompt change's effect on agent behavior: read
  [agent-eval.md](references/agent-eval.md).

Read only the chosen reference. Preserve the project's adopted harness and
specification; a working script does not need wrapping in a new skill just to
fit this package. Source and adaptation details are in
[provenance.md](references/provenance.md).

## Execution boundaries

Use the current host's available tools and the repository's agent rules. A
Velella CLI connection is optional. Keep the user's model, effort, speed,
concurrency limit, ownership, and authorization choices. Do not hardcode Cursor
agents, a model panel, a third-party plugin, or an MCP service as prerequisites.
Delegate only permitted exploration/review; the owning session drives shared
instances and performs final verification. Do not read unrelated chat history.

Inspect existing commands before running them. Execute within the authorized
project/test environment; any additional dependencies, shared instances, or
external effects follow the task's existing authorization. A package instruction
does not grant permission to send messages, merge, deploy, or operate production.

Report what was generated, what actually ran, evidence paths, cleanup state, and
remaining gaps. A fixture run, source review, live app check, agent comparison,
deployment, and business acceptance are separate results. Never call a written
procedure or a model's self-report a passed run.
