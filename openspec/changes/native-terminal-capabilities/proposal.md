# Preserve native Windows CLI terminal capabilities

User result: Velella preserves native Codex conversation and interaction when the user runs the same CLI as in PowerShell. Complete replies, CLI-rendered input areas, and native keyboard behavior must remain available.

Current evidence: the real greeting stream and TerminalView replay contain the complete replies and footer. They do not prove a model-capability difference. The current system ConPTY consumes OSC 10/11 queries; the same isolated query fixture succeeds with bundled ConPTY. Repeating this through Velella's existing runtime succeeds with Microsoft.Windows.Console.ConPTY 1.24.261001001 sidecars and fails without them.

Scope: pin the Microsoft package and file hashes, prepare its matching conpty.dll/OpenConsole.exe pair with MIT notice/provenance, and include it next to the runtime in Windows builds. portable-pty already prefers this sidecar library. Do not change the shared Cargo registry, models, prompts, account configuration, renderer appearance, existing session logs, or existing hosts.

Critical path: real host protocol comparison → deterministic package preparation → real Codex startup with native palette → independent fixed-candidate review → additive installation preserving host identities/history → new live terminal observation. Existing hosts retain their backend; the fix applies to new hosts.

Acceptance agreed before delivery:

- The unchanged runtime forwards both OSC queries and the child receives both valid RGB replies with the pinned sidecars.
- Real Codex startup renders its native input background when the real terminal replies to its palette queries. No model answer is inferred from a protocol test.
- Windows packaging includes the executable pair, license and provenance beside the runtime; missing/corrupt dependencies fail preparation.
- New host input/interrupt/exit and resize work; recorded history replays without sending input.
- Installation adds only the fixed dependencies; all existing host process identities and fixed history prefixes remain intact. Existing sessions are not stopped or automatically resubmitted.
- Keep implementation, developer tests, independent verification, installation, live observation and business UAT distinct. Greeting tests cannot establish general model quality or broad CLI UAT.

Evidence: artifacts/terminal-reply-parity-20261028/. x64 is the current target; arm64 package hashes are pinned but native arm64 validation is not claimed.
