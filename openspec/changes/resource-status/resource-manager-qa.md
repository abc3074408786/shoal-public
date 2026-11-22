# Resource manager delivery — 2026-10-28

Installed user capability: click the bottom-right memory indicator to open a
resource manager above the status bar. Expand projects/workspaces to inspect
terminal-host process trees, CPU, WS and Private; switch the memory column; open
an existing session across workspaces. The footer value remains explicitly
desktop-private memory while the panel includes desktop plus assigned terminal
process trees. Resource collection is lazy and stops when closed/hidden.

Space: explicit read-only scan, bounded counts with “at least” for incomplete
results, individual workspace rescan, preview of four known build-cache paths,
explicit confirmation and OS recycling. General source/worktree deletion is not
implemented. User workspaces were not cleaned during this delivery.

## Evidence layers

- Implementation/integration: complete for the documented resource-manager and
  build-cache scope in `resource-manager.md`.
- Development tests: 5 unit tests passed, including CPU identity/normalization,
  host descendant attribution and deduplication, lazy sampling, invalid/stale
  previews, active-session refusal, bounds and junction handling.
- Browser integration: passed. Tests include 800×520 and 1440×960 panels,
  800–1440 footer layout, changing WS/Private, cross-workspace navigation,
  failure recovery, closed/hidden sampling pause, manual scan, partial-cache
  disable, cancel and confirm of exact paths. No terminal lifecycle/input RPCs.
- Native integration: actual Electron/preload IPC with real runtime session
  metadata and Windows process metrics passed. Real host descendants observed.
  OS recycling passed only on dedicated `cleanup-fixture-*` cache directories;
  fixture source remained intact.
- Build: passed, existing large-bundle warning retained.
- Independent review: `resource_manager_review` inspected the full scope and
  found one incomplete-scan marker defect. After correction,
  `resource_scan_fix_review` confirmed both skip paths and independently passed
  all 5 tests. It did not repeat the full review.
- Technical acceptance: passed; source hashes frozen before package preparation.
- Deployment: installed archive matches candidate; runtime hash unchanged;
  preexisting runtime host identities and history prefixes preserved. Backup is
  retained beside the deployment receipt.
- Installed GUI: the bottom-anchored resource tree displayed project/workspace/
  terminal hierarchy and actual CPU/WS/Private readings, including real host PID
  metadata. Screenshot and AX capture saved. The app was minimized before the
  attempted installed scan click; no installed scan result or user cleanup is
  claimed. Scan/cleanup functionality is covered by native fixture + browser tests.
- User UAT: not yet reported. User-session navigation and induced host failure
  were not executed in the installed user workspace.

## Corrections and bounded attempts

The first browser run exposed the existing native confirmation dialog being
hidden by Radix `hideOthers`. Replaced it locally with a nested Radix confirmation;
the second browser run passed. Initial deployment preflight stopped before backup
or mutation because Python decoded UTF-8 acceptance evidence as GBK. Explicit
UTF-8 reads fixed the script; the second deployment passed. Neither failure caused
user-file cleanup or a partial archive replacement.

## Fixed package and evidence

- App SHA-256: `90d356ca8d78e290fdea884af0cf35807c1929061f4d0b8cce9287b7544c630e`
- Runtime SHA-256: `01c24d59951e96c204b41e02a58059bb0b18ca1157fc31910cb557f0966a123b`
- Installed archive: `D:/orcas/artifacts/installed-no-codex-chat-20261027/resources/app.asar`
- Evidence: `D:/orcas/artifacts/resource-manager-20261028/`
- See `plan.json`, `source-hashes.json`, `acceptance.json`, `installation.json`,
  `browser-result.json`, `native-result.json`, `installed-manager.png`,
  `installed-manager-ax.txt`, and `backup/`.
- Native package prepared from the prior installed archive: two new resource
  modules, three narrow IPC/factory patches, rebuilt renderer, 12 other installed
  files preserved byte-for-byte. Rust runtime was not rebuilt or replaced.

Limits: CPU first sample is unknown; WS includes shared pages; unassigned daemon
processes are excluded; space reports logical sizes, not physical disk allocation,
and nested roots are not added together. Cleanup metadata checks reduce stale
preview risk but cannot prevent an external writer from changing files between
the last check and the operating-system recycle operation.
