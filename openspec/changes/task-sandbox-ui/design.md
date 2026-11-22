# Design decisions
Primary route: existing UI craft. User job: choose the execution boundary before
starting an unattended coding task. Existing theme.css, style.css and motion.css
are the authoritative visual foundations; record them rather than redesign them.

A native select offers local and isolated execution. Capability status is separate
from selection. Loading/error/unavailable states never assert readiness. An explicit
isolated selection is retained when probing fails, and blocks submission. Native
mode remains available. Direct-folder projects explain that isolation needs Git.

Advanced read-only/writable directories and network choice are only shown for
isolated mode. Network defaults to denied and describes full host access when
selected. The existing account check is explicitly labelled as a host check.
Task details show the saved policy, not current machine capability.

QA: real browser interaction against a local fixture RPC, desktop and narrow
viewport; stale capability response, error retry, request payload, keyboard labels,
long error wrapping and submission gate. Fixture evidence is not kernel acceptance.
