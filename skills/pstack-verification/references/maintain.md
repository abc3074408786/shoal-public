# Maintain a project verification path

Locate the selected project's adopted verification procedure and feature map.
Several plausible targets require selecting the intended one; no target routes
to creation. Do not invent a replacement for a working harness.

The edit scope is the verification procedure, its feature map, and its owned
helpers. A product regression is reported separately. Do not change product code
or weaken the documented expected behavior to make this maintenance pass green.

## Source coverage

Read the index and compare it with the actual feature files. Resolve missing,
duplicate, dead, or unindexed entries. Check recent source changes for user paths
the map omits; a candidate needs a concrete source entry point.

Inspect every mapped feature from source. When repository rules permit and the
work is wide enough, assign independent read-only feature checks within the
configured concurrency limit. Otherwise do them sequentially. A source reader
returns the behavior, file/line evidence, possible drift, and one live recipe;
it never drives the shared instance or edits the project. Follow the host's
waiting/ownership rules. The owner spot-checks the cited changes and combines
recipes without rereading all clean source.

## Live coverage

Exercise every mapped feature, even when the source looks unchanged. Use one
health-checked, serially driven instance for a long-lived app when its harness
requires it; use an isolated session per drive for a short-lived CLI.

Run doctor before first use, for each fresh session where appropriate, and after
unexpected failure. Clean failed-iteration residue before continuing. Reset or
relaunch invalidated state; never guess that a healthy process means a usable UI.
Keep evidence through every cleanup and verify that it remains available.

Classify each discrepancy:

- **Documentation drift:** the current spec and working app agree; update stale
  instructions or map entries, citing both.
- **Harness gap:** the intended behavior works but the harness cannot reach or
  observe it. Correct the owned helper and re-drive that feature.
- **Product regression:** the app violates the current expected behavior.
  Report it without rewriting the expectation or repairing product code here.
- **Unreachable prerequisite:** name the route attempted and the missing auth,
  entitlement, platform, or external state. It is not a passed feature.

After a failed attempt, diagnose and change the relevant prerequisite before at
most one retry. Stop with the exact remaining gap rather than retrying unchanged.
Teardown happens after the final drive and any re-proof; preserve all evidence.

## Result

Report per-feature source/live coverage and one outcome:

- **clean:** all features were checked from source and live; no correction needed.
- **changed:** verified harness/map corrections are ready or applied within scope.
- **blocked:** coverage or safe completion is incomplete, with the exact reason.

Report product regressions explicitly alongside the outcome; they prevent a clean
verdict. Opening a PR is optional and follows the task's requested delivery form.
Do not publish, commit, or schedule maintenance just because upstream did so.
