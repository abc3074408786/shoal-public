#!/usr/bin/env python3
"""Bounded local benchmark of native, restricted-exec and experimental JSONL RPC.

No credentials, model, network listener, installed app, or host settings are used.
The output directory must be new or empty. All workload data uses a private tmpdir.
"""
import argparse
import base64
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import queue
import resource
import shutil
import statistics
import subprocess
import tempfile
import threading
import time


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def stats(samples):
    ordered = sorted(samples)
    return {
        "samples": len(samples),
        "median_ms": statistics.median(samples),
        "p95_ms": ordered[math.ceil(.95 * len(ordered)) - 1],
        "min_ms": ordered[0],
        "max_ms": ordered[-1],
        "raw_ms": samples,
    }


class Client:
    def __init__(self, adapter, config, env):
        started = time.perf_counter_ns()
        self.child = subprocess.Popen(
            ["/usr/bin/python3", "-I", str(adapter), "--config", str(config)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            env=env, start_new_session=True)
        self.sequence = 0
        self.messages = queue.Queue()
        self.errors = bytearray()

        def read():
            try:
                while line := self.child.stdout.readline(2 * 1024 * 1024 + 1):
                    self.messages.put(json.loads(line))
                self.messages.put(RuntimeError("adapter output closed"))
            except Exception as error:
                self.messages.put(error)

        def stderr():
            while chunk := self.child.stderr.read(8192):
                self.errors.extend(chunk[:max(0, 65536 - len(self.errors))])

        threading.Thread(target=read, daemon=True).start()
        threading.Thread(target=stderr, daemon=True).start()
        self.rpc("initialize", {"clientName": "velella-benchmark"})
        self.startup_ms = (time.perf_counter_ns() - started) / 1e6

    def receive(self, deadline):
        value = self.messages.get(timeout=max(.001, deadline - time.monotonic()))
        if isinstance(value, Exception):
            raise value
        if "error" in value:
            raise RuntimeError(value["error"])
        return value

    def send(self, method, params):
        self.sequence += 1
        self.child.stdin.write(json.dumps(
            {"id": self.sequence, "method": method, "params": params}).encode() + b"\n")
        self.child.stdin.flush()
        return self.sequence

    def rpc(self, method, params):
        request_id = self.send(method, params)
        deadline = time.monotonic() + 15
        while True:
            value = self.receive(deadline)
            if value.get("id") == request_id:
                return value["result"]

    def command(self, argv, work):
        process_id = f"bench-{self.sequence + 1}"
        request_id = self.send("process/start", {
            "processId": process_id, "argv": argv, "cwd": work.as_uri(),
            "env": {}, "tty": False})
        deadline = time.monotonic() + 15
        response, exit_code, closed = False, None, False
        stdout = bytearray()
        stderr = bytearray()
        while not (response and closed):
            value = self.receive(deadline)
            if value.get("id") == request_id:
                response = True
            params = value.get("params", {})
            if params.get("processId") != process_id:
                continue
            if value.get("method") == "process/exited":
                exit_code = params["exitCode"]
            elif value.get("method") == "process/closed":
                closed = True
            elif value.get("method") == "process/output":
                output = stdout if params["stream"] == "stdout" else stderr
                output.extend(base64.b64decode(params["chunk"]))
        if exit_code != 0:
            raise RuntimeError(f"adapter exit {exit_code}: {stderr.decode(errors='replace')}")
        return bytes(stdout)

    def memory(self):
        fields = {}
        for line in Path(f"/proc/{self.child.pid}/status").read_text().splitlines():
            if line.startswith(("VmRSS:", "VmHWM:")):
                key, value, _unit = line.split()
                fields[key.rstrip(":") + "_KiB"] = int(value)
        return fields

    def close(self):
        self.child.stdin.close()
        try:
            self.child.wait(timeout=6)
        except subprocess.TimeoutExpired:
            self.child.terminate()
            try:
                self.child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.child.kill()
                self.child.wait(timeout=5)
        if self.child.returncode != 0:
            raise RuntimeError(f"adapter exit {self.child.returncode}: {self.errors!r}")
        for stream in (self.child.stdout, self.child.stderr):
            stream.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    repo = Path(__file__).resolve().parents[1]
    parser.add_argument("--runtime", type=Path, default=repo / "target/debug/velella-runtime")
    parser.add_argument("--adapter", type=Path, default=repo / "scripts/fixtures/restricted-exec-server.py",
                        help="trusted adapter source; permits a preserved baseline for A/B measurement")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--samples", type=int, default=30)
    parser.add_argument("--warmup", type=int, default=3)
    args = parser.parse_args()
    if not (5 <= args.samples <= 40 and 0 <= args.warmup <= 10
            and args.samples + args.warmup <= 60):
        parser.error("require 5..40 samples, 0..10 warmup, and total <= 60")
    output, runtime = args.output.resolve(), args.runtime.resolve(strict=True)
    if output.exists() and any(output.iterdir()):
        parser.error("output directory must be new or empty")
    output.mkdir(parents=True, exist_ok=True)
    adapter = args.adapter.resolve(strict=True)
    sources = [Path(__file__).resolve(), adapter, repo / "crates/velella/src/restricted.rs",
               repo / "crates/velella/src/restricted_fs.rs"]
    hashes = {str(path.relative_to(repo)): sha256(path) for path in sources}
    result = {
        "schema": 1, "time": datetime.now(timezone.utc).isoformat(),
        "status": "running", "no_login": True, "no_model": True,
        "host_credentials_read": False, "runtime": str(runtime),
        "adapter": str(adapter),
        "runtime_sha256": sha256(runtime), "source_hashes": hashes,
        "build_profile": "debug" if "target/debug/" in str(runtime) else "unspecified",
        "platform": platform.platform(), "machine": platform.machine(),
        "logical_cpus": os.cpu_count(), "python": platform.python_version(),
        "cpu_model": next((line.split(":", 1)[1].strip()
                           for line in Path("/proc/cpuinfo").read_text().splitlines()
                           if line.startswith("model name")), None),
        "loadavg_before": os.getloadavg(),
        "method": {
            "clock": "time.perf_counter_ns monotonic wall time",
            "samples": args.samples, "warmup_per_case_per_path": args.warmup,
            "order": "round-robin native/direct/adapter, rotate first path each round",
            "p95": "nearest-rank 95th percentile; descriptive only for this sample count",
            "native_direct_timer": "before Popen through communicate and successful child exit",
            "adapter_process_timer": "before JSONL process/start write through process/closed notification; includes protocol and full drain",
            "adapter_fs_timer": "before fs/readFile write through complete JSONL reply",
            "adapter_startup_excluded": True,
            "filesystem_cache": "warm page cache; same fixed 4096 byte file",
            "limitations": [
                "debug runtime, experimental Python adapter, single client and sequential workloads",
                "no Orca A/B, no real model latency, no UI, WebSocket bridge or app-server overhead",
                "native in-process read and adapter fs RPC have different execution boundaries",
                "not a throughput, concurrency, cold-start or maximum-memory benchmark",
            ],
        },
        "cases": {}, "adapter_instances": [], "cleanup": {},
    }
    root = Path(tempfile.mkdtemp(prefix="vl-restricted-benchmark-"))
    client = None
    try:
        work, home = root / "work", root / "home"
        work.mkdir()
        home.mkdir()
        file = work / "file.bin"
        data = b"v" * 4096
        file.write_bytes(data)
        env = {"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "HOME": str(home)}
        config = root / "config.json"
        config.write_text(json.dumps({"runtime": str(runtime), "work": str(work),
                                      "home": str(home), "read_only": []}))
        config.chmod(0o600)
        python = "/usr/bin/python3"
        workloads = {
            "empty_process": (["/bin/true"], b""),
            "python_short": ([python, "-I", "-c", "print(1)"], b"1\n"),
            "cpu_pbkdf2_400k": ([python, "-I", "-c",
                                "import hashlib; print(hashlib.pbkdf2_hmac('sha256',b'x',b'y',400000).hex())"],
                               hashlib.pbkdf2_hmac("sha256", b"x", b"y", 400000).hex().encode() + b"\n"),
            "read_4KiB_process": ([python, "-I", "-c",
                                  "import pathlib,sys; sys.stdout.buffer.write(pathlib.Path(sys.argv[1]).read_bytes())",
                                  str(file)], data),
        }
        totals = {name: {"user_ms": 0, "system_ms": 0}
                  for name in ("native", "direct")}
        adapter_cpu = {"user_ms": 0, "system_ms": 0}
        for name, (argv, expected) in workloads.items():
            usage_before_adapter = resource.getrusage(resource.RUSAGE_CHILDREN)
            client = Client(adapter, config, env)
            values = {path: [] for path in ("native", "direct", "adapter")}
            spec = json.dumps({
                "policy": {"mode": "restricted", "network": "denied",
                           "home": str(home), "read_only": [], "writable": []},
                "program": argv[0], "args": argv[1:], "cwd": str(work), "env": {},
            }, separators=(",", ":"))
            child_usage = {"user_ms": 0, "system_ms": 0}
            for index in range(-args.warmup, args.samples):
                paths = ["native", "direct", "adapter"]
                offset = (index + args.warmup) % len(paths)
                for path in paths[offset:] + paths[:offset]:
                    before = resource.getrusage(resource.RUSAGE_CHILDREN)
                    start = time.perf_counter_ns()
                    if path == "adapter":
                        actual = client.command(argv, work)
                    else:
                        command = argv if path == "native" else [str(runtime), "restricted-exec", spec]
                        completed = subprocess.run(
                            command, cwd=work, env=env, stdin=subprocess.DEVNULL,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            start_new_session=True, timeout=15)
                        if completed.returncode:
                            raise RuntimeError(f"{path} exit {completed.returncode}: {completed.stderr!r}")
                        actual = completed.stdout
                    elapsed = (time.perf_counter_ns() - start) / 1e6
                    after = resource.getrusage(resource.RUSAGE_CHILDREN)
                    if actual != expected:
                        raise AssertionError(f"{name}/{path}: output mismatch")
                    if path != "adapter":
                        for key, attr in (("user_ms", "ru_utime"), ("system_ms", "ru_stime")):
                            delta = (getattr(after, attr) - getattr(before, attr)) * 1000
                            child_usage[key] += delta
                            if index >= 0:
                                totals[path][key] += delta
                    if index >= 0:
                        values[path].append(elapsed)
            result["cases"][name] = {path: stats(samples) for path, samples in values.items()}
            result["adapter_instances"].append({
                "case": name, "startup_to_initialize_ms": client.startup_ms,
                "after_workload_memory": client.memory()})
            client.close()
            client = None
            usage_after_adapter = resource.getrusage(resource.RUSAGE_CHILDREN)
            for key, attr in (("user_ms", "ru_utime"), ("system_ms", "ru_stime")):
                adapter_cpu[key] += ((getattr(usage_after_adapter, attr) -
                                      getattr(usage_before_adapter, attr)) * 1000 - child_usage[key])
            print(name, {path: round(stats(samples)["median_ms"], 3)
                         for path, samples in values.items()}, flush=True)
        client = Client(adapter, config, env)
        values = {"native_inprocess_read": [], "adapter_fs_readFile": []}
        for index in range(-args.warmup, args.samples):
            paths = list(values)
            if index % 2:
                paths.reverse()
            for path in paths:
                start = time.perf_counter_ns()
                if path == "native_inprocess_read":
                    actual = file.read_bytes()
                else:
                    reply = client.rpc("fs/readFile", {"path": file.as_uri()})
                    actual = base64.b64decode(reply["dataBase64"])
                elapsed = (time.perf_counter_ns() - start) / 1e6
                if actual != data:
                    raise AssertionError("filesystem output mismatch")
                if index >= 0:
                    values[path].append(elapsed)
        result["cases"]["read_4KiB_api"] = {path: stats(samples)
                                            for path, samples in values.items()}
        result["adapter_instances"].append({
            "case": "read_4KiB_api", "startup_to_initialize_ms": client.startup_ms,
            "after_workload_memory": client.memory()})
        client.close()
        client = None
        result["cpu_accounting"] = {
            "native_and_direct": totals,
            "native_and_direct_definition": "waited direct child RUSAGE_CHILDREN deltas, measured samples only, all process workloads combined",
            "adapter_process_workloads_total": adapter_cpu,
            "adapter_definition": "waited adapter plus descendants, four process-workload adapter lifetimes including startup and warmup; subtract native/direct children; excludes fs workload",
            "note": "CPU totals use different warmup/startup boundaries; wall-time table is the comparison metric.",
        }
        result["status"] = "passed"
    except Exception as error:
        result["status"] = "failed"
        result["error"] = str(error)
        raise
    finally:
        if client is not None:
            client.close()
        shutil.rmtree(root)
        result["cleanup"]["temporary_directory_removed"] = not root.exists()
        result["cleanup"]["adapters_exited"] = client is None
        result["runtime_unchanged"] = sha256(runtime) == result["runtime_sha256"]
        result["sources_unchanged"] = all(sha256(path) == hashes[str(path.relative_to(repo))]
                                          for path in sources)
        result["loadavg_after"] = os.getloadavg()
        (output / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(output / "result.json")


if __name__ == "__main__":
    main()
