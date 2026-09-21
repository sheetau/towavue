import test from "node:test";
import assert from "node:assert/strict";
import { advanceAudio, createAudioState, toggleShuffle } from "../src/site/audio-playback.mjs";

const tracks = [{ duration: 10 }, { duration: 20 }, { duration: 30 }];
const initial = () => ({ ...createAudioState(tracks.length), playing: true });

test("paused audio stays still; playback carries elapsed time into the next track", () => {
  const paused = createAudioState(3);
  assert.equal(advanceAudio(paused, 15, tracks), paused);
  assert.deepEqual(advanceAudio(initial(), 12, tracks), { ...initial(), track: 1, time: 2 });
  assert.deepEqual(advanceAudio(initial(), 60, tracks), { ...initial(), track: 2, time: 30, playing: false });
});
test("repeat all wraps the queue and repeat one retains the selected track", () => {
  assert.deepEqual(advanceAudio({ ...initial(), repeat: "all" }, 62, tracks), { ...initial(), repeat: "all", time: 2 });
  assert.deepEqual(advanceAudio({ ...initial(), track: 1, repeat: "one" }, 42, tracks), { ...initial(), track: 1, repeat: "one", time: 2 });
});
test("shuffle preserves the playing track, visits each other track once, then stops", () => {
  const state = { ...initial(), track: 1, time: 4 };
  const shuffled = toggleShuffle(state, () => 0);
  assert.deepEqual(shuffled.order, [1, 2, 0]);
  assert.equal(shuffled.track, state.track);
  assert.equal(shuffled.time, state.time);
  assert.deepEqual(advanceAudio(shuffled, 17, tracks), { ...shuffled, track: 2, time: 1 });
  assert.deepEqual(advanceAudio(shuffled, 56, tracks), { ...shuffled, track: 0, time: 10, playing: false });
  assert.deepEqual(toggleShuffle(shuffled), state);
});
