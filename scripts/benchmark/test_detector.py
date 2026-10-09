"""Measurement validation; run after preparing the local pilot corpus."""
import json
from pathlib import Path
import unittest

from PIL import Image
from visual import detect


class DetectorTests(unittest.TestCase):
    def test_blank_screen_is_not_content(self):
        for color in ("black", "white", (8, 240, 96)):
            self.assertIsNone(detect(Image.new("RGB", (800, 650), color), 1))

    def test_identity_and_scaling(self):
        corpus = Path(__file__).resolve().parents[2] / "tests/generated/benchmark-v1"
        manifest = json.loads((corpus / "manifest.json").read_text(encoding="utf-8"))
        for case in manifest["files"]:
            if case["kind"] == "audio":
                continue
            reference = corpus / "references" / (case["id"] + ".png")
            with Image.open(reference) as source:
                source = source.convert("RGB")
                source.thumbnail((800, 550))
                canvas = Image.new("RGB", (800, 650), "black")
                canvas.paste(source, ((800 - source.width) // 2, 40))
                with self.subTest(case=case["id"]):
                    self.assertIsNotNone(detect(canvas, case["identity"]))
                    for wrong in range(1, 16):
                        if wrong != case["identity"]:
                            self.assertIsNone(detect(canvas, wrong))


if __name__ == "__main__":
    unittest.main()
