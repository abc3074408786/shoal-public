import asyncio
import base64
import io
import json
import random
import unittest
from unittest.mock import patch

from PIL import Image
from velella_computer_use import server, windows


class BoundaryTests(unittest.TestCase):
    def tearDown(self):
        server._STATE.clear()

    def test_high_entropy_scaled_image_and_negative_origin_map(self):
        image = Image.frombytes('RGB', (7680, 2160), random.Random(42).randbytes(7680 * 2160 * 3))
        geometry = windows.DesktopGeometry(-3840, -120, 7680, 2160)
        with patch.object(windows, 'capture_desktop', return_value=(image, geometry)):
            blocks, meta = server._screenshot_payload()
        decoded = Image.open(io.BytesIO(base64.b64decode(blocks[0].data)))
        self.assertEqual(list(decoded.size), meta['size_px'])
        self.assertEqual(meta['origin_px'], [-3840, -120])
        self.assertLess(decoded.width, 7680)
        wire = json.dumps({'jsonrpc': '2.0', 'id': 1, 'result': {'content': [b.model_dump(mode='json', exclude_none=True) for b in blocks], 'isError': False}}, ensure_ascii=False).encode()
        self.assertLess(len(wire) + 1, server.FRAME_LIMIT)
        self.assertEqual(json.loads(blocks[1].text)['size_px'], list(decoded.size))
        with patch.object(windows, 'virtual_desktop', return_value=geometry):
            mapped = server._map_point(decoded.width // 2, decoded.height // 2)
            self.assertEqual(mapped, ((decoded.width // 2) * 7680 // decoded.width, (decoded.height // 2) * 2160 // decoded.height))
            with self.assertRaises(ValueError):
                server._map_point(decoded.width, 0)
        with patch.object(windows, 'virtual_desktop', return_value=windows.DesktopGeometry(0, 0, 10, 10)):
            with self.assertRaisesRegex(ValueError, '布局已变化'):
                server._map_point(1, 1)

    def test_invalid_drag_endpoint_injects_nothing(self):
        geometry = windows.DesktopGeometry(0, 0, 100, 100)
        with patch.object(windows.user32, 'SetCursorPos') as move, patch.object(windows, '_send') as send:
            with self.assertRaises(windows.InvalidInput):
                windows.drag(10, 10, 100, 20, geometry)
            move.assert_not_called()
            send.assert_not_called()

    def test_drag_releases_button_when_move_fails(self):
        geometry = windows.DesktopGeometry(0, 0, 100, 100)
        with patch.object(windows, '_send') as send, patch.object(windows.user32, 'SetCursorPos', side_effect=[True, False]):
            with self.assertRaises(windows.DesktopUnavailable):
                windows.drag(10, 10, 20, 20, geometry)
        self.assertEqual([args.args[0][0].mi.dwFlags for args in send.call_args_list], [windows.MOUSEEVENTF_LEFTDOWN, windows.MOUSEEVENTF_LEFTUP])

    def test_partial_keyboard_injection_attempts_key_release_and_errors(self):
        events = [windows._keyboard_input(windows.VK_CONTROL, 0), windows._keyboard_input(65, 0)]
        with patch.object(windows, 'require_interactive_desktop'), patch.object(windows.user32, 'SendInput', side_effect=[1, 1]) as send:
            with self.assertRaisesRegex(windows.DesktopUnavailable, '1/2'):
                windows._send(events)
        self.assertEqual(send.call_args_list[1].args[1][0].ki.dwFlags, windows.KEYEVENTF_KEYUP)

    def test_locked_desktop_rejects_before_input(self):
        with patch.object(windows.user32, 'OpenInputDesktop', return_value=0), patch.object(windows.user32, 'SendInput') as send:
            with self.assertRaises(windows.DesktopUnavailable):
                windows._send([windows._keyboard_input(65, 0)])
            send.assert_not_called()

    def test_oversized_image_and_invalid_scroll_fail_before_action(self):
        with patch.object(server, 'MAX_IMAGE_PIXELS', 10):
            with self.assertRaises(windows.DesktopUnavailable):
                server._image_bytes(Image.new('RGB', (4, 4)), 1000)
        with patch.object(windows, 'move_mouse') as move:
            with self.assertRaises(windows.InvalidInput):
                windows.scroll(1, 1, 51)
            move.assert_not_called()

    def test_read_only_dispatch_cannot_use_control_even_when_lease_exists(self):
        server._STATE['allow_input'] = False
        with self.assertRaisesRegex(ValueError, '只读|观察'):
            asyncio.run(server._call_tool('computer_acquire_control', {}))


if __name__ == '__main__':
    unittest.main()
