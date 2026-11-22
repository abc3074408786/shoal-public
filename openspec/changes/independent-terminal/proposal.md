# Independent terminal integration

Replace the confirmed Orca-derived terminal helpers, npm patches and theme tokens with project-owned implementations and unmodified official xterm packages. Preserve selection indentation, Unicode grapheme width, normal and TUI scrolling, IME commit/cancel/focus behavior, renderer fallback and light/dark usability. Do not change Agent orchestration or install/restart the production application.

Use the official Unicode graphemes addon. Own browser IME transactions at the terminal DOM boundary and submit through xterm's public input API. Normalize wheel distance linearly with fractional carry and a per-event bound; do not reproduce the previous acceleration curve. Use public buffer cells to locate recognized painted carets. Derive UI tokens from the existing Velella surface direction. Stop applying vendor patches and verify installed distributions against official npm tarballs.

Acceptance: targeted unit tests, production build, real TerminalView browser tests for IME, clipboard, scroll, renderer and session disposal; record independent review separately from developer checks. Native OS candidate-window UAT and deployment remain separate. Retain the old MIT notice for repository history; do not claim a legal clean-room audit or that all third-party code has been removed.
