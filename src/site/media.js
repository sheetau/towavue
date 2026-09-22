import manifest from "./media-manifest.json";

export const photos = manifest.photos;

// Audio names and metadata are fictional, consistent with the silent preview.
export const tracks = [
  { name: "Leaves_rustling.mp3", duration: 214, size: "8.6 MB", format: "MP3", sampleRate: "44.1 kHz", quality: "320 kbps" },
  { name: "A_walk_in_the_garden.flac", duration: 186, size: "22.4 MB", format: "FLAC", sampleRate: "48 kHz", quality: "24-bit" },
  { name: "September_rain.wav", duration: 248, size: "71.4 MB", format: "WAV", sampleRate: "48 kHz", quality: "24-bit" },
  { name: "Quiet_afternoon.mp3", duration: 197, size: "7.9 MB", format: "MP3", sampleRate: "44.1 kHz", quality: "320 kbps" },
  { name: "Between_the_trees.flac", duration: 265, size: "28.1 MB", format: "FLAC", sampleRate: "48 kHz", quality: "24-bit" },
  { name: "Last_light.mp3", duration: 231, size: "9.2 MB", format: "MP3", sampleRate: "44.1 kHz", quality: "320 kbps" },
];

export const sampleVideo = manifest.sampleVideo;
export const formatSize = (bytes) => bytes >= 1000000 ? `${(bytes / 1000000).toFixed(1)} MB` : `${Math.round(bytes / 1000)} KB`;

export function formatTime(seconds) {
  const safe = Math.max(0, Math.floor(Number.isFinite(seconds) ? seconds : 0));
  return `${Math.floor(safe / 60).toString().padStart(2, "0")}:${(safe % 60).toString().padStart(2, "0")}`;
}
