# Runtime, loaded only when the CLI is needed

These recipes target RepoWise 0.60.1. Check `repowise --version` and the relevant
subcommand's `--help` when reusing a different installed version. Do not silently
upgrade a working installation or assume flags exist on older versions.

RepoWise needs Python 3.11 or later. Its dependency bundle is substantial even
though this Skill is small. An isolated first run may download hundreds of MB;
subsequent `uv tool run` calls reuse the cache. No Node.js dashboard is required.
Indexing also creates a local cache-signing key in the user's RepoWise config
directory; this is engine cache state, not an Agent or MCP configuration.

## Invocation

Use a compatible installed `repowise` directly. If missing and `uv` is available,
replace the `repowise` prefix in the recipes with:

```text
uv tool run --python 3.11 --from repowise==0.60.1 repowise
```

This runs an isolated cached environment and does not install a global executable.
Run it only for an analysis task, never for Skill discovery. It can download the
package, dependencies, and Python if absent. When offline or downloads are outside
the task's authorization, use an existing cached runtime or report the missing
runtime. Do not install `uv`, change PATH, or use the system Python environment as
an implicit fallback.

Set these variables in the child command environment, not persistent user/system
settings. They suppress optional telemetry, editor wiring, and key persistence:

```powershell
$env:DO_NOT_TRACK = '1'
$env:REPOWISE_SKIP_EDITOR_SETUP = '1'
$env:REPOWISE_NO_SAVE_KEY = '1'
```

POSIX equivalent:

```sh
export DO_NOT_TRACK=1 REPOWISE_SKIP_EDITOR_SETUP=1 REPOWISE_NO_SAVE_KEY=1
```

Do not print credentials or inspect unrelated account files. Free deterministic
commands need no provider key. The index recipes explicitly disable model prose;
avoid `ask`, `generate`, semantic embedding setup, and generated refactoring code
in this Skill's default workflow.

Upstream: [CLI reference](https://github.com/repowise-dev/repowise/blob/main/docs/reference/CLI_REFERENCE.md),
[package and license](https://github.com/repowise-dev/repowise/blob/main/pyproject.toml).
This Skill contains integration instructions, not a bundled copy of the engine.
