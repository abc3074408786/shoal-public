# Runtime and extension connection

These recipes were checked with `@opentabs-dev/cli` 0.0.115 and Node.js 24.22.1 on
Windows. Node.js 22+ is required. Use the compatible installed CLI; a missing
runtime is a separate installation action, not a reason to run unpinned `npx`.

Check the version and health:

```text
opentabs --version
opentabs status --json
```

Require a live bridge and `extensionConnections > 0` before browser execution.
`mcpClients: 0` is normal in CLI mode. Tools are discovered through REST, but the
same upstream server also exposes MCP endpoints; this workflow registers no client.

## Starting and stopping

If the task requires OpenTabs and the bridge is unavailable, run with the host's
Python executable:

```text
python "B/scripts/bridge.py"
```

Replace `B` with the current Skill/snapshot path. The helper uses the installed
npm CLI, keeps permission bypass disabled, opts its child processes out of
telemetry, starts at most once, and checks health for up to 15 seconds. It emits
only selected status fields. Upstream `start --background` may return before the
HTTP listener is ready; do not restart a successfully spawned server to fix this
startup delay. If startup fails, inspect the cause before one changed retry.

The default config directory is the user's `~/.opentabs/`; an explicitly selected
`OPENTABS_CONFIG_DIR` is inherited. Do not switch profiles to reach a different
signed-in session. No tool output, state, config or credentials belong inside the
Skill package or its frozen snapshot.

When the user wants the bridge stopped, and it is the intended shared instance:

```text
opentabs stop
```

Do not stop a bridge other tasks are using or add login/startup automation as an
implicit part of using this Skill. `opentabs doctor` diagnoses server/extension
readiness; its MCP-client warning does not by itself block CLI mode.

## Chrome extension

The installed extension directory is `~/.opentabs/extension/`. In Chrome the user
loads it through `chrome://extensions/`, Developer mode, Load unpacked. Its 0.0.115
manifest requests all-site access and browser capabilities including cookies,
debugger, history, bookmarks and downloads. The browser extension's access and
OpenTabs tool permissions are different layers; default tool Off does not narrow
the extension's manifest permissions.

Follow the active browser confirmation policy for this installation. If browser
tools prohibit internal Chrome pages, ask the user to load the prepared directory;
do not alter Chrome preference files, use command-line flags, or another UI surface
to bypass the restriction. Once the user has loaded it, verify connection via
`opentabs status --json`; do not infer success from a file copy.

The local authentication token remains in OpenTabs' own auth/config files. Avoid
`opentabs start --show-config` and `opentabs config show --show-secret`: they expose
live credentials. CLI commands handle local authentication themselves.
