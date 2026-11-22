#!/usr/bin/env python3
"""Strict Linux kernel acceptance. Never changes security settings or falls back."""
import argparse
import json
import os
import pathlib
import platform
import subprocess
import sys
from datetime import datetime, timezone


def host_diagnostics():
    """Read only a fixed allowlist; never collect environment or credentials."""
    diagnostics = {'kernel': platform.release()}
    for key, filename in [
        ('userns_enabled', '/proc/sys/kernel/unprivileged_userns_clone'),
        ('max_user_namespaces', '/proc/sys/user/max_user_namespaces'),
        ('apparmor_userns_restriction', '/proc/sys/kernel/apparmor_restrict_unprivileged_userns'),
        ('security_profile', '/proc/self/attr/current'),
    ]:
        try:
            diagnostics[key] = pathlib.Path(filename).read_text().strip()
        except OSError:
            diagnostics[key] = None
    diagnostics['bubblewrap_executable'] = os.access('/usr/bin/bwrap', os.X_OK)
    diagnostics['python_executable'] = os.access('/usr/bin/python3', os.X_OK)
    return diagnostics


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', default='artifacts/sandbox-acceptance')
    args = parser.parse_args()
    repo = pathlib.Path(__file__).resolve().parents[1]
    output = pathlib.Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    receipt = {'schema': 1, 'platform': platform.system(), 'time': datetime.now(timezone.utc).isoformat(),
               'status': 'running', 'checks': [], 'scope': 'kernel primitives and task assembly; no authenticated Agent acceptance'}

    def finish(status, reason, code):
        receipt.update(status=status, reason=reason)
        (output / 'result.json').write_text(json.dumps(receipt, indent=2) + '\n')
        print(json.dumps(receipt, indent=2))
        return code

    def run(name, command, env=None):
        try:
            result = subprocess.run(command, cwd=repo, env=env, capture_output=True, text=True, timeout=900)
        except (OSError, subprocess.TimeoutExpired) as error:
            receipt['checks'].append({'name': name, 'passed': False, 'error': str(error)})
            return None
        (output / (name + '.log')).write_text(result.stdout + result.stderr)
        receipt['checks'].append({'name': name, 'passed': result.returncode == 0, 'exit': result.returncode})
        return result

    if sys.platform != 'linux':
        return finish('blocked', 'Linux acceptance requires a Linux host.', 2)
    receipt['host'] = host_diagnostics()
    metadata = run('metadata', ['cargo', 'metadata', '--no-deps', '--format-version', '1'])
    if metadata is None or metadata.returncode:
        return finish('failed', 'Cannot locate Cargo build output; see metadata.log.', 1)
    try:
        target = pathlib.Path(json.loads(metadata.stdout)['target_directory'])
        if not target.is_absolute():
            raise ValueError('target_directory must be absolute')
    except (ValueError, KeyError, TypeError):
        return finish('failed', 'Cargo metadata did not contain a valid target_directory.', 1)
    build = run('build', ['cargo', 'build', '-p', 'velella', '--bin', 'velella-runtime'])
    if build is None or build.returncode:
        return finish('failed', 'Runtime build failed; see build.log.', 1)
    probe = run('capabilities', [str(target / 'debug/velella-runtime'), 'sandbox-capabilities'])
    if probe is None or probe.returncode:
        return finish('failed', 'Capability probe failed.', 1)
    try:
        capability = json.loads(probe.stdout)
    except ValueError:
        return finish('failed', 'Capability probe did not return JSON.', 1)
    if not isinstance(capability, dict) or type(capability.get('available')) is not bool:
        return finish('failed', 'Capability probe returned an invalid available field.', 1)
    receipt['capabilities'] = capability
    if capability.get('available') is not True:
        receipt['next_step'] = (
            'Run this same script on a prepared Linux host that permits bubblewrap '
            'user/PID/network namespaces and a private /proc mount. The diagnostic '
            'values alone do not identify which host rule denied the operation. '
            'Do not count refusal as confinement acceptance.'
        )
        return finish('blocked', capability.get('reason') or 'Sandbox unavailable.', 2)
    if not receipt['host']['python_executable']:
        return finish('blocked', 'Kernel acceptance requires /usr/bin/python3 inside the sandbox.', 2)
    env = dict(os.environ, VELELLA_REQUIRE_SANDBOX='1',
               VELELLA_TEST_RUNTIME=str(target / 'debug/velella-runtime'))
    for name, selector in [('kernel', 'sandbox::tests::live_confinement_or_explicit_unavailable'),
                           ('task-policy', 'task_sandbox::'), ('hook-bridge', 'host::tests::sandbox')]:
        result = run(name, ['cargo', 'test', '-p', 'velella', '--lib', selector, '--', '--nocapture'], env)
        if result is None or result.returncode:
            return finish('failed', f'{name} failed; see its log.', 1)
    result = run('task-pipeline', [
        'cargo', 'test', '-p', 'velella', '--lib', 'live_task_pipeline_with_fixture_agent',
        '--', '--ignored', '--nocapture'], env)
    if result is None or result.returncode:
        return finish('failed', 'Fixture Agent task pipeline failed; see task-pipeline.log.', 1)
    return finish('passed', 'Kernel confinement, task policy, hook bridge and real task pipeline with a deterministic test Agent passed. No authenticated model was used.', 0)


if __name__ == '__main__':
    raise SystemExit(main())
