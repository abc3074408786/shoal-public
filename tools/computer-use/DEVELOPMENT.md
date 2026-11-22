# Development acknowledgement — Computer Use for the swarm

Date: 2026-11-17
Executor: MiniMax Code (Standard, bounded deadline), implementation engine only.
Authority: `_bmad-output/initiative-velella-standards/plan-computer-use-for-swarm.md` is the sole source of truth.

## Scope

Deliver an opt-in local Windows MCP service plus a generated fish catalog:

- `tools/computer-use/src/velella_computer_use/server.py` — MCP tools over the official Python MCP SDK (stdio).
- `tools/computer-use/src/velella_computer_use/windows.py` — capture, Unicode input, coordinate transform.
- `tools/computer-use/src/velella_computer_use/control.py` — explicit cross-process control lease.
- `tools/computer-use/src/velella_computer_use/catalog.py` — read-only / control catalog entries.
- `tools/computer-use/tests/`, `scripts/verify-computer-use.py`, `tools/computer-use/README.md`.
- `openspec/changes/computer-use-for-swarm/proposal.md` — matching proposal only; no competing delivery plan.

## Boundaries honoured

- Only files above plus `scripts/fixtures/computer-use-*` are touched. All other snapshot files are preserved.
- The dirty original checkout, live profiles, CLI settings and running processes are not touched. Nothing is published, installed or migrated.
- Observation (screenshot) is separate from input. Control requires an explicit lease acquired by the owning fish.
- One Windows desktop allows one active controlling process at a time across fish roles and workflows.
  The lease is bounded, expires, releases on disconnect/crash, and can be stopped locally.
  No silent transfer of another fish's lease.
- Never: auto-enable input for every fish; execute shell commands as a tool; alter clipboard contents;
  silently route remote roles to this local desktop; any macOS/Linux/remote implementation;
  real-account business actions.
- These are cooperative workflow controls, not a security sandbox against same-user native programs.

## Verification plan (developer level)

1. `uv sync --frozen --python 3.12` in `tools/computer-use` — reproducible local environment.
2. `uv run --frozen python -m unittest discover -s tests` — protocol, lease/lifecycle, image budget, catalog shape.
3. `uv run --project tools/computer-use --frozen python scripts/verify-computer-use.py --mode protocol --evidence <dir>`
   — actual fish relay with the pinned runtime and temporary data directories.
4. `--mode native` — owned fixture window only, foreground guards, clipboard preserved, owned processes closed.
   Actual mouse/keyboard fixture operations are left to root final validation; the developer does not
   operate user applications.

Native fixture acceptance is recorded as evidence, not asserted as business success. Receipts do not
prove business outcomes.

## Known limits carried into the implementation

- Windows only. Non-Windows hosts fail explicitly instead of pretending to control a desktop.
- No clipboard writes; Unicode text is injected as keystrokes.
- The relay frame limit is 1,048,576 bytes; screenshots are re-encoded to fit the whole JSON-RPC frame.
