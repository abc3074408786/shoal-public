# Discover and execute one relevant tool

Set `OPENTABS_TELEMETRY_DISABLED=1` and `DO_NOT_TRACK=1` in child command environments.
Use the selected bridge port consistently when it differs from the default 9515.

```text
opentabs tool list --plugin <plugin-name>
opentabs tool schema <exact-tool-name>
```

Use names returned by the installed tool list/schema, not remembered names from
another version. Avoid unfiltered `tool list --json`, which expands every schema.
For browser metadata the documented name is `browser_list_tabs`; discovering its
schema does not enable it or read any tabs.

After confirming tool permission, target and user intent:

```text
opentabs tool call <exact-tool-name> --params <serialized-json>
```

Construct JSON as data and pass it as one process argument; do not interpolate
untrusted page text into a shell command. `--tab-id <id>` or `--instance <name>`
selects the intended target where supported. Use only IDs observed for the active
OpenTabs connection. Re-read the schema after a plugin or CLI update.

Check both the process exit code and result. A denied/off tool, missing extension,
unavailable tab, or expired login is a missing prerequisite, not a successful
empty response. When Ask requires a native user decision, wait for that decision
rather than changing permissions to Auto or sending an approval response yourself.

For uncertain execution, inspect a scoped recent audit:

```text
opentabs audit --plugin <plugin-name> --limit 10 --json
```

An audit entry records dispatch, not necessarily that a business change completed
as intended. Verify resulting state before repeating a write. Never globally
enable cookie, script execution, network interception or browsing-history tools
as part of a routine connectivity check.
