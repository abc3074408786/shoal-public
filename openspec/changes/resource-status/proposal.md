# Desktop resource and connection status

User-visible result: bottom-right status-bar indicators for memory, running
terminal count and connection state, each opening useful details. The project path
on the left shrinks to keep all three indicators accessible at 800–1440 px.
This is an independent implementation based on
the requested interaction, not copied Orca code.

Memory means the sum of Electron desktop-process private bytes, converted from
the API's KiB to bytes; it excludes Rust runtime, Shell/Claude/Codex subprocesses
and is not resident physical memory. Missing measurements remain unavailable,
not zero. Details identify process type, PID and the same memory value.

Terminal count is scoped to the selected persisted workspace. Count live
terminal-kind, non-chat sessions with a valid host PID, null exit status and no
failed/host-lost state, including sessions without open tabs. Details open the
chosen existing session through the workbench's existing navigation path.

Connection status reuses the existing snapshot loop: initial connecting,
connected, retrying after failures, and failed after three consecutive failures;
automatic retries continue. Resource sampling is serialized at five-second
intervals and stops while the document is hidden. Workspace changes cannot show
the previous workspace's count. IPC is restricted to the trusted main frame.

Acceptance: unit tests for units, unavailable fields, cache, counts and states;
native Electron/preload IPC measurement; real WorkspaceApp browser checks for
details, session navigation, failure/recovery, hidden polling and 800–1440 px
layout; build and independent review. Install only accepted changes over the
current archive, preserving the runtime, other native modules, user sessions
and history. Verify the installed bottom status bar and native details before completion.
