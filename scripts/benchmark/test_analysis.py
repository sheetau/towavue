import unittest

from analyze import summarize


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


if __name__ == "__main__":
    unittest.main()
