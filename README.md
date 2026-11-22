# 鱼群（Velella）

鱼群是一个 Windows 桌面 AI 开发工作台，用于管理原生 Agent CLI、终端、项目和任务。

> A colony of agents, sailing while you sleep.

**velella** is an open-source, daemon-first **agent task runtime**: it runs coding-agent
CLI sessions (Claude Code, Codex, Gemini CLI, …) in the background for hours, verifies
their work against machine-checkable done-criteria, and pings you only when a human is
actually needed.

The velella (sail jellyfish) is not one animal but a colony — thousands of polyps each
doing one job, crossing the open ocean as a single being. A velella task is the same:
one worktree, one agent, one budget, one verifier and one review gate, moving as a
single unit you can inspect at any moment.

## Why

Agent capability lives in their CLIs — skills, hooks, MCP, memory, permission systems —
and it updates weekly. velella does not reimplement agents; it **drives the real CLIs**
over a PTY and adds what CLIs lack for unattended work:

- **Daemon-first** — sessions survive the UI; the same runtime serves CLI, TUI, CI and
  any editor plugin through a documented NDJSON API.
- **Tasks as code** — a task is a committed `.velella/tasks/*.yaml` file: reviewable,
  shareable, replayable in CI.
- **Verification-first** — done is machine-checked (tests, build, guardrails), failures
  are fed back to the agent with bounded retries, and only then does a human see a diff.
  `on_done: pr` commits the worktree, pushes the branch and opens a PR (`gh` when
  installed, a compare link otherwise).
- **Supervised, not babysat** — a classifier folds each agent's screen into a normalized
  state (running / waiting_permission / waiting_input / …) every second; notable changes
  hit your `notify_cmd`. Agent screens drift weekly, so patterns are data (`.velella/
  adapters/*.yaml` overrides ship without a rebuild) and matching is whitespace-
  insensitive because agents draw prompts with cursor repositioning.

## Status: pre-alpha scaffold

This repository holds the v0 foundation: the normalized state machines, the task
format, the append-only event log, and the CLI spine. PTY session management, the first
agent adapter (Claude Code), the verification loop and the TUI board land next — see
the roadmap in [docs/open-source-plan.md](docs/open-source-plan.md).

## Layout

| Path | What |
|---|---|
| `crates/velella-core` | Domain: task format, normalized state machines, SQLite event log |
| `crates/velella` | The `velella` CLI: `init`, `serve`, `task list/show/run`, `version` |
| `docs/` | Direction analysis and open-source plan (Chinese) |

## Quick start

To manage your own fish swarm from a standalone command, build `velella-cli`:
`cargo build -p velella --bin velella-cli --release`. It manages desktop projects,
Agent terminals, tasks and v2 orchestration through the existing local runtime.
Start with `velella-cli status --json` and `velella-cli guide`;
see [Velella CLI guide](docs/velella-cli-management.md).

For the desktop app's projects, workspaces and live terminals, use
`velella-cli status --json` (or `velella desktop status --json`). See the
[desktop CLI guide](docs/velella-cli.md). The commands below use the older
headless task/session runtime.

```bash
cargo build
cargo run -p velella -- init         # scaffold .velella/ in this directory
cargo run -p velella -- task run .velella/tasks/example.yaml
cargo run -p velella -- task list
cargo run -p velella -- serve        # headless runtime, NDJSON over TCP
cargo run -p velella -- board        # keyboard cockpit: tasks, sessions, live output
# browser dashboard: open http://127.0.0.1:7302 while serve runs

# sessions: the daemon owns the PTY — exit anytime, agents keep sailing
cargo run -p velella -- session spawn --adapter claude-code -- cmd   # or bash / claude
cargo run -p velella -- session list            # classified states: waiting_permission, waiting_input, …
cargo run -p velella -- session attach <id>     # Ctrl-C detaches, session lives on
cargo run -p velella -- session send <id> "1"   # answer a permission dialog from anywhere
cargo run -p velella -- session kill <id>

# get pinged when an agent needs you (toast / Slack / anything):
#   .velella/config.toml -> notify_cmd = "curl -s -X POST $SLACK_HOOK -d $VELELLA_STATE"

# one request per connection:
echo '{"cmd":"status"}'    | nc 127.0.0.1 7301
echo '{"cmd":"task.list"}' | nc 127.0.0.1 7301

# the phone door — answer an agent's permission dialog from anywhere.
#   .velella/config.toml -> http_token = "long-secret"  (required beyond localhost)
curl -s -X POST http://127.0.0.1:7302/sessions/<id>/approve \
     -H "Authorization: Bearer long-secret" -d '{"answer":"1"}'
```

## Notes

- **Environment inheritance**: agents are children of `velella serve`, so they
  inherit its environment — start serve from a shell where your agent CLIs
  work (proxy vars, credentials, PATH shims included). The compat suite
  catches this class of drift: `velella compat`.
- **Budgets**: `wall_clock` is enforced; `tokens` is declared in the task
  format but reserved until agents expose reliable spend signals.

## License

Apache-2.0
