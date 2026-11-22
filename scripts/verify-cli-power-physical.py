"""Observe physical power transitions and two installed-runtime PTYs.

The observer never creates a power request, changes policy, injects input, locks
the computer or touches the production profile. Hardware actions belong to the
user. Windows API references are recorded in the adjacent OpenSpec UAT map.
"""
import argparse
import bisect
import ctypes as c
from ctypes import wintypes as w
import hashlib
import json
import os
from pathlib import Path
import queue
import re
import subprocess
import sys
import time
import uuid
from cli_power_window_notifications import WindowObserver


def save(path, value):
    temporary = path.with_suffix('.tmp')
    temporary.write_text(json.dumps(value, indent=2), encoding='utf-8')
    os.replace(temporary, path)


class PowerStatus(c.Structure):
    _fields_ = [('ac', w.BYTE), ('flags', w.BYTE), ('percent', w.BYTE),
                ('saver', w.BYTE), ('life', w.DWORD), ('full', w.DWORD)]


class WtsLevel1(c.Structure):
    _fields_ = [('session', w.ULONG), ('state', c.c_int), ('flags', w.LONG),
                ('station', w.WCHAR * 33), ('user', w.WCHAR * 21), ('domain', w.WCHAR * 18),
                *[(name, c.c_int64) for name in ('logon', 'connect', 'disconnect', 'last_input', 'now')],
                *[(name, w.DWORD) for name in ('in_bytes', 'out_bytes', 'in_frames', 'out_frames', 'in_compressed', 'out_compressed')]]


class WtsInfo(c.Structure):
    _fields_ = [('level', w.DWORD), ('data', WtsLevel1)]


class Observer:
    SETTINGS = {'lid': 'ba3e0f4d-b817-4094-a2d1-d56379e6a0f3',
                'display': '2b84c20e-ad23-4ddf-93db-05ffbd7efca5',
                'source': '5d3e9a59-e9d5-4b00-a6bd-ff34ff516548'}

    def __init__(self):
        self.events, self.values, self.handles = queue.SimpleQueue(), {}, []
        self.dll = c.WinDLL('powrprof.dll')
        self.kernel = c.WinDLL('kernel32.dll', use_last_error=True)
        self.kernel.GetSystemPowerStatus.argtypes = [c.POINTER(PowerStatus)]
        self.kernel.GetSystemPowerStatus.restype = w.BOOL
        self.wts = c.WinDLL('wtsapi32.dll', use_last_error=True)
        self.wts.WTSQuerySessionInformationW.argtypes = [c.c_void_p, w.DWORD, c.c_int, c.POINTER(c.c_void_p), c.POINTER(w.DWORD)]
        self.wts.WTSQuerySessionInformationW.restype = w.BOOL
        self.wts.WTSFreeMemory.argtypes = [c.c_void_p]
        self.dll.PowerSettingRegisterNotification.argtypes = [c.c_void_p, w.DWORD, c.c_void_p, c.POINTER(c.c_void_p)]
        self.dll.PowerSettingRegisterNotification.restype = w.DWORD
        self.dll.PowerSettingUnregisterNotification.argtypes = [c.c_void_p]
        self.dll.PowerSettingUnregisterNotification.restype = w.DWORD
        callback_type = c.WINFUNCTYPE(w.DWORD, c.c_void_p, w.DWORD, c.c_void_p)

        class Subscribe(c.Structure):
            _fields_ = [('callback', callback_type), ('context', c.c_void_p)]

        def changed(context, kind, setting):
            if kind == 0x8013 and setting:
                header = c.string_at(setting, 20)
                name = str(uuid.UUID(bytes_le=header[:16]))
                size = int.from_bytes(header[16:], 'little')
                if size == 4:
                    value = int.from_bytes(c.string_at(setting + 20, 4), 'little')
                    self.events.put({'wall_ns': time.time_ns(), 'guid': name, 'value': value,
                                     'channel': 'callback', 'kind': 'power'})
            return 0

        self.callback = callback_type(changed)
        self.subscription = Subscribe(self.callback, None)
        try:
            for name, guid in self.SETTINGS.items():
                raw = c.create_string_buffer(uuid.UUID(guid).bytes_le)
                handle = c.c_void_p()
                code = self.dll.PowerSettingRegisterNotification(raw, 2, c.byref(self.subscription), c.byref(handle))
                if code:
                    raise OSError(code, 'Cannot observe ' + name)
                self.handles.append(handle)
            self.window = WindowObserver()
        except BaseException:
            self.close()
            raise

    def sample(self):
        status = PowerStatus()
        if not self.kernel.GetSystemPowerStatus(c.byref(status)):
            raise c.WinError(c.get_last_error())
        updates = []
        while not self.events.empty():
            item = self.events.get()
            name = next((key for key, guid in self.SETTINGS.items() if guid == item['guid']), None)
            if name:
                item['setting'] = name
                self.values[name] = item['value']
                updates.append(item)
        window_events = self.window.drain()
        updates.extend(window_events)
        pointer, length = c.c_void_p(), w.DWORD()
        if not self.wts.WTSQuerySessionInformationW(None, 0xffffffff, 25, c.byref(pointer), c.byref(length)):
            raise c.WinError(c.get_last_error())
        try:
            if length.value < c.sizeof(WtsInfo):
                raise RuntimeError('Incomplete WTS session state')
            info = c.cast(pointer, c.POINTER(WtsInfo)).contents
            if info.level != 1 or info.data.flags not in (0, 1):
                raise RuntimeError('Unknown lock state')
            locked = info.data.flags == 0
        finally:
            self.wts.WTSFreeMemory(pointer)
        return {'wall_ns': time.time_ns(), 'ac': status.ac, 'battery_percent': status.percent,
                'locked': locked, **self.values}, updates

    def close(self):
        if hasattr(self, 'window'):
            self.window.close()
        for handle in self.handles:
            self.dll.PowerSettingUnregisterNotification(handle)
        self.handles.clear()


def analyze(root, minimum=60, write_result=True):
    run = json.loads((root / 'run.json').read_text(encoding='utf-8'))
    samples = [json.loads(line) for line in (root / 'samples.jsonl').read_text(encoding='utf-8').splitlines()]
    event_path = root / 'events.jsonl'
    events = [json.loads(line) for line in event_path.read_text(encoding='utf-8').splitlines()] if event_path.exists() else []
    heartbeats = {}
    for label in ('A', 'B'):
        data = (root / label / 'terminal.bin').read_bytes()
        rows = [(int(seq), int(wall)) for seq, wall, tick in re.findall(
            rb'VPUAT ' + label.encode() + rb' (\d+) (\d+) (\d+)', data)]
        heartbeats[label] = rows
    results = {}
    for feature, predicate in [('lid', lambda row: row.get('lid') == 0),
                               ('display', lambda row: row.get('display') == 0),
                               ('lock', lambda row: row.get('locked') is True)]:
        intervals, start = [], None
        for index, sample in enumerate(samples):
            if predicate(sample) and sample['ac'] == 1:
                if start is None:
                    start = sample['wall_ns']
            elif start is not None:
                intervals.append((start, sample['wall_ns'], True, 'sample'))
                start = None
        if start is not None and samples:
            intervals.append((start, samples[-1]['wall_ns'], False, 'sample'))
        # Keep raw transitions: a callback off/on pair between two samples must
        # not disappear merely because sample() kept only the latest value.
        relevant_events = [event for event in events if event.get('setting') == feature]
        for channel in ('callback', 'window'):
            start = None
            for event in sorted(relevant_events, key=lambda row: row['wall_ns']):
                if event.get('channel', 'callback') != channel:
                    continue
                if event['value'] == 0 and start is None:
                    start = event['wall_ns']
                elif event['value'] == 1 and start is not None:
                    intervals.append((start, event['wall_ns'], True, channel))
                    start = None
            if start is not None and samples and samples[-1]['wall_ns'] >= start:
                intervals.append((start, samples[-1]['wall_ns'], False, channel))
        checked = []
        sample_times = [sample['wall_ns'] for sample in samples]
        for start, end, closed, source in intervals:
            duration = (end - start) / 1e9
            if duration < minimum or not closed:
                continue
            before = bisect.bisect_right(sample_times, start) - 1
            after = bisect.bisect_left(sample_times, end)
            ac_confirmed = (before >= 0 and after < len(samples) and
                            all(row['ac'] == 1 for row in samples[before:after + 1]))
            checks = {}
            for label, rows in heartbeats.items():
                times = [wall for _, wall in rows]
                left, right = bisect.bisect_left(times, start), bisect.bisect_right(times, end)
                bracketed = left > 0 and right < len(rows)
                span = rows[max(0, left - 1):min(len(rows), right + 1)]
                gaps = [(b[1] - a[1]) / 1e9 for a, b in zip(span, span[1:])]
                sequence = all(b[0] == a[0] + 1 for a, b in zip(span, span[1:]))
                maximum = max(gaps, default=float('inf'))
                checks[label] = {'ok': bracketed and sequence and maximum <= 3 and len(span) >= duration * .8,
                                 'lines': len(span), 'max_gap_seconds': maximum, 'bracketed': bracketed}
            checked.append({'start_ns': start, 'end_ns': end, 'duration_seconds': duration, 'source': source,
                            'ac_confirmed': ac_confirmed,
                            'ok': ac_confirmed and all(value['ok'] for value in checks.values()), 'clis': checks})
        observed = [{'start_ns': start, 'end_ns': end, 'closed': closed,
                     'duration_seconds': (end-start)/1e9, 'source': source}
                    for start, end, closed, source in intervals]
        fully_covered = [row for row in checked if row['ac_confirmed'] and
                         all(cli['bracketed'] for cli in row['clis'].values())]
        results[feature] = {'status': ('passed' if any(row['ok'] for row in checked) else
                                      'failed' if fully_covered else
                                      'incomplete_coverage' if observed else 'not_observed'),
                            'coverage_note': 'No observed qualifying interval does not mean the user did not perform the action.',
                            'raw_transition_count': len(relevant_events),
                            'observed_intervals': observed, 'intervals': checked}
    result = {'run': run, 'features': results, 'all_passed': all(row['status'] == 'passed' for row in results.values()),
              'heartbeat_lines': {key: len(value) for key, value in heartbeats.items()},
              'no_model_requests': True, 'observer_holds_no_power_request': True,
              'production_profile_modified': False}
    if write_result:
        save(root / 'result.json', result)
    return result


def capture_progress(result, feature):
    """A reply/stop request is never evidence that a physical interval passed."""
    checked = result['features'][feature]
    observed = checked['observed_intervals']
    if checked['status'] == 'passed':
        phase = 'qualified'
    elif any(not row['closed'] for row in observed):
        phase = 'capturing_interval'
    elif any(row['closed'] and row['duration_seconds'] >= result['run']['minimum_segment_seconds']
             for row in observed):
        phase = 'checking_continuity_and_post_transition_output'
    elif observed:
        phase = 'interval_too_short'
    else:
        phase = 'waiting_for_transition'
    return {'feature': feature, 'phase': phase, 'status': checked['status'],
            'at_ns': time.time_ns(), 'qualified': checked['status'] == 'passed',
            'observed_intervals': observed}


def verify_owned_host(process, label):
    # The original Popen process handle is the identity/liveness authority.
    # status.json is a concurrently replaced UI snapshot. Opening it on Windows
    # can race MoveFileExW and must not terminate an otherwise healthy capture.
    if process.poll() is not None:
        raise RuntimeError('Test host exited unexpectedly: ' + label)


def monitor(args):
    runtime = args.runtime.resolve(strict=True)
    if hashlib.sha256(runtime.read_bytes()).hexdigest() != args.expected_sha256:
        raise RuntimeError('Installed runtime fingerprint changed')
    root = args.output.resolve()
    root.mkdir(parents=True, exist_ok=False)
    observer, processes, handles = Observer(), [], []
    finish_reason = 'error'
    try:
        initial, events = observer.sample()
        if initial['ac'] != 1:
            raise RuntimeError('Connect AC power before this AC-only physical test')
        run = {'runtime': str(runtime), 'sha256': args.expected_sha256, 'started_ns': time.time_ns(),
               'observer_version': 4, 'observer_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
               'window_notifications_ready': observer.window.ping.is_set(),
               'limit_seconds': args.seconds, 'minimum_segment_seconds': args.minimum_seconds, 'hosts': {},
               'until_feature': args.until_feature,
               'initial': initial, 'production_profile_modified': False, 'observer_holds_no_power_request': True}
        for label in ('A', 'B'):
            directory = root / label
            directory.mkdir()
            descriptor = {'id': 'physical-uat-' + label, 'endpoint': 'velella-power-uat-' + uuid.uuid4().hex,
                          'directory': str(directory), 'spawn': {'program': sys.executable,
                          'args': ['-u', str(Path(__file__).resolve()), '--child', label, '--seconds', str(args.seconds + 30)],
                          'adapter': 'generic' if label == 'A' else 'physical-uat-unknown-cli', 'env': {}}}
            launch = directory / 'launch.json'
            save(launch, descriptor)
            stdout, stderr = [(directory / name).open('wb') for name in ('stdout.log', 'stderr.log')]
            handles.extend((stdout, stderr))
            process = subprocess.Popen([str(runtime), 'host', str(launch)], stdin=subprocess.DEVNULL,
                                       stdout=stdout, stderr=stderr, creationflags=subprocess.CREATE_NO_WINDOW)
            processes.append(process)
            run['hosts'][label] = {'pid': process.pid, 'directory': str(directory)}
        save(root / 'run.json', run)
        with (root / 'samples.jsonl').open('a', encoding='utf-8', buffering=1) as samples, (root / 'events.jsonl').open('a', encoding='utf-8', buffering=1) as event_log:
            for event in events:
                event_log.write(json.dumps(event) + '\n')
            until, ready = time.monotonic() + args.seconds, False
            while time.monotonic() < until:
                if (root / 'stop').exists():
                    finish_reason = 'aborted_by_stop_file'
                    break
                sample, updates = observer.sample()
                for event in updates:
                    event_log.write(json.dumps(event) + '\n')
                for label, process in zip(('A', 'B'), processes):
                    verify_owned_host(process, label)
                samples.write(json.dumps(sample) + '\n')
                if not ready and all((root / label / 'terminal.bin').exists() and b'VPUAT ' + label.encode() + b' 5 ' in (root / label / 'terminal.bin').read_bytes() for label in ('A', 'B')):
                    ready = True
                    save(root / 'ready.json', {'ready': True, 'at_ns': time.time_ns(), 'root': str(root), 'initial': initial})
                    print(json.dumps({'ready': True, 'root': str(root)}), flush=True)
                if ready and args.until_feature:
                    progress = capture_progress(analyze(root, args.minimum_seconds, write_result=False), args.until_feature)
                    save(root / 'progress.json', progress)
                    if progress['qualified']:
                        finish_reason = 'qualified_interval_and_post_transition_output'
                        break
                time.sleep(1)
            else:
                finish_reason = 'deadline_without_qualified_interval' if args.until_feature else 'duration_elapsed'
        if not ready:
            raise RuntimeError('Heartbeat warmup did not complete')
    finally:
        # These Popen instances are owned by this probe; closing each host's job
        # terminates only its test PTY. Never stop the real daemon or user hosts.
        for process in processes:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=10)
        for handle in handles:
            handle.close()
        observer.close()
        save(root / 'cleanup.json', {'owned_hosts_stopped': all(process.poll() is not None for process in processes),
                                    'finished_ns': time.time_ns(), 'finish_reason': finish_reason,
                                    'production_profile_modified': False})
    print(json.dumps(analyze(root, args.minimum_seconds)), flush=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--expected-sha256')
    parser.add_argument('--output', type=Path)
    parser.add_argument('--seconds', type=int, default=900)
    parser.add_argument('--minimum-seconds', type=int, default=60)
    parser.add_argument('--child', choices=('A', 'B'))
    parser.add_argument('--analyze', action='store_true')
    parser.add_argument('--until-feature', choices=('lock', 'display', 'lid'),
                        help='Finish after a qualifying complete interval and output after return; --seconds is a hard deadline')
    args = parser.parse_args()
    if args.child:
        end, seq = time.monotonic() + args.seconds, 0
        while time.monotonic() < end:
            seq += 1
            print(f'VPUAT {args.child} {seq} {time.time_ns()} {time.perf_counter_ns()}', flush=True)
            time.sleep(1)
    elif args.analyze:
        print(json.dumps(analyze(args.output.resolve(), args.minimum_seconds)), flush=True)
    else:
        if not args.runtime or not args.expected_sha256 or not args.output:
            parser.error('--runtime, --expected-sha256 and --output are required')
        monitor(args)


if __name__ == '__main__':
    main()
