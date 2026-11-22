# Resource manager correction — 2026-10-28

Final user-visible outcome: the existing bottom-right memory entry opens a compact
resource manager anchored above the status bar. Match the provided interaction:
CPU / sum WS / sum Private, expandable project → workspace → terminal rows,
recent CPU trends, refresh, workspace cleanup preview, and on-demand space scan.
Own implementation, no Orca source copied. The existing footer memory remains
explicitly desktop-only; panel totals include terminal-host descendants and
desktop processes. No samples run for the resource tree until the panel opens.

Metrics: Windows process CPU deltas normalized to machine logical processors;
first sample unknown. WS and Private are separate byte metrics. Session host PIDs
come from runtime metadata, child edges require valid creation order, roots are
not double-counted. Electron processes are separately grouped. Unavailable values
remain unknown; WS may double-count shared pages. Unattributed daemon/background
processes are excluded and the panel explains this. Recent sparkline is local to
this panel session and does not invent history. Existing session navigation must
work across workspaces without process start/stop/input side effects.

User selected scan → preview → clean caches/files rather than archive. This slice
supports explicit regenerable build-cache directories: .vite, .turbo,
.parcel-cache, node_modules/.cache. It does not guess arbitrary source or worktree
deletion rules. Scan is read-only, bounded, explicit, skips links and reports
incomplete counts. Per-workspace logical sizes may overlap; no global sum.
Only complete cache scans without skipped entries can be selected. Cleanup needs
the user's UI confirmation of exact paths and sizes, a recent server-owned preview
token, unchanged workspace/path/fingerprint, and no live/starting/unknown sessions.
Use the OS recycle bin. No cleanup of the user's data is authorized in agent tests;
native cleanup validation uses only a dedicated fixture cache.

Acceptance agreed before review: focused unit tests (attribution, CPU identity,
cache/serialization, scan limits, junctions, stale preview, active-session refusal),
browser integration (tree, 800×520/1440×960 geometry, cross-workspace navigation,
closed/hidden polling stop, unavailable recovery, manual scan and confirmation),
native Electron/preload IPC with real runtime metadata/processes and disposable
cache recycling; build; independent review; frozen narrow archive patch; guarded
installation preserving runtime/terminal processes/history; installed UI inspection.
Native live user-process failure injection and user business UAT are separate.
