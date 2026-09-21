export const photos = [
  "Barleria_cristata.jpg",
  "White_blossom.jpg",
  "Garden_after_rain.jpg",
  "Flowers_in_shadow.jpg",
  "Monochrome_garden.jpg",
  "A_quiet_bloom.jpg",
  "Evening_garden.jpg",
];

export const tracks = [
  { name: "Leaves_rustling.mp3", duration: 214 },
  { name: "A_walk_in_the_garden.flac", duration: 186 },
  { name: "September_rain.wav", duration: 248 },
  { name: "Quiet_afternoon.mp3", duration: 197 },
  { name: "Between_the_trees.flac", duration: 265 },
  { name: "Last_light.mp3", duration: 231 },
];

export function formatTime(seconds) {
  const safe = Math.max(0, Math.floor(Number.isFinite(seconds) ? seconds : 0));
  return `${Math.floor(safe / 60).toString().padStart(2, "0")}:${(safe % 60).toString().padStart(2, "0")}`;
}
