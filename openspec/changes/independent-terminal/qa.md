# Verification — 2026-10-27

The local source replacement was subsequently installed on explicit user request.
See the deployment addendum below for the current installation state.
Starting repository HEAD: `62769b4a8ea0ec4c7f347c16726888cbaf3db64a` with existing
uncommitted work. Exact pre-edit copies of affected files are preserved under
`artifacts/independent-terminal-20261027/baseline`; unrelated work is retained.

## Implementation

- Replaced the five previously attributed helper modules. Unicode now delegates
  to official `@xterm/addon-unicode-graphemes@0.4.0`; it remains an experimental
  upstream addon, so tested Unicode cases are enumerated below.
- Deleted the two vendor patches and their postinstall script. Verified every
  distributed file of installed xterm and WebGL against npm tarballs: 123 + 29
  files, zero mismatches. Checked tarball/lockfile SHA-512 against the official
  registry, including the new grapheme addon.
- Implemented IME commit/cancel handling outside xterm, using ancestor capture
  listeners and public `Terminal.input`. Composition preedit is a project-owned
  DOM overlay. The original terminal host and input-provenance checks remain.
- Replaced theme tokens with Velella semantic surfaces and retained existing
  component token names. Historical Orca MIT attribution remains in the repo;
  no claim of whole-repository clean-room provenance is made.

## Developer checks

- `npm run build`: passed, existing large-bundle warning remains.
- `node --test tests/terminal-owned.test.mjs tests/terminal-parity.test.mjs
  tests/terminal-ime-anchor.test.mjs tests/terminal-input.test.mjs
  tests/terminal-frames.test.mjs`: 25 passed. Includes CRLF copying, fractional
  wheel distance/reversal/bounds, CJK, combining marks, family ZWJ emoji, skin
  modifiers, flags, PUA, replay provenance and immutable history frames.
- `terminal-ime.browser.mjs`: passed on headless Edge at 1x/1.5x/2x, dark/light,
  using actual TerminalView with mocked RPC and synthetic OS-style events.
  Covers one commit for keypress+input, delayed/missing input, intentional repeated
  characters, composition redraw, Enter, trailing events, blur/refocus, Escape,
  immediate new composition, read-only rejection and session disposal.
- The initial IME run exposed duplicate input because xterm's target capture
  listener ran first. Moving the interceptor to ancestor capture fixed the
  cause; the bounded retry passed. Added edge scenarios passed subsequently.
- `terminal-ime-preposition.browser.mjs`: passed at 1x/1.5x/2x, including preparing
  the recognized painted caret before composition starts and disposal.
- `terminal-parity.browser.mjs`: passed at 1x/1.5x/2x: copy/paste ownership,
  normal/TUI scrolling, search, preferences, real WebGL and context-loss fallback.
- `terminal-cursor.browser.mjs` with `CURSOR_QA_SCENARIOS=dom:1,webgl:1.5`:
  passed both scenarios, including focus/visibility, theme changes, application
  cursor overrides and IME. Other cursor/DPI combinations were not rerun.
- Scoped `git diff --check`: passed after normalizing edited source line endings.
- Design-pipeline doctor: ready. Inspected the terminal screenshot: aligned
  controls, readable Chinese content, preserved gray-green terminal surface.

## Evidence and limits

Evidence root: `artifacts/independent-terminal-20261027/`.
`registry-integrity.json`, `upstream-integrity.json`, `build.log`,
`ime/browser-regression.json`, `ime/preposition-regression.json`,
`parity/browser-parity.json`, `parity/browser-terminal.png`, and cursor output
record the checks above. Browser fixtures use isolated loopback servers and
mock runtime requests; they do not drive authenticated CLI accounts.

Independent review: passed. A separate read-only reviewer inspected the bounded
replacement, checked removal of patch/install entrypoints, and independently ran
15 unit tests (owned helpers, parity and IME anchors), all passing. No reproducible
blocking regression was found. This is technical acceptance of the local source
replacement, with browser coverage supplied by the developer's evidence above.
The implementation is frozen at the source hashes recorded with that evidence.
At the source-acceptance stage, no installation, production restart,
current online validation, real OS IME candidate-window UAT or business UAT was
performed. Visual inspection is not the owner's visual acceptance. Rare upstream
WebGL atlas/marker bugs are not certified absent by these bounded regressions.

## Deployment addendum — 2026-10-27

Explicit user authorization covered the installed update and subsequent bounded
native acceptance attempts. The final corrected fixture passed renderer startup,
sandboxed preload/IPC, real PowerShell execution through the new UI, and the
project-owned composition preedit. Earlier fixture failures and their corrections
are retained in `artifacts/independent-terminal-20261027/deployment/status.md`.

Installed the accepted renderer and Unicode dependency metadata into the existing
Velella app.asar. Preserved all 11 installed Electron files and the existing runtime;
verified their hashes. Backed up the old archive and database/config snapshots.
Existing runtime process identities and terminal-history prefixes were unchanged.
Opened the installed desktop and observed the connected backend, terminal input
and original PowerShell prompt using native accessibility and screenshot tools.
Installation receipt: `artifacts/independent-terminal-20261027/deployment/installation.json`.
Real system IME manual UAT and business UAT remain not run.
