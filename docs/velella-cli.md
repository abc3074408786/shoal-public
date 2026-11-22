# Velella CLI

`velella-cli` controls Velella's existing desktop daemon through owner-only local IPC.
It is separate from the original Orca application's `orca` command.
The same commands are available as `velella desktop ...`. The older `velella task`
and `velella session` commands use the legacy TCP daemon, not the desktop daemon.

## Start here

Open Velella normally, then run:

```powershell
velella-cli status --json
velella-cli project list --json
velella-cli workspace list --json
velella-cli terminal list --workspace <workspace-id> --json
velella-cli skills bundled --json
velella-cli skills get velella-cli
velella-cli skills get velella-collaboration
```

Global options can appear before or after subcommands:

- `--json`: one JSON object on stdout; success is `{"ok":true,"result":...}`.
  Failure is `{"ok":false,"error":{"code":...,"message":...}}` and exit code 1.
  Invalid command arguments exit 2. Help/version exit 0.
- `--data-dir <path>`: select the exact desktop profile. Otherwise use
  `VELELLA_DATA_DIR`, then `%LOCALAPPDATA%\Velella` on Windows or `$HOME/Velella`
  on Unix, matching the desktop. The CLI never starts or restarts a daemon.
- `--timeout-ms <1..60000>`: IPC deadline, default 45000. No automatic mutation retries.
- `guide`: print this bundled guide without connecting to a daemon.

`skills bundled` discovers version-matched built-in guides; `skills get <name>`
prints one as Markdown, or returns it with CLI version and guide revision under
`--json`. These commands work offline and do not install skills. Even with SSH
options, they describe the local CLI binary, not the remote runtime. `skills list`
(alias `installed`) and `skills show` retain daemon-discovered user skill behavior.

## Session collaboration

`velella-cli collab` (alias `collaboration`) provides direct local IPC access to
the existing session membership, mailbox, work assignment and review protocol.
Start with `velella-cli skills get velella-collaboration`. It shares credentials,
native-session checks and stable mutation IDs with `velella-runtime collab`;
the old entrypoint remains compatible. No MCP server is required for direct CLI
operations. The stdio server remains `velella-runtime collab mcp`; the public
CLI rejects `collab mcp` rather than mixing protocol streams with JSON output.

`collab` rejects SSH options before connecting or touching credentials. Do not
copy local member credentials to remote profiles. `swarm` remains the user-side
v2 orchestration control surface, including human plan confirmation.

## Projects, workspaces and tasks

```powershell
velella-cli project add "D:\my project" --json
velella-cli project show <project-id> --json
velella-cli workspace list --project <project-id> --include-archived --json
velella-cli workspace show <workspace-id> --json
velella-cli workspace create --project <project-id> --name cli-work --base main --request-id <uuid> --json
velella-cli task list --project <project-id> --json
velella-cli task show <task-id> --json
```

Always copy exact IDs from list replies. There are no implicit active-workspace
selectors. Folder projects have one primary workspace; only Git projects can create
additional worktrees. Workspace creation skips setup scripts and launches no agent.
It leaves branch/base validation, locks and creation recovery to the existing daemon.
Keep a creation's request UUID and identical options to recover that operation;
do not invent a new UUID after an uncertain response.

Task commands also support creation from YAML/JSON, events/output/diff, verification,
feedback, explicit recovery and cancellation. `swarm` commands manage the existing
v2 orchestration runtime. See `docs/velella-cli-management.md` for management commands.
Delivery remains subject to the
runtime's review and acceptance gates; these commands do not publish changes.

## Terminals

```powershell
velella-cli terminal create --workspace <workspace-id> --request-id <uuid> --json
velella-cli terminal create --workspace <workspace-id> --launch codex --request-id <uuid> --json
velella-cli terminal show --workspace <workspace-id> --terminal <session-id> --json
velella-cli terminal switch --workspace <workspace-id> --terminal <session-id> --json
velella-cli terminal read --workspace <workspace-id> --terminal <session-id> --cursor 0 --limit 65536 --json
velella-cli terminal send --workspace <workspace-id> --terminal <session-id> --text "Get-Date" --enter --json
velella-cli terminal send --workspace <workspace-id> --terminal <session-id> --text-file prompt.txt --request-id <uuid> --enter --json
velella-cli terminal stop --workspace <workspace-id> --terminal <session-id> --confirm --json
```

Creation supports `terminal` (the host's default shell), `codex`, and `claude-code`.
Use a stable launch UUID for recovery; it is returned as `request_id` even on an
uncertain transport failure. Native agent launches retain normal login/permission
behavior. Stopping a task-owned terminal stops that task and its other sessions.

`terminal switch` (also `agent switch`) selects an existing native terminal in the
same profile's local desktop, including a terminal inside a split. A closed tab
is reopened without starting or recovering its process. Success requires the
renderer to confirm the exact terminal's textarea focus. It requires a desktop
supporting `velella-terminal-switch-v1`; an older or closed desktop fails clearly.
Archived workspaces, native chat surfaces, SSH routing, and an active desktop
dialog/menu are rejected. If the desktop is currently using SSH, select local in
the desktop first. No input is sent. A timeout is unconfirmed: the selection may
have changed; the CLI never retries automatically. Exited terminals may be shown
for history without restarting them. This does not establish PTY input readiness.

Read before sending. Output reads return `from`, `next_cursor`, `total`, `has_more`,
`text`, and base64 `data`. Cursors are absolute **byte offsets**, never character
counts. Continue from `next_cursor`; one page is at most 128 KiB. `text` is a UTF-8
preview with ANSI escapes retained. For lossless capture across page boundaries,
decode and concatenate `data` before decoding UTF-8. A cursor beyond the retained
output clamps to the current end. Output is a stream, not a rendered screen snapshot.

Shell sends check the observed host PID and input sequence immediately before
writing. `--expected-sequence <n>` also rejects input newer than the caller's prior
observation. Shell input has no durable replay receipt: after any uncertain response,
inspect the session and output; do not resend blindly. `input_accepted` confirms PTY
input only. It does not confirm command completion or a model turn.

Codex/Claude sends use the daemon's existing durable feedback queue, safe-idle
checks, bracketed paste and separate Enter. Without `--enter`, the prompt is only
drafted. Use the same `--request-id`, text, Enter flag and expected sequence to read
back a saved delivery receipt without duplicating the prompt. `submitted` means
Enter reached the PTY, not that a model turn was observed. An uncertain/sending
receipt returns failure and the saved receipt; inspect it rather than starting a
new request. Other agent providers can be listed/read/stopped, but prompt delivery
through this initial CLI is limited to Codex/Claude and plain shells.

Text files must be UTF-8 and at most 64 KiB. NUL and terminal control sequences
are rejected. Native agent feedback uses the daemon's stricter text checks.

### Native execution receipts (upgraded runtime required)

```powershell
velella-cli terminal create --workspace <workspace-id> --launch codex --track-submission --request-id <uuid> --json
velella-cli terminal send --workspace <workspace-id> --terminal <session-id> --text-file prompt.txt --enter --request-id <uuid> --wait-submit 10 --json
velella-cli terminal receipt --workspace <workspace-id> --terminal <session-id> --request-id <same-send-uuid> --json
```

Tracking is opt-in for a **new native Codex/Claude session**. It adds synchronous
lifecycle hooks and can add native hook startup latency. Existing sessions are
unchanged. The runtime must advertise `submission_tracking:1`, and the session must
advertise `submission_protocol:1` with a known native identity. Unsupported runtimes
reject tracked creation before launch; unsupported sessions reject `--wait-submit`
before sending. Normal sends retain their existing compatibility.

`--wait-submit` accepts 0..60 seconds and requires `--enter`. It observes the same
request, never sends again on a timeout. `turn_started_confirmed:true` requires a
matching native prompt digest and then a native `PreToolUse` or `Stop` event for the
same session and guarded input sequence (and turn ID when supplied). SessionStart,
UserPromptSubmit alone, terminal output and `running` never prove execution. A plain
text turn may remain unconfirmed until its Stop hook. Native versions that omit or
transform the hook's prompt or correlation fields remain unconfirmed. This evidence
does not prove a successful task, valid answer, or upstream billing.

Timeout returns a successful input receipt with `turn_started_confirmed:false`;
it is not proof of failed execution. Read `terminal receipt`, or replay the identical
send with the same UUID/text/Enter/sequence to read the durable delivery receipt.
Receipt reads never send input, and confirmed evidence survives later turns and host
exit. Changed human input, permission interactions, absent hooks or a replaced host
can leave a request unconfirmed. Drafts, shells and SSH submission tracking are not
supported. No credentials or prompt plaintext appear in execution receipts.

## Native conversation search

```powershell
velella-cli search "search phrase" --workspace <workspace-id> --json
velella-cli search "search phrase" --path D:\my-project --agent codex --json
velella-cli search "tool output phrase" --path D:\my-project --scope all --session <native-session-uuid> --json
```

This is a read-only, bounded JSONL scan of local Codex/Claude history, separate from
`terminal search` (PTY recording bytes). `--path` works without a daemon. `--workspace`
resolves the exact local folder through the selected daemon. Only canonical matching
working directories and valid native session identities are included. Codex uses
`CODEX_HOME/sessions` (default `.codex/sessions`); Claude uses
`CLAUDE_CONFIG_DIR/projects` (default `.claude/projects`) under the CLI user's home.
SSH is rejected before any connection; `--data-dir` does not select another user's
native history. Linked history roots/files are skipped and reported incomplete.

The default `conversation` scope searches visible user/assistant text. `--scope all`
also searches supported tool arguments/results. System messages, model reasoning,
auth/config files and arbitrary metadata are excluded. Matching is case-sensitive
literal text; results contain bounded snippets, native session IDs, source paths and
JSONL byte ranges. These ranges identify source records, not PTY byte positions.

There is no persistent index or all-machine search. Defaults: 200 files, 32 MiB total
JSONL bytes, 100 matching records, 30 seconds; discovery is capped at 20,000 directory
entries and records at 2 MiB. Limits are adjustable within help's bounds. Files are
scanned newest first, including files whose cwd later proves unrelated. Read
`complete`, `omitted_files`, `files[].range_complete` and `errors`; an incomplete scan
with no hits does not mean no matching history exists. Narrow by `--agent`/`--session`
or increase limits. Partial final records and unsupported/corrupt data are never
silently called complete. Each file has a fixed byte end; history must remain
append-only during the scan. Archived histories and Claude subagent files are not
included in this initial search.

## Scope and build

Local commands target the selected Velella profile. Explicit `--ssh-profile`,
`--server-id` and `--remote-data-dir` select the separately inspected remote runtime
for supported commands; see the management guide. The CLI does not follow Electron's
SSH execution-host selector. Changing `--data-dir` alone never selects a remote
host. Live model requests are not required to use the
CLI and are separate from shell/IPC integration verification.

```powershell
cargo build -j 1 -p velella --bin velella-cli --bin velella --bin velella-runtime --release
cargo test -p velella --lib desktop_cli
python scripts/verify-velella-cli.py --cli target/release/velella-cli.exe --runtime target/release/velella-runtime.exe
```

The CLI is a standalone binary. It can be placed beside the installed app without
replacing the desktop runtime or restarting existing sessions. Future desktop
packages include it under `resources/bin/`; put that directory on PATH to use its
short command. Repository-local invocation is `target/release/velella-cli.exe` on
Windows or `target/release/velella-cli` on Unix.

On Windows, `powershell -File scripts/install-velella-cli.ps1` installs the built
binary to `%LOCALAPPDATA%\Programs\Velella CLI`, backs up a previous differing
binary, and adds this directory to the current user's PATH. New terminals inherit
that PATH; already running terminals can use the absolute installed path. Pass
`-SkipPath` to copy only. This installer does not replace the desktop app/runtime.
