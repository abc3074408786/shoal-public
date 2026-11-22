# Terminal window scheduling — verification, 2026-10-27

Implemented and installed backgroundThrottling=false on the main terminal window.
This removes a proven hidden-window scheduling delay; it does not certify the
user's entire foreground Claude startup latency as fixed.

## Evidence

- Actual output pump, native Electron 44.9.2, 12 serial pages: default hidden-window
  scheduling 10,402.3 ms; disabled throttling 127.5 ms. Regression assertions passed.
  Run Electron with `desktop/tests/terminal-scheduling.native.cjs`.
- Earlier isolated timer probe: 11,616.1 ms versus 138.5 ms. The initial probe
  stayed visible and did not reproduce; hiding the loaded window established the
  required condition. No user profile or PTY was used by either native test.
- JavaScript syntax and scoped diff whitespace checks passed.
- Installed archive changed only electron/main.cjs; all 113 other packaged files
  were verified unchanged. Runtime hash, persistent process identities and terminal
  history prefixes were retained. Desktop reopened through its normal startup;
  daemon PID remained 31828. Native accessibility verified a connected backend,
  two retained PowerShell tabs and terminal input.
- Evidence: `artifacts/terminal-window-scheduling/result.json`, `plan.json`,
  `installation.json`; rollback archive and data snapshots in `backup/` there.

## Acceptance limits

Developer checks, native regression, installed-package and online UI checks passed.
No separate independent reviewer was used for this small option change. Real-user
Claude startup UAT remains pending. Extra background CPU work is an intentional
tradeoff; application polling/backoff is retained.

The user's manual capture emitted first-screen bytes after about 3.84 s while the
user perceived >10 s. That observer did not measure physical presentation or full
CLI readiness. Orca's SessionStart hook has a 10 s configured timeout but completed
near 0.1 s in the scoped debug test; it was not disabled. No global Claude settings,
updater behavior, trust or account configuration changed.

The separately observed clipboard.readImage incompatibility remains open. Existing
user CLI sessions were not stopped or given input during installation.
