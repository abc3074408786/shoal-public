#!/usr/bin/python3
"""Fixed Codex protocol fixture: no model, login, network, or credentials.

This executable exercises Velella's real sandbox, PTY, and hook helper. It is
deliberately not a Codex implementation or evidence of a successful model call.
"""

import json
import os
import pathlib
import re
import socket
import stat
import subprocess
import sys
import time
import uuid


VERSION = "velella-codex-protocol-fixture 1 (no model; no login)"
EVENTS = {
    "SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse",
    "PermissionRequest", "Stop", "Interrupt",
}
REQUIRED_EVENTS = {"SessionStart", "UserPromptSubmit", "Stop"}
# serde_json::to_string() emits the command as a JSON string inside this
# restricted TOML shape. Parse only that shape, without requiring Python 3.11.
HOOK_SETTING = re.compile(
    r'hooks\.(?P<event>[A-Za-z]+)\s*=\s*'
    r'\[\s*\{\s*hooks\s*=\s*\[\s*\{\s*type\s*=\s*"command"\s*,\s*'
    r'command\s*=\s*(?P<command>"(?:\\.|[^"\\])*")'
    r'\s*\}\s*\]\s*\}\s*\]'
)
CORRECT_CLAMP = (
    "def clamp(value, lower, upper):\n"
    "    if lower > upper:\n"
    "        raise ValueError('lower must not exceed upper')\n"
    "    return max(lower, min(value, upper))\n"
)


def reject_login(arguments):
    for argument in arguments:
        if (
            re.match(r"^(?:--?)?login(?:=|$)", argument, re.IGNORECASE)
            or argument.split("=", 1)[0] in {"--device-auth", "--with-api-key"}
        ):
            raise ValueError("this fixture refuses all login operations")


def parse_hooks(arguments):
    """Extract the runtime's fixed -c hooks.Event=[...] assignments."""
    reject_login(arguments)
    hooks = {}
    index = 0
    while index < len(arguments):
        if arguments[index] != "-c":
            index += 1
            continue
        if index + 1 >= len(arguments):
            raise ValueError("missing hook setting after -c")
        match = HOOK_SETTING.fullmatch(arguments[index + 1])
        if match is None or match["event"] not in EVENTS:
            raise ValueError("unsupported fixture hook setting")
        event = match["event"]
        if event in hooks:
            raise ValueError("duplicate fixture hook setting")
        command = json.loads(match["command"])
        if not command or "\0" in command:
            raise ValueError("invalid fixture hook command")
        hooks[event] = command
        index += 2
    if not REQUIRED_EVENTS.issubset(hooks):
        raise ValueError("required fixture hooks were not supplied")
    return hooks


def hook(command, name, session):
    subprocess.run(
        command,
        shell=True,
        check=True,
        timeout=10,
        input=json.dumps({"hook_event_name": name, "session_id": session}),
        text=True,
    )


def main(arguments):
    reject_login(arguments)
    if arguments == ["--version"]:
        print(VERSION, flush=True)
        return
    hooks = parse_hooks(arguments)
    restricted = bool(os.environ.get("VELELLA_HOOK_INBOX"))
    home = pathlib.Path(os.environ["HOME"]) if restricted else pathlib.Path("/home/velella")
    if os.environ.get("HOME") != str(home) or pathlib.Path.home() != home:
        raise RuntimeError("fixture requires its dedicated HOME")
    if restricted:
        outside = pathlib.Path((home / "outside-marker-path").read_text())
        try:
            outside.read_text()
        except PermissionError:
            pass
        else:
            raise RuntimeError("restricted fixture can read outside contents")
        for family, kind in [(socket.AF_INET, socket.SOCK_STREAM),
                             (socket.AF_INET, socket.SOCK_DGRAM),
                             (socket.AF_INET6, socket.SOCK_STREAM),
                             (socket.AF_UNIX, socket.SOCK_STREAM)]:
            try:
                socket.socket(family, kind)
            except PermissionError:
                pass
            else:
                raise RuntimeError("restricted fixture can create a socket")
    if (home / "setup-state").read_text() != "fixture-prepared":
        raise RuntimeError("fixture setup state is missing")
    # Check presence only. Never read or copy authentication data.
    if os.path.lexists(home / ".codex" / "auth.json"):
        raise RuntimeError("fixture refuses a HOME containing authentication")
    if not restricted and not os.environ.get("VELELLA_SESSION_ENDPOINT"):
        raise RuntimeError("fixture requires the runtime's isolated hook endpoint")
    candidate = pathlib.Path("clamp.py")
    candidate_stat = candidate.lstat()
    if not stat.S_ISREG(candidate_stat.st_mode) or candidate_stat.st_nlink != 1:
        raise RuntimeError("fixture requires a regular, unlinked clamp.py")
    session = str(uuid.uuid4())
    if arguments[:1] == ["resume"]:
        if len(arguments) < 2:
            raise ValueError("missing resumed fixture session")
        session = str(uuid.UUID(arguments[1]))
    print(VERSION, flush=True)
    hook(hooks["SessionStart"], "SessionStart", session)
    hook(hooks["UserPromptSubmit"], "UserPromptSubmit", session)
    candidate.write_text(CORRECT_CLAMP)
    hook(hooks["Stop"], "Stop", session)
    print("Fixed fixture edit complete; waiting for the runtime to stop it.", flush=True)
    while True:
        time.sleep(1)


if __name__ == "__main__":
    try:
        main(sys.argv[1:])
    except Exception as error:
        print("Codex protocol fixture failed: " + str(error), file=sys.stderr, flush=True)
        sys.exit(1)
