#!/usr/bin/python3
"""Deterministic integration-test agent. Does not call a model or use credentials."""
import json
import os
import pathlib
import subprocess
import sys
import time

args = sys.argv[1:]
settings = json.loads(pathlib.Path(args[args.index('--settings') + 1]).read_text())
resumed = '--resume' in args
session = args[args.index('--resume' if resumed else '--session-id') + 1]
assert pathlib.Path.home() == pathlib.Path('/home/velella')
assert (pathlib.Path.home() / 'setup-state').read_text() == 'retained'
assert pathlib.Path('setup-ok').read_text() == 'prepared'
assert 'VELELLA_SESSION_ENDPOINT' in os.environ
# The actual hook helper crosses the read-only mounted event-only Unix socket.
def hook(name):
    command = settings['hooks'][name][0]['hooks'][0]['command']
    subprocess.run(command, shell=True, check=True, timeout=10,
                   input=json.dumps({'hook_event_name': name, 'session_id': session}), text=True)
hook('SessionStart')
hook('UserPromptSubmit')
subprocess.run(['git', 'status', '--porcelain'], check=True, timeout=10)
pathlib.Path('agent-result').write_text('verified')
if resumed:
    pathlib.Path('resumed-ok').write_text(session)
hook('Stop')
while True:
    time.sleep(1)
