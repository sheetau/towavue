import unittest

from analyze import summarize
from analyze_interaction import summarize as summarize_interaction


class SummaryTests(unittest.TestCase):
    def test_warmup_is_excluded_but_failures_are_retained(self):
        base = dict(scenario="example", endpoint="content", case="image", app="example", phase="measurement")
        rows = [dict(base, status="ok", latency_lower_ms=10, latency_upper_ms=20),
                dict(base, status="ok", latency_lower_ms=30, latency_upper_ms=40),
                dict(base, status="timeout"),
                dict(base, status="ok", phase="warmup", latency_lower_ms=900, latency_upper_ms=1000)]
        result, = summarize(rows)
        self.assertEqual(result["attempts"], 3)
        self.assertEqual(result["successes"], 2)
        self.assertEqual(result["median_lower_ms"], 20)
        self.assertEqual(result["median_upper_ms"], 30)
        self.assertEqual(result["p95_upper_ms"], 40)
        self.assertEqual(result["median_interval_width_ms"], 10)
        self.assertIn('"timeout": 1', result["failures"])

    def test_all_failures_still_produce_a_row(self):
        result, = summarize([dict(scenario="example", endpoint="content", case="image", app="example",
                                 phase="measurement", status="process_exited")])
        self.assertEqual(result["success_rate"], 0)
        self.assertNotIn("median_upper_ms", result)

    def test_interaction_preserves_observation_bounds_and_missing_endpoints(self):
        rows = [dict(block=0, lower=10, upper=20),
                dict(block=0, lower=30, upper=50),
                dict(block=1, lower=60, upper=80),
                dict(block=1, status="timeout")]
        result = summarize_interaction(rows, "upper", "lower")
        self.assertEqual((result["attempts"], result["successful"], result["blocks"]), (4, 3, 2))
        self.assertEqual((result["median_lower_ms"], result["median_ms"]), (30, 50))
        self.assertEqual((result["p95_lower_ms"], result["p95_ms"]), (60, 80))
        self.assertEqual(result["median_interval_width_ms"], 20)
        self.assertLessEqual(result["median_ci_low_ms"], 50)
        self.assertGreaterEqual(result["median_ci_high_ms"], 50)


if __name__ == "__main__":
    unittest.main()
