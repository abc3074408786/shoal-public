---
name: Velella interface foundations
---
# Velella interface foundations
## Product context
A local desktop workbench for coding agents. Users configure tasks, supervise
permissions and inspect changes before delivery. Execution scope must be explicit.
## Overview
Preserve the existing compact workstation interface. Status and recovery actions
have priority over decoration. User-facing copy is concise Simplified Chinese.
## Colors
Use semantic variables from desktop/src/theme.css, including --foreground,
--muted-foreground, --border, --danger and --status-success in both themes.
## Typography
Reuse --font-sans and --font-mono. Form labels and descriptions remain readable;
long paths wrap instead of widening dialogs.
## Layout
Reuse desktop/src/style.css and orca-shell.css. Dialog body scrolls independently
and retains access to footer actions. Form rows stack at narrow widths.
## Components
Reuse ui.tsx Button, Input, Textarea and NativeDialog; native selects for small
exclusive choices. Inputs have visible labels; errors use role=alert and async
status uses role=status. Preserve keyboard operation and focus-visible styling.
## Do's and don'ts
State what a permission allows. Keep unavailable and unverified distinct. Never
silently downgrade isolated execution or claim that a worktree is a security boundary.
Do not add decorative motion or new libraries for settings controls.
## Source decisions
Adapted from this repository's existing theme.css, style.css, motion.css, ui.tsx
and task form. This documents current conventions, not a new visual redesign.

Adopted: existing semantic tokens and native controls. Rejected: new visual themes or dependencies for settings forms.

## Desktop light workbench

The light project/terminal surface follows the Orca reference in
`docs/design/orca-light-workbench.md`: softened `#f1f1f1` terminal paper,
`#e5e5e5` sidebar, `#e3e3e3` input/message surfaces, `#dcdcdc` active tabs,
`#d3d7dc` boundaries, neutral UI ink, and
the complete Builtin Tango Light ANSI palette with a visible blue selection.
Keep message/input/selection hierarchy; do not erase every ANSI background into
the canvas. Adapt retained dark TUI fills only at render time, preserving buffer,
recordings, native input, saved layout, existing dark mode and the fish identity.
Use compact status/recovery controls and hide an empty unavailable work-record
strip for an exited session. This is a scoped light skin, not a new navigation
or a replacement settings design. Reference and acceptance evidence live with
the surface brief and release record.

The refined light chrome uses a quiet gray-blue sidebar and small functional
blue accents, canvas-colored active tabs, consistent icon weight, and compact
controls with visible hover/focus states. Preserve the existing terminal font
fallback stack and user preferences; visual polish must not change terminal typography.

Agent settings reuse the same gray-white surfaces in a full settings page:
a category sidebar with Back to app, plain list rows, the approved dual-column
Agent picker and persistent footer actions. Preserve the mounted project behind
the page; settings participate in back/forward navigation without a modal backdrop.
Keep installation/detection status factual and distinguish unknown from missing.
Settings only affect future launches; retain access to existing sessions.

## Orca project navigation

Project pages use the supplied Orca screenshot as visual authority. Keep the
homepage independent. The workspace sidebar defaults to 280px and resizes from
220 to 500px under its own preference. Folder headings are 28px tall; workspace
cards have a 20px status slot, 13px/20px titles and a 16px host badge. Match the
reference's serif navigation with local Georgia/SimSun fallbacks, scoped to
project navigation and menus; terminal fonts retain user preferences.

Dark sidebar is #2a2a2a, selected card #404040 with #656565 boundary. Light sidebar
is #f5f5f5, selected #e6e6e6. Menus are 208px wide, 11px radius, compact rows,
clear disabled states and neutral hover feedback. Respect keyboard focus and
host-bound action ownership. Dark terminal canvas uses #282c34 with #abb2bf ink
and #3e4451 selection; light terminal/ANSI adaptation remains unchanged. No
visual change may alter terminal history, input, runtime processes or saved layouts.
