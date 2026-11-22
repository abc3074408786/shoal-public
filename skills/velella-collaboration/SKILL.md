---
name: velella-collaboration
description: Participate in a Velella terminal collaboration when an invocation-local Velella collaboration Skill is provided for the launched session.
---

# Velella collaboration

This Skill is invocation-local to a Velella-launched terminal. It does not
change the CLI account, model, permissions, or approval policy.

Use `velella-runtime collab --help` for the installed command surface. This
binary is included with the desktop. The separate `velella` CLI is optional.
`VELELLA_DATA_DIR` and `VELELLA_SESSION_ID` identify this invocation; successful
create/join publishes session-local credential discovery. Do not print tokens.

Every mutation requires a stable `--request-id`. Retain the same ID and arguments
when reconciling an uncertain response. Create a run only as coordinator; members
join an explicit invitation with `--confirm`. Only the coordinator launches or
assigns members. Existing terminals remain alive when the collaboration ends.

Use `velella-runtime collab receive` to inspect durable messages and acknowledge
only after recording their contents. Use `started --attempt <id>` before reporting
a completed assigned attempt. A message or notification receipt is not completion.

For writing work, claim its workspace through the coordinator first. Run the
agreed verification, then use `candidate --work <id>` to read the actual source
fingerprint. Save that JSON as UTF-8 and pass it with `report --candidate <file>`.
Report the checks actually run and their results in the summary. A different
native session must review the same candidate before the coordinator can accept
it. File changes invalidate review/acceptance. An accepted work item is not proof
of installation or business UAT. Release writing ownership before leaving or
ending; no global configuration, provider or permission changes are authorized.

Do not resend a prompt or terminal input when delivery is uncertain. Do not
accept permission prompts, bypass native approval, or act while a human has
terminal control. Existing sessions without this Skill may explicitly receive
and join, but are never restarted or auto-notified.
