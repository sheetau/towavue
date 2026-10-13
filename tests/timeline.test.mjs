import test from "node:test";
import assert from "node:assert/strict";
import { updateSelection, MIN_SELECTION, diagonalSize } from "../src/site/timeline.mjs";

test("selection resizing cannot cross the other edge or escape the timeline", () => {
  assert.deepEqual(updateSelection([0.3, 0.7], "start", -2), [0, 0.7]);
  assert.deepEqual(updateSelection([0.3, 0.7], "end", 2), [0.3, 1]);
  const [start, end] = updateSelection([0.3, 0.7], "start", 0.9);
  assert.ok(Math.abs(end - start - MIN_SELECTION) < 1e-10);
  const [start2, end2] = updateSelection([0.3, 0.7], "end", 0);
  assert.ok(Math.abs(end2 - start2 - MIN_SELECTION) < 1e-10);
});

test("moving the selection preserves its duration at both boundaries", () => {
  for (const target of [-3, 0, 0.2, 0.8, 3]) {
    const [start, end] = updateSelection([0.25, 0.75], "selection", target);
    assert.ok(start >= 0 && end <= 1);
    assert.ok(Math.abs(end - start - 0.5) < 1e-10);
  }
});

test("landscape, portrait, and square images share one diagonal without distortion", () => {
  for (const [width, height] of [[6000, 4000], [4000, 6000], [3000, 3000], [900, 1600]]) {
    const size = diagonalSize(width, height);
    assert.ok(Math.abs(Math.hypot(size.width, size.height) - 1) < 1e-10);
    assert.ok(Math.abs(size.width / size.height - width / height) < 1e-10);
  }
});
