export const photos = [
  { name: "Barleria_cristata.jpg", bytes: 1109879, width: 6016, height: 4016 },
  { name: "White_blossom.jpg", bytes: 673155, width: 3456, height: 5184 },
  { name: "Garden_after_rain.jpg", bytes: 2131599, width: 5184, height: 3456 },
  { name: "Flowers_in_shadow.jpg", bytes: 263516, width: 4272, height: 2848 },
  { name: "Monochrome_garden.jpg", bytes: 1186618, width: 3072, height: 4096 },
  { name: "A_quiet_bloom.jpg", bytes: 383716, width: 4241, height: 2828 },
  { name: "Evening_garden.jpg", bytes: 286556, width: 3000, height: 1987 },
];

// Audio names and metadata are fictional, consistent with the silent preview.
export const tracks = [
  { name: "Leaves_rustling.mp3", duration: 214, size: "8.6 MB", format: "MP3", sampleRate: "44.1 kHz", quality: "320 kbps" },
  { name: "A_walk_in_the_garden.flac", duration: 186, size: "22.4 MB", format: "FLAC", sampleRate: "48 kHz", quality: "24-bit" },
  { name: "September_rain.wav", duration: 248, size: "71.4 MB", format: "WAV", sampleRate: "48 kHz", quality: "24-bit" },
  { name: "Quiet_afternoon.mp3", duration: 197, size: "7.9 MB", format: "MP3", sampleRate: "44.1 kHz", quality: "320 kbps" },
  { name: "Between_the_trees.flac", duration: 265, size: "28.1 MB", format: "FLAC", sampleRate: "48 kHz", quality: "24-bit" },
  { name: "Last_light.mp3", duration: 231, size: "9.2 MB", format: "MP3", sampleRate: "44.1 kHz", quality: "320 kbps" },
];

export const sampleVideo = { name: "Monochaetum.mp4", bytes: 1404678, width: 1280, height: 720, fps: 30 };
export const formatSize = (bytes) => bytes >= 1000000 ? `${(bytes / 1000000).toFixed(1)} MB` : `${Math.round(bytes / 1000)} KB`;

export function formatTime(seconds) {
  const safe = Math.max(0, Math.floor(Number.isFinite(seconds) ? seconds : 0));
  return `${Math.floor(safe / 60).toString().padStart(2, "0")}:${(safe % 60).toString().padStart(2, "0")}`;
}
