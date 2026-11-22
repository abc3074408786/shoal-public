# Bound hidden-terminal active polling

User outcome: lower background work with several streaming terminal tabs while
the active terminal stays responsive and all output appears after switching back.

Scope: when a TerminalView is hidden and its output page has caught up with the
host, wait at least 250 ms before the next periodic read, including when the last
page changed. Retain progressive idle backoff, immediate backlog draining, explicit
wake/reveal, serialized reads, persistent host recording and all replay semantics.
Do not change window-minimization behavior, protocol reply routing, frontend
history, renderer selection or the accepted foreground startup repair.

Acceptance: reproduce high hidden-active read count with the production scheduler;
bound caught-up hidden reads to 41 per 10 virtual seconds including initial read;
retain existing backlog/input/visible streaming/disposal tests. Exercise actual
TerminalView in a browser with continuous hidden output, compare read counts to
the fixed baseline, reveal it and verify exact final output and responsive input.
Independent read-only review precedes freezing and installation. Preserve installed
non-renderer files, runtime identities and existing sessions during update; verify
the installed change with an owned terminal or isolated packaged-renderer evidence.

The measured metric is output requests, not a promised CPU percentage. Large
continuous backlogs still drain promptly and are outside this reduction bound.
