# Settings extension — 2026-11-02

User outcome: one discoverable settings page for existing tools, appearance and editing, customizable app shortcuts, task desktop notifications, and Codex/Claude account configuration directories. Extend the accepted gray-white Operate surface, category navigation and persistent Save/Cancel footer. Do not redesign the Agent picker.

Baseline: working tree based on 3cdb9a4, already contains extensive unrelated edits. Preserve them. Original Orca D:/orca is read-only reference.

Acceptance before implementation: draft edits never apply until Save; Cancel/Back discard; preferences survive reload; storage errors and concurrent-window edits are explicit; all settings have a real consumer. Keep mounted terminals and saved session descriptors. Scope interface/editor/terminal settings to this desktop; scope Agent/account choices to the selected host. Existing tool links must reach their real pages, with unsaved draft confirmation before leaving.

Accounts: register existing absolute configuration directories by name, choose the directory for newly opened native Codex/Claude terminals, inspect login status, and open an explicitly requested login terminal. Only metadata is stored in UI preferences. Never copy/display tokens, delete account directories, change the OS home, or silently switch an existing/recovered/replayed session. Restricted/task/orchestrated launches retain their own policy. Unsupported runtimes must reject custom profiles, not silently use the default account. Directory profiles do not promise isolation from API keys inherited by the CLI.

Notifications: opt-in; initial snapshots, reconnect snapshots and host switches do not notify historical changes. Notify subsequent configured phase transitions once; clicking targets the issuing host's task. Test notification reports dispatch only, not OS delivery. Automatic file saves are opt-in, debounced and use existing conflict checks; errors stop automatic retries.

Validation: TypeScript/build, focused persistence/shortcut/notification/account policy tests, isolated browser fixture at desktop and narrow sizes (Save/Cancel/reload, real option consumers, no overflow), backend account-request and replay tests, isolated synthetic CLI/profile integration. No real model requests, provider login, credential changes or runtime replacement during validation. At most one changed-condition retry per root failure. Deployment/online checks and real-account UAT are separate from source/build evidence.
