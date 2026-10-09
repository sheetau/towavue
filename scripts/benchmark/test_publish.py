"""Check matching and fixture weighting in the public benchmark overview."""
import json
from pathlib import Path
import unittest

import publish


def cell(case, action, app, value, attempts=1):
    return {"category": "navigation", "case": case, "scenario": action,
            "metric": "latency", "app": app, "median_ms": value, "attempts": attempts}


class AggregationTests(unittest.TestCase):
    def test_equal_fixture_weight_despite_different_actions_and_samples(self):
        rows = [cell("a", action, app, value, attempts)
                for action in ("next", "previous", "distant_first", "distant_last")
                for app, value, attempts in (("towavue", 10, 100), ("irfanview", 20, 1))]
        rows += [cell("b", "next", "towavue", 90), cell("b", "next", "irfanview", 120)]
        result = publish.aggregate(rows, ["towavue", "irfanview"])
        self.assertEqual(result["values_ms"], {"towavue": 50, "irfanview": 70})
        self.assertEqual((result["fixtures"], result["conditions"]), (2, 5))

    def test_missing_comparator_excludes_condition_from_every_app(self):
        rows = [cell("a", "next", "towavue", 10), cell("a", "next", "irfanview", 20),
                cell("b", "next", "towavue", 500)]
        result = publish.aggregate(rows, ["towavue", "irfanview"])
        self.assertEqual(result["values_ms"], {"towavue": 10, "irfanview": 20})
        self.assertEqual(result["fixtures"], 1)

    def test_duplicate_cell_cannot_silently_replace_a_measurement(self):
        row = cell("a", "next", "towavue", 10)
        with self.assertRaises(ValueError):
            publish.aggregate([row, row], ["towavue", "irfanview"])

    def test_public_endpoint_inventory_and_coverage(self):
        data = json.loads((Path(__file__).resolve().parents[2] / "docs/benchmarks/data.json").read_text(encoding="utf-8"))
        publish.validate(data)
        items = publish.overview(data)
        self.assertEqual([item["workload"] for item in items], list(publish.WORKLOADS))
        self.assertEqual([item["fixtures"] for item in items], [10, 5, 14, 5, 5, 5, 5, 10, 5, 10, 10])
        for item in items:
            keys = item["matched_conditions"]
            self.assertTrue(all(key[0] != "pacing" for key in keys))
            if item["workload"] == "Audio seek":
                self.assertTrue(all(key[1].startswith("seek-") and key[3] == "audio" for key in keys))
            if item["workload"] == "Video seek":
                self.assertTrue(all(key[3] == "video" for key in keys))
            if item["workload"].endswith("file open"):
                self.assertTrue(all(key[0] == "direct-open" for key in keys))


if __name__ == "__main__":
    unittest.main()
