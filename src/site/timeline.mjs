export const MIN_SELECTION = 0.06;
export const clamp = (value, min = 0, max = 1) => Math.min(max, Math.max(min, value));

// Values are normalized so pointer and keyboard input share the same bounds.
export function updateSelection(selection, mode, value) {
  const [start, end] = selection;
  if (mode === "start") return [clamp(value, 0, end - MIN_SELECTION), end];
  if (mode === "end") return [start, clamp(value, start + MIN_SELECTION, 1)];
  const next = clamp(value, 0, 1 - (end - start));
  return [next, next + end - start];
}

export function diagonalSize(width, height) {
  const diagonal = Math.hypot(width, height);
  return { width: width / diagonal, height: height / diagonal };
}
