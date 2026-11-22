# Website plugins and permissions

This integration installs the OpenTabs framework only. No website plugin or
browser tool was enabled during setup. Use only a plugin that fits the user's
actual requested site and operation.

```text
opentabs plugin list --json
opentabs plugin search <site>
```

A listing or npm description is a lead. Before adopting a plugin, inspect the
specific publisher/version and source, its URL patterns, tools, settings and data
access. Record the exact npm package/version and update path. Tool permission is
not a substitute for source review: adapters run in the signed-in browser session.

Install a chosen plugin only within existing task authorization. Use a reviewed
exact package/version, then refresh its tool list and schema. Do not accept a
similarly named community package merely because the official one is missing.

New tools begin Off. A user can choose Ask for a specific needed tool; do not
enable a whole plugin or set Auto as a generic installation step. Preserve native
approval dialogs and the browser confirmation policy. Updates can reset tool
permissions and need source/schema review again.

Do not run `opentabs update` or unpinned npm upgrades implicitly. The installed
framework was fixed to 0.0.115 for this integration; an upgrade is a separate
reviewed change. No account login, messages, uploads, payments, or other website
writes are part of framework installation.

Upstream documentation: https://opentabs.dev/docs/reference/cli and
https://opentabs.dev/docs/reference/mcp-server#cli-mode .
