"""目录清单形状与坐标换算的纯逻辑检查。"""

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from velella_computer_use import catalog, windows  # noqa: E402


class CatalogTests(unittest.TestCase):
    def test_manifest_shape_matches_fish_source_contract(self):
        manifest = catalog.build_manifest(Path(sys.executable))
        catalog.validate_manifest(manifest)
        self.assertEqual(manifest["version"], 1)
        ids = [entry["id"] for entry in manifest["entries"]]
        self.assertIn(catalog.OBSERVE_ENTRY_ID, ids)
        self.assertIn(catalog.CONTROL_ENTRY_ID, ids)
        for entry in manifest["entries"]:
            self.assertTrue(Path(entry["command"]).is_absolute())
            self.assertLessEqual(len(entry["args"]), 64)
            self.assertTrue(entry["catalog"])
            self.assertLessEqual(set(entry), {"id", "command", "args", "catalog"})

    def test_observe_entry_is_read_only(self):
        manifest = catalog.build_manifest(Path(sys.executable))
        observe = catalog.entry(manifest, catalog.OBSERVE_ENTRY_ID)
        self.assertEqual(observe["catalog"], ["computer_screenshot"])
        control = catalog.entry(manifest, catalog.CONTROL_ENTRY_ID)
        self.assertIn("computer_type_text", control["catalog"])
        self.assertIn("computer_click", control["catalog"])

    def test_manifest_rejects_credential_fields(self):
        manifest = catalog.build_manifest(Path(sys.executable))
        manifest["entries"][0]["env"] = {"TOKEN": "x"}
        with self.assertRaises(catalog.CatalogError):
            catalog.validate_manifest(manifest)

    def test_manifest_rejects_relative_command(self):
        manifest = catalog.build_manifest(Path(sys.executable))
        manifest["entries"][0]["command"] = "python.exe"
        with self.assertRaises(catalog.CatalogError):
            catalog.validate_manifest(manifest)

    def test_written_manifest_round_trips(self):
        with tempfile.TemporaryDirectory(prefix="velella-computer-use-catalog-") as directory:
            path = catalog.write_manifest(Path(directory) / "catalog.json", Path(sys.executable))
            data = json.loads(Path(path).read_text(encoding="utf-8"))
            catalog.validate_manifest(data)
            self.assertTrue(Path(path).is_absolute())


class GeometryTests(unittest.TestCase):
    def test_negative_origin_maps_to_physical(self):
        geometry = windows.DesktopGeometry(origin_x=-1920, origin_y=-120, width=3840, height=1080)
        self.assertEqual(geometry.to_physical(0, 0), (-1920, -120))
        self.assertEqual(geometry.to_physical(1920, 120), (0, 0))
        self.assertEqual(geometry.to_image(-1920, -120), (0, 0))
        self.assertEqual(geometry.to_image(0, 0), (1920, 120))

    def test_bounds_reject_out_of_range_points(self):
        geometry = windows.DesktopGeometry(origin_x=0, origin_y=0, width=800, height=600)
        self.assertTrue(geometry.contains(0, 0))
        self.assertTrue(geometry.contains(799, 599))
        self.assertFalse(geometry.contains(800, 0))
        self.assertFalse(geometry.contains(-1, 0))
        self.assertFalse(geometry.contains(0, 600))

    def test_describe_reports_transform_and_negative_origin(self):
        geometry = windows.DesktopGeometry(origin_x=-1920, origin_y=0, width=3840, height=1080)
        described = geometry.describe()
        self.assertTrue(described["negative_origin"])
        self.assertEqual(described["origin_px"], [-1920, 0])
        self.assertEqual(described["transform"], "physical = origin_px + image_px")


if __name__ == "__main__":
    unittest.main()
