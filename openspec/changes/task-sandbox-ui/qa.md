# Sandbox controls QA — 2026-10-20

Implemented in the existing task dialog and inspector with existing components,
CSS tokens and Chinese copy. No new component library or animation added.

## Verified

- Frontend suite: 77 passed, including five sandbox policy tests.
- Production frontend build: passed; existing large-bundle warning remains.
- Rust `cargo test -p velella --lib sandbox:: -- --nocapture`: 10 passed.
  The live test exercised explicit refusal on this host, not positive confinement.
- Rust `cargo check -p velella --all-targets`: passed with existing warnings.
- Real browser, development fixture, 1280x720 default viewport: native defaults;
  isolated selection; explicit host networking; read-only and writable paths;
  submitted JSON preserves all selected fields and omits HOME/credentials.
- Browser flaky-probe scenario: after selecting isolation and acknowledging,
  recheck failure retains isolated selection and disables Create and Run.
- Browser unavailable scenario: disabled isolated option, expandable raw error,
  readable fieldset and scrollable dialog. Screenshot inspected in session.
- Browser Windows capability fixture: unsupported message; explicitly selected
  native submission still works. This is UI simulation, not a Windows run.
- Browser direct-folder fixture: isolated option disabled with Git requirement.
  Dialog client/scroll widths both 578px; no horizontal overflow. Tab from retry
  reaches host CLI/login check. Console error/warning capture was empty.

## Evidence limits

Strict `python3 scripts/verify-sandbox.py` receipt is blocked (exit 2):
`bwrap: Can't mount proc on /newroot/proc: Operation not permitted`.
Logs and receipt are in ignored artifacts/sandbox-acceptance/. No host security
settings were changed. Positive kernel tests, authenticated Agent end-to-end,
Windows/macOS native backends, and remote workflow execution remain unverified.
Browser screenshots were inspected inline, not saved as a persistent artifact.
Narrow viewport, screen-reader and full keyboard traversal were not tested.

Design-pipeline route/toolchain/foundation records are present. Work continued
in the existing uncommitted checkout to retain the preceding sandbox work and
other user changes; full isolated execution-route/gate certification was not
performed. This report does not claim all design-pipeline gates passed.

## User-requested retest

- Re-ran all frontend tests: 77 passed, zero failures.
- Expanded runtime regression to the entire `velella --lib` suite: 91 passed,
  3 ignored, zero failures.
- Strict acceptance rerun: exit 2, still blocked by /proc mount permission.
  Fresh receipt: artifacts/sandbox-retest/result.json.
- Browser fixture: relative path rejected before any task.create request; duplicate
  read-only directory deduplicated; default network serialized as denied.
- Browser fixture recheck failure: isolated mode retained and submit disabled even
  with acknowledgement checked. Screenshot inspected; no console errors/warnings.
- These browser checks mock RPC and do not constitute a real sandboxed Agent run.

## Hosted kernel acceptance (supersedes earlier kernel-evidence limitation)

Run https://github.com/abc3074408786/shoal/actions/runs/39371410672 passed on
GitHub-hosted Ubuntu 22.04. Commit 1e29a4fa6d0f9fcd46b6dae0eb529241824db51b,
branch codex/sandbox-kernel-probe-20261020; main unchanged. The minimal test crate
contains byte-identical copies of sandbox.rs and core execution.rs at that run;
downloaded source hashes were compared successfully. Six tests passed with
VELELLA_REQUIRE_SANDBOX=1, so refusal could not count as successful confinement.
Read-only enforcement, hidden host paths and symlink targets, writable task/cache,
PID namespace, denied network, explicit host network, HOME persistence and /tmp
reset were exercised. No authenticated Agent/full desktop task was tested remotely.
Evidence: artifacts/sandbox-hosted-kernel/. Current local host remains blocked.

Added local SessionManager PTY regression with valid cwd and a real shell command:
unavailable isolation refuses launch, registers no session, and creates no marker.
The targeted test passed. No existing user tasks or installed application changed.

## Hosted task pipeline acceptance

Run https://github.com/abc3074408786/shoal/actions/runs/39372050379 passed on
Ubuntu 22.04, commit 8664a5d49bbc40de35d68585b077b8d283262dbf. Verified all 53
runtime-source/manifest hashes against the checkout at that run. Ten policy/kernel
tests passed plus the explicitly invoked ignored task-pipeline test (1.91s).
Real runtime, worktree preparation, setup, PTY launch, staged hook runtime,
event-only socket, verification, verified-review transition, stop and restored
session were exercised. JSON reload models task recovery; a complete daemon
restart was not performed. Deterministic Python fixture Agent only, no model
call, credentials upload, desktop task-create RPC or UI in this run.
Evidence: artifacts/sandbox-hosted-pipeline/verified-result.json and test logs.
The strict acceptance script now includes this test; its 5 receipt tests pass.

## Actual Codex CLI inside isolation, without login

Run https://github.com/abc3074408786/shoal/actions/runs/39406355190 passed on
Ubuntu 22.04, commit a719f3fb643a2c1be8338e4870ff31ff6a8ec6e0. The actual
Codex CLI 0.153.4 ran --version, exec --help and login status through the Velella
isolated launcher with network denied and a dedicated HOME. Startup/help passed;
login status reported not logged in, and no auth.json was created. Source hashes
were verified. Evidence: artifacts/sandbox-real-cli-startup/result.json.
This tests CLI startup compatibility; no login or model request occurred.

The prepared real-model acceptance harness now requires explicit manual login
authorization, stops the Agent before checking the complete fixture inventory,
and reruns trusted tests through isolated task.verify. Its 17 tests pass; workflow
syntax checks pass. The model/login workflow has not been enabled or run remotely.
Authenticated model execution inside the custom sandbox remains unverified,
consistent with the user's preparation-only instruction.

## Real daemon RPC stop and re-verification, without login

Run https://github.com/abc3074408786/shoal/actions/runs/39412000066 passed in 40s
on Ubuntu 22.04, commit b6c0bdd26aff7606391812f7fada17ef699c439c. All 57 copied
source/test file hashes match the local checkout. Twenty harness tests passed,
including an early-exit regression. The fixed protocol Agent then exercised
real project/task RPC, setup, PTY, sandboxed hook events, six functional tests,
review, task.cancel, isolated task.verify and verified review again. The receipt
confirms denied networking, unchanged fixed tests/original checkout, stopped
task and removed temporary HOME. Evidence:
artifacts/sandbox-rpc-fixture-hosted/verified-result.json.

The Agent is a fixed Python fixture, not a model. The real-model login workflow
exists only locally and as a nested source fixture on the temporary test branch;
it has not been enabled or run remotely. The no-login workflow alone was run.
The current host still refuses /proc mounting and correctly exits 2 before
starting this test's daemon or Agent. Windows/macOS backends remain untested.
