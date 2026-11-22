# Collaboration integration on the current desktop

User outcome: the current Electron desktop and runtime can use the existing
native-terminal collaboration implementation, with durable assignment, receipt,
fixed-candidate review and explicit acceptance. Ordinary terminals, source tasks,
sandbox modes and existing data keep their current behavior.

Baseline: main `e9939732179e11a91f54c70326c2c1e68c11a178`. Collaboration source:
`5654dbbd11ea576ee880459b07ba9977e21216e4`, changes after its `4275e8d` baseline.
Only collaboration source and its integration seams are migrated. The older
desktop, IPC implementation, benchmarks and packaging are not replacements for
the newer baseline. Work is local on `codex/integrate-collaboration-20261022`.

Acceptance for this integration:

- Both binaries expose collaboration; the installed runtime alone is sufficient.
- Launch preserves provider/model/approval defaults and all existing modes.
- Mutation replay, explicit membership, writer ownership, current-attempt reports
  and separate-session review retain their existing guards.
- Candidate identity is computed from actual source, including dirty/untracked
  files and deletions. Report, review and acceptance reject changed content.
- Host rejection cannot become an input-accepted receipt. Uncertain input is not
  resent. Mailbox waits do not hold the global desktop lock.
- Workspace Rust tests, frontend tests/build, Windows desktop smoke, and a local
  collaboration flow pass against the fixed integration candidate. Record which
  checks use fixture agents and which run real models; never substitute one for
  the other.

The critical path is integration -> regression/receipt verification -> native
model compatibility -> reviewed local candidate. At most two failed attempts per
root cause; retry only after changing a diagnosed condition. Installation,
external issue/PR writes, global Agent configuration and provider changes are
outside this authorization. Existing running sessions and credentials are not
test fixtures. The historical six-Agent/two-hour qualification remains separate
and must not be implied by these checks.

## Terminal entry

Start the desktop, get a workspace ID from `velella-runtime rpc <data-dir>
'{"cmd":"workbench.snapshot"}'`, then run `velella-runtime collab --help`.
Use `collab --data-dir <profile> --request-id <uuid> start --workspace <id>
--agent codex` (or `claude-code`) for a coordinator terminal. It opens without
submitting a prompt or changing native approval defaults. Within it, read the
file pointed to by `VELELLA_COLLABORATION_SKILL`, then create a run. Existing
native terminals can instead create/join explicitly using `--session` and
`--credentials`; they are not restarted or assumed ready for notifications.

The coordinator launches/invites members, explicitly confirms membership, claims
a writer and assigns work. After running the agreed checks, the writer obtains
`candidate --work <id>`, saves the JSON as UTF-8 outside the source directory, and
reports it. A separate member reviews that exact candidate; the coordinator then
accepts the work. Release writer ownership before ending. Ending retains sessions
and history. Source fingerprints bind the files reviewed, not proof that tests
were actually run: retain test receipts with the report and inspect them.

## Evidence

Build and test outputs are retained in
`artifacts/collaboration-integration-20261022/`. This document is a contract and
usage guide, not an assertion that pending native validation or UAT has passed.

Current checkpoint (2026-10-22): Rust workspace 167 passed, 3 ignored; frontend
96 passed; Windows Electron smoke 26 passed with no console errors. Release
binaries, frontend build, Clippy and skill validation passed. The collaboration
subset has 17 passing tests and one subprocess entry point intentionally ignored
by the normal test runner. Its end-to-end test uses real source files/native IPC
with fixture host responses, not real models.

Independent source review identified externally forgeable notification acceptance
and false success when replaying a failed launch. Both were repaired and covered
by root-run regression tests; the reviewer did not rerun the fixed candidate.

Codex 0.162.1 and Claude Code 2.1.238 launched in an isolated profile. Both require
native trust confirmation for the fixture project. The Codex update chooser was
dismissed without updating; a later read-only probe was not confirmed executed
and its receipt is uncertain/session-exited. It was not resent. Claude remained
at its trust prompt. The isolated service was stopped and owned processes were
checked absent. No model-completed collaboration, installation, long-run
qualification, or business UAT pass is claimed. Next action requires explicit
permission to save native trust for this fixture only:
`artifacts/collaboration-integration-20261022/native-58d4d118ac/project`.

The Windows installer is under
`artifacts/collaboration-integration-20261022/package-verified-dist/`. The first
packaging attempt hit an extraction-directory rename lock. The one retry used the
checksum-verified, already-extracted Electron 44.9.2 distribution and succeeded.
