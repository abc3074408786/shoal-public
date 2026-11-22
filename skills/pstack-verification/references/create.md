# Create a project verification path

## Establish what can actually run

From the selected checkout, identify the user-facing surface and its real entry
points: CLI commands, routes, UI controls, API endpoints, or library interfaces.
Read the existing build/start/test configuration and inspect any adopted harness
before selecting tools. Record the actual readiness signal, seed/auth needs,
observable results, isolation mechanism, and teardown command.

Prefer the project's runner. Browser/CDP, PTY, plain subprocesses, or HTTP are
choices based on the surface, not mandatory dependencies. A CLI with no TTY
behavior can use ordinary subprocesses. Verify interactive behavior through a
real terminal when that behavior is under test.

If the baseline cannot start, report the concrete failure. Fix it only within
the authorized scope; do not write fictional launch instructions or silently
substitute a mock for the target. Verification-only scaffolding must be identified.

## Write the smallest reusable result

Extend an existing verification script/skill when suitable. Otherwise put a
project-local skill at the repository's adopted skill location, or
`P/.agents/skills/verify-<app>/SKILL.md` when none is established. Use a unique
name and preserve any existing files. Include valid name/description frontmatter.

Write exact, observed commands, not placeholders:

- **Launch:** build/start from an isolated state, initialize data, wait for a
  usable app, and state what process the run owns. A service answering its port
  is insufficient when migrations or seed data are still running.
- **Doctor:** a read-only check of the correct build, instance, readiness, and
  required auth. Run before driving and again after unexpected failure. Reset
  or relaunch a wedged state even if the process remains healthy.
- **Drive:** exercise the public path with stable handles. Use app-specific
  commands/selectors already found in the checkout.
- **Evidence:** capture the action, outcome, and relevant side effects. Keep
  logs, responses, screenshots, or transcripts outside disposable state. Prove
  any claimed dry-run boundary by observing it.
- **Cleanup:** stop only processes the run started; remove owned scratch state
  and retain proof. Do not kill by process name or reset a user's live session.

Ship helper scripts only when reusable mechanics warrant them. Document their
invocation and dependencies, and make them executable on the target platform.
Avoid a shell-specific recipe when a small existing cross-platform runner suffices.

## Map the behavior

Create a small feature index beside the verification procedure, with a file per
important user-facing feature. Start with representative critical features,
normally three to five; do not invent features to meet a count.

Each entry names the user-visible behavior, route to it, preconditions, exact
drive recipe, expected observable end state, and known limits. Add references to
the current spec/acceptance criteria rather than copying a second specification.
Record missing seed data or entitlement as a gap, not an easier passing journey.

## Execute the generated procedure

Run launch, doctor, one mapped feature, evidence capture, and cleanup on the
actual authorized target. Then check both that the evidence survived and that
owned processes/scratch state are gone. A failure gets a diagnosis before any
retry; allow one retry only after changed prerequisites or new evidence.

Keep generated instructions as a draft if they were not executed. A local fixture
or stand-in establishes only that environment; identify target-only setup still
unverified. Report the command, environment, feature exercised, result, evidence,
cleanup, and mapped features not yet run. Maintenance uses `maintain.md`.
