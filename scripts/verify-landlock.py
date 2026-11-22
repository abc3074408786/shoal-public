#!/usr/bin/env python3
"""Run the filesystem-only Landlock prototype against owned, fixed fixtures."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time


CASE_NAMES = [
    "work_create_read_write_rename_truncate_delete_allowed",
    "readonly_read_allowed",
    "readonly_write_denied",
    "readonly_truncate_denied",
    "readonly_readonly_open_truncate_denied",
    "readonly_delete_denied",
    "outside_read_denied",
    "outside_write_denied",
    "outside_create_denied",
    "outside_list_denied",
    "outside_truncate_denied",
    "outside_readonly_open_truncate_denied",
    "outside_hardlink_into_work_denied",
    "outside_rename_into_work_denied",
    "symlink_escape_read_denied",
    "symlink_escape_write_denied",
    "inherited_file_descriptor_closed",
    "environment_cleared",
    "child_and_grandchild_inherit_restrictions",
]


def execute(command, *, cwd, timeout, env, pass_fds=()):
    """A fresh process group makes timeout cleanup include all descendants."""
    process = subprocess.Popen(
        command, cwd=cwd, env=env, pass_fds=pass_fds,
        stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, text=True,
        start_new_session=True,
    )
    try:
        stdout, stderr = process.communicate(timeout=timeout)
        return subprocess.CompletedProcess(command, process.returncode, stdout, stderr)
    finally:
        # Also remove any descendants left behind after a normal parent exit.
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        if process.poll() is None:
            process.communicate(timeout=5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", default="artifacts/landlock-local")
    parser.add_argument("--timeout", type=float, default=30,
                        help="Maximum seconds for each compile/probe/test process group.")
    args = parser.parse_args()
    if not 0 < args.timeout < float("inf"):
        parser.error("--timeout must be finite and positive")
    output_argument = Path(args.output)
    if output_argument.is_symlink():
        parser.error("--output must not be a symlink")
    output = output_argument.resolve()
    if output.exists() and (not output.is_dir() or any(output.iterdir())):
        parser.error("--output must be new or empty; existing evidence is never reused")
    output.mkdir(parents=True, exist_ok=True)
    repo = Path(__file__).resolve().parents[1]
    source = repo / "scripts" / "landlock-probe.c"
    fixture = repo / "scripts" / "fixtures" / "landlock-checks.py"
    receipt = {
        "schema": 1,
        "time": datetime.now(timezone.utc).isoformat(),
        "status": "running",
        "scope": "filesystem prototype",
        "network_isolation": False,
        "pid_isolation": False,
        "task_runtime_integrated": False,
        "no_model": True,
        "no_login": True,
        "checks": [],
        "source_hashes": {},
    }
    start = time.monotonic()

    def finish(status, reason, code):
        receipt.update(status=status, reason=reason,
                       elapsed_seconds=round(time.monotonic() - start, 3))
        data = json.dumps(receipt, indent=2, sort_keys=True) + "\n"
        pending = output / "result.json.tmp"
        pending.write_text(data)
        pending.replace(output / "result.json")
        print(data, end="")
        return code

    if sys.platform != "linux":
        return finish("blocked", "This filesystem prototype requires Linux.", 2)
    import fcntl

    if not os.access("/usr/bin/python3", os.X_OK):
        return finish("blocked", "The fixed fixture requires /usr/bin/python3.", 2)
    temporary = None
    inherited_fd = None
    previous_handler = signal.getsignal(signal.SIGTERM)

    def interrupted(_signal, _frame):
        raise InterruptedError("acceptance interrupted")

    signal.signal(signal.SIGTERM, interrupted)
    status, reason, code = "failed", "Acceptance did not finish.", 1
    try:
        for path in (source, fixture, Path(__file__).resolve()):
            receipt["source_hashes"][str(path.relative_to(repo))] = hashlib.sha256(path.read_bytes()).hexdigest()
        temporary = tempfile.TemporaryDirectory(prefix="vl-landlock-")
        root = Path(temporary.name).resolve()
        work, readonly, outside, tools = (root / name for name in ("work", "readonly", "outside", "tools"))
        for path in (work, readonly, outside, tools):
            path.mkdir()
        (readonly / "content.txt").write_text("readonly fixture\n")
        (outside / "secret.txt").write_text("synthetic outside fixture\n")
        (work / "escape").symlink_to(outside, target_is_directory=True)
        shutil.copyfile(fixture, tools / "landlock-checks.py")
        helper = tools / "landlock-probe"
        environment = {
            "PATH": "/usr/bin:/bin", "LANG": "C.UTF-8",
            "HOME": str(root), "TMPDIR": str(root),
        }
        build = execute(
            ["cc", "-std=c11", "-Wall", "-Wextra", "-Werror", "-O2", str(source), "-o", str(helper)],
            cwd=root, timeout=args.timeout, env=environment,
        )
        (output / "build.log").write_text(build.stdout + build.stderr)
        receipt["checks"].append({"name": "compile_helper", "passed": build.returncode == 0, "exit": build.returncode})
        if build.returncode:
            raise RuntimeError("C helper compilation failed; see build.log.")
        probe = execute([str(helper), "--abi"], cwd=root, timeout=args.timeout, env=environment)
        if probe.returncode:
            status, reason, code = "blocked", "The kernel did not expose usable Landlock support.", 2
        else:
            abi = json.loads(probe.stdout).get("abi")
            if type(abi) is not int or abi < 0:
                raise ValueError("Invalid ABI probe result.")
            receipt["landlock_abi"] = abi
            if abi < 3:
                status, reason, code = "blocked", "Landlock ABI 3 or later is required to restrict truncation.", 2
            else:
                readonly_paths = [Path("/usr").resolve(), readonly, tools]
                for path in (Path("/lib"), Path("/lib64")):
                    if path.exists():
                        canonical = path.resolve()
                        if not any(canonical == existing or existing in canonical.parents for existing in readonly_paths):
                            readonly_paths.append(canonical)
                command = [str(helper), "--rw", str(work)]
                for path in readonly_paths:
                    command.extend(["--ro", str(path)])
                with (outside / "secret.txt").open("rb") as stream:
                    inherited_fd = fcntl.fcntl(stream.fileno(), fcntl.F_DUPFD_CLOEXEC, 100)
                command.extend([
                    "--", "/usr/bin/python3", "-I", str(tools / "landlock-checks.py"),
                    "--work", str(work), "--readonly", str(readonly),
                    "--outside", str(outside), "--inherited-fd", str(inherited_fd),
                ])
                test_environment = dict(environment, LANDLOCK_SYNTHETIC_SECRET="not-a-real-credential")
                outcome = execute(command, cwd=work, timeout=args.timeout, env=test_environment,
                                  pass_fds=(inherited_fd,))
                # This log is fixed fixture JSON plus helper errors, never a model
                # transcript, inherited environment dump, or real file contents.
                (output / "checks.log").write_text(outcome.stdout + outcome.stderr)
                payload = json.loads(outcome.stdout)
                checks = payload.get("checks")
                if not isinstance(checks, list) or payload.get("generation") != 0:
                    raise ValueError("Fixture returned malformed evidence.")
                if [item.get("name") for item in checks] != CASE_NAMES:
                    raise ValueError("Fixture did not return the complete expected cases.")
                if any(type(item.get("passed")) is not bool for item in checks):
                    raise ValueError("Fixture returned invalid case results.")
                receipt["checks"].extend(checks)
                unchanged = (
                    sorted(path.name for path in readonly.iterdir()) == ["content.txt"]
                    and sorted(path.name for path in outside.iterdir()) == ["secret.txt"]
                    and not (readonly / "content.txt").is_symlink()
                    and not (outside / "secret.txt").is_symlink()
                    and (readonly / "content.txt").stat().st_nlink == 1
                    and (outside / "secret.txt").stat().st_nlink == 1
                    and (readonly / "content.txt").read_text() == "readonly fixture\n"
                    and (outside / "secret.txt").read_text() == "synthetic outside fixture\n"
                )
                receipt["checks"].append({"name": "host_confirms_readonly_and_outside_unchanged", "passed": unchanged})
                descendant_writes = all(
                    (work / f"descendant-{generation}.txt").read_text() == "allowed descendant\n"
                    for generation in (1, 2)
                )
                receipt["checks"].append({"name": "host_confirms_child_and_grandchild_work_writes", "passed": descendant_writes})
                if outcome.returncode or not all(item["passed"] for item in receipt["checks"]):
                    raise RuntimeError("One or more real filesystem confinement checks failed.")
                status, reason, code = "passed", "Fixed filesystem checks passed locally; network, PID and task runtime isolation were not tested.", 0
    except subprocess.TimeoutExpired:
        status, reason, code = "failed", "A test process group exceeded its deadline and was terminated.", 1
    except (KeyboardInterrupt, InterruptedError):
        status, reason, code = "failed", "Acceptance was interrupted; the active process group was terminated.", 1
    except Exception as error:
        status, reason, code = "failed", f"Acceptance failed ({type(error).__name__}); inspect fixed test logs.", 1
    finally:
        if inherited_fd is not None:
            os.close(inherited_fd)
        if temporary is not None:
            try:
                temporary.cleanup()
                receipt["temporary_files_removed"] = True
            except OSError:
                receipt["temporary_files_removed"] = False
                status, reason, code = "failed", "Owned temporary fixture cleanup failed.", 1
        signal.signal(signal.SIGTERM, previous_handler)
    return finish(status, reason, code)


if __name__ == "__main__":
    raise SystemExit(main())
