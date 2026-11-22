#!/usr/bin/env python3
"""Accept the actual Linux restricted-exec launcher using owned fixed fixtures.

This does not start the task daemon, log in, or call a model. It has no native
fallback. Filesystem metadata visibility and lack of a PID namespace are stated
limits, not security properties inferred from these checks.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import stat
import subprocess
import sys
import tempfile
import time
import runpy


PROBE_RECEIPT = "velella-restricted-probe-v3:filesystem-and-network-denied"


def execute(command, *, cwd, timeout, env, pass_fds=(), stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE):
    process = subprocess.Popen(
        command, cwd=cwd, env=env, pass_fds=pass_fds,
        stdin=stdin, stdout=stdout, stderr=stderr,
        text=True, start_new_session=True,
    )
    try:
        stdout, stderr = process.communicate(timeout=timeout)
        return subprocess.CompletedProcess(command, process.returncode, stdout or "", stderr or "")
    finally:
        # The unrestricted parent owns the complete fixed-fixture process group.
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        if process.poll() is None:
            process.communicate(timeout=5)


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def snapshot(path):
    metadata = path.lstat()
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
        raise ValueError("Fixed host fixture must be a regular single-link file.")
    return {
        "sha256": sha256(path), "mode": metadata.st_mode,
        "uid": metadata.st_uid, "gid": metadata.st_gid,
        "size": metadata.st_size, "inode": metadata.st_ino,
        "nlink": metadata.st_nlink, "mtime_ns": metadata.st_mtime_ns,
        "ctime_ns": metadata.st_ctime_ns,
        "xattrs": {name: os.getxattr(path, name).hex() for name in os.listxattr(path)},
    }


def main():
    repo = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", default=str(repo / "target/debug/velella-runtime"))
    parser.add_argument("--output", default="artifacts/restricted-local")
    parser.add_argument("--timeout", type=float, default=30)
    args = parser.parse_args()
    if not 0 < args.timeout < float("inf"):
        parser.error("--timeout must be finite and positive")
    output_argument = Path(args.output)
    if output_argument.is_symlink():
        parser.error("--output must not be a symlink")
    output = output_argument.resolve()
    if output.exists() and (not output.is_dir() or any(output.iterdir())):
        parser.error("--output must be new or empty; old evidence is never reused")
    output.mkdir(parents=True, exist_ok=True)
    runtime = Path(args.runtime).resolve()
    fixture = repo / "scripts/fixtures/restricted-checks.py"
    receipt = {
        "schema": 1, "time": datetime.now(timezone.utc).isoformat(),
        "status": "running", "scope": "actual runtime restricted-exec fixed Linux checks",
        "backend": "landlock-seccomp", "mode": "restricted", "network": "denied",
        "process_namespace": False, "filesystem_metadata_hidden": False,
        "task_pipeline_tested": False, "no_model": True, "no_login": True,
        "native_fallback": False, "checks": [], "source_hashes": {},
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
        return finish("blocked", "The restricted backend requires Linux.", 2)
    if not runtime.is_file() or not os.access(runtime, os.X_OK):
        return finish("blocked", "Build the actual velella-runtime executable first.", 2)
    if not os.access("/usr/bin/python3", os.X_OK):
        return finish("blocked", "The fixed fixture requires /usr/bin/python3.", 2)
    import fcntl

    temporary = None
    descriptors = []
    previous_handler = signal.getsignal(signal.SIGTERM)

    def interrupted(_signal, _frame):
        raise InterruptedError("acceptance interrupted")

    signal.signal(signal.SIGTERM, interrupted)
    status, reason, code = "failed", "Acceptance did not finish.", 1
    try:
        source_paths = [
            Path(__file__).resolve(), fixture,
            repo / "crates/velella/src/restricted.rs",
            repo / "crates/velella/src/restricted_fs.rs",
            repo / "crates/velella/src/sandbox.rs",
            repo / "crates/velella/src/lib.rs",
            repo / "crates/velella-core/src/execution.rs",
        ]
        for path in source_paths:
            receipt["source_hashes"][str(path.relative_to(repo))] = sha256(path)
        receipt["runtime_sha256"] = sha256(runtime)
        # Import only the checked-in fixed fixture to share its expected case list.
        fixed = runpy.run_path(str(fixture))
        temporary = tempfile.TemporaryDirectory(prefix="vl-restricted-check-")
        root = Path(temporary.name).resolve()
        work, home, readonly, outside, tools = (
            root / name for name in ("work", "home", "readonly", "outside", "tools")
        )
        for path in (work, home, readonly, outside, tools):
            path.mkdir()
        ro_file, secret = readonly / "content.txt", outside / "secret.txt"
        ro_file.write_text("readonly fixture\n")
        secret.write_text("synthetic outside fixture\n")
        (work / "escape").symlink_to(outside, target_is_directory=True)
        shutil.copyfile(fixture, tools / fixture.name)
        baseline = {str(path): snapshot(path) for path in (ro_file, secret)}
        environment = {
            "PATH": "/usr/bin:/bin", "LANG": "C.UTF-8",
            "HOME": str(root), "TMPDIR": str(root),
        }
        probe = execute(
            [str(runtime), "restricted-probe"], cwd=root,
            timeout=args.timeout, env=environment,
        )
        (output / "probe.log").write_text(probe.stdout + probe.stderr)
        available = probe.returncode == 0 and probe.stdout.strip() == PROBE_RECEIPT
        receipt["checks"].append({"name": "actual_runtime_capability_probe",
                                  "passed": available, "exit": probe.returncode})
        if not available:
            status, reason, code = "blocked", "Actual runtime restricted capability unavailable; see probe.log. No native fallback was attempted.", 2
        else:
            with secret.open("rb") as stream:
                descriptors.append(fcntl.fcntl(stream.fileno(), fcntl.F_DUPFD_CLOEXEC, 100))
            first, second = socket.socketpair()
            try:
                descriptors.append(fcntl.fcntl(first.fileno(), fcntl.F_DUPFD_CLOEXEC, 200))
            finally:
                first.close()
                second.close()
            synthetic = dict(environment, OPENAI_API_KEY="synthetic-not-a-real-key",
                             ANTHROPIC_API_KEY="synthetic-not-a-real-key",
                             CODEX_HOME=str(outside), VELELLA_SESSION_ENDPOINT="synthetic-endpoint",
                             RESTRICTED_SYNTHETIC_SECRET="synthetic-only")
            fixture_args = [
                "-I", str(tools / fixture.name), "--work", str(work),
                "--home", str(home), "--readonly", str(readonly), "--outside", str(outside),
            ]
            for descriptor in descriptors:
                fixture_args.extend(["--inherited-fd", str(descriptor)])
            launch_spec = {
                "policy": {"mode": "restricted", "network": "denied",
                           "home": str(home), "read_only": [str(readonly), str(tools)],
                           "writable": []},
                "program": "/usr/bin/python3", "args": fixture_args,
                "cwd": str(work), "env": synthetic,
            }
            # A rejected stdio descriptor must prevent exec, not merely fail
            # after the candidate command starts. Test every standard descriptor.
            for kind in ("outside_file", "socket"):
                for standard in ("stdin", "stdout", "stderr"):
                    marker = work / f"unexpected-stdio-exec-{kind}-{standard}"
                    rejection_spec = dict(launch_spec, args=[
                        "-I", "-c",
                        "from pathlib import Path; import sys; Path(sys.argv[1]).write_text('unexpected exec')",
                        str(marker),
                    ])
                    forwarded = ""
                    if kind == "outside_file":
                        # Launcher error reporting may write to stderr itself.
                        # Keep this owned log separate from the protected fixtures.
                        stdio_path = root / f"unsafe-{standard}.log"
                        with stdio_path.open("w+b") as stream:
                            rejected = execute(
                                [str(runtime), "restricted-exec", json.dumps(rejection_spec)],
                                cwd=work, timeout=args.timeout, env=synthetic, **{standard: stream},
                            )
                        forwarded = stdio_path.read_text()
                    else:
                        first, second = socket.socketpair()
                        try:
                            rejected = execute(
                                [str(runtime), "restricted-exec", json.dumps(rejection_spec)],
                                cwd=work, timeout=args.timeout, env=synthetic,
                                **{standard: first.fileno()},
                            )
                            first.close()
                            second.settimeout(1)
                            chunks = []
                            while True:
                                data = second.recv(65536)
                                if not data:
                                    break
                                chunks.append(data)
                            forwarded = b"".join(chunks).decode("utf-8")
                        finally:
                            first.close()
                            second.close()
                    diagnostics = rejected.stdout + rejected.stderr + forwarded
                    (output / f"reject-{standard}-{kind}.log").write_text(diagnostics)
                    receipt["checks"].append({
                        "name": f"inherited_{kind}_{standard}_rejected_before_exec",
                        "passed": (rejected.returncode != 0 and not marker.exists()
                                   and "restricted stdin/stdout/stderr must be" in diagnostics),
                        "exit": rejected.returncode,
                    })
            outcome = execute(
                [str(runtime), "restricted-exec", json.dumps(launch_spec)],
                cwd=work, timeout=args.timeout, env=synthetic, pass_fds=tuple(descriptors),
            )
            # Only fixed fixture JSON/errors, never real credentials or model output.
            (output / "checks.log").write_text(outcome.stdout + outcome.stderr)
            receipt["fixture_exit"] = outcome.returncode
            # Independently inspect host fixtures even if the child returned bad JSON.
            unchanged = (
                sorted(path.name for path in readonly.iterdir()) == ["content.txt"]
                and sorted(path.name for path in outside.iterdir()) == ["secret.txt"]
                and all(snapshot(Path(path)) == old for path, old in baseline.items())
            )
            receipt["checks"].append({"name": "host_confirms_readonly_outside_contents_and_metadata_unchanged",
                                      "passed": unchanged})
            payload = json.loads(outcome.stdout)
            checks = fixed["validate_evidence"](payload)
            receipt["checks"].extend(checks)
            descendant_writes = all(
                (work / f"descendant-{generation}.txt").read_text() == "allowed descendant\n"
                for generation in (1, 2)
            )
            receipt["checks"].append({"name": "host_confirms_child_and_grandchild_work_writes",
                                      "passed": descendant_writes})
            home_writes = all(
                path.read_text() == "owned restricted fixture\n"
                for path in (home / "home-marker.txt", home / ".tmp" / "tmp-marker.txt")
            )
            receipt["checks"].append({"name": "host_confirms_dedicated_home_and_tmp_writes",
                                      "passed": home_writes})
            if outcome.returncode or not all(item["passed"] for item in receipt["checks"]):
                raise RuntimeError("Real runtime restrictions did not pass every fixed check.")
            status, reason, code = "passed", "Actual runtime file access, metadata writes, external network/UNIX denial, internal anonymous stream/seqpacket pairs, descriptor cleanup and inherited restrictions passed; PID namespace and metadata hiding are not provided.", 0
    except subprocess.TimeoutExpired:
        status, reason, code = "failed", "A process group exceeded its deadline and was terminated.", 1
    except (KeyboardInterrupt, InterruptedError):
        status, reason, code = "failed", "Acceptance interrupted; the active process group was terminated.", 1
    except Exception as error:
        status, reason, code = "failed", f"Acceptance failed ({type(error).__name__}); inspect fixed fixture logs.", 1
    finally:
        for descriptor in descriptors:
            try:
                os.close(descriptor)
            except OSError:
                status, reason, code = "failed", "Owned descriptor cleanup failed.", 1
        if temporary is not None:
            try:
                temporary.cleanup()
                receipt["temporary_files_removed"] = not Path(temporary.name).exists()
                if not receipt["temporary_files_removed"]:
                    raise OSError("owned temporary directory remains")
                receipt["checks"].append({"name": "owned_temporary_files_removed", "passed": True})
            except OSError:
                receipt["temporary_files_removed"] = False
                receipt["checks"].append({"name": "owned_temporary_files_removed", "passed": False})
                status, reason, code = "failed", "Owned temporary fixture cleanup failed.", 1
        signal.signal(signal.SIGTERM, previous_handler)
        # Preserve the initial provenance rather than replacing it with sources
        # that may have changed while the checked binary/fixture was executing.
        try:
            final_hashes = {
                str(path.relative_to(repo)): sha256(path) for path in source_paths
            }
            unchanged = (final_hashes == receipt["source_hashes"]
                         and sha256(runtime) == receipt["runtime_sha256"])
            receipt["sources_and_binary_unchanged_during_test"] = unchanged
            if not unchanged:
                receipt["final_source_hashes"] = final_hashes
                status, reason, code = "failed", "Sources or runtime changed during acceptance.", 1
        except (OSError, KeyError):
            receipt["sources_and_binary_unchanged_during_test"] = False
            status, reason, code = "failed", "Could not verify final test provenance.", 1
    return finish(status, reason, code)


if __name__ == "__main__":
    raise SystemExit(main())
