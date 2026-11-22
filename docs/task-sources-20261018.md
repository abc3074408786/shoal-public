# Task sources — 2026-10-18

Owner: current Codex chat 01a14f6d-61ac-77e1-93c8-9f20c97b3aa3. Only this chat writes this worktree; scouts are read-only.

User outcome: GitHub, GitLab, Linear and Jira tasks provide the starting point for development. Connect a source, browse/search/page issues, read a task, create or reopen its workspace, and return to the original task from the workspace. Create new issues through an explicit form submission. Existing terminal-first development remains available.

Baseline: installed scroll-fix source snapshot, digest c8325e32547081b9e77f9d34f7fe87bfe0ca02407d31e8fd691aaa45200d6f8d. Per-file hashes: artifacts/task-sources-20261018/baseline.json. No mutation of the frozen source, current installation, user data or live sessions.

Acceptance: four real HTTP adapters, provider-specific auth and pagination, durable source/workspace association, reopening without another worktree, visible error/empty/loading states, no credentials in responses/logs/process arguments, network operations outside the terminal service lock. Tests use isolated HTTP/SQLite fixtures; browser checks use the real entry and isolated runtime. Real private-account access, native installed interaction and business UAT are separate evidence tiers.

Implementation: Rust task_sources module owns provider HTTP and a separate user-data SQLite store. Windows DPAPI protects tokens. The existing workspace composer and terminal workflow consume cached source identities. Task UI does not execute remote issue text by itself. External creation requires a deliberate user submit; uncertain create responses are not automatically retried.

Official contracts: https://docs.github.com/en/rest/issues/issues ; https://docs.gitlab.com/api/issues/ ; https://linear.app/developers/graphql ; https://developer.atlassian.com/cloud/jira/platform/rest/v3/api-group-issue-search/ ; https://developer.atlassian.com/cloud/jira/platform/rest/v3/api-group-issues/ . Orca source comparison used its task-page workspace actions and provider implementations.

Critical path: adapters and persistence -> task page and composer links -> isolated tests/build -> independent review -> candidate. Installation requires separate authority.

Release continuation: preserved the current installed Workbench menu removal (SHA-256 fb84e381180178cdad93d351a1974d991d31c2436ece0c3ae18c5599ec223282). Both Release binaries built successfully with embedded Tauri assets. An isolated native window verified the public GitHub task -> only-create worktree -> return to source -> reopen same worktree flow and the four provider tabs. No external issue was created. The native profile had zero sessions after the flow. Candidate processes were removed after identity checks.

Upgrade acceptance: an isolated live ConPTY host and shell survived old -> new -> old daemon handoff with unchanged PIDs, layout and prior history bytes. The installation helper has read-only preparation by default, exact artifact/process checks, all-GUI exit gates, legacy and workbench host protection, SQLite backups, history-prefix checks and individually recorded rollback steps. Existing database state must also remain unchanged; failed rollback validation is not reported as success. Actual installation and failure injection remain unperformed. See artifacts/task-sources-20261018/continuation/ for receipts and the concrete pending installation plan.
