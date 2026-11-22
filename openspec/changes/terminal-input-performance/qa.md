# Terminal input and performance evidence — 2026-10-27

Latest status: the export gap recorded below has been closed by the native save
receipt change. See [export-receipt-qa.md](./export-receipt-qa.md) for current
installed hashes, real saved-file acceptance and startup timing. The following
sections retain the earlier candidate's evidence and chronology.

## Outcome and acceptance

The installed candidate repairs Electron 44 clipboard text/image handling and adds
an explicit “导出终端性能记录” action. The total user-facing objective is reliable
input and measurable startup latency while preserving existing CLI sessions.
Full acceptance is **not complete**. Installed-window Chinese paste and a real
OS IME candidate commit have now passed. Claude was visibly ready by the 6.255 s
observation in one run. Installed export still lacks a saved-file receipt; exact
stage timing and repeated user-perceived startup acceptance remain pending.

## Implementation and development verification

- Single-snapshot asynchronous Electron clipboard reads; awaited text writes;
  PNG/JPEG/WebP image decoding to PNG, with text/image size limits and legacy API
  compatibility.
- A bounded 512-entry memory-only recorder per terminal. Export is explicit;
  command text, output, keystrokes and clipboard contents are excluded.
- Input/queue/RPC/output/parser/render/frame-opportunity timing is recorded.
  A frame opportunity is not proof of physical display completion.
- Six unit tests, four browser cases, IME replay at 1x/1.5x/2x, build, type checks,
  syntax checks and scoped diff checks passed. The existing bundle-size warning
  remains. IME event replay does not establish real OS candidate-window behavior.
- A real Electron clipboard roundtrip passed for Chinese multiline text and image
  conversion to a valid PNG. The original clipboard was restored.

Evidence is under `artifacts/terminal-input-performance/`: `acceptance.json`,
`clipboard-native.json`, `performance-export.json`, `ime/browser-regression.json`
and `build.log`.

## Independent review and technical disposition

The independent read-only reviewer ran all six unit tests successfully and found
no newly introduced blocking defect. It reported existing unbounded clipboard PNG
retention as requiring a policy fix. That issue is deferred: deleting images now
could break history references. This is an explicit disposition, not an
unqualified reviewer pass. No cache files were removed.

## Integration, installation and online state

Only the accepted renderer and narrow clipboard helper/IPC change were packaged
into the installed application. Eleven other installed non-renderer files were
preserved, as were the runtime path and prior background-throttling repair.

- Installed executable: `D:/orcas/artifacts/installed-no-codex-chat-20261027/Velella.exe`
- Installed app archive SHA-256:
  `bdf431458b57a2ce40fc67c6b75d257b0df772adf39637ea316ac4bd5c8ee188`
- Runtime SHA-256:
  `01c24d59951e96c204b41e02a58059bb0b18ca1157fc31910cb557f0966a123b`
- Backup: `artifacts/terminal-input-performance/backup/`
- Installation receipt: `artifacts/terminal-input-performance/installation.json`

Installation verified preserved daemon/session process identities and history
prefixes. The receipt remains `installed_backend_verified`; it is not promoted to
full native UI acceptance.

## Native verification and business UAT

An isolated PowerShell verification session was created and its empty prompt was
observed. On continuation, the UI tool reported the Velella window was minimized
and could not obtain its state. No further input was sent. A read-only output
check confirmed the owned session remained at its initial prompt. Only that
session was stopped successfully with `confirmed: true`:
`01a17d88-8534-7561-94bc-88a3f6201a15`.

See `artifacts/terminal-input-performance/native-ui-result.json`.
The original user Claude session was not used for test input or stopped.
The remaining acceptance is native clipboard/export interaction, actual Chinese
IME composition and end-to-end perceived Claude readiness. Prior backend and
browser timing improvements do not prove the reported ten-second delay is gone.

## Resumed native verification

On the user's continuation, the installed archive and runtime hashes still
matched the accepted candidate. The documented app launcher restored the existing
window through the application's single-instance activation handler.

A new owned PowerShell session was created:
`01a17dab-e6f7-7754-bdfd-2a465064be61`. Installed-window text paste passed:
`Write-Output '中文粘贴验证'` was visibly pasted and the backend output contained
the expected Chinese result followed by a fresh prompt. Native key presses for
`nihao` displayed a real IME candidate window with `你好` as the first candidate.
This establishes candidate-window display only, not final composition correctness.

Before candidate submission, the UI tool reported user input in the window; the
next observation reported it minimized. Further UI input stopped. The owned
session is retained pending the user's availability for the remaining checks.
No Claude command was launched in this resumed run. Native commit-once behavior,
installed-menu JSON export and full startup measurement remain incomplete.

Evidence: `artifacts/terminal-input-performance/native-ui-resume-before.json`,
`native-ui-resume-session.json` and `native-ui-resume-output.json`.

## Completed native checks after the user resumed access

The interrupted partial shell line was cancelled. A fresh native IME sequence
`nihao` followed by Space selected `你好`; executing the harmless Write-Output
command produced exactly one `你好` result. Chinese clipboard output also passed.
The application remained at its existing input-method configuration.

In that owned session, `claude` was pasted and one native Return executed it.
At 353 ms the screen showed the shell's next line; at 6,255 ms a screenshot showed
the complete Claude welcome panel and input prompt. This is a single observation
upper bound, not the actual time the frame first appeared, a keyboard-typing
equivalence claim, a repeated-startup guarantee, or a measured stage breakdown.
No model prompt or trust approval was submitted by this test.

The owned process was stopped with `confirmed: true`, and its UI tab was closed.
The original user session was not stopped. Final native check metadata is in
`artifacts/terminal-input-performance/native-ui-resume-result.json`.

## Export acceptance gap

The installed toolbar export was activated once by mouse and once by keyboard
with the correct menu item visibly selected. Neither attempt yielded a matching
file in the OS Downloads directory or a visible save dialog. Do not mark export
as passed or promote the installation receipt to fully verified.

Two isolated diagnostics narrow the gap without proving installed acceptance:

- `export-toolbar.browser.mjs` exercises the real TerminalView toolbar action and
  validates the downloaded JSON; passed after supplying the existing Playwright
  dependency path. The initial missing-module error was a harness dependency issue.
- `export-native-probe.cjs` executes the source export function in an isolated
  Electron window with the same navigation/permission restrictions, captures
  `will-download`, and provides an explicit destination. It saved valid JSON.
  The initial three-second observation ended too early; a fifteen-second bounded
  observation captured completion. See `export-native-probe.json`.

Thus the toolbar callback and Electron Blob generation can work. The installed
default save/download flow has not been identified as the root cause. No product
code was changed based on this uncertainty, and no further identical UI retry
was made. The next gap to close is an installed export with an explicit saved
path/receipt, followed by an end-to-end startup trace. Existing accepted input
and rendering code remains frozen.
