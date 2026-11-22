# Source and adaptation

Adapted from Lauren Tan's MIT-licensed pstack, version `0.15.36`, in
`cursor/plugins` at commit `a5539b15a3d56e04992565aafee2b37051329ce3`.

Selected source files:

- [create-verification-skill](https://github.com/cursor/plugins/blob/a5539b15a3d56e04992565aafee2b37051329ce3/pstack/skills/create-verification-skill/SKILL.md)
- [maintain-verification-skill](https://github.com/cursor/plugins/blob/a5539b15a3d56e04992565aafee2b37051329ce3/pstack/skills/maintain-verification-skill/SKILL.md)
- [Eval playbook](https://github.com/cursor/plugins/blob/a5539b15a3d56e04992565aafee2b37051329ce3/pstack/skills/poteto-mode/playbooks/eval.md)

The upstream license is retained in `B/LICENSE`. This is a Velella/Codex
adaptation, not an official Cursor, pstack, or OpenAI integration.

Adaptations:

- One discovery entry routes to three references, rather than installing all
  pstack skills or adopting poteto-mode's global behavior.
- Existing project locations are reused; `.agents/skills` is the portable default
  in place of Cursor-only `.cursor/skills`.
- Existing host tools, model choices, permissions, ownership and agent limits
  replace Cursor Task types, arena/swarm dependencies and prescribed model panels.
- Maintenance can be sequential, and its delivery need not be a PR. Product
  regressions prevent a clean verdict and remain outside maintenance edits.
- Blinding hides experimental identity and grading material while retaining
  genuine task assets. It does not ban ordinary project files named `test`.
- Candidate/judge execution and trace evidence are explicit capabilities, not
  assumed from a Cursor transcript path. Missing capabilities stay unexecuted.

Not included: poteto-mode, shipping/autopilot/automation playbooks, model setup,
formal verification generation, Cursor-only control skills, and other pstack
plugins or agents. This package neither installs those dependencies nor replaces
the project's BMAD/OpenSpec workflow. Updates require reviewing a new pinned
source and adaptation; ordinary use does not update itself.
