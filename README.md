# towavue

A lightweight image viewer, video player, and audio player for Windows — with a minimal interface and non-destructive editing.

## Features

- **One app for your media.** Browse images, watch videos, and play music in tabs.
- **Familiar folder browsing.** Navigate in Windows Explorer's folder order, with a thumbnail filmstrip and recent files.
- **Flexible viewing.** Zoom, pan, fit or fill the window, and switch to fullscreen.
- **Reading mode.** View multiple images as connected pages, with adjustable page count, layout, and direction.
- **Image editing.** Select, crop, rotate, flip, resize, and copy an image or selection to the clipboard. Paste a clipboard image into a new Untitled tab, edit it, and save it as a file.
- **Video and audio editing.** Select time ranges, remove or keep sections, and adjust gain and playback speed. Listening volume and mute do not change exported audio.
- **Undo, save, and export.** Edit without changing the source, then save it or use Save as to continue with another file. Undo remains available after saving while the tab stays open, and keeps the saved destination as the current file. Deleting an open file keeps its media editable until you leave or close the document; Save can recreate it at the original location.
- **Video frame images.** Export the current edited frame to PNG, without window zoom or changing the video's save state.
- **Keyboard-friendly controls.** Use menus, the command palette, or customizable shortcuts.
- **Multiple windows.** Move tabs between windows while retaining their edits and playback state.

## Media support

Images include PNG, APNG, JPEG, GIF, WebP, BMP, TIFF, and AVIF. Video and audio support uses FFmpeg, including common MP4, MKV, WebM, MP3, FLAC, WAV, and Opus files. Actual support depends on the file's codec and profile.

Viewing support does not imply lossless export or animation-preserving export for every format. Supported APNG, GIF, WebP, and AVIF animations can be saved or converted between these formats within the destination's exact timing and loop limits. Separate APNG poster images require APNG output (.png or .apng). Animated WebP saves use lossless full-frame encoding, which can increase file size. GIF palette conversion can change colors and transparency. Other animated-image export formats remain limited.

AVIF viewing and saving support transparency, embedded display cropping, quarter-turn orientation, and mirroring in still images and animations. Static AVIF saves preserve the edited 8-bit RGBA pixels; supported sequences, including single-frame sequences, also retain timing and loops. Animation conversion to AVIF preserves edited 8-bit RGBA but does not support zero-delay frames.

## Getting started

towavue supports **Windows 11 x64**. Published builds are listed in [GitHub Releases](https://github.com/sheetau/towavue/releases). Download the version's `windows-x64-setup.exe` and run it to install for the current user. Setup includes the Microsoft Visual C++ prerequisite and asks you to review its terms if installation is needed. The initial EXE and Setup are unsigned, so Windows may show an unknown-publisher warning. Windows 10 and ARM64 are not supported.

Open a file or folder from the welcome screen, drag media from Explorer, or pass a path to the application:

```text
towavue.exe "path/to/media.mp4"
```

Installed builds check for updates automatically. Use **Help > Check for updates** for a manual check, then choose **Install now** or **Install on next launch** after a verified download. Unsaved edits still receive Save / Discard / Cancel prompts. Close the app before running Setup manually. Update metadata is independently signed even though the initial EXE and Setup have no Windows code signature.

Uninstall through Windows Settings > Apps > Installed apps. Your media and settings are preserved, and the shared Microsoft Visual C++ runtime is not removed. **Help > About** shows the installed version; **Help > Show licenses and sources** opens the installed notices and the matching source download link. Each release includes a separate `sources.zip`; extract it and open `START-HERE.html` for the application/native sources, patches and original notices.

To build from source, see the [development guide](docs/DEVELOPMENT.md). Known limitations are recorded in each release's notes.

## Shortcuts

| Action | Shortcut |
|---|---|
| Open file / folder | Ctrl+O / Ctrl+Shift+O |
| Command palette | Ctrl+Shift+P |
| Keyboard Shortcuts settings | Ctrl+K Ctrl+S |
| Switch tabs | Ctrl+Tab / Ctrl+Shift+Tab |
| Close / reopen tab | Ctrl+W / Ctrl+Shift+T |
| Fullscreen | F11 / Enter |
| Filmstrip | F |
| Image navigation | Left / Right; follows reading direction in reading mode |
| Image zoom | Ctrl+wheel |
| Reading mode | B |
| Reverse reading direction | H / V (reading mode) |
| Reload Explorer folder order | F5 |
| Play / pause video or audio | Space |
| Seek backward / forward | Left / Right |
| Undo / redo | Ctrl+Z / Ctrl+Shift+Z |
| Save as / save current file | Ctrl+Shift+S / Ctrl+S |
| Paste image into a new tab | Ctrl+V |

Available actions depend on the current media and editing mode. Enter retains its usual confirmation or activation behavior in text fields, menus, dialogs, and focused buttons. Menus show the active shortcuts. Open the Keyboard Shortcuts tab to search, record, edit, remove or reset bindings. Saves apply to all windows in the current host. Keyboard bindings are stored in `%APPDATA%\towavue\shortcuts.conf`; manual file changes use **Reload keyboard shortcuts** from the menu or command palette. Grid layout customization remains in `grid.conf`.

## License

towavue is available under either the [MIT License](LICENSE-MIT) or [Apache License 2.0](LICENSE-APACHE), at your option. FFmpeg and other third-party components retain their own licenses; see [third-party notices](third-party/README.md).
