"""Collector boundaries: sleep is not a dropped frame, missing data is not zero."""
import importlib.util
from pathlib import Path
import unittest
import tempfile

SCRIPT = Path(__file__).resolve().parents[3] / "dev/scripts/profile-runtime.py"
spec = importlib.util.spec_from_file_location("profile_runtime", SCRIPT)
collector = importlib.util.module_from_spec(spec)
spec.loader.exec_module(collector)


class CaptureTests(unittest.TestCase):
    def row(self, interval, budget, exclusion="none"):
        return (f"KEINE_TRACE\t3.5\t121\t{interval}\t6\t{budget}\tchapter2\t8"
                f"\tscripts/2.shou\tActive\t12\ttrue\t1920\t1080\t{exclusion}")

    def test_script_location_and_monitor_budget_survive_capture(self):
        frames = collector.read_frames(self.row(20, 1000 / 60) + "\n" + self.row(10, 1000 / 120))
        item = collector.rank_locations(frames)[0]
        self.assertEqual((item["scene"], item["source"], item["line"]),
                         ("chapter2", "scripts/2.shou", 12))
        self.assertEqual(item["over_budget_frames"], 2)
        self.assertAlmostEqual(item["excess_ms"], 5)

    def test_sleep_background_and_unknown_refresh_do_not_fabricate_hotspots(self):
        rows = [self.row(1000, 16, exclusion)
                for exclusion in ("sleep", "unfocused", "display-change", "sample-boundary")]
        rows.append(self.row(1000, "unknown"))
        frames = collector.read_frames("\n".join(rows))
        self.assertIsNone(frames[-1]["budget_ms"])
        self.assertEqual(collector.rank_locations(frames), [])

    def test_truncated_raw_record_is_an_error(self):
        with self.assertRaises(ValueError):
            collector.read_frames("KEINE_TRACE\t1\t2")

    def test_suite_report_retains_exclusions_source_and_workload(self):
        header = "RAWFRAME\tworkload\trun\ttotal_runs\ttarget\tcamera_profile\t" + "\t".join(collector.TRACE_FIELDS)
        row = "RAWFRAME\tidle\t1\t1\topening\truntime\t" + self.row(1000, 16, "sleep").split("\t", 1)[1]
        frames = collector.read_frames(header + "\n" + row)
        self.assertEqual((frames[0]["workload"], frames[0]["source"], frames[0]["exclusion"]),
                         ("idle", "scripts/2.shou", "sleep"))
        self.assertEqual(collector.rank_locations(frames), [])

    def test_bundle_immutability_hash_covers_store_and_segments(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "data").mkdir()
            (root / "game.haku").write_bytes(b"snapshot")
            (root / "data/a.taku").write_bytes(b"segment")
            before = collector.source_digests(root)
            (root / "capture.json").write_text("output")
            self.assertEqual(before, collector.source_digests(root))
            (root / "data/a.taku").write_bytes(b"changed")
            self.assertNotEqual(before, collector.source_digests(root))

    def test_suite_does_not_mix_camera_decomposition_costs(self):
        trace = self.row(20, 16).split("\t", 1)[1]
        frames = collector.read_frames(
            "RAWFRAME\tscene only\t1\t1\topening\tscene\t" + trace + "\n" +
            "RAWFRAME\tscene and dialog\t1\t1\topening\tscene-dialog\t" + trace)
        self.assertEqual(len(collector.rank_locations(frames)), 2)


if __name__ == "__main__":
    unittest.main()
