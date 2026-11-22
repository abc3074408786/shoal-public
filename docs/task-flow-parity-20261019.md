# Source task workflow parity

User outcome: use source tasks in Velella with Orca's development flow, not just four HTTP integrations. The user confirmed the entire task flow on 2026-10-19. Existing installed Release is the protected baseline; its source and hashes remain in artifacts/task-sources-20261018/continuation/source-b14937bddf6a.

Acceptance: existing Git projects discover GitHub/GitLab remotes and reuse gh/glab authentication without copying tokens into forms; Linear/Jira connect independently of a development project and select teams/projects while browsing; selecting an issue opens a project/Agent composer with its task instruction; an Agent receives a draft without automatic submission; reopening reuses the associated workspace; the source remains reachable for updating the task. Credentials, historical associations, live sessions and terminal-first operation must remain intact.

Source evidence: Orca task-page-default-repo-selection.ts, use-task-page-repo-selection.ts, github/auth-diagnose.ts, gitlab/gitlab-auth-and-rate-limit.ts, use-task-page-workspace-actions.ts, use-task-page-composer-actions.ts and renderer/src/lib/new-workspace.ts. GitHub/GitLab default issue prompt is `Complete {{artifact_url}}`. Codex/Claude draft delivery uses bracketed paste without submission. A conditional workspace-board to Linear status sync exists; automatic completion of every provider issue after an Agent exits was not established.

Critical path: source/account discovery and scope model -> composer project choice and Agent draft delivery -> source lifecycle controls -> contract and browser tests -> native isolated candidate. Product code stays separate from current installed binaries. No existing host or shell may be terminated. Current account contents are not evidence fixtures, and verification must not create or update external issues.

This continuation does not claim parity before the above user-visible path passes. PR/MR review, inline review comments and other independent Orca capabilities must not be implied by an Issue-flow result.
