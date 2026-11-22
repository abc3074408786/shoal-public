"""控制租约的生命周期与互斥行为。"""

import os
import sys
import tempfile
import time
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from velella_computer_use.control import (  # noqa: E402
    ControlBusy,
    ControlLease,
    ControlStopped,
    ControlUnavailable,
)


class LeaseTests(unittest.TestCase):
    def setUp(self):
        self._temp = tempfile.TemporaryDirectory(prefix="velella-computer-use-lease-")
        self.root = Path(self._temp.name)
        self.lease = ControlLease(state_dir=self.root, ttl=60.0, owner="fish-a")

    def tearDown(self):
        self._temp.cleanup()

    def test_acquire_creates_bounded_lease(self):
        held = self.lease.acquire(role="observer")
        self.assertEqual(held.owner, "fish-a")
        self.assertEqual(held.role, "observer")
        self.assertGreater(held.expires_at, held.acquired_at)
        self.assertLessEqual(held.expires_at - held.acquired_at, 60.0)

    def test_second_process_is_busy_and_gets_no_ownership(self):
        self.lease.acquire(role="first")
        other = ControlLease(state_dir=self.root, ttl=60.0, owner="fish-b")
        with self.assertRaises(ControlBusy) as caught:
            other.acquire(role="second")
        self.assertIn("fish-a", str(caught.exception))
        self.assertEqual(caught.exception.owner["owner"], "fish-a")
        # 现有拥有者继续有效，没有发生静默转移。
        self.assertTrue(self.lease.require())

    def test_require_rejects_missing_and_foreign_leases(self):
        with self.assertRaises(ControlUnavailable):
            self.lease.require()
        self.lease.acquire()
        stranger = ControlLease(state_dir=self.root, ttl=60.0, owner="fish-c")
        with self.assertRaises(ControlUnavailable):
            stranger.require()

    def test_expiry_releases_ownership(self):
        short = ControlLease(state_dir=self.root, ttl=5.0, owner="fish-a")
        short.acquire()
        # 人为把到期时间提前，模拟租约过期；独立进程/真实时限另由验证器覆盖。
        payload = short.lease_path.read_text(encoding="utf-8")
        import json

        data = json.loads(payload)
        data["expires_at"] = time.time() - 1
        short.lease_path.write_text(json.dumps(data), encoding="utf-8")
        with self.assertRaises(ControlUnavailable):
            short.require()
        other = ControlLease(state_dir=self.root, ttl=60.0, owner="fish-d")
        held = other.acquire()
        self.assertEqual(held.owner, "fish-d")

    def test_release_and_stop_require_explicit_reacquire(self):
        self.lease.acquire()
        self.assertTrue(self.lease.release())
        with self.assertRaises(ControlUnavailable):
            self.lease.require()
        self.lease.acquire()
        receipt = self.lease.stop()
        self.assertTrue(receipt["stopped"])
        with self.assertRaises(ControlStopped):
            self.lease.acquire()
        with self.assertRaises(ControlStopped):
            self.lease.require()
        resumed = self.lease.resume()
        self.assertTrue(resumed["was_stopped"])
        self.lease.acquire()
        self.assertTrue(self.lease.require())

    def test_owner_exit_makes_lease_reclaimable(self):
        self.lease.acquire()
        import json

        data = json.loads(self.lease.lease_path.read_text(encoding="utf-8"))
        data["pid"] = 999999  # 不存在的进程
        self.lease.lease_path.write_text(json.dumps(data), encoding="utf-8")
        other = ControlLease(state_dir=self.root, ttl=60.0, owner="fish-e")
        held = other.acquire()
        self.assertEqual(held.owner, "fish-e")

    def test_stop_by_other_process_revokes_and_resume_cannot_resurrect(self):
        old = self.lease.acquire()
        other = ControlLease(state_dir=self.root, owner="local-stop")
        other.stop()
        other.resume()
        with self.assertRaises(ControlUnavailable):
            self.lease.require(old.token)
        new = self.lease.acquire()
        self.assertNotEqual(old.token, new.token)
        with self.assertRaises(ControlUnavailable):
            self.lease.require(old.token)
        with self.lease.action(new.token):
            self.assertTrue(self.lease.require(new.token))

    def test_bad_ttl_is_rejected(self):
        for ttl in (float('nan'), float('inf'), -1, 901):
            with self.assertRaises(ValueError):
                ControlLease(state_dir=self.root, ttl=ttl)

    def test_resume_filesystem_failure_is_not_reported_as_success(self):
        from unittest.mock import patch
        self.lease.stop()
        with patch.object(Path, 'unlink', side_effect=PermissionError('fixture denied')):
            with self.assertRaises(PermissionError):
                self.lease.resume()
        self.assertTrue(self.lease.stopped())

    def test_agent_cannot_undo_or_downgrade_a_local_user_stop(self):
        self.lease.acquire()
        self.lease.stop(local=True)
        other = ControlLease(state_dir=self.root, owner='agent')
        other.stop()  # Must not replace local authority with a soft Agent stop.
        with self.assertRaises(ControlStopped):
            other.resume()
        self.assertTrue(self.lease.stopped())
        self.lease.resume(local=True)
        with self.assertRaises(ControlUnavailable):
            self.lease.require()


if __name__ == "__main__":
    unittest.main()
