"""Synthetic evidence-classification tests; these are never physical UAT."""
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
spec = importlib.util.spec_from_file_location('power_physical', Path(__file__).with_name('verify-cli-power-physical.py'))
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


class ObservationTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix='velella-power-observation-')).resolve()
        (self.root / 'run.json').write_text(json.dumps({'synthetic': True, 'minimum_segment_seconds': 60}), encoding='utf-8')
        rows = [{'wall_ns': second * 10**9, 'ac': 1, 'locked': False, 'display': 1, 'lid': 1} for second in range(76)]
        (self.root / 'samples.jsonl').write_text('\n'.join(map(json.dumps, rows)), encoding='utf-8')
        for label in ('A', 'B'):
            (self.root / label).mkdir()
            (self.root / label / 'terminal.bin').write_bytes(''.join(
                f'VPUAT {label} {second+1} {second*10**9} {second*10**9}\r\n' for second in range(76)).encode())

    def tearDown(self):
        expected_parent = Path(tempfile.gettempdir()).resolve()
        assert self.root.parent == expected_parent and self.root.name.startswith('velella-power-observation-')
        shutil.rmtree(self.root)

    def events(self, setting, start, end, channel='window'):
        rows = [{'wall_ns': int(start * 10**9), 'channel': channel, 'setting': setting, 'value': 0},
                {'wall_ns': int(end * 10**9), 'channel': channel, 'setting': setting, 'value': 1}]
        (self.root / 'events.jsonl').write_text('\n'.join(map(json.dumps, rows)), encoding='utf-8')

    def test_direct_lock_events_remain_visible_when_polled_state_stays_unlocked(self):
        self.events('lock', 5, 66)
        result = probe.analyze(self.root)
        self.assertTrue(result['run']['synthetic'])
        self.assertEqual(result['features']['lock']['status'], 'passed')
        self.assertEqual(result['features']['lock']['intervals'][0]['source'], 'window')

    def test_short_between_sample_transition_is_retained_but_not_a_pass(self):
        self.events('display', 5.2, 5.7, 'callback')
        result = probe.analyze(self.root)
        self.assertEqual(result['features']['display']['raw_transition_count'], 2)
        self.assertEqual(result['features']['display']['status'], 'incomplete_coverage')

    def test_no_observation_does_not_deny_user_action(self):
        result = probe.analyze(self.root)
        self.assertEqual(result['features']['lock']['status'], 'not_observed')
        self.assertIn('does not mean', result['features']['lock']['coverage_note'])
        self.assertFalse(result['all_passed'])

    def test_output_gap_still_fails_despite_valid_os_events(self):
        self.events('lock', 5, 66)
        path = self.root / 'A' / 'terminal.bin'
        lines = path.read_bytes().splitlines(keepends=True)
        path.write_bytes(b''.join(lines[:20] + lines[40:]))
        self.assertEqual(probe.analyze(self.root)['features']['lock']['status'], 'failed')

    def test_battery_interval_cannot_pass_ac_acceptance(self):
        self.events('lock', 5, 66)
        path = self.root / 'samples.jsonl'
        rows = [json.loads(line) for line in path.read_text(encoding='utf-8').splitlines()]
        rows[30]['ac'] = 0
        path.write_text('\n'.join(map(json.dumps, rows)), encoding='utf-8')
        self.assertNotEqual(probe.analyze(self.root)['features']['lock']['status'], 'passed')

    def test_open_lock_is_observed_but_never_completes_capture(self):
        path = self.root / 'samples.jsonl'
        rows = [json.loads(line) for line in path.read_text(encoding='utf-8').splitlines()]
        for row in rows[5:]:
            row['locked'] = True
        path.write_text('\n'.join(map(json.dumps, rows)), encoding='utf-8')
        result = probe.analyze(self.root)
        self.assertEqual(result['features']['lock']['status'], 'incomplete_coverage')
        progress = probe.capture_progress(result, 'lock')
        self.assertEqual(progress['phase'], 'capturing_interval')
        self.assertFalse(progress['qualified'])

    def test_unlock_without_output_after_return_does_not_complete_capture(self):
        self.events('lock', 5, 75)
        result = probe.analyze(self.root)
        self.assertEqual(result['features']['lock']['status'], 'incomplete_coverage')
        self.assertFalse(probe.capture_progress(result, 'lock')['qualified'])

    def test_full_interval_and_return_output_complete_capture_without_overwriting_result(self):
        self.events('lock', 5, 66)
        result = probe.analyze(self.root, write_result=False)
        self.assertFalse((self.root / 'result.json').exists())
        self.assertTrue(probe.capture_progress(result, 'lock')['qualified'])

    def test_unclosed_raw_window_lock_is_not_silently_discarded(self):
        row = {'wall_ns': 5*10**9, 'channel': 'window', 'setting': 'lock', 'value': 0}
        (self.root / 'events.jsonl').write_text(json.dumps(row), encoding='utf-8')
        result = probe.analyze(self.root)
        self.assertEqual(result['features']['lock']['status'], 'incomplete_coverage')
        self.assertEqual(probe.capture_progress(result, 'lock')['phase'], 'capturing_interval')

    def test_windows_status_file_lock_cannot_abort_owned_live_host_check(self):
        path = self.root / 'status.json'
        path.write_text('{}', encoding='utf-8')
        kernel = probe.c.WinDLL('kernel32.dll', use_last_error=True)
        kernel.CreateFileW.argtypes = [probe.w.LPCWSTR, probe.w.DWORD, probe.w.DWORD,
                                      probe.c.c_void_p, probe.w.DWORD, probe.w.DWORD, probe.c.c_void_p]
        kernel.CreateFileW.restype = probe.c.c_void_p
        kernel.CloseHandle.argtypes = [probe.c.c_void_p]
        handle = kernel.CreateFileW(str(path), 0x80000000, 0, None, 3, 0, None)
        self.assertNotEqual(handle, probe.c.c_void_p(-1).value)
        child = None
        try:
            with self.assertRaises(PermissionError):
                path.read_text(encoding='utf-8')
            child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(30)'],
                                     creationflags=subprocess.CREATE_NO_WINDOW)
            probe.verify_owned_host(child, 'A')
        finally:
            if child is not None:
                child.kill()
                child.wait(timeout=5)
            kernel.CloseHandle(handle)

    def test_exited_owned_host_still_fails_even_without_status_snapshot(self):
        child = subprocess.Popen([sys.executable, '-c', 'pass'], creationflags=subprocess.CREATE_NO_WINDOW)
        child.wait(timeout=5)
        with self.assertRaisesRegex(RuntimeError, 'Test host exited unexpectedly'):
            probe.verify_owned_host(child, 'A')


if __name__ == '__main__':
    unittest.main()
