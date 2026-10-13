import ja from "./ja.json";

export const languages = { en: "English", ja: "日本語" };

const en = {
  title: "towavue | Ahead of every frame.",
  description:
    "Images, videos, and audio in one fast, beautifully minimal Windows workspace. Browse, play, and edit with towavue. Free and open source.",
  ogLocale: "en_US",
  shareImageAlt: "towavue product preview with its logo and a white magnolia in the image viewer",
  skip: "Skip to content",
  home: "towavue home",
  download: "Download",
  downloadWindows: "Download for Windows",
  downloading: "Finding download…",
  downloadStarted: "Your download is starting.",
  downloadError: "Couldn’t get the installer. Please try again or visit Releases.",
  releases: "Releases",
  license: "License",
  language: "Language",
  hero: "Ahead of every frame.",
  heroDescription:
    "Images, videos, and audio in one beautifully minimal workspace built in Rust, this is the last media viewer you’ll ever need.",
  closingTitle: "See what fast feels like.",
  demo: {
    label: "towavue gallery preview with 20 archive images",
    gallery: "Gallery",
    date: "October 13, 2026",
  },
  benchmark: {
    label: "01 — BENCHMARK",
    title: "Windows’ fastest media viewer.",
    description:
      "Built for maximum viewing speed. GPU rendering powers the interface and media display. Supported hardware-decoded video stays on the GPU through presentation, avoiding frame copies back to the CPU.",
    scale: "Completion-time index · towavue = 100 · Lower is better • Windows 11 • October 9, 2026",
    method:
      "Each pair compares towavue with the named app across shared workloads: 3 image operations for IrfanView; 8 video and audio operations for MPC-HC and VLC. The index is the equal-weight geometric mean of per-operation time ratios, using fixture-median benchmark scores. Faster percentages = (competitor index / 100 − 1) × 100. Development builds; results vary by file and PC.",
    link: "See full benchmark",
  },
  workspace: {
    label: "02 — WORKSPACE",
    title: "One workspace for all your media.",
    description: "Transition between images, video, and audio inside a single cohesive window.",
    items: [
      {
        title: "01 / IMAGES",
        description: "Look closer. Zoom, rotate and browse stills or animations without breaking your flow.",
        alt: "A white magnolia in towavue’s image viewer",
      },
      {
        title: "02 / VIDEO",
        description: "Scrub to the moment you need, step through frames, and pick up playback where you left off.",
        alt: "A flowering branch in towavue’s video viewer",
      },
      {
        title: "03 / AUDIO",
        description: "Open a track and the audio files in its folder become your playlist.",
        alt: "A folder of tracks in towavue’s audio playlist",
      },
    ],
  },
  editing: {
    label: "03 — EDITING",
    title: "Even the editing gets done.",
    description:
      "That crop, that short clip, that one frame worth keeping. Make the change where you already are, with no detour into another app.",
    items: [
      {
        title: "A full set of everyday edits",
        description: "Crop, rotate and resize images. Trim video and audio, adjust speed and gain, or save a frame.",
      },
      {
        title: "Room to change your mind",
        description: "Work non-destructively, with undo available even after saving.",
      },
      {
        title: "At your fingertips",
        description:
          "With customizable shortcuts or the Command Palette, your work gets done without even reaching for the mouse.",
      },
    ],
  },
  discovery: {
    label: "04 — DISCOVERY",
    title: "A place for every next step.",
    description:
      "Follow Explorer’s folder order, scan the thumbnail filmstrip or return to a recent file from the gallery.",
    items: [
      {
        title: "Tabs that travel with you",
        description: "Move tabs between windows with edits and playback position intact.",
      },
      {
        title: "Settle into the page",
        description: "Choose a reading layout and direction, connect images into pages and go fullscreen.",
      },
      {
        title: "Folders within reach",
        description: "Move up, down, or to folders at the same level freely with the breadcrumbs or shortcuts.",
      },
    ],
  },
  timeline: {
    position: "Playback position",
    start: "Selection start",
    end: "Selection end",
    selection: "Move selection",
    instructions:
      "Click or drag to seek. Drag either dotted edge to resize the selection. Right-drag inside the selection to move it, or drag it with touch. Focus a control and use the arrow keys to adjust it; Shift makes larger steps. Home and End move to the limits.",
  },
  filmstrip: {
    label: "Filmstrip preview. Hover, tap, or use the arrow keys to explore 15 archive images.",
  },
};

export const locales = { en, ja };
