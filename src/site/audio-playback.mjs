export function createAudioState(count) {
  return { track: 0, time: 0, playing: false, repeat: "off", shuffle: false, order: Array.from({ length: count }, (_, index) => index) };
}

export function toggleShuffle(state, random = Math.random) {
  if (state.shuffle) return { ...state, shuffle: false, order: state.order.map((_, index) => index) };
  const order = state.order.filter((index) => index !== state.track);
  for (let index = order.length - 1; index > 0; index--) {
    const other = Math.floor(random() * (index + 1));
    [order[index], order[other]] = [order[other], order[index]];
  }
  return { ...state, shuffle: true, order: [state.track, ...order] };
}

// The audio demo advances a clock and playlist, without an audio source.
export function advanceAudio(state, elapsed, tracks) {
  if (!state.playing || !Number.isFinite(elapsed) || elapsed <= 0) return state;
  let track = state.track;
  let time = state.time + elapsed;
  if (state.repeat === "one") return { ...state, time: time % tracks[track].duration };
  while (time >= tracks[track].duration) {
    const next = state.order.indexOf(track) + 1;
    if (next === state.order.length && state.repeat === "off") return { ...state, track, time: tracks[track].duration, playing: false };
    time -= tracks[track].duration;
    track = state.order[next % state.order.length];
  }
  return { ...state, track, time };
}
