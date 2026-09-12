# towavue

A lightweight image viewer, video player, and audio player for Windows — with a minimal interface and non-destructive editing.

## Features

- **One app for your media.** Browse images, watch videos, and play music in tabs.
- **Familiar folder browsing.** Navigate in Windows Explorer's folder order, with a thumbnail filmstrip and recent files.
- **Flexible viewing.** Zoom, pan, fit or fill the window, and switch to fullscreen.
- **Reading mode.** View multiple images as connected pages, with adjustable page count, layout, and direction.
- **Image editing.** Select, crop, rotate, flip, resize, and copy an image or selection to the clipboard.
- **Video and audio editing.** Select time ranges, remove or keep sections, and adjust volume and playback speed.
- **Undo and export.** Keep the source unchanged while editing, then export the result.
- **Keyboard-friendly controls.** Use menus, the command palette, or customizable shortcuts.
- **Multiple windows.** Move tabs between windows while retaining their edits and playback state.

## Media support

Images include PNG, APNG, JPEG, GIF, WebP, BMP, TIFF, and AVIF. Video and audio support uses FFmpeg, including common MP4, MKV, WebM, MP3, FLAC, WAV, and Opus files. Actual support depends on the file's codec and profile.

Viewing support does not imply lossless export or animation-preserving export for every format. Supported APNG, GIF, and WebP saves retain frames, timing, and loops; APNG also retains separate poster images. Animated WebP saves use lossless full-frame encoding, which can increase file size. GIF encoding can change colors and transparency through palette conversion. Other animated-image export formats remain limited.

AVIF viewing and saving support transparency, embedded display cropping, quarter-turn orientation, and mirroring in still images and animations. Static AVIF saves preserve the edited 8-bit RGBA pixels; supported sequences, including single-frame sequences, also retain timing and loops. Animation conversion between formats remains limited.

## Getting started

towavue targets Windows 10 22H2 or later, x64. Open a file or folder from the welcome screen, drag media from Explorer, or pass a path to the application:

```text
towavue.exe "path/to/media.mp4"
```

Public installer distribution is not yet available. To run from source, see the [development guide](docs/DEVELOPMENT.md).

## Shortcuts

| Action | Shortcut |
|---|---|
| Open file / folder | Ctrl+O / Ctrl+Shift+O |
| Command palette | Ctrl+Shift+P |
| Switch tabs | Ctrl+Tab / Ctrl+Shift+Tab |
| Close / reopen tab | Ctrl+W / Ctrl+Shift+T |
| Fullscreen | F11 |
| Filmstrip | F |
| Previous / next image | Left / Right |
| Image zoom | Ctrl+wheel |
| Reading mode | B |
| Play / pause video or audio | Space |
| Seek backward / forward | Left / Right |
| Undo / redo | Ctrl+Z / Ctrl+Shift+Z |
| Export as / save to the last export target | Ctrl+Shift+S / Ctrl+S |

Available actions depend on the current media and editing mode. Menus show the active shortcuts. Keyboard bindings are stored in `%APPDATA%\towavue\shortcuts.conf`.

## License

towavue is available under either the [MIT License](LICENSE-MIT) or [Apache License 2.0](LICENSE-APACHE), at your option. FFmpeg and other third-party components retain their own licenses; see [third-party notices](third-party/README.md).
