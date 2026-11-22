#!/usr/bin/env python3
"""Fixed, credential-free filesystem operations for the Landlock prototype."""
import argparse
import errno
import json
import os
from pathlib import Path
import subprocess
import sys


DENIED = {errno.EACCES, errno.EPERM, errno.EXDEV}


def denied(operation):
    try:
        operation()
    except OSError as error:
        if error.errno in DENIED:
            return
        raise AssertionError(f"unexpected errno {error.errno}") from None
    raise AssertionError("operation unexpectedly succeeded")


def truncate_with_readonly_open(path):
    descriptor = os.open(path, os.O_RDONLY | os.O_TRUNC)
    os.close(descriptor)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--work", required=True)
    parser.add_argument("--readonly", required=True)
    parser.add_argument("--outside", required=True)
    parser.add_argument("--inherited-fd", type=int, required=True)
    parser.add_argument("--generation", type=int, default=0)
    args = parser.parse_args()
    work, readonly, outside = map(Path, (args.work, args.readonly, args.outside))
    ro_file = readonly / "content.txt"
    secret = outside / "secret.txt"
    checks = []

    def check(name, operation):
        try:
            details = operation()
        except Exception as error:
            # Report only exception class and errno, never file contents or env.
            result = {"name": name, "passed": False, "error": type(error).__name__}
            if isinstance(error, OSError):
                result["errno"] = error.errno
            checks.append(result)
        else:
            result = {"name": name, "passed": True}
            if isinstance(details, dict):
                result["details"] = details
            checks.append(result)

    def same(actual, expected):
        if actual != expected:
            raise AssertionError("value did not match fixed fixture")

    def allowed_crud():
        directory = work / "created-directory"
        directory.mkdir()
        path = directory / "created.txt"
        path.write_text("allowed fixture\n")
        same(path.read_text(), "allowed fixture\n")
        moved = directory / "renamed.txt"
        path.rename(moved)
        same(moved.read_text(), "allowed fixture\n")
        with moved.open("r+") as stream:
            stream.truncate(7)
        same(moved.read_text(), "allowed")
        moved.unlink()
        directory.rmdir()

    def closed_inherited_fd():
        try:
            os.fstat(args.inherited_fd)
        except OSError as error:
            if error.errno == errno.EBADF:
                return
            raise
        raise AssertionError("launcher kept an inherited file descriptor")

    def clean_environment():
        expected = {
            "PATH": "/usr/bin:/bin",
            "LANG": "C.UTF-8",
            "HOME": str(work),
            "TMPDIR": str(work),
        }
        same(dict(os.environ), expected)

    def descendants():
        result = subprocess.run(
            [
                sys.executable, "-I", str(Path(__file__).resolve()),
                "--work", str(work), "--readonly", str(readonly),
                "--outside", str(outside), "--inherited-fd", str(args.inherited_fd),
                "--generation", str(args.generation + 1),
            ],
            capture_output=True, text=True, timeout=10, check=False,
        )
        payload = json.loads(result.stdout)
        expected_names = [
            "descendant_outside_read_denied",
            "descendant_readonly_write_denied",
            "descendant_work_write_allowed",
        ]
        if args.generation == 0:
            expected_names.append("grandchild_inherits_restrictions")
        same(result.returncode, 0)
        same(payload.get("generation"), args.generation + 1)
        same([item.get("name") for item in payload["checks"]], expected_names)
        if not all(item.get("passed") is True for item in payload["checks"]):
            raise AssertionError("descendant checks did not all pass")
        # Preserve named evidence for each generation rather than just its exit.
        return payload

    if args.generation:
        check("descendant_outside_read_denied", lambda: denied(secret.read_text))
        check("descendant_readonly_write_denied", lambda: denied(lambda: ro_file.write_text("changed\n")))
        check("descendant_work_write_allowed", lambda: same(
            (work / f"descendant-{args.generation}.txt").write_text("allowed descendant\n"),
            len("allowed descendant\n"),
        ))
        if args.generation == 1:
            check("grandchild_inherits_restrictions", descendants)
    else:
        check("work_create_read_write_rename_truncate_delete_allowed", allowed_crud)
        check("readonly_read_allowed", lambda: same(ro_file.read_text(), "readonly fixture\n"))
        check("readonly_write_denied", lambda: denied(lambda: ro_file.write_text("changed\n")))
        check("readonly_truncate_denied", lambda: denied(lambda: os.truncate(ro_file, 0)))
        check("readonly_readonly_open_truncate_denied", lambda: denied(lambda: truncate_with_readonly_open(ro_file)))
        check("readonly_delete_denied", lambda: denied(ro_file.unlink))
        check("outside_read_denied", lambda: denied(secret.read_text))
        check("outside_write_denied", lambda: denied(lambda: secret.write_text("changed\n")))
        check("outside_create_denied", lambda: denied(lambda: (outside / "created.txt").write_text("changed\n")))
        check("outside_list_denied", lambda: denied(lambda: list(outside.iterdir())))
        check("outside_truncate_denied", lambda: denied(lambda: os.truncate(secret, 0)))
        check("outside_readonly_open_truncate_denied", lambda: denied(lambda: truncate_with_readonly_open(secret)))
        check("outside_hardlink_into_work_denied", lambda: denied(lambda: os.link(secret, work / "hardlink")))
        check("outside_rename_into_work_denied", lambda: denied(lambda: secret.rename(work / "moved-secret")))
        check("symlink_escape_read_denied", lambda: denied((work / "escape" / "secret.txt").read_text))
        check("symlink_escape_write_denied", lambda: denied(lambda: (work / "escape" / "secret.txt").write_text("changed\n")))
        check("inherited_file_descriptor_closed", closed_inherited_fd)
        check("environment_cleared", clean_environment)
        check("child_and_grandchild_inherit_restrictions", descendants)

    payload = {"generation": args.generation, "checks": checks}
    print(json.dumps(payload, sort_keys=True))
    return 0 if all(item["passed"] for item in checks) else 1


if __name__ == "__main__":
    raise SystemExit(main())
