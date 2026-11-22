# Scoped CLI queries

Run from the selected checkout; paths below are repository-relative examples.
Use the child environment specified in `SKILL.md`. If `repowise` is unavailable,
read `runtime.md` for its invocation prefix. Check installed `--help` if the
version differs from 0.60.1.

## Without an index

For an explicit commit or comparison range:

```text
repowise risk main...HEAD --baseline 0 --format json
```

Choose a revision that actually exists in this checkout. `--baseline 0` avoids
walking 200 commits; it omits the repository-relative percentile. Add a baseline
only when that comparison matters. Without a revision, `risk` analyzes uncommitted
work, falling back to HEAD when clean; use an explicit revision when that fallback
would be misleading.

For the code shape of a single current file:

```text
repowise health --score src/example.py --format json --no-workspace
```

`--score` can run without an index. Without stored history, coverage, and graph
evidence, report it as a code-shape assessment, not full repository health. Do not
use whole-repository `health --recompute` for a one-file question.

## Using an existing index

Check freshness with the status recipe in `index.md` before relying on stored
relationships. These queries do not need a persistent server:

```text
repowise context src/example.py src/consumer.py --include callers --format json --no-workspace
repowise search ExampleService --mode symbol --limit 5 --format json --no-workspace
repowise search "authentication" --mode fulltext --limit 5 --format json --no-workspace
repowise health --file src/example.py --format json --no-workspace
repowise risk --target src/example.py --path . --format json
repowise impacted-tests main...HEAD --format json
```

`risk --target` reads indexed file history; ordinary `risk REVSPEC` is the Git-only
change analysis above. `impacted-tests` defaults to staged changes when no revision
is supplied outside CI, so provide an explicit range for a branch review. Tests
inferred from imports or naming are not measured coverage. No known reaching test
means the appropriate suite still needs selecting and running.

Use `--include` only for needed context blocks. Do not request `--full` by default;
the JSON projection retains errors, ambiguity, and continuation signals. Follow a
reported continuation when relevant instead of interpreting a cut result as
complete. For a known source span, use the host's native file reader.

For cleanup questions only:

```text
repowise dead-code --safe-only --min-confidence 0.8 --format json --no-workspace
```

Dead-code candidates still require live-code and framework checks before deletion.
Do not turn a score, a suggested test list, or an empty result into an acceptance
verdict. Query failure and an empty successful result are different outcomes.
