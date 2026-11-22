"""Isolated native-host continuity probe; no model, GUI or live policy changes."""
import argparse
import base64
import json
import os
from pathlib import Path
import re
import subprocess
import time
import uuid


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--runtime", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if os.name != "nt":
        raise RuntimeError("This probe requires Windows")
    runtime = args.runtime.resolve(strict=True)
    output = args.output.resolve()
    root = output / ("fixture-" + uuid.uuid4().hex)
    root.mkdir(parents=True)
    children, files = [], []
    result = {"ok": False, "runtime": str(runtime), "fixture": str(root),
              "livePowerChanges": False, "physicalScreenOrLidTest": False}
    try:
        for label, count, adapter in [("first", 15, "generic"), ("second", 60, "unlisted-power-fixture")]:
            directory = root / label
            directory.mkdir()
            command = (f"1..{count} | ForEach-Object {{ "
                       f"Write-Output ('{label}:' + $_ + '@' + [DateTime]::UtcNow.ToString('o')); "
                       "Start-Sleep -Milliseconds 100 }")
            encoded = base64.b64encode(command.encode("utf-16-le")).decode("ascii")
            descriptor = {"id": label, "endpoint": "velella-power-test-" + uuid.uuid4().hex,
                          "directory": str(directory), "spawn": {
                              "program": "powershell", "args": ["-NoProfile", "-EncodedCommand", encoded],
                              "env": {}, "adapter": adapter}}
            launch = directory / "launch.json"
            launch.write_text(json.dumps(descriptor), encoding="utf-8")
            stdout, stderr = [(directory / name).open("wb") for name in ("stdout.log", "stderr.log")]
            files.extend([stdout, stderr])
            child = subprocess.Popen([str(runtime), "host", str(launch)], stdin=subprocess.DEVNULL,
                                     stdout=stdout, stderr=stderr, creationflags=subprocess.CREATE_NO_WINDOW)
            children.append(child)

        def status(label):
            try:
                return json.loads((root / label / "status.json").read_text(encoding="utf-8"))
            except (OSError, ValueError):
                return {}

        def log(label):
            path = root / label / "terminal.bin"
            return path.read_bytes() if path.exists() else b""

        def until(predicate, seconds=20):
            end = time.monotonic() + seconds
            while time.monotonic() < end:
                if predicate():
                    return
                time.sleep(0.03)
            raise RuntimeError("Native fixture deadline exceeded")

        until(lambda: status("first").get("pid") and status("second").get("pid"))
        pids = {name: status(name)["pid"] for name in ("first", "second")}
        assert pids == {"first": children[0].pid, "second": children[1].pid}
        until(lambda: children[0].poll() is not None)
        assert children[0].returncode == 0, "First host failed"
        assert children[1].poll() is None, "Second CLI must outlive the first host"
        previous = len(log("second"))
        until(lambda: len(log("second")) > previous)
        result["secondContinuedAfterFirstHostExit"] = True
        until(lambda: children[1].poll() is not None)
        assert children[1].returncode == 0, "Second host failed"
        result["sessions"] = {}
        for label, count in [("first", 15), ("second", 60)]:
            document = status(label)
            # CSI sequences can end in an ASCII letter immediately before the
            # first marker; a regex word boundary would miss that valid line.
            matches = re.findall(label.encode() + rb":(\d+)@", log(label))
            assert list(map(int, matches)) == list(range(1, count + 1)), (label, matches)
            assert document["pid"] == pids[label], "Host identity changed"
            assert document["exit"] == 0 and document["state"] == "done", document
            result["sessions"][label] = {"pid": pids[label], "lines": count,
                                         "bytes": len(log(label)), "state": document["state"]}
        result["ok"] = True
    finally:
        for child in children:
            if child.poll() is None:
                child.kill()  # only this probe's isolated hosts; their jobs own their PTYs
                child.wait(timeout=10)
        for file in files:
            file.close()
        (output / "result.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
