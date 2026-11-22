# Orca-reference light workbench

Scope: refine the existing Velella desktop light skin. Keep navigation, fish identity, projects, terminal/input behavior, saved geometry and dark mode. This is an Operate surface, not a new product or a new visual identity.

User direction: the flat all-white 0.2.10 treatment was rejected. Reference Orca for a useful white interface, with surface hierarchy instead of erasing every background. No alternative direction or new asset is required.

2026-11-01 follow-up: user rejected the large pure-white area as too bright. Supersede the white-paper color requirement below with a neutral gray-white canvas `#f1f1f1`, sidebar `#e5e5e5`, chrome `#eaeaea`, prompt/input blocks `#e3e3e3`, active tabs `#dcdcdc`, and boundaries `#d3d3d3`. Preserve layout, ANSI colors, selection visibility and all running sessions. Neutral fills emitted by already-light CLIs also use the darker prompt tone; no green tint or decorative motion is introduced.

2026-11-01 polish follow-up: user requested a more attractive finish. Keep the accepted canvas/input colors and structure. Refine chrome to `#e8eaed`, sidebar to `#e7e9ec`, muted labels to `#626973`, boundaries to `#d3d7dc`, and use `#49658b` only for functional emphasis. Active tabs connect visually to the canvas with a quiet boundary, while navigation has a muted blue selected state. Keep visible hover/focus states and project-bar hiding. Add platform CJK sans fallbacks after the existing Latin monospace choices, preserving the selected font and terminal grid. No new branding, animation, cards, dependencies or terminal commands. Verify at 1280x720 and 900x700 before native deployment; native confirmation and process/history preservation close delivery, with final aesthetic acceptance left to the user.

2026-11-01 font correction: user rejected the typography change. The polish paragraph's CJK fallback change is revoked. Restore the exact pre-polish terminal fallback stack, preserve the user's font settings and keep the accepted visual refinements. Future color/layout polish must preserve terminal typography.

Reference facts: `D:/orca/src/renderer/src/assets/main.css:176-240` supplies white canvas, `#f5f5f5` sidebar/muted surfaces, `#e5e5e5` borders and neutral text. Its chat user surface mixes 4% foreground into the canvas. `D:/orca/src/shared/terminal-themes/defaults.ts:31-55` supplies Builtin Tango Light, its complete ANSI palette, and a blue selection. These are inspected source facts, not a claim of pixel parity with the installed Orca binary.

Direction contract:

- White terminal paper; quiet gray titlebar/sidebar and a light active tab. No new cards, decorative gradients or motion.
- Historical Codex prompt/input blocks use a neutral `#f5f5f5` fill; inverse selections and actual text selection remain visible. Semantic ANSI foreground colors remain readable on a light surface.
- Preserve original buffer and recordings. The renderer may adapt old dark fills; it never rewrites recorded bytes or sends model input.
- Status and recovery stay findable in a compact row. An exited session with no work records does not show an empty unavailable-work-record strip; live errors and saved records remain available.
- UI labels keep the existing type family and compact native controls. Verify at desktop width and the app's narrow desktop size; there is no mobile application target for this change.

Acceptance: the actual installed Codex history should show distinct but pale input blocks, readable body/metadata and selection, neutral chrome, no green background, and unchanged project/session/history identities. Review is bounded to one rendered inspection and one correction batch. No randomized concept seed is applicable to this precise reference adaptation of an existing surface.
