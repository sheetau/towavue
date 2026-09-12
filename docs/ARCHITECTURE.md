# Architecture

Current accepted contracts, organized by subsystem. For unfinished requirements and measured evidence see [STATUS](STATUS.md); for setup see [DEVELOPMENT](DEVELOPMENT.md). Versions live in Cargo.toml/Cargo.lock and rust-toolchain.toml. Edit the relevant section when a contract changes; do not prepend checkpoint reports.

## Platform and ownership

Windows 10 22H2+ x64, Rust/MSVC. winit owns events; egui renders through the patched egui-directx11 adapter, not eframe/wgpu. FFmpeg supplies media codecs, image/png supply image decoding, and D3D11/DXGI supply presentation.

| Boundary | Owns | Must not expose |
|---|---|---|
| towavue-core | Time, commands, history, geometry, tab and folder contracts | Unsafe code, Windows/FFmpeg types |
| towavue-runtime-windows | Native windows, COM/FFI, FFmpeg, D3D11, WASAPI, Shell, workers/queues/clocks | Native frame/COM/FFmpeg pointers to app or core |
| towavue-app | Event loop, UI state, tab orchestration, command dispatch | Ownership of native decode/render handles |

Unsafe operations stay behind safe runtime interfaces with explicit lifetime, thread, and synchronization invariants. Runtime frame leases own native surfaces; app receives safe metadata/events or RGBA images.

Keep the single-D3D11-device design: hardware decode, video processing, image/UI rendering, and all host windows use the same adapter/device. D3D11VA falls back to software, not CUDA/QSV. Default audio is event-driven WASAPI Shared, not Exclusive.

## Playback, timing, and recovery

- Video and audio use independent demux/decode feeds and bounded queues. Audio output waits must not block video initialization or paused Seek. Do not drop packets or normal audio samples to relieve backpressure.
- Use the selected FFmpeg best streams consistently for playback, preview, waveform, and export.
- Media time is integer nanoseconds relative to the input origin. Audio clock is master while audio runs; monotonic anchors handle priming, video-only playback, and drained audio. Drop only eligible late decoded video frames at presentation; retain terminal video during a longer audio tail.
- Seek invalidates generation, clears queues, seeks/flushes, decodes preroll, and primes at the target. Check the selected key packet's PTS and back up a GOP if necessary; TS retains its packet/byte-position path. Do not replace exact frame selection with approximate keyframe display.
- Frame stepping finds distinct actual PTS before/after the displayed frame through a cancellable software probe, not nominal FPS arithmetic. App queues at most 32 directions and rejects stale tab/session/generation/request results.
- Results are scoped to owner and generation. Cancellation is cooperative at I/O/decode/conversion boundaries, not a promise to interrupt arbitrary FFmpeg or OS calls immediately.
- Volume and rate apply to playback and export. Rate changes preserve pitch but may re-prime; they are not guaranteed gapless.
- Device removal pauses affected sessions and rebuilds all host surfaces on one new device before restoration. Failed recovery retains edits and stopped positions; Retry must not command dead workers. Endpoint recovery preserves the same ownership discipline.
- HDR/10-bit presentation must follow an explicit validated color path; do not treat unverified passthrough or ignored metadata as HDR support.

## Windows, tabs, and input

- The host shares graphics across windows. Normal launches from the same SID/session/executable forward path-only requests with acknowledgment; failure must not silently create a duplicate host. No live-state transfer between independent old processes.
- Tabs own history, export target/options, image/view/reading state, transport, focus role, and scroll positions. Background audio continues. Hidden video limits decode work and retains a frame without pinning an entire decoder surface pool.
- Moving a tab transfers its live state and shared image data only after the destination is ready; failure leaves the source intact. Filmstrip tear-off opens the original file independently, without copying edits. Hit testing respects actual window occlusion.
- Native DWM caption controls coexist with an input-transparent child render surface. Preserve native hit testing, resizing, system controls, DPI-aware client geometry, monitor work-area placement, grab offsets, and fullscreen restoration.
- Menu, palette, grid, keyboard, pointer, and UIA dispatch shared commands/context rules. Text/IME and explicit custom bindings take precedence over implicit aliases. Do not bypass modal or editing-context guards.
- Gestures have one owner and commit once on release; Escape, focus loss, stale identity, or incompatible state cancels and restores pre-drag state. Preserve event-time pointer/modifier order, including multiple events in one frame.
- Non-active wheel input is processed only when the OS delivers it and the local hit/context permits it; it must not steal focus. Selection and right-drag editing still require focus.
- Native file dialogs and ordinary unsaved confirmations use owned STA work. Unsaved confirmation defaults to Cancel. Stateful export progress, long errors, and editing forms stay in egui.
- Heavy work, metadata/stat calls, Shell calls, dialogs, and export must not block the UI thread. Redraw on input, results, animation, or explicit deadlines, not unconditional idle polling.

## Images, reading, and previews

- Decode static images and animations to safe RGBA frames with delays. EXIF orientation is applied. AVIF uses the existing FFmpeg path; animated PNG uses the runtime compositor.
- APNG snapshots the affected region immediately before PREVIOUS blending, publishes the displayed canvas, then restores it. First animation PREVIOUS acts as BACKGROUND. An independent default poster is not an animation frame. OVER uses the image crate's pixel operation.
- APNG expands low-bit/palette colors and strips 16-bit to 8-bit. Retained RGBA has a caller budget; canvas + raw frame + PREVIOUS region have a 512 MiB working limit. Neither is a whole-process memory cap.
- Foreground loading is latest-only. A separate prefetch worker can hand an in-progress decode to foreground rather than decoding twice. Normal prefetch uses up to nine neighbors; reading prefetch covers the next spread within budget.
- Shared decoded-image cache is bounded to 10 images/256 MiB; display texture cache to 8/256 MiB. Open tabs may pin additional data. Managed texture transfers share pixel allocations and respect row stride.
- First-frame previews are borrowed during decoding; optional JPEG/BMP fast paths and cached thumbnails reduce initial waiting. Previews preserve original dimensions/orientation and never become edit, copy, or export sources.
- For normal one-image navigation, retain the previous full image until the next is ready and presented; queue at most 256 direction commands. During the handoff, disable editing/export/image or path copy. Direct destinations, failures, and incompatible state cancel queued directions.
- Last-image-tab close clears original-image caches even when audio/video tabs remain. All-window-empty GPU Trim occurs once after an empty presentation and one idle second; suppress/cancel it during new media or other work. Do not recreate/flush the device or trim while any media remains.
- Host thumbnail cache: 64 entries/16 MiB RGBA, dimensions up to 240×160; duration-success cache: 64 entries; disk preview cache: 64 MiB. Deduplicate requests and reject stale results. Video previews share bounded 16-frame sheets, with single-frame fallback while preparing.
- Image zoom is cursor-anchored and reflects raw wheel events immediately; actual size uses physical pixels. Pan only overflowing axes. Selection has corner/edge resize, Shift ratio retention, in-bounds movement, and outside-click cancellation. Clicking inside zooms to the selection; the removed crop-preview mode must not return.
- Reading is display-only: 2–10 joined pages, independent first-spread count, axis and order reversal, non-overlapping navigation. Dirty images cannot enter reading; editing/Undo/Redo is disabled there.
- Clipboard copies edited full-resolution pixels or the selection, the current animation frame, or the current source page in reading. Text focus keeps normal text-copy behavior.

## Editing and export

- Source files remain unchanged. Each tab has ordered non-destructive operations and Undo/Redo. Preview and export share operation geometry, but interpolation/compression can prevent byte identity.
- Dirty comparison uses the saved snapshot, not operation count alone: normalize effective settings, compare exact duration-aware timeline intervals, and compare all rendered image pixels when needed. Reuse materialized display pixels; compare on the existing worker, reject stale evidence, and remain dirty while pending. Undo/Redo history is retained.
- Raster operations apply in order. Crop uses integer pixels (image 1px, video even pixels); the current H.264 path requires at least 16×16 video. Respect SAR/orientation and validate source geometry before applying stored operations.
- Resize exposes Nearest, Bilinear, Bicubic, and Lanczos. Arbitrary rotation uses 0.1-degree geometry and a bounded containing canvas; image alpha interpolation is premultiplied. Video GPU raster stays on the shared device with bounded intermediates, while export is separately encoded. Do not promise GPU/export byte identity or full HDR fidelity.
- Single-source EditTimeline uses half-open intervals and exact integer mapping for Delete/Keep, gain, stretch, playback, waveform placement, and export. Empty timelines are Undoable but cannot be exported. Waveform rearrangement is an overview, not exact post-tempo PCM.
- Video visual editing is enabled only while the timeline is visible. Audio timeline remains visible even in fullscreen. Selection playback is temporary, stops at the selected end, and suppresses normal repeat/auto-next.
- The CTI triangle owns Seek dragging; its line does not. Time selection supports endpoint resize, gain dragging, and Alt stretch. Time selection uses white 20% difference fill and two 1px dotted boundaries; image selection retains its separate inversion outline.
- Exports run cancellably into owned staging files. Verify source identity, requested metadata, output validity, and cancellation before publication; failed/cancelled work must preserve an existing target. History and saved baseline change only for the matching successful request.
- Audio-only export, peak normalization, and mono/stereo options are explicit per-source output settings. Metadata uses Keep/Set/Remove; UI reads existing values asynchronously and rejects stale snapshots.
- PNG/APNG retains ten supported text fields; JPEG and static WebP retain nine supported XMP fields. Preserve supported language/author structures and existing noncanonical date/track values under Keep; Set validates typed values. Unknown XMP, EXIF/IPTC/COM/ICC synchronization and cross-format transfer are not implied.
- PNG metadata scanning validates CRC, bounded text (128 chunks / 1 MiB stored or expanded), and animation controls. Supported APNG export retains 1–65536 frames, exact delay fractions and loop count, including separate posters.
- APNG export assembles independently encoded RGBA8 PNGs into full-canvas SOURCE/NONE frames without recompressing their IDAT data. Runtime first streams the optional poster + one animation cycle into a staged PNG sequence, then one FFmpeg process applies the edits. Sharing the display compositor avoids divergent OVER rounding and FFmpeg's partial-first-frame demuxer limitation. Poster IDAT precedes animation fcTL/fdAT and is excluded from frame count.
- APNG assembly checks CRC, IHDR agreement, frame counts and controls before metadata readback/publication. Intermediate disk/encoding cost can exceed delta-compressed input, including posterless animations. Full 16-bit/color retention and other animation formats remain separate limitations.

## Shell ordering and asynchronous services

Use live Explorer view order first, preferring a foreground matching window; otherwise resolve persisted Shell view/default template through IExplorerBrowser with EBO_NOPERSISTVIEWSTATE. Read public IFolderView2 sort metadata and enumeration; never parse registry Bags or substitute name sorting silently.

A shared immutable FolderSnapshot feeds filmstrip, playlist, and navigation. Filter supported media after Shell enumeration. Only on Shell failure use Windows natural-name fallback and label it. Empty-folder Open must not replace current media/history/navigation.

Shell work stays on a dedicated STA with message-aware waiting (MsgWaitForMultipleObjectsEx), not a condition variable that starves COM windows. Use generation/path checks, latest-request mailboxes, and debounced directory notifications. Refresh order on media load and filmstrip opening; reconcile by item identity/path, not old index.

## UI conventions

Keep a media-first grayscale UI: Figtree, proportional Japanese fallback, tabular numeric glyphs, Codicon, #2C2C2C hover backgrounds, centered shadow offset, and stable left-aligned status values. Tabs reserve 3 logical px top/bottom and the native 1 physical px boundary.

Media notices go in the left status area; image information stays right. Fullscreen uses the bottom bar. Save progress occupies the toolbar boundary, not ordinary image-navigation loading. Volume uses a 1.2s thin HUD without replacing the path notice.

Media previews are immediate, noninteractive and clipped to valid hover ownership; help tooltips keep their delay and close on clipped/covered/changed targets. The logo's directional menus and normal click/keyboard entry share context/dispatch. Exact widget values live with UI code and regression tests.

## Distribution boundary

The application is MIT OR Apache-2.0; dependencies retain their licenses. The desired eventual distribution is an assisted per-user Setup.exe with selectable destination, not a standalone exe or Electron migration. Publication remains separately scoped.

Native/source/license and install/update/recovery contracts are maintained in the [packaging references](DEVELOPMENT.md#packaging-references--only-when-needed), not duplicated here. Never infer LGPL suitability from FFmpeg's label alone, silently replace user DLLs, execute an old uninstaller during update, or recursively delete an install tree. Preserve explicit ownership, inventories, journals, rollback, and user media/settings.
