#!/usr/bin/env python3
"""Fixed, credential-free checks for the real restricted-exec launcher."""
import argparse
import errno
import fcntl
import json
import os
from pathlib import Path
import socket
import subprocess
import sys


CASE_NAMES = [
    "work_create_read_write_rename_truncate_delete_allowed",
    "dedicated_home_and_tmp_write_allowed",
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
    "outside_chmod_denied",
    "outside_chown_denied",
    "outside_utime_denied",
    "outside_xattr_write_denied",
    "outside_stat_remains_visible",
    "ipv4_tcp_socket_denied",
    "ipv6_tcp_socket_denied",
    "ipv4_udp_socket_denied",
    "ipv6_udp_socket_denied",
    "unix_socket_denied",
    "unix_stream_socketpair_internal_read_write_allowed",
    "unix_seqpacket_socketpair_internal_read_write_allowed",
    "nonunix_socketpair_denied",
    "datagram_socketpair_denied",
    "socketpair_protocol_denied",
    "socketpair_unknown_flags_denied",
    "anonymous_pair_external_operations_denied",
    "new_session_denied",
    "new_process_group_denied",
    "fcntl_async_signal_owner_denied",
    "inherited_file_and_socket_descriptors_closed",
    "synthetic_credentials_and_environment_cleared",
    "child_and_grandchild_inherit_restrictions",
]
DESCENDANT_NAMES = [
    "descendant_outside_read_denied",
    "descendant_readonly_write_denied",
    "descendant_network_socket_denied",
    "descendant_work_write_allowed",
]


def denied(operation, errors=(errno.EACCES, errno.EPERM)):
    try:
        operation()
    except OSError as error:
        if error.errno in errors:
            return
        raise AssertionError(f"unexpected errno {error.errno}") from None
    raise AssertionError("operation unexpectedly succeeded")


def same(actual, expected):
    if actual != expected:
        raise AssertionError("value did not match fixed fixture")


def truncate_with_readonly_open(path):
    descriptor = os.open(path, os.O_RDONLY | os.O_TRUNC)
    os.close(descriptor)


def make_socket(family, kind):
    with socket.socket(family, kind):
        pass


def make_socketpair(family=socket.AF_UNIX, kind=socket.SOCK_STREAM, protocol=0):
    first, second = socket.socketpair(family, kind, protocol)
    first.close()
    second.close()


def internal_pairs(kind):
    for flags in (0, socket.SOCK_NONBLOCK, socket.SOCK_CLOEXEC,
                  socket.SOCK_NONBLOCK | socket.SOCK_CLOEXEC):
        first, second = socket.socketpair(socket.AF_UNIX, kind | flags, 0)
        try:
            # Rust's signal driver uses read/write. sendmsg/recvmsg are deliberately
            # denied so these handles cannot receive outside descriptor grants.
            same(os.write(first.fileno(), b"internal"), 8)
            same(os.read(second.fileno(), 8), b"internal")
            same(os.write(second.fileno(), b"reply"), 5)
            same(os.read(first.fileno(), 5), b"reply")
        finally:
            first.close()
            second.close()


def pair_external_operations_denied(outside):
    for kind in (socket.SOCK_STREAM, socket.SOCK_SEQPACKET):
        first, second = socket.socketpair(socket.AF_UNIX, kind, 0)
        try:
            denied(lambda: first.connect(str(outside / "external.sock")), (errno.EPERM,))
            denied(lambda: first.bind(str(outside / "external.sock")), (errno.EPERM,))
            denied(lambda: first.listen(1), (errno.EPERM,))
            denied(first.accept, (errno.EPERM,))
            denied(lambda: first.sendmsg([b"no descriptors"]), (errno.EPERM,))
            # If recvmsg is accidentally permitted, it completes and fails this check
            # immediately instead of waiting indefinitely for data.
            os.write(second.fileno(), b"x")
            denied(lambda: first.recvmsg(1), (errno.EPERM,))
        finally:
            first.close()
            second.close()


def validate_evidence(payload, generation=0):
    """Require complete typed evidence, including the nested real descendants."""
    if not isinstance(payload, dict) or type(payload.get("generation")) is not int:
        raise ValueError("Fixture generation evidence must be an integer.")
    if payload["generation"] != generation or generation not in (0, 1, 2):
        raise ValueError("Fixture generation evidence does not match.")
    expected = CASE_NAMES if generation == 0 else DESCENDANT_NAMES + (
        ["grandchild_inherits_restrictions"] if generation == 1 else []
    )
    checks = payload.get("checks")
    if (not isinstance(checks, list) or any(not isinstance(item, dict) for item in checks)
            or [item.get("name") for item in checks] != expected
            or any(type(item.get("passed")) is not bool for item in checks)):
        raise ValueError("Fixture did not return complete expected evidence.")
    if generation < 2:
        descendants = checks[-1]
        if descendants["passed"]:
            nested = validate_evidence(descendants.get("details"), generation + 1)
            if not all(item["passed"] for item in nested):
                raise ValueError("Parent claimed success despite failed descendant evidence.")
    return checks


def denied_in_nonleader_child(operation):
    """A session leader already gets EPERM from setsid/setpgid without seccomp."""
    if operation not in ("setsid", "setpgid"):
        raise ValueError("Unknown fixed child operation.")
    source = (
        "import errno,json,os,sys\n"
        "assert os.getpid() != os.getpgrp(), 'probe child must not lead the group'\n"
        "assert os.getpid() != os.getsid(0), 'probe child must not lead the session'\n"
        "try:\n"
        f"    os.{operation}(" + ("" if operation == "setsid" else "0, 0") + ")\n"
        "except OSError as error:\n"
        "    print(json.dumps({'errno': error.errno, 'nonleader': True}))\n"
        "    sys.exit(0 if error.errno == errno.EPERM else 1)\n"
        "print(json.dumps({'errno': None, 'nonleader': True}))\n"
        "sys.exit(1)\n"
    )
    result = subprocess.run(
        [sys.executable, "-I", "-c", source], capture_output=True, text=True, timeout=5,
    )
    same(result.returncode, 0)
    same(json.loads(result.stdout), {"errno": errno.EPERM, "nonleader": True})


def main():
    parser = argparse.ArgumentParser()
    for name in ("work", "home", "readonly", "outside"):
        parser.add_argument(f"--{name}", required=True)
    parser.add_argument("--inherited-fd", type=int, action="append", required=True)
    parser.add_argument("--generation", type=int, default=0)
    args = parser.parse_args()
    work, home, readonly, outside = map(
        Path, (args.work, args.home, args.readonly, args.outside)
    )
    ro_file, secret = readonly / "content.txt", outside / "secret.txt"
    checks = []

    def check(name, operation):
        try:
            details = operation()
        except Exception as error:
            result = {"name": name, "passed": False, "error": type(error).__name__}
            if isinstance(error, OSError):
                result["errno"] = error.errno
        else:
            result = {"name": name, "passed": True}
            if isinstance(details, dict):
                result["details"] = details
        checks.append(result)

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

    def home_and_tmp():
        same(os.environ.get("HOME"), str(home))
        same(os.environ.get("TMPDIR"), str(home / ".tmp"))
        for path in (home / "home-marker.txt", home / ".tmp" / "tmp-marker.txt"):
            path.write_text("owned restricted fixture\n")
            same(path.read_text(), "owned restricted fixture\n")

    def closed_inherited_fds():
        for descriptor in args.inherited_fd:
            if descriptor < 3:
                raise AssertionError("fixture descriptor must not be stdio")
            try:
                os.fstat(descriptor)
            except OSError as error:
                if error.errno == errno.EBADF:
                    continue
                raise
            raise AssertionError("launcher kept an inherited descriptor")

    def clean_environment():
        same(dict(os.environ), {
            "PATH": "/usr/local/bin:/usr/bin:/bin",
            "LANG": "C.UTF-8",
            "TERM": "xterm-256color",
            "COLORTERM": "truecolor",
            "HOME": str(home),
            "TMPDIR": str(home / ".tmp"),
        })

    def metadata_visibility():
        # Landlock controls access, not pathname metadata visibility.
        metadata = secret.stat()
        same(metadata.st_size, len("synthetic outside fixture\n"))

    def descendants():
        command = [
            sys.executable, "-I", str(Path(__file__).resolve()),
            "--work", str(work), "--home", str(home),
            "--readonly", str(readonly), "--outside", str(outside),
            "--generation", str(args.generation + 1),
        ]
        for descriptor in args.inherited_fd:
            command.extend(["--inherited-fd", str(descriptor)])
        result = subprocess.run(command, capture_output=True, text=True, timeout=10)
        payload = json.loads(result.stdout)
        same(result.returncode, 0)
        evidence = validate_evidence(payload, args.generation + 1)
        if not all(item["passed"] for item in evidence):
            raise AssertionError("descendant checks did not all pass")
        return payload

    if args.generation:
        check("descendant_outside_read_denied", lambda: denied(secret.read_text))
        check("descendant_readonly_write_denied", lambda: denied(lambda: ro_file.write_text("changed\n")))
        check("descendant_network_socket_denied", lambda: denied(
            lambda: make_socket(socket.AF_INET, socket.SOCK_STREAM), (errno.EPERM,)))
        check("descendant_work_write_allowed", lambda: same(
            (work / f"descendant-{args.generation}.txt").write_text("allowed descendant\n"),
            len("allowed descendant\n"),
        ))
        if args.generation == 1:
            check("grandchild_inherits_restrictions", descendants)
    else:
        check("work_create_read_write_rename_truncate_delete_allowed", allowed_crud)
        check("dedicated_home_and_tmp_write_allowed", home_and_tmp)
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
        check("outside_hardlink_into_work_denied", lambda: denied(
            lambda: os.link(secret, work / "hardlink"), (errno.EACCES, errno.EPERM, errno.EXDEV)))
        check("outside_rename_into_work_denied", lambda: denied(
            lambda: secret.rename(work / "moved-secret"), (errno.EACCES, errno.EPERM, errno.EXDEV)))
        check("symlink_escape_read_denied", lambda: denied((work / "escape" / "secret.txt").read_text))
        check("symlink_escape_write_denied", lambda: denied(lambda: (work / "escape" / "secret.txt").write_text("changed\n")))
        check("outside_chmod_denied", lambda: denied(lambda: secret.chmod(0o777), (errno.EPERM,)))
        check("outside_chown_denied", lambda: denied(lambda: os.chown(secret, os.getuid(), os.getgid()), (errno.EPERM,)))
        check("outside_utime_denied", lambda: denied(lambda: os.utime(secret, ns=(0, 0)), (errno.EPERM,)))
        check("outside_xattr_write_denied", lambda: denied(lambda: os.setxattr(secret, "user.velella_fixture", b"changed"), (errno.EPERM,)))
        check("outside_stat_remains_visible", metadata_visibility)
        for name, family, kind in [
            ("ipv4_tcp_socket_denied", socket.AF_INET, socket.SOCK_STREAM),
            ("ipv6_tcp_socket_denied", socket.AF_INET6, socket.SOCK_STREAM),
            ("ipv4_udp_socket_denied", socket.AF_INET, socket.SOCK_DGRAM),
            ("ipv6_udp_socket_denied", socket.AF_INET6, socket.SOCK_DGRAM),
            ("unix_socket_denied", socket.AF_UNIX, socket.SOCK_STREAM),
        ]:
            check(name, lambda family=family, kind=kind: denied(
                lambda: make_socket(family, kind), (errno.EPERM,)))
        check("unix_stream_socketpair_internal_read_write_allowed", lambda: internal_pairs(socket.SOCK_STREAM))
        check("unix_seqpacket_socketpair_internal_read_write_allowed", lambda: internal_pairs(socket.SOCK_SEQPACKET))
        check("nonunix_socketpair_denied", lambda: denied(
            lambda: make_socketpair(socket.AF_INET), (errno.EPERM,)))
        check("datagram_socketpair_denied", lambda: denied(
            lambda: make_socketpair(socket.AF_UNIX, socket.SOCK_DGRAM), (errno.EPERM,)))
        check("socketpair_protocol_denied", lambda: denied(
            lambda: make_socketpair(socket.AF_UNIX, socket.SOCK_STREAM, 1), (errno.EPERM,)))
        check("socketpair_unknown_flags_denied", lambda: denied(
            lambda: make_socketpair(socket.AF_UNIX, socket.SOCK_STREAM | 0x2000), (errno.EPERM,)))
        check("anonymous_pair_external_operations_denied", lambda: pair_external_operations_denied(outside))
        check("new_session_denied", lambda: denied_in_nonleader_child("setsid"))
        check("new_process_group_denied", lambda: denied_in_nonleader_child("setpgid"))
        check("fcntl_async_signal_owner_denied", lambda: denied(
            lambda: fcntl.fcntl(1, fcntl.F_SETOWN, os.getpid()), (errno.EPERM,)))
        check("inherited_file_and_socket_descriptors_closed", closed_inherited_fds)
        check("synthetic_credentials_and_environment_cleared", clean_environment)
        check("child_and_grandchild_inherit_restrictions", descendants)

    print(json.dumps({"generation": args.generation, "checks": checks}, sort_keys=True))
    return 0 if all(item["passed"] for item in checks) else 1


if __name__ == "__main__":
    raise SystemExit(main())
