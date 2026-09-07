//! Application entry point for towavue.

#![forbid(unsafe_code)]

mod chrome;
mod cursor;
mod filmstrip;
mod fonts;
mod grid;
mod menu;
mod palette;
mod playlist;
mod seekbar;
mod selection;
mod shortcuts;
mod timeline_input;
mod trim;
mod welcome;
mod wheel_input;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Align2, Color32, RichText, TextureHandle, TextureOptions};
use egui_winit::accesskit_winit;
use towavue_core::{
    CommandContext, CommandId, EditHistory, EditOperation, FolderSnapshot, FolderSnapshotSource,
    ImageViewState, Key, KeyStroke, MediaKind, MediaTime, Modifiers, PixelCrop, PlaybackGeneration,
    PlaybackState, ReadingAxis, ReadingSettings, ShortcutBindings, ShortcutMatch, TabId, TabSet,
    TabTarget, UnitPoint, UnitRect, ZoomMode, command_definitions,
};
use towavue_runtime_windows::{
    AudioOutputEvent, DecodedImage, DialogError, ExportError, ExportEvent, ExportJob,
    ExportRequest, FileDialogKind, FolderOrderProvider, FolderWatcher, FrameRenderer, ImageLoader,
    LatestTask, PlaybackEvent, PlaybackSession, PreviewCache, PromptButtons, PromptResponse,
    RenderError, canonical_shell_path, configure_mouse_input, cursor_position_in_window, pick_path,
    show_prompt,
};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey, PhysicalKey};
use winit::window::{Fullscreen, Window, WindowId};

const AUDIO_EVENT_POLL_INTERVAL: Duration = Duration::from_millis(20);
const FOLDER_EVENT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const PREFIX_TIMEOUT: Duration = Duration::from_secs(1);
const STATUS_MESSAGE_DURATION: Duration = Duration::from_secs(4);
const VIDEO_EARLY_TOLERANCE: Duration = Duration::from_millis(5);
const KEYBOARD_SEEK_STEP: Duration = Duration::from_secs(5);
const VIDEO_LATE_TOLERANCE: Duration = Duration::from_millis(40);

fn main() -> Result<(), Box<dyn Error>> {
    let initial_path = parse_initial_path()?;
    let mut event_loop = EventLoop::<AppEvent>::with_user_event();
    configure_mouse_input(&mut event_loop);
    let event_loop = event_loop.build()?;
    let proxy = event_loop.create_proxy();
    let accessibility_proxy = proxy.clone();
    let mut application = Application::new(initial_path, move |event| {
        let _ = proxy.send_event(event);
    })?;
    application.event_loop_proxy = Some(accessibility_proxy);
    event_loop.run_app(&mut application)?;
    Ok(())
}

fn parse_initial_path() -> Result<Option<PathBuf>, Box<dyn Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let Some(path) = arguments.next() else {
        return Ok(None);
    };
    if arguments.next().is_some() {
        return Err("towavue accepts at most one media file or folder".into());
    }
    let path = PathBuf::from(path);
    if !path.exists() {
        return Err(format!("path does not exist: {}", path.display()).into());
    }
    Ok(Some(path))
}

struct PlaybackClock {
    media_anchor: MediaTime,
    wall_anchor: Instant,
    paused_at: Option<Instant>,
    rate: f64,
}

impl PlaybackClock {
    fn new(media_anchor: MediaTime, rate: f32) -> Self {
        Self {
            media_anchor,
            wall_anchor: Instant::now(),
            paused_at: None,
            rate: f64::from(rate),
        }
    }

    fn set_paused(&mut self, paused: bool) {
        match (paused, self.paused_at) {
            (true, None) => self.paused_at = Some(Instant::now()),
            (false, Some(paused_at)) => {
                self.wall_anchor += Instant::now().saturating_duration_since(paused_at);
                self.paused_at = None;
            }
            _ => {}
        }
    }

    fn due_at(&self, media_time: MediaTime) -> Instant {
        let delta = media_time
            .as_nanoseconds()
            .saturating_sub(self.media_anchor.as_nanoseconds());
        self.wall_anchor + Duration::from_nanos(delta.max(0) as u64).div_f64(self.rate)
    }

    fn position(&self) -> MediaTime {
        let now = self.paused_at.unwrap_or_else(Instant::now);
        self.media_anchor.saturating_add(
            now.saturating_duration_since(self.wall_anchor)
                .mul_f64(self.rate),
        )
    }
}

#[derive(Clone, PartialEq)]
enum UiAction {
    Minimize,
    Maximize,
    CloseWindow,
    DragWindow,
    ResizeWindow(winit::window::ResizeDirection),
    Command(CommandId),
    ActivateTab(TabId),
    ReorderTab(TabId, usize),
    TrimEndpoint(TabId, EditOperation),
    Volume(TabId, f32),
    CloseTab(TabId),
    DetachTab(TabId),
    OpenMedia(PathBuf, bool),
    Seek(MediaTime),
    ResolveGuard(GuardDecision),
    CancelExport,
    DismissExportError,
}

enum AppEvent {
    Accessibility(accesskit_winit::Event),
    ImagesReady,
    FolderReady,
    FilmstripReady,
    DialogFinished(Result<Option<PathBuf>, DialogError>),
    PromptFinished(Result<PromptResponse, DialogError>),
    Export(ExportEvent),
    Playback(u64, PlaybackEvent),
    Duration(PathBuf, u64, Result<Duration, String>),
    Waveform(
        PathBuf,
        u64,
        Result<towavue_runtime_windows::PreviewImage, String>,
    ),
    Thumbnail(
        PathBuf,
        u64,
        u64,
        Result<towavue_runtime_windows::PreviewImage, String>,
    ),
}

impl From<accesskit_winit::Event> for AppEvent {
    fn from(event: accesskit_winit::Event) -> Self {
        Self::Accessibility(event)
    }
}

fn handle_accesskit_window_event(
    state: &mut egui_winit::State,
    event: accesskit_winit::WindowEvent,
) {
    match event {
        accesskit_winit::WindowEvent::InitialTreeRequested => state.egui_ctx().enable_accesskit(),
        accesskit_winit::WindowEvent::ActionRequested(request) => {
            state.on_accesskit_action_request(request);
        }
        accesskit_winit::WindowEvent::AccessibilityDeactivated => {
            state.egui_ctx().disable_accesskit()
        }
    }
}

fn keep_accessibility_focus_live(context: &egui::Context, output: &mut egui::PlatformOutput) {
    let Some(update) = &mut output.accesskit_update else {
        return;
    };
    let Some(tree) = &update.tree else { return };
    // The pinned egui emits complete root trees, not incremental node lists.
    if update.tree_id == egui::accesskit::TreeId::ROOT
        && update.focus != tree.root
        && !update.nodes.iter().any(|(id, _)| *id == update.focus)
    {
        context.memory_mut(|memory| {
            if let Some(id) = memory
                .focused()
                .filter(|id| id.accesskit_id() == update.focus)
            {
                memory.surrender_focus(id);
            }
        });
        update.focus = tree.root;
    }
}

enum FolderIntent {
    Open,
    Refresh(PathBuf),
}

enum DialogIntent {
    OpenFile,
    OpenFolder,
    Export {
        tab: TabId,
        source: PathBuf,
        kind: MediaKind,
        continuation: Option<GuardedAction>,
    },
}

#[derive(Clone)]
enum GuardedAction {
    CloseTab(TabId),
    DetachTab(TabId),
    Navigate(PathBuf),
    Exit,
}

#[derive(Clone, Copy, PartialEq)]
enum GuardDecision {
    Save,
    Discard,
    Cancel,
}

enum FallbackPrompt {
    Recovery {
        position: MediaTime,
        state: PlaybackState,
        error: String,
    },
    Guard,
    ExportError,
    ExportBusy,
}

struct ActiveExport {
    job: ExportJob,
    tab: TabId,
    request: ExportRequest,
    encoded: Duration,
    cancelling: bool,
    continuation: Option<GuardedAction>,
}

#[derive(Clone)]
struct ImagePresentation {
    decoded: Arc<DecodedImage>,
    texture: TextureHandle,
    frame_index: usize,
    next_frame_at: Option<Instant>,
}

struct ImageTextureCache {
    entries: VecDeque<ImagePresentation>,
    byte_limit: usize,
}

impl ImageTextureCache {
    fn new(byte_limit: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            byte_limit,
        }
    }

    fn load(
        &mut self,
        context: &egui::Context,
        path: &Path,
        decoded: Arc<DecodedImage>,
    ) -> Result<ImagePresentation, String> {
        if let Some(index) = self
            .entries
            .iter()
            .position(|image| Arc::ptr_eq(&image.decoded, &decoded))
        {
            let image = self.entries.remove(index).expect("cached image");
            self.entries.push_back(image.clone());
            return Ok(image);
        }
        let image = ImagePresentation::from_decoded(context, path, decoded)?;
        let bytes = image.decoded.retained_bytes();
        if !image.decoded.is_animated() && bytes <= self.byte_limit {
            while self.entries.len() >= 8
                || self
                    .entries
                    .iter()
                    .map(|image| image.decoded.retained_bytes())
                    .sum::<usize>()
                    + bytes
                    > self.byte_limit
            {
                self.entries.pop_front();
            }
            self.entries.push_back(image.clone());
        }
        Ok(image)
    }
}

impl ImagePresentation {
    fn from_decoded(
        context: &egui::Context,
        path: &Path,
        decoded: Arc<DecodedImage>,
    ) -> Result<Self, String> {
        let first = decoded
            .frames
            .first()
            .ok_or_else(|| "decoded image contained no frames".to_owned())?;
        let limit = context.input(|input| input.max_texture_side);
        if decoded
            .frames
            .iter()
            .any(|frame| frame.width as usize > limit || frame.height as usize > limit)
        {
            return Err(format!(
                "Image dimensions exceed this graphics device's {limit}px texture limit"
            ));
        }
        let texture = context.load_texture(
            format!("image:{}", path.display()),
            color_image(first),
            TextureOptions::LINEAR,
        );
        let next_frame_at = decoded.is_animated().then(|| Instant::now() + first.delay);
        Ok(Self {
            decoded,
            texture,
            frame_index: 0,
            next_frame_at,
        })
    }

    fn dimensions(&self) -> (u32, u32) {
        self.decoded.dimensions()
    }

    fn advance_animation(&mut self, now: Instant) -> bool {
        let Some(mut deadline) = self.next_frame_at else {
            return false;
        };
        if now < deadline {
            return false;
        }
        let previous_frame = self.frame_index;
        let previous_deadline = deadline;
        while deadline <= now {
            self.frame_index = (self.frame_index + 1) % self.decoded.frames.len();
            deadline += self.decoded.frames[self.frame_index].delay;
            if self.frame_index == previous_frame && deadline <= now {
                // Skip complete cycles without iterating over time spent away from the UI.
                let cycle = deadline - previous_deadline;
                let remainder = (now - deadline).as_nanos() % cycle.as_nanos();
                deadline = now
                    - Duration::new(
                        (remainder / 1_000_000_000) as u64,
                        (remainder % 1_000_000_000) as u32,
                    );
            }
        }
        self.next_frame_at = Some(deadline);
        if self.frame_index == previous_frame {
            return false;
        }
        self.texture.set(
            color_image(&self.decoded.frames[self.frame_index]),
            TextureOptions::LINEAR,
        );
        true
    }
}

fn color_image(frame: &towavue_runtime_windows::DecodedImageFrame) -> egui::ColorImage {
    let size = [frame.width as usize, frame.height as usize];
    assert_eq!(size[0] * size[1] * 4, frame.rgba.len());
    let mut pixels = Vec::with_capacity(size[0] * size[1]);
    for row in frame.rgba.chunks_exact(size[0].max(1) * 4) {
        let row = row.as_chunks::<4>().0;
        // Opaque rows need no alpha conversion; mixed rows keep egui's exact rounding.
        if row.iter().all(|pixel| pixel[3] == 255) {
            pixels.extend(
                row.iter()
                    .map(|p| Color32::from_rgba_premultiplied(p[0], p[1], p[2], p[3])),
            );
        } else {
            pixels.extend(
                row.iter()
                    .map(|p| Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3])),
            );
        }
    }
    egui::ColorImage::new(size, pixels)
}

#[derive(Clone, Copy)]
enum SelectionDrag {
    New(UnitPoint),
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Copy)]
enum ViewDrag {
    Selection {
        mode: SelectionDrag,
        before: Option<UnitRect>,
    },
    Pan {
        origin: egui::Pos2,
        before: (f32, f32),
    },
}

#[derive(Clone, Copy)]
struct ImageTransform {
    uv: [UnitPoint; 4],
    size: (f32, f32),
}

impl ImageTransform {
    fn new(size: (u32, u32), operations: &[EditOperation]) -> Self {
        Self::with_orientation(
            size,
            towavue_runtime_windows::VideoOrientation::default(),
            operations,
        )
    }

    fn with_orientation(
        size: (u32, u32),
        orientation: towavue_runtime_windows::VideoOrientation,
        operations: &[EditOperation],
    ) -> Self {
        let mut transform = Self {
            uv: orientation.source_uv(),
            size: if orientation.swaps_axes() {
                (size.1 as f32, size.0 as f32)
            } else {
                (size.0 as f32, size.1 as f32)
            },
        };
        for operation in operations {
            match *operation {
                EditOperation::Crop(region) => transform.crop_pixels(region),
                EditOperation::RotateClockwise => {
                    transform.uv.rotate_right(1);
                    transform.size = (transform.size.1, transform.size.0);
                }
                EditOperation::RotateCounterclockwise => {
                    transform.uv.rotate_left(1);
                    transform.size = (transform.size.1, transform.size.0);
                }
                EditOperation::FlipHorizontal => {
                    transform.uv.swap(0, 1);
                    transform.uv.swap(3, 2);
                }
                EditOperation::FlipVertical => {
                    transform.uv.swap(0, 3);
                    transform.uv.swap(1, 2);
                }
                EditOperation::SetTrimStart(_)
                | EditOperation::SetTrimEnd(_)
                | EditOperation::SetVolume(_)
                | EditOperation::SetRate(_) => {}
            }
        }
        transform
    }

    fn crop(&mut self, region: UnitRect) {
        if let Some(crop) = PixelCrop::from_selection(
            region,
            (self.size.0 as u32, self.size.1 as u32),
            MediaKind::Image,
        ) {
            self.crop_pixels(crop);
        }
    }

    fn crop_pixels(&mut self, crop: PixelCrop) {
        let region = crop.unit_rect((self.size.0 as u32, self.size.1 as u32));
        let previous = self.uv;
        self.uv = [
            bilinear_uv(previous, region.min.x, region.min.y),
            bilinear_uv(previous, region.max.x, region.min.y),
            bilinear_uv(previous, region.max.x, region.max.y),
            bilinear_uv(previous, region.min.x, region.max.y),
        ];
        self.size = (crop.width as f32, crop.height as f32);
    }

    fn pixel_aspect(&self, source_aspect: f32) -> f32 {
        if self.uv[0].x == self.uv[1].x {
            1.0 / source_aspect
        } else {
            source_aspect
        }
    }
}

struct Application<N> {
    initial_path: Option<PathBuf>,
    notify: Arc<N>,
    event_loop_proxy: Option<EventLoopProxy<AppEvent>>,
    window: Option<Arc<Window>>,
    fullscreen: bool,
    fullscreen_controls_visible: bool,
    fullscreen_controls_focus_requested: bool,
    fullscreen_controls_keyboard: bool,
    fullscreen_was_maximized: bool,
    viewing_cursor: cursor::ViewingCursor,
    pending_dialog: Option<DialogIntent>,
    renderer: Option<FrameRenderer>,
    ui_context: Option<egui::Context>,
    ui_state: Option<egui_winit::State>,
    folder_order: FolderOrderProvider,
    pending_folder: Option<(u64, FolderIntent)>,
    folder_snapshot: Option<FolderSnapshot>,
    folder_watcher: Option<(PathBuf, FolderWatcher)>,
    tabs: TabSet,
    edits: BTreeMap<TabId, EditHistory>,
    export_paths: BTreeMap<TabId, PathBuf>,
    active_export: Option<ActiveExport>,
    export_error: Option<String>,
    grid_layouts: grid::GridLayouts,
    grid_path: PathBuf,
    path: Option<PathBuf>,
    media_kind: Option<MediaKind>,
    image: Option<ImagePresentation>,
    image_texture_cache: ImageTextureCache,
    image_loader: ImageLoader,
    image_generation: u64,
    image_loading: bool,
    image_error: Option<String>,
    playback_error: Option<String>,
    image_view: ImageViewState,
    image_viewport: egui::Vec2,
    view_drag: Option<ViewDrag>,
    reading_mode: bool,
    reading_settings: ReadingSettings,
    reading_pages: Vec<Result<ImagePresentation, String>>,
    preview_cache: PreviewCache,
    duration_worker: LatestTask,
    waveform_worker: LatestTask,
    thumbnail_worker: LatestTask,
    timeline_open: bool,
    waveform: Option<TextureHandle>,
    media_duration: Option<Duration>,
    hover_thumbnail: Option<(u64, TextureHandle)>,
    media_generation: u64,
    failed_thumbnails: BTreeSet<u64>,
    waveform_loading: bool,
    thumbnail_loading: Option<u64>,
    session: Option<PlaybackSession>,
    pending_time: Option<MediaTime>,
    video_rect: Option<egui::Rect>,
    video_uv: [UnitPoint; 4],
    ui_repaint_at: Option<Instant>,
    restore_ui_textures: bool,
    queued_recovery: Option<FallbackPrompt>,
    native_prompt: Option<FallbackPrompt>,
    clock: Option<PlaybackClock>,
    state: PlaybackState,
    decode_finished: bool,
    audio_drained: bool,
    metrics_recorded: bool,
    generation: PlaybackGeneration,
    pending_seek_started: Option<Instant>,
    seek_latencies: Vec<Duration>,
    drift_samples: Vec<Duration>,
    shortcuts: ShortcutBindings,
    shortcut_path: PathBuf,
    entered_shortcut: Vec<KeyStroke>,
    prefix_started: Option<Instant>,
    modifiers: ModifiersState,
    filmstrip_open: bool,
    filmstrip_return_focus: Option<(u64, egui::Id)>,
    filmstrip: filmstrip::Filmstrip,
    playlist: playlist::Playlist,
    image_seek_preview_active: bool,
    grid_open: bool,
    palette_open: bool,
    command_overlay_return_focus: Option<egui::Id>,
    palette: palette::CommandPalette,
    status_message: Option<(String, Instant)>,
    pending_guard: Option<GuardedAction>,
    guard_return_focus: Option<(TabId, egui::Id)>,
    exit_requested: bool,
    prefer_hardware_encode: bool,
}

impl<N> Application<N>
where
    N: Fn(AppEvent) + Send + Sync + 'static,
{
    fn new(initial_path: Option<PathBuf>, notify: N) -> Result<Self, Box<dyn Error>> {
        let (shortcuts, shortcut_path) = shortcuts::load()
            .map_err(|error| format!("could not load keyboard shortcuts: {error}"))?;
        let (grid_layouts, grid_path) =
            grid::load().map_err(|error| format!("could not load grid menu: {error}"))?;
        let preview_cache = PreviewCache::local()
            .map_err(|error| format!("could not open preview cache: {error}"))?;
        let notify = Arc::new(notify);
        let image_notify = Arc::clone(&notify);
        let image_loader = ImageLoader::new(move || image_notify(AppEvent::ImagesReady))?;
        let folder_notify = Arc::clone(&notify);
        let folder_order =
            FolderOrderProvider::with_notify(move || folder_notify(AppEvent::FolderReady))?;
        let filmstrip_notify = Arc::clone(&notify);
        let filmstrip = filmstrip::Filmstrip::new(preview_cache.clone(), move || {
            filmstrip_notify(AppEvent::FilmstripReady)
        })?;
        Ok(Self {
            initial_path,
            notify,
            event_loop_proxy: None,
            window: None,
            fullscreen: false,
            fullscreen_controls_visible: false,
            fullscreen_controls_focus_requested: false,
            fullscreen_controls_keyboard: false,
            fullscreen_was_maximized: false,
            viewing_cursor: cursor::ViewingCursor::default(),
            pending_dialog: None,
            renderer: None,
            ui_context: None,
            ui_state: None,
            folder_order,
            pending_folder: None,
            folder_snapshot: None,
            folder_watcher: None,
            tabs: TabSet::default(),
            edits: BTreeMap::new(),
            export_paths: BTreeMap::new(),
            active_export: None,
            export_error: None,
            grid_layouts,
            grid_path,
            path: None,
            media_kind: None,
            image: None,
            image_texture_cache: ImageTextureCache::new(256 * 1024 * 1024),
            image_loader,
            image_generation: 0,
            image_loading: false,
            image_error: None,
            playback_error: None,
            image_view: ImageViewState::default(),
            image_viewport: egui::Vec2::ZERO,
            view_drag: None,
            reading_mode: false,
            reading_settings: ReadingSettings::default(),
            reading_pages: Vec::new(),
            preview_cache,
            duration_worker: LatestTask::new("towavue-duration")?,
            waveform_worker: LatestTask::new("towavue-waveform")?,
            thumbnail_worker: LatestTask::new("towavue-thumbnail")?,
            timeline_open: false,
            waveform: None,
            media_duration: None,
            hover_thumbnail: None,
            media_generation: 0,
            failed_thumbnails: BTreeSet::new(),
            waveform_loading: false,
            thumbnail_loading: None,
            session: None,
            pending_time: None,
            video_rect: None,
            video_uv: ImageTransform::new((1, 1), &[]).uv,
            ui_repaint_at: None,
            restore_ui_textures: false,
            queued_recovery: None,
            native_prompt: None,
            clock: None,
            state: PlaybackState::Loading,
            decode_finished: false,
            audio_drained: true,
            metrics_recorded: false,
            generation: PlaybackGeneration::INITIAL,
            pending_seek_started: None,
            seek_latencies: Vec::new(),
            drift_samples: Vec::new(),
            shortcuts,
            shortcut_path,
            entered_shortcut: Vec::new(),
            prefix_started: None,
            modifiers: ModifiersState::default(),
            filmstrip_open: false,
            filmstrip_return_focus: None,
            filmstrip,
            playlist: playlist::Playlist::default(),
            image_seek_preview_active: false,
            grid_open: false,
            palette_open: false,
            command_overlay_return_focus: None,
            palette: palette::CommandPalette::default(),
            status_message: None,
            pending_guard: None,
            guard_return_focus: None,
            exit_requested: false,
            prefer_hardware_encode: false,
        })
    }

    fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Box<dyn Error>> {
        let attributes = Window::default_attributes()
            .with_title(self.title())
            .with_inner_size(LogicalSize::new(960, 576))
            .with_min_inner_size(LogicalSize::new(480, 300))
            .with_visible(false)
            .with_decorations(false);
        let window = Arc::new(event_loop.create_window(attributes)?);
        let mut renderer = FrameRenderer::new(&window)?;
        let size = window.inner_size();
        renderer.resize_surface(size.width, size.height)?;
        let context = egui::Context::default();
        context.set_visuals(egui::Visuals::dark());
        fonts::install(&context);
        context.style_mut_of(egui::Theme::Dark, chrome::style);
        context.input_mut(|input| input.max_texture_side = renderer.max_texture_side());
        let mut state = egui_winit::State::new(
            context.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            window.theme(),
            Some(renderer.max_texture_side()),
        );
        state.init_accesskit(
            event_loop,
            &window,
            self.event_loop_proxy
                .as_ref()
                .expect("native application event loop proxy")
                .clone(),
        );
        window.set_visible(true);
        self.window = Some(window);
        self.renderer = Some(renderer);
        self.ui_context = Some(context);
        self.ui_state = Some(state);
        if let Some(path) = self.initial_path.take() {
            if path.is_dir() {
                self.open_folder_path(path);
            } else {
                self.open_external(path, false);
            }
        } else {
            self.state = PlaybackState::Paused;
        }
        self.refresh_title();
        self.request_redraw();
        Ok(())
    }

    fn open_external(&mut self, path: PathBuf, force_new_tab: bool) {
        let path = match canonical_shell_path(&path) {
            Ok(path) => path,
            Err(error) => {
                self.set_status(format!("Could not open {}: {error}", path.display()));
                return;
            }
        };
        let Some(kind) = MediaKind::from_path(&path) else {
            self.set_status(format!("Unsupported media: {}", path.display()));
            return;
        };
        let folder = path.parent().unwrap_or_else(|| Path::new(""));
        let dirty_playlist = kind == MediaKind::Audio
            && self.tabs.tabs().iter().any(|tab| {
                matches!(&tab.target, TabTarget::AudioFolder { folder: open, .. } if open == folder)
                    && (self.edits.get(&tab.id).is_some_and(EditHistory::is_dirty)
                        || self
                            .active_export
                            .as_ref()
                            .is_some_and(|export| export.tab == tab.id))
            });
        let id = if force_new_tab || dirty_playlist {
            self.tabs.open_new(path.clone(), kind)
        } else {
            self.tabs.open_external(path.clone(), kind)
        };
        self.edits.entry(id).or_default();
        self.load_path(path, kind);
    }

    fn open_dropped_path(&mut self, path: PathBuf) {
        if self.modal_input_blocked() {
            self.set_status("Close the dialog before dropping files.".into());
            return;
        }
        self.palette_open = false;
        self.command_overlay_return_focus = None;
        self.grid_open = false;
        self.cancel_shortcut_prefix();
        if path.is_dir() {
            self.open_folder_path(path);
        } else {
            self.open_external(path, false);
        }
    }

    fn open_folder_path(&mut self, folder: PathBuf) {
        let generation = self.folder_order.request(Some(folder));
        self.pending_folder = Some((generation, FolderIntent::Open));
        self.request_redraw();
    }

    fn load_path(&mut self, path: PathBuf, kind: MediaKind) {
        self.guard_return_focus = None;
        self.playlist.clear();
        self.cancel_view_drag();
        self.playback_error = None;
        self.pending_folder = None;
        self.folder_order.request(None);
        if self
            .folder_snapshot
            .as_ref()
            .is_some_and(|snapshot| Some(snapshot.folder_path.as_path()) != path.parent())
        {
            self.folder_snapshot = None;
        }
        self.image_generation = self.image_loader.request(Vec::new());
        self.image_loading = false;
        self.image_error = None;
        self.session.take();
        self.image = None;
        self.reading_pages.clear();
        self.timeline_open = kind == MediaKind::Audio;
        self.waveform = None;
        self.media_duration = None;
        self.hover_thumbnail = None;
        self.media_generation = self.media_generation.wrapping_add(1);
        self.duration_worker.clear();
        self.waveform_worker.clear();
        self.thumbnail_worker.clear();
        self.failed_thumbnails.clear();
        self.waveform_loading = false;
        self.thumbnail_loading = None;
        self.image_view = ImageViewState::default();
        self.path = Some(path.clone());
        self.media_kind = Some(kind);
        self.pending_time = None;
        self.clock = None;
        self.decode_finished = false;
        self.audio_drained = true;
        self.metrics_recorded = false;
        self.pending_seek_started = None;
        self.seek_latencies.clear();
        self.drift_samples.clear();
        self.refresh_folder_snapshot();
        if kind == MediaKind::Image {
            self.state = PlaybackState::Loading;
            self.rebuild_reading_pages();
            self.refresh_title();
            self.request_redraw();
            return;
        }
        let Some(renderer) = self.renderer.as_ref() else {
            self.fail("renderer is unavailable".to_owned());
            return;
        };
        let graphics_device = renderer.graphics_device();
        let notify = Arc::clone(&self.notify);
        let media_generation = self.media_generation;
        match PlaybackSession::open(
            &path,
            graphics_device,
            self.edit_state().volume,
            self.edit_state().rate,
            self.edit_state().playback_range(),
            move |event| notify(AppEvent::Playback(media_generation, event)),
        ) {
            Ok(session) => {
                self.generation = session.generation();
                self.audio_drained = !session.has_audio();
                self.session = Some(session);
                self.state = PlaybackState::Playing;
            }
            Err(error) => self.fail(error.to_string()),
        }
        self.load_duration(path.clone());
        if self.timeline_open {
            self.load_waveform();
        }
        self.refresh_title();
        self.request_redraw();
    }

    fn load_waveform(&mut self) {
        if self.waveform_loading {
            return;
        }
        let Some(path) = self.path.clone() else {
            return;
        };
        let cache = self.preview_cache.clone();
        let notify = Arc::clone(&self.notify);
        let generation = self.media_generation;
        self.waveform_loading = true;
        self.waveform_worker.submit(move |cancellation| {
            let cache = cache.cancellable(cancellation);
            let result = cache
                .waveform(&path, 640, 96)
                .map_err(|error| error.to_string());
            notify(AppEvent::Waveform(path, generation, result));
        });
    }

    fn load_hover_thumbnail(&mut self, position: Duration, bucket: u64) {
        if self.thumbnail_loading.is_some() || self.failed_thumbnails.contains(&bucket) {
            return;
        }
        let Some(path) = self.path.clone() else {
            return;
        };
        let generation = self.media_generation;
        let cache = self.preview_cache.clone();
        let notify = Arc::clone(&self.notify);
        self.thumbnail_loading = Some(bucket);
        self.thumbnail_worker.submit(move |cancellation| {
            let cache = cache.cancellable(cancellation);
            let result = cache
                .thumbnail(&path, position, 240)
                .map_err(|error| error.to_string());
            notify(AppEvent::Thumbnail(path, generation, bucket, result));
        });
    }

    fn load_duration(&mut self, path: PathBuf) {
        let cache = self.preview_cache.clone();
        let notify = Arc::clone(&self.notify);
        let generation = self.media_generation;
        self.duration_worker.submit(move |cancellation| {
            let cache = cache.cancellable(cancellation);
            let result = cache.duration(&path).map_err(|error| error.to_string());
            notify(AppEvent::Duration(path, generation, result));
        });
    }

    fn refresh_folder_snapshot(&mut self) {
        if matches!(self.pending_folder, Some((_, FolderIntent::Open))) {
            return;
        }
        let Some(path) = self.path.clone() else {
            return;
        };
        let Some(folder) = path.parent() else { return };
        self.watch_folder(folder);
        let generation = self.folder_order.request(Some(folder.to_owned()));
        self.pending_folder = Some((generation, FolderIntent::Refresh(path)));
        self.request_redraw();
    }

    fn finish_folder_load(&mut self) {
        let Some(snapshot) = self.folder_order.take_completed() else {
            return;
        };
        if self
            .pending_folder
            .as_ref()
            .map(|(generation, _)| *generation)
            != Some(snapshot.generation)
        {
            return;
        }
        let (_, intent) = self.pending_folder.take().expect("current folder request");
        match intent {
            FolderIntent::Open => {
                if let Some(first) = snapshot.items.first() {
                    let path = first.path.clone();
                    self.open_external(path.clone(), false);
                    if self.path.as_ref() == Some(&path) {
                        self.folder_order.request(None);
                        self.pending_folder = None;
                        self.apply_folder_snapshot(snapshot);
                    }
                } else {
                    self.set_status(format!(
                        "No supported media in {}",
                        snapshot.folder_path.display()
                    ));
                    self.refresh_folder_snapshot();
                }
            }
            FolderIntent::Refresh(path) if self.path.as_ref() == Some(&path) => {
                self.apply_folder_snapshot(snapshot);
            }
            FolderIntent::Refresh(_) => {}
        }
        self.request_redraw();
    }

    fn apply_folder_snapshot(&mut self, snapshot: FolderSnapshot) {
        self.filmstrip.clear();
        let current_path = self.path.clone();
        let current_identity = current_path.as_deref().and_then(|path| {
            self.folder_snapshot
                .as_ref()
                .and_then(|current| current.items.iter().find(|item| item.path == path))
                .map(|item| item.identity.clone())
        });
        let remapped = current_path.as_deref().and_then(|path| {
            snapshot
                .match_item(current_identity.as_ref(), path)
                .map(|item| (item.path.clone(), item.kind))
        });
        if snapshot.source == FolderSnapshotSource::NaturalNameFallback {
            self.set_status("Explorer order unavailable; using natural-name order".into());
        }
        self.folder_snapshot = Some(snapshot);
        if let Some((path, kind)) = remapped
            && self.path.as_ref() != Some(&path)
        {
            if let Some(tab) = self.tabs.active_mut() {
                tab.target.set_current_path(path.clone(), kind);
            }
            self.path = Some(path);
            self.media_kind = Some(kind);
            self.refresh_title();
            if self.image_loading && !self.reading_mode {
                self.rebuild_reading_pages();
            }
        }
        if self.reading_mode && self.media_kind == Some(MediaKind::Image) {
            self.rebuild_reading_pages();
        }
    }

    fn rebuild_reading_pages(&mut self) {
        self.reading_pages.clear();
        self.image_generation = self.image_loader.request(Vec::new());
        self.image_loading = false;
        if self.media_kind != Some(MediaKind::Image) {
            return;
        }
        if !self.reading_mode && self.image.is_some() {
            return;
        }
        let Some(path) = self.path.as_ref() else {
            return;
        };
        let mut paths = vec![path.clone()];
        if self.reading_mode
            && let Some(snapshot) = &self.folder_snapshot
        {
            paths.extend(
                snapshot
                    .reading_items(path, self.reading_settings.page_count, false)
                    .into_iter()
                    .filter(|item| &item.path != path)
                    .map(|item| item.path.clone()),
            );
        }
        self.image_error = None;
        self.image_loading = true;
        self.image_generation = self.image_loader.request(paths);
        self.request_redraw();
    }

    fn finish_image_load(&mut self) {
        let Some(result) = self.image_loader.take_completed() else {
            return;
        };
        if result.generation != self.image_generation {
            return;
        }
        let Some(context) = self.ui_context.clone() else {
            return;
        };
        self.image_loading = false;
        let mut images = result.images.into_iter();
        let Some((path, decoded)) = images.next() else {
            return;
        };
        if self.path.as_ref() != Some(&path) {
            return;
        }
        match decoded
            .map_err(|error| error.to_string())
            .and_then(|decoded| self.image_texture_cache.load(&context, &path, decoded))
        {
            Ok(image) => {
                let (width, height) = image.dimensions();
                self.set_status(format!(
                    "{} · {width} × {height} · {} frame(s)",
                    image.decoded.format,
                    image.decoded.frames.len()
                ));
                self.image = Some(image);
                self.image_error = None;
                self.state = PlaybackState::Paused;
            }
            Err(error) => {
                self.image = None;
                self.image_error = Some(error.clone());
                self.fail(error);
            }
        }
        self.reading_pages = images
            .map(|(path, decoded)| {
                decoded
                    .map_err(|error| error.to_string())
                    .and_then(|decoded| self.image_texture_cache.load(&context, &path, decoded))
                    .map_err(|error| format!("{}: {error}", display_name(&path)))
            })
            .collect();
        self.refresh_title();
        self.request_redraw();
    }

    fn watch_folder(&mut self, folder: &Path) {
        if self
            .folder_watcher
            .as_ref()
            .is_some_and(|(watched, _)| watched == folder)
        {
            return;
        }
        self.folder_watcher = None;
        match FolderWatcher::new(folder) {
            Ok(watcher) => self.folder_watcher = Some((folder.to_owned(), watcher)),
            Err(error) => self.set_status(error.to_string()),
        }
    }

    fn handle_app_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::Accessibility(event) => {
                if self.window.as_ref().map(|window| window.id()) == Some(event.window_id)
                    && let Some(state) = self.ui_state.as_mut()
                {
                    handle_accesskit_window_event(state, event.window_event);
                    self.request_redraw();
                }
            }
            AppEvent::ImagesReady => self.finish_image_load(),
            AppEvent::FolderReady => self.finish_folder_load(),
            AppEvent::FilmstripReady => {
                if let Some(context) = &self.ui_context {
                    self.filmstrip.finish(context);
                    self.request_redraw();
                }
            }
            AppEvent::DialogFinished(result) => self.finish_dialog(result),
            AppEvent::PromptFinished(result) => self.finish_native_prompt(result),
            AppEvent::Export(event) => self.handle_export_event(event),
            AppEvent::Playback(generation, event) if generation == self.media_generation => {
                self.handle_playback_event(event);
            }
            AppEvent::Duration(path, generation, result)
                if self.path.as_ref() == Some(&path) && generation == self.media_generation =>
            {
                match result {
                    Ok(duration) => {
                        self.media_duration = Some(duration);
                        self.request_redraw();
                    }
                    Err(error) => self.set_status(format!("Duration unavailable: {error}")),
                }
            }
            AppEvent::Waveform(path, generation, result)
                if self.path.as_ref() == Some(&path) && generation == self.media_generation =>
            {
                self.waveform_loading = false;
                match result {
                    Ok(preview) => {
                        if let Some(context) = self.ui_context.as_ref() {
                            let image = egui::ColorImage::from_rgba_unmultiplied(
                                [preview.width as usize, preview.height as usize],
                                &preview.rgba,
                            );
                            self.waveform = Some(context.load_texture(
                                format!("waveform:{}", path.display()),
                                image,
                                TextureOptions::LINEAR,
                            ));
                            self.request_redraw();
                        }
                    }
                    Err(error) => self.set_status(format!("Waveform unavailable: {error}")),
                }
            }
            AppEvent::Thumbnail(path, generation, bucket, result)
                if self.path.as_ref() == Some(&path) && generation == self.media_generation =>
            {
                if self.thumbnail_loading != Some(bucket) {
                    return;
                }
                self.thumbnail_loading = None;
                if let Err(error) = &result {
                    self.failed_thumbnails.insert(bucket);
                    eprintln!("towavue: thumbnail unavailable at bucket {bucket}: {error}");
                    self.request_redraw();
                }
                if let Ok(preview) = result
                    && let Some(context) = self.ui_context.as_ref()
                {
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [preview.width as usize, preview.height as usize],
                        &preview.rgba,
                    );
                    self.hover_thumbnail = Some((
                        bucket,
                        context.load_texture(
                            format!("thumbnail:{}:{bucket}", path.display()),
                            image,
                            TextureOptions::LINEAR,
                        ),
                    ));
                    self.request_redraw();
                }
            }
            AppEvent::Playback(_, _)
            | AppEvent::Duration(_, _, _)
            | AppEvent::Waveform(_, _, _)
            | AppEvent::Thumbnail(_, _, _, _) => {}
        }
    }

    fn handle_playback_event(&mut self, event: PlaybackEvent) {
        if self.session.is_none() || event.generation() != self.generation {
            return;
        }
        match event {
            PlaybackEvent::VideoReady(_) => {
                self.load_next_frame();
            }
            PlaybackEvent::AudioReady(_) => self.poll_audio(),
            PlaybackEvent::DecodePathSelected(_, path) => {
                eprintln!("towavue: decode path selected: {path:?}");
            }
            PlaybackEvent::DecodeFinished(_) => {
                self.decode_finished = true;
                self.check_eof();
            }
            PlaybackEvent::DeviceRemoved(_, reason) => {
                eprintln!("towavue: recovering removed D3D11 device: {reason}");
                self.recover_graphics_device(self.current_position());
            }
            PlaybackEvent::Failed(_, error) => self.fail(error),
        }
        self.request_redraw();
    }

    fn load_next_frame(&mut self) {
        if self.pending_time.is_some() {
            return;
        }
        let Some(presentation_time) = self
            .session
            .as_mut()
            .and_then(PlaybackSession::pending_video_time)
        else {
            return;
        };
        if self.clock.is_none() {
            let target = self
                .session
                .as_ref()
                .map_or(presentation_time, PlaybackSession::target);
            let mut clock = PlaybackClock::new(presentation_time.max(target), self.playback_rate());
            clock.set_paused(self.state == PlaybackState::Paused);
            self.clock = Some(clock);
        }
        self.pending_time = Some(presentation_time);
    }

    fn frame_is_due(&self) -> bool {
        let Some(presentation_time) = self.pending_time else {
            return false;
        };
        let deadline = self
            .audio_master_position()
            .map(|position| position.saturating_add(VIDEO_EARLY_TOLERANCE))
            .or_else(|| self.clock.as_ref().map(PlaybackClock::position));
        let paused_preview = self.state == PlaybackState::Paused
            && self
                .session
                .as_ref()
                .is_some_and(|session| session.video_geometry().is_none());
        video_frame_due(presentation_time, deadline, paused_preview)
    }

    fn advance_media(&mut self) {
        self.discard_late_video_frames();
        let due = self.frame_is_due();
        if due {
            let video_time = self.pending_time.take().expect("due frame exists");
            if let Some(audio_time) = self.audio_master_position()
                && self
                    .session
                    .as_ref()
                    .is_some_and(|session| video_time >= session.target())
            {
                self.drift_samples.push(Duration::from_nanos(
                    video_time
                        .as_nanoseconds()
                        .abs_diff(audio_time.as_nanoseconds()),
                ));
            }
            if let Some(session) = self.session.as_mut() {
                session.advance_pending();
            }
            self.load_next_frame();
        }
    }

    fn discard_late_video_frames(&mut self) {
        if let Some(cutoff) = late_video_cutoff(self.state, self.audio_master_position())
            && let Some(session) = self.session.as_mut()
            && session.drop_video_before(cutoff) > 0
        {
            self.pending_time = session.pending_video_time();
        }
    }

    fn render_frame(&mut self) {
        self.advance_media();
        if self.renderer.is_none() {
            self.show_native_fallback();
            return;
        }
        let (Some(window), Some(context)) = (self.window.as_ref(), self.ui_context.clone()) else {
            return;
        };
        let input = self
            .ui_state
            .as_mut()
            .expect("UI state exists")
            .take_egui_input(window);
        let mut actions = Vec::new();
        let mut output = context.run_ui(input, |ui| {
            self.draw_ui(ui, &mut actions);
        });
        keep_accessibility_focus_live(&context, &mut output.platform_output);
        if self.restore_ui_textures {
            output
                .textures_delta
                .set
                .extend(self.restored_ui_textures(&context));
        }
        let repaint_delay = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map_or(Duration::MAX, |viewport| viewport.repaint_delay);
        self.ui_repaint_at = ui_repaint_deadline(
            Instant::now(),
            repaint_delay,
            self.state == PlaybackState::Playing
                && self.pending_time.is_none()
                && !self.audio_drained,
        );
        let renderer = self.renderer.as_mut().expect("renderer exists");
        let media_result = renderer.clear([0.025, 0.025, 0.03, 1.0]).and_then(|()| {
            if let (Some(session), Some(rect)) = (&mut self.session, self.video_rect) {
                session.draw_current(renderer, rect * context.pixels_per_point(), self.video_uv)
            } else {
                Ok(false)
            }
        });
        let media_drawn = match media_result {
            Ok(drawn) => drawn,
            Err(error) => {
                self.handle_render_error(error);
                return;
            }
        };
        let mut platform_output = match self
            .renderer
            .as_mut()
            .expect("renderer exists")
            .render_ui(&context, output)
        {
            Ok(output) => output,
            Err(error) => {
                self.handle_render_error(error);
                return;
            }
        };
        if self.viewing_cursor.hidden {
            platform_output.cursor_icon = egui::CursorIcon::None;
            platform_output.cursor_image = None;
        }
        self.ui_state
            .as_mut()
            .expect("UI state exists")
            .handle_platform_output(
                self.window.as_ref().expect("window exists"),
                platform_output,
            );
        if let Err(error) = self
            .renderer
            .as_ref()
            .expect("renderer exists")
            .present_surface()
        {
            self.handle_render_error(error);
            return;
        }
        self.record_seek_presentation(media_drawn);
        self.restore_ui_textures = false;
        self.check_eof();
        // Consumed keys can emit actions only in an earlier, discarded layout pass.
        for (index, action) in actions.iter().enumerate() {
            if !actions[..index].contains(action) {
                self.handle_ui_action(action.clone());
            }
        }
    }

    fn draw_ui(&mut self, root: &mut egui::Ui, actions: &mut Vec<UiAction>) {
        self.image_seek_preview_active = false;
        self.video_rect = None;
        let context = root.ctx().clone();
        let modal_blocked = self.modal_input_blocked();
        // Decide before the menu can close itself with this same Escape event.
        if self.grid_open
            && !self.palette_open
            && !modal_blocked
            && !egui::Popup::is_any_open(&context)
            && context
                .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            if self.prefix_started.is_some() {
                self.cancel_shortcut_prefix();
            } else {
                self.cancel_command_overlay();
            }
        }
        if !modal_blocked && self.guard_return_focus.is_some() {
            if context.memory(|memory| memory.top_modal_layer().is_none()) {
                if let Some((tab, id)) = self.guard_return_focus.take()
                    && self.tabs.active().is_some_and(|active| active.id == tab)
                {
                    context.memory_mut(|memory| memory.request_focus(id));
                }
            } else {
                // egui retains the previous pass's modal layer until the next pass ends.
                context.request_repaint();
            }
        }
        if modal_blocked {
            let opacity = root.opacity();
            root.disable();
            root.set_opacity(opacity);
            egui::Popup::close_all(&context);
        }
        wheel_input::begin_frame(&context);
        let mut volume_targets = Vec::new();
        let status_rect = if self.fullscreen {
            None
        } else {
            self.draw_top_bar(root, actions);
            let rect = self.draw_status_bar(root, actions, &mut volume_targets);
            self.draw_timeline(root, actions);
            Some(rect)
        };
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(root, |ui| {
                if self.path.is_none() {
                    if let Some(command) = welcome::show(ui, &self.shortcuts) {
                        actions.push(UiAction::Command(command));
                    }
                } else if self.state == PlaybackState::Faulted
                    && self.media_kind != Some(MediaKind::Image)
                    && let Some(error) = &self.playback_error
                {
                    ui.centered_and_justified(|ui| {
                        ui.label(format!("Could not play media\n{error}"));
                    });
                } else if self.media_kind == Some(MediaKind::Audio) {
                    self.draw_audio_playlist(ui, actions);
                } else if self.media_kind == Some(MediaKind::Image) {
                    self.draw_image(ui);
                    if self.image_loading {
                        egui::Area::new("image-loading".into())
                            .fixed_pos(ui.max_rect().center_top() + egui::vec2(-65.0, 12.0))
                            .interactable(false)
                            .show(ui.ctx(), |ui| {
                                egui::Frame::popup(ui.style()).show(ui, |ui| {
                                    ui.label("Loading images…");
                                });
                            });
                    }
                } else if self.media_kind == Some(MediaKind::Video) {
                    self.draw_video_edit_overlay(ui, &mut volume_targets);
                }
            });
        if let Some(rect) = status_rect {
            self.draw_seek_bar(&context, rect, None, actions);
        }
        self.draw_fullscreen_controls(&context, actions, &mut volume_targets);
        self.volume_wheel(&context, &volume_targets, actions);
        if self.fullscreen
            && let Some((message, started)) = &self.status_message
            && started.elapsed() < STATUS_MESSAGE_DURATION
        {
            egui::Area::new("fullscreen-status".into())
                .anchor(Align2::CENTER_TOP, [0.0, 12.0])
                .interactable(false)
                .show(&context, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_max_width((context.content_rect().width() - 32.0).max(100.0));
                        ui.label(message);
                    });
                });
        }
        if self.filmstrip_open {
            if !modal_blocked {
                self.filmstrip.show(
                    &context,
                    self.folder_snapshot.as_ref(),
                    self.path.as_deref(),
                    !self.palette_open && !self.grid_open,
                    actions,
                );
            }
        } else if !self.image_seek_preview_active {
            self.filmstrip.clear();
        }
        if self.palette_open && !modal_blocked {
            self.draw_command_palette(&context, actions);
        }
        if !modal_blocked {
            self.draw_grid_menu(&context, actions);
        }
        if self.export_error.is_none() {
            self.draw_export_status(&context, actions);
        }
        if let Some(error) = &self.export_error {
            let modal = egui::Modal::new("export-error".into()).show(&context, |ui| {
                ui.set_width((context.content_rect().width() - 32.0).clamp(1.0, 520.0));
                chrome::modal_heading(ui, "Export failed");
                ui.label("Your edits and existing files have been kept.");
                egui::ScrollArea::vertical()
                    .max_height((context.content_rect().height() - 130.0).clamp(20.0, 220.0))
                    .min_scrolled_height(20.0)
                    .show(ui, |ui| {
                        ui.label(error);
                    });
                if ui.button("OK").clicked() {
                    actions.push(UiAction::DismissExportError);
                }
            });
            if modal.is_top_modal
                && !modal.any_popup_open
                && context
                    .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            {
                actions.push(UiAction::DismissExportError);
            }
        } else if self.pending_guard.is_some() {
            self.draw_unsaved_guard(&context, actions);
        }
        if context.input(|input| !input.raw.hovered_files.is_empty()) {
            let painter = context.layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                egui::Id::new("external-file-drop"),
            ));
            let rect = context.content_rect().shrink(8.0);
            painter.rect_filled(rect, 6.0, egui::Color32::from_black_alpha(190));
            painter.rect_stroke(
                rect,
                6.0,
                egui::Stroke::new(1.0, chrome::MUTED),
                egui::StrokeKind::Inside,
            );
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                if self.modal_input_blocked() {
                    "Close the dialog before dropping files"
                } else {
                    "Drop to open media files or a folder"
                },
                egui::FontId::proportional(18.0),
                egui::Color32::WHITE,
            );
        }
    }

    fn draw_export_status(&self, context: &egui::Context, actions: &mut Vec<UiAction>) {
        let Some(export) = &self.active_export else {
            return;
        };
        let mut contents = |ui: &mut egui::Ui| {
            ui.set_width((context.content_rect().width() - 32.0).clamp(1.0, 340.0));
            ui.add(egui::Label::new(display_name(&export.request.target)).truncate())
                .on_hover_text(export.request.target.display().to_string());
            ui.label(if export.cancelling {
                "Cancelling export…".to_owned()
            } else {
                format!("Encoded {}", format_time(media_time(export.encoded)))
            });
            if ui
                .add_enabled(!export.cancelling, egui::Button::new("Cancel export"))
                .clicked()
            {
                actions.push(UiAction::CancelExport);
            }
        };
        if export.continuation.is_some() {
            egui::Modal::new("export-before-continuing".into()).show(context, |ui| {
                ui.set_width((context.content_rect().width() - 32.0).clamp(1.0, 340.0));
                chrome::modal_heading(ui, "Exporting before continuing");
                contents(ui);
            });
        } else {
            egui::Window::new("Exporting")
                .id("export-progress".into())
                .anchor(Align2::RIGHT_BOTTOM, [-12.0, -44.0])
                .resizable(false)
                .collapsible(false)
                .show(context, contents);
        }
    }

    fn draw_unsaved_guard(&self, context: &egui::Context, actions: &mut Vec<UiAction>) {
        let name = self
            .path
            .as_deref()
            .map_or_else(|| "this media".to_owned(), display_name);
        let modal = egui::Modal::new("unsaved-edit-guard".into()).show(context, |ui| {
            ui.set_width((context.content_rect().width() - 32.0).clamp(1.0, 520.0));
            chrome::modal_heading(ui, "Unsaved edits");
            ui.separator();
            ui.label("Export edits before continuing?");
            ui.add(egui::Label::new(&name).truncate())
                .on_hover_text(&name);
            ui.label("Source file unchanged.");
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        self.active_export.is_none(),
                        egui::Button::new("Export and continue"),
                    )
                    .clicked()
                {
                    actions.push(UiAction::ResolveGuard(GuardDecision::Save));
                }
                if ui.button("Discard edits").clicked() {
                    actions.push(UiAction::ResolveGuard(GuardDecision::Discard));
                }
                if ui.button("Cancel").clicked() {
                    actions.push(UiAction::ResolveGuard(GuardDecision::Cancel));
                }
                if self.active_export.is_some() && ui.button("Cancel current export").clicked() {
                    actions.push(UiAction::CancelExport);
                }
            });
        });
        if modal.is_top_modal
            && !modal.any_popup_open
            && context
                .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            actions.push(UiAction::ResolveGuard(GuardDecision::Cancel));
        }
    }

    fn draw_image(&mut self, ui: &mut egui::Ui) {
        if self.reading_mode {
            self.draw_reading_pages(ui);
            return;
        }
        if let Some(error) = &self.image_error {
            ui.centered_and_justified(|ui| {
                ui.label(format!("Could not load image\n{error}"));
            });
            return;
        }
        let Some(image) = self.image.as_ref() else {
            return;
        };
        let texture = image.texture.id();
        let mut transform = self.visual_transform(image.dimensions());
        let viewport = ui
            .max_rect()
            .shrink(if self.fullscreen { 0.0 } else { 8.0 });
        let region = self.image_view.preview_region();
        transform.crop(region);
        let image_size = (transform.size.0 as u32, transform.size.1 as u32);
        self.image_viewport = viewport.size();
        let pixels_per_point = ui.ctx().pixels_per_point();
        let physical_viewport = viewport.size() * pixels_per_point;
        let mut scale =
            self.image_view.scale(image_size, physical_viewport.into()) / pixels_per_point;
        let center = viewport.center() + egui::vec2(self.image_view.pan.0, self.image_view.pan.1);
        let response = ui.interact(
            viewport,
            ui.id().with("image-surface"),
            egui::Sense::click_and_drag(),
        );

        let (shift, zoom, pointer) = ui.input(|input| {
            (
                input.modifiers.shift,
                input.zoom_delta(),
                input.pointer.hover_pos(),
            )
        });
        if zoom != 1.0
            && response.hovered()
            && !self.modal_input_blocked()
            && !self.palette_open
            && !self.grid_open
        {
            let old_scale = scale;
            self.image_view
                .zoom_by(zoom, image_size, physical_viewport.into());
            scale = self.image_view.scale(image_size, physical_viewport.into()) / pixels_per_point;
            if let Some(pointer) = pointer {
                let from_center = pointer - center;
                let correction = from_center * (1.0 - scale / old_scale);
                self.image_view.pan.0 += correction.x;
                self.image_view.pan.1 += correction.y;
            }
        }
        self.update_pan(&response, pointer);

        let displayed = egui::vec2(transform.size.0 * scale, transform.size.1 * scale);
        let center = viewport.center() + egui::vec2(self.image_view.pan.0, self.image_view.pan.1);
        let image_rect = egui::Rect::from_center_size(center, displayed);

        if !self.image_view.crop_preview {
            self.update_selection(&response, image_rect, image_size, shift, pointer);
        }

        let painter = ui.painter_at(viewport);
        painter.add(egui::Shape::mesh(transformed_image_mesh(
            texture, image_rect, transform,
        )));
        if !self.image_view.crop_preview
            && let Some(selection) = self.image_view.selection
        {
            paint_selection(&painter, image_rect, selection);
            self.selection_controls(ui, image_rect, image_size);
            if let Some(selection) = self.image_view.selection {
                let offset = selection::reveal_offset(
                    ui.ctx(),
                    self.selection_identity(),
                    viewport,
                    image_rect,
                    selection,
                );
                if offset != egui::Vec2::ZERO {
                    self.image_view.pan.0 += offset.x;
                    self.image_view.pan.1 += offset.y;
                    self.request_redraw();
                }
            }
        }
    }

    fn draw_video_edit_overlay(
        &mut self,
        ui: &mut egui::Ui,
        volume_targets: &mut Vec<egui::Response>,
    ) {
        let Some((width, height, pixel_aspect)) = self
            .session
            .as_ref()
            .and_then(PlaybackSession::video_geometry)
        else {
            return;
        };
        let transform = self.visual_transform((width, height));
        let size = (transform.size.0 as u32, transform.size.1 as u32);
        let pixel_aspect = transform.pixel_aspect(pixel_aspect);
        let viewport = fitted_video_rect(ui.max_rect(), size, pixel_aspect);
        self.video_rect = Some(viewport);
        self.video_uv = transform.uv;
        let response = ui.interact(
            viewport,
            ui.id().with("video-edit-surface"),
            egui::Sense::click_and_drag(),
        );
        let (shift, pointer) = ui.input(|input| (input.modifiers.shift, input.pointer.hover_pos()));
        volume_targets.push(response.clone());
        self.update_selection(&response, viewport, size, shift, pointer);
        if let Some(selection) = self.image_view.selection {
            paint_selection(&ui.painter_at(viewport), viewport, selection);
            self.selection_controls(ui, viewport, size);
        }
    }

    fn selection_identity(&self) -> egui::Id {
        egui::Id::new((
            "media-selection",
            self.tabs.active().map(|tab| tab.id),
            &self.path,
        ))
    }

    fn selection_controls(&mut self, ui: &mut egui::Ui, rect: egui::Rect, size: (u32, u32)) {
        let (Some(selected), Some(kind)) = (self.image_view.selection, self.media_kind) else {
            return;
        };
        let enabled = self.view_drag_allowed(ui.ctx());
        let (changed, invalid) = selection::controls(
            ui,
            self.selection_identity(),
            rect,
            selected,
            size,
            kind,
            enabled,
        );
        if changed.is_some() || invalid {
            self.cancel_view_drag();
            if let Some(changed) = changed {
                self.image_view.selection = Some(changed);
            }
            if invalid {
                self.set_status("Selection edges must leave a non-empty rectangle".into());
            }
            self.request_redraw();
        }
    }

    fn volume_wheel(
        &self,
        context: &egui::Context,
        targets: &[egui::Response],
        actions: &mut Vec<UiAction>,
    ) {
        let Some(tab) = self.tabs.active() else {
            return;
        };
        if !matches!(self.media_kind, Some(MediaKind::Video | MediaKind::Audio))
            || self.modal_input_blocked()
            || self.palette_open
            || self.grid_open
            || self.filmstrip_open
            || egui::Popup::is_any_open(context)
        {
            return;
        }
        if context.input(|input| {
            !input.focused
                || input.pointer.any_down()
                || !input.modifiers.is_none()
                || input.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::PointerButton { pressed: true, .. }
                            | egui::Event::WindowFocused(false)
                    )
                })
        }) {
            return;
        }
        let delta = wheel_input::volume_delta(context, targets);
        let before = self.edit_state().volume;
        let volume = (before + delta * 0.1).clamp(0.0, 2.0);
        if volume != before {
            actions.push(UiAction::Volume(tab.id, volume));
        }
    }

    fn cancel_view_drag(&mut self) -> bool {
        let mut canceled_press = self.ui_context.as_ref().is_some_and(timeline_input::cancel);
        if let Some(state) = &mut self.ui_state {
            // Commands and focus changes can precede the frame that would start the drag.
            state.egui_input_mut().events.retain(|event| {
                let press = matches!(
                    event,
                    egui::Event::PointerButton {
                        button: egui::PointerButton::Primary | egui::PointerButton::Secondary,
                        pressed: true,
                        ..
                    }
                );
                canceled_press |= press;
                !press
            });
        }
        let Some(drag) = self.view_drag.take() else {
            if canceled_press {
                self.request_redraw();
            }
            return canceled_press;
        };
        match drag {
            ViewDrag::Selection { before, .. } => self.image_view.selection = before,
            ViewDrag::Pan { before, .. } => self.image_view.pan = before,
        }
        self.request_redraw();
        true
    }

    fn view_drag_allowed(&mut self, context: &egui::Context) -> bool {
        if !context.input(|input| input.focused)
            || self.modal_input_blocked()
            || self.palette_open
            || self.grid_open
            || self.filmstrip_open
            || egui::Popup::is_any_open(context)
        {
            self.cancel_view_drag();
            return false;
        }
        true
    }

    fn update_pan(&mut self, response: &egui::Response, pointer: Option<egui::Pos2>) {
        if !self.view_drag_allowed(&response.ctx) {
            return;
        }
        let (origin, release) =
            view_drag_button_positions(response, egui::PointerButton::Secondary);
        if self.view_drag.is_none()
            && let Some(origin) = origin
        {
            self.view_drag = Some(ViewDrag::Pan {
                origin,
                before: self.image_view.pan,
            });
        }
        if let Some(ViewDrag::Pan { origin, before }) = self.view_drag {
            if (response.dragged_by(egui::PointerButton::Secondary) || release.is_some())
                && let Some(pointer) = release.or(pointer)
            {
                let delta = pointer - origin;
                self.image_view.pan = (before.0 + delta.x, before.1 + delta.y);
            }
            if release.is_some() {
                self.view_drag = None;
            }
        }
    }

    fn update_selection(
        &mut self,
        response: &egui::Response,
        image_rect: egui::Rect,
        image_size: (u32, u32),
        square: bool,
        pointer: Option<egui::Pos2>,
    ) {
        if !self.view_drag_allowed(&response.ctx) {
            return;
        }
        let (origin, release) = view_drag_button_positions(response, egui::PointerButton::Primary);
        let released = release.is_some();
        let Some(pointer) = release.or(pointer) else {
            return;
        };
        let point = unit_point(pointer, image_rect);
        if self.view_drag.is_none()
            && let Some(origin) = origin
            && let Some(mode) = self
                .image_view
                .selection
                .and_then(|selection| selection_edge(origin, image_rect, selection))
                .or_else(|| {
                    image_rect
                        .contains(origin)
                        .then(|| SelectionDrag::New(unit_point(origin, image_rect)))
                })
        {
            self.view_drag = Some(ViewDrag::Selection {
                mode,
                before: self.image_view.selection,
            });
        }
        let selection_owned = matches!(self.view_drag, Some(ViewDrag::Selection { .. }));
        // A move and release in one frame can bypass egui's drag-start notification.
        let dragging = selection_owned
            && (response.dragged_by(egui::PointerButton::Primary)
                || (released && !response.clicked_by(egui::PointerButton::Primary)));
        if dragging {
            match self.view_drag {
                Some(ViewDrag::Selection {
                    mode: SelectionDrag::New(start),
                    ..
                }) => {
                    self.image_view.selection =
                        Some(UnitRect::from_drag(start, point, image_size, square));
                }
                Some(ViewDrag::Selection { mode, .. }) => {
                    self.resize_selection(mode, point, square, image_size)
                }
                _ => {}
            }
        }
        if dragging && released {
            self.image_view.selection = self.image_view.selection.and_then(|selection| {
                PixelCrop::from_selection(selection, image_size, self.media_kind?)
                    .map(|crop| crop.unit_rect(image_size))
            });
        }
        if released && matches!(self.view_drag, Some(ViewDrag::Selection { .. })) {
            self.view_drag = None;
        }
        if self.media_kind == Some(MediaKind::Image)
            && selection_owned
            && response.clicked_by(egui::PointerButton::Primary)
            && self
                .image_view
                .selection
                .is_some_and(|selection| selection.contains(point))
        {
            self.image_view.crop_preview = true;
            self.image_view.fit();
        }
    }

    fn resize_selection(
        &mut self,
        edge: SelectionDrag,
        point: UnitPoint,
        preserve_ratio: bool,
        image_size: (u32, u32),
    ) {
        let Some(mut selection) = self.image_view.selection else {
            return;
        };
        let ratio_selection = match self.view_drag {
            Some(ViewDrag::Selection {
                before: Some(before),
                ..
            }) => before,
            _ => selection,
        };
        let pixel_ratio = ratio_selection.width() * image_size.0 as f32
            / (ratio_selection.height() * image_size.1 as f32).max(1.0);
        match edge {
            SelectionDrag::Left => selection.min.x = point.x.min(selection.max.x),
            SelectionDrag::Right => selection.max.x = point.x.max(selection.min.x),
            SelectionDrag::Top => selection.min.y = point.y.min(selection.max.y),
            SelectionDrag::Bottom => selection.max.y = point.y.max(selection.min.y),
            SelectionDrag::New(_) => return,
        }
        if preserve_ratio && image_size.0 > 0 && image_size.1 > 0 {
            match edge {
                SelectionDrag::Left | SelectionDrag::Right => {
                    let center = (selection.min.y + selection.max.y) * 0.5;
                    let width = selection.width().min(
                        2.0 * center.min(1.0 - center) * image_size.1 as f32 * pixel_ratio
                            / image_size.0 as f32,
                    );
                    if matches!(edge, SelectionDrag::Left) {
                        selection.min.x = selection.max.x - width;
                    } else {
                        selection.max.x = selection.min.x + width;
                    }
                    let height = width * image_size.0 as f32
                        / pixel_ratio.max(f32::EPSILON)
                        / image_size.1 as f32;
                    selection.min.y = (center - height * 0.5).max(0.0);
                    selection.max.y = (center + height * 0.5).min(1.0);
                }
                SelectionDrag::Top | SelectionDrag::Bottom => {
                    let center = (selection.min.x + selection.max.x) * 0.5;
                    let height = selection.height().min(
                        2.0 * center.min(1.0 - center) * image_size.0 as f32
                            / pixel_ratio.max(f32::EPSILON)
                            / image_size.1 as f32,
                    );
                    if matches!(edge, SelectionDrag::Top) {
                        selection.min.y = selection.max.y - height;
                    } else {
                        selection.max.y = selection.min.y + height;
                    }
                    let width = height * image_size.1 as f32 * pixel_ratio / image_size.0 as f32;
                    selection.min.x = (center - width * 0.5).max(0.0);
                    selection.max.x = (center + width * 0.5).min(1.0);
                }
                SelectionDrag::New(_) => {}
            }
        }
        self.image_view.selection = Some(selection);
    }

    fn draw_reading_pages(&self, ui: &mut egui::Ui) {
        let viewport = ui.max_rect();
        let first = self
            .image
            .as_ref()
            .map(Ok)
            .or_else(|| self.image_error.as_ref().map(Err));
        let pages: Vec<_> = first
            .into_iter()
            .chain(self.reading_pages.iter().map(|page| page.as_ref()))
            .collect();
        if pages.is_empty() {
            ui.centered_and_justified(|ui| ui.label("No image pages available"));
            return;
        }
        let sizes: Vec<_> = pages
            .iter()
            .map(|page| page.map_or(egui::Vec2::splat(1.0), |image| image.texture.size_vec2()))
            .collect();
        let rects = reading_page_rects(
            viewport,
            &sizes,
            self.reading_settings.axis,
            self.reading_settings.reversed,
        );
        let painter = ui.painter_at(viewport);
        for (image, page) in pages.into_iter().zip(rects) {
            let image = match image {
                Ok(image) => image,
                Err(error) => {
                    ui.scope_builder(egui::UiBuilder::new().max_rect(page), |ui| {
                        ui.set_clip_rect(page);
                        ui.centered_and_justified(|ui| ui.label(error));
                    });
                    continue;
                }
            };
            painter.image(
                image.texture.id(),
                page,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }

    fn draw_top_bar(&self, root: &mut egui::Ui, actions: &mut Vec<UiAction>) {
        let return_to_tab = root.ctx().data_mut(|data| {
            data.remove_temp::<bool>("filmstrip-return-tab".into())
                .unwrap_or(false)
        });
        let window_rect = root.max_rect();
        egui::Panel::top("tabs")
            .exact_size(32.0)
            .frame(chrome::bar())
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    ui.visuals_mut().widgets.inactive.weak_bg_fill = chrome::BACKGROUND;
                    let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
                    let menu = ui.menu_button("    ", |ui| {
                        menu::show(ui, self.command_context(), &self.shortcuts)
                    });
                    if return_to_tab && self.tabs.active().is_none() {
                        menu.response.request_focus();
                    }
                    if let Some(Some(command)) = menu.inner {
                        menu.response.request_focus();
                        actions.push(UiAction::Command(command));
                    } else if escape
                        && menu.inner.is_some()
                        && !egui::Popup::is_id_open(
                            ui.ctx(),
                            egui::Popup::default_response_id(&menu.response),
                        )
                    {
                        menu.response.request_focus();
                    }
                    chrome::logo(ui, menu.response.rect);
                    menu.response.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            ui.is_enabled(),
                            "towavue menu",
                        )
                    });
                    menu.response.on_hover_text("towavue menu");

                    let controls_width = 98.0;
                    let strip_width = (ui.available_width() - controls_width - 56.0).max(80.0);
                    let width = chrome::tab_width(strip_width, self.tabs.tabs().len());
                    egui::ScrollArea::horizontal()
                        .id_salt("tab-strip")
                        .max_width(strip_width)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let mut tab_rects = Vec::new();
                                let mut dragged = None;
                                for (index, tab) in self.tabs.tabs().iter().enumerate() {
                                    let active =
                                        self.tabs.active().is_some_and(|item| item.id == tab.id);
                                    let dirty =
                                        self.edits.get(&tab.id).is_some_and(EditHistory::is_dirty);
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(width, 26.0),
                                        egui::Sense::hover(),
                                    );
                                    tab_rects.push(rect);
                                    if active {
                                        let focus = (tab.id, index, width, strip_width);
                                        let focus_id = ui.id().with("visible-active-tab");
                                        let changed = ui.data_mut(|data| {
                                            let changed = data
                                                .get_temp::<(TabId, usize, f32, f32)>(focus_id)
                                                != Some(focus);
                                            data.insert_temp(focus_id, focus);
                                            changed
                                        });
                                        if changed {
                                            ui.scroll_to_rect(rect, None);
                                        }
                                        ui.painter().rect_filled(rect, 3.0, Color32::from_gray(28));
                                    }
                                    let label_rect = egui::Rect::from_min_max(
                                        rect.min,
                                        rect.max - egui::vec2(24.0, 0.0),
                                    );
                                    let label = format!(
                                        "{}{}",
                                        display_name(tab.target.current_path()),
                                        if dirty { " *" } else { "" }
                                    );
                                    // Cached actions and keyboard focus must follow the tab, not its slot.
                                    let mut tab_ui = ui.new_child(
                                        egui::UiBuilder::new()
                                            .id(ui.id().with(("media-tab", tab.id)))
                                            .max_rect(rect),
                                    );
                                    let response = tab_ui
                                        .put(
                                            label_rect,
                                            egui::Button::new(RichText::new(label).color(
                                                if active {
                                                    Color32::from_gray(230)
                                                } else {
                                                    chrome::MUTED
                                                },
                                            ))
                                            .frame(false)
                                            .truncate()
                                            .sense(egui::Sense::click_and_drag()),
                                        )
                                        .on_hover_text(
                                            tab.target.current_path().display().to_string(),
                                        );
                                    if response.clicked() {
                                        actions.push(UiAction::ActivateTab(tab.id));
                                    }
                                    if return_to_tab && active {
                                        response.request_focus();
                                    }
                                    if response.clicked_by(egui::PointerButton::Middle) {
                                        actions.push(UiAction::CloseTab(tab.id));
                                    }
                                    if response.dragged_by(egui::PointerButton::Primary)
                                        || response.drag_stopped_by(egui::PointerButton::Primary)
                                    {
                                        dragged = Some((tab.id, response));
                                    }
                                    let close_rect = egui::Rect::from_min_max(
                                        egui::pos2(rect.right() - 24.0, rect.top()),
                                        rect.max,
                                    );
                                    let close = tab_ui
                                        .put(close_rect, egui::Button::new("×").frame(false))
                                        .on_hover_text("Close tab");
                                    close.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::WidgetType::Button,
                                            tab_ui.is_enabled(),
                                            format!(
                                                "Close tab: {}",
                                                display_name(tab.target.current_path())
                                            ),
                                        )
                                    });
                                    tab_ui.ctx().accesskit_node_builder(close.id, |node| {
                                        node.set_description(
                                            tab.target.current_path().display().to_string(),
                                        );
                                    });
                                    if close.clicked() {
                                        actions.push(UiAction::CloseTab(tab.id));
                                    }
                                }
                                if let Some((id, response)) = dragged {
                                    if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                                        ui.ctx().stop_dragging();
                                    } else if let Some(pointer) = response.interact_pointer_pos() {
                                        let strip = ui.min_rect().intersect(ui.clip_rect());
                                        if let Some((gap, x)) =
                                            chrome::tab_drop_gap(&tab_rects, strip, pointer)
                                        {
                                            ui.painter_at(strip).line_segment(
                                                [
                                                    egui::pos2(x, strip.top()),
                                                    egui::pos2(x, strip.bottom()),
                                                ],
                                                egui::Stroke::new(2.0, Color32::from_gray(225)),
                                            );
                                            if response
                                                .drag_stopped_by(egui::PointerButton::Primary)
                                            {
                                                actions.push(UiAction::ReorderTab(id, gap));
                                            }
                                        } else if response
                                            .drag_stopped_by(egui::PointerButton::Primary)
                                            && !window_rect.contains(pointer)
                                        {
                                            actions.push(UiAction::DetachTab(id));
                                        }
                                    }
                                }
                            });
                        });
                    let (drag_rect, response) = ui.allocate_exact_size(
                        egui::vec2((ui.available_width() - controls_width).max(20.0), 26.0),
                        egui::Sense::click_and_drag(),
                    );
                    if self.tabs.tabs().is_empty() {
                        let welcome_rect = egui::Rect::from_min_size(
                            drag_rect.min,
                            egui::vec2(drag_rect.width().min(150.0), drag_rect.height()),
                        );
                        ui.painter()
                            .rect_filled(welcome_rect, 3.0, Color32::from_gray(28));
                        ui.painter().text(
                            welcome_rect.left_center() + egui::vec2(10.0, 0.0),
                            Align2::LEFT_CENTER,
                            "Welcome",
                            egui::FontId::proportional(12.0),
                            chrome::MUTED,
                        );
                    }
                    if response.double_clicked() {
                        actions.push(UiAction::Maximize);
                    } else if response.drag_started() {
                        actions.push(UiAction::DragWindow);
                    }
                    if chrome::button(ui, "−", "Minimize").clicked() {
                        actions.push(UiAction::Minimize);
                    }
                    let maximized = self
                        .window
                        .as_ref()
                        .is_some_and(|window| window.is_maximized());
                    if chrome::button(ui, if maximized { "▣" } else { "□" }, "Maximize / restore")
                        .clicked()
                    {
                        actions.push(UiAction::Maximize);
                    }
                    if chrome::button(ui, "×", "Close window").clicked() {
                        actions.push(UiAction::CloseWindow);
                    }
                });
            });
        if !self
            .window
            .as_ref()
            .is_some_and(|window| window.is_maximized())
            && let Some(position) = root.input(|input| input.pointer.hover_pos())
            && let Some(direction) = chrome::resize_edge(window_rect, position)
        {
            root.ctx().set_cursor_icon(match direction {
                winit::window::ResizeDirection::North | winit::window::ResizeDirection::South => {
                    egui::CursorIcon::ResizeVertical
                }
                winit::window::ResizeDirection::East | winit::window::ResizeDirection::West => {
                    egui::CursorIcon::ResizeHorizontal
                }
                winit::window::ResizeDirection::NorthWest
                | winit::window::ResizeDirection::SouthEast => egui::CursorIcon::ResizeNwSe,
                _ => egui::CursorIcon::ResizeNeSw,
            });
            if root.input(|input| input.pointer.primary_pressed()) {
                actions.push(UiAction::ResizeWindow(direction));
            }
        }
    }

    fn draw_grid_menu(&mut self, context: &egui::Context, actions: &mut Vec<UiAction>) {
        let opacity =
            context.animate_bool_with_time("grid-menu-animation".into(), self.grid_open, 0.12);
        if opacity <= 0.0 {
            return;
        }
        let Some(kind) = self.media_kind else { return };
        let commands = *self.grid_layouts.get(kind);
        let available = context.content_rect().size() - egui::vec2(28.0, 50.0);
        let cell = ((available - egui::vec2(18.0, 18.0)) / 4.0)
            .clamp(egui::Vec2::splat(1.0), egui::vec2(110.0, 52.0));
        egui::Area::new("grid-menu".into())
            .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .order(egui::Order::Foreground)
            .show(context, |ui| {
                ui.set_opacity(opacity);
                if !self.grid_open {
                    ui.disable();
                }
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(cell.x * 4.0 + 18.0);
                    egui::Grid::new("command-grid")
                        .spacing([6.0, 6.0])
                        .min_col_width(cell.x)
                        .max_col_width(cell.x)
                        .show(ui, |ui| {
                            for (index, command) in commands.iter().copied().enumerate() {
                                let title = command_definitions()
                                    .iter()
                                    .find(|definition| definition.id == command)
                                    .map_or(command.as_str(), |definition| definition.title);
                                let enabled = command_definitions()
                                    .iter()
                                    .find(|definition| definition.id == command)
                                    .is_some_and(|definition| {
                                        definition.is_enabled(self.command_context())
                                    });
                                let label = format!("{}\n{}", grid::KEYS[index], title);
                                let mut font = egui::TextStyle::Button.resolve(ui.style());
                                if cell.y < 44.0 {
                                    font.size = 11.0;
                                }
                                let row_height = ui.fonts_mut(|fonts| fonts.row_height(&font));
                                let padding = ui.spacing().button_padding * 2.0;
                                let mut text = egui::text::LayoutJob::simple(
                                    label,
                                    font,
                                    ui.visuals().text_color(),
                                    (cell.x - padding.x).max(1.0),
                                );
                                text.wrap.max_rows =
                                    (((cell.y - padding.y) / row_height) as usize).max(1);
                                let text = ui.fonts_mut(|fonts| fonts.layout_job(text));
                                if ui
                                    .add_enabled(enabled, egui::Button::new(text).min_size(cell))
                                    .on_hover_text(title)
                                    .on_disabled_hover_text(title)
                                    .clicked()
                                {
                                    if !matches!(
                                        command,
                                        CommandId::ToggleGridMenu
                                            | CommandId::ToggleCommandPalette
                                            | CommandId::ToggleFilmstrip
                                    ) {
                                        self.grid_open = false;
                                    }
                                    actions.push(UiAction::Command(command));
                                }
                                if index % 4 == 3 {
                                    ui.end_row();
                                }
                            }
                        });
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(format!("Edit {}", self.grid_path.display()))
                                .weak(),
                        )
                        .truncate(),
                    )
                    .on_hover_text(self.grid_path.display().to_string());
                });
            });
    }

    fn draw_fullscreen_controls(
        &mut self,
        context: &egui::Context,
        actions: &mut Vec<UiAction>,
        volume_targets: &mut Vec<egui::Response>,
    ) {
        let screen = context.content_rect();
        let eligible = self.fullscreen
            && !self.modal_input_blocked()
            && !self.palette_open
            && !self.grid_open
            && !self.filmstrip_open
            && !egui::Popup::is_any_open(context)
            && self.view_drag.is_none();
        let controls_have_focus = || {
            context
                .memory(egui::Memory::focused)
                .and_then(|id| context.read_response(id))
                .is_some_and(|response| {
                    ["fullscreen-controls", "compact-seek-bar"]
                        .into_iter()
                        .any(|id| {
                            response.layer_id == egui::LayerId::new(egui::Order::Middle, id.into())
                        })
                })
        };
        let was_visible = self.fullscreen_controls_visible;
        let (held, at_edge, tab, focused, outside_press) =
            context.input(|input| {
                let held = input.pointer.any_down() || input.pointer.any_released();
                let at_edge = input.pointer.hover_pos().is_some_and(|pointer| {
                    screen.contains(pointer) && pointer.y >= screen.bottom() - 48.0
                });
                let tab = input.events.iter().any(|event| matches!(event,
                    egui::Event::Key { key: egui::Key::Tab, pressed: true, modifiers, .. }
                    if !modifiers.ctrl && !modifiers.alt && !modifiers.mac_cmd && !modifiers.command
                ));
                (
                    held,
                    at_edge,
                    tab,
                    input.focused,
                    input.pointer.any_pressed() && !at_edge,
                )
            });
        if eligible
            && outside_press
            && controls_have_focus()
            && let Some(id) = context.memory(egui::Memory::focused)
        {
            context.memory_mut(|memory| memory.surrender_focus(id));
        }
        // Tab traversal can briefly have no focused widget while wrapping.
        self.fullscreen_controls_keyboard = eligible
            && focused
            && !outside_press
            && (self.fullscreen_controls_keyboard || (!held && tab) || controls_have_focus());
        // A newly shown Area needs a sizing pass before its Exit button can receive focus.
        self.fullscreen_controls_focus_requested = eligible
            && focused
            && !outside_press
            && (self.fullscreen_controls_focus_requested || (tab && !was_visible && !held));
        self.fullscreen_controls_visible = eligible
            && focused
            && (self.fullscreen_controls_keyboard
                || self.fullscreen_controls_focus_requested
                || (was_visible && (held || controls_have_focus()))
                || (!held && at_edge));
        if !self.fullscreen_controls_visible {
            return;
        }
        let rect = egui::Rect::from_min_max(
            egui::pos2(screen.left(), screen.bottom() - 30.0),
            screen.max,
        );
        egui::Area::new("fullscreen-controls".into())
            .order(egui::Order::Middle)
            .fixed_pos(rect.min)
            .default_size(rect.size())
            .movable(false)
            .constrain(false)
            .show(context, |ui| {
                ui.set_width(rect.width());
                ui.set_height(rect.height());
                let status = self.draw_status_bar(ui, actions, volume_targets);
                self.draw_seek_bar(context, status, Some(ui.layer_id()), actions);
            });
        if controls_have_focus() {
            self.fullscreen_controls_focus_requested = false;
        }
    }

    fn draw_status_bar(
        &self,
        root: &mut egui::Ui,
        actions: &mut Vec<UiAction>,
        volume_targets: &mut Vec<egui::Response>,
    ) -> egui::Rect {
        egui::Panel::bottom("status")
            .exact_size(30.0)
            .frame(chrome::bar())
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if self.fullscreen {
                        let response = chrome::button(
                            ui,
                            "▣",
                            &self.command_hint(CommandId::ToggleFullscreen, "Exit fullscreen"),
                        );
                        if self.fullscreen_controls_focus_requested && response.enabled() {
                            response.request_focus();
                        }
                        if response.clicked() {
                            actions.push(UiAction::Command(CommandId::ToggleFullscreen));
                        }
                    }
                    if self.media_kind.is_some_and(|kind| kind != MediaKind::Image) {
                        let playing = self.state == PlaybackState::Playing;
                        if chrome::button(
                            ui,
                            if playing { "Ⅱ" } else { "▶" },
                            &self.command_hint(
                                CommandId::TogglePause,
                                if playing { "Pause" } else { "Play / replay" },
                            ),
                        )
                        .clicked()
                        {
                            actions.push(UiAction::Command(CommandId::TogglePause));
                        }
                        let duration = self
                            .media_duration
                            .map(|duration| format_time(MediaTime::ZERO.saturating_add(duration)))
                            .unwrap_or_else(|| "—".into());
                        ui.label(
                            RichText::new(format!(
                                "{} / {duration}",
                                format_time(self.current_position())
                            ))
                            .size(12.0)
                            .color(chrome::MUTED),
                        );
                        if chrome::button(
                            ui,
                            "≋",
                            &self.command_hint(CommandId::ToggleTimeline, "Waveform timeline"),
                        )
                        .clicked()
                        {
                            actions.push(UiAction::Command(CommandId::ToggleTimeline));
                        }
                        let volume = ui
                            .add_sized(
                                [40.0, 24.0],
                                egui::Label::new(
                                    RichText::new(format!(
                                        "{:.0}%",
                                        self.edit_state().volume * 100.0
                                    ))
                                    .size(12.0)
                                    .color(chrome::MUTED),
                                ),
                            )
                            .on_hover_text("Volume · wheel to adjust (playback and export)");
                        volume_targets.push(volume);
                    } else if self.media_kind == Some(MediaKind::Image) {
                        if chrome::button(
                            ui,
                            "◫",
                            &self.command_hint(CommandId::ToggleReadingMode, "Reading mode"),
                        )
                        .clicked()
                        {
                            actions.push(UiAction::Command(CommandId::ToggleReadingMode));
                        }
                        if self.image_view.selection.is_some()
                            && ui
                                .selectable_label(self.image_view.crop_preview, "Crop preview")
                                .clicked()
                        {
                            actions.push(UiAction::Command(CommandId::ToggleCropPreview));
                        }
                    }
                    let mut details = Vec::new();
                    if let Some((_, intent)) = &self.pending_folder {
                        details.push(match intent {
                            FolderIntent::Open => "Opening folder".into(),
                            FolderIntent::Refresh(_) => "Loading order".into(),
                        });
                    }
                    if let Some(image) = &self.image {
                        let (width, height) = image.dimensions();
                        let zoom = match self.image_view.zoom {
                            ZoomMode::Fit => "Fit".into(),
                            ZoomMode::Actual => "100%".into(),
                            ZoomMode::Custom(scale) => {
                                format!("{:.*}%", if scale < 0.1 { 2 } else { 0 }, scale * 100.0)
                            }
                        };
                        details.push(zoom);
                        details.push(format!("{} {width}×{height}", image.decoded.format));
                    } else if self.session.is_some() {
                        let edit = self.edit_state();
                        details.push(format!("{:.2}×", edit.rate));
                        if edit.trim_start.is_some() || edit.trim_end.is_some() {
                            details.push("Trim (T)".into());
                        }
                    }
                    if let Some(path) = &self.path {
                        if let Some(snapshot) = &self.folder_snapshot
                            && let Some(index) = snapshot.item_index(path)
                        {
                            details.push(format!("{} / {}", index + 1, snapshot.items.len()));
                            if snapshot.source == FolderSnapshotSource::NaturalNameFallback {
                                details.push("Name fallback".into());
                            }
                        }
                        if let Ok(metadata) = path.metadata() {
                            details.push(format_size(metadata.len()));
                        }
                    }
                    if self.tabs.active().is_some_and(|tab| {
                        self.edits.get(&tab.id).is_some_and(EditHistory::is_dirty)
                    }) {
                        details.push("Unsaved".into());
                    }
                    let info = details.join("   ");
                    let remaining = ui.available_width();
                    let info_width = if info.is_empty() {
                        0.0
                    } else {
                        (remaining * 0.52).min(410.0)
                    };
                    let path_width = (remaining - info_width - 6.0).max(0.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(path_width, 24.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(path_width);
                            let (text, color, tooltip) =
                                if let Some((message, _)) = &self.status_message {
                                    (
                                        message.clone(),
                                        Color32::from_rgb(216, 205, 167),
                                        message.clone(),
                                    )
                                } else if let Some(path) = &self.path {
                                    let parent = path
                                        .parent()
                                        .and_then(Path::file_name)
                                        .map(|name| name.to_string_lossy())
                                        .unwrap_or_default();
                                    (
                                        format!("{parent}\\{}", display_name(path)),
                                        chrome::MUTED,
                                        path.display().to_string(),
                                    )
                                } else {
                                    (
                                        "Open a file or folder to begin".into(),
                                        chrome::MUTED,
                                        format!("Shortcuts: {}", self.shortcut_path.display()),
                                    )
                                };
                            ui.add(
                                egui::Label::new(RichText::new(text).size(12.0).color(color))
                                    .truncate(),
                            )
                            .on_hover_text(tooltip);
                        },
                    );
                    ui.allocate_ui_with_layout(
                        egui::vec2(info_width, 24.0),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(info_width);
                            let source = self
                                .folder_snapshot
                                .as_ref()
                                .map_or("", |snapshot| snapshot_source(snapshot.source));
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&info).size(12.0).color(chrome::MUTED),
                                )
                                .truncate(),
                            )
                            .on_hover_text(format!("{info}\n{source}"));
                        },
                    );
                });
            })
            .response
            .rect
    }

    fn draw_seek_bar(
        &mut self,
        context: &egui::Context,
        status: egui::Rect,
        parent: Option<egui::LayerId>,
        actions: &mut Vec<UiAction>,
    ) {
        if self.media_kind == Some(MediaKind::Image) {
            let Some(snapshot) = &self.folder_snapshot else {
                return;
            };
            let images: Vec<_> = snapshot.items_of_kind(MediaKind::Image).collect();
            let Some(index) = images
                .iter()
                .position(|item| Some(item.path.as_path()) == self.path.as_deref())
            else {
                return;
            };
            let progress = if images.len() > 1 {
                index as f32 / (images.len() - 1) as f32
            } else {
                0.0
            };
            let enabled = !self.modal_input_blocked();
            let (response, commit) = seekbar::show(context, status, progress, parent, enabled);
            let value = seekbar::value_input(
                &response,
                "Image position",
                (index + 1) as f64,
                1.0..=images.len() as f64,
                1.0,
                enabled,
            );
            if let Some(pointer) = response.interact_pointer_pos().or(response.hover_pos()) {
                let target =
                    seekbar::item_index(seekbar::ratio(response.rect, pointer.x), images.len());
                if !self.filmstrip_open
                    && !self.palette_open
                    && !self.grid_open
                    && !self.modal_input_blocked()
                {
                    let paths: Vec<_> = if self.reading_mode {
                        snapshot
                            .reading_items(
                                &images[target].path,
                                self.reading_settings.page_count,
                                self.reading_settings.reversed,
                            )
                            .into_iter()
                            .map(|item| item.path.clone())
                            .collect()
                    } else {
                        vec![images[target].path.clone()]
                    };
                    let position = if paths.len() > 1 {
                        format!("{}–{}", target + 1, target + paths.len())
                    } else {
                        (target + 1).to_string()
                    };
                    self.image_seek_preview_active = true;
                    self.filmstrip.show_seek_preview(
                        &response,
                        seekbar::ratio(response.rect, pointer.x),
                        &paths,
                        &format!(
                            "{position} / {}  {}",
                            images.len(),
                            display_name(&images[target].path)
                        ),
                        self.reading_settings.axis,
                    );
                }
            }
            if let Some(target) = value.map(|value| value.round() as usize - 1).or_else(|| {
                commit.map(|pointer| {
                    seekbar::item_index(seekbar::ratio(response.rect, pointer.x), images.len())
                })
            }) && target != index
            {
                actions.push(UiAction::OpenMedia(images[target].path.clone(), false));
            }
            return;
        }
        if (self.timeline_open && !self.fullscreen)
            || self.session.is_none()
            || matches!(self.state, PlaybackState::Loading | PlaybackState::Faulted)
        {
            return;
        }
        let Some(duration) = self.media_duration.filter(|duration| !duration.is_zero()) else {
            return;
        };
        let progress = (self.current_position().as_seconds_f64() / duration.as_secs_f64())
            .clamp(0.0, 1.0) as f32;
        let enabled = !self.modal_input_blocked();
        let (response, commit) = seekbar::show(context, status, progress, parent, enabled);
        let value = seekbar::value_input(
            &response,
            "Playback position (seconds)",
            self.current_position().as_seconds_f64(),
            0.0..=duration.as_secs_f64(),
            KEYBOARD_SEEK_STEP.as_secs_f64(),
            enabled,
        );
        if let Some(pointer) = response.interact_pointer_pos().or(response.hover_pos()) {
            let ratio = seekbar::ratio(response.rect, pointer.x);
            self.draw_seek_preview(&response, ratio, duration);
        }
        if let Some(value) = value {
            actions.push(UiAction::Seek(media_time(Duration::from_secs_f64(value))));
        } else if let Some(pointer) = commit {
            let ratio = seekbar::ratio(response.rect, pointer.x);
            actions.push(UiAction::Seek(media_time(duration.mul_f32(ratio))));
        }
    }

    fn draw_seek_preview(&mut self, response: &egui::Response, ratio: f32, duration: Duration) {
        let bucket = ((ratio * 20.0).floor() as u64).min(19);
        if self.media_kind == Some(MediaKind::Video)
            && self
                .hover_thumbnail
                .as_ref()
                .is_none_or(|(cached, _)| *cached != bucket)
        {
            self.load_hover_thumbnail(duration.mul_f64((bucket as f64 + 0.5) / 20.0), bucket);
        }
        seekbar::preview_tooltip(response, ratio).show(|ui| {
            if let Some((cached, texture)) = &self.hover_thumbnail
                && self.media_kind == Some(MediaKind::Video)
                && *cached == bucket
            {
                // Reserve space above the track for the caption, frame and tooltip gap.
                let height = (response.rect.top() - response.ctx.viewport_rect().top() - 40.0)
                    .clamp(1.0, 108.0);
                ui.add(
                    egui::Image::new((texture.id(), texture.size_vec2()))
                        .max_size(egui::vec2(160.0, height)),
                );
            }
            if self.failed_thumbnails.contains(&bucket) {
                ui.label("Thumbnail unavailable");
            }
            ui.monospace(format_time(media_time(duration.mul_f32(ratio))));
        });
    }

    fn draw_timeline(&mut self, root: &mut egui::Ui, actions: &mut Vec<UiAction>) {
        if !self.timeline_open
            || !matches!(self.media_kind, Some(MediaKind::Video | MediaKind::Audio))
        {
            return;
        }
        let max_height = root.available_height() * 0.6;
        egui::Panel::bottom("timeline")
            .default_size(96.0)
            .size_range(64.0_f32.min(max_height)..=max_height)
            .resizable(true)
            .show(root, |ui| {
                let rect = ui.available_rect_before_wrap();
                if let Some(waveform) = &self.waveform {
                    ui.painter().image(
                        waveform.id(),
                        rect,
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                        Color32::from_white_alpha(150),
                    );
                } else {
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        "No audio waveform",
                        egui::TextStyle::Body.resolve(ui.style()),
                        Color32::GRAY,
                    );
                }
                let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
                let duration = self.media_duration.unwrap_or_default();
                if duration.is_zero() {
                    return;
                }
                let progress = (self.current_position().as_seconds_f64() / duration.as_secs_f64())
                    .clamp(0.0, 1.0) as f32;
                let x = egui::lerp(rect.x_range(), progress);
                ui.painter()
                    .vline(x, rect.y_range(), (2.0, Color32::LIGHT_BLUE));
                let edit = self.edit_state();
                let tab = self.tabs.active().map(|tab| tab.id);
                let trim_operations = trim::timeline(
                    ui,
                    rect,
                    &edit,
                    media_time(duration),
                    self.state == PlaybackState::Paused
                        && !edit.playback_range().contains(self.current_position()),
                    ui.id().with(("trim-controls", tab, self.path.as_ref())),
                    ui.id().with((
                        tab,
                        self.generation,
                        edit.trim_start.map(MediaTime::as_nanoseconds),
                        edit.trim_end.map(MediaTime::as_nanoseconds),
                    )),
                );
                if let Some(tab) = tab {
                    actions.extend(
                        trim_operations
                            .into_iter()
                            .map(|operation| UiAction::TrimEndpoint(tab, operation)),
                    );
                }
                let enabled = !self.modal_input_blocked()
                    && !matches!(self.state, PlaybackState::Loading | PlaybackState::Faulted);
                let value = seekbar::value_input(
                    &response,
                    "Playback position (seconds)",
                    self.current_position().as_seconds_f64(),
                    0.0..=duration.as_secs_f64(),
                    KEYBOARD_SEEK_STEP.as_secs_f64(),
                    enabled,
                );
                if response.has_focus() && enabled {
                    ui.painter().rect_stroke(
                        rect.shrink(1.0),
                        0.0,
                        ui.visuals().selection.stroke,
                        egui::StrokeKind::Inside,
                    );
                }
                if let Some(value) = value {
                    actions.push(UiAction::Seek(media_time(Duration::from_secs_f64(value))));
                } else if let Some(position) = timeline_input::seek_commit(&response) {
                    let ratio = seekbar::ratio(rect, position.x);
                    actions.push(UiAction::Seek(media_time(duration.mul_f32(ratio))));
                }
                let Some(position) = response.interact_pointer_pos().or(response.hover_pos())
                else {
                    return;
                };
                let ratio = ((position.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                self.draw_seek_preview(&response, ratio, duration);
            });
    }

    fn draw_audio_playlist(&mut self, ui: &mut egui::Ui, actions: &mut Vec<UiAction>) {
        let enabled = !self.modal_input_blocked()
            && !self.palette_open
            && !self.grid_open
            && !self.filmstrip_open
            && !egui::Popup::is_any_open(ui.ctx());
        if !enabled {
            let opacity = ui.opacity();
            ui.disable();
            ui.set_opacity(opacity);
        }
        if let Some(path) = self.playlist.show(
            ui,
            self.folder_snapshot.as_ref(),
            self.path.as_deref(),
            enabled,
        ) {
            actions.push(UiAction::OpenMedia(path, false));
        }
    }

    fn draw_command_palette(&mut self, context: &egui::Context, actions: &mut Vec<UiAction>) {
        let commands = self.command_context();
        let (chosen, close) = self.palette.show(context, commands, &self.shortcuts);
        if let Some(command) = chosen {
            actions.push(UiAction::Command(command));
        }
        if close {
            self.cancel_command_overlay();
        }
    }

    fn cancel_command_overlay(&mut self) {
        self.palette_open = false;
        self.grid_open = false;
        if let Some(id) = self.command_overlay_return_focus.take()
            && let Some(context) = &self.ui_context
        {
            context.memory_mut(|memory| memory.request_focus(id));
        }
        self.request_redraw();
    }

    fn handle_ui_action(&mut self, action: UiAction) {
        if self.pending_dialog.is_some() || self.native_prompt.is_some() {
            return;
        }
        if self.modal_input_blocked()
            && !match action {
                UiAction::ResolveGuard(_) => {
                    self.pending_guard.is_some() && self.export_error.is_none()
                }
                UiAction::DismissExportError => self.export_error.is_some(),
                UiAction::CancelExport => {
                    self.active_export.is_some() && self.export_error.is_none()
                }
                _ => false,
            }
        {
            return;
        }
        match action {
            UiAction::Minimize => {
                if let Some(window) = &self.window {
                    window.set_minimized(true);
                }
            }
            UiAction::Maximize => {
                if let Some(window) = &self.window {
                    window.set_maximized(!window.is_maximized());
                }
            }
            UiAction::CloseWindow => self.request_guarded(GuardedAction::Exit),
            UiAction::DragWindow => {
                if let Some(window) = &self.window {
                    let _ = window.drag_window();
                }
            }
            UiAction::ResizeWindow(direction) => {
                if let Some(window) = &self.window {
                    let _ = window.drag_resize_window(direction);
                }
            }
            UiAction::Command(command) => self.dispatch(command),
            UiAction::ActivateTab(id) => self.activate_tab(id),
            UiAction::ReorderTab(id, gap) => {
                self.tabs.reorder(id, gap);
                self.request_redraw();
            }
            UiAction::TrimEndpoint(id, operation) => {
                if !self.modal_input_blocked() && self.tabs.active().is_some_and(|tab| tab.id == id)
                {
                    self.push_edit(operation);
                }
            }
            UiAction::CloseTab(id) => self.request_guarded(GuardedAction::CloseTab(id)),
            UiAction::DetachTab(id) => self.request_guarded(GuardedAction::DetachTab(id)),
            UiAction::OpenMedia(path, force_new) => {
                if force_new {
                    self.open_external(path, true);
                } else {
                    self.request_guarded(GuardedAction::Navigate(path));
                }
            }
            UiAction::Seek(target) => self.seek_to(target),
            UiAction::Volume(id, volume) => {
                if self.tabs.active().is_some_and(|tab| tab.id == id) {
                    self.push_edit(EditOperation::SetVolume(volume));
                }
            }
            UiAction::ResolveGuard(decision) => self.resolve_guard(decision),
            UiAction::CancelExport => {
                if let Some(export) = &mut self.active_export {
                    export.cancelling = true;
                    export.job.cancel();
                }
                self.request_redraw();
            }
            UiAction::DismissExportError => {
                self.export_error = None;
                self.request_redraw();
            }
        }
    }

    fn dispatch(&mut self, command: CommandId) {
        self.cancel_shortcut_prefix();
        self.cancel_view_drag();
        if self.pending_dialog.is_some() || self.native_prompt.is_some() {
            return;
        }
        if command == CommandId::ToggleFilmstrip && !self.filmstrip_open {
            let focus = if self.palette_open || self.grid_open {
                self.command_overlay_return_focus
            } else {
                self.ui_context
                    .as_ref()
                    .and_then(|context| context.memory(egui::Memory::focused))
            };
            self.filmstrip_return_focus = focus.map(|id| (self.media_generation, id));
        }
        self.command_overlay_return_focus = if matches!(
            command,
            CommandId::ToggleCommandPalette | CommandId::ToggleGridMenu
        ) {
            if self.palette_open || self.grid_open {
                self.command_overlay_return_focus
            } else {
                self.ui_context
                    .as_ref()
                    .and_then(|context| context.memory(|memory| memory.focused()))
            }
        } else {
            None
        };
        self.palette_open = false;
        match command {
            CommandId::ToggleFullscreen => self.set_fullscreen(!self.fullscreen),
            CommandId::OpenFile => {
                self.begin_dialog(FileDialogKind::OpenFile, DialogIntent::OpenFile);
            }
            CommandId::OpenFolder => {
                self.begin_dialog(FileDialogKind::OpenFolder, DialogIntent::OpenFolder);
            }
            CommandId::CloseTab => {
                if let Some(id) = self.tabs.active().map(|tab| tab.id) {
                    self.request_guarded(GuardedAction::CloseTab(id));
                }
            }
            CommandId::NextTab => self.cycle_tab(true),
            CommandId::PreviousTab => self.cycle_tab(false),
            CommandId::TogglePause => self.toggle_pause(),
            CommandId::SeekBackward => self.seek_relative(false),
            CommandId::SeekForward => self.seek_relative(true),
            CommandId::PreviousMedia => self.navigate(false, false),
            CommandId::NextMedia => self.navigate(true, false),
            CommandId::PreviousSameKind | CommandId::PreviousImage => self.navigate(false, true),
            CommandId::NextSameKind | CommandId::NextImage => self.navigate(true, true),
            CommandId::FirstImage => self.navigate_image_boundary(false),
            CommandId::LastImage => self.navigate_image_boundary(true),
            CommandId::ToggleFilmstrip => {
                if !self.filmstrip_open {
                    self.refresh_folder_snapshot();
                    self.filmstrip.focus_current();
                    self.grid_open = false;
                    self.filmstrip_open = true;
                } else {
                    self.close_filmstrip();
                }
                self.request_redraw();
            }
            CommandId::ToggleCommandPalette => {
                self.grid_open = false;
                self.palette_open = !self.palette_open;
                self.palette.reset();
                self.request_redraw();
            }
            CommandId::ToggleGridMenu => {
                if self.grid_open {
                    self.cancel_command_overlay();
                } else {
                    self.grid_open = true;
                }
                self.request_redraw();
            }
            CommandId::ReloadShortcuts => match shortcuts::load() {
                Ok((bindings, path)) => {
                    self.shortcuts = bindings;
                    self.shortcut_path = path;
                    match grid::load() {
                        Ok((layouts, grid_path)) => {
                            self.grid_layouts = layouts;
                            self.grid_path = grid_path;
                            self.set_status("Keyboard shortcuts and grid reloaded".into());
                        }
                        Err(error) => self.set_status(format!("Grid reload failed: {error}")),
                    }
                }
                Err(error) => self.set_status(format!("Shortcut reload failed: {error}")),
            },
            CommandId::ZoomIn => self.zoom_image(1.25),
            CommandId::ZoomOut => self.zoom_image(0.8),
            CommandId::ActualSize => {
                self.image_view.actual_size();
                self.request_redraw();
            }
            CommandId::FitToWindow => {
                self.image_view.fit();
                self.request_redraw();
            }
            CommandId::ClearSelection => {
                self.image_view.selection = None;
                self.image_view.crop_preview = false;
                self.request_redraw();
            }
            CommandId::SelectAll => {
                if self.reading_mode {
                    return;
                }
                let size = match self.media_kind {
                    Some(MediaKind::Image) => {
                        self.image.as_ref().map(ImagePresentation::dimensions)
                    }
                    Some(MediaKind::Video) => self
                        .session
                        .as_ref()
                        .and_then(PlaybackSession::video_geometry)
                        .map(|(w, h, _)| (w, h)),
                    _ => None,
                };
                if size.is_none() {
                    self.set_status("Wait for media to load before selecting".into());
                    return;
                }
                self.image_view.selection = Some(UnitRect::FULL);
                self.image_view.crop_preview = false;
                if let Some(context) = &self.ui_context {
                    selection::focus_first(context, self.selection_identity());
                }
                self.request_redraw();
            }
            CommandId::ToggleCropPreview => {
                if self.image_view.selection.is_some() {
                    self.image_view.crop_preview = !self.image_view.crop_preview;
                    self.image_view.fit();
                    self.request_redraw();
                } else {
                    self.set_status("Drag on the image to create a crop selection".into());
                }
            }
            CommandId::ToggleReadingMode => {
                self.reading_mode = !self.reading_mode;
                self.rebuild_reading_pages();
                self.request_redraw();
            }
            CommandId::IncreaseReadingPages => {
                self.reading_settings.increase_pages();
                self.rebuild_reading_pages();
                self.request_redraw();
            }
            CommandId::DecreaseReadingPages => {
                self.reading_settings.decrease_pages();
                self.rebuild_reading_pages();
                self.request_redraw();
            }
            CommandId::ToggleReadingAxis => {
                self.reading_settings.toggle_axis();
                self.request_redraw();
            }
            CommandId::ReverseReadingOrder => {
                self.reading_settings.reversed = !self.reading_settings.reversed;
                self.request_redraw();
            }
            CommandId::Undo => self.undo_edit(false),
            CommandId::Redo => self.undo_edit(true),
            CommandId::ApplyCrop => {
                let size = match self.media_kind {
                    Some(MediaKind::Image) => {
                        self.image.as_ref().map(ImagePresentation::dimensions)
                    }
                    Some(MediaKind::Video) => self
                        .session
                        .as_ref()
                        .and_then(PlaybackSession::video_geometry)
                        .map(|(w, h, _)| (w, h)),
                    _ => None,
                };
                if let Some(size) = size {
                    self.crop_selection(size);
                } else {
                    self.set_status("Wait for media to load before cropping".into());
                }
            }
            CommandId::RotateClockwise => self.push_visual_edit(EditOperation::RotateClockwise),
            CommandId::RotateCounterclockwise => {
                self.push_visual_edit(EditOperation::RotateCounterclockwise)
            }
            CommandId::FlipHorizontal => self.push_visual_edit(EditOperation::FlipHorizontal),
            CommandId::FlipVertical => self.push_visual_edit(EditOperation::FlipVertical),
            CommandId::SetTrimStart => {
                self.push_edit(EditOperation::SetTrimStart(self.current_position()))
            }
            CommandId::SetTrimEnd => {
                self.push_edit(EditOperation::SetTrimEnd(self.current_position()))
            }
            CommandId::VolumeDown => {
                let volume = (self.edit_state().volume - 0.1).max(0.0);
                self.push_edit(EditOperation::SetVolume(volume));
            }
            CommandId::VolumeUp => {
                let volume = (self.edit_state().volume + 0.1).min(2.0);
                self.push_edit(EditOperation::SetVolume(volume));
            }
            CommandId::ToggleMute => {
                let volume =
                    if self.edit_state().volume == 0.0 {
                        self.tabs
                            .active()
                            .and_then(|tab| self.edits.get(&tab.id))
                            .and_then(|history| {
                                history.operations().iter().rev().find_map(|operation| {
                                    match *operation {
                                        EditOperation::SetVolume(volume) if volume > 0.0 => {
                                            Some(volume.min(2.0))
                                        }
                                        _ => None,
                                    }
                                })
                            })
                            .unwrap_or(1.0)
                    } else {
                        0.0
                    };
                self.push_edit(EditOperation::SetVolume(volume));
            }
            CommandId::RateDown => {
                let rate = (self.edit_state().rate - 0.25).max(0.25);
                self.push_edit(EditOperation::SetRate(rate));
            }
            CommandId::RateUp => {
                let rate = (self.edit_state().rate + 0.25).min(4.0);
                self.push_edit(EditOperation::SetRate(rate));
            }
            CommandId::ResetRate => self.push_edit(EditOperation::SetRate(1.0)),
            CommandId::Save => {
                self.export_current(false, None);
            }
            CommandId::ExportAs => {
                self.export_current(true, None);
            }
            CommandId::ToggleTimeline => {
                if self.fullscreen {
                    self.set_fullscreen(false);
                    self.timeline_open = true;
                } else {
                    self.timeline_open = !self.timeline_open;
                }
                if self.timeline_open && self.waveform.is_none() {
                    self.load_waveform();
                }
                self.request_redraw();
            }
            CommandId::ToggleHardwareEncode => {
                self.prefer_hardware_encode = !self.prefer_hardware_encode;
                self.set_status(if self.prefer_hardware_encode {
                    "Hardware encode preferred; software remains the fallback".into()
                } else {
                    "Software encode selected".into()
                });
            }
        }
    }

    fn crop_selection(&mut self, source_size: (u32, u32)) {
        let (Some(selection), Some(kind)) = (self.image_view.selection, self.media_kind) else {
            self.set_status("Drag on the media to create a crop selection".into());
            return;
        };
        let transform = self.visual_transform(source_size);
        let size = (transform.size.0 as u32, transform.size.1 as u32);
        let Some(crop) = PixelCrop::from_selection(selection, size, kind) else {
            self.set_status("Cannot crop this selection; video needs at least 16 × 16 px".into());
            return;
        };
        // The pinned default H.264 encoder rejects dimensions smaller than one macroblock.
        if kind == MediaKind::Video && (crop.width < 16 || crop.height < 16) {
            self.set_status("Video crop needs at least 16 × 16 px; selection kept".into());
            return;
        }
        if (crop.width, crop.height) != size {
            self.push_edit(EditOperation::Crop(crop));
        }
        self.image_view.selection = None;
        self.image_view.crop_preview = false;
        self.image_view.fit();
        self.set_status(format!(
            "Crop {} × {} px (source unchanged)",
            crop.width, crop.height
        ));
    }

    fn visual_transform(&self, size: (u32, u32)) -> ImageTransform {
        let operations = self
            .tabs
            .active()
            .and_then(|tab| self.edits.get(&tab.id))
            .map_or(&[][..], EditHistory::operations);
        if self.media_kind == Some(MediaKind::Video) {
            let orientation = self
                .session
                .as_ref()
                .and_then(PlaybackSession::video_orientation)
                .unwrap_or_default();
            ImageTransform::with_orientation(size, orientation, operations)
        } else {
            ImageTransform::new(size, operations)
        }
    }

    fn push_visual_edit(&mut self, operation: EditOperation) {
        self.push_edit(operation);
        self.image_view.selection = None;
        self.image_view.crop_preview = false;
        self.image_view.fit();
    }

    fn push_edit(&mut self, operation: EditOperation) {
        if matches!(
            operation,
            EditOperation::SetTrimStart(_) | EditOperation::SetTrimEnd(_)
        ) && !self.prepare_trim_edit(operation)
        {
            return;
        }
        let (Some(tab), Some(kind)) = (self.tabs.active(), self.media_kind) else {
            return;
        };
        if self.edits.entry(tab.id).or_default().push(operation, kind) {
            self.sync_playback_edits();
            self.set_status(match operation {
                EditOperation::SetVolume(_) => format!(
                    "Volume {:.0}% · playback and export (source unchanged)",
                    self.edit_state().volume * 100.0
                ),
                EditOperation::SetRate(_) => format!(
                    "Rate {:.2}× · playback and export (source unchanged)",
                    self.edit_state().rate
                ),
                EditOperation::SetTrimStart(_) | EditOperation::SetTrimEnd(_) => trim::label(
                    &self.edit_state(),
                    media_time(self.media_duration.unwrap_or_default()),
                )
                .unwrap_or_default(),
                _ => "Edit added (source unchanged)".into(),
            });
            self.refresh_title();
            self.request_redraw();
        }
    }

    fn prepare_trim_edit(&mut self, operation: EditOperation) -> bool {
        if !self
            .media_kind
            .is_some_and(|kind| operation.applies_to(kind))
        {
            return false;
        }
        let Some(duration) = self
            .media_duration
            .filter(|duration| !duration.is_zero())
            .map(media_time)
        else {
            self.set_status("Wait for the media duration before setting trim".into());
            return false;
        };
        let previous = self.edit_state();
        let mut next = previous.clone();
        match operation {
            EditOperation::SetTrimStart(time) => next.trim_start = Some(time),
            EditOperation::SetTrimEnd(time) => next.trim_end = Some(time),
            _ => return false,
        }
        if !next.trim_is_valid(Some(duration)) {
            self.set_status("Trim unchanged: use 0 ≤ start < end ≤ source duration".into());
            return false;
        }
        if previous.trim_start.unwrap_or(MediaTime::ZERO)
            == next.trim_start.unwrap_or(MediaTime::ZERO)
            && previous.trim_end.unwrap_or(duration) == next.trim_end.unwrap_or(duration)
        {
            self.set_status("Trim unchanged: this endpoint is already selected".into());
            return false;
        }
        self.set_fullscreen(false);
        self.timeline_open = true;
        self.load_waveform();
        true
    }

    fn undo_edit(&mut self, redo: bool) {
        let Some(id) = self.tabs.active().map(|tab| tab.id) else {
            return;
        };
        let previous = self.edit_state();
        let changed = self
            .edits
            .get_mut(&id)
            .is_some_and(|history| if redo { history.redo() } else { history.undo() });
        if changed {
            self.sync_playback_edits();
            let next = self.edit_state();
            if (previous.trim_start, previous.trim_end) != (next.trim_start, next.trim_end) {
                self.set_status(
                    trim::label(&next, media_time(self.media_duration.unwrap_or_default()))
                        .unwrap_or_else(|| "Trim cleared · source playback".into()),
                );
            }
            self.image_view.selection = None;
            self.image_view.crop_preview = false;
            self.image_view.fit();
            self.refresh_title();
            self.request_redraw();
        }
    }

    fn sync_playback_edits(&mut self) {
        let state = self.edit_state();
        if let Some(session) = &mut self.session {
            session.set_volume(state.volume);
            if session.rate() != state.rate || session.range() != state.playback_range() {
                self.seek_to(self.current_position());
            }
        }
    }

    fn playback_rate(&self) -> f32 {
        self.session.as_ref().map_or(1.0, PlaybackSession::rate)
    }

    fn edit_state(&self) -> towavue_core::EditState {
        self.tabs
            .active()
            .and_then(|tab| self.edits.get(&tab.id))
            .map(EditHistory::state)
            .unwrap_or_default()
    }

    fn begin_dialog(&mut self, kind: FileDialogKind, intent: DialogIntent) -> bool {
        if self.pending_dialog.is_some() || self.native_prompt.is_some() {
            return false;
        }
        let Some(window) = self.window.clone() else {
            self.set_status("The file dialog's owner window is unavailable.".into());
            return false;
        };
        if matches!(self.pending_folder, Some((_, FolderIntent::Open))) {
            self.folder_order.request(None);
            self.pending_folder = None;
        }
        let notify = Arc::clone(&self.notify);
        match pick_path(window, kind, move |result| {
            notify(AppEvent::DialogFinished(result))
        }) {
            Ok(()) => {
                self.pending_dialog = Some(intent);
                self.request_redraw();
                true
            }
            Err(error) => {
                self.set_status(error.to_string());
                false
            }
        }
    }

    fn refresh_pointer_position(&mut self) {
        if let (Some(window), Some(state)) = (&self.window, &mut self.ui_state)
            && let Some((x, y)) = cursor_position_in_window(window)
        {
            // Native dialog/focus transitions can omit CursorEntered for a stationary pointer.
            let _ = state.on_window_event(
                window,
                &WindowEvent::CursorMoved {
                    device_id: winit::event::DeviceId::dummy(),
                    position: winit::dpi::PhysicalPosition::new(f64::from(x), f64::from(y)),
                },
            );
        }
    }

    fn finish_dialog(&mut self, result: Result<Option<PathBuf>, DialogError>) {
        self.refresh_pointer_position();
        let Some(intent) = self.pending_dialog.take() else {
            return;
        };
        match intent {
            DialogIntent::OpenFile => match result {
                Ok(Some(path)) => self.open_external(path, false),
                Ok(None) => {}
                Err(error) => self.set_status(error.to_string()),
            },
            DialogIntent::OpenFolder => match result {
                Ok(Some(path)) => self.open_folder_path(path),
                Ok(None) => {}
                Err(error) => self.set_status(error.to_string()),
            },
            DialogIntent::Export {
                tab,
                source,
                kind,
                continuation,
            } => match result {
                Ok(Some(target))
                    if self.tabs.active().is_some_and(|active| {
                        active.id == tab && active.target.current_path() == source
                    }) =>
                {
                    self.start_export(tab, source, kind, target, continuation);
                }
                other => {
                    self.pending_guard = continuation;
                    match other {
                        Err(error) => self.set_status(error.to_string()),
                        Ok(Some(_)) => self.set_status(
                            "The source changed while choosing an export path; nothing was exported."
                                .into(),
                        ),
                        Ok(None) => {}
                    }
                }
            },
        }
        self.request_redraw();
    }

    fn export_current(&mut self, force_dialog: bool, continuation: Option<GuardedAction>) -> bool {
        if self.active_export.is_some() || self.pending_dialog.is_some() {
            self.set_status(
                "An export is already running. Wait for it or choose Cancel export.".into(),
            );
            return false;
        }
        let Some((id, source, kind)) = self.tabs.active().map(|tab| {
            (
                tab.id,
                tab.target.current_path().to_owned(),
                tab.target.media_kind(),
            )
        }) else {
            return false;
        };
        let target = if !force_dialog {
            self.export_paths.get(&id).cloned()
        } else {
            None
        };
        match target {
            Some(target) => self.start_export(id, source, kind, target, continuation),
            None => {
                let suggested_name = export_name(&source);
                self.begin_dialog(
                    FileDialogKind::SaveFile { suggested_name },
                    DialogIntent::Export {
                        tab: id,
                        source,
                        kind,
                        continuation,
                    },
                )
            }
        }
    }

    fn start_export(
        &mut self,
        id: TabId,
        source: PathBuf,
        kind: MediaKind,
        target: PathBuf,
        continuation: Option<GuardedAction>,
    ) -> bool {
        let operations = self
            .edits
            .get(&id)
            .map(|history| history.operations().to_vec())
            .unwrap_or_default();
        let request = ExportRequest {
            source,
            target: target.clone(),
            kind,
            operations,
            hardware_encode: self.prefer_hardware_encode,
        };
        let notify = Arc::clone(&self.notify);
        match ExportJob::start(request.clone(), move |event| {
            notify(AppEvent::Export(event))
        }) {
            Ok(job) => {
                self.active_export = Some(ActiveExport {
                    job,
                    tab: id,
                    request,
                    encoded: Duration::ZERO,
                    cancelling: false,
                    continuation,
                });
                self.refresh_title();
                self.request_redraw();
                true
            }
            Err(error) => {
                self.export_error = Some(error.to_string());
                self.pending_guard = continuation;
                self.request_redraw();
                false
            }
        }
    }

    fn handle_export_event(&mut self, event: ExportEvent) {
        match event {
            ExportEvent::Progress(time) => {
                if let Some(export) = &mut self.active_export {
                    export.encoded = time;
                }
            }
            ExportEvent::Finished(result) => {
                let Some(export) = self.active_export.take() else {
                    return;
                };
                match result {
                    Ok(outcome) => {
                        if self.tabs.tabs().iter().any(|tab| {
                            tab.id == export.tab
                                && tab.target.current_path() == export.request.source
                        }) {
                            self.export_paths
                                .insert(export.tab, export.request.target.clone());
                            self.edits
                                .entry(export.tab)
                                .or_default()
                                .mark_exported(&export.request.operations);
                        }
                        let encoder = if outcome.used_hardware_encoder {
                            "hardware"
                        } else {
                            "software"
                        };
                        self.set_status(format!(
                            "{} {} ({encoder} encode)",
                            if export.cancelling {
                                "Export completed before cancellation; automatic leaving cancelled:"
                            } else {
                                "Exported"
                            },
                            export.request.target.display()
                        ));
                        self.refresh_title();
                        if !export.cancelling
                            && let Some(action) = export.continuation
                        {
                            if self.native_prompt.is_some() {
                                self.pending_guard = Some(action);
                            } else {
                                self.request_guarded(action);
                            }
                        }
                    }
                    Err(error) => {
                        if matches!(error, ExportError::Cancelled) {
                            self.set_status(error.to_string());
                        } else {
                            self.export_error = Some(error.to_string());
                        }
                        self.pending_guard = export.continuation.or(self.pending_guard.take());
                    }
                }
            }
        }
        self.refresh_title();
        self.request_redraw();
    }

    fn zoom_image(&mut self, factor: f32) {
        let Some(image) = &self.image else { return };
        let Some(context) = &self.ui_context else {
            return;
        };
        if self.image_viewport.min_elem() <= 0.0 {
            return;
        }
        let mut transform = self.visual_transform(image.dimensions());
        transform.crop(self.image_view.preview_region());
        self.image_view.zoom_by(
            factor,
            (transform.size.0 as u32, transform.size.1 as u32),
            (self.image_viewport * context.pixels_per_point()).into(),
        );
        self.request_redraw();
    }

    fn activate_tab(&mut self, id: TabId) {
        if self.tabs.active().is_some_and(|tab| tab.id == id) {
            return;
        }
        if self.tabs.activate(id)
            && let Some((path, kind)) = self.tabs.active().map(|tab| {
                (
                    tab.target.current_path().to_owned(),
                    tab.target.media_kind(),
                )
            })
        {
            self.load_path(path, kind);
        }
    }

    fn request_guarded(&mut self, action: GuardedAction) {
        if matches!(&action, GuardedAction::Navigate(path) if self.path.as_ref() == Some(path)) {
            return;
        }
        self.cancel_shortcut_prefix();
        self.cancel_view_drag();
        if self.pending_dialog.is_some() || self.native_prompt.is_some() {
            self.set_status("Close the file dialog before leaving.".into());
            return;
        }
        if matches!(self.pending_folder, Some((_, FolderIntent::Open))) {
            self.folder_order.request(None);
            self.pending_folder = None;
        }
        if let Some(export) = &self.active_export {
            let affects_export = match &action {
                GuardedAction::CloseTab(id) | GuardedAction::DetachTab(id) => *id == export.tab,
                GuardedAction::Navigate(_) => {
                    self.tabs.active().is_some_and(|tab| tab.id == export.tab)
                }
                GuardedAction::Exit => true,
            };
            if affects_export {
                if self.renderer.is_none() {
                    self.open_native_prompt(FallbackPrompt::ExportBusy);
                }
                self.set_status(
                    "Export is in progress. Wait for it or choose Cancel export before leaving."
                        .into(),
                );
                return;
            }
        }
        let dirty = match action {
            GuardedAction::CloseTab(id) => self
                .edits
                .get(&id)
                .is_some_and(EditHistory::is_dirty)
                .then_some(id),
            GuardedAction::DetachTab(id) => self
                .edits
                .get(&id)
                .is_some_and(EditHistory::is_dirty)
                .then_some(id),
            GuardedAction::Navigate(_) => self.tabs.active().and_then(|tab| {
                self.edits
                    .get(&tab.id)
                    .is_some_and(EditHistory::is_dirty)
                    .then_some(tab.id)
            }),
            GuardedAction::Exit => self
                .edits
                .iter()
                .find_map(|(id, history)| history.is_dirty().then_some(*id)),
        };
        if let Some(id) = dirty {
            if self.pending_guard.is_none() && self.guard_return_focus.is_none() {
                self.guard_return_focus = self
                    .tabs
                    .active()
                    .filter(|tab| tab.id == id)
                    .and(self.ui_context.as_ref())
                    .and_then(|context| context.memory(|memory| memory.focused()))
                    .map(|focus| (id, focus));
            }
            if self.tabs.active().is_none_or(|tab| tab.id != id) {
                self.activate_tab(id);
            }
            self.pending_guard = Some(action);
            self.request_redraw();
        } else {
            self.perform_guarded(action);
        }
    }

    fn resolve_guard(&mut self, decision: GuardDecision) {
        let Some(action) = self.pending_guard.take() else {
            return;
        };
        match decision {
            GuardDecision::Save => {
                if !self.export_current(false, Some(action.clone())) {
                    self.pending_guard = Some(action);
                }
            }
            GuardDecision::Discard => self.perform_guarded(action),
            GuardDecision::Cancel => self.request_redraw(),
        }
    }

    fn perform_guarded(&mut self, action: GuardedAction) {
        self.guard_return_focus = None;
        match action {
            GuardedAction::CloseTab(id) => self.close_tab_unchecked(id),
            GuardedAction::DetachTab(id) => self.detach_tab_unchecked(id),
            GuardedAction::Navigate(path) => self.navigate_to_unchecked(path),
            GuardedAction::Exit => {
                self.exit_requested = true;
                self.request_redraw();
            }
        }
    }

    fn detach_tab_unchecked(&mut self, id: TabId) {
        let Some(path) = self
            .tabs
            .tabs()
            .iter()
            .find(|tab| tab.id == id)
            .map(|tab| tab.target.current_path().to_owned())
        else {
            return;
        };
        let result = std::env::current_exe()
            .and_then(|executable| std::process::Command::new(executable).arg(&path).spawn())
            .map(|_| ());
        match result {
            Ok(()) => self.close_tab_unchecked(id),
            Err(error) => self.set_status(format!("Could not detach tab: {error}")),
        }
    }

    fn close_tab_unchecked(&mut self, id: TabId) {
        let was_active = self.tabs.active().is_some_and(|tab| tab.id == id);
        if self.tabs.close(id).is_none() {
            return;
        }
        self.edits.remove(&id);
        self.export_paths.remove(&id);
        if !was_active {
            self.request_redraw();
            return;
        }
        if let Some((path, kind)) = self.tabs.active().map(|tab| {
            (
                tab.target.current_path().to_owned(),
                tab.target.media_kind(),
            )
        }) {
            self.load_path(path, kind);
        } else {
            self.session.take();
            self.playback_error = None;
            self.image_generation = self.image_loader.request(Vec::new());
            self.image_loading = false;
            self.image_error = None;
            self.image = None;
            self.reading_pages.clear();
            self.path = None;
            self.playlist.clear();
            self.media_kind = None;
            self.timeline_open = false;
            self.waveform = None;
            self.media_duration = None;
            self.hover_thumbnail = None;
            self.media_generation = self.media_generation.wrapping_add(1);
            self.duration_worker.clear();
            self.waveform_worker.clear();
            self.thumbnail_worker.clear();
            self.failed_thumbnails.clear();
            self.waveform_loading = false;
            self.thumbnail_loading = None;
            self.image_view = ImageViewState::default();
            self.folder_snapshot = None;
            self.folder_watcher = None;
            self.folder_order.request(None);
            self.pending_folder = None;
            self.pending_time = None;
            self.clock = None;
            self.decode_finished = false;
            self.audio_drained = true;
            self.metrics_recorded = false;
            self.pending_seek_started = None;
            self.seek_latencies.clear();
            self.drift_samples.clear();
            self.status_message = None;
            self.state = PlaybackState::Paused;
            self.refresh_title();
            self.request_redraw();
        }
    }

    fn cycle_tab(&mut self, forward: bool) {
        let id = {
            let tabs = self.tabs.tabs();
            if tabs.is_empty() {
                return;
            }
            let current = self
                .tabs
                .active()
                .and_then(|active| tabs.iter().position(|tab| tab.id == active.id))
                .unwrap_or(0);
            let index = if forward {
                (current + 1) % tabs.len()
            } else {
                (current + tabs.len() - 1) % tabs.len()
            };
            tabs[index].id
        };
        self.activate_tab(id);
    }

    fn navigate(&mut self, forward: bool, same_kind: bool) {
        let (Some(snapshot), Some(path), Some(kind)) = (
            self.folder_snapshot.as_ref(),
            self.path.as_ref(),
            self.media_kind,
        ) else {
            return;
        };
        let paths = snapshot
            .items
            .iter()
            .filter(|item| !same_kind || item.kind == kind)
            .map(|item| item.path.clone())
            .collect::<Vec<_>>();
        let Some(current) = paths.iter().position(|candidate| candidate == path) else {
            return;
        };
        let index = if forward {
            (current + 1) % paths.len()
        } else {
            (current + paths.len() - 1) % paths.len()
        };
        self.request_guarded(GuardedAction::Navigate(paths[index].clone()));
    }

    fn navigate_image_boundary(&mut self, last: bool) {
        let (Some(snapshot), Some(path)) = (&self.folder_snapshot, &self.path) else {
            return;
        };
        if !snapshot
            .items_of_kind(MediaKind::Image)
            .any(|item| &item.path == path)
        {
            return;
        }
        let target = if last {
            snapshot.items_of_kind(MediaKind::Image).last()
        } else {
            snapshot.items_of_kind(MediaKind::Image).next()
        };
        if let Some(target) = target
            && &target.path != path
        {
            self.request_guarded(GuardedAction::Navigate(target.path.clone()));
        }
    }

    fn navigate_to_unchecked(&mut self, path: PathBuf) {
        let Some(kind) = MediaKind::from_path(&path) else {
            return;
        };
        if let Some(tab) = self.tabs.active_mut() {
            let id = tab.id;
            tab.target.set_current_path(path.clone(), kind);
            self.edits.insert(id, EditHistory::default());
            self.export_paths.remove(&id);
        }
        self.load_path(path, kind);
    }

    fn toggle_pause(&mut self) {
        let Some(next) = self.state.after_play_pause() else {
            return;
        };
        let range = self.edit_state().playback_range();
        let position = self.current_position();
        let restart = next == PlaybackState::Playing
            && (self.state == PlaybackState::Ended
                || self.media_duration.is_some_and(|duration| {
                    !duration.is_zero() && position >= media_time(duration)
                }));
        let target = if restart {
            range.start
        } else {
            range.play_target(position)
        };
        if restart || target != position {
            self.seek_to(target);
            if self.state == PlaybackState::Faulted {
                return;
            }
        }
        let paused = next == PlaybackState::Paused;
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if let Err(error) = session.set_paused(paused) {
            self.fail(error.to_string());
            return;
        }
        if let Some(clock) = self.clock.as_mut() {
            clock.set_paused(paused);
        }
        self.state = if paused {
            PlaybackState::Paused
        } else {
            PlaybackState::Playing
        };
        self.refresh_title();
        self.request_redraw();
    }

    fn seek_relative(&mut self, forward: bool) {
        if !matches!(
            self.state,
            PlaybackState::Playing | PlaybackState::Paused | PlaybackState::Ended
        ) || self.session.is_none()
        {
            return;
        }
        let position = self.current_position();
        let target = if forward {
            position.saturating_add(KEYBOARD_SEEK_STEP)
        } else {
            position
                .saturating_sub(KEYBOARD_SEEK_STEP)
                .max(MediaTime::ZERO)
        };
        self.seek_to(target);
    }

    fn seek_to(&mut self, target: MediaTime) {
        let end = self
            .media_duration
            .filter(|duration| !duration.is_zero())
            .map(media_time);
        let target = end
            .map_or(target, |end| target.min(end))
            .max(MediaTime::ZERO);
        let started = Instant::now();
        self.pending_seek_started = None;
        let edit = self.edit_state();
        let range = edit.playback_range();
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let pause =
            self.state == PlaybackState::Ended || end == Some(target) || !range.contains(target);
        match session.seek_with_edits(target, edit.rate, range, pause) {
            Ok(generation) => {
                if pause {
                    self.state = PlaybackState::Paused;
                }
                self.generation = generation;
                self.pending_time = None;
                self.clock = None;
                self.decode_finished = false;
                self.audio_drained = !session.has_audio();
                self.metrics_recorded = false;
                self.pending_seek_started =
                    (self.media_kind == Some(MediaKind::Video)).then_some(started);
                self.set_status(if range.contains(target) {
                    format!("Position {:.3}s", target.as_seconds_f64())
                } else {
                    "Outside trim · paused source preview; Play returns to trim start".into()
                });
                self.refresh_title();
            }
            Err(error) => self.fail(error.to_string()),
        }
    }

    fn fail_graphics_recovery(&mut self, position: MediaTime, state: PlaybackState, error: String) {
        let mut clock = PlaybackClock::new(position, self.playback_rate());
        clock.paused_at = Some(clock.wall_anchor);
        self.clock = Some(clock);
        self.queued_recovery = Some(FallbackPrompt::Recovery {
            position,
            state,
            error: error.clone(),
        });
        self.fail(error);
    }

    fn show_native_fallback(&mut self) {
        if self.pending_dialog.is_some() || self.native_prompt.is_some() {
            return;
        }
        let prompt = if let Some(recovery) = self.queued_recovery.take() {
            recovery
        } else if self.export_error.is_some() {
            FallbackPrompt::ExportError
        } else if self.pending_guard.is_some() {
            FallbackPrompt::Guard
        } else {
            return;
        };
        self.open_native_prompt(prompt);
    }

    fn open_native_prompt(&mut self, prompt: FallbackPrompt) {
        if self.pending_dialog.is_some() || self.native_prompt.is_some() {
            return;
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        let (message, buttons) = match &prompt {
            FallbackPrompt::Recovery { error, .. } => (
                format!("Graphics could not be restored.\n\n{error}\n\nRetry: restore graphics at the saved playback position.\nCancel: keep all edits. Press Alt+F4 afterward to export or close."),
                PromptButtons::RetryCancel,
            ),
            FallbackPrompt::Guard => {
                let name = self.path.as_deref().map(display_name).unwrap_or_default();
                let discard = if matches!(self.pending_guard, Some(GuardedAction::Exit)) {
                    "discard ALL unsaved edits and exit"
                } else { "discard this file's edits and continue" };
                (format!("Graphics are unavailable. Unsaved edits: {name}\n\nYes: export this file before continuing.\nNo: {discard}.\nCancel: keep edits and stop this action.\n\nExport never overwrites the source."), PromptButtons::YesNoCancel)
            }
            FallbackPrompt::ExportError => (
                format!("Export failed. Your edits are retained.\n\n{}", self.export_error.as_deref().unwrap_or_default()),
                PromptButtons::Ok,
            ),
            FallbackPrompt::ExportBusy => (
                "Export may finish while this dialog is open.\n\nYes: request cancellation and stop automatic leaving.\nNo or Cancel: keep waiting.\n\nIf already saved, the output is kept. Otherwise existing files and edits are retained.\nThe window title reports export progress.".into(),
                PromptButtons::YesNoCancel,
            ),
        };
        let notify = Arc::clone(&self.notify);
        match show_prompt(window, message, buttons, move |result| {
            notify(AppEvent::PromptFinished(result))
        }) {
            Ok(()) => self.native_prompt = Some(prompt),
            Err(error) => eprintln!("towavue: native graphics fallback failed: {error}"),
        }
    }

    fn finish_native_prompt(&mut self, result: Result<PromptResponse, DialogError>) {
        self.refresh_pointer_position();
        let Some(prompt) = self.native_prompt.take() else {
            return;
        };
        let response = match result {
            Ok(response) => response,
            Err(error) => {
                eprintln!("towavue: native graphics fallback failed: {error}");
                return;
            }
        };
        match prompt {
            FallbackPrompt::Recovery {
                position, state, ..
            } if response == PromptResponse::Retry => {
                self.state = state;
                self.recover_graphics_device(position);
            }
            FallbackPrompt::Recovery { .. } => {}
            FallbackPrompt::Guard => self.resolve_guard(match response {
                PromptResponse::Yes => GuardDecision::Save,
                PromptResponse::No => GuardDecision::Discard,
                _ => GuardDecision::Cancel,
            }),
            FallbackPrompt::ExportError => self.export_error = None,
            FallbackPrompt::ExportBusy => {
                if response == PromptResponse::Yes {
                    if let Some(export) = &mut self.active_export {
                        export.job.cancel();
                        export.cancelling = true;
                    } else {
                        self.pending_guard = None;
                        self.set_status(
                            "Export already finished; automatic leaving cancelled.".into(),
                        );
                    }
                }
            }
        }
        if self.active_export.is_none()
            && self.pending_dialog.is_none()
            && self.export_error.is_none()
            && let Some(action) = self.pending_guard.take()
        {
            self.request_guarded(action);
        }
        self.request_redraw();
    }

    fn restored_ui_textures(
        &self,
        context: &egui::Context,
    ) -> Vec<(egui::TextureId, egui::epaint::ImageDelta)> {
        let font_options = context
            .tex_manager()
            .read()
            .meta(egui::TextureId::default())
            .expect("font texture exists after UI layout")
            .options;
        let mut textures = vec![(
            egui::TextureId::default(),
            egui::epaint::ImageDelta::full(context.fonts(|fonts| fonts.image()), font_options),
        )];
        for image in self.image.iter().chain(
            self.reading_pages
                .iter()
                .filter_map(|page| page.as_ref().ok()),
        ) {
            textures.push((
                image.texture.id(),
                egui::epaint::ImageDelta::full(
                    color_image(&image.decoded.frames[image.frame_index]),
                    TextureOptions::LINEAR,
                ),
            ));
        }
        textures
    }

    fn recover_graphics_device(&mut self, position: MediaTime) {
        self.image_texture_cache.entries.clear();
        let state = self.state;
        self.queued_recovery = None;
        self.pending_seek_started = None;
        if let Some(session) = &mut self.session {
            session.suspend_for_graphics_recovery();
        }
        if let Some(renderer) = self.renderer.take() {
            renderer.release_surface();
        }
        let Some(window) = &self.window else {
            self.fail("window was unavailable during graphics recovery".to_owned());
            return;
        };
        let mut renderer = match FrameRenderer::new(window) {
            Ok(renderer) => renderer,
            Err(error) => {
                self.fail_graphics_recovery(
                    position,
                    state,
                    format!("D3D11 device recovery failed: {error}"),
                );
                return;
            }
        };
        let size = window.inner_size();
        if let Err(error) = renderer.resize_surface(size.width, size.height) {
            renderer.release_surface();
            self.fail_graphics_recovery(
                position,
                state,
                format!("D3D11 surface recovery failed: {error}"),
            );
            return;
        }
        let graphics_device = renderer.graphics_device();
        if let Some(context) = &self.ui_context {
            context.input_mut(|input| input.max_texture_side = renderer.max_texture_side());
        }
        self.renderer = Some(renderer);
        self.restore_ui_textures = true;
        self.playback_error = None;
        self.refresh_title();
        self.filmstrip.clear();
        self.waveform = None;
        self.hover_thumbnail = None;
        if self.timeline_open {
            self.load_waveform();
        }
        self.request_redraw();
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if self.state == PlaybackState::Ended || !session.range().contains(position) {
            if let Err(error) = session.set_paused(true) {
                self.fail(error.to_string());
                return;
            }
            self.state = PlaybackState::Paused;
        }
        match session.replace_graphics_device(graphics_device, position) {
            Ok(generation) => {
                self.generation = generation;
                self.pending_time = None;
                self.clock = None;
                self.decode_finished = false;
                self.audio_drained = !session.has_audio();
                self.metrics_recorded = false;
            }
            Err(error) => self.fail(format!("D3D11 pipeline recovery failed: {error}")),
        }
        self.refresh_title();
    }

    fn poll_audio(&mut self) {
        match self
            .session
            .as_ref()
            .and_then(PlaybackSession::try_audio_event)
        {
            Some(AudioOutputEvent::Drained) => {
                if let Some(position) = self
                    .session
                    .as_ref()
                    .and_then(PlaybackSession::audio_position)
                {
                    let mut clock = PlaybackClock::new(position, self.playback_rate());
                    clock.set_paused(self.state == PlaybackState::Paused);
                    self.clock = Some(clock);
                }
                self.audio_drained = true;
                self.check_eof();
            }
            Some(AudioOutputEvent::EndpointChanged) => {
                eprintln!("towavue: recovering changed or invalidated audio endpoint");
                self.seek_to(self.current_position());
                self.pending_seek_started = None;
            }
            Some(AudioOutputEvent::Failed(error)) => self.fail(error),
            None => {}
        }
    }

    fn audio_master_position(&self) -> Option<MediaTime> {
        (!self.audio_drained)
            .then(|| {
                self.session
                    .as_ref()
                    .and_then(PlaybackSession::audio_position)
            })
            .flatten()
    }

    fn current_position(&self) -> MediaTime {
        let position = self
            .audio_master_position()
            .or_else(|| self.clock.as_ref().map(PlaybackClock::position))
            .or_else(|| {
                self.session
                    .as_ref()
                    .and_then(PlaybackSession::audio_position)
            })
            .or(self.pending_time)
            .or_else(|| self.session.as_ref().map(PlaybackSession::target))
            .unwrap_or(MediaTime::ZERO);
        let position = self
            .media_duration
            .filter(|duration| !duration.is_zero())
            .map_or(position, |duration| position.min(media_time(duration)));
        self.session
            .as_ref()
            .and_then(PlaybackSession::range_end)
            .map_or(position, |end| position.min(end))
    }

    fn check_eof(&mut self) {
        if matches!(self.state, PlaybackState::Playing | PlaybackState::Paused)
            && self.decode_finished
            && self.pending_time.is_none()
            && self.audio_drained
        {
            if let Some(end) = self.session.as_ref().and_then(PlaybackSession::range_end) {
                if self.clock.is_none() {
                    let mut clock =
                        PlaybackClock::new(self.current_position(), self.playback_rate());
                    clock.set_paused(self.state == PlaybackState::Paused);
                    self.clock = Some(clock);
                }
                if self.current_position() < end {
                    return;
                }
                self.clock = Some(PlaybackClock::new(end, self.playback_rate()));
            }
            if let Some(clock) = &mut self.clock {
                clock.set_paused(true);
            }
            self.state = PlaybackState::Ended;
            if !self.metrics_recorded {
                if let Some(session) = &self.session {
                    let metrics = session.metrics();
                    eprintln!(
                        "towavue: adapter={:08x}:{:08x} hardware_frames={} cpu_transfers={} presented={} dropped={}",
                        metrics.adapter_luid.high_part,
                        metrics.adapter_luid.low_part,
                        metrics.hardware_frame_count,
                        metrics.cpu_transfer_count,
                        metrics.presented_frame_count,
                        metrics.dropped_frame_count
                    );
                }
                self.report_timing_metrics();
                self.metrics_recorded = true;
            }
            self.refresh_title();
            self.request_redraw();
        }
    }

    fn handle_render_error(&mut self, error: RenderError) {
        let error = self
            .renderer
            .as_ref()
            .and_then(FrameRenderer::device_removed_reason)
            .map_or(error, RenderError::DeviceRemoved);
        match error {
            RenderError::DeviceRemoved(reason) => {
                eprintln!("towavue: recovering removed D3D11 device: {reason}");
                self.recover_graphics_device(self.current_position());
            }
            other => {
                self.session.take();
                self.fail(other.to_string());
            }
        }
    }

    fn fail(&mut self, error: String) {
        self.pending_seek_started = None;
        eprintln!("towavue: {error}");
        self.playback_error = Some(error.clone());
        self.set_status(error);
        self.state = PlaybackState::Faulted;
        self.refresh_title();
    }

    fn set_status(&mut self, message: String) {
        self.status_message = Some((message, Instant::now()));
        self.request_redraw();
    }

    fn record_seek_presentation(&mut self, media_drawn: bool) {
        if media_drawn && let Some(started) = self.pending_seek_started.take() {
            let latency = started.elapsed();
            self.seek_latencies.push(latency);
            eprintln!(
                "towavue: seek_latency_ms={:.3}",
                latency.as_secs_f64() * 1000.0
            );
        }
    }

    fn report_timing_metrics(&self) {
        if !self.seek_latencies.is_empty() {
            eprintln!(
                "towavue: seek_p95_ms={:.3}",
                percentile_95(&self.seek_latencies).as_secs_f64() * 1000.0
            );
        }
        if !self.drift_samples.is_empty() {
            eprintln!(
                "towavue: av_drift_p95_ms={:.3} av_drift_max_ms={:.3}",
                percentile_95(&self.drift_samples).as_secs_f64() * 1000.0,
                self.drift_samples
                    .iter()
                    .max()
                    .copied()
                    .unwrap_or_default()
                    .as_secs_f64()
                    * 1000.0
            );
        }
    }

    fn refresh_title(&self) {
        if let Some(window) = &self.window {
            window.set_title(&self.title());
        }
    }

    fn title(&self) -> String {
        let mut name = self
            .path
            .as_deref()
            .map(display_name)
            .unwrap_or_else(|| "Welcome".to_owned());
        if self
            .tabs
            .active()
            .and_then(|tab| self.edits.get(&tab.id))
            .is_some_and(EditHistory::is_dirty)
        {
            name.push_str(" *");
        }
        if self.renderer.is_none()
            && let Some(export) = &self.active_export
        {
            return format!(
                "{name} — towavue (Exporting {:.1}s)",
                export.encoded.as_secs_f64()
            );
        }
        format!("{name} — towavue ({:?})", self.state)
    }

    fn command_hint(&self, command: CommandId, title: &str) -> String {
        self.shortcuts.get(command).map_or_else(
            || title.to_owned(),
            |sequence| format!("{title} ({sequence})"),
        )
    }

    fn command_context(&self) -> CommandContext {
        CommandContext {
            media_kind: self.media_kind,
            palette_open: self.palette_open,
            filmstrip_open: self.filmstrip_open,
            reading_mode: self.reading_mode,
        }
    }

    fn modal_input_blocked(&self) -> bool {
        self.pending_dialog.is_some()
            || self.native_prompt.is_some()
            || self.pending_guard.is_some()
            || self.export_error.is_some()
            || self
                .active_export
                .as_ref()
                .is_some_and(|export| export.continuation.is_some())
    }

    fn set_fullscreen(&mut self, enabled: bool) {
        if enabled == self.fullscreen {
            return;
        }
        self.fullscreen = enabled;
        self.fullscreen_controls_visible = false;
        self.fullscreen_controls_focus_requested = false;
        self.fullscreen_controls_keyboard = false;
        self.viewing_cursor.activity();
        if let Some(window) = &self.window {
            let monitor = window.current_monitor();
            if enabled {
                // A maximized Win32 client otherwise retains its work-area inset in fullscreen.
                self.fullscreen_was_maximized = window.is_maximized();
                if self.fullscreen_was_maximized {
                    window.set_maximized(false);
                }
            }
            window.set_fullscreen(enabled.then_some(Fullscreen::Borderless(monitor)));
            if !enabled && self.fullscreen_was_maximized {
                window.set_maximized(true);
                self.fullscreen_was_maximized = false;
            }
        }
        self.set_status(if enabled {
            "Fullscreen — Tab or bottom edge for controls · Escape to return".into()
        } else {
            "Windowed view".into()
        });
    }

    fn close_filmstrip(&mut self) {
        self.filmstrip_open = false;
        let origin = self
            .filmstrip_return_focus
            .take()
            .filter(|(generation, _)| *generation == self.media_generation);
        if let Some(context) = &self.ui_context {
            if let Some((_, id)) = origin {
                context.memory_mut(|memory| memory.request_focus(id));
            } else if self.fullscreen {
                self.fullscreen_controls_keyboard = true;
                self.fullscreen_controls_focus_requested = true;
            } else {
                context.data_mut(|data| data.insert_temp("filmstrip-return-tab".into(), true));
            }
        }
        self.request_redraw();
    }

    fn dismiss_overlay_or_fullscreen(&mut self) -> bool {
        if self.modal_input_blocked() {
            return false;
        }
        if self.palette_open || self.grid_open {
            self.cancel_command_overlay();
            true
        } else if self.filmstrip_open {
            self.close_filmstrip();
            true
        } else if self.cancel_view_drag() {
            true
        } else if self.fullscreen {
            self.set_fullscreen(false);
            true
        } else {
            false
        }
    }

    fn owns_tab_key(&self, stroke: &KeyStroke) -> bool {
        if stroke.key != Key::Tab
            || self.palette_open
            || self.grid_open
            || self.modal_input_blocked()
            || self
                .ui_context
                .as_ref()
                .is_some_and(egui::Popup::is_any_open)
        {
            return false;
        }
        let modified = stroke.modifiers.control || stroke.modifiers.alt || stroke.modifiers.logo;
        if self.filmstrip_open && !modified {
            return true;
        }
        if !modified
            && self
                .ui_context
                .as_ref()
                .is_some_and(egui::Context::egui_wants_keyboard_input)
        {
            return false;
        }
        let mut entered = self.entered_shortcut.clone();
        entered.push(stroke.clone());
        let context = self.command_context();
        self.shortcuts.resolve(&entered, context) != ShortcutMatch::None
            || self
                .shortcuts
                .resolve(std::slice::from_ref(stroke), context)
                != ShortcutMatch::None
    }

    fn owns_seek_shortcut(&self, stroke: &KeyStroke) -> bool {
        !self.palette_open
            && !self.grid_open
            && !self.filmstrip_open
            && !self.modal_input_blocked()
            && self.ui_context.as_ref().is_some_and(|context| {
                (seekbar::has_value_focus(context) || selection::has_focus(context))
                    && !egui::Popup::is_any_open(context)
            })
            && stroke.key != Key::Tab
            && !(self.prefix_started.is_none()
                && stroke.modifiers == Modifiers::default()
                && matches!(
                    stroke.key,
                    Key::ArrowLeft | Key::ArrowRight | Key::Home | Key::End
                ))
            && !(self.prefix_started.is_none()
                && stroke.modifiers == Modifiers::default()
                && matches!(stroke.key, Key::ArrowUp | Key::ArrowDown)
                && self.ui_context.as_ref().is_some_and(selection::has_focus))
    }

    fn owns_focused_shortcut(&self, stroke: &KeyStroke) -> bool {
        if self.owns_seek_shortcut(stroke) {
            return true;
        }
        let Some(context) = &self.ui_context else {
            return false;
        };
        if self.palette_open
            || self.grid_open
            || self.filmstrip_open
            || self.modal_input_blocked()
            || egui::Popup::is_any_open(context)
            || !context.egui_wants_keyboard_input()
            || context.text_edit_focused()
            || seekbar::has_value_focus(context)
            || selection::has_focus(context)
            || stroke.key == Key::Tab
        {
            return false;
        }
        self.prefix_started.is_some()
            || ((stroke.modifiers.control
                || stroke.modifiers.alt
                || stroke.modifiers.logo
                || !matches!(
                    stroke.key,
                    Key::Space
                        | Key::ArrowLeft
                        | Key::ArrowRight
                        | Key::ArrowUp
                        | Key::ArrowDown
                        | Key::Home
                        | Key::End
                        | Key::Escape
                ))
                && self
                    .shortcuts
                    .resolve(std::slice::from_ref(stroke), self.command_context())
                    != ShortcutMatch::None)
    }

    fn process_key(&mut self, event: &KeyEvent) {
        if self.modal_input_blocked() {
            return;
        }
        if event.state != ElementState::Pressed || event.repeat {
            return;
        }
        self.expire_shortcut_prefix();
        if event.logical_key == WinitKey::Named(NamedKey::Escape) && self.prefix_started.is_some() {
            self.cancel_shortcut_prefix();
            return;
        }
        if event.logical_key == WinitKey::Named(NamedKey::Escape)
            && self.dismiss_overlay_or_fullscreen()
        {
            return;
        }
        if let Some(index) = self.grid_key_index(event.physical_key)
            && let Some(kind) = self.media_kind
        {
            let command = self.grid_layouts.get(kind)[index];
            let enabled = command_definitions()
                .iter()
                .find(|definition| definition.id == command)
                .is_some_and(|definition| definition.is_enabled(self.command_context()));
            if enabled {
                if !matches!(
                    command,
                    CommandId::ToggleGridMenu
                        | CommandId::ToggleCommandPalette
                        | CommandId::ToggleFilmstrip
                ) {
                    self.grid_open = false;
                }
                self.dispatch(command);
            }
            return;
        }
        if self.filmstrip_open
            && event.logical_key == WinitKey::Named(NamedKey::Tab)
            && !self
                .modifiers
                .intersects(ModifiersState::CONTROL | ModifiersState::ALT | ModifiersState::SUPER)
        {
            self.navigate(!self.modifiers.shift_key(), false);
            self.filmstrip.focus_current();
            return;
        }
        let Some(stroke) = self.key_stroke(event) else {
            return;
        };
        self.process_shortcut(stroke);
    }

    fn process_shortcut(&mut self, stroke: KeyStroke) {
        self.entered_shortcut.push(stroke.clone());
        let context = self.command_context();
        let mut matched = self.shortcuts.resolve(&self.entered_shortcut, context);
        if matched == ShortcutMatch::None {
            self.cancel_shortcut_prefix();
            self.entered_shortcut.push(stroke);
            matched = self.shortcuts.resolve(&self.entered_shortcut, context);
        }
        match matched {
            ShortcutMatch::Command(command) => {
                self.dispatch(command);
            }
            ShortcutMatch::Prefix => {
                self.set_status(format!(
                    "{} …",
                    self.entered_shortcut
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(" ")
                ));
                self.prefix_started = self.status_message.as_ref().map(|(_, shown)| *shown);
            }
            ShortcutMatch::None => self.cancel_shortcut_prefix(),
        }
    }

    fn cancel_shortcut_prefix(&mut self) {
        let started = self.prefix_started.take();
        self.entered_shortcut.clear();
        if started.is_some() {
            // Only the notice created with this prefix belongs to the cancellation.
            if self
                .status_message
                .as_ref()
                .is_some_and(|(_, shown)| Some(*shown) == started)
            {
                self.status_message = None;
            }
            self.request_redraw();
        }
    }

    fn expire_shortcut_prefix(&mut self) {
        if self
            .prefix_started
            .is_some_and(|started| started.elapsed() >= PREFIX_TIMEOUT)
        {
            self.cancel_shortcut_prefix();
        }
    }

    fn grid_key_index(&self, physical_key: PhysicalKey) -> Option<usize> {
        if !self.grid_open
            || self.palette_open
            || self.modal_input_blocked()
            || self
                .modifiers
                .intersects(ModifiersState::CONTROL | ModifiersState::ALT | ModifiersState::SUPER)
        {
            return None;
        }
        grid::key_index(physical_key)
    }

    fn key_stroke(&self, event: &KeyEvent) -> Option<KeyStroke> {
        let (key, shift_consumed) = match &event.logical_key {
            WinitKey::Character(value) if value.chars().count() == 1 => {
                let character = value.chars().next()?.to_ascii_lowercase();
                (Key::Character(character), character == '+')
            }
            WinitKey::Named(NamedKey::Space) => (Key::Space, false),
            WinitKey::Named(NamedKey::ArrowLeft) => (Key::ArrowLeft, false),
            WinitKey::Named(NamedKey::ArrowRight) => (Key::ArrowRight, false),
            WinitKey::Named(NamedKey::ArrowUp) => (Key::ArrowUp, false),
            WinitKey::Named(NamedKey::ArrowDown) => (Key::ArrowDown, false),
            WinitKey::Named(NamedKey::Tab) => (Key::Tab, false),
            WinitKey::Named(NamedKey::Escape) => (Key::Escape, false),
            WinitKey::Named(NamedKey::F11) => (Key::F11, false),
            WinitKey::Named(NamedKey::Home) => (Key::Home, false),
            WinitKey::Named(NamedKey::End) => (Key::End, false),
            _ => return None,
        };
        Some(KeyStroke {
            modifiers: Modifiers {
                control: self.modifiers.control_key(),
                alt: self.modifiers.alt_key(),
                shift: self.modifiers.shift_key() && !shift_consumed,
                logo: self.modifiers.super_key(),
            },
            key,
        })
    }

    fn schedule(&mut self, event_loop: &ActiveEventLoop) {
        if self.exit_requested {
            // Windows winit waits once after AboutToWait, even when it requested exit.
            event_loop.set_control_flow(ControlFlow::Poll);
            event_loop.exit();
            return;
        }
        self.poll_audio();
        self.check_eof();
        let now = Instant::now();
        let was_hidden = self.viewing_cursor.hidden;
        let pointer_ready = self
            .window
            .as_ref()
            .is_some_and(|window| window.has_focus())
            && self
                .ui_state
                .as_ref()
                .is_some_and(|state| state.is_pointer_in_window());
        self.viewing_cursor
            .update(now, self.cursor_can_hide(pointer_ready));
        if was_hidden != self.viewing_cursor.hidden {
            self.request_redraw();
        }
        if self.ui_repaint_at.is_some_and(|deadline| deadline <= now) {
            self.ui_repaint_at = None;
            self.request_redraw();
        }
        let mut image_changed = self
            .image
            .as_mut()
            .is_some_and(|image| image.advance_animation(now));
        for image in self
            .reading_pages
            .iter_mut()
            .filter_map(|page| page.as_mut().ok())
        {
            image_changed |= image.advance_animation(now);
        }
        if image_changed {
            self.request_redraw();
        }
        if self
            .folder_watcher
            .as_mut()
            .is_some_and(|(_, watcher)| watcher.try_changed())
        {
            self.refresh_folder_snapshot();
            self.request_redraw();
        }
        if self
            .status_message
            .as_ref()
            .is_some_and(|(_, shown)| shown.elapsed() >= STATUS_MESSAGE_DURATION)
        {
            self.status_message = None;
            self.request_redraw();
        }
        self.expire_shortcut_prefix();
        self.discard_late_video_frames();
        if self.state == PlaybackState::Playing {
            if let (Some(window), Some(presentation_time)) = (&self.window, self.pending_time) {
                let due_at = if let Some(audio_position) = self.audio_master_position() {
                    let threshold = audio_position.saturating_add(VIDEO_EARLY_TOLERANCE);
                    let wait = presentation_time
                        .as_nanoseconds()
                        .saturating_sub(threshold.as_nanoseconds())
                        .max(0) as u64;
                    Instant::now()
                        + Duration::from_nanos(wait)
                            .div_f64(f64::from(self.playback_rate()))
                            .min(AUDIO_EVENT_POLL_INTERVAL)
                } else if let Some(clock) = &self.clock {
                    clock.due_at(presentation_time)
                } else {
                    Instant::now()
                };
                let due_at = self.ui_repaint_at.map_or(due_at, |ui| due_at.min(ui));
                if due_at <= Instant::now() {
                    window.request_redraw();
                    event_loop.set_control_flow(ControlFlow::Wait);
                    return;
                }
                event_loop.set_control_flow(ControlFlow::WaitUntil(due_at));
                return;
            }
            if !self.audio_drained {
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    self.ui_repaint_at
                        .map_or(Instant::now() + AUDIO_EVENT_POLL_INTERVAL, |ui| {
                            ui.min(Instant::now() + AUDIO_EVENT_POLL_INTERVAL)
                        }),
                ));
                return;
            }
        }
        if let Some(deadline) = self.idle_wakeup(now) {
            event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }

    fn idle_wakeup(&self, now: Instant) -> Option<Instant> {
        let next_image_frame = self
            .image
            .iter()
            .chain(
                self.reading_pages
                    .iter()
                    .filter_map(|page| page.as_ref().ok()),
            )
            .filter_map(|image| image.next_frame_at)
            .min();
        let folder_poll = self
            .folder_watcher
            .is_some()
            .then_some(now + FOLDER_EVENT_POLL_INTERVAL);
        let status_expiry = self
            .status_message
            .as_ref()
            .map(|(_, shown)| *shown + STATUS_MESSAGE_DURATION);
        let prefix_expiry = self.prefix_started.map(|started| started + PREFIX_TIMEOUT);
        [
            folder_poll,
            next_image_frame,
            self.ui_repaint_at,
            status_expiry,
            prefix_expiry,
            self.viewing_cursor.deadline,
            (self.state == PlaybackState::Playing
                && self.decode_finished
                && self.pending_time.is_none()
                && self.audio_drained)
                .then(|| {
                    self.session
                        .as_ref()
                        .and_then(PlaybackSession::range_end)
                        .and_then(|end| self.clock.as_ref().map(|clock| clock.due_at(end)))
                })
                .flatten(),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    fn request_redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn cursor_can_hide(&self, pointer_ready: bool) -> bool {
        pointer_ready
            && self.fullscreen
            && !self.fullscreen_controls_visible
            && matches!(self.media_kind, Some(MediaKind::Image | MediaKind::Video))
            && !self.image_loading
            && self.image_error.is_none()
            && !self.reading_pages.iter().any(Result::is_err)
            && !matches!(self.state, PlaybackState::Loading | PlaybackState::Faulted)
            && self.view_drag.is_none()
            && !self.filmstrip_open
            && !self.palette_open
            && !self.grid_open
            && self.active_export.is_none()
            && !self.modal_input_blocked()
            && self.ui_context.as_ref().is_some_and(|context| {
                context
                    .input(|input| !input.pointer.any_down() && input.raw.hovered_files.is_empty())
            })
    }
}

fn ui_repaint_deadline(now: Instant, egui_delay: Duration, playing_audio: bool) -> Option<Instant> {
    let delay = if playing_audio {
        egui_delay.min(AUDIO_EVENT_POLL_INTERVAL)
    } else {
        egui_delay
    };
    now.checked_add(delay)
}

fn late_video_cutoff(state: PlaybackState, audio_position: Option<MediaTime>) -> Option<MediaTime> {
    (state == PlaybackState::Playing)
        .then_some(audio_position)
        .flatten()
        .map(|position| position.saturating_sub(VIDEO_LATE_TOLERANCE))
}

fn video_frame_due(
    presentation_time: MediaTime,
    deadline: Option<MediaTime>,
    paused_preview: bool,
) -> bool {
    // A paused clock cannot reach the first frame after a non-frame-aligned seek.
    paused_preview || deadline.is_none_or(|deadline| presentation_time <= deadline)
}

fn percentile_95(samples: &[Duration]) -> Duration {
    let mut ordered = samples.to_vec();
    ordered.sort_unstable();
    let index = (ordered.len() * 95).div_ceil(100).saturating_sub(1);
    ordered[index]
}

fn view_drag_button_positions(
    response: &egui::Response,
    button: egui::PointerButton,
) -> (Option<egui::Pos2>, Option<egui::Pos2>) {
    let (origin, release) = response.ctx.input(|input| {
        let mut origin = None;
        let mut release = None;
        for event in &input.events {
            if let egui::Event::PointerButton {
                pos,
                button: event_button,
                pressed,
                ..
            } = event
                && *event_button == button
            {
                if *pressed {
                    origin.get_or_insert(*pos);
                } else {
                    release = Some(*pos);
                }
            }
        }
        (origin, release)
    });
    // A complete gesture in one frame no longer has egui's held-button ownership.
    let origin = origin.filter(|origin| {
        response.enabled()
            && response.interact_rect.contains(*origin)
            && response.ctx.layer_id_at(*origin) == Some(response.layer_id)
            && response.ctx.dragged_id().is_none_or(|id| id == response.id)
    });
    (origin, release)
}

fn unit_point(point: egui::Pos2, image_rect: egui::Rect) -> UnitPoint {
    UnitPoint {
        x: ((point.x - image_rect.left()) / image_rect.width()).clamp(0.0, 1.0),
        y: ((point.y - image_rect.top()) / image_rect.height()).clamp(0.0, 1.0),
    }
}

fn bilinear_uv(corners: [UnitPoint; 4], x: f32, y: f32) -> UnitPoint {
    let top = UnitPoint {
        x: corners[0].x + (corners[1].x - corners[0].x) * x,
        y: corners[0].y + (corners[1].y - corners[0].y) * x,
    };
    let bottom = UnitPoint {
        x: corners[3].x + (corners[2].x - corners[3].x) * x,
        y: corners[3].y + (corners[2].y - corners[3].y) * x,
    };
    UnitPoint {
        x: top.x + (bottom.x - top.x) * y,
        y: top.y + (bottom.y - top.y) * y,
    }
}

fn transformed_image_mesh(
    texture: egui::TextureId,
    rect: egui::Rect,
    transform: ImageTransform,
) -> egui::Mesh {
    let mut mesh = egui::Mesh::with_texture(texture);
    let half = egui::vec2(0.5 / transform.size.0, 0.5 / transform.size.1);
    // Constant UVs in the outer half-pixel bands clamp sampling to the cropped image.
    for y in [0.0, half.y, 1.0 - half.y, 1.0] {
        for x in [0.0, half.x, 1.0 - half.x, 1.0] {
            let uv = bilinear_uv(
                transform.uv,
                x.clamp(half.x, 1.0 - half.x),
                y.clamp(half.y, 1.0 - half.y),
            );
            mesh.vertices.push(egui::epaint::Vertex {
                pos: rect.min + rect.size() * egui::vec2(x, y),
                uv: egui::pos2(uv.x, uv.y),
                color: Color32::WHITE,
            });
        }
    }
    for row in 0..3 {
        for column in 0..3 {
            let index = row * 4 + column;
            mesh.indices.extend_from_slice(&[
                index,
                index + 1,
                index + 5,
                index,
                index + 5,
                index + 4,
            ]);
        }
    }
    mesh
}

fn selection_rect(image_rect: egui::Rect, selection: UnitRect) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(
            image_rect.left() + image_rect.width() * selection.min.x,
            image_rect.top() + image_rect.height() * selection.min.y,
        ),
        egui::pos2(
            image_rect.left() + image_rect.width() * selection.max.x,
            image_rect.top() + image_rect.height() * selection.max.y,
        ),
    )
}

fn selection_edge(
    pointer: egui::Pos2,
    image_rect: egui::Rect,
    selection: UnitRect,
) -> Option<SelectionDrag> {
    let rect = selection_rect(image_rect, selection);
    let tolerance = 8.0;
    let mut candidates = Vec::new();
    if pointer.y >= rect.top() - tolerance && pointer.y <= rect.bottom() + tolerance {
        candidates.push(((pointer.x - rect.left()).abs(), SelectionDrag::Left));
        candidates.push(((pointer.x - rect.right()).abs(), SelectionDrag::Right));
    }
    if pointer.x >= rect.left() - tolerance && pointer.x <= rect.right() + tolerance {
        candidates.push(((pointer.y - rect.top()).abs(), SelectionDrag::Top));
        candidates.push(((pointer.y - rect.bottom()).abs(), SelectionDrag::Bottom));
    }
    candidates
        .into_iter()
        .filter(|(distance, _)| *distance <= tolerance)
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, edge)| edge)
}

fn paint_selection(painter: &egui::Painter, image_rect: egui::Rect, selection: UnitRect) {
    let selected = selection_rect(image_rect, selection);
    let shade = Color32::from_black_alpha(150);
    for rect in [
        egui::Rect::from_min_max(
            image_rect.min,
            egui::pos2(image_rect.right(), selected.top()),
        ),
        egui::Rect::from_min_max(
            egui::pos2(image_rect.left(), selected.bottom()),
            image_rect.max,
        ),
        egui::Rect::from_min_max(
            egui::pos2(image_rect.left(), selected.top()),
            egui::pos2(selected.left(), selected.bottom()),
        ),
        egui::Rect::from_min_max(
            egui::pos2(selected.right(), selected.top()),
            egui::pos2(image_rect.right(), selected.bottom()),
        ),
    ] {
        if rect.is_positive() {
            painter.rect_filled(rect, 0.0, shade);
        }
    }
    painter.rect_stroke(
        selected,
        0.0,
        egui::Stroke::new(1.5, Color32::WHITE),
        egui::StrokeKind::Inside,
    );
    for center in [
        selected.left_center(),
        selected.right_center(),
        selected.center_top(),
        selected.center_bottom(),
    ] {
        painter.rect_filled(
            egui::Rect::from_center_size(center, egui::vec2(7.0, 7.0)),
            1.0,
            Color32::WHITE,
        );
    }
}

fn fitted_video_rect(viewport: egui::Rect, size: (u32, u32), pixel_aspect: f32) -> egui::Rect {
    let display = egui::vec2(size.0 as f32 * pixel_aspect, size.1 as f32);
    let scale = (viewport.width() / display.x).min(viewport.height() / display.y);
    egui::Rect::from_center_size(viewport.center(), display * scale)
}

fn reading_page_rects(
    viewport: egui::Rect,
    sizes: &[egui::Vec2],
    axis: ReadingAxis,
    reversed: bool,
) -> Vec<egui::Rect> {
    if sizes.is_empty() {
        return Vec::new();
    }
    let (along, across) = match axis {
        ReadingAxis::Horizontal => (egui::vec2(1.0, 0.0), egui::vec2(0.0, 1.0)),
        ReadingAxis::Vertical => (egui::vec2(0.0, 1.0), egui::vec2(1.0, 0.0)),
    };
    let lengths: Vec<_> = sizes
        .iter()
        .map(|size| size.dot(along) / size.dot(across))
        .collect();
    let total: f32 = lengths.iter().sum();
    let spread = along * total + across;
    let scale = (viewport.width() / spread.x).min(viewport.height() / spread.y);
    let origin = viewport.center() - spread * scale * 0.5;
    let mut offset = 0.0;
    lengths
        .into_iter()
        .map(|length| {
            let position = if reversed {
                total - offset - length
            } else {
                offset
            };
            offset += length;
            egui::Rect::from_min_size(
                origin + along * position * scale,
                (along * length + across) * scale,
            )
        })
        .collect()
}

fn display_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn export_name(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|value| value.to_string_lossy())
        .unwrap_or_default();
    let extension = path
        .extension()
        .map(|value| value.to_string_lossy())
        .unwrap_or_default();
    if extension.is_empty() {
        format!("{stem}-export")
    } else {
        format!("{stem}-export.{extension}")
    }
}

fn format_time(time: MediaTime) -> String {
    let seconds = time.as_nanoseconds().max(0) as u64 / 1_000_000_000;
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

fn media_time(duration: Duration) -> MediaTime {
    MediaTime::from_nanoseconds(duration.as_nanos().min(i64::MAX as u128) as i64)
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{bytes} B")
    }
}

fn snapshot_source(source: FolderSnapshotSource) -> &'static str {
    match source {
        FolderSnapshotSource::LiveExplorerView => "Explorer live order",
        FolderSnapshotSource::PersistedShellView => "Explorer saved order",
        FolderSnapshotSource::NaturalNameFallback => "Natural-name fallback",
    }
}

impl<N> ApplicationHandler<AppEvent> for Application<N>
where
    N: Fn(AppEvent) + Send + Sync + 'static,
{
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none()
            && let Err(error) = self.start(event_loop)
        {
            eprintln!("towavue: {error}");
            event_loop.exit();
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: AppEvent) {
        self.handle_app_event(event);
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.window.as_ref().map(|window| window.id()) != Some(window_id) {
            return;
        }
        if matches!(event, WindowEvent::Focused(false)) {
            self.cancel_view_drag();
        }
        if matches!(
            event,
            WindowEvent::Focused(false)
                | WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    ..
                }
        ) {
            self.cancel_shortcut_prefix();
        }
        if matches!(
            event,
            WindowEvent::CursorMoved { .. }
                | WindowEvent::CursorEntered { .. }
                | WindowEvent::CursorLeft { .. }
                | WindowEvent::MouseInput { .. }
                | WindowEvent::MouseWheel { .. }
                | WindowEvent::KeyboardInput { .. }
                | WindowEvent::Focused(_)
                | WindowEvent::Touch(_)
        ) {
            if self.viewing_cursor.hidden {
                self.request_redraw();
            }
            self.viewing_cursor.activity();
        }
        if (self.fullscreen
            || (self.filmstrip_open
                && !self
                    .ui_context
                    .as_ref()
                    .is_some_and(egui::Popup::is_any_open))
            || self.view_drag.is_some()
            || self
                .ui_context
                .as_ref()
                .is_some_and(timeline_input::is_active))
            && !self.palette_open
            && !self.modal_input_blocked()
            && let WindowEvent::KeyboardInput { event, .. } = &event
            && event.state == ElementState::Pressed
            && event.logical_key == WinitKey::Named(NamedKey::Escape)
        {
            self.process_key(event);
            return;
        }
        // Keep bound Tab chords and non-value shortcuts out of egui's focus-wide key capture.
        if let WindowEvent::KeyboardInput {
            event,
            is_synthetic,
            ..
        } = &event
        {
            self.expire_shortcut_prefix();
            if self.key_stroke(event).is_some_and(|stroke| {
                self.owns_tab_key(&stroke) || (!is_synthetic && self.owns_focused_shortcut(&stroke))
            }) {
                self.process_key(event);
                return;
            }
        }
        if let WindowEvent::KeyboardInput { event, .. } = &event
            && self.grid_key_index(event.physical_key).is_some()
        {
            self.process_key(event);
            return;
        }
        let event_response = match (self.window.as_ref(), self.ui_state.as_mut()) {
            (Some(window), Some(state)) => Some(state.on_window_event(window, &event)),
            _ => None,
        };
        let (consumed, repaint) = event_response
            .map(|response| (response.consumed, response.repaint))
            .unwrap_or_default();
        // This event already renders below; re-queuing it would keep an idle window spinning.
        if repaint && !matches!(event, WindowEvent::RedrawRequested) {
            self.request_redraw();
        }
        match event {
            WindowEvent::CloseRequested => self.request_guarded(GuardedAction::Exit),
            WindowEvent::Focused(true) => {
                self.refresh_pointer_position();
                self.request_redraw();
            }
            WindowEvent::DroppedFile(path) => self.open_dropped_path(path),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_mut()
                    && let Err(error) = renderer.resize_surface(size.width, size.height)
                {
                    self.handle_render_error(error);
                }
                self.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput { event, .. } if !consumed => self.process_key(&event),
            WindowEvent::RedrawRequested => self.render_frame(),
            _ if consumed => self.request_redraw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.schedule(event_loop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playlist_overlays_block_background_actions_and_hover_without_changing_rows() {
        let Some(root) = isolated_test_root(
            "tests::playlist_overlays_block_background_actions_and_hover_without_changing_rows",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let paths: Vec<_> = (1..=3)
            .map(|index| root.join(format!("{index:03}.wav")))
            .collect();
        app.path = Some(paths[0].clone());
        app.media_kind = Some(MediaKind::Audio);
        app.folder_snapshot = Some(FolderSnapshot {
            folder_identity: towavue_core::ShellIdentity::new(vec![]),
            folder_path: root,
            items: paths
                .iter()
                .map(|path| towavue_core::FolderMediaItem {
                    identity: towavue_core::ShellIdentity::new(vec![]),
                    path: path.clone(),
                    kind: MediaKind::Audio,
                })
                .collect(),
            sort_columns: vec![],
            source: FolderSnapshotSource::LiveExplorerView,
            generation: 1,
            captured_at: std::time::SystemTime::now(),
        });
        let context = egui::Context::default();
        context.enable_accesskit();
        context.global_style_mut(|style| style.interaction.tooltip_delay = 0.0);
        let popup_active = std::cell::Cell::new(false);
        let frame = |app: &mut Application<_>, events| {
            if popup_active.get() {
                // This focused test has no menu widget to keep the synthetic popup open.
                egui::Popup::open_id(&context, "playlist-test-menu".into());
            }
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(480.0, 240.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| app.draw_audio_playlist(ui, &mut actions),
            );
            (output, actions)
        };
        frame(&mut app, vec![]);
        let output = frame(&mut app, vec![]).0;
        let (id, row) = output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("tree")
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("3. 003.wav"))
            .expect("row");
        let id = *id;
        let bounds = row.bounds();
        let click = || {
            egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::Click,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: id,
                data: None,
            })
        };
        for overlay in 0..5 {
            app.filmstrip_open = overlay == 0;
            app.palette_open = overlay == 1;
            app.grid_open = overlay == 2;
            app.pending_guard = (overlay == 3).then_some(GuardedAction::Exit);
            popup_active.set(overlay == 4);
            let output = frame(&mut app, vec![]).0;
            let row = &output
                .platform_output
                .accesskit_update
                .as_ref()
                .expect("tree")
                .nodes
                .iter()
                .find(|(candidate, _)| *candidate == id)
                .expect("same row")
                .1;
            assert!(
                row.is_disabled(),
                "background row must be disabled for overlay {overlay}"
            );
            assert_eq!(row.bounds(), bounds, "overlay must not change row layout");
            assert!(frame(&mut app, vec![click()]).1.is_empty());
            let pos = egui::pos2(200.0, 88.0);
            for _ in 0..3 {
                let (output, actions) = frame(&mut app, vec![egui::Event::PointerMoved(pos)]);
                assert!(actions.is_empty());
                assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == "003.wav")), "no background tooltip for overlay {overlay}");
            }
            for pressed in [true, false] {
                assert!(
                    frame(
                        &mut app,
                        vec![egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE
                        }]
                    )
                    .1
                    .is_empty()
                );
            }
            app.filmstrip_open = false;
            app.palette_open = false;
            app.grid_open = false;
            app.pending_guard = None;
            popup_active.set(false);
            egui::Popup::close_all(&context);
            frame(&mut app, vec![egui::Event::PointerGone]);
            assert!(
                frame(&mut app, vec![click()]).1 == [UiAction::OpenMedia(paths[2].clone(), false)]
            );
        }
    }

    #[test]
    fn accessibility_modal_blocks_background_and_preserves_guard_decisions() {
        let Some(root) = isolated_test_root(
            "tests::accessibility_modal_blocks_background_and_preserves_guard_decisions",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let path = root.join("image.png");
        let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
        app.path = Some(path);
        app.media_kind = Some(MediaKind::Image);
        app.state = PlaybackState::Paused;
        app.edits
            .entry(tab)
            .or_default()
            .push(EditOperation::RotateClockwise, MediaKind::Image);
        let history = app.edits[&tab].clone();
        let context = egui::Context::default();
        context.enable_accesskit();
        let frame = |app: &mut Application<_>, events| {
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960.0, 576.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| app.draw_ui(ui, &mut actions),
            );
            (
                output.platform_output.accesskit_update.expect("tree"),
                actions,
            )
        };
        let click = |target_node| {
            egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::Click,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node,
                data: None,
            })
        };
        frame(&mut app, vec![]);
        app.pending_guard = Some(GuardedAction::CloseTab(tab));
        app.palette_open = true;
        app.handle_ui_action(UiAction::Command(CommandId::ToggleReadingMode));
        assert!(
            !app.reading_mode,
            "queued background command must not bypass guard"
        );
        for _ in 0..3 {
            let (tree, actions) = frame(&mut app, vec![]);
            assert!(actions.is_empty());
            assert!(app.palette_open);
            assert!(
                !tree
                    .nodes
                    .iter()
                    .any(|(_, node)| node.label() == Some("Search commands"))
            );
            for label in ["towavue menu", "Reading mode (B)", "Close window"] {
                let (id, node) = tree
                    .nodes
                    .iter()
                    .find(|(_, node)| node.label() == Some(label))
                    .expect("background button");
                assert!(node.is_disabled(), "background {label} must be disabled");
                assert!(frame(&mut app, vec![click(*id)]).1.is_empty());
            }
        }
        let (tree, _) = frame(&mut app, vec![]);
        let discard = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Discard edits"))
            .expect("discard button")
            .0;
        app.export_error = Some("Deliberate fixture error".into());
        assert!(frame(&mut app, vec![click(discard)]).1.is_empty());
        app.handle_ui_action(UiAction::ResolveGuard(GuardDecision::Discard));
        assert_eq!(app.edits[&tab], history);
        assert!(app.pending_guard.is_some());
        app.handle_ui_action(UiAction::DismissExportError);
        let (tree, _) = frame(&mut app, vec![]);
        let cancel = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Cancel"))
            .expect("cancel button")
            .0;
        let (_, actions) = frame(&mut app, vec![click(cancel)]);
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            actions[0],
            UiAction::ResolveGuard(GuardDecision::Cancel)
        ));
        app.handle_ui_action(actions[0].clone());
        assert!(app.pending_guard.is_none());
        assert_eq!(app.edits[&tab], history);
        let (tree, _) = frame(&mut app, vec![]);
        assert!(
            tree.nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Search commands"))
        );
        assert!(
            tree.nodes
                .iter()
                .any(|(_, node)| node.label() == Some("towavue menu") && !node.is_disabled())
        );
        app.palette_open = false;
        for _ in 0..5 {
            for label in ["Close window", "Cancel"] {
                let (tree, _) = frame(&mut app, vec![]);
                let target = tree
                    .nodes
                    .iter()
                    .find(|(_, node)| node.label() == Some(label))
                    .expect("action remains available")
                    .0;
                let mut focus = click(target);
                if let egui::Event::AccessKitActionRequest(request) = &mut focus {
                    request.action = egui::accesskit::Action::Focus;
                }
                let (tree, actions) = frame(&mut app, vec![focus, click(target)]);
                assert!(tree.nodes.iter().any(|(id, _)| *id == tree.focus));
                assert_eq!(actions.len(), 1, "one {label} action per cycle");
                app.handle_ui_action(actions[0].clone());
                assert_eq!(app.pending_guard.is_some(), label == "Close window");
                assert!(!app.exit_requested);
                assert_eq!(app.edits[&tab], history);
            }
        }
    }

    #[test]
    fn accessibility_activation_and_actions_reach_existing_welcome_commands() {
        let context = egui::Context::default();
        let mut state = egui_winit::State::new(
            context.clone(),
            egui::ViewportId::ROOT,
            &winit::raw_window_handle::DisplayHandle::windows(),
            Some(1.0),
            None,
            None,
        );
        let frame = |state: &mut egui_winit::State| {
            let mut chosen = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(480.0, 300.0),
                    )),
                    events: std::mem::take(&mut state.egui_input_mut().events),
                    ..Default::default()
                },
                |ui| {
                    if let Some(command) = welcome::show(ui, &ShortcutBindings::default()) {
                        chosen.push(command);
                    }
                },
            );
            (output.platform_output.accesskit_update, chosen)
        };
        assert!(frame(&mut state).0.is_none());
        for _ in 0..2 {
            handle_accesskit_window_event(
                &mut state,
                accesskit_winit::WindowEvent::InitialTreeRequested,
            );
            let (tree, chosen) = frame(&mut state);
            assert!(chosen.is_empty());
            let tree = tree.expect("requested accessibility tree");
            for (label, command) in [
                ("Open File…", CommandId::OpenFile),
                ("Open Folder…", CommandId::OpenFolder),
            ] {
                let (target, node) = tree
                    .nodes
                    .iter()
                    .find(|(_, node)| node.label() == Some(label))
                    .expect("named Welcome button");
                assert_eq!(node.role(), egui::accesskit::Role::Button);
                assert!(node.supports_action(egui::accesskit::Action::Click));
                handle_accesskit_window_event(
                    &mut state,
                    accesskit_winit::WindowEvent::ActionRequested(egui::accesskit::ActionRequest {
                        action: egui::accesskit::Action::Click,
                        target_tree: egui::accesskit::TreeId::ROOT,
                        target_node: *target,
                        data: None,
                    }),
                );
                assert_eq!(frame(&mut state).1, [command]);
                assert!(frame(&mut state).1.is_empty());
            }
            handle_accesskit_window_event(
                &mut state,
                accesskit_winit::WindowEvent::AccessibilityDeactivated,
            );
            assert!(frame(&mut state).0.is_none());
        }
    }

    #[test]
    fn accessibility_names_the_painted_menu_button() {
        let app = Application::new(None, |_| {}).expect("headless application");
        let context = egui::Context::default();
        context.enable_accesskit();
        let output = context.run_ui(egui::RawInput::default(), |ui| {
            app.draw_top_bar(ui, &mut Vec::new());
        });
        let tree = output.platform_output.accesskit_update.expect("tree");
        assert!(tree.nodes.iter().any(|(_, node)| {
            node.label() == Some("towavue menu")
                && node.role() == egui::accesskit::Role::Button
                && node.supports_action(egui::accesskit::Action::Click)
        }));
    }

    #[test]
    fn menu_escape_returns_to_logo_for_keyboard_reopening() {
        let Some(_root) =
            isolated_test_root("tests::menu_escape_returns_to_logo_for_keyboard_reopening")
        else {
            return;
        };
        let app = Application::new(None, |_| {}).expect("headless application");
        let context = egui::Context::default();
        context.enable_accesskit();
        let frame = |events| {
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(480.0, 300.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| app.draw_top_bar(ui, &mut actions),
            );
            assert!(actions.is_empty(), "menu cancellation executes no command");
            output.platform_output.accesskit_update.expect("tree")
        };
        frame(vec![]);
        let tree = frame(vec![]);
        let logo = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("towavue menu"))
            .expect("logo")
            .0;
        let key = |key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![key(egui::Key::Tab)]);
        assert_eq!(frame(vec![]).focus, logo);
        for (submenu, activation) in [(false, egui::Key::Enter), (true, egui::Key::Space)] {
            frame(vec![key(activation)]);
            frame(vec![]);
            assert!(egui::Popup::is_any_open(&context));
            if submenu {
                frame(vec![key(egui::Key::ArrowRight)]);
                frame(vec![]);
            }
            frame(vec![key(egui::Key::Escape)]);
            let tree = frame(vec![]);
            assert!(!egui::Popup::is_any_open(&context));
            assert_eq!(tree.focus, logo, "Escape returns focus to the menu button");
            frame(vec![key(activation)]);
            frame(vec![]);
            assert!(egui::Popup::is_any_open(&context));
            frame(vec![key(egui::Key::Escape)]);
            assert_eq!(frame(vec![]).focus, logo);
        }
        frame(vec![key(egui::Key::Tab)]);
        let next = frame(vec![]).focus;
        assert_ne!(next, logo);
        for _ in 0..3 {
            assert_eq!(frame(vec![]).focus, next, "idle does not steal focus");
        }
        frame(vec![egui::Event::AccessKitActionRequest(
            egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::Focus,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: logo,
                data: None,
            },
        )]);
        frame(vec![key(egui::Key::Enter)]);
        frame(vec![]);
        for pressed in [true, false] {
            frame(vec![egui::Event::PointerButton {
                pos: egui::pos2(450.0, 270.0),
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }]);
        }
        frame(vec![]);
        assert!(!egui::Popup::is_any_open(&context));
        assert_ne!(frame(vec![]).focus, logo, "outside click is not Escape");
    }

    #[test]
    fn menu_palette_round_trip_keeps_focus_in_the_live_accessibility_tree() {
        let Some(root) = isolated_test_root(
            "tests::menu_palette_round_trip_keeps_focus_in_the_live_accessibility_tree",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let context = egui::Context::default();
        context.enable_accesskit();
        app.ui_context = Some(context.clone());
        let frame = |app: &mut Application<_>, events| {
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(480.0, 300.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| app.draw_ui(ui, &mut actions),
            );
            for action in actions {
                app.handle_ui_action(action);
            }
            let tree = output.platform_output.accesskit_update.expect("tree");
            assert!(
                tree.nodes.iter().any(|(id, _)| *id == tree.focus),
                "the native accessibility consumer rejects a missing focused node"
            );
            tree
        };
        let key = |key, modifiers| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        frame(&mut app, vec![]);
        let tree = frame(&mut app, vec![]);
        let logo = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("towavue menu"))
            .expect("logo")
            .0;
        for label in ["towavue menu", "View", "Show command palette"] {
            let tree = frame(&mut app, vec![]);
            let target = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label().is_some_and(|name| name.starts_with(label)))
                .unwrap_or_else(|| panic!("missing {label}"))
                .0;
            frame(
                &mut app,
                vec![egui::Event::AccessKitActionRequest(
                    egui::accesskit::ActionRequest {
                        action: egui::accesskit::Action::Click,
                        target_tree: egui::accesskit::TreeId::ROOT,
                        target_node: target,
                        data: None,
                    },
                )],
            );
            frame(&mut app, vec![]);
        }
        assert!(app.palette_open);
        assert_eq!(
            frame(&mut app, vec![]).focus,
            egui::Id::new("command-palette-query").accesskit_id()
        );
        frame(
            &mut app,
            vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert!(!app.palette_open);
        assert_eq!(frame(&mut app, vec![]).focus, logo);

        let path = root.join("image.png");
        let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
        app.path = Some(path);
        app.media_kind = Some(MediaKind::Image);
        let invoke = |app: &mut Application<_>, label: &str| {
            let tree = frame(app, vec![]);
            let target = tree
                .nodes
                .iter()
                .find(|(_, node)| {
                    node.role() == egui::accesskit::Role::Button
                        && node.label().is_some_and(|name| name.starts_with(label))
                })
                .unwrap_or_else(|| panic!("missing {label}"))
                .0;
            frame(
                app,
                vec![egui::Event::AccessKitActionRequest(
                    egui::accesskit::ActionRequest {
                        action: egui::accesskit::Action::Click,
                        target_tree: egui::accesskit::TreeId::ROOT,
                        target_node: target,
                        data: None,
                    },
                )],
            );
            frame(app, vec![])
        };
        for _ in 0..3 {
            for command in ["Rotate clockwise", "Undo edit"] {
                let tree = invoke(&mut app, "towavue menu");
                let focused = tree.nodes.iter().find(|(id, _)| *id == tree.focus);
                assert!(
                    focused.is_some_and(|(_, node)| node
                        .label()
                        .is_some_and(|name| name.starts_with("File"))),
                    "reopened menu starts at File: {focused:?}"
                );
                frame(
                    &mut app,
                    vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
                );
                frame(
                    &mut app,
                    vec![key(egui::Key::ArrowRight, egui::Modifiers::NONE)],
                );
                invoke(&mut app, command);
                assert_eq!(app.edits[&tab].is_dirty(), command == "Rotate clockwise");
                if command == "Rotate clockwise" {
                    let tree = invoke(&mut app, "towavue menu");
                    assert!(tree.nodes.iter().any(|(_, node)| {
                        (node.label() == Some("Edit added (source unchanged)")
                            || node.value() == Some("Edit added (source unchanged)"))
                            && node.role() != egui::accesskit::Role::Button
                    }));
                    frame(
                        &mut app,
                        vec![key(egui::Key::ArrowRight, egui::Modifiers::NONE)],
                    );
                    invoke(&mut app, "Close tab");
                    assert!(app.pending_guard.is_some());
                    invoke(&mut app, "Cancel");
                    assert!(app.pending_guard.is_none());
                    assert!(app.edits[&tab].is_dirty());
                    assert_eq!(frame(&mut app, vec![]).focus, logo);
                }
            }
        }
    }

    #[test]
    fn accessibility_output_rejects_removed_focus_without_changing_live_nodes() {
        let context = egui::Context::default();
        context.enable_accesskit();
        let removed = egui::Id::new("removed-menu-item");
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            ui.label("Existing content");
            ui.memory_mut(|memory| memory.request_focus(removed));
        });
        let tree = output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("tree");
        assert_eq!(tree.focus, removed.accesskit_id());
        assert!(!tree.nodes.iter().any(|(id, _)| *id == tree.focus));
        let nodes = tree.nodes.clone();
        keep_accessibility_focus_live(&context, &mut output.platform_output);
        let tree = output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("tree");
        assert_eq!(tree.focus, tree.tree.as_ref().expect("root tree").root);
        assert_eq!(tree.nodes, nodes);
        assert!(context.memory(|memory| memory.focused()).is_none());
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            ui.button("Live control").request_focus();
        });
        let tree = output.platform_output.accesskit_update.clone();
        let focus = context.memory(|memory| memory.focused());
        keep_accessibility_focus_live(&context, &mut output.platform_output);
        assert_eq!(output.platform_output.accesskit_update, tree);
        assert_eq!(context.memory(|memory| memory.focused()), focus);
        keep_accessibility_focus_live(&context, &mut egui::PlatformOutput::default());
        assert_eq!(context.memory(|memory| memory.focused()), focus);
    }

    #[test]
    fn accessible_selection_edges_preserve_pixel_bounds_and_crop_history() {
        let Some(root) = isolated_test_root(
            "tests::accessible_selection_edges_preserve_pixel_bounds_and_crop_history",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let context = egui::Context::default();
        context.enable_accesskit();
        app.ui_context = Some(context.clone());
        let path = root.join("image.png");
        let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
        app.path = Some(path.clone());
        app.media_kind = Some(MediaKind::Image);
        app.image = Some(
            ImagePresentation::from_decoded(
                &context,
                &path,
                DecodedImage {
                    format: "test",
                    frames: vec![towavue_runtime_windows::DecodedImageFrame {
                        width: 400,
                        height: 200,
                        rgba: vec![255; 400 * 200 * 4],
                        delay: Duration::ZERO,
                    }],
                }
                .into(),
            )
            .expect("image texture"),
        );
        app.dispatch(CommandId::SelectAll);
        let frame = |app: &mut Application<_>, events| {
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960.0, 576.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| app.draw_ui(ui, &mut actions),
            );
            for action in actions {
                app.handle_ui_action(action);
            }
            output.platform_output.accesskit_update.expect("tree")
        };
        frame(&mut app, vec![]);
        let tree = frame(&mut app, vec![]);
        let mut ids = Vec::new();
        for label in [
            "Selection left (pixels)",
            "Selection right (pixels)",
            "Selection top (pixels)",
            "Selection bottom (pixels)",
        ] {
            let (id, node) = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label))
                .expect("named selection edge");
            assert_eq!(node.role(), egui::accesskit::Role::Slider);
            assert_eq!(node.numeric_value_step(), Some(1.0));
            ids.push(*id);
        }
        assert_eq!(tree.focus, ids[0], "Select all focuses the left edge");
        let request = |index: usize, value: f64| {
            egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::SetValue,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: ids[index],
                data: Some(egui::accesskit::ActionData::NumericValue(value)),
            })
        };
        let crop = |app: &Application<_>| {
            PixelCrop::from_selection(
                app.image_view.selection.expect("selection"),
                (400, 200),
                MediaKind::Image,
            )
            .expect("pixel bounds")
        };
        frame(
            &mut app,
            vec![
                request(0, 100.0),
                request(1, 300.0),
                request(2, 20.0),
                request(3, 180.0),
            ],
        );
        assert_eq!(
            crop(&app),
            PixelCrop {
                x: 100,
                y: 20,
                width: 200,
                height: 160
            }
        );
        frame(
            &mut app,
            vec![
                request(1, 350.0),
                request(0, 310.0),
                request(1, 300.0),
                request(0, f64::NAN),
            ],
        );
        assert_eq!(
            crop(&app),
            PixelCrop {
                x: 310,
                y: 20,
                width: 40,
                height: 160
            },
            "preserve received order and reject crossing/NaN"
        );
        frame(&mut app, vec![request(0, 100.0), request(1, 300.0)]);
        let selected = app.image_view.selection;
        app.view_drag = Some(ViewDrag::Selection {
            mode: SelectionDrag::Right,
            before: selected,
        });
        frame(&mut app, vec![request(0, 101.0)]);
        assert!(
            app.view_drag.is_none(),
            "value change cancels pointer ownership"
        );
        let key = |key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        frame(&mut app, vec![key(egui::Key::ArrowRight)]);
        assert_eq!(crop(&app).x, 102);
        assert_eq!(frame(&mut app, vec![]).focus, ids[0]);
        let stroke = |key| KeyStroke {
            key,
            modifiers: Modifiers::default(),
        };
        assert!(!app.owns_seek_shortcut(&stroke(Key::ArrowUp)));
        assert!(app.owns_seek_shortcut(&stroke(Key::Character('r'))));
        for key in [
            Key::ArrowUp,
            Key::ArrowRight,
            Key::Home,
            Key::Space,
            Key::Character('r'),
        ] {
            assert_eq!(
                app.owns_focused_shortcut(&stroke(key.clone())),
                app.owns_seek_shortcut(&stroke(key)),
                "selection keeps its existing key ownership"
            );
        }
        frame(&mut app, vec![key(egui::Key::Tab)]);
        assert_eq!(frame(&mut app, vec![]).focus, ids[1]);
        for escape_event in [true, false] {
            app.dispatch(CommandId::ToggleCommandPalette);
            frame(&mut app, vec![]);
            assert_eq!(
                frame(&mut app, vec![]).focus,
                egui::Id::new("command-palette-query").accesskit_id()
            );
            if escape_event {
                frame(&mut app, vec![key(egui::Key::Escape)]);
            } else {
                assert!(app.dismiss_overlay_or_fullscreen());
            }
            assert!(!app.palette_open);
            assert_eq!(
                frame(&mut app, vec![]).focus,
                ids[1],
                "palette cancellation returns to the invoking edge"
            );
            let right = crop(&app).x + crop(&app).width;
            frame(&mut app, vec![key(egui::Key::ArrowLeft)]);
            assert_eq!(crop(&app).x + crop(&app).width, right - 1);
            frame(&mut app, vec![key(egui::Key::ArrowRight)]);
        }
        for switch_overlay in [false, true] {
            app.dispatch(CommandId::ToggleGridMenu);
            frame(&mut app, vec![]);
            if switch_overlay {
                app.dispatch(CommandId::ToggleCommandPalette);
                frame(&mut app, vec![]);
                app.dispatch(CommandId::ToggleGridMenu);
            }
            let tree = frame(&mut app, vec![]);
            let target = tree
                .nodes
                .iter()
                .find(|(_, node)| {
                    node.role() == egui::accesskit::Role::Button
                        && node
                            .label()
                            .is_some_and(|label| label.contains("Rotate clockwise"))
                })
                .expect("grid rotation button")
                .0;
            frame(
                &mut app,
                vec![egui::Event::AccessKitActionRequest(
                    egui::accesskit::ActionRequest {
                        action: egui::accesskit::Action::Focus,
                        target_tree: egui::accesskit::TreeId::ROOT,
                        target_node: target,
                        data: None,
                    },
                )],
            );
            assert_eq!(frame(&mut app, vec![]).focus, target);
            if !switch_overlay {
                app.process_shortcut("Ctrl+K".parse().expect("default prefix"));
                assert!(app.prefix_started.is_some());
                frame(&mut app, vec![key(egui::Key::Escape)]);
                assert!(app.grid_open, "Escape cancels the active prefix first");
                assert!(app.prefix_started.is_none());
            }
            if switch_overlay {
                let logo = tree
                    .nodes
                    .iter()
                    .find(|(_, node)| node.label() == Some("towavue menu"))
                    .expect("logo")
                    .0;
                frame(
                    &mut app,
                    vec![egui::Event::AccessKitActionRequest(
                        egui::accesskit::ActionRequest {
                            action: egui::accesskit::Action::Click,
                            target_tree: egui::accesskit::TreeId::ROOT,
                            target_node: logo,
                            data: None,
                        },
                    )],
                );
                frame(&mut app, vec![]);
                assert!(egui::Popup::is_any_open(&context));
                frame(&mut app, vec![key(egui::Key::Escape)]);
                assert!(!egui::Popup::is_any_open(&context));
                assert!(app.grid_open, "menu Escape leaves the underlying grid open");
            }
            frame(&mut app, vec![key(egui::Key::Escape)]);
            assert!(!app.grid_open, "one Escape closes a focused grid");
            assert_eq!(frame(&mut app, vec![]).focus, ids[1]);
            let right = crop(&app).x + crop(&app).width;
            frame(&mut app, vec![key(egui::Key::ArrowLeft)]);
            assert_eq!(crop(&app).x + crop(&app).width, right - 1);
            frame(&mut app, vec![key(egui::Key::ArrowRight)]);
        }
        let before = app.image_view.selection;
        for overlay in 0..5 {
            app.pending_guard = (overlay == 0).then_some(GuardedAction::Exit);
            app.palette_open = overlay == 1;
            app.grid_open = overlay == 2;
            app.filmstrip_open = overlay == 3;
            if overlay == 4 {
                egui::Popup::open_id(&context, "selection-test-popup".into());
            }
            frame(&mut app, vec![]);
            let tree = frame(&mut app, vec![request(0, 50.0)]);
            assert_eq!(app.image_view.selection, before);
            assert!(
                tree.nodes
                    .iter()
                    .find(|(id, _)| *id == ids[0])
                    .expect("disabled edge")
                    .1
                    .is_disabled()
            );
            app.pending_guard = None;
            app.palette_open = false;
            app.grid_open = false;
            app.filmstrip_open = false;
            egui::Popup::close_all(&context);
        }
        frame(&mut app, vec![request(0, 100.0)]);
        assert!(!app.edits.get(&tab).is_some_and(EditHistory::is_dirty));
        app.dispatch(CommandId::ApplyCrop);
        assert_eq!(
            app.edits[&tab].operations(),
            &[EditOperation::Crop(PixelCrop {
                x: 100,
                y: 20,
                width: 200,
                height: 160
            })]
        );
        assert!(app.image_view.selection.is_none());
        app.undo_edit(false);
        assert!(!app.edits[&tab].is_dirty());
        app.reading_mode = true;
        app.dispatch(CommandId::SelectAll);
        assert!(
            app.image_view.selection.is_none(),
            "reading is display-only"
        );
        app.reading_mode = false;
        app.image_view.zoom = ZoomMode::Custom(4.0);
        app.dispatch(CommandId::SelectAll);
        let visible = |tree: &egui::accesskit::TreeUpdate, index: usize| {
            let bounds = tree
                .nodes
                .iter()
                .find(|(id, _)| *id == ids[index])
                .expect("edge")
                .1
                .bounds()
                .expect("edge bounds");
            assert!(
                bounds.x0 >= 8.0 && bounds.x1 <= 952.0 && bounds.y0 >= 40.0 && bounds.y1 <= 538.0,
                "focused edge {index} outside viewport: {bounds:?}"
            );
            assert_eq!(tree.focus, ids[index]);
        };
        frame(&mut app, vec![]);
        visible(&frame(&mut app, vec![]), 0);
        assert_eq!(
            app.image_view.pan,
            (338.0, 0.0),
            "move only enough to reveal the focused handle"
        );
        for index in [1, 2, 3, 0] {
            frame(
                &mut app,
                vec![egui::Event::AccessKitActionRequest(
                    egui::accesskit::ActionRequest {
                        action: egui::accesskit::Action::Focus,
                        target_tree: egui::accesskit::TreeId::ROOT,
                        target_node: ids[index],
                        data: None,
                    },
                )],
            );
            visible(&frame(&mut app, vec![]), index);
            assert_eq!(app.image_view.zoom, ZoomMode::Custom(4.0));
            assert_eq!(app.image_view.selection, Some(UnitRect::FULL));
        }
        let pan = app.image_view.pan;
        for _ in 0..3 {
            frame(&mut app, vec![]);
        }
        assert_eq!(app.image_view.pan, pan, "idle redraw does not recenter");
        app.image_view.pan = (-800.0, 600.0);
        for _ in 0..3 {
            frame(&mut app, vec![]);
        }
        assert_eq!(
            app.image_view.pan,
            (-800.0, 600.0),
            "manual pan is retained"
        );
        frame(
            &mut app,
            vec![egui::Event::AccessKitActionRequest(
                egui::accesskit::ActionRequest {
                    action: egui::accesskit::Action::Focus,
                    target_tree: egui::accesskit::TreeId::ROOT,
                    target_node: ids[0],
                    data: None,
                },
            )],
        );
        visible(&frame(&mut app, vec![]), 0);
        app.image_view.pan = (-800.0, 600.0);
        app.dispatch(CommandId::SelectAll);
        frame(&mut app, vec![]);
        visible(&frame(&mut app, vec![]), 0);
        frame(&mut app, vec![request(0, 350.0)]);
        visible(&frame(&mut app, vec![]), 0);
        assert_eq!(crop(&app).x, 350);
        assert_eq!(app.image_view.zoom, ZoomMode::Custom(4.0));
        assert!(!app.edits[&tab].is_dirty());
        frame(&mut app, vec![key(egui::Key::Tab)]);
        assert_eq!(frame(&mut app, vec![]).focus, ids[1]);
        app.dispatch(CommandId::ToggleCommandPalette);
        frame(&mut app, vec![]);
        app.dispatch(CommandId::ToggleCommandPalette);
        frame(&mut app, vec![]);
        frame(&mut app, vec![key(egui::Key::Escape)]);
        frame(&mut app, vec![]);
        visible(&frame(&mut app, vec![]), 1);
        app.dispatch(CommandId::ToggleCommandPalette);
        frame(&mut app, vec![]);
        app.dispatch(CommandId::SelectAll);
        frame(&mut app, vec![]);
        visible(&frame(&mut app, vec![]), 0);
        assert!(app.command_overlay_return_focus.is_none());
        app.dispatch(CommandId::ToggleCommandPalette);
        frame(&mut app, vec![]);
        frame(&mut app, vec![key(egui::Key::Escape)]);
        frame(&mut app, vec![]);
        visible(&frame(&mut app, vec![]), 0);
        assert!(app.command_overlay_return_focus.is_none());
        app.push_edit(EditOperation::FlipHorizontal);
        app.dispatch(CommandId::SelectAll);
        frame(&mut app, vec![]);
        frame(&mut app, vec![key(egui::Key::Tab)]);
        let history = app.edits[&tab].clone();
        for escape_event in [true, false] {
            assert_eq!(frame(&mut app, vec![]).focus, ids[1]);
            let before = app.image_view.selection;
            app.request_guarded(GuardedAction::CloseTab(tab));
            frame(&mut app, vec![]);
            frame(&mut app, vec![]);
            if !escape_event {
                let return_focus = app.guard_return_focus;
                frame(&mut app, vec![key(egui::Key::Tab)]);
                app.request_guarded(GuardedAction::CloseTab(tab));
                assert_eq!(app.guard_return_focus, return_focus);
                app.pending_dialog = Some(DialogIntent::Export {
                    tab,
                    source: path.clone(),
                    kind: MediaKind::Image,
                    continuation: app.pending_guard.take(),
                });
                frame(&mut app, vec![]);
                app.finish_dialog(Ok(None));
                frame(&mut app, vec![]);
                assert!(app.pending_guard.is_some());
                assert_eq!(app.guard_return_focus, return_focus);
            }
            if escape_event {
                frame(&mut app, vec![key(egui::Key::Escape)]);
            } else {
                app.handle_ui_action(UiAction::ResolveGuard(GuardDecision::Cancel));
            }
            assert!(app.pending_guard.is_none());
            if escape_event {
                app.request_guarded(GuardedAction::CloseTab(tab));
                frame(&mut app, vec![]);
                frame(&mut app, vec![]);
                app.resolve_guard(GuardDecision::Cancel);
            }
            frame(&mut app, vec![]);
            visible(&frame(&mut app, vec![]), 1);
            assert_eq!(app.image_view.selection, before);
            let right = crop(&app).x + crop(&app).width;
            frame(&mut app, vec![key(egui::Key::ArrowLeft)]);
            assert_eq!(crop(&app).x + crop(&app).width, right - 1);
            assert_eq!(app.edits[&tab], history);
            assert_eq!(app.tabs.active().map(|tab| tab.id), Some(tab));
            assert!(app.guard_return_focus.is_none());
        }
        let other = app.tabs.open_new(root.join("other.png"), MediaKind::Image);
        app.tabs.activate(tab);
        app.request_guarded(GuardedAction::CloseTab(tab));
        frame(&mut app, vec![]);
        app.resolve_guard(GuardDecision::Discard);
        assert!(app.guard_return_focus.is_none());
        assert_eq!(app.tabs.active().map(|tab| tab.id), Some(other));
        for _ in 0..3 {
            assert!(!ids.contains(&frame(&mut app, vec![]).focus));
        }
    }

    #[test]
    fn accessible_trim_values_preserve_targets_validation_and_undo() {
        let Some(root) = isolated_test_root(
            "tests::accessible_trim_values_preserve_targets_validation_and_undo",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let path = root.join("audio.wav");
        let tab = app.tabs.open_new(path.clone(), MediaKind::Audio);
        app.path = Some(path);
        app.media_kind = Some(MediaKind::Audio);
        app.media_duration = Some(Duration::from_secs(10));
        app.state = PlaybackState::Paused;
        app.timeline_open = true;
        let context = egui::Context::default();
        context.enable_accesskit();
        app.ui_context = Some(context.clone());
        let frame = |app: &mut Application<_>, events| {
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960.0, 576.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| app.draw_ui(ui, &mut actions),
            );
            (
                output.platform_output.accesskit_update.expect("tree"),
                actions,
            )
        };
        frame(&mut app, vec![]);
        let (tree, _) = frame(&mut app, vec![]);
        let ids: Vec<_> = ["Trim start (seconds)", "Trim end (seconds)"]
            .iter()
            .map(|label| {
                let (id, node) = tree
                    .nodes
                    .iter()
                    .find(|(_, node)| node.label() == Some(*label))
                    .expect("named trim slider");
                assert_eq!(node.role(), egui::accesskit::Role::Slider);
                assert_eq!(node.min_numeric_value(), Some(0.0));
                assert_eq!(node.max_numeric_value(), Some(10.0));
                assert_eq!(node.numeric_value_step(), Some(1.0));
                *id
            })
            .collect();
        let request = |index, value| {
            egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::SetValue,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: ids[index],
                data: Some(egui::accesskit::ActionData::NumericValue(value)),
            })
        };
        let (_, actions) = frame(&mut app, vec![request(0, 2.5), request(1, 7.5)]);
        assert!(
            matches!(actions.as_slice(), [UiAction::TrimEndpoint(a, _), UiAction::TrimEndpoint(b, _)] if *a == tab && *b == tab)
        );
        for action in actions {
            app.handle_ui_action(action);
        }
        assert_eq!(
            app.edit_state().trim_start,
            Some(media_time(Duration::from_secs_f64(2.5)))
        );
        assert_eq!(
            app.edit_state().trim_end,
            Some(media_time(Duration::from_secs_f64(7.5)))
        );
        let history = app.edits[&tab].clone();
        let (_, actions) = frame(&mut app, vec![request(0, 8.0)]);
        for action in actions {
            app.handle_ui_action(action);
        }
        assert_eq!(app.edits[&tab], history, "crossed endpoints must not edit");
        let (_, actions) = frame(&mut app, vec![request(1, 9.0), request(0, 8.0)]);
        for action in actions {
            app.handle_ui_action(action);
        }
        assert_eq!(
            app.edit_state().trim_start,
            Some(media_time(Duration::from_secs(8))),
            "end must expand before start moves past its old limit"
        );
        assert_eq!(
            app.edit_state().trim_end,
            Some(media_time(Duration::from_secs(9)))
        );
        app.undo_edit(false);
        app.undo_edit(false);
        let (_, actions) = frame(
            &mut app,
            vec![
                request(1, 9.0),
                request(0, 8.0),
                request(1, 8.0),
                request(0, 6.0),
                request(1, 7.0),
            ],
        );
        assert_eq!(actions.len(), 5);
        for action in actions {
            app.handle_ui_action(action);
        }
        assert_eq!(
            app.edit_state().trim_start,
            Some(media_time(Duration::from_secs(6)))
        );
        assert_eq!(
            app.edit_state().trim_end,
            Some(media_time(Duration::from_secs(7)))
        );
        for _ in 0..4 {
            app.undo_edit(false);
        }
        assert_eq!(app.edit_state(), history.state().clone());
        assert!(frame(&mut app, vec![request(0, f64::NAN)]).1.is_empty());
        let mut focus = request(0, 2.5);
        if let egui::Event::AccessKitActionRequest(request) = &mut focus {
            request.action = egui::accesskit::Action::Focus;
            request.data = None;
        }
        frame(&mut app, vec![focus]);
        assert!(app.owns_seek_shortcut(&KeyStroke {
            key: Key::Character('z'),
            modifiers: Modifiers {
                control: true,
                ..Default::default()
            }
        }));
        let (_, actions) = frame(
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::ArrowRight,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(
            matches!(actions.as_slice(), [UiAction::TrimEndpoint(id, EditOperation::SetTrimStart(value))] if *id == tab && value.as_seconds_f64() == 3.5)
        );
        app.handle_ui_action(actions[0].clone());
        let (focus_tree, _) = frame(&mut app, vec![]);
        assert_eq!(focus_tree.focus, ids[0]);
        app.undo_edit(false);
        assert_eq!(
            app.edits[&tab].state().trim_start,
            history.state().trim_start
        );
        app.undo_edit(true);
        assert_eq!(
            app.edit_state().trim_start,
            Some(media_time(Duration::from_secs_f64(3.5)))
        );
        app.pending_guard = Some(GuardedAction::Exit);
        frame(&mut app, vec![]);
        let (tree, actions) = frame(&mut app, vec![request(1, 9.0)]);
        assert!(actions.is_empty());
        assert!(
            tree.nodes
                .iter()
                .find(|(id, _)| *id == ids[1])
                .expect("disabled trim")
                .1
                .is_disabled()
        );
    }

    #[test]
    fn pointer_trim_uses_validation_undo_and_modal_tab_guards() {
        let Some(root) =
            isolated_test_root("tests::pointer_trim_uses_validation_undo_and_modal_tab_guards")
        else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let previous_tab = app.tabs.open_new(root.join("other.wav"), MediaKind::Audio);
        let tab = app.tabs.open_new(root.join("audio.wav"), MediaKind::Audio);
        app.media_kind = Some(MediaKind::Audio);
        app.media_duration = Some(Duration::from_secs(10));
        app.state = PlaybackState::Paused;
        let start = EditOperation::SetTrimStart(MediaTime::from_nanoseconds(2_500_000_000));
        let end = EditOperation::SetTrimEnd(MediaTime::from_nanoseconds(7_500_000_000));
        app.handle_ui_action(UiAction::TrimEndpoint(tab, start));
        assert_eq!(
            app.edit_state().trim_start,
            Some(MediaTime::from_nanoseconds(2_500_000_000))
        );
        app.handle_ui_action(UiAction::TrimEndpoint(tab, end));
        app.undo_edit(false);
        let history = app.edits[&tab].clone();
        for operation in [start, EditOperation::SetTrimEnd(MediaTime::ZERO)] {
            app.handle_ui_action(UiAction::TrimEndpoint(tab, operation));
            assert_eq!(
                app.edits[&tab], history,
                "duplicate/invalid edits preserve redo"
            );
        }
        app.export_error = Some("fixture modal".into());
        app.handle_ui_action(UiAction::TrimEndpoint(tab, end));
        assert_eq!(app.edits[&tab], history);
        app.export_error = None;
        app.handle_ui_action(UiAction::TrimEndpoint(previous_tab, end));
        assert_eq!(app.edits[&tab], history);
        app.undo_edit(true);
        assert_eq!(
            app.edit_state().trim_end,
            Some(MediaTime::from_nanoseconds(7_500_000_000))
        );
        app.undo_edit(false);
        app.undo_edit(false);
        assert!(!app.edits[&tab].is_dirty());
    }

    #[test]
    fn accessible_timeline_values_use_source_time_and_respect_disabled_state() {
        let Some(_root) = isolated_test_root(
            "tests::accessible_timeline_values_use_source_time_and_respect_disabled_state",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        app.media_kind = Some(MediaKind::Audio);
        app.media_duration = Some(Duration::from_secs(10));
        app.timeline_open = true;
        app.state = PlaybackState::Paused;
        let mut clock = PlaybackClock::new(MediaTime::from_nanoseconds(2_000_000_000), 1.0);
        clock.paused_at = Some(clock.wall_anchor);
        app.clock = Some(clock);
        let context = egui::Context::default();
        context.enable_accesskit();
        app.ui_context = Some(context.clone());
        let frame = |app: &mut Application<_>, events, disabled| {
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(500.0, 300.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    if disabled {
                        ui.disable();
                    }
                    app.draw_timeline(ui, &mut actions);
                    if context.current_pass_index() == 0 {
                        context.request_discard("accessibility input test");
                    }
                },
            );
            (
                output.platform_output.accesskit_update.expect("tree"),
                actions,
            )
        };
        frame(&mut app, vec![], false);
        let (tree, _) = frame(&mut app, vec![], false);
        let (id, node) = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Playback position (seconds)"))
            .expect("named seek slider");
        assert_eq!(node.role(), egui::accesskit::Role::Slider);
        assert_eq!(node.min_numeric_value(), Some(0.0));
        assert_eq!(node.max_numeric_value(), Some(10.0));
        assert_eq!(node.numeric_value(), Some(2.0));
        assert_eq!(node.numeric_value_step(), Some(5.0));
        let request = |value| {
            egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::SetValue,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: *id,
                data: Some(egui::accesskit::ActionData::NumericValue(value)),
            })
        };
        for (value, expected) in [
            (4.25, Some(4.25)),
            (-5.0, Some(0.0)),
            (50.0, Some(10.0)),
            (2.0, None),
            (f64::NAN, None),
            (f64::INFINITY, None),
        ] {
            let (_, actions) = frame(&mut app, vec![request(value)], false);
            if let Some(expected) = expected {
                assert!(
                    matches!(actions.as_slice(), [UiAction::Seek(time)] if time.as_seconds_f64() == expected)
                );
            } else {
                assert!(actions.is_empty());
            }
            assert!(frame(&mut app, vec![], false).1.is_empty());
        }
        let (tree, actions) = frame(&mut app, vec![request(5.0)], true);
        assert!(actions.is_empty());
        assert!(
            tree.nodes
                .iter()
                .find(|(candidate, _)| candidate == id)
                .expect("disabled slider")
                .1
                .is_disabled()
        );
        let action = |action| {
            egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
                action,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: *id,
                data: None,
            })
        };
        for (kind, expected) in [
            (egui::accesskit::Action::Increment, 7.0),
            (egui::accesskit::Action::Decrement, 0.0),
        ] {
            let (_, actions) = frame(&mut app, vec![action(kind)], false);
            assert!(
                matches!(actions.as_slice(), [UiAction::Seek(time)] if time.as_seconds_f64() == expected)
            );
        }
        let (_, actions) = frame(
            &mut app,
            vec![request(1.5), action(egui::accesskit::Action::Increment)],
            false,
        );
        assert!(
            matches!(actions.as_slice(), [UiAction::Seek(time)] if time.as_seconds_f64() == 6.5)
        );
        let mut wrong_target = request(5.0);
        if let egui::Event::AccessKitActionRequest(request) = &mut wrong_target {
            request.target_node = egui::Id::new("other seek").accesskit_id();
        }
        assert!(frame(&mut app, vec![wrong_target], false).1.is_empty());
        for wrong_tree in [false, true] {
            let mut invalid = request(5.0);
            if let egui::Event::AccessKitActionRequest(request) = &mut invalid {
                if wrong_tree {
                    request.target_tree.0 = "00000000-0000-0000-0000-000000000001"
                        .parse()
                        .expect("test tree ID");
                } else {
                    request.data = Some(egui::accesskit::ActionData::Value("5".into()));
                }
            }
            assert!(frame(&mut app, vec![invalid], false).1.is_empty());
        }
        frame(
            &mut app,
            vec![action(egui::accesskit::Action::Focus)],
            false,
        );
        let stroke = |key| KeyStroke {
            key,
            modifiers: Modifiers::default(),
        };
        assert!(app.owns_seek_shortcut(&stroke(Key::Character('r'))));
        assert!(app.owns_seek_shortcut(&stroke(Key::Space)));
        assert!(!app.owns_seek_shortcut(&stroke(Key::Tab)));
        assert!(!app.owns_seek_shortcut(&stroke(Key::ArrowRight)));
        assert!(app.owns_seek_shortcut(&KeyStroke {
            key: Key::ArrowRight,
            modifiers: Modifiers {
                control: true,
                ..Default::default()
            }
        }));
        app.prefix_started = Some(Instant::now());
        assert!(app.owns_seek_shortcut(&stroke(Key::ArrowRight)));
        app.prefix_started = None;
        app.palette_open = true;
        assert!(!app.owns_seek_shortcut(&stroke(Key::Character('r'))));
        app.palette_open = false;
        for (key, expected) in [
            (egui::Key::ArrowRight, 7.0),
            (egui::Key::ArrowLeft, 0.0),
            (egui::Key::Home, 0.0),
            (egui::Key::End, 10.0),
        ] {
            let key = egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            };
            let (_, actions) = frame(&mut app, vec![key], false);
            assert!(
                matches!(actions.as_slice(), [UiAction::Seek(time)] if time.as_seconds_f64() == expected)
            );
        }
        app.state = PlaybackState::Faulted;
        assert!(frame(&mut app, vec![request(5.0)], false).1.is_empty());
    }

    #[test]
    fn accessible_image_position_keeps_shell_order_and_dirty_guard() {
        let Some(root) = isolated_test_root(
            "tests::accessible_image_position_keeps_shell_order_and_dirty_guard",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let paths = [root.join("z.png"), root.join("a.png"), root.join("m.png")];
        let tab = app.tabs.open_new(paths[0].clone(), MediaKind::Image);
        app.path = Some(paths[0].clone());
        app.media_kind = Some(MediaKind::Image);
        app.state = PlaybackState::Paused;
        app.edits
            .entry(tab)
            .or_default()
            .push(EditOperation::RotateClockwise, MediaKind::Image);
        let history = app.edits[&tab].clone();
        app.folder_snapshot = Some(FolderSnapshot {
            folder_identity: towavue_core::ShellIdentity::new(vec![]),
            folder_path: root.clone(),
            items: paths
                .iter()
                .enumerate()
                .map(|(index, path)| towavue_core::FolderMediaItem {
                    identity: towavue_core::ShellIdentity::new(vec![index as u8]),
                    path: path.clone(),
                    kind: MediaKind::Image,
                })
                .collect(),
            sort_columns: vec![],
            source: FolderSnapshotSource::LiveExplorerView,
            generation: 1,
            captured_at: std::time::SystemTime::now(),
        });
        app.folder_snapshot
            .as_mut()
            .expect("snapshot")
            .items
            .insert(
                1,
                towavue_core::FolderMediaItem {
                    identity: towavue_core::ShellIdentity::new(vec![4]),
                    path: root.join("ignored.wav"),
                    kind: MediaKind::Audio,
                },
            );
        let context = egui::Context::default();
        context.enable_accesskit();
        app.ui_context = Some(context.clone());
        let frame = |app: &mut Application<_>, events| {
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(500.0, 300.0),
                    )),
                    events,
                    ..Default::default()
                },
                |_| {
                    app.draw_seek_bar(
                        &context,
                        egui::Rect::from_min_max(egui::pos2(0.0, 270.0), egui::pos2(500.0, 300.0)),
                        None,
                        &mut actions,
                    )
                },
            );
            (
                output.platform_output.accesskit_update.expect("tree"),
                actions,
            )
        };
        frame(&mut app, vec![]);
        let (tree, _) = frame(&mut app, vec![]);
        let (id, node) = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Image position"))
            .expect("image slider");
        assert_eq!(node.numeric_value(), Some(1.0));
        assert_eq!(node.min_numeric_value(), Some(1.0));
        assert_eq!(node.max_numeric_value(), Some(3.0));
        let request = |value| {
            egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::SetValue,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: *id,
                data: Some(egui::accesskit::ActionData::NumericValue(value)),
            })
        };
        for (value, index) in [(2.0, 1), (50.0, 2), (1.5, 1)] {
            let (_, actions) = frame(&mut app, vec![request(value)]);
            assert!(
                matches!(actions.as_slice(), [UiAction::OpenMedia(path, false)] if path == &paths[index])
            );
        }
        assert!(frame(&mut app, vec![request(1.0)]).1.is_empty());
        let (_, actions) = frame(&mut app, vec![request(3.0)]);
        app.handle_ui_action(actions[0].clone());
        assert!(
            matches!(app.pending_guard, Some(GuardedAction::Navigate(ref path)) if path == &paths[2])
        );
        assert_eq!(app.path.as_ref(), Some(&paths[0]));
        assert_eq!(app.edits[&tab], history);
        let (tree, actions) = frame(&mut app, vec![request(2.0)]);
        assert!(actions.is_empty());
        assert!(
            tree.nodes
                .iter()
                .find(|(candidate, _)| candidate == id)
                .expect("disabled image slider")
                .1
                .is_disabled()
        );
    }

    #[test]
    fn seek_cancellation_blocks_later_release_and_preserves_fullscreen() {
        let Some(_root) = isolated_test_root(
            "tests::seek_cancellation_blocks_later_release_and_preserves_fullscreen",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        app.media_kind = Some(MediaKind::Audio);
        app.media_duration = Some(Duration::from_secs(10));
        app.state = PlaybackState::Paused;
        let start = egui::pos2(100.0, 250.0);
        let end = egui::pos2(400.0, 250.0);
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let escape = egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: Some(egui::Key::Escape),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        for interruption in 0..8 {
            let context = egui::Context::default();
            app.ui_context = Some(context.clone());
            app.timeline_open = true;
            let frame = |app: &mut Application<_>, events, focused, disabled| {
                let mut actions = Vec::new();
                let _ = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(500.0, 300.0),
                        )),
                        events,
                        focused,
                        ..Default::default()
                    },
                    |ui| {
                        if disabled {
                            ui.disable();
                        }
                        app.draw_timeline(ui, &mut actions);
                    },
                );
                actions
            };
            for _ in 0..3 {
                frame(&mut app, vec![], true, false);
            }
            frame(
                &mut app,
                vec![egui::Event::PointerMoved(start)],
                true,
                false,
            );
            assert!(frame(&mut app, vec![button(start, true)], true, false).is_empty());
            assert!(frame(&mut app, vec![egui::Event::PointerMoved(end)], true, false).is_empty());
            assert!(timeline_input::is_active(&context));
            let mut release = vec![button(end, false)];
            match interruption {
                0 => {
                    frame(&mut app, vec![escape.clone()], true, false);
                }
                1 => release.push(escape.clone()),
                2 => {
                    frame(&mut app, vec![], false, false);
                }
                3 => release.extend([
                    egui::Event::WindowFocused(false),
                    egui::Event::WindowFocused(true),
                ]),
                4 => {
                    app.fullscreen = true;
                    assert!(app.dismiss_overlay_or_fullscreen());
                    assert!(app.fullscreen);
                    app.fullscreen = false;
                }
                5 => {
                    app.dispatch(CommandId::ToggleTimeline);
                    app.timeline_open = true;
                }
                6 => {
                    egui::Popup::open_id(&context, "seek-blocker".into());
                    frame(&mut app, vec![], true, false);
                    egui::Popup::close_id(&context, "seek-blocker".into());
                }
                _ => {
                    frame(&mut app, vec![], true, true);
                }
            }
            assert!(
                frame(&mut app, release, true, false).is_empty(),
                "interruption={interruption}"
            );
            assert!(!timeline_input::is_active(&context));
            frame(&mut app, vec![button(start, true)], true, false);
            frame(&mut app, vec![egui::Event::PointerMoved(end)], true, false);
            let actions = frame(&mut app, vec![button(end, false)], true, false);
            assert!(
                matches!(actions.as_slice(), [UiAction::Seek(_)]),
                "new press after interruption={interruption}"
            );
            assert!(!timeline_input::is_active(&context));
        }
    }

    #[test]
    fn timeline_and_folder_seek_only_commit_primary_pointer_gestures() {
        let Some(root) = isolated_test_root(
            "tests::timeline_and_folder_seek_only_commit_primary_pointer_gestures",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        app.path = Some(root.join("0.png"));
        app.folder_snapshot = Some(FolderSnapshot {
            folder_identity: towavue_core::ShellIdentity::new(vec![]),
            folder_path: root.clone(),
            items: (0..3)
                .map(|index| towavue_core::FolderMediaItem {
                    identity: towavue_core::ShellIdentity::new(vec![index]),
                    path: root.join(format!("{index}.png")),
                    kind: MediaKind::Image,
                })
                .collect(),
            sort_columns: vec![],
            source: FolderSnapshotSource::LiveExplorerView,
            generation: 1,
            captured_at: std::time::SystemTime::now(),
        });
        app.timeline_open = true;
        app.media_duration = Some(Duration::from_secs(10));
        app.state = PlaybackState::Paused;
        for timeline in [true, false] {
            app.media_kind = Some(if timeline {
                MediaKind::Audio
            } else {
                MediaKind::Image
            });
            for button in [
                egui::PointerButton::Primary,
                egui::PointerButton::Secondary,
                egui::PointerButton::Middle,
                egui::PointerButton::Extra1,
                egui::PointerButton::Extra2,
            ] {
                for drag in [false, true] {
                    let context = egui::Context::default();
                    let mut frame = |events| {
                        let mut actions = Vec::new();
                        let _ = context.run_ui(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(500.0, 300.0),
                                )),
                                events,
                                ..Default::default()
                            },
                            |ui| {
                                if timeline {
                                    app.draw_timeline(ui, &mut actions);
                                } else {
                                    app.draw_seek_bar(
                                        ui.ctx(),
                                        egui::Rect::from_min_size(
                                            egui::pos2(0.0, 260.0),
                                            egui::vec2(500.0, 40.0),
                                        ),
                                        None,
                                        &mut actions,
                                    );
                                }
                            },
                        );
                        actions
                    };
                    let end = egui::pos2(400.0, if timeline { 250.0 } else { 260.0 });
                    let start = if drag {
                        end - egui::vec2(300.0, 0.0)
                    } else {
                        end
                    };
                    let event = |pos, pressed| egui::Event::PointerButton {
                        pos,
                        button,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    };
                    for _ in 0..3 {
                        assert!(frame(vec![]).is_empty());
                    }
                    assert!(frame(vec![egui::Event::PointerMoved(start)]).is_empty());
                    assert!(frame(vec![event(start, true)]).is_empty());
                    assert!(frame(vec![egui::Event::PointerMoved(end)]).is_empty());
                    let actions = frame(vec![
                        event(end, false),
                        egui::Event::PointerMoved(egui::pos2(50.0, end.y)),
                    ]);
                    if button != egui::PointerButton::Primary {
                        assert!(
                            actions.is_empty(),
                            "timeline={timeline}, {button:?}, drag={drag}"
                        );
                    } else if timeline {
                        assert!(
                            matches!(actions.as_slice(), [UiAction::Seek(time)] if time.as_seconds_f64() > 7.0 && time.as_seconds_f64() < 9.0)
                        );
                    } else {
                        assert!(
                            matches!(actions.as_slice(), [UiAction::OpenMedia(path, false)] if *path == root.join("2.png"))
                        );
                    }
                    assert!(frame(vec![]).is_empty(), "do not repeat the commit");
                }
            }
        }
        assert_eq!(app.path, Some(root.join("0.png")));
        assert!(app.edits.is_empty());
    }

    #[test]
    fn timeline_resize_preserves_media_state_and_leaves_room_after_window_shrink() {
        let Some(root) = isolated_test_root(
            "tests::timeline_resize_preserves_media_state_and_leaves_room_after_window_shrink",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let path = root.join("video.mp4");
        let tab = app.tabs.open_new(path.clone(), MediaKind::Video);
        app.path = Some(path);
        app.media_kind = Some(MediaKind::Video);
        app.media_duration = Some(Duration::from_secs(120));
        app.timeline_open = true;
        app.state = PlaybackState::Paused;
        app.edits.entry(tab).or_default().push(
            EditOperation::SetTrimStart(MediaTime::from_nanoseconds(10_000_000_000)),
            MediaKind::Video,
        );
        let history = app.edits[&tab].clone();
        let position = app.current_position();
        let generation = app.generation;
        let context = egui::Context::default();
        let mut frame = |size, events| {
            let mut actions = Vec::new();
            let _ = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    events,
                    ..Default::default()
                },
                |ui| app.draw_ui(ui, &mut actions),
            );
            assert!(actions.is_empty(), "resize must not seek or edit");
            egui::containers::panel::PanelState::load(&context, egui::Id::new("timeline"))
                .expect("timeline panel")
                .outer_rect
        };
        let size = egui::vec2(960.0, 576.0);
        for _ in 0..3 {
            frame(size, vec![]);
        }
        let before = frame(size, vec![]);
        let start = before.center_top();
        let end = start - egui::vec2(0.0, 90.0);
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        for events in [
            vec![egui::Event::PointerMoved(start)],
            vec![button(start, true)],
            vec![egui::Event::PointerMoved(start - egui::vec2(0.0, 10.0))],
            vec![egui::Event::PointerMoved(end)],
            vec![button(end, false)],
        ] {
            frame(size, events);
        }
        let after = frame(size, vec![]);
        assert!(
            after.height() > before.height() + 70.0,
            "{before:?} -> {after:?}"
        );
        for _ in 0..3 {
            frame(egui::vec2(240.0, 150.0), vec![]);
        }
        let small = frame(egui::vec2(240.0, 150.0), vec![]);
        assert!(small.height() <= 54.0, "small panel: {small:?}");
        assert!(small.top() >= 64.0, "keep a media viewport: {small:?}");
        assert_eq!(app.edits[&tab], history);
        assert_eq!(app.current_position(), position);
        assert_eq!(app.generation, generation);
        assert_eq!(app.state, PlaybackState::Paused);
    }

    #[test]
    fn accessible_tab_targets_survive_reordering_and_keep_close_guard() {
        let Some(root) = isolated_test_root(
            "tests::accessible_tab_targets_survive_reordering_and_keep_close_guard",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let a = app.tabs.open_new(root.join("a.png"), MediaKind::Image);
        let b = app.tabs.open_new(root.join("b.png"), MediaKind::Image);
        let c = app.tabs.open_new(root.join("c.png"), MediaKind::Image);
        app.path = Some(root.join("c.png"));
        app.media_kind = Some(MediaKind::Image);
        app.state = PlaybackState::Paused;
        let context = egui::Context::default();
        context.enable_accesskit();
        let frame = |app: &mut Application<_>, events| {
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960.0, 576.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| app.draw_ui(ui, &mut actions),
            );
            (
                output.platform_output.accesskit_update.expect("tree"),
                actions,
            )
        };
        let click = |target_node| {
            egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::Click,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node,
                data: None,
            })
        };
        frame(&mut app, vec![]);
        let (tree, _) = frame(&mut app, vec![]);
        let activate_b = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("b.png"))
            .expect("tab button")
            .0;
        let (close_b, node) = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Close tab: b.png"))
            .expect("named close button");
        assert_eq!(
            node.description(),
            Some(root.join("b.png").to_string_lossy().as_ref())
        );
        let close_b = *close_b;
        let mut focus = click(activate_b);
        if let egui::Event::AccessKitActionRequest(request) = &mut focus {
            request.action = egui::accesskit::Action::Focus;
        }
        frame(&mut app, vec![focus]);
        app.handle_ui_action(UiAction::ReorderTab(a, 3));
        let (tree, _) = frame(&mut app, vec![]);
        assert_eq!(tree.focus, activate_b);
        let (_, actions) = frame(&mut app, vec![click(activate_b)]);
        assert!(
            actions == [UiAction::ActivateTab(b)],
            "cached target must follow the tab, not its old slot"
        );
        app.close_tab_unchecked(c);
        app.edits
            .entry(b)
            .or_default()
            .push(EditOperation::RotateClockwise, MediaKind::Image);
        let history = app.edits[&b].clone();
        frame(&mut app, vec![]);
        let (_, actions) = frame(&mut app, vec![click(close_b)]);
        assert!(actions == [UiAction::CloseTab(b)]);
        app.handle_ui_action(actions[0].clone());
        assert!(matches!(app.pending_guard, Some(GuardedAction::CloseTab(id)) if id == b));
        frame(&mut app, vec![]);
        let (tree, actions) = frame(&mut app, vec![click(close_b)]);
        assert!(actions.is_empty());
        assert!(
            tree.nodes
                .iter()
                .find(|(id, _)| *id == close_b)
                .expect("disabled close button")
                .1
                .is_disabled()
        );
        app.handle_ui_action(UiAction::ResolveGuard(GuardDecision::Cancel));
        assert_eq!(app.edits[&b], history);
        assert!(app.tabs.tabs().iter().any(|tab| tab.id == b));
        app.close_tab_unchecked(b);
        frame(&mut app, vec![]);
        assert!(
            frame(&mut app, vec![click(close_b), click(activate_b)])
                .1
                .is_empty()
        );
        assert_eq!(app.tabs.tabs().len(), 1);
        assert_eq!(app.tabs.tabs()[0].id, a);
    }

    #[test]
    fn tab_drag_commits_once_on_release_and_preserves_the_active_edit() {
        let Some(root) = isolated_test_root(
            "tests::tab_drag_commits_once_on_release_and_preserves_the_active_edit",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let a = app.tabs.open_new(root.join("a.png"), MediaKind::Image);
        let b = app.tabs.open_new(root.join("b.png"), MediaKind::Image);
        let c = app.tabs.open_new(root.join("c.png"), MediaKind::Image);
        app.path = Some(root.join("c.png"));
        app.media_kind = Some(MediaKind::Image);
        app.edits
            .entry(c)
            .or_default()
            .push(EditOperation::RotateClockwise, MediaKind::Image);
        app.export_paths.insert(c, root.join("saved.png"));
        let history = app.edits[&c].clone();
        let original = app.tabs.clone();
        for (target, cancel, expected) in [
            (
                egui::pos2(500.0, 14.0),
                false,
                Some(UiAction::ReorderTab(a, 3)),
            ),
            (egui::pos2(300.0, 90.0), false, None),
            (egui::pos2(500.0, 14.0), true, None),
            (egui::pos2(-20.0, 90.0), false, Some(UiAction::DetachTab(a))),
        ] {
            app.tabs = original.clone();
            let context = egui::Context::default();
            let button = |pressed, pos| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::default(),
            };
            let start = egui::pos2(70.0, 14.0);
            let mut frames = vec![
                vec![],
                vec![egui::Event::PointerMoved(start)],
                vec![button(true, start)],
                vec![egui::Event::PointerMoved(egui::pos2(95.0, 14.0))],
                vec![egui::Event::PointerMoved(target)],
            ];
            if cancel {
                frames.push(vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::default(),
                }]);
            }
            frames.push(vec![button(false, target)]);
            let last = frames.len() - 1;
            for (index, events) in frames.into_iter().enumerate() {
                let mut actions = Vec::new();
                let _ = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(960.0, 576.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| app.draw_top_bar(ui, &mut actions),
                );
                if index == last {
                    assert!(
                        actions == expected.clone().into_iter().collect::<Vec<_>>(),
                        "release at {target:?}, cancelled={cancel}"
                    );
                } else {
                    assert!(actions.is_empty(), "must not commit during drag");
                }
            }
        }
        app.handle_ui_action(UiAction::ReorderTab(a, 3));
        assert_eq!(
            app.tabs.tabs().iter().map(|tab| tab.id).collect::<Vec<_>>(),
            [b, c, a]
        );
        assert_eq!(app.tabs.active().expect("active").id, c);
        assert_eq!(app.path, Some(root.join("c.png")));
        assert_eq!(app.edits[&c], history);
        assert_eq!(app.export_paths[&c], root.join("saved.png"));
        assert!(app.pending_guard.is_none());
    }

    #[test]
    fn overflowing_tab_bar_reveals_active_changes_but_preserves_manual_scroll() {
        let Some(root) = isolated_test_root(
            "tests::overflowing_tab_bar_reveals_active_changes_but_preserves_manual_scroll",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless app");
        let tabs: Vec<_> = (0..12)
            .map(|index| {
                app.tabs
                    .open_new(root.join(format!("tab-{index:02}.png")), MediaKind::Image)
            })
            .collect();
        let generation = app.media_generation;
        let context = egui::Context::default();
        context.global_style_mut(crate::chrome::style);
        let time = std::cell::Cell::new(0.0);
        let frame = |app: &Application<_>, width, events| {
            time.set(time.get() + 1.0 / 60.0);
            let output = context.run_ui(
                egui::RawInput {
                    time: Some(time.get()),
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 300.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let mut actions = Vec::new();
                    app.draw_top_bar(ui, &mut actions);
                    assert!(
                        actions.is_empty(),
                        "scrolling must not dispatch media actions"
                    );
                },
            );
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Rect(rect)
                        if rect.fill == Color32::from_gray(28)
                            && rect.rect.width() >= 70.0
                            && rect.rect.top() < 32.0 =>
                    {
                        Some((rect.rect, shape.clip_rect))
                    }
                    _ => None,
                })
                .expect("active tab rectangle")
        };
        let settle = |app: &Application<_>, width| {
            for _ in 0..90 {
                frame(app, width, vec![]);
            }
            frame(app, width, vec![])
        };
        let visible = |(rect, clip): (egui::Rect, egui::Rect)| {
            assert!(
                rect.left() >= clip.left() && rect.right() <= clip.right(),
                "active {rect:?} outside {clip:?}"
            );
        };
        visible(settle(&app, 480.0));
        app.tabs.activate(tabs[0]);
        visible(settle(&app, 480.0));
        frame(
            &app,
            480.0,
            vec![
                egui::Event::PointerMoved(egui::pos2(80.0, 14.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(-300.0, 0.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let manual = settle(&app, 480.0);
        assert!(
            manual.0.right() < manual.1.left(),
            "manual scroll can leave the current tab offscreen"
        );
        assert_eq!(
            settle(&app, 480.0),
            manual,
            "idle frames retain the user's position"
        );
        app.tabs.activate(tabs[11]);
        visible(settle(&app, 480.0));
        visible(settle(&app, 960.0));
        visible(settle(&app, 480.0));
        app.tabs.reorder(tabs[11], 0);
        visible(settle(&app, 480.0));
        let last = app.tabs.open_new(root.join("new.png"), MediaKind::Image);
        visible(settle(&app, 480.0));
        assert_eq!(app.tabs.active().expect("active tab").id, last);
        assert_eq!(app.media_generation, generation);
        assert!(app.edits.is_empty());
        assert!(app.pending_guard.is_none());
    }

    #[test]
    fn cursor_idle_is_limited_to_unobstructed_fullscreen_visual_media() {
        let Some(_root) = isolated_test_root(
            "tests::cursor_idle_is_limited_to_unobstructed_fullscreen_visual_media",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        app.ui_context = Some(egui::Context::default());
        app.fullscreen = true;
        app.state = PlaybackState::Paused;
        for kind in [
            None,
            Some(MediaKind::Audio),
            Some(MediaKind::Image),
            Some(MediaKind::Video),
        ] {
            app.media_kind = kind;
            assert_eq!(
                app.cursor_can_hide(true),
                matches!(kind, Some(MediaKind::Image | MediaKind::Video))
            );
        }
        assert!(!app.cursor_can_hide(false));
        for blocked in 0..14 {
            match blocked {
                0 => app.fullscreen = false,
                1 => app.image_loading = true,
                2 => app.image_error = Some("fixture failure".into()),
                3 => app.state = PlaybackState::Faulted,
                4 => app.filmstrip_open = true,
                5 => app.palette_open = true,
                6 => app.grid_open = true,
                7 => app.pending_dialog = Some(DialogIntent::OpenFile),
                8 => app.pending_guard = Some(GuardedAction::Exit),
                9 => app.export_error = Some("fixture failure".into()),
                10 => {
                    app.view_drag = Some(ViewDrag::Selection {
                        mode: SelectionDrag::Left,
                        before: None,
                    })
                }
                11 => app.state = PlaybackState::Loading,
                12 => app.fullscreen_controls_visible = true,
                _ => app.reading_pages.push(Err("fixture failure".into())),
            }
            assert!(!app.cursor_can_hide(true));
            app.fullscreen = true;
            app.fullscreen_controls_visible = false;
            app.image_loading = false;
            app.image_error = None;
            app.state = PlaybackState::Paused;
            app.filmstrip_open = false;
            app.palette_open = false;
            app.grid_open = false;
            app.pending_dialog = None;
            app.pending_guard = None;
            app.export_error = None;
            app.view_drag = None;
            app.reading_pages.clear();
            assert!(app.cursor_can_hide(true));
        }
        for button in [
            egui::PointerButton::Primary,
            egui::PointerButton::Secondary,
            egui::PointerButton::Middle,
        ] {
            for pressed in [true, false] {
                let _ = app.ui_context.as_ref().expect("UI context").run_ui(
                    egui::RawInput {
                        events: vec![egui::Event::PointerButton {
                            pos: egui::pos2(20.0, 20.0),
                            button,
                            pressed,
                            modifiers: egui::Modifiers::default(),
                        }],
                        ..Default::default()
                    },
                    |_| {},
                );
                assert_eq!(app.cursor_can_hide(true), !pressed);
            }
        }
        let _ = app.ui_context.as_ref().expect("UI context").run_ui(
            egui::RawInput {
                hovered_files: vec![egui::HoveredFile::default()],
                ..Default::default()
            },
            |_| {},
        );
        assert!(!app.cursor_can_hide(true));
        let now = Instant::now();
        app.viewing_cursor.update(now, true);
        assert_eq!(app.idle_wakeup(now), app.viewing_cursor.deadline);
        app.viewing_cursor
            .update(now + Duration::from_secs(2), true);
        assert!(app.viewing_cursor.hidden);
        assert_eq!(app.idle_wakeup(now), None);
        app.set_fullscreen(false);
        assert!(!app.viewing_cursor.hidden);
        assert_eq!(app.viewing_cursor.deadline, None);
    }

    fn isolated_test_root(test_name: &str) -> Option<PathBuf> {
        const TEST_ROOT: &str = "TOWAVUE_APP_TEST_ROOT";
        let Some(root) = std::env::var_os(TEST_ROOT) else {
            static NEXT_ROOT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time is after the epoch")
                .as_nanos();
            // Parallel tests can observe the same Windows clock tick.
            let sequence = NEXT_ROOT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "towavue-app-test-{}-{unique}-{sequence}",
                std::process::id()
            ));
            std::fs::create_dir(&root).expect("create isolated test root");
            // Isolate first-run configuration without changing this test process's environment.
            let result =
                std::process::Command::new(std::env::current_exe().expect("test executable"))
                    .args(["--exact", test_name, "--nocapture"])
                    .env(TEST_ROOT, &root)
                    .env("APPDATA", root.join("config"))
                    .env("LOCALAPPDATA", root.join("local"))
                    .output()
                    .expect("run isolated application test");
            std::fs::remove_dir_all(&root).expect("remove isolated test files");
            eprint!("{}", String::from_utf8_lossy(&result.stderr));
            assert!(
                result.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            return None;
        };
        Some(canonical_shell_path(&PathBuf::from(root)).expect("canonical test root"))
    }

    #[test]
    fn unsaved_guard_keyboard_focus_selects_only_the_requested_decision() {
        let Some(root) = isolated_test_root(
            "tests::unsaved_guard_keyboard_focus_selects_only_the_requested_decision",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let path = root.join("image.png");
        let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
        app.path = Some(path);
        app.media_kind = Some(MediaKind::Image);
        app.edits
            .entry(tab)
            .or_default()
            .push(EditOperation::RotateClockwise, MediaKind::Image);
        app.pending_guard = Some(GuardedAction::CloseTab(tab));
        let history = app.edits[&tab].clone();
        for (tabs, reverse, activation, expected) in [
            (1, false, egui::Key::Enter, GuardDecision::Save),
            (2, false, egui::Key::Enter, GuardDecision::Discard),
            (3, false, egui::Key::Enter, GuardDecision::Cancel),
            (1, true, egui::Key::Space, GuardDecision::Cancel),
        ] {
            let context = egui::Context::default();
            let run = |events| {
                let mut actions = Vec::new();
                let _ = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(480.0, 300.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |_| app.draw_unsaved_guard(&context, &mut actions),
                );
                actions
            };
            let key = |key, shift| egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    shift,
                    ..Default::default()
                },
            };
            for _ in 0..3 {
                assert!(run(vec![]).is_empty());
            }
            assert!(run(vec![key(egui::Key::Enter, false)]).is_empty());
            for _ in 0..tabs {
                assert!(run(vec![key(egui::Key::Tab, reverse)]).is_empty());
            }
            assert!(matches!(
                run(vec![key(activation, false)]).as_slice(),
                [UiAction::ResolveGuard(decision)] if *decision == expected
            ));
            assert_eq!(app.edits[&tab], history);
            assert_eq!(app.tabs.active().map(|tab| tab.id), Some(tab));
            assert!(app.active_export.is_none());
        }
    }

    #[test]
    fn confirmation_layout_keeps_actions_visible_after_resizing_with_long_text() {
        let Some(_root) = isolated_test_root(
            "tests::confirmation_layout_keeps_actions_visible_after_resizing_with_long_text",
        ) else {
            return;
        };
        let context = egui::Context::default();
        context.enable_accesskit();
        let mut app = Application::new(None, |_| {}).expect("headless application");
        app.path = Some(PathBuf::from(format!(
            "{}.png",
            "long-file-name-".repeat(17)
        )));
        let path = app.path.clone().expect("fixture path");
        let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
        for mode in 0..5 {
            app.pending_guard = matches!(mode, 0 | 1 | 4).then_some(GuardedAction::Exit);
            app.export_error = (mode == 1).then(|| "Long export error with details. ".repeat(200));
            app.active_export = (mode >= 2).then(|| {
                let request = ExportRequest {
                    source: path.clone(),
                    target: path.clone(),
                    kind: MediaKind::Image,
                    operations: Vec::new(),
                    hardware_encode: false,
                };
                // Same-source rejection keeps this UI fixture free of file writes and FFmpeg.
                ActiveExport {
                    job: ExportJob::start(request.clone(), |_| {}).expect("fixture worker"),
                    tab,
                    request,
                    encoded: Duration::ZERO,
                    cancelling: false,
                    continuation: (mode == 3).then_some(GuardedAction::Exit),
                }
            });
            for size in [
                egui::vec2(960.0, 576.0),
                egui::vec2(480.0, 300.0),
                egui::vec2(320.0, 200.0),
                egui::vec2(240.0, 150.0),
                egui::vec2(960.0, 576.0),
            ] {
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                let mut output = egui::FullOutput::default();
                for _ in 0..4 {
                    output = context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(screen),
                            ..Default::default()
                        },
                        |ui| app.draw_ui(ui, &mut Vec::new()),
                    );
                }
                let expected: &[&str] = match mode {
                    1 => &["Export failed", "OK"],
                    2 => &["Cancel export"],
                    3 => &["Exporting before continuing", "Cancel export"],
                    4 => &[
                        "Unsaved edits",
                        "Export and continue",
                        "Discard edits",
                        "Cancel",
                        "Cancel current export",
                    ],
                    _ => &[
                        "Unsaved edits",
                        "Export and continue",
                        "Discard edits",
                        "Cancel",
                    ],
                };
                let tree = output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .expect("tree");
                let dialogs: Vec<_> = tree
                    .nodes
                    .iter()
                    .filter(|(_, node)| node.role() == egui::accesskit::Role::Dialog)
                    .collect();
                if mode == 2 {
                    assert!(dialogs.is_empty(), "background export is not modal");
                } else {
                    assert_eq!(dialogs.len(), 1, "one named active modal");
                    let (_, dialog) = dialogs[0];
                    assert_eq!(dialog.label(), Some(expected[0]));
                    assert!(dialog.is_modal());
                    let mut descendants = dialog.children().to_vec();
                    let mut index = 0;
                    while index < descendants.len() {
                        let id = descendants[index];
                        let (_, node) = tree
                            .nodes
                            .iter()
                            .find(|(node_id, _)| *node_id == id)
                            .expect("modal descendant");
                        descendants.extend_from_slice(node.children());
                        index += 1;
                    }
                    for label in &expected[1..] {
                        assert!(
                            tree.nodes.iter().any(|(id, node)| {
                                node.role() == egui::accesskit::Role::Button
                                    && node.label() == Some(*label)
                                    && descendants.contains(id)
                            }),
                            "{label} must belong to the dialog"
                        );
                    }
                }
                let mut target = egui::Pos2::ZERO;
                for label in expected {
                    let shape = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Text(text) if text.galley.text() == *label => {
                                Some((shape, text))
                            }
                            _ => None,
                        })
                        .unwrap_or_else(|| panic!("missing {label}"));
                    let bounds = shape.1.galley.rect.translate(shape.1.pos.to_vec2());
                    assert!(
                        screen.contains_rect(bounds),
                        "{label} outside {screen:?}: {bounds:?}"
                    );
                    assert!(shape.0.clip_rect.contains_rect(bounds), "{label} clipped");
                    target = bounds.center();
                }
                let mut actions = Vec::new();
                for pressed in [true, false] {
                    let _ = context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(screen),
                            events: vec![
                                egui::Event::PointerMoved(target),
                                egui::Event::PointerButton {
                                    pos: target,
                                    button: egui::PointerButton::Primary,
                                    pressed,
                                    modifiers: egui::Modifiers::NONE,
                                },
                            ],
                            ..Default::default()
                        },
                        |ui| app.draw_ui(ui, &mut actions),
                    );
                }
                let action = match mode {
                    0 => UiAction::ResolveGuard(GuardDecision::Cancel),
                    1 => UiAction::DismissExportError,
                    _ => UiAction::CancelExport,
                };
                assert!(
                    actions == [action],
                    "one confirmation action for mode {mode}"
                );
            }
        }
    }

    #[test]
    fn modal_escape_preserves_edits_and_dismisses_only_the_top_confirmation() {
        let Some(root) = isolated_test_root(
            "tests::modal_escape_preserves_edits_and_dismisses_only_the_top_confirmation",
        ) else {
            return;
        };
        let context = egui::Context::default();
        let mut app = Application::new(None, |_| {}).expect("headless application");
        std::fs::write(root.join("image.png"), []).expect("media placeholder");
        app.open_external(root.join("image.png"), false);
        app.dispatch(CommandId::RotateClockwise);
        let tab = app.tabs.active().expect("active tab").id;
        app.fullscreen = true;
        app.request_guarded(GuardedAction::CloseTab(tab));
        app.export_error = Some("fixture export failure".into());
        let render = |app: &mut Application<_>, events| {
            let mut actions = Vec::new();
            let _ = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960.0, 576.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| app.draw_ui(ui, &mut actions),
            );
            actions
        };
        let escape = || {
            [true, false]
                .into_iter()
                .map(|pressed| egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: Some(egui::Key::Escape),
                    pressed,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                })
                .collect()
        };
        for error_visible in [true, false] {
            for _ in 0..3 {
                assert!(render(&mut app, Vec::new()).is_empty());
            }
            for pressed in [true, false] {
                assert!(
                    render(
                        &mut app,
                        vec![
                            egui::Event::PointerMoved(egui::pos2(10.0, 100.0)),
                            egui::Event::PointerButton {
                                pos: egui::pos2(10.0, 100.0),
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ],
                    )
                    .is_empty()
                );
            }
            let actions = render(&mut app, escape());
            if error_visible {
                assert!(matches!(actions.as_slice(), [UiAction::DismissExportError]));
            } else {
                assert!(matches!(
                    actions.as_slice(),
                    [UiAction::ResolveGuard(GuardDecision::Cancel)]
                ));
            }
            for action in actions {
                app.handle_ui_action(action);
            }
            assert!(app.export_error.is_none());
            assert_eq!(app.pending_guard.is_some(), error_visible);
            assert!(app.pending_dialog.is_none() && app.active_export.is_none());
            assert!(app.fullscreen && !app.exit_requested);
            assert_eq!(app.tabs.active().expect("retained tab").id, tab);
            assert!(app.edits.get(&tab).expect("retained edits").is_dirty());
        }
    }

    #[test]
    fn fullscreen_preserves_media_state_and_prioritizes_modal_and_overlay_escape() {
        let Some(root) = isolated_test_root(
            "tests::fullscreen_preserves_media_state_and_prioritizes_modal_and_overlay_escape",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        app.path = Some(root.join("image.png"));
        app.media_kind = Some(MediaKind::Image);
        app.state = PlaybackState::Paused;
        app.image_view.selection = Some(UnitRect::FULL);
        app.timeline_open = true;
        let generation = app.generation;
        app.dispatch(CommandId::ToggleFullscreen);
        assert!(app.fullscreen);
        for blocked in 0..3 {
            match blocked {
                0 => app.pending_dialog = Some(DialogIntent::OpenFile),
                1 => app.pending_guard = Some(GuardedAction::Exit),
                _ => app.export_error = Some("fixture failure".into()),
            }
            assert!(!app.dismiss_overlay_or_fullscreen());
            assert!(app.fullscreen);
            app.pending_dialog = None;
            app.pending_guard = None;
            app.export_error = None;
        }
        for overlay in 0..3 {
            app.palette_open = overlay == 0;
            app.filmstrip_open = overlay == 1;
            app.grid_open = overlay == 2;
            assert!(app.dismiss_overlay_or_fullscreen());
            assert!(app.fullscreen);
            assert!(!app.palette_open && !app.filmstrip_open && !app.grid_open);
        }
        assert!(app.dismiss_overlay_or_fullscreen());
        assert!(!app.fullscreen);
        assert!(app.timeline_open);
        assert_eq!(app.image_view.selection, Some(UnitRect::FULL));
        assert_eq!(app.state, PlaybackState::Paused);
        assert_eq!(app.generation, generation);
        assert!(!app.dismiss_overlay_or_fullscreen());
        app.media_kind = Some(MediaKind::Video);
        for was_open in [false, true] {
            app.timeline_open = was_open;
            app.dispatch(CommandId::ToggleFullscreen);
            app.dispatch(CommandId::ToggleTimeline);
            assert!(!app.fullscreen);
            assert!(app.timeline_open);
        }
    }

    #[test]
    fn image_zoom_uses_physical_pixels_and_the_current_preview_viewport() {
        let Some(root) = isolated_test_root(
            "tests::image_zoom_uses_physical_pixels_and_the_current_preview_viewport",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let context = egui::Context::default();
        app.ui_context = Some(context.clone());
        app.fullscreen = true;
        app.media_kind = Some(MediaKind::Image);
        app.path = Some(root.join("image.png"));
        app.image = Some(
            ImagePresentation::from_decoded(
                &context,
                &root.join("image.png"),
                DecodedImage {
                    format: "test",
                    frames: vec![towavue_runtime_windows::DecodedImageFrame {
                        width: 400,
                        height: 200,
                        rgba: vec![255; 400 * 200 * 4],
                        delay: Duration::ZERO,
                    }],
                }
                .into(),
            )
            .expect("image texture"),
        );
        let texture = app.image.as_ref().expect("image").texture.id();
        let render = |app: &mut Application<_>, density: f32| {
            let mut output = egui::FullOutput::default();
            for _ in 0..3 {
                let mut input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 600.0) / density,
                    )),
                    ..Default::default()
                };
                input
                    .viewports
                    .get_mut(&egui::ViewportId::ROOT)
                    .expect("root viewport")
                    .native_pixels_per_point = Some(density);
                output = context.run_ui(input, |ui| app.draw_ui(ui, &mut Vec::new()));
            }
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Mesh(mesh) if mesh.texture_id == texture => {
                        Some(mesh.calc_bounds().size() * output.pixels_per_point)
                    }
                    _ => None,
                })
                .expect("image mesh")
        };
        for density in [1.0, 1.25, 1.5, 2.0, 1.0] {
            for preview in [false, true] {
                app.image_view.crop_preview = preview;
                app.image_view.selection = Some(UnitRect {
                    min: UnitPoint { x: 0.0, y: 0.0 },
                    max: UnitPoint { x: 0.5, y: 0.5 },
                });
                app.image_view.actual_size();
                let expected = egui::vec2(400.0, 200.0) / if preview { 2.0 } else { 1.0 };
                assert!((render(&mut app, density) - expected).length() < 0.01);
                app.image_view.fit();
                let fitted = render(&mut app, density);
                assert!(
                    (fitted - egui::vec2(800.0, 400.0)).length() < 0.1,
                    "density={density} preview={preview} fitted={fitted:?}"
                );
                app.zoom_image(1.25);
                assert!((render(&mut app, density) - fitted * 1.25).length() < 0.1);
            }
        }
        app.image_view.crop_preview = false;
        app.image_view.selection = None;
        app.image_view.actual_size();
        render(&mut app, 2.0);
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 300.0),
            )),
            events: vec![
                egui::Event::PointerMoved(egui::pos2(230.0, 160.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, 1.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::CTRL,
                },
            ],
            ..Default::default()
        };
        input
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .expect("root viewport")
            .native_pixels_per_point = Some(2.0);
        let output = context.run_ui(input, |ui| {
            app.draw_ui(ui, &mut Vec::new());
        });
        let factor = (1.0_f32 / 200.0).exp();
        assert!((app.image_view.pan.0 - 30.0 * (1.0 - factor)).abs() < 0.001);
        assert!((app.image_view.pan.1 - 10.0 * (1.0 - factor)).abs() < 0.001);
        let bounds = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Mesh(mesh) if mesh.texture_id == texture => Some(mesh.calc_bounds()),
                _ => None,
            })
            .expect("image mesh in wheel frame");
        assert!((bounds.size() * 2.0 - egui::vec2(400.0, 200.0) * factor).length() < 0.01);
        let anchored = bounds.center() + egui::vec2(30.0, 10.0) * factor;
        assert!((anchored - egui::pos2(230.0, 160.0)).length() < 0.001);
        assert!((render(&mut app, 2.0) - egui::vec2(400.0, 200.0) * factor).length() < 0.01);

        let wheel = |delta, modifiers| egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, delta),
            phase: egui::TouchPhase::Move,
            modifiers,
        };
        let mut time = 100.0;
        let mut wheel_frame = |app: &mut Application<_>, dt, events, covered| {
            time += dt;
            let _ = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(400.0, 300.0),
                    )),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    app.draw_ui(ui, &mut Vec::new());
                    if covered {
                        egui::Area::new("wheel-cover".into())
                            .order(egui::Order::Foreground)
                            .fixed_pos(egui::pos2(200.0, 120.0))
                            .show(ui.ctx(), |ui| {
                                ui.allocate_exact_size(
                                    egui::vec2(80.0, 80.0),
                                    egui::Sense::hover(),
                                );
                            });
                    }
                },
            );
        };
        for (dt, delta) in [(1.0 / 30.0, 60.0), (1.0 / 120.0, 60.0), (1.0 / 60.0, -60.0)] {
            app.image_view.actual_size();
            wheel_frame(
                &mut app,
                dt,
                vec![wheel(delta, egui::Modifiers::CTRL)],
                false,
            );
            for _ in 0..90 {
                wheel_frame(&mut app, dt, vec![], false);
            }
            let expected = (delta / 200.0).exp();
            assert!(
                matches!(app.image_view.zoom, ZoomMode::Custom(scale) if (scale - expected).abs() < 0.0001)
            );
        }
        for blocked in 0..7 {
            app.image_view.actual_size();
            app.palette_open = blocked == 2;
            app.grid_open = blocked == 3;
            app.pending_guard = (blocked == 4).then_some(GuardedAction::Exit);
            app.pending_dialog = (blocked == 5).then_some(DialogIntent::OpenFile);
            let point = if blocked == 1 {
                egui::pos2(-10.0, -10.0)
            } else {
                egui::pos2(230.0, 160.0)
            };
            for _ in 0..3 {
                wheel_frame(
                    &mut app,
                    0.1,
                    vec![egui::Event::PointerMoved(point)],
                    blocked == 6,
                );
            }
            let before = (app.image_view.zoom, app.image_view.pan);
            wheel_frame(
                &mut app,
                0.1,
                vec![wheel(
                    60.0,
                    if blocked == 0 {
                        egui::Modifiers::NONE
                    } else {
                        egui::Modifiers::CTRL
                    },
                )],
                blocked == 6,
            );
            for _ in 0..10 {
                wheel_frame(&mut app, 0.1, vec![], blocked == 6);
            }
            assert_eq!(
                (app.image_view.zoom, app.image_view.pan),
                before,
                "wheel input case {blocked}"
            );
            app.palette_open = false;
            app.grid_open = false;
            app.pending_guard = None;
            app.pending_dialog = None;
        }

        app.tabs.open_new(root.join("image.png"), MediaKind::Image);
        app.push_edit(EditOperation::Crop(PixelCrop {
            x: 0,
            y: 0,
            width: 200,
            height: 100,
        }));
        app.push_edit(EditOperation::RotateClockwise);
        app.image_view.actual_size();
        assert!((render(&mut app, 1.5) - egui::vec2(100.0, 200.0)).length() < 0.1);
        app.image_view.fit();
        let fitted = render(&mut app, 1.5);
        assert!((fitted - egui::vec2(300.0, 600.0)).length() < 0.1);
        app.zoom_image(1.25);
        assert!((render(&mut app, 1.5) - fitted * 1.25).length() < 0.1);
        for (scale, label) in [(0.0025, "0.25%"), (0.01355, "1.36%"), (0.1, "10%")] {
            app.image_view.zoom = ZoomMode::Custom(scale);
            let output = context.run_ui(Default::default(), |ui| {
                app.draw_status_bar(ui, &mut Vec::new(), &mut Vec::new());
            });
            assert!(output.shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text().contains(label))
            }));
        }
    }

    #[test]
    fn fullscreen_layout_hides_chrome_and_fills_the_image_viewport() {
        let Some(root) = isolated_test_root(
            "tests::fullscreen_layout_hides_chrome_and_fills_the_image_viewport",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let context = egui::Context::default();
        let path = root.join("image.png");
        app.path = Some(path.clone());
        app.media_kind = Some(MediaKind::Image);
        app.image = Some(
            ImagePresentation::from_decoded(
                &context,
                &path,
                DecodedImage {
                    format: "test",
                    frames: vec![towavue_runtime_windows::DecodedImageFrame {
                        width: 5,
                        height: 3,
                        rgba: vec![255; 60],
                        delay: Duration::ZERO,
                    }],
                }
                .into(),
            )
            .expect("image texture"),
        );
        let texture = app.image.as_ref().expect("image").texture.id();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 576.0));
        for (fullscreen, controls) in [(false, false), (true, false), (true, true), (false, false)]
        {
            app.set_fullscreen(fullscreen);
            app.status_message = None;
            let mut output = egui::FullOutput::default();
            for _ in 0..3 {
                output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        events: vec![egui::Event::PointerMoved(if controls {
                            screen.center_bottom() - egui::vec2(0.0, 15.0)
                        } else {
                            screen.center()
                        })],
                        ..Default::default()
                    },
                    |ui| app.draw_ui(ui, &mut Vec::new()),
                );
            }
            let has_close = output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == "×"));
            assert_eq!(has_close, !fullscreen);
            assert_eq!(app.fullscreen_controls_visible, controls);
            let image_rect = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Mesh(mesh) if mesh.texture_id == texture => {
                        Some(mesh.calc_bounds())
                    }
                    _ => None,
                })
                .expect("image mesh");
            if fullscreen {
                assert_eq!(image_rect, screen);
            } else {
                assert!(image_rect.top() >= 32.0 && image_rect.bottom() <= 546.0);
            }
        }
    }

    #[test]
    fn seek_preview_follows_hover_and_bounds_portrait_images() {
        let Some(_root) =
            isolated_test_root("tests::seek_preview_follows_hover_and_bounds_portrait_images")
        else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        app.media_kind = Some(MediaKind::Video);
        for pixels_per_point in [1.0, 2.0] {
            for screen_size in [egui::vec2(960.0, 576.0), egui::vec2(320.0, 240.0)] {
                for (image_size, track_height) in [
                    ([240, 144], 12.0),
                    ([24, 400], 12.0),
                    ([240, 144], 96.0),
                    ([24, 400], 96.0),
                ] {
                    let context = egui::Context::default();
                    context.set_pixels_per_point(pixels_per_point);
                    context.global_style_mut(|style| style.interaction.tooltip_delay = 0.0);
                    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, screen_size);
                    let track = egui::Rect::from_min_max(
                        egui::pos2(8.0, screen.bottom() - 30.0 - track_height),
                        egui::pos2(screen.right() - 8.0, screen.bottom() - 24.0),
                    );
                    let texture = context.load_texture(
                        "hover-fixture",
                        egui::ColorImage::filled(image_size, Color32::WHITE),
                        Default::default(),
                    );
                    for x in [track.left() + 1.0, track.center().x, track.right() - 1.0] {
                        let ratio = seekbar::ratio(track, x);
                        let bucket = ((ratio * 20.0).floor() as u64).min(19);
                        app.hover_thumbnail = Some((bucket, texture.clone()));
                        let mut output = egui::FullOutput::default();
                        for frame in 0..5 {
                            output = context.run_ui(
                                egui::RawInput {
                                    screen_rect: Some(screen),
                                    time: Some(f64::from(x) + f64::from(frame)),
                                    events: vec![egui::Event::PointerMoved(egui::pos2(
                                        x,
                                        track.center().y,
                                    ))],
                                    ..Default::default()
                                },
                                |ui| {
                                    let response = ui.allocate_rect(track, egui::Sense::hover());
                                    app.draw_seek_preview(
                                        &response,
                                        ratio,
                                        Duration::from_secs(30),
                                    );
                                },
                            );
                        }
                        let bounds = output
                            .shapes
                            .iter()
                            .find_map(|shape| match &shape.shape {
                                egui::Shape::Rect(rect)
                                    if rect.brush.as_ref().is_some_and(|brush| {
                                        brush.fill_texture_id == texture.id()
                                    }) =>
                                {
                                    Some(rect.rect)
                                }
                                _ => None,
                            })
                            .expect("hover preview image");
                        assert!(
                            bounds.width() <= 160.1 && bounds.height() <= 108.1,
                            "{bounds:?}"
                        );
                        assert!(screen.contains_rect(bounds));
                        assert!(bounds.bottom() < track.top());
                        if x == track.center().x {
                            assert!(
                                (bounds.center().x - x).abs() <= 1.0,
                                "preview must follow hover: {bounds:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn failed_hover_thumbnail_is_not_retried_until_media_reload() {
        let Some(root) =
            isolated_test_root("tests::failed_hover_thumbnail_is_not_retried_until_media_reload")
        else {
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = Application::new(None, move |event| {
            let _ = tx.send(event);
        })
        .expect("headless application");
        let path = root.join("missing.mp4");
        app.path = Some(path.clone());
        app.media_kind = Some(MediaKind::Video);
        app.state = PlaybackState::Playing;
        for bucket in [10, 11] {
            app.load_hover_thumbnail(Duration::from_secs(15), bucket);
            let event = rx
                .recv_timeout(Duration::from_secs(5))
                .expect("preview failure");
            app.handle_app_event(event);
            for _ in 0..100 {
                app.load_hover_thumbnail(Duration::from_secs(15), bucket);
                assert!(
                    app.thumbnail_loading.is_none(),
                    "failed bucket must stay idle"
                );
            }
        }
        assert_eq!(app.state, PlaybackState::Playing);
        assert!(app.playback_error.is_none());
        let previous_generation = app.media_generation;
        app.load_path(path.clone(), MediaKind::Video);
        app.load_hover_thumbnail(Duration::from_secs(15), 10);
        assert_eq!(app.thumbnail_loading, Some(10), "reload permits retry");
        let preview = towavue_runtime_windows::PreviewImage {
            width: 1,
            height: 1,
            rgba: vec![255; 4],
        };
        app.ui_context = Some(egui::Context::default());
        for result in [Err("old failure".into()), Ok(preview.clone())] {
            app.handle_app_event(AppEvent::Thumbnail(
                path.clone(),
                previous_generation,
                10,
                result,
            ));
            assert_eq!(app.thumbnail_loading, Some(10));
            assert!(app.failed_thumbnails.is_empty());
            assert!(app.hover_thumbnail.is_none());
        }
        loop {
            let event = rx
                .recv_timeout(Duration::from_secs(5))
                .expect("reload preview failure");
            if matches!(event, AppEvent::Thumbnail(_, _, _, _)) {
                app.handle_app_event(event);
                break;
            }
        }
        assert!(app.failed_thumbnails.contains(&10));
        app.thumbnail_loading = Some(11);
        app.handle_app_event(AppEvent::Thumbnail(
            path,
            app.media_generation,
            11,
            Ok(preview),
        ));
        assert_eq!(
            app.hover_thumbnail.as_ref().expect("successful neighbor").0,
            11
        );
        assert!(!app.failed_thumbnails.contains(&11));
    }

    #[test]
    fn image_seek_previews_follow_shell_order_without_navigating_on_hover() {
        let Some(root) = isolated_test_root(
            "tests::image_seek_previews_follow_shell_order_without_navigating_on_hover",
        ) else {
            return;
        };
        let source = root.join("z-current.png");
        towavue_runtime_windows::export_media(&towavue_runtime_windows::ExportRequest {
            source: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/generated/m1/h264-aac.mp4"),
            target: source.clone(),
            kind: MediaKind::Image,
            operations: vec![],
            hardware_encode: false,
        })
        .expect("image fixture");
        for name in ["a-next.png", "b-last.png"] {
            std::fs::copy(&source, root.join(name)).expect("page fixture");
        }
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let tab = app.tabs.open_new(source.clone(), MediaKind::Image);
        app.path = Some(source.clone());
        app.media_kind = Some(MediaKind::Image);
        app.edits
            .entry(tab)
            .or_default()
            .push(EditOperation::RotateClockwise, MediaKind::Image);
        let edits = app.edits.clone();
        app.folder_snapshot = Some(FolderSnapshot {
            folder_identity: towavue_core::ShellIdentity::new(vec![]),
            folder_path: root.clone(),
            items: ["z-current.png", "skip.wav", "a-next.png", "b-last.png"]
                .into_iter()
                .map(|name| towavue_core::FolderMediaItem {
                    identity: towavue_core::ShellIdentity::new(vec![]),
                    path: root.join(name),
                    kind: MediaKind::from_path(Path::new(name)).expect("kind"),
                })
                .collect(),
            sort_columns: vec![],
            source: FolderSnapshotSource::LiveExplorerView,
            generation: 1,
            captured_at: std::time::SystemTime::now(),
        });
        let context = egui::Context::default();
        context.global_style_mut(|style| style.interaction.tooltip_delay = 0.0);
        let mut time = 0.0;
        let mut draw = |app: &mut Application<_>, events| {
            app.filmstrip.finish(&context);
            time += 0.1;
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960.0, 576.0),
                    )),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| app.draw_ui(ui, &mut actions),
            );
            (output, actions)
        };
        for reading in [false, true] {
            app.reading_mode = reading;
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                let (output, actions) = draw(
                    &mut app,
                    vec![egui::Event::PointerMoved(egui::pos2(480.0, 548.0))],
                );
                assert!(actions.is_empty());
                assert_eq!(app.path.as_ref(), Some(&source));
                assert_eq!(app.edits, edits);
                let images = output
                    .shapes
                    .iter()
                    .filter(|shape| {
                        matches!(&shape.shape,
                    egui::Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::Managed(0))
                    })
                    .count();
                if images == if reading { 2 } else { 1 } {
                    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                        egui::Shape::Text(text) if text.galley.text().contains(if reading { "2–3 / 3  a-next.png" } else { "2 / 3  a-next.png" }))));
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "image seek preview did not complete"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        draw(
            &mut app,
            vec![egui::Event::PointerMoved(egui::pos2(480.0, 300.0))],
        );
        assert!(!app.image_seek_preview_active);
        for overlay in 0..4 {
            app.palette_open = overlay == 0;
            app.grid_open = overlay == 1;
            app.filmstrip_open = overlay == 2;
            app.pending_guard = (overlay == 3).then_some(GuardedAction::Exit);
            draw(
                &mut app,
                vec![egui::Event::PointerMoved(egui::pos2(480.0, 548.0))],
            );
            assert!(!app.image_seek_preview_active);
        }
        app.pending_guard = None;
        app.handle_ui_action(UiAction::OpenMedia(root.join("a-next.png"), false));
        assert!(app.pending_guard.is_some());
        assert_eq!(app.path.as_ref(), Some(&source));
        assert_eq!(app.edits, edits);
    }

    #[test]
    fn status_control_hints_follow_current_bindings_without_changing_actions() {
        let Some(_root) = isolated_test_root(
            "tests::status_control_hints_follow_current_bindings_without_changing_actions",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless app");
        for (kind, state, fullscreen, x, command, title) in [
            (
                MediaKind::Audio,
                PlaybackState::Playing,
                false,
                20.0,
                CommandId::TogglePause,
                "Pause",
            ),
            (
                MediaKind::Video,
                PlaybackState::Paused,
                false,
                20.0,
                CommandId::TogglePause,
                "Play / replay",
            ),
            (
                MediaKind::Video,
                PlaybackState::Ended,
                false,
                20.0,
                CommandId::TogglePause,
                "Play / replay",
            ),
            (
                MediaKind::Audio,
                PlaybackState::Paused,
                false,
                130.0,
                CommandId::ToggleTimeline,
                "Waveform timeline",
            ),
            (
                MediaKind::Image,
                PlaybackState::Paused,
                false,
                20.0,
                CommandId::ToggleReadingMode,
                "Reading mode",
            ),
            (
                MediaKind::Video,
                PlaybackState::Paused,
                true,
                20.0,
                CommandId::ToggleFullscreen,
                "Exit fullscreen",
            ),
        ] {
            app.media_kind = Some(kind);
            app.state = state;
            app.fullscreen = fullscreen;
            for binding in [Some("Ctrl+K Ctrl+P"), Some("K"), None] {
                app.shortcuts = ShortcutBindings::default();
                if let Some(binding) = binding {
                    app.shortcuts
                        .set(command, binding.parse().expect("custom binding"));
                }
                let context = egui::Context::default();
                context.global_style_mut(|style| style.interaction.tooltip_delay = 0.0);
                app.media_duration = Some(Duration::from_secs(30));
                let pos = egui::pos2(x, 284.0);
                let frame = |events| {
                    let mut actions = Vec::new();
                    let output = context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(480.0, 300.0),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ui| {
                            app.draw_status_bar(ui, &mut actions, &mut Vec::new());
                        },
                    );
                    (output, actions)
                };
                for _ in 0..3 {
                    frame(vec![egui::Event::PointerMoved(pos)]);
                }
                let (output, actions) = frame(vec![]);
                let expected =
                    binding.map_or_else(|| title.to_owned(), |key| format!("{title} ({key})"));
                assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == expected)), "missing {expected}");
                assert!(actions.is_empty());
                frame(vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }]);
                let actions = frame(vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }])
                .1;
                assert_eq!(actions.len(), 1);
                assert!(matches!(actions[0], UiAction::Command(id) if id == command));
            }
        }
    }

    #[test]
    fn audio_volume_wheel_is_limited_to_its_status_label() {
        let Some(root) =
            isolated_test_root("tests::audio_volume_wheel_is_limited_to_its_status_label")
        else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let tab = app.tabs.open_new(root.join("audio.wav"), MediaKind::Audio);
        app.path = Some(root.join("audio.wav"));
        app.media_kind = Some(MediaKind::Audio);
        for width in [240.0, 480.0, 960.0] {
            let context = egui::Context::default();
            let mut frame = |events| {
                let mut actions = Vec::new();
                let output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 300.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| app.draw_ui(ui, &mut actions),
                );
                (output, actions)
            };
            for _ in 0..4 {
                frame(vec![]);
            }
            let output = frame(vec![]).0;
            let label = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == "100%" => {
                        Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
                    }
                    _ => None,
                })
                .expect("dedicated volume label");
            assert!(label.left() >= 0.0 && label.right() <= width);
            let outside = egui::pos2(100.0, 100.0);
            let wheel = egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::vec2(0.0, -1.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            };
            for (events, expected) in [
                (
                    vec![
                        egui::Event::PointerMoved(outside),
                        wheel.clone(),
                        egui::Event::PointerMoved(label.center()),
                    ],
                    false,
                ),
                (
                    vec![
                        egui::Event::PointerMoved(label.center()),
                        wheel.clone(),
                        egui::Event::PointerMoved(outside),
                    ],
                    true,
                ),
                (
                    vec![
                        egui::Event::PointerMoved(outside),
                        wheel.clone(),
                        egui::Event::PointerMoved(label.center()),
                        wheel.clone(),
                    ],
                    true,
                ),
            ] {
                let actions = frame(events).1;
                assert!(
                    if expected {
                        actions == [UiAction::Volume(tab, 0.9)]
                    } else {
                        actions.is_empty()
                    },
                    "each wheel belongs to its event-time position"
                );
            }
            for (pos, expected) in [(label.center(), true), (egui::pos2(100.0, 100.0), false)] {
                let actions = frame(vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Line,
                        delta: egui::vec2(0.0, -1.0),
                        phase: egui::TouchPhase::Move,
                        modifiers: egui::Modifiers::NONE,
                    },
                ])
                .1;
                assert_eq!(
                    actions
                        .iter()
                        .filter(|action| matches!(action, UiAction::Volume(..)))
                        .count(),
                    usize::from(expected)
                );
                if expected {
                    assert!(actions == [UiAction::Volume(tab, 0.9)]);
                }
            }
        }
    }

    #[test]
    fn volume_wheel_uses_raw_events_and_respects_input_ownership() {
        let Some(root) =
            isolated_test_root("tests::volume_wheel_uses_raw_events_and_respects_input_ownership")
        else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let tab = app.tabs.open_new(root.join("audio.wav"), MediaKind::Audio);
        app.path = Some(root.join("audio.wav"));
        app.media_kind = Some(MediaKind::Audio);
        let context = egui::Context::default();
        let point = egui::pos2(100.0, 100.0);
        let wheel = |unit, delta, modifiers| egui::Event::MouseWheel {
            unit,
            delta,
            modifiers,
            phase: egui::TouchPhase::Move,
        };
        let mut time = 0.0;
        let mut frame = |app: &mut Application<_>, events, focused, dt| {
            time += dt;
            let mut actions = Vec::new();
            let _ = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(400.0, 300.0),
                    )),
                    time: Some(time),
                    events,
                    focused,
                    ..Default::default()
                },
                |ui| {
                    let response = ui.allocate_rect(
                        egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(200.0, 160.0)),
                        egui::Sense::hover(),
                    );
                    wheel_input::begin_frame(ui.ctx());
                    app.volume_wheel(ui.ctx(), &[response], &mut actions);
                },
            );
            actions
        };
        for _ in 0..3 {
            frame(&mut app, vec![egui::Event::PointerMoved(point)], true, 0.1);
        }
        for kind in [MediaKind::Audio, MediaKind::Video] {
            app.media_kind = Some(kind);
            for (unit, delta, expected) in [
                (egui::MouseWheelUnit::Line, 1.0, 1.1),
                (egui::MouseWheelUnit::Page, -2.0, 0.8),
                (egui::MouseWheelUnit::Point, 5.0, 1.01),
                (egui::MouseWheelUnit::Point, -50.0, 0.9),
            ] {
                for dt in [1.0 / 30.0, 1.0 / 120.0] {
                    let actions = frame(
                        &mut app,
                        vec![wheel(unit, egui::vec2(0.0, delta), egui::Modifiers::NONE)],
                        true,
                        dt,
                    );
                    assert_eq!(actions.len(), 1);
                    assert!(
                        matches!(actions[0], UiAction::Volume(id, volume) if id == tab && (volume - expected).abs() < 0.0001)
                    );
                    for _ in 0..30 {
                        assert!(
                            frame(&mut app, vec![], true, dt).is_empty(),
                            "no smooth-scroll tail edits"
                        );
                    }
                }
            }
        }
        let actions = frame(
            &mut app,
            vec![
                wheel(
                    egui::MouseWheelUnit::Line,
                    egui::vec2(0.0, -1.0),
                    egui::Modifiers::NONE,
                ),
                wheel(
                    egui::MouseWheelUnit::Line,
                    egui::vec2(0.0, -2.0),
                    egui::Modifiers::NONE,
                ),
            ],
            true,
            0.1,
        );
        assert_eq!(actions.len(), 1, "one edit for batched wheel events");
        app.handle_ui_action(actions[0].clone());
        assert!((app.edit_state().volume - 0.7).abs() < 0.0001);
        app.dispatch(CommandId::Undo);
        assert_eq!(app.edit_state().volume, 1.0);
        for blocked in 0..11 {
            if blocked == 10 {
                egui::Popup::open_id(&context, egui::Id::new("volume-test-menu"));
            }
            app.media_kind = Some(if blocked == 0 {
                MediaKind::Image
            } else {
                MediaKind::Audio
            });
            app.filmstrip_open = blocked == 1;
            app.palette_open = blocked == 2;
            app.grid_open = blocked == 3;
            app.pending_guard = (blocked == 4).then_some(GuardedAction::Exit);
            app.pending_dialog = (blocked == 5).then_some(DialogIntent::OpenFile);
            let pos = if blocked == 7 {
                egui::pos2(350.0, 250.0)
            } else {
                point
            };
            frame(&mut app, vec![egui::Event::PointerMoved(pos)], true, 0.1);
            let modifiers = if blocked == 8 {
                egui::Modifiers::CTRL
            } else {
                egui::Modifiers::NONE
            };
            let mut events = vec![wheel(
                egui::MouseWheelUnit::Line,
                egui::vec2(0.0, 1.0),
                modifiers,
            )];
            if blocked == 9 {
                events.insert(
                    0,
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers,
                    },
                );
            }
            assert!(
                frame(&mut app, events, blocked != 6, 0.1).is_empty(),
                "blocked input {blocked}"
            );
            frame(
                &mut app,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                true,
                0.1,
            );
        }
        app.pending_dialog = None;
        app.pending_guard = None;
        egui::Popup::close_all(&context);
        for modifiers in [egui::Modifiers::SHIFT, egui::Modifiers::ALT] {
            assert!(
                frame(
                    &mut app,
                    vec![wheel(
                        egui::MouseWheelUnit::Line,
                        egui::vec2(0.0, 1.0),
                        modifiers
                    )],
                    true,
                    0.1
                )
                .is_empty()
            );
        }
        assert!(
            frame(
                &mut app,
                vec![wheel(
                    egui::MouseWheelUnit::Line,
                    egui::vec2(1.0, 0.0),
                    egui::Modifiers::NONE
                )],
                true,
                0.1
            )
            .is_empty()
        );
        for volume in [0.0, 2.0] {
            app.push_edit(EditOperation::SetVolume(volume));
            let delta = if volume == 0.0 { -100.0 } else { 100.0 };
            assert!(
                frame(
                    &mut app,
                    vec![wheel(
                        egui::MouseWheelUnit::Line,
                        egui::vec2(0.0, delta),
                        egui::Modifiers::NONE
                    )],
                    true,
                    0.1
                )
                .is_empty(),
                "no edits at volume limits"
            );
        }
        app.tabs.open_new(root.join("other.wav"), MediaKind::Audio);
        app.handle_ui_action(UiAction::Volume(tab, 0.4));
        assert_eq!(app.edit_state().volume, 1.0, "ignore stale target tab");
    }

    #[test]
    fn unmute_restores_applied_volume_history_for_each_tab() {
        let Some(root) =
            isolated_test_root("tests::unmute_restores_applied_volume_history_for_each_tab")
        else {
            return;
        };
        for (kind, name) in [(MediaKind::Audio, "one.wav"), (MediaKind::Video, "one.mp4")] {
            let mut app = Application::new(None, |_| {}).expect("headless application");
            let source = root.join(name);
            let first = app.tabs.open_new(source.clone(), kind);
            app.path = Some(source.clone());
            app.media_kind = Some(kind);
            app.state = PlaybackState::Paused;
            let generation = app.media_generation;
            app.dispatch(CommandId::ToggleMute);
            assert_eq!(app.edit_state().volume, 0.0);
            app.dispatch(CommandId::ToggleMute);
            assert_eq!(app.edit_state().volume, 1.0, "initial default volume");
            for volume in [0.35, 1.6, 0.2] {
                app.push_edit(EditOperation::SetVolume(volume));
                app.dispatch(CommandId::ToggleMute);
                assert_eq!(app.edit_state().volume, 0.0);
                app.push_edit(EditOperation::SetRate(1.25));
                app.dispatch(CommandId::ToggleMute);
                assert_eq!(
                    app.edit_state().volume,
                    volume,
                    "restore previous nonzero volume"
                );
                app.dispatch(CommandId::Undo);
                assert_eq!(app.edit_state().volume, 0.0);
                app.dispatch(CommandId::Redo);
                assert_eq!(app.edit_state().volume, volume);
            }
            app.dispatch(CommandId::ToggleMute);
            app.push_edit(EditOperation::SetVolume(0.8));
            app.dispatch(CommandId::Undo);
            app.dispatch(CommandId::ToggleMute);
            assert_eq!(app.edit_state().volume, 0.2, "ignore unapplied redo volume");
            assert_eq!(app.edit_state().rate, 1.25);
            assert!(
                !app.edits.get_mut(&first).expect("history").redo(),
                "unmute branches the history"
            );
            app.dispatch(CommandId::ToggleMute);
            let second = app.tabs.open_new(root.join("second.wav"), kind);
            app.push_edit(EditOperation::SetVolume(0.65));
            app.dispatch(CommandId::ToggleMute);
            app.dispatch(CommandId::ToggleMute);
            assert_eq!(app.edit_state().volume, 0.65);
            assert_eq!(app.edits[&first].state().volume, 0.0);
            app.tabs.activate(first);
            app.dispatch(CommandId::ToggleMute);
            assert_eq!(
                app.edit_state().volume,
                0.2,
                "return to the first tab's own volume"
            );
            assert_eq!(app.edits[&second].state().volume, 0.65);
            app.push_edit(EditOperation::SetVolume(0.0));
            app.push_edit(EditOperation::SetVolume(0.0));
            app.dispatch(CommandId::ToggleMute);
            assert_eq!(
                app.edit_state().volume,
                0.2,
                "skip repeated zero-volume edits"
            );
            assert_ne!(first, second);
            assert_eq!(
                app.media_generation, generation,
                "volume does not reload media"
            );
            assert_eq!(app.state, PlaybackState::Paused);
            assert!(app.pending_guard.is_none());
        }
    }

    #[test]
    fn navigation_to_current_media_keeps_view_playback_and_edits() {
        let Some(root) =
            isolated_test_root("tests::navigation_to_current_media_keeps_view_playback_and_edits")
        else {
            return;
        };
        for (kind, name) in [
            (MediaKind::Image, "only.png"),
            (MediaKind::Video, "only.mp4"),
            (MediaKind::Audio, "only.wav"),
        ] {
            let mut app = Application::new(None, |_| {}).expect("headless application");
            let source = root.join(name);
            let tab = app.tabs.open_new(source.clone(), kind);
            app.path = Some(source.clone());
            app.media_kind = Some(kind);
            app.state = PlaybackState::Paused;
            let mut clock = PlaybackClock::new(MediaTime::from_nanoseconds(9_000_000_000), 1.0);
            clock.paused_at = Some(clock.wall_anchor);
            app.clock = Some(clock);
            app.image_view = ImageViewState {
                zoom: ZoomMode::Actual,
                pan: (30.0, -12.0),
                selection: Some(UnitRect::FULL),
                crop_preview: true,
            };
            app.folder_snapshot = Some(FolderSnapshot {
                folder_identity: towavue_core::ShellIdentity::new(vec![]),
                folder_path: root.clone(),
                items: vec![towavue_core::FolderMediaItem {
                    identity: towavue_core::ShellIdentity::new(vec![]),
                    path: source.clone(),
                    kind,
                }],
                sort_columns: vec![],
                source: FolderSnapshotSource::LiveExplorerView,
                generation: 1,
                captured_at: std::time::SystemTime::now(),
            });
            for dirty in [false, true] {
                if dirty {
                    let operation = if kind == MediaKind::Image {
                        EditOperation::RotateClockwise
                    } else {
                        EditOperation::SetVolume(0.5)
                    };
                    app.edits.entry(tab).or_default().push(operation, kind);
                }
                let edits = app.edits.clone();
                let view = app.image_view;
                let generation = app.media_generation;
                let position = app.current_position();
                for action in 0..5 {
                    match action {
                        0 => app.navigate(true, false),
                        1 => app.navigate(false, false),
                        2 => app.navigate(true, true),
                        3 => app.navigate(false, true),
                        _ => app.handle_ui_action(UiAction::OpenMedia(source.clone(), false)),
                    }
                    assert_eq!(
                        app.media_generation, generation,
                        "same media must not reload"
                    );
                    assert!(app.pending_guard.is_none());
                    assert_eq!(app.image_view, view);
                    assert_eq!(app.edits, edits);
                    assert_eq!(app.path.as_ref(), Some(&source));
                    assert_eq!(app.current_position(), position);
                    assert_eq!(app.state, PlaybackState::Paused);
                }
            }
            let other_kind = if kind == MediaKind::Image {
                MediaKind::Audio
            } else {
                MediaKind::Image
            };
            let other = root.join(if other_kind == MediaKind::Image {
                "other.png"
            } else {
                "other.wav"
            });
            app.folder_snapshot.as_mut().expect("snapshot").items.push(
                towavue_core::FolderMediaItem {
                    identity: towavue_core::ShellIdentity::new(vec![]),
                    path: other.clone(),
                    kind: other_kind,
                },
            );
            app.navigate(true, true);
            assert!(app.pending_guard.is_none(), "only one matching media kind");
            app.navigate(true, false);
            assert!(
                matches!(&app.pending_guard, Some(GuardedAction::Navigate(path)) if path == &other)
            );
            app.resolve_guard(GuardDecision::Cancel);
            assert_eq!(app.path.as_ref(), Some(&source));
        }
    }

    #[test]
    fn image_boundaries_preserve_shell_order_guards_and_current_endpoint() {
        let Some(root) = isolated_test_root(
            "tests::image_boundaries_preserve_shell_order_guards_and_current_endpoint",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let tab = app.tabs.open_new(root.join("middle.png"), MediaKind::Image);
        app.media_kind = Some(MediaKind::Image);
        app.edits
            .entry(tab)
            .or_default()
            .push(EditOperation::RotateClockwise, MediaKind::Image);
        let edits = app.edits.clone();
        app.folder_snapshot = Some(FolderSnapshot {
            folder_identity: towavue_core::ShellIdentity::new(vec![]),
            folder_path: root.clone(),
            items: [
                "skip.wav",
                "z-first.png",
                "middle.png",
                "a-last.png",
                "skip.mp4",
            ]
            .into_iter()
            .map(|name| towavue_core::FolderMediaItem {
                identity: towavue_core::ShellIdentity::new(vec![]),
                path: root.join(name),
                kind: MediaKind::from_path(Path::new(name)).expect("kind"),
            })
            .collect(),
            sort_columns: vec![],
            source: FolderSnapshotSource::LiveExplorerView,
            generation: 1,
            captured_at: std::time::SystemTime::now(),
        });
        for reading in [false, true] {
            app.reading_mode = reading;
            for current in ["z-first.png", "middle.png", "a-last.png", "missing.png"] {
                app.path = Some(root.join(current));
                app.tabs
                    .active_mut()
                    .expect("active tab")
                    .target
                    .set_current_path(root.join(current), MediaKind::Image);
                for (command, target) in [
                    (CommandId::FirstImage, "z-first.png"),
                    (CommandId::LastImage, "a-last.png"),
                ] {
                    let generation = app.media_generation;
                    app.dispatch(command);
                    if current == target || current == "missing.png" {
                        assert!(
                            app.pending_guard.is_none(),
                            "current/missing endpoint must not reload or prompt"
                        );
                    } else {
                        assert!(
                            matches!(&app.pending_guard, Some(GuardedAction::Navigate(path)) if *path == root.join(target))
                        );
                        app.resolve_guard(GuardDecision::Cancel);
                    }
                    assert_eq!(app.path.as_ref(), Some(&root.join(current)));
                    assert_eq!(app.media_generation, generation);
                    assert_eq!(app.edits, edits);
                    assert_eq!(app.reading_mode, reading);
                }
            }
        }
        app.folder_snapshot
            .as_mut()
            .expect("snapshot")
            .items
            .clear();
        for command in [CommandId::FirstImage, CommandId::LastImage] {
            app.dispatch(command);
            assert!(app.pending_guard.is_none());
        }
    }

    #[test]
    fn image_commands_preserve_snapshot_order_and_dirty_edits() {
        let Some(root) =
            isolated_test_root("tests::image_commands_preserve_snapshot_order_and_dirty_edits")
        else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let source = root.join("middle.png");
        let tab = app.tabs.open_new(source.clone(), MediaKind::Image);
        app.path = Some(source.clone());
        app.media_kind = Some(MediaKind::Image);
        app.edits
            .entry(tab)
            .or_default()
            .push(EditOperation::RotateClockwise, MediaKind::Image);
        let edits = app.edits.clone();
        app.folder_snapshot = Some(FolderSnapshot {
            folder_identity: towavue_core::ShellIdentity::new(vec![]),
            folder_path: root.clone(),
            items: ["middle.png", "other.wav", "first.png", "last.png"]
                .into_iter()
                .map(|name| towavue_core::FolderMediaItem {
                    identity: towavue_core::ShellIdentity::new(vec![]),
                    path: root.join(name),
                    kind: MediaKind::from_path(Path::new(name)).expect("media kind"),
                })
                .collect(),
            sort_columns: vec![],
            source: FolderSnapshotSource::LiveExplorerView,
            generation: 1,
            captured_at: std::time::SystemTime::now(),
        });
        for (command, target) in [
            (CommandId::NextImage, "first.png"),
            (CommandId::PreviousImage, "last.png"),
        ] {
            app.dispatch(command);
            assert!(matches!(
                &app.pending_guard,
                Some(GuardedAction::Navigate(path)) if *path == root.join(target)
            ));
            assert_eq!(app.path.as_ref(), Some(&source));
            assert_eq!(app.edits, edits);
            app.resolve_guard(GuardDecision::Cancel);
            assert!(app.pending_guard.is_none());
            assert_eq!(app.path.as_ref(), Some(&source));
            assert_eq!(app.edits, edits);
        }
    }

    #[test]
    fn fullscreen_controls_are_reachable_and_retained_by_keyboard_focus() {
        let Some(root) = isolated_test_root(
            "tests::fullscreen_controls_are_reachable_and_retained_by_keyboard_focus",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        app.fullscreen = true;
        app.media_kind = Some(MediaKind::Image);
        app.path = Some(root.join("image.png"));
        app.state = PlaybackState::Paused;
        let context = egui::Context::default();
        context.enable_accesskit();
        app.ui_context = Some(context.clone());
        let frame = |app: &mut Application<_>, events, focused| {
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960.0, 576.0),
                    )),
                    events,
                    focused,
                    ..Default::default()
                },
                |ui| app.draw_ui(ui, &mut actions),
            );
            assert!(actions.is_empty());
            output.platform_output.accesskit_update.expect("tree")
        };
        let tab = |shift| egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: if shift {
                egui::Modifiers::SHIFT
            } else {
                egui::Modifiers::NONE
            },
        };
        frame(&mut app, vec![], true);
        assert!(!app.fullscreen_controls_visible);
        frame(&mut app, vec![tab(false)], true);
        let tree = frame(&mut app, vec![], true);
        assert!(
            app.fullscreen_controls_visible,
            "Tab reveals controls without a pointer"
        );
        assert!(tree.nodes.iter().any(|(id, node)| {
            *id == tree.focus
                && node
                    .label()
                    .is_some_and(|label| label.starts_with("Exit fullscreen"))
        }));
        for shift in [false, true, false, false] {
            frame(&mut app, vec![tab(shift)], true);
            frame(
                &mut app,
                vec![egui::Event::PointerMoved(egui::pos2(720.0, 300.0))],
                true,
            );
            assert!(
                app.fullscreen_controls_visible,
                "focus survives pointer movement"
            );
        }
        for pressed in [true, false] {
            frame(
                &mut app,
                vec![egui::Event::PointerButton {
                    pos: egui::pos2(720.0, 300.0),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }],
                true,
            );
        }
        frame(&mut app, vec![], true);
        assert!(
            !app.fullscreen_controls_visible,
            "content click returns to pointer behavior"
        );
        frame(&mut app, vec![tab(false)], true);
        frame(&mut app, vec![], true);
        assert!(app.fullscreen_controls_visible);
        frame(&mut app, vec![], false);
        assert!(!app.fullscreen_controls_visible);
        frame(&mut app, vec![], true);
        assert!(!app.fullscreen_controls_visible);
        frame(&mut app, vec![tab(true)], true);
        frame(&mut app, vec![], true);
        assert!(
            app.fullscreen_controls_visible,
            "Shift+Tab also reveals controls"
        );
        app.pending_guard = Some(GuardedAction::Exit);
        frame(&mut app, vec![tab(false)], true);
        assert!(!app.fullscreen_controls_visible);
        app.pending_guard = None;
        frame(&mut app, vec![], true);
        assert!(!app.fullscreen_controls_visible);
    }

    #[test]
    fn fullscreen_controls_hold_through_release_and_keep_seek_above_status() {
        let Some(root) = isolated_test_root(
            "tests::fullscreen_controls_hold_through_release_and_keep_seek_above_status",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        app.fullscreen = true;
        app.media_kind = Some(MediaKind::Image);
        app.path = Some(root.join("0.png"));
        app.folder_snapshot = Some(FolderSnapshot {
            folder_identity: towavue_core::ShellIdentity::new(vec![]),
            folder_path: root.clone(),
            items: (0..5)
                .map(|index| towavue_core::FolderMediaItem {
                    identity: towavue_core::ShellIdentity::new(vec![index]),
                    path: root.join(format!("{index}.png")),
                    kind: MediaKind::Image,
                })
                .collect(),
            sort_columns: vec![],
            source: FolderSnapshotSource::LiveExplorerView,
            generation: 1,
            captured_at: std::time::SystemTime::now(),
        });
        let context = egui::Context::default();
        let frame = |app: &mut Application<_>, events, focused| {
            let mut actions = Vec::new();
            let _ = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960.0, 576.0),
                    )),
                    events,
                    focused,
                    ..Default::default()
                },
                |ui| app.draw_ui(ui, &mut actions),
            );
            actions
        };
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let edge = egui::pos2(20.0, 561.0);
        let center = egui::pos2(720.0, 300.0);
        for _ in 0..3 {
            assert!(frame(&mut app, vec![egui::Event::PointerMoved(center)], true).is_empty());
        }
        assert!(!app.fullscreen_controls_visible);
        frame(&mut app, vec![button(center, true)], true);
        frame(&mut app, vec![egui::Event::PointerMoved(edge)], true);
        assert!(
            !app.fullscreen_controls_visible,
            "do not reveal during a content gesture"
        );
        frame(&mut app, vec![button(edge, false)], true);
        for _ in 0..3 {
            frame(&mut app, vec![], true);
        }
        assert!(app.fullscreen_controls_visible);
        frame(&mut app, vec![button(edge, true)], true);
        let actions = frame(&mut app, vec![button(edge, false)], true);
        assert!(matches!(
            actions.as_slice(),
            [UiAction::Command(CommandId::ToggleFullscreen)]
        ));
        // The status Area has just been raised by its button. Both halves of Seek must still work.
        for y in [549.0, 543.0] {
            let seek = egui::pos2(480.0, y);
            frame(&mut app, vec![egui::Event::PointerMoved(seek)], true);
            assert!(frame(&mut app, vec![button(seek, true)], true).is_empty());
            let actions = frame(&mut app, vec![button(seek, false)], true);
            assert!(
                matches!(actions.as_slice(), [UiAction::OpenMedia(path, false)] if *path == root.join("2.png")),
                "seek y={y}"
            );
        }
        let seek = egui::pos2(480.0, 543.0);
        frame(&mut app, vec![egui::Event::PointerMoved(seek)], true);
        frame(&mut app, vec![button(seek, true)], true);
        assert!(frame(&mut app, vec![egui::Event::PointerMoved(center)], true).is_empty());
        assert!(app.fullscreen_controls_visible);
        let actions = frame(&mut app, vec![button(center, false)], true);
        assert!(
            matches!(actions.as_slice(), [UiAction::OpenMedia(path, false)] if *path == root.join("3.png"))
        );
        assert!(app.fullscreen_controls_visible, "retain the release frame");
        frame(&mut app, vec![], true);
        assert!(!app.fullscreen_controls_visible);
        for blocked in 0..6 {
            app.palette_open = blocked == 0;
            app.grid_open = blocked == 1;
            app.filmstrip_open = blocked == 2;
            app.pending_guard = (blocked == 3).then_some(GuardedAction::Exit);
            app.view_drag = (blocked == 4).then_some(ViewDrag::Selection {
                mode: SelectionDrag::Left,
                before: None,
            });
            frame(
                &mut app,
                vec![egui::Event::PointerMoved(edge)],
                blocked != 5,
            );
            assert!(!app.fullscreen_controls_visible);
        }
    }

    #[test]
    fn focused_controls_allow_bound_shortcuts_without_stealing_ui_keys_or_text() {
        let Some(root) = isolated_test_root(
            "tests::focused_controls_allow_bound_shortcuts_without_stealing_ui_keys_or_text",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless app");
        let path = root.join("image.png");
        let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
        app.path = Some(path);
        app.media_kind = Some(MediaKind::Image);
        let context = egui::Context::default();
        app.ui_context = Some(context.clone());
        let focus_button = || {
            let _ = context.run_ui(egui::RawInput::default(), |ui| {
                ui.button("Control").request_focus();
            });
        };
        focus_button();
        let stroke = |key: &str| key.parse::<KeyStroke>().expect("shortcut");
        assert!(app.owns_focused_shortcut(&stroke("R")));
        app.process_shortcut(stroke("R"));
        assert!(app.edits[&tab].is_dirty());
        assert!(app.owns_focused_shortcut(&stroke("Ctrl+Z")));
        app.process_shortcut(stroke("Ctrl+Z"));
        assert!(!app.edits[&tab].is_dirty());
        for key in [
            "Space",
            "Tab",
            "Left",
            "Right",
            "Up",
            "Down",
            "Home",
            "End",
            "Escape",
            "Shift+Space",
        ] {
            assert!(!app.owns_focused_shortcut(&stroke(key)), "UI owns {key}");
        }
        assert!(!app.owns_focused_shortcut(&stroke("Ctrl+Alt+9")));
        app.shortcuts
            .set(CommandId::RotateClockwise, "K".parse().expect("custom"));
        assert!(!app.owns_focused_shortcut(&stroke("R")), "no fixed alias");
        assert!(app.owns_focused_shortcut(&stroke("K")));
        app.process_shortcut(stroke("K"));
        app.shortcuts
            .set(CommandId::Undo, "Ctrl+K Space".parse().expect("prefix"));
        assert!(app.owns_focused_shortcut(&stroke("Ctrl+K")));
        app.process_shortcut(stroke("Ctrl+K"));
        assert!(
            app.owns_focused_shortcut(&stroke("Space")),
            "prefix continuation"
        );
        app.process_shortcut(stroke("Space"));
        assert!(!app.edits[&tab].is_dirty());
        app.process_shortcut(stroke("Ctrl+K"));
        assert!(app.owns_focused_shortcut(&stroke("Escape")));
        assert!(app.owns_focused_shortcut(&stroke("Ctrl+Alt+9")));
        app.cancel_shortcut_prefix();
        assert!(!app.owns_focused_shortcut(&stroke("Space")));
        for blocked in 0..5 {
            app.palette_open = blocked == 0;
            app.grid_open = blocked == 1;
            app.filmstrip_open = blocked == 2;
            app.pending_guard = (blocked == 3).then_some(GuardedAction::Exit);
            if blocked == 4 {
                egui::Popup::open_id(&context, "shortcut-test-popup".into());
            }
            assert!(!app.owns_focused_shortcut(&stroke("K")));
            app.palette_open = false;
            app.grid_open = false;
            app.filmstrip_open = false;
            app.pending_guard = None;
            egui::Popup::close_all(&context);
        }
        let mut text = String::new();
        let _ = context.run_ui(egui::RawInput::default(), |ui| {
            ui.text_edit_singleline(&mut text).request_focus();
        });
        assert!(context.text_edit_focused());
        assert!(!app.owns_focused_shortcut(&stroke("K")));
        assert!(!app.owns_focused_shortcut(&stroke("Ctrl+K")));
    }

    #[test]
    fn tab_shortcuts_reach_bindings_without_stealing_ui_focus_navigation() {
        let Some(_root) = isolated_test_root(
            "tests::tab_shortcuts_reach_bindings_without_stealing_ui_focus_navigation",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless app");
        let stroke = |key: &str| key.parse::<KeyStroke>().expect("Tab shortcut");
        for filmstrip in [false, true] {
            app.filmstrip_open = filmstrip;
            for key in ["Ctrl+Tab", "Ctrl+Shift+Tab"] {
                assert!(app.owns_tab_key(&stroke(key)), "app owns {key}");
            }
            for key in ["Tab", "Shift+Tab"] {
                assert_eq!(app.owns_tab_key(&stroke(key)), filmstrip);
            }
            assert!(!app.owns_tab_key(&stroke("Alt+Tab")));
            app.palette_open = true;
            assert!(!app.owns_tab_key(&stroke("Ctrl+Tab")));
            app.palette_open = false;
            app.grid_open = true;
            assert!(!app.owns_tab_key(&stroke("Ctrl+Tab")));
            app.grid_open = false;
            app.pending_guard = Some(GuardedAction::Exit);
            assert!(!app.owns_tab_key(&stroke("Ctrl+Tab")));
            app.pending_guard = None;
        }
        app.filmstrip_open = false;
        let context = egui::Context::default();
        context.memory_mut(|memory| memory.request_focus(egui::Id::new("focused control")));
        app.ui_context = Some(context);
        assert!(
            app.owns_tab_key(&stroke("Ctrl+Tab")),
            "focused button permits tab cycling"
        );
        assert!(
            !app.owns_tab_key(&stroke("Tab")),
            "bare Tab traverses controls"
        );
        egui::Popup::open_id(
            app.ui_context.as_ref().expect("context"),
            "tab-blocker".into(),
        );
        assert!(!app.owns_tab_key(&stroke("Ctrl+Tab")), "menu owns keys");
        app.ui_context = None;
        app.shortcuts.set(
            CommandId::NextTab,
            "Ctrl+N".parse().expect("custom next tab"),
        );
        assert!(!app.owns_tab_key(&stroke("Ctrl+Tab")), "no fixed alias");
        app.shortcuts.set(
            CommandId::NextTab,
            "Ctrl+K Ctrl+Tab".parse().expect("prefix binding"),
        );
        app.entered_shortcut = vec![stroke("Ctrl+K")];
        assert!(app.owns_tab_key(&stroke("Ctrl+Tab")), "prefix suffix");
        app.entered_shortcut.clear();
        assert!(!app.owns_tab_key(&stroke("Ctrl+Tab")));
        app.shortcuts.set(
            CommandId::NextTab,
            "Ctrl+Tab Ctrl+N".parse().expect("Tab prefix"),
        );
        assert!(app.owns_tab_key(&stroke("Ctrl+Tab")), "prefix start");
        app.shortcuts.set(
            CommandId::NextTab,
            "Tab".parse().expect("explicit bare Tab"),
        );
        assert!(app.owns_tab_key(&stroke("Tab")));
    }

    #[test]
    fn shortcut_prefix_restart_keeps_the_new_prefix_and_dispatches_once() {
        let Some(_root) = isolated_test_root(
            "tests::shortcut_prefix_restart_keeps_the_new_prefix_and_dispatches_once",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless app");
        app.media_kind = Some(MediaKind::Video);
        app.shortcuts = ShortcutBindings::default();
        for (command, sequence) in [
            (CommandId::ToggleGridMenu, "Ctrl+J Ctrl+P"),
            (CommandId::ToggleCommandPalette, "Ctrl+K Ctrl+S"),
            (CommandId::ToggleTimeline, "T"),
        ] {
            app.shortcuts
                .set(command, sequence.parse().expect("test binding"));
        }
        let stroke = |key: &str| key.parse::<KeyStroke>().expect("test stroke");
        for first in ["Ctrl+K", "Ctrl+J"] {
            app.grid_open = false;
            app.process_shortcut(stroke(first));
            let old_started = Instant::now() - PREFIX_TIMEOUT / 2;
            app.prefix_started = Some(old_started);
            app.status_message.as_mut().expect("prefix notice").1 = old_started;
            app.process_shortcut(stroke("Ctrl+J"));
            assert_eq!(app.entered_shortcut, [stroke("Ctrl+J")]);
            assert!(app.prefix_started.expect("new prefix deadline") > old_started);
            let (notice, shown) = app.status_message.as_ref().expect("new prefix notice");
            assert_eq!(notice, "Ctrl+J …");
            assert_eq!(Some(*shown), app.prefix_started);
            app.process_shortcut(stroke("Ctrl+P"));
            assert!(app.grid_open);
            assert!(app.entered_shortcut.is_empty());
            assert!(app.prefix_started.is_none());
            assert!(app.status_message.is_none());
            app.process_shortcut(stroke("Ctrl+P"));
            assert!(app.grid_open, "the suffix alone must not toggle again");
        }
        app.process_shortcut(stroke("Ctrl+J"));
        app.process_shortcut(stroke("T"));
        assert!(app.timeline_open, "single-key fallback remains available");
        assert!(app.entered_shortcut.is_empty());
        assert!(app.prefix_started.is_none());
        app.process_shortcut(stroke("Ctrl+J"));
        app.set_status("A later diagnostic".into());
        app.process_shortcut(stroke("Ctrl+X"));
        assert!(app.entered_shortcut.is_empty());
        assert!(app.prefix_started.is_none());
        assert_eq!(
            app.status_message.as_ref().map(|(text, _)| text.as_str()),
            Some("A later diagnostic")
        );
        app.process_shortcut(stroke("Ctrl+K"));
        app.process_shortcut(stroke("Ctrl+S"));
        assert!(
            app.palette_open,
            "an uninterrupted sequence still dispatches"
        );
        assert!(app.entered_shortcut.is_empty());
        assert!(app.prefix_started.is_none());
    }

    #[test]
    fn shortcut_prefix_cancellation_removes_only_its_own_notice() {
        let Some(_root) =
            isolated_test_root("tests::shortcut_prefix_cancellation_removes_only_its_own_notice")
        else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let prefix: KeyStroke = "Ctrl+K".parse().expect("prefix stroke");
        let seed = |app: &mut Application<_>, started| {
            app.entered_shortcut = vec![prefix.clone()];
            app.prefix_started = Some(started);
            app.status_message = Some(("Ctrl+K …".into(), started));
        };
        let now = Instant::now();
        seed(&mut app, now);
        app.expire_shortcut_prefix();
        assert_eq!(app.entered_shortcut, vec![prefix.clone()]);
        assert!(app.status_message.is_some());
        let strokes = [prefix.clone(), "Ctrl+S".parse().expect("suffix stroke")];
        assert_eq!(
            app.shortcuts.resolve(&strokes, app.command_context()),
            ShortcutMatch::Command(CommandId::ReloadShortcuts)
        );
        app.dispatch(CommandId::ToggleGridMenu);
        assert!(app.entered_shortcut.is_empty());
        assert!(app.prefix_started.is_none());
        assert!(app.status_message.is_none());

        seed(&mut app, now - PREFIX_TIMEOUT);
        app.expire_shortcut_prefix();
        assert!(app.entered_shortcut.is_empty());
        assert!(app.prefix_started.is_none());
        assert!(app.status_message.is_none());

        seed(&mut app, now - PREFIX_TIMEOUT);
        app.status_message = Some(("A later diagnostic".into(), now));
        app.expire_shortcut_prefix();
        assert_eq!(
            app.status_message.as_ref().map(|(text, _)| text.as_str()),
            Some("A later diagnostic")
        );
        seed(&mut app, now);
        app.request_guarded(GuardedAction::Exit);
        assert!(app.entered_shortcut.is_empty());
        assert!(app.status_message.is_none());
    }

    #[test]
    fn grid_keys_yield_to_shortcut_modifiers_palette_and_modal_input() {
        let Some(_root) = isolated_test_root(
            "tests::grid_keys_yield_to_shortcut_modifiers_palette_and_modal_input",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let key = PhysicalKey::Code(winit::keyboard::KeyCode::KeyS);
        assert_eq!(app.grid_key_index(key), None);
        app.grid_open = true;
        for modifiers in [ModifiersState::empty(), ModifiersState::SHIFT] {
            app.modifiers = modifiers;
            assert_eq!(app.grid_key_index(key), Some(9));
        }
        for modifier in [
            ModifiersState::CONTROL,
            ModifiersState::ALT,
            ModifiersState::SUPER,
        ] {
            for modifiers in [modifier, modifier | ModifiersState::SHIFT] {
                app.modifiers = modifiers;
                assert_eq!(app.grid_key_index(key), None);
            }
        }
        app.modifiers = ModifiersState::empty();
        app.palette_open = true;
        assert_eq!(app.grid_key_index(key), None);
        app.palette_open = false;
        app.pending_guard = Some(GuardedAction::Exit);
        assert_eq!(app.grid_key_index(key), None);
        app.pending_guard = None;
        app.pending_dialog = Some(DialogIntent::OpenFile);
        assert_eq!(app.grid_key_index(key), None);
        app.pending_dialog = None;
        assert_eq!(app.grid_key_index(key), Some(9));
        app.dispatch(CommandId::ToggleCommandPalette);
        assert!(app.palette_open);
        assert!(!app.grid_open);
    }

    #[test]
    fn covered_filmstrip_rejects_cached_actions_and_resumes_after_overlays() {
        let Some(root) = isolated_test_root(
            "tests::covered_filmstrip_rejects_cached_actions_and_resumes_after_overlays",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let first = root.join("first.wav");
        let second = root.join("second.wav");
        app.tabs.open_new(first.clone(), MediaKind::Audio);
        app.path = Some(first.clone());
        app.media_kind = Some(MediaKind::Audio);
        app.filmstrip_open = true;
        app.folder_snapshot = Some(FolderSnapshot {
            folder_identity: towavue_core::ShellIdentity::new(vec![]),
            folder_path: root.clone(),
            items: [&first, &second]
                .into_iter()
                .map(|path| towavue_core::FolderMediaItem {
                    identity: towavue_core::ShellIdentity::new(vec![]),
                    path: path.clone(),
                    kind: MediaKind::Audio,
                })
                .collect(),
            sort_columns: vec![],
            source: FolderSnapshotSource::LiveExplorerView,
            generation: 1,
            captured_at: std::time::SystemTime::now(),
        });
        let context = egui::Context::default();
        context.enable_accesskit();
        app.ui_context = Some(context.clone());
        let popup = std::cell::Cell::new(false);
        let frame = |app: &mut Application<_>, events| {
            if popup.get() {
                egui::Popup::open_id(&context, "covered-filmstrip-menu".into());
            }
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960.0, 576.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| app.draw_ui(ui, &mut actions),
            );
            (
                output.platform_output.accesskit_update.expect("tree"),
                actions,
            )
        };
        frame(&mut app, vec![]);
        let tree = frame(&mut app, vec![]).0;
        let target = tree
            .nodes
            .iter()
            .find(|(_, node)| {
                node.label() == Some("second.wav") && node.role() == egui::accesskit::Role::Button
            })
            .expect("filmstrip second item")
            .0;
        let request = |action| {
            egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
                action,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: target,
                data: None,
            })
        };
        for overlay in 0..3 {
            app.palette_open = overlay == 0;
            app.grid_open = overlay == 1;
            popup.set(overlay == 2);
            frame(&mut app, vec![]);
            let (tree, actions) = frame(&mut app, vec![request(egui::accesskit::Action::Click)]);
            assert!(
                actions.is_empty(),
                "covered filmstrip action must not select media"
            );
            assert!(
                tree.nodes
                    .iter()
                    .find(|(id, _)| *id == target)
                    .expect("stable item")
                    .1
                    .is_disabled()
            );
            let (tree, actions) = frame(&mut app, vec![request(egui::accesskit::Action::Focus)]);
            assert!(actions.is_empty());
            assert_ne!(tree.focus, target);
            assert_eq!(app.path.as_ref(), Some(&first));
            if overlay < 2 {
                assert!(app.dismiss_overlay_or_fullscreen());
                assert!(app.filmstrip_open, "close only the top command overlay");
            }
            app.palette_open = false;
            app.grid_open = false;
            popup.set(false);
            egui::Popup::close_all(&context);
            frame(&mut app, vec![]);
            let (tree, actions) = frame(&mut app, vec![request(egui::accesskit::Action::Click)]);
            assert!(
                !tree
                    .nodes
                    .iter()
                    .find(|(id, _)| *id == target)
                    .expect("same item")
                    .1
                    .is_disabled()
            );
            assert!(
                matches!(actions.as_slice(), [UiAction::OpenMedia(path, false)] if path == &second)
            );
        }
    }

    #[test]
    fn filmstrip_focus_returns_to_the_origin_or_current_media_controls() {
        let Some(root) = isolated_test_root(
            "tests::filmstrip_focus_returns_to_the_origin_or_current_media_controls",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let path = root.join("first.png");
        app.tabs.open_new(path.clone(), MediaKind::Image);
        app.path = Some(path.clone());
        app.media_kind = Some(MediaKind::Image);
        app.folder_snapshot = Some(FolderSnapshot {
            folder_identity: towavue_core::ShellIdentity::new(vec![]),
            folder_path: root.clone(),
            items: vec![towavue_core::FolderMediaItem {
                identity: towavue_core::ShellIdentity::new(vec![]),
                path: path.clone(),
                kind: MediaKind::Image,
            }],
            sort_columns: vec![],
            source: FolderSnapshotSource::LiveExplorerView,
            generation: 1,
            captured_at: std::time::SystemTime::now(),
        });
        let context = egui::Context::default();
        context.enable_accesskit();
        app.ui_context = Some(context.clone());
        let frame = |app: &mut Application<_>, events| {
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960.0, 576.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| app.draw_ui(ui, &mut actions),
            );
            for action in actions {
                app.handle_ui_action(action);
            }
            output.platform_output.accesskit_update.expect("tree")
        };
        let focus = |target| {
            egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::Focus,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: target,
                data: None,
            })
        };
        frame(&mut app, vec![]);
        let tree = frame(&mut app, vec![]);
        let origin = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("first.png"))
            .expect("invoking tab")
            .0;
        for via_palette in [false, true] {
            frame(&mut app, vec![focus(origin)]);
            if via_palette {
                app.dispatch(CommandId::ToggleCommandPalette);
                frame(&mut app, vec![]);
            }
            app.dispatch(CommandId::ToggleFilmstrip);
            frame(&mut app, vec![]);
            let tree = frame(&mut app, vec![]);
            let item = tree
                .nodes
                .iter()
                .find(|(_, node)| {
                    node.description()
                        .is_some_and(|text| text.ends_with("(current item)"))
                })
                .expect("current filmstrip item")
                .0;
            assert_eq!(tree.focus, item, "opening focuses the current item");
            if via_palette {
                app.push_edit(EditOperation::RotateClockwise);
                app.request_guarded(GuardedAction::Navigate(root.join("other.png")));
                frame(&mut app, vec![]);
                assert!(app.pending_guard.is_some());
                app.resolve_guard(GuardDecision::Cancel);
                for _ in 0..3 {
                    frame(&mut app, vec![]);
                }
                assert!(app.filmstrip_open);
                assert_eq!(frame(&mut app, vec![]).focus, item);
            }
            assert!(app.dismiss_overlay_or_fullscreen());
            assert_eq!(frame(&mut app, vec![]).focus, origin);
            if via_palette {
                app.dispatch(CommandId::Undo);
            }
        }
        app.dispatch(CommandId::ToggleFilmstrip);
        frame(&mut app, vec![]);
        let next = root.join("second.png");
        app.tabs.open_new(next.clone(), MediaKind::Image);
        app.path = Some(next);
        app.media_generation += 1;
        assert!(app.dismiss_overlay_or_fullscreen());
        let tree = frame(&mut app, vec![]);
        let current = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("second.png"))
            .expect("current tab")
            .0;
        assert_eq!(
            tree.focus, current,
            "changed media does not restore the old tab"
        );
        frame(&mut app, vec![focus(origin)]);
        assert_eq!(frame(&mut app, vec![]).focus, origin, "return is one-shot");
        app.fullscreen = true;
        app.dispatch(CommandId::ToggleFilmstrip);
        app.media_generation += 1;
        assert!(app.dismiss_overlay_or_fullscreen());
        for _ in 0..3 {
            frame(&mut app, vec![]);
        }
        let tree = frame(&mut app, vec![]);
        assert!(app.fullscreen);
        let focused = tree
            .nodes
            .iter()
            .find(|(id, _)| *id == tree.focus)
            .expect("fullscreen focus");
        assert!(
            focused
                .1
                .label()
                .is_some_and(|label| label.starts_with("Exit fullscreen"))
        );
        app.fullscreen = false;
        app.dispatch(CommandId::ToggleFilmstrip);
        while let Some(tab) = app.tabs.active().map(|tab| tab.id) {
            app.tabs.close(tab);
        }
        app.path = None;
        app.media_kind = None;
        app.media_generation += 1;
        assert!(app.dismiss_overlay_or_fullscreen());
        let tree = frame(&mut app, vec![]);
        assert!(
            tree.nodes
                .iter()
                .any(|(id, node)| { *id == tree.focus && node.label() == Some("towavue menu") })
        );
    }

    #[test]
    fn configured_grid_actions_preserve_only_cancellation_focus() {
        let Some(root) =
            isolated_test_root("tests::configured_grid_actions_preserve_only_cancellation_focus")
        else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let path = root.join("image.png");
        let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
        app.path = Some(path);
        app.media_kind = Some(MediaKind::Image);
        let context = egui::Context::default();
        context.enable_accesskit();
        app.ui_context = Some(context.clone());
        let mut time = 0.0;
        let mut frame = |app: &mut Application<_>, events, focus_origin| {
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let response = ui.button("Origin");
                    if focus_origin {
                        response.request_focus();
                    }
                    if app.palette_open {
                        app.draw_command_palette(&context, &mut actions);
                    }
                    app.draw_grid_menu(&context, &mut actions);
                },
            );
            time += 0.1;
            for action in actions {
                app.handle_ui_action(action);
            }
            output.platform_output.accesskit_update.expect("tree")
        };
        for command in [
            CommandId::ToggleCommandPalette,
            CommandId::ToggleGridMenu,
            CommandId::ToggleFilmstrip,
            CommandId::RotateClockwise,
        ] {
            std::fs::write(
                &app.grid_path,
                format!("image = {}\n", [command.as_str(); 16].join(", ")),
            )
            .expect("isolated grid configuration");
            app.grid_layouts = grid::load().expect("custom grid").0;
            let origin = frame(&mut app, vec![], true).focus;
            app.dispatch(CommandId::ToggleGridMenu);
            frame(&mut app, vec![], false);
            let tree = frame(&mut app, vec![], false);
            let target = tree
                .nodes
                .iter()
                .find(|(_, node)| {
                    node.role() == egui::accesskit::Role::Button
                        && node.label().is_some_and(|label| label.starts_with("1\n"))
                })
                .expect("first configured cell")
                .0;
            frame(
                &mut app,
                vec![egui::Event::AccessKitActionRequest(
                    egui::accesskit::ActionRequest {
                        action: egui::accesskit::Action::Click,
                        target_tree: egui::accesskit::TreeId::ROOT,
                        target_node: target,
                        data: None,
                    },
                )],
                false,
            );
            assert!(!app.grid_open);
            if command == CommandId::ToggleCommandPalette {
                assert!(app.palette_open);
                frame(&mut app, vec![], false);
                app.cancel_command_overlay();
            }
            if command == CommandId::ToggleFilmstrip {
                assert!(app.filmstrip_open);
                app.close_filmstrip();
            }
            assert!(app.command_overlay_return_focus.is_none());
            if command == CommandId::RotateClockwise {
                assert!(app.edits[&tab].is_dirty());
                app.dispatch(CommandId::Undo);
            } else {
                assert_eq!(frame(&mut app, vec![], false).focus, origin);
            }
        }
    }

    #[test]
    fn grid_cells_remain_inside_small_windows_with_long_labels_and_paths() {
        let Some(_root) = isolated_test_root(
            "tests::grid_cells_remain_inside_small_windows_with_long_labels_and_paths",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        app.grid_open = true;
        app.grid_path = PathBuf::from("long-directory-name/".repeat(30)).join("grid.conf");
        for kind in [MediaKind::Image, MediaKind::Video, MediaKind::Audio] {
            app.media_kind = Some(kind);
            for size in [
                egui::vec2(960.0, 576.0),
                egui::vec2(480.0, 300.0),
                egui::vec2(320.0, 200.0),
            ] {
                app.grid_open = true;
                let context = egui::Context::default();
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                let mut output = egui::FullOutput::default();
                for frame in 0..5 {
                    output = context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(screen),
                            time: Some(frame as f64 * 0.1),
                            ..Default::default()
                        },
                        |_| app.draw_grid_menu(&context, &mut Vec::new()),
                    );
                }
                let mut labels = 0;
                let mut target = None;
                for shape in &output.shapes {
                    if let egui::Shape::Text(text) = &shape.shape {
                        let bounds = egui::Rect::from_min_size(text.pos, text.galley.size());
                        assert!(
                            screen.contains_rect(bounds),
                            "{kind:?}, {size:?}: {bounds:?}"
                        );
                        if grid::KEYS
                            .iter()
                            .any(|key| text.galley.text().starts_with(&format!("{key}\n")))
                        {
                            assert!(text.galley.rows.len() >= 2);
                            labels += 1;
                        }
                        if text.galley.text().starts_with("w\n") {
                            target = Some(bounds.center());
                        }
                    }
                }
                assert_eq!(labels, 16);
                let target = target.expect("second row second cell");
                let mut actions = Vec::new();
                for (frame, pressed) in [true, false, true, false].into_iter().enumerate() {
                    let _ = context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(screen),
                            time: Some(0.5 + frame as f64 * 0.01),
                            events: vec![
                                egui::Event::PointerMoved(target),
                                egui::Event::PointerButton {
                                    pos: target,
                                    button: egui::PointerButton::Primary,
                                    pressed,
                                    modifiers: egui::Modifiers::NONE,
                                },
                            ],
                            ..Default::default()
                        },
                        |_| app.draw_grid_menu(&context, &mut actions),
                    );
                }
                assert!(!app.grid_open);
                assert!(
                    matches!(actions.as_slice(), [UiAction::Command(command)] if *command == app.grid_layouts.get(kind)[5])
                );
            }
        }
    }

    #[test]
    fn grid_repaint_stops_after_open_and_close_animations_settle() {
        let Some(_root) =
            isolated_test_root("tests::grid_repaint_stops_after_open_and_close_animations_settle")
        else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        app.media_kind = Some(MediaKind::Image);
        let context = egui::Context::default();
        let mut time = 0.0;
        for open in [false, true, false] {
            app.grid_open = open;
            let mut repaint_delay = Duration::ZERO;
            for _ in 0..20 {
                let output = context.run_ui(
                    egui::RawInput {
                        time: Some(time),
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(960.0, 576.0),
                        )),
                        ..Default::default()
                    },
                    |_| app.draw_grid_menu(&context, &mut Vec::new()),
                );
                repaint_delay = output.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
                time += 0.1;
            }
            assert!(
                repaint_delay > Duration::from_secs(1),
                "settled grid (open={open}) still requests continuous repaint"
            );
        }
    }

    #[test]
    fn audio_ui_uses_a_deadline_while_playing_and_settles_when_paused() {
        let now = Instant::now();
        assert_eq!(
            ui_repaint_deadline(now, Duration::MAX, true),
            Some(now + AUDIO_EVENT_POLL_INTERVAL)
        );
        assert_eq!(ui_repaint_deadline(now, Duration::MAX, false), None);
        for playing_audio in [false, true] {
            assert_eq!(
                ui_repaint_deadline(now, Duration::ZERO, playing_audio),
                Some(now)
            );
            let tooltip = Duration::from_millis(5);
            assert_eq!(
                ui_repaint_deadline(now, tooltip, playing_audio),
                Some(now + tooltip)
            );
        }
    }

    #[test]
    fn idle_wakeup_includes_status_and_shortcut_expiry_without_media() {
        let Some(_root) = isolated_test_root(
            "tests::idle_wakeup_includes_status_and_shortcut_expiry_without_media",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let now = Instant::now();
        assert_eq!(app.idle_wakeup(now), None);
        app.status_message = Some(("Reloaded".into(), now));
        assert_eq!(app.idle_wakeup(now), Some(now + STATUS_MESSAGE_DURATION));
        app.prefix_started = Some(now);
        assert_eq!(app.idle_wakeup(now), Some(now + PREFIX_TIMEOUT));
        app.ui_repaint_at = Some(now);
        assert_eq!(app.idle_wakeup(now), Some(now));
        app.ui_repaint_at = None;
        app.status_message = None;
        assert_eq!(app.idle_wakeup(now), Some(now + PREFIX_TIMEOUT));
        app.prefix_started = None;
        assert_eq!(app.idle_wakeup(now), None);
    }

    #[test]
    fn file_drops_preserve_edits_and_obey_modal_guards() {
        let Some(root) =
            isolated_test_root("tests::file_drops_preserve_edits_and_obey_modal_guards")
        else {
            return;
        };
        for name in ["one.png", "two.png", "one.wav", "two.wav", "notes.txt"] {
            std::fs::write(root.join(name), []).expect("media placeholder");
        }
        let mut app = Application::new(None, |_| {}).expect("headless application");
        app.open_dropped_path(root.join("one.png"));
        let image_tab = app.tabs.active().expect("image tab").id;
        app.dispatch(CommandId::RotateClockwise);
        app.open_dropped_path(root.join("two.png"));
        assert_eq!(app.tabs.tabs().len(), 2);
        assert!(app.edits[&image_tab].is_dirty());
        let tabs = app.tabs.clone();
        for blocked in 0..3 {
            match blocked {
                0 => app.pending_guard = Some(GuardedAction::CloseTab(image_tab)),
                1 => app.pending_dialog = Some(DialogIntent::OpenFile),
                _ => app.export_error = Some("export failure".into()),
            }
            app.open_dropped_path(root.join("one.png"));
            app.open_dropped_path(root.clone());
            assert_eq!(app.tabs, tabs);
            assert!(!matches!(app.pending_folder, Some((_, FolderIntent::Open))));
            assert!(app.edits[&image_tab].is_dirty());
            app.pending_guard = None;
            app.pending_dialog = None;
            app.export_error = None;
        }
        for rejected in ["notes.txt", "missing.png"] {
            app.open_dropped_path(root.join(rejected));
            assert_eq!(app.tabs, tabs);
            assert!(app.edits[&image_tab].is_dirty());
        }
        app.open_dropped_path(root.join("one.wav"));
        let audio_tab = app.tabs.active().expect("audio tab").id;
        app.open_dropped_path(root.join("two.wav"));
        assert_eq!(app.tabs.active().expect("reused playlist").id, audio_tab);
        app.push_edit(EditOperation::SetVolume(0.5));
        app.open_dropped_path(root.join("one.wav"));
        assert_ne!(app.tabs.active().expect("new playlist").id, audio_tab);
        assert!(app.edits[&audio_tab].is_dirty());
        assert_eq!(app.tabs.tabs().len(), 4);
    }

    #[test]
    fn folder_drop_uses_asynchronous_shell_open() {
        let Some(root) = isolated_test_root("tests::folder_drop_uses_asynchronous_shell_open")
        else {
            return;
        };
        let folder = root.join("folder");
        std::fs::create_dir(&folder).expect("fixture folder");
        let source = folder.join("one.png");
        std::fs::write(&source, []).expect("media placeholder");
        let mut app = Application::new(None, |_| {}).expect("headless application");
        app.open_dropped_path(folder.clone());
        assert!(matches!(app.pending_folder, Some((_, FolderIntent::Open))));
        assert!(app.tabs.tabs().is_empty());
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.pending_folder.is_some() && Instant::now() < deadline {
            app.finish_folder_load();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(app.pending_folder.is_none());
        assert_eq!(app.path, Some(source));
        assert_eq!(
            app.folder_snapshot.expect("Shell snapshot").folder_path,
            folder
        );
    }

    #[test]
    fn empty_folder_open_preserves_current_navigation_and_edits() {
        let Some(root) =
            isolated_test_root("tests::empty_folder_open_preserves_current_navigation_and_edits")
        else {
            return;
        };
        let media = root.join("media");
        let empty = root.join("empty");
        let unsupported = root.join("unsupported");
        for folder in [&media, &empty, &unsupported] {
            std::fs::create_dir(folder).expect("create fixture folder");
        }
        // These placeholders exercise folder selection, not image decoding or GPU presentation.
        for name in ["one.png", "two.png"] {
            std::fs::write(media.join(name), []).expect("create media placeholder");
        }
        std::fs::write(unsupported.join("notes.txt"), b"not media").expect("create non-media file");
        let mut app = Application::new(None, |_| {}).expect("create headless application");
        let wait_for_folder = |app: &mut Application<_>| {
            let deadline = Instant::now() + Duration::from_secs(10);
            while app.pending_folder.is_some() && Instant::now() < deadline {
                app.finish_folder_load();
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(app.pending_folder.is_none(), "folder request completed");
        };
        app.open_external(media.join("one.png"), false);
        wait_for_folder(&mut app);
        app.dispatch(CommandId::RotateClockwise);
        let path = app.path.clone();
        let tabs = app.tabs.clone();
        let snapshot = app
            .folder_snapshot
            .clone()
            .expect("initial folder snapshot");
        let edit = app.edit_state();
        assert!(app.edits.values().any(EditHistory::is_dirty));
        for rejected in [empty, unsupported] {
            app.open_folder_path(rejected);
            wait_for_folder(&mut app);
            assert_eq!(app.path, path);
            assert_eq!(app.tabs, tabs);
            let current = app.folder_snapshot.as_ref().expect("retained snapshot");
            assert_eq!(current.folder_path, snapshot.folder_path);
            // The follow-up refresh may change PIDL metadata without changing navigation.
            let navigation = |snapshot: &FolderSnapshot| {
                snapshot
                    .items
                    .iter()
                    .map(|item| (item.path.clone(), item.kind))
                    .collect::<Vec<_>>()
            };
            assert_eq!(navigation(current), navigation(&snapshot));
            assert_eq!(app.edit_state(), edit);
            assert!(app.edits.values().any(EditHistory::is_dirty));
            assert!(
                app.status_message
                    .as_ref()
                    .expect("empty-folder status")
                    .0
                    .starts_with("No supported media")
            );
        }
        app.open_folder_path(root.join("empty"));
        let open_generation = app.pending_folder.as_ref().expect("pending Open").0;
        app.refresh_folder_snapshot();
        assert_eq!(
            app.pending_folder
                .as_ref()
                .expect("Open survives background refresh")
                .0,
            open_generation
        );
        app.open_external(media.join("two.png"), false);
        wait_for_folder(&mut app);
        assert_eq!(app.path.as_deref(), Some(media.join("two.png").as_path()));
        assert_eq!(
            app.folder_snapshot
                .as_ref()
                .expect("current folder")
                .folder_path,
            media
        );
        app.open_folder_path(root.join("empty"));
        let ids: Vec<_> = app.tabs.tabs().iter().map(|tab| tab.id).collect();
        for id in ids {
            app.close_tab_unchecked(id);
        }
        assert!(app.pending_folder.is_none());
        assert!(app.folder_snapshot.is_none());
        assert!(app.path.is_none());
    }

    #[test]
    fn asynchronous_dialogs_preserve_guards_and_reject_changed_export_sources() {
        let Some(root) = isolated_test_root(
            "tests::asynchronous_dialogs_preserve_guards_and_reject_changed_export_sources",
        ) else {
            return;
        };
        let media = root.join("media");
        std::fs::create_dir(&media).expect("fixture folder");
        for name in ["one.png", "two.png"] {
            std::fs::write(media.join(name), []).expect("media placeholder");
        }
        let mut app = Application::new(None, |_| {}).expect("headless application");
        app.open_external(media.join("one.png"), false);
        app.dispatch(CommandId::RotateClockwise);
        let tab = app.tabs.active().expect("active tab").id;
        let source = app.path.clone().expect("source path");
        let intent = || DialogIntent::Export {
            tab,
            source: source.clone(),
            kind: MediaKind::Image,
            continuation: Some(GuardedAction::CloseTab(tab)),
        };
        for result in [Ok(None), Err(DialogError::OwnerUnavailable)] {
            app.pending_dialog = Some(intent());
            app.dispatch(CommandId::RotateClockwise);
            app.request_guarded(GuardedAction::Exit);
            assert!(!app.exit_requested);
            assert_eq!(
                app.edits
                    .get(&tab)
                    .expect("edit history")
                    .operations()
                    .len(),
                1
            );
            app.finish_dialog(result);
            assert!(app.pending_dialog.is_none());
            assert!(app.active_export.is_none());
            assert!(
                matches!(app.pending_guard.take(), Some(GuardedAction::CloseTab(id)) if id == tab)
            );
            assert!(app.edits.get(&tab).expect("retained edits").is_dirty());
        }
        app.pending_dialog = Some(intent());
        app.navigate_to_unchecked(media.join("two.png"));
        let output = root.join("must-not-be-written.png");
        app.finish_dialog(Ok(Some(output.clone())));
        assert!(!output.exists());
        assert!(app.active_export.is_none());
        assert!(matches!(app.pending_guard.take(), Some(GuardedAction::CloseTab(id)) if id == tab));
        assert!(
            app.status_message
                .as_ref()
                .expect("source-change status")
                .0
                .contains("source changed")
        );
        app.pending_dialog = Some(DialogIntent::OpenFile);
        app.finish_dialog(Ok(Some(media.join("one.png"))));
        assert_eq!(app.tabs.tabs().len(), 2);
        assert_eq!(app.path.as_ref(), Some(&source));
        app.pending_dialog = Some(DialogIntent::OpenFolder);
        app.finish_dialog(Ok(None));
        assert_eq!(app.path.as_ref(), Some(&source));
    }

    #[test]
    fn export_cancellation_never_resumes_exit_even_when_publication_wins() {
        let Some(root) = isolated_test_root(
            "tests::export_cancellation_never_resumes_exit_even_when_publication_wins",
        ) else {
            return;
        };
        for phase in 0..3 {
            let (notify, events) = std::sync::mpsc::channel();
            let (release, gate) = std::sync::mpsc::channel();
            let gate = std::sync::Mutex::new(gate);
            let blocked = std::sync::atomic::AtomicBool::new(false);
            let mut app = Application::new(None, move |event| {
                let hold = phase == 0
                    && matches!(event, AppEvent::Export(ExportEvent::Progress(_)))
                    && !blocked.swap(true, std::sync::atomic::Ordering::Relaxed);
                let _ = notify.send(event);
                if hold {
                    gate.lock()
                        .expect("test gate")
                        .recv_timeout(Duration::from_secs(10))
                        .expect("release export");
                }
            })
            .expect("headless application");
            let source = root.join(format!("source-{phase}.ppm"));
            let target = root.join(format!("output-{phase}.png"));
            let source_bytes = b"P6\n2 1\n255\n\xff\x00\x00\x00\xff\x00";
            std::fs::write(&source, source_bytes).expect("source");
            std::fs::write(&target, b"existing output").expect("target");
            let tab = app.tabs.open_new(source.clone(), MediaKind::Image);
            app.path = Some(source.clone());
            app.media_kind = Some(MediaKind::Image);
            app.edits
                .entry(tab)
                .or_default()
                .push(EditOperation::RotateClockwise, MediaKind::Image);
            app.export_paths.insert(tab, target.clone());
            assert!(app.export_current(false, Some(GuardedAction::Exit)));
            app.native_prompt = Some(FallbackPrompt::ExportBusy);
            let next_export = || loop {
                if let AppEvent::Export(event) = events
                    .recv_timeout(Duration::from_secs(10))
                    .expect("export event")
                {
                    break event;
                }
            };
            if phase == 0 {
                assert!(matches!(next_export(), ExportEvent::Progress(_)));
                app.finish_native_prompt(Ok(PromptResponse::Yes));
                assert!(
                    app.active_export
                        .as_ref()
                        .expect("export still running")
                        .cancelling
                );
                release.send(()).expect("release worker after cancellation");
            }
            let finished = loop {
                let event = next_export();
                if matches!(event, ExportEvent::Finished(_)) {
                    break event;
                }
            };
            if phase == 1 {
                app.native_prompt = None;
                app.handle_ui_action(UiAction::CancelExport);
            }
            app.handle_export_event(finished);
            if phase == 2 {
                app.finish_native_prompt(Ok(PromptResponse::Yes));
            }
            assert!(
                !app.exit_requested,
                "cancel must stop the pending exit: phase {phase}"
            );
            assert!(app.export_error.is_none());
            assert_eq!(
                std::fs::read(source).expect("source retained"),
                source_bytes
            );
            let output = std::fs::read(target).expect("output");
            if phase == 0 {
                assert_eq!(output, b"existing output");
                assert!(app.edits[&tab].is_dirty());
                assert!(matches!(app.pending_guard, Some(GuardedAction::Exit)));
                app.native_prompt = Some(FallbackPrompt::Guard);
                app.finish_native_prompt(Ok(PromptResponse::Cancel));
            } else {
                assert!(output.starts_with(b"\x89PNG"));
                assert!(!app.edits[&tab].is_dirty());
                assert!(app.pending_guard.is_none());
            }
        }
    }

    #[test]
    fn export_finishing_during_native_prompt_rechecks_remaining_dirty_tabs() {
        let Some(root) = isolated_test_root(
            "tests::export_finishing_during_native_prompt_rechecks_remaining_dirty_tabs",
        ) else {
            return;
        };
        for recovery in [true, false] {
            let (notify, events) = std::sync::mpsc::channel();
            let mut app = Application::new(None, move |event| {
                let _ = notify.send(event);
            })
            .expect("headless application");
            let source_bytes = b"P6\n2 1\n255\n\xff\x00\x00\x00\xff\x00";
            let mut tabs = Vec::new();
            let mut outputs = Vec::new();
            for index in 0..2 {
                let source = root.join(format!("{recovery}-{index}.ppm"));
                let output = root.join(format!("{recovery}-{index}.png"));
                std::fs::write(&source, source_bytes).expect("tiny export source");
                std::fs::write(&output, b"existing output").expect("existing target");
                let tab = app.tabs.open_new(source, MediaKind::Image);
                app.edits
                    .entry(tab)
                    .or_default()
                    .push(EditOperation::RotateClockwise, MediaKind::Image);
                app.export_paths.insert(tab, output.clone());
                tabs.push(tab);
                outputs.push(output);
            }
            app.tabs.activate(tabs[0]);
            app.path = Some(
                app.tabs
                    .active()
                    .expect("first tab")
                    .target
                    .current_path()
                    .to_owned(),
            );
            app.media_kind = Some(MediaKind::Image);
            assert!(app.export_current(false, Some(GuardedAction::Exit)));
            app.native_prompt = Some(if recovery {
                FallbackPrompt::Recovery {
                    position: MediaTime::ZERO,
                    state: PlaybackState::Paused,
                    error: "test graphics failure".into(),
                }
            } else {
                FallbackPrompt::ExportBusy
            });
            let finish_export = |app: &mut Application<_>| {
                let deadline = Instant::now() + Duration::from_secs(10);
                while app.active_export.is_some() {
                    let event = events
                        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                        .expect("export event");
                    if let AppEvent::Export(event) = event {
                        app.handle_export_event(event);
                    }
                }
                assert!(app.export_error.is_none(), "{:?}", app.export_error);
            };
            finish_export(&mut app);
            assert!(!app.edits[&tabs[0]].is_dirty());
            assert!(app.edits[&tabs[1]].is_dirty());
            assert!(!app.exit_requested);
            assert_eq!(
                app.tabs.active().expect("still owned by prompt").id,
                tabs[0]
            );
            app.finish_native_prompt(Ok(PromptResponse::Cancel));
            assert_eq!(app.tabs.active().expect("next unsaved tab").id, tabs[1]);
            assert!(matches!(app.pending_guard, Some(GuardedAction::Exit)));
            assert!(!app.exit_requested);
            app.native_prompt = Some(FallbackPrompt::Guard);
            app.finish_native_prompt(Ok(PromptResponse::Yes));
            finish_export(&mut app);
            assert!(app.edits.values().all(|history| !history.is_dirty()));
            assert!(app.exit_requested);
            for output in outputs {
                assert!(
                    std::fs::read(output)
                        .expect("published PNG")
                        .starts_with(b"\x89PNG")
                );
            }
            for tab in app.tabs.tabs() {
                assert_eq!(
                    std::fs::read(tab.target.current_path()).expect("source retained"),
                    source_bytes
                );
            }
        }
    }

    #[test]
    fn native_export_failure_keeps_guard_edits_and_existing_output() {
        let Some(root) = isolated_test_root(
            "tests::native_export_failure_keeps_guard_edits_and_existing_output",
        ) else {
            return;
        };
        let (notify, events) = std::sync::mpsc::channel();
        let mut app = Application::new(None, move |event| {
            let _ = notify.send(event);
        })
        .expect("headless application");
        let source = root.join("missing.png");
        let target = root.join("existing.png");
        std::fs::write(&target, b"existing output").expect("existing target");
        let tab = app.tabs.open_new(source.clone(), MediaKind::Image);
        app.path = Some(source);
        app.media_kind = Some(MediaKind::Image);
        app.edits
            .entry(tab)
            .or_default()
            .push(EditOperation::RotateClockwise, MediaKind::Image);
        app.export_paths.insert(tab, target.clone());
        app.pending_guard = Some(GuardedAction::Exit);
        app.native_prompt = Some(FallbackPrompt::Guard);
        app.finish_native_prompt(Ok(PromptResponse::Yes));
        assert!(app.active_export.is_some());
        assert!(app.title().contains("Exporting"));
        assert!(!app.exit_requested);
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.active_export.is_some() {
            let event = events
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("export completion");
            if let AppEvent::Export(event) = event {
                app.handle_export_event(event);
            }
        }
        assert!(app.export_error.is_some());
        assert!(matches!(app.pending_guard, Some(GuardedAction::Exit)));
        assert!(app.edits[&tab].is_dirty());
        assert!(!app.exit_requested);
        assert_eq!(
            std::fs::read(target).expect("retained target"),
            b"existing output"
        );
        app.native_prompt = Some(FallbackPrompt::ExportError);
        app.finish_native_prompt(Ok(PromptResponse::Ok));
        assert!(matches!(app.pending_guard, Some(GuardedAction::Exit)));
        app.native_prompt = Some(FallbackPrompt::Guard);
        app.finish_native_prompt(Ok(PromptResponse::No));
        assert!(
            app.exit_requested,
            "only explicit discard permits leaving after failure"
        );
    }

    #[test]
    fn native_graphics_failure_cancel_preserves_position_edits_and_guard() {
        let Some(_root) = isolated_test_root(
            "tests::native_graphics_failure_cancel_preserves_position_edits_and_guard",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let position = media_time(Duration::from_secs(17));
        let tab = app
            .tabs
            .open_new(PathBuf::from("edited.png"), MediaKind::Image);
        app.edits
            .entry(tab)
            .or_default()
            .push(EditOperation::RotateClockwise, MediaKind::Image);
        let edits = app.edits.clone();
        app.fail_graphics_recovery(
            position,
            PlaybackState::Paused,
            "test graphics failure".into(),
        );
        assert_eq!(app.current_position(), position);
        assert!(
            app.clock
                .as_ref()
                .expect("frozen clock")
                .paused_at
                .is_some()
        );
        assert_eq!(app.state, PlaybackState::Faulted);
        app.pending_guard = Some(GuardedAction::Exit);
        app.native_prompt = app.queued_recovery.take();
        assert!(app.modal_input_blocked());
        app.request_guarded(GuardedAction::Navigate(PathBuf::from("other.png")));
        assert!(matches!(app.pending_guard, Some(GuardedAction::Exit)));
        app.finish_native_prompt(Ok(PromptResponse::Cancel));
        assert!(!app.exit_requested);
        assert_eq!(app.current_position(), position);
        assert!(matches!(app.pending_guard, Some(GuardedAction::Exit)));
        app.native_prompt = Some(FallbackPrompt::Guard);
        app.finish_native_prompt(Ok(PromptResponse::Cancel));
        assert!(app.pending_guard.is_none());
        assert!(!app.exit_requested);

        app.pending_guard = Some(GuardedAction::Exit);
        app.export_error = Some("test export failure".into());
        app.native_prompt = Some(FallbackPrompt::ExportError);
        app.finish_native_prompt(Ok(PromptResponse::Ok));
        assert!(app.export_error.is_none());
        assert!(matches!(app.pending_guard, Some(GuardedAction::Exit)));
        assert!(!app.exit_requested);
        assert_eq!(app.edits, edits);
    }

    #[test]
    fn graphics_recovery_uploads_full_font_and_current_image_frames_without_resetting_them() {
        let Some(_root) = isolated_test_root(
            "tests::graphics_recovery_uploads_full_font_and_current_image_frames_without_resetting_them",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let context = egui::Context::default();
        let _ = context.run_ui(Default::default(), |ui| {
            ui.label("Recovery glyphs");
        });
        let decoded = DecodedImage {
            format: "GIF",
            frames: vec![
                towavue_runtime_windows::DecodedImageFrame {
                    width: 1,
                    height: 1,
                    rgba: vec![255, 0, 0, 255],
                    delay: Duration::from_millis(100),
                },
                towavue_runtime_windows::DecodedImageFrame {
                    width: 1,
                    height: 1,
                    rgba: vec![0, 0, 255, 255],
                    delay: Duration::from_millis(100),
                },
            ],
        };
        let mut image = ImagePresentation::from_decoded(
            &context,
            Path::new("image.gif"),
            decoded.clone().into(),
        )
        .expect("image");
        image.frame_index = 1;
        let deadline = image.next_frame_at;
        let image_id = image.texture.id();
        app.image = Some(image);
        let page = ImagePresentation::from_decoded(&context, Path::new("page.gif"), decoded.into())
            .expect("page");
        let page_id = page.texture.id();
        app.reading_pages = vec![Err("unreadable page".into()), Ok(page)];
        let _ = context.tex_manager().write().take_delta();

        for _ in 0..2 {
            let textures = app.restored_ui_textures(&context);
            assert_eq!(textures.len(), 3);
            assert_eq!(textures[0].0, egui::TextureId::default());
            assert_eq!(textures[1].0, image_id);
            assert_eq!(textures[2].0, page_id);
            assert!(textures.iter().all(|(_, delta)| delta.pos.is_none()));
            let egui::ImageData::Color(font) = &textures[0].1.image;
            assert_eq!(**font, context.fonts(|fonts| fonts.image()));
            let egui::ImageData::Color(image) = &textures[1].1.image;
            assert_eq!(image.pixels, vec![egui::Color32::BLUE]);
            let egui::ImageData::Color(page) = &textures[2].1.image;
            assert_eq!(page.pixels, vec![egui::Color32::RED]);
        }
        let image = app.image.as_ref().expect("image retained");
        assert_eq!(image.frame_index, 1);
        assert_eq!(image.next_frame_at, deadline);
        assert!(app.reading_pages[0].is_err());
    }

    #[test]
    fn unavailable_graphics_recovery_faults_without_panicking_on_redraw() {
        let Some(_root) = isolated_test_root(
            "tests::unavailable_graphics_recovery_faults_without_panicking_on_redraw",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let context = egui::Context::default();
        app.image = Some(
            app.image_texture_cache
                .load(
                    &context,
                    Path::new("image.png"),
                    Arc::new(DecodedImage {
                        format: "PNG",
                        frames: vec![towavue_runtime_windows::DecodedImageFrame {
                            width: 1,
                            height: 1,
                            rgba: vec![255; 4],
                            delay: Duration::ZERO,
                        }],
                    }),
                )
                .expect("cached current image"),
        );
        let _ = context.run_ui(Default::default(), |ui| {
            ui.label("fixture");
        });
        let image_id = app.image.as_ref().expect("image").texture.id();
        assert_eq!(app.image_texture_cache.entries.len(), 1);
        app.pending_seek_started = Some(Instant::now());
        app.recover_graphics_device(MediaTime::ZERO);
        assert!(app.image_texture_cache.entries.is_empty());
        assert_eq!(
            app.image
                .as_ref()
                .expect("current image survives cache clear")
                .texture
                .id(),
            image_id
        );
        assert!(
            app.restored_ui_textures(&context)
                .iter()
                .any(|(id, _)| *id == image_id)
        );
        assert_eq!(app.state, PlaybackState::Faulted);
        assert!(app.pending_seek_started.is_none());
        assert!(app.renderer.is_none());
        app.render_frame();
        assert!(
            app.playback_error
                .as_ref()
                .expect("diagnostic")
                .contains("window was unavailable")
        );
    }

    #[test]
    fn seek_latency_waits_for_video_presentation_and_records_the_original_start_once() {
        let Some(_root) = isolated_test_root(
            "tests::seek_latency_waits_for_video_presentation_and_records_the_original_start_once",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let started = Instant::now() - Duration::from_millis(300);
        app.pending_seek_started = Some(started);
        app.handle_playback_event(PlaybackEvent::VideoReady(app.generation.next()));
        app.handle_playback_event(PlaybackEvent::VideoReady(app.generation));
        app.record_seek_presentation(false);
        assert_eq!(app.pending_seek_started, Some(started));
        assert!(app.seek_latencies.is_empty());

        app.record_seek_presentation(true);
        assert!(app.pending_seek_started.is_none());
        assert_eq!(app.seek_latencies.len(), 1);
        assert!(app.seek_latencies[0] >= Duration::from_millis(300));
        app.record_seek_presentation(true);
        assert_eq!(app.seek_latencies.len(), 1);
    }

    #[test]
    fn failed_or_unavailable_seek_does_not_report_a_later_presentation() {
        let Some(_root) = isolated_test_root(
            "tests::failed_or_unavailable_seek_does_not_report_a_later_presentation",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        app.pending_seek_started = Some(Instant::now());
        app.fail("test playback failure".into());
        app.record_seek_presentation(true);
        assert!(app.pending_seek_started.is_none());
        assert!(app.seek_latencies.is_empty());

        app.pending_seek_started = Some(Instant::now());
        app.seek_to(MediaTime::ZERO);
        app.record_seek_presentation(true);
        assert!(app.pending_seek_started.is_none());
        assert!(app.seek_latencies.is_empty());
    }

    #[test]
    fn late_video_cutoff_is_fresh_and_preserves_paused_and_video_only_frames() {
        let frame = media_time(Duration::from_secs(2));
        let before_stall = media_time(Duration::from_millis(1995));
        let after_stall = media_time(Duration::from_millis(2295));
        assert!(
            frame
                >= late_video_cutoff(PlaybackState::Playing, Some(before_stall))
                    .expect("running audio cutoff")
        );
        assert!(
            frame
                < late_video_cutoff(PlaybackState::Playing, Some(after_stall))
                    .expect("updated audio cutoff")
        );
        assert_eq!(
            late_video_cutoff(PlaybackState::Playing, Some(after_stall)),
            Some(media_time(Duration::from_millis(2255)))
        );
        assert_eq!(late_video_cutoff(PlaybackState::Playing, None), None);
        for state in [
            PlaybackState::Paused,
            PlaybackState::Ended,
            PlaybackState::Loading,
            PlaybackState::Faulted,
        ] {
            assert_eq!(late_video_cutoff(state, Some(after_stall)), None);
        }
        assert!(
            late_video_cutoff(PlaybackState::Playing, Some(MediaTime::ZERO))
                .expect("startup cutoff")
                < MediaTime::ZERO
        );
    }

    #[test]
    fn paused_seek_presents_one_preview_even_when_the_first_frame_is_after_the_audio_clock() {
        let target = media_time(Duration::from_millis(1508));
        let frame = media_time(Duration::from_millis(1533));
        assert!(video_frame_due(frame, Some(target), true));
        assert!(!video_frame_due(frame, Some(target), false));
        assert!(video_frame_due(frame, Some(frame), false));
        assert!(video_frame_due(frame, None, false));
    }

    #[test]
    fn video_fit_preserves_aspect_and_keeps_all_edges_inside_the_media_panel() {
        let viewport = egui::Rect::from_min_max(egui::pos2(0.0, 32.0), egui::pos2(960.0, 546.0));
        for (size, sar) in [
            ((1920, 1080), 1.0),
            ((1080, 1920), 1.0),
            ((720, 576), 16.0 / 15.0),
        ] {
            let rect = fitted_video_rect(viewport, size, sar);
            assert!(viewport.contains_rect(rect));
            assert_eq!(rect.center(), viewport.center());
            assert!((rect.aspect_ratio() - size.0 as f32 * sar / size.1 as f32).abs() < 0.0001);
        }
        let timeline = egui::Rect::from_min_max(viewport.min, viewport.max - egui::vec2(0.0, 96.0));
        assert!(fitted_video_rect(timeline, (1920, 1080), 1.0).bottom() <= timeline.bottom());
    }

    #[test]
    fn playback_clock_scales_source_time_and_deadlines_while_pause_holds_position() {
        for rate in [0.25, 0.5, 1.0, 2.0, 4.0] {
            let mut clock = PlaybackClock::new(MediaTime::from_nanoseconds(10_000_000_000), rate);
            let elapsed = Duration::from_secs(2);
            clock.paused_at = Some(clock.wall_anchor + elapsed);
            let position = clock.position();
            assert_eq!(position.as_seconds_f64(), 10.0 + 2.0 * f64::from(rate));
            assert_eq!(clock.position(), position);
            assert_eq!(clock.due_at(position), clock.wall_anchor + elapsed);
        }
    }

    #[test]
    fn animation_catches_up_after_a_long_deadline_gap() {
        let context = egui::Context::default();
        let decoded = Arc::new(DecodedImage {
            format: "GIF",
            frames: [10, 20, 30]
                .into_iter()
                .map(|milliseconds| towavue_runtime_windows::DecodedImageFrame {
                    width: 1,
                    height: 1,
                    rgba: vec![milliseconds as u8, 0, 0, 255],
                    delay: Duration::from_millis(milliseconds),
                })
                .collect(),
        });
        let mut image = ImagePresentation::from_decoded(&context, Path::new("a.gif"), decoded)
            .expect("animation");
        let start = Instant::now();
        image.next_frame_at = Some(start + Duration::from_millis(10));
        let _ = context.tex_manager().write().take_delta();
        assert!(!image.advance_animation(start + Duration::from_millis(60)));
        assert_eq!(image.next_frame_at, Some(start + Duration::from_millis(70)));
        assert!(context.tex_manager().write().take_delta().set.is_empty());
        let now = start + Duration::from_secs(2 * 24 * 60 * 60) + Duration::from_millis(35);
        assert!(image.advance_animation(now));
        assert_eq!(image.frame_index, 2);
        assert_eq!(image.next_frame_at, Some(now + Duration::from_millis(25)));
        let now = start + Duration::from_secs(3650 * 24 * 60 * 60) + Duration::from_millis(35);
        assert!(!image.advance_animation(now));
        assert_eq!(image.frame_index, 2);
        assert_eq!(image.next_frame_at, Some(now + Duration::from_millis(25)));
    }

    #[test]
    fn animation_deadlines_match_framewise_advancement_at_cycle_boundaries() {
        let context = egui::Context::default();
        let decoded = Arc::new(DecodedImage {
            format: "GIF",
            frames: [10_000_001, 20_000_003, 30_000_007]
                .into_iter()
                .map(|nanoseconds| towavue_runtime_windows::DecodedImageFrame {
                    width: 1,
                    height: 1,
                    rgba: vec![255; 4],
                    delay: Duration::from_nanos(nanoseconds),
                })
                .collect(),
        });
        let start = Instant::now();
        for current in 0..decoded.frames.len() {
            for elapsed in [
                0,
                1,
                10,
                11,
                20_000_003,
                30_000_017,
                60_000_021,
                2_000_000_000,
            ] {
                let mut image =
                    ImagePresentation::from_decoded(&context, Path::new("a.gif"), decoded.clone())
                        .expect("animation");
                image.frame_index = current;
                image.next_frame_at = Some(start + Duration::from_nanos(10));
                let _ = context.tex_manager().write().take_delta();
                let now = start + Duration::from_nanos(elapsed);
                let mut expected_frame = current;
                let mut expected_deadline = image.next_frame_at.expect("deadline");
                while expected_deadline <= now {
                    expected_frame = (expected_frame + 1) % decoded.frames.len();
                    expected_deadline += decoded.frames[expected_frame].delay;
                }
                assert_eq!(image.advance_animation(now), expected_frame != current);
                assert_eq!(image.frame_index, expected_frame);
                assert_eq!(image.next_frame_at, Some(expected_deadline));
                assert_eq!(
                    context.tex_manager().write().take_delta().set.len(),
                    usize::from(expected_frame != current)
                );
            }
        }
    }

    #[test]
    fn selection_edge_ratio_stops_at_bounds_without_moving_its_anchor() {
        let Some(_root) = isolated_test_root(
            "tests::selection_edge_ratio_stops_at_bounds_without_moving_its_anchor",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        for size in [(1_000, 500), (500, 1_000)] {
            for edge in [
                SelectionDrag::Left,
                SelectionDrag::Right,
                SelectionDrag::Top,
                SelectionDrag::Bottom,
            ] {
                let horizontal = matches!(edge, SelectionDrag::Left | SelectionDrag::Right);
                let before = if horizontal {
                    UnitRect {
                        min: UnitPoint { x: 0.4, y: 0.1 },
                        max: UnitPoint { x: 0.6, y: 0.3 },
                    }
                } else {
                    UnitRect {
                        min: UnitPoint { x: 0.1, y: 0.4 },
                        max: UnitPoint { x: 0.3, y: 0.6 },
                    }
                };
                app.image_view.selection = Some(before);
                app.view_drag = Some(ViewDrag::Selection {
                    mode: edge,
                    before: Some(before),
                });
                let collapsed = match edge {
                    SelectionDrag::Left | SelectionDrag::Top => before.max,
                    _ => before.min,
                };
                app.resize_selection(edge, collapsed, true, size);
                let target = match edge {
                    SelectionDrag::Left | SelectionDrag::Top => UnitPoint { x: 0.0, y: 0.0 },
                    _ => UnitPoint { x: 1.0, y: 1.0 },
                };
                app.resize_selection(edge, target, true, size);
                let after = app.image_view.selection.expect("selection");
                assert!(
                    (after.width() / after.height() - before.width() / before.height()).abs()
                        < 0.00001
                );
                assert!(after.min.x >= 0.0 && after.min.y >= 0.0);
                assert!(after.max.x <= 1.0 && after.max.y <= 1.0);
                if horizontal {
                    assert!(
                        (after.min.y + after.max.y - before.min.y - before.max.y).abs() < 0.00001
                    );
                } else {
                    assert!(
                        (after.min.x + after.max.x - before.min.x - before.max.x).abs() < 0.00001
                    );
                }
                match edge {
                    SelectionDrag::Left => assert_eq!(after.max.x, before.max.x),
                    SelectionDrag::Right => assert_eq!(after.min.x, before.min.x),
                    SelectionDrag::Top => assert_eq!(after.max.y, before.max.y),
                    SelectionDrag::Bottom => assert_eq!(after.min.y, before.min.y),
                    _ => unreachable!(),
                }
            }
        }
    }

    #[test]
    fn image_color_conversion_matches_egui_for_opaque_and_transparent_rows() {
        for width in [1, 3, 256, 257] {
            let mut rgba = Vec::new();
            for alpha in 0..=255 {
                for x in 0..width {
                    let value = x as u8;
                    rgba.extend_from_slice(&[value, 255 - value, value / 2, alpha]);
                }
            }
            let mut frame = towavue_runtime_windows::DecodedImageFrame {
                width,
                height: 256,
                rgba,
                delay: Duration::ZERO,
            };
            let compare = |frame: &towavue_runtime_windows::DecodedImageFrame| {
                assert_eq!(
                    color_image(frame),
                    egui::ColorImage::from_rgba_unmultiplied(
                        [frame.width as usize, frame.height as usize],
                        &frame.rgba,
                    )
                );
            };
            compare(&frame);
            for pixel in frame.rgba.as_chunks_mut::<4>().0 {
                pixel[3] = 255;
            }
            compare(&frame);
            *frame.rgba.last_mut().expect("last alpha") = 128;
            compare(&frame);
            frame.rgba[3] = 0;
            compare(&frame);
        }
    }

    #[test]
    fn failed_image_navigation_keeps_reading_pages_and_recovers_without_restarting() {
        let Some(root) = isolated_test_root(
            "tests::failed_image_navigation_keeps_reading_pages_and_recovers_without_restarting",
        ) else {
            return;
        };
        let broken = root.join("01-broken.bmp");
        let good = root.join("02-good.bmp");
        let mut bitmap = vec![0_u8; 62];
        bitmap[..2].copy_from_slice(b"BM");
        bitmap[2..6].copy_from_slice(&62_u32.to_le_bytes());
        bitmap[10..14].copy_from_slice(&54_u32.to_le_bytes());
        bitmap[14..18].copy_from_slice(&40_u32.to_le_bytes());
        bitmap[18..22].copy_from_slice(&2_u32.to_le_bytes());
        bitmap[22..26].copy_from_slice(&1_u32.to_le_bytes());
        bitmap[26..28].copy_from_slice(&1_u16.to_le_bytes());
        bitmap[28..30].copy_from_slice(&24_u16.to_le_bytes());
        bitmap[34..38].copy_from_slice(&8_u32.to_le_bytes());
        bitmap[54..].copy_from_slice(&[0, 0, 255, 0, 255, 0, 0, 0]);
        std::fs::write(&broken, b"malformed image").expect("broken fixture");
        std::fs::write(&good, &bitmap).expect("valid fixture");
        let (notify, events) = std::sync::mpsc::channel();
        let mut app = Application::new(None, move |event| {
            let _ = notify.send(event);
        })
        .expect("headless application");
        let context = egui::Context::default();
        app.ui_context = Some(context.clone());
        app.tabs.open_new(broken.clone(), MediaKind::Image);
        app.folder_snapshot = Some(FolderSnapshot {
            folder_identity: towavue_core::ShellIdentity::new(vec![0]),
            folder_path: root.clone(),
            items: [&broken, &good]
                .into_iter()
                .enumerate()
                .map(|(index, path)| towavue_core::FolderMediaItem {
                    identity: towavue_core::ShellIdentity::new(vec![index as u8]),
                    path: path.clone(),
                    kind: MediaKind::Image,
                })
                .collect(),
            sort_columns: Vec::new(),
            source: towavue_core::FolderSnapshotSource::LiveExplorerView,
            generation: 1,
            captured_at: std::time::SystemTime::UNIX_EPOCH,
        });
        let wait = |app: &mut Application<_>| {
            let deadline = Instant::now() + Duration::from_secs(5);
            while app.image_loading {
                let event = events
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .expect("image worker completion");
                if matches!(event, AppEvent::ImagesReady) {
                    app.finish_image_load();
                }
            }
        };
        app.reading_mode = true;
        app.load_path(broken.clone(), MediaKind::Image);
        wait(&mut app);
        assert_eq!(app.state, PlaybackState::Faulted);
        assert!(app.image.is_none() && app.image_error.is_some());
        let page = app.reading_pages[0]
            .as_ref()
            .expect("valid second page")
            .texture
            .id();
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(960.0, 576.0),
                )),
                ..Default::default()
            },
            |ui| app.draw_image(ui),
        );
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Mesh(mesh) if mesh.texture_id == page
        )));
        app.dispatch(CommandId::NextImage);
        wait(&mut app);
        assert_eq!(app.path.as_ref(), Some(&good));
        assert_eq!(app.state, PlaybackState::Paused);
        assert!(app.image_error.is_none() && app.playback_error.is_none());
        app.dispatch(CommandId::PreviousImage);
        wait(&mut app);
        assert_eq!(app.state, PlaybackState::Faulted);
        std::fs::write(&broken, &bitmap).expect("repair owned fixture");
        app.dispatch(CommandId::NextImage);
        wait(&mut app);
        app.dispatch(CommandId::PreviousImage);
        wait(&mut app);
        assert_eq!(app.path.as_ref(), Some(&broken));
        assert_eq!(app.state, PlaybackState::Paused);
        assert!(app.image_error.is_none() && app.playback_error.is_none());
        assert_eq!(
            app.image.as_ref().expect("repaired image").decoded.frames[0].rgba,
            [255, 0, 0, 255, 0, 255, 0, 255]
        );
        app.dispatch(CommandId::CloseTab);
        assert!(app.tabs.tabs().is_empty() && app.path.is_none());
        assert!(app.image.is_none() && app.image_error.is_none());
        assert!(app.reading_pages.is_empty() && !app.image_loading);
    }

    #[test]
    fn image_texture_cache_reuses_only_shared_static_decodes_within_its_limits() {
        let context = egui::Context::default();
        let make_image = |value| {
            Arc::new(DecodedImage {
                format: "PNG",
                frames: vec![towavue_runtime_windows::DecodedImageFrame {
                    width: 1,
                    height: 1,
                    rgba: vec![value; 4],
                    delay: Duration::ZERO,
                }],
            })
        };
        let mut cache = ImageTextureCache::new(8);
        let first = make_image(10);
        let first_id = cache
            .load(&context, Path::new("same.png"), first.clone())
            .expect("first")
            .texture
            .id();
        let _ = context.tex_manager().write().take_delta();
        assert_eq!(
            cache
                .load(&context, Path::new("same.png"), first.clone())
                .expect("cached")
                .texture
                .id(),
            first_id
        );
        assert!(
            context.tex_manager().write().take_delta().set.is_empty(),
            "cache hit must not enqueue another upload"
        );
        let second = make_image(20);
        let second_id = cache
            .load(&context, Path::new("same.png"), second.clone())
            .expect("changed content")
            .texture
            .id();
        assert_ne!(
            first_id, second_id,
            "same path with a new decode must not use stale pixels"
        );
        cache
            .load(&context, Path::new("first.png"), first.clone())
            .expect("refresh LRU");
        cache
            .load(&context, Path::new("third.png"), make_image(30))
            .expect("evict second");
        assert_eq!(cache.entries.len(), 2);
        assert_ne!(
            cache
                .load(&context, Path::new("second.png"), second)
                .expect("reupload evicted")
                .texture
                .id(),
            second_id
        );
        let mut animation = (*make_image(40)).clone();
        animation.frames.push(animation.frames[0].clone());
        let animation = Arc::new(animation);
        let a = cache
            .load(&context, Path::new("a.gif"), animation.clone())
            .expect("animation");
        let b = cache
            .load(&context, Path::new("a.gif"), animation)
            .expect("uncached animation");
        assert_ne!(a.texture.id(), b.texture.id());
        assert_eq!(cache.entries.len(), 2);
        let mut cache = ImageTextureCache::new(3);
        cache
            .load(&context, Path::new("large.png"), first.clone())
            .expect("display larger than cache");
        assert!(cache.entries.is_empty());
        let mut cache = ImageTextureCache::new(100);
        for value in 0..9 {
            cache
                .load(&context, Path::new("page.png"), make_image(value))
                .expect("page");
        }
        assert_eq!(cache.entries.len(), 8);
        cache.entries.clear();
        assert_ne!(
            cache
                .load(&context, Path::new("same.png"), first)
                .expect("reload after clear")
                .texture
                .id(),
            first_id
        );
    }

    #[test]
    fn image_texture_creation_obeys_configured_device_limit_without_panicking() {
        let context = egui::Context::default();
        let decoded = DecodedImage {
            format: "PNG",
            frames: vec![towavue_runtime_windows::DecodedImageFrame {
                width: 4096,
                height: 1,
                rgba: vec![255; 4096 * 4],
                delay: Duration::ZERO,
            }],
        };
        assert!(
            ImagePresentation::from_decoded(
                &context,
                Path::new("wide.png"),
                decoded.clone().into()
            )
            .is_err()
        );
        context.input_mut(|input| input.max_texture_side = 8192);
        let image =
            ImagePresentation::from_decoded(&context, Path::new("wide.png"), decoded.into())
                .expect("device supports 4096px image");
        assert_eq!(image.dimensions(), (4096, 1));
    }

    #[test]
    fn formats_compact_status_values() {
        assert_eq!(
            format_time(MediaTime::from_nanoseconds(125_000_000_000)),
            "02:05"
        );
        assert_eq!(format_size(1_500_000), "1.5 MB");
    }

    #[test]
    fn source_labels_distinguish_shell_fallback() {
        assert_ne!(
            snapshot_source(FolderSnapshotSource::LiveExplorerView),
            snapshot_source(FolderSnapshotSource::NaturalNameFallback)
        );
    }

    #[test]
    fn reading_draw_joins_pages_and_centers_the_spread_without_changing_media() {
        let Some(root) = isolated_test_root(
            "tests::reading_draw_joins_pages_and_centers_the_spread_without_changing_media",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let context = egui::Context::default();
        let make_page = |width, height| {
            ImagePresentation::from_decoded(
                &context,
                &root.join("page.png"),
                DecodedImage {
                    format: "PNG",
                    frames: vec![towavue_runtime_windows::DecodedImageFrame {
                        width,
                        height,
                        rgba: vec![255; (width * height * 4) as usize],
                        delay: Duration::ZERO,
                    }],
                }
                .into(),
            )
            .expect("page")
        };
        app.image = Some(make_page(8, 16));
        app.reading_pages = vec![Ok(make_page(16, 8))];
        let ids = [
            app.image.as_ref().expect("first").texture.id(),
            app.reading_pages[0].as_ref().expect("second").texture.id(),
        ];
        let generation = app.image_generation;
        for axis in [ReadingAxis::Horizontal, ReadingAxis::Vertical] {
            for reversed in [false, true] {
                for fullscreen in [false, true] {
                    for size in [
                        egui::vec2(960.0, 514.0),
                        egui::vec2(480.0, 238.0),
                        egui::vec2(200.0, 400.0),
                    ] {
                        let viewport = egui::Rect::from_min_size(egui::pos2(0.0, 32.0), size);
                        app.reading_settings.axis = axis;
                        app.reading_settings.reversed = reversed;
                        app.fullscreen = fullscreen;
                        let output = context.run_ui(
                            egui::RawInput {
                                screen_rect: Some(viewport),
                                ..Default::default()
                            },
                            |ui| app.draw_reading_pages(ui),
                        );
                        let bounds = ids.map(|id| {
                            output
                                .shapes
                                .iter()
                                .find_map(|shape| match &shape.shape {
                                    egui::Shape::Mesh(mesh) if mesh.texture_id == id => {
                                        Some(mesh.calc_bounds())
                                    }
                                    _ => None,
                                })
                                .expect("drawn page")
                        });
                        let spread = bounds[0].union(bounds[1]);
                        assert!((spread.center() - viewport.center()).length() < 0.01);
                        assert!(viewport.expand(0.01).contains_rect(spread));
                        assert!(
                            (spread.width() - viewport.width()).abs() < 0.01
                                || (spread.height() - viewport.height()).abs() < 0.01
                        );
                        assert!((bounds[0].aspect_ratio() - 0.5).abs() < 0.001);
                        assert!((bounds[1].aspect_ratio() - 2.0).abs() < 0.001);
                        let [first, second] = if reversed {
                            [bounds[1], bounds[0]]
                        } else {
                            bounds
                        };
                        match axis {
                            ReadingAxis::Horizontal => {
                                assert!(
                                    (first.right() - second.left()).abs() < 0.01,
                                    "horizontal seam: {bounds:?}"
                                );
                                assert!((first.height() - second.height()).abs() < 0.01);
                            }
                            ReadingAxis::Vertical => {
                                assert!(
                                    (first.bottom() - second.top()).abs() < 0.01,
                                    "vertical seam: {bounds:?}"
                                );
                                assert!((first.width() - second.width()).abs() < 0.01);
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(app.image_generation, generation);
        assert!(app.edits.is_empty());
        app.reading_pages.insert(0, Err("unreadable page".into()));
        let output = context.run_ui(Default::default(), |ui| app.draw_reading_pages(ui));
        assert!(output.shapes.iter().any(|shape| {
            matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == "unreadable page")
        }));
        for id in ids {
            assert!(output.shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == id)
            }));
        }
    }

    #[test]
    fn reading_page_geometry_follows_the_selected_axis() {
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(300.0, 200.0));

        let sizes = [egui::vec2(3.0, 2.0); 2];
        let horizontal = reading_page_rects(viewport, &sizes, ReadingAxis::Horizontal, false)[1];
        let vertical = reading_page_rects(viewport, &sizes, ReadingAxis::Vertical, false)[1];

        assert_eq!(horizontal.left(), 150.0);
        assert_eq!(horizontal.width(), 150.0);
        assert_eq!(vertical.top(), 100.0);
        assert_eq!(vertical.height(), 100.0);
        for count in [0, 1, 10] {
            for axis in [ReadingAxis::Horizontal, ReadingAxis::Vertical] {
                let rects =
                    reading_page_rects(viewport, &vec![egui::Vec2::splat(1.0); count], axis, true);
                assert_eq!(rects.len(), count);
                assert!(
                    rects
                        .iter()
                        .all(|rect| viewport.expand(0.001).contains_rect(*rect))
                );
                for pair in rects.windows(2) {
                    assert_eq!(
                        match axis {
                            ReadingAxis::Horizontal => pair[0].left() - pair[1].right(),
                            ReadingAxis::Vertical => pair[0].top() - pair[1].bottom(),
                        },
                        0.0
                    );
                }
            }
        }
    }

    #[test]
    fn selection_edge_hit_test_prefers_the_nearest_edge() {
        let image = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 100.0));
        let selection = UnitRect {
            min: UnitPoint { x: 0.2, y: 0.2 },
            max: UnitPoint { x: 0.8, y: 0.8 },
        };

        assert!(matches!(
            selection_edge(egui::pos2(21.0, 50.0), image, selection),
            Some(SelectionDrag::Left)
        ));
        assert!(selection_edge(egui::pos2(50.0, 50.0), image, selection).is_none());
    }

    #[test]
    fn image_transform_applies_crop_then_rotation_in_order() {
        let transform = ImageTransform::new(
            (40, 30),
            &[
                EditOperation::Crop(PixelCrop {
                    x: 10,
                    y: 0,
                    width: 20,
                    height: 30,
                }),
                EditOperation::RotateClockwise,
            ],
        );

        assert_eq!(transform.size, (30.0, 20.0));
        assert_eq!(transform.uv[0], UnitPoint { x: 0.25, y: 1.0 });
        assert_eq!(transform.uv[2], UnitPoint { x: 0.75, y: 0.0 });
    }

    #[test]
    fn export_name_preserves_the_source_extension() {
        assert_eq!(
            export_name(Path::new("photo.final.png")),
            "photo.final-export.png"
        );
    }

    #[test]
    fn keyboard_seek_from_eof_uses_the_paused_preview_path() {
        let Some(root) =
            isolated_test_root("tests::keyboard_seek_from_eof_uses_the_paused_preview_path")
        else {
            return;
        };
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/m1/h264-aac.mp4");
        let path = root.join("video-only.mp4");
        let ffmpeg = PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg"))
            .join("bin/ffmpeg.exe");
        assert!(
            std::process::Command::new(ffmpeg)
                .args(["-v", "error", "-i"])
                .arg(source)
                .args(["-an", "-c:v", "copy"])
                .arg(&path)
                .status()
                .expect("video-only fixture")
                .success()
        );
        struct Trial(PathBuf);
        impl ApplicationHandler for Trial {
            fn resumed(&mut self, event_loop: &ActiveEventLoop) {
                let window = Arc::new(
                    event_loop
                        .create_window(Window::default_attributes().with_visible(false))
                        .expect("hidden seek test window"),
                );
                let renderer = match FrameRenderer::new(&window) {
                    Ok(renderer) => renderer,
                    Err(error) => {
                        eprintln!("SKIP EOF keyboard seek: D3D11 unavailable: {error}");
                        event_loop.exit();
                        return;
                    }
                };
                let mut app = Application::new(None, |_| {}).expect("test app");
                app.window = Some(window);
                app.renderer = Some(renderer);
                app.load_path(self.0.clone(), MediaKind::Video);
                assert_eq!(app.state, PlaybackState::Playing);
                let tab = app.tabs.open_new(self.0.clone(), MediaKind::Video);
                let time = |seconds| media_time(Duration::from_secs(seconds));
                for kind in [MediaKind::Video, MediaKind::Audio] {
                    app.media_kind = Some(kind);
                    for forward in [false, true] {
                        app.seek_to(time(1));
                        app.state = PlaybackState::Ended;
                        let generation = app.generation;
                        app.dispatch(if forward {
                            CommandId::SeekForward
                        } else {
                            CommandId::SeekBackward
                        });
                        assert_eq!(app.generation, generation.next());
                        assert_eq!(app.state, PlaybackState::Paused);
                        assert_eq!(app.current_position(), time(if forward { 6 } else { 0 }));
                        assert!(!app.edits.contains_key(&tab));
                    }
                }
                app.media_kind = Some(MediaKind::Video);
                app.media_duration = Some(Duration::from_secs(2));
                app.state = PlaybackState::Playing;
                app.seek_to(time(999));
                assert_eq!(app.current_position(), time(2));
                assert_eq!(app.state, PlaybackState::Paused);
                let deadline = Instant::now() + Duration::from_secs(5);
                while app.pending_time.is_none() && Instant::now() < deadline {
                    app.load_next_frame();
                    std::thread::sleep(Duration::from_millis(5));
                }
                assert!(app.pending_time.is_some(), "terminal video preview");
                assert_eq!(app.current_position(), time(2));
                assert_eq!(
                    app.session
                        .as_mut()
                        .expect("session")
                        .drop_video_before(time(3)),
                    0,
                    "the terminal preview is not a late playback frame"
                );
                app.advance_media();
                assert!(
                    app.session
                        .as_ref()
                        .expect("session")
                        .video_geometry()
                        .is_some()
                );
                app.toggle_pause();
                assert_eq!(app.state, PlaybackState::Playing);
                assert_eq!(app.current_position(), MediaTime::ZERO);
                app.seek_to(MediaTime::from_nanoseconds(-1));
                assert_eq!(app.current_position(), MediaTime::ZERO);
                let deadline = Instant::now() + Duration::from_secs(5);
                while app.pending_time.is_none() && Instant::now() < deadline {
                    app.load_next_frame();
                    std::thread::sleep(Duration::from_millis(5));
                }
                assert!(app.pending_time.is_some(), "ordinary playback frame");
                assert!(
                    app.session
                        .as_mut()
                        .expect("session")
                        .drop_video_before(time(1))
                        > 0,
                    "ordinary late frames must still be discarded"
                );
                app.edits
                    .entry(tab)
                    .or_default()
                    .push(EditOperation::SetTrimStart(time(1)), MediaKind::Video);
                app.seek_to(time(1));
                app.state = PlaybackState::Ended;
                app.dispatch(CommandId::SeekBackward);
                assert_eq!(app.current_position(), MediaTime::ZERO);
                assert_eq!(app.state, PlaybackState::Paused);
                assert!(app.edits[&tab].is_dirty());
                app.toggle_pause();
                assert_eq!(app.state, PlaybackState::Playing);
                assert_eq!(app.current_position(), time(1));
                app.seek_to(time(999));
                app.toggle_pause();
                assert_eq!(app.state, PlaybackState::Playing);
                assert_eq!(app.current_position(), time(1));
                for state in [PlaybackState::Loading, PlaybackState::Faulted] {
                    app.state = state;
                    let generation = app.generation;
                    app.dispatch(CommandId::SeekBackward);
                    assert_eq!(app.generation, generation);
                    assert_eq!(app.state, state);
                }
                eprintln!("EOF keyboard seek: paused targets and trim restart passed");
                event_loop.exit();
            }

            fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
        }
        use winit::platform::windows::EventLoopBuilderExtWindows;
        let mut builder = EventLoop::builder();
        builder.with_any_thread(true);
        builder
            .build()
            .expect("test event loop")
            .run_app(&mut Trial(path))
            .expect("seek trial");
    }

    #[test]
    fn eof_poll_does_not_replace_faults_or_loading_with_a_successful_end() {
        let Some(_root) = isolated_test_root(
            "tests::eof_poll_does_not_replace_faults_or_loading_with_a_successful_end",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        app.decode_finished = true;
        app.audio_drained = true;
        for state in [
            PlaybackState::Faulted,
            PlaybackState::Loading,
            PlaybackState::Ended,
        ] {
            app.state = state;
            app.check_eof();
            assert_eq!(app.state, state);
        }
        app.state = PlaybackState::Playing;
        app.check_eof();
        assert_eq!(app.state, PlaybackState::Ended);
    }

    #[test]
    fn trim_endpoints_reject_invalid_and_duplicate_edits_without_losing_redo() {
        let Some(root) = isolated_test_root(
            "tests::trim_endpoints_reject_invalid_and_duplicate_edits_without_losing_redo",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let tab = app.tabs.open_new(root.join("trim.mp4"), MediaKind::Video);
        app.media_kind = Some(MediaKind::Video);
        app.state = PlaybackState::Paused;
        let time = |seconds| media_time(Duration::from_secs(seconds));
        app.pending_time = Some(time(2));
        let start = EditOperation::SetTrimStart(time(2));
        app.push_edit(start);
        assert!(!app.edits.contains_key(&tab));
        assert!(
            app.status_message
                .as_ref()
                .expect("duration notice")
                .0
                .contains("duration")
        );
        app.media_duration = Some(Duration::from_secs(30));
        app.push_edit(EditOperation::SetTrimStart(MediaTime::ZERO));
        app.push_edit(EditOperation::SetTrimEnd(time(30)));
        assert!(
            !app.edits.contains_key(&tab),
            "implicit full range is a no-op"
        );
        app.fullscreen = true;
        app.push_edit(start);
        assert!(!app.fullscreen);
        assert!(app.timeline_open);
        assert_eq!(app.state, PlaybackState::Paused);
        assert_eq!(app.pending_time, Some(time(2)));
        let end = EditOperation::SetTrimEnd(time(5));
        app.push_edit(end);
        app.edits.get_mut(&tab).expect("history").mark_saved();
        app.undo_edit(false);
        let history = app.edits[&tab].clone();
        for operation in [
            start,
            EditOperation::SetTrimStart(MediaTime::from_nanoseconds(-1)),
            EditOperation::SetTrimStart(time(30)),
            EditOperation::SetTrimEnd(time(2)),
            EditOperation::SetTrimEnd(time(1)),
            EditOperation::SetTrimEnd(time(31)),
        ] {
            app.push_edit(operation);
            assert_eq!(app.edits[&tab], history, "rejected {operation:?}");
        }
        app.undo_edit(true);
        assert!(
            !app.edits[&tab].is_dirty(),
            "saved redo survived rejected inputs"
        );
        assert_eq!(app.edit_state().trim_end, Some(time(5)));
        app.tabs.open_new(root.join("track.wav"), MediaKind::Audio);
        app.media_kind = Some(MediaKind::Audio);
        assert_eq!(app.edit_state().trim_start, None);
        app.push_edit(end);
        assert_eq!(
            app.edit_state().trim_start,
            None,
            "end-first uses source zero"
        );
        app.undo_edit(false);
        assert_eq!(
            app.status_message.as_ref().expect("undo notice").0,
            "Trim cleared · source playback"
        );
        assert_eq!(app.edits[&tab].state().trim_start, Some(time(2)));
    }

    #[test]
    fn pixel_crop_preserves_rejected_video_selection_and_avoids_no_op_history() {
        let Some(root) = isolated_test_root(
            "tests::pixel_crop_preserves_rejected_video_selection_and_avoids_no_op_history",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let tab = app.tabs.open_new(root.join("tiny.png"), MediaKind::Image);
        app.media_kind = Some(MediaKind::Image);
        let selection = UnitRect {
            min: UnitPoint { x: 0.4, y: 0.3 },
            max: UnitPoint { x: 0.42, y: 0.32 },
        };
        app.image_view.selection = Some(selection);
        app.crop_selection((8, 8));
        assert_eq!(
            app.edits[&tab].operations(),
            &[EditOperation::Crop(PixelCrop {
                x: 3,
                y: 2,
                width: 1,
                height: 1
            })]
        );
        assert_eq!(app.visual_transform((8, 8)).size, (1.0, 1.0));
        app.edits.get_mut(&tab).expect("history").mark_saved();
        app.image_view.selection = Some(UnitRect::FULL);
        app.crop_selection((8, 8));
        assert!(!app.edits[&tab].is_dirty());
        assert_eq!(app.edits[&tab].operations().len(), 1);
        let video = app.tabs.open_new(root.join("video.mp4"), MediaKind::Video);
        app.media_kind = Some(MediaKind::Video);
        app.image_view.selection = Some(selection);
        app.crop_selection((160, 96));
        assert!(!app.edits.contains_key(&video));
        assert_eq!(app.image_view.selection, Some(selection));
        assert!(
            app.status_message
                .as_ref()
                .expect("status")
                .0
                .contains("16 × 16")
        );
        app.dispatch(CommandId::ApplyCrop);
        assert_eq!(app.image_view.selection, Some(selection));
        assert!(
            app.status_message
                .as_ref()
                .expect("status")
                .0
                .contains("load")
        );
    }

    #[test]
    fn cropped_image_mesh_never_samples_neighbors_outside_its_pixel_bounds() {
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 500.0));
        let mut transform = ImageTransform::new(
            (8, 8),
            &[EditOperation::Crop(PixelCrop {
                x: 3,
                y: 2,
                width: 1,
                height: 1,
            })],
        );
        let mesh = transformed_image_mesh(egui::TextureId::Managed(0), rect, transform);
        assert_eq!(mesh.calc_bounds(), rect);
        assert!(
            mesh.vertices
                .iter()
                .all(|vertex| vertex.uv == egui::pos2(3.5 / 8.0, 2.5 / 8.0))
        );
        transform = ImageTransform::new(
            (8, 8),
            &[
                EditOperation::Crop(PixelCrop {
                    x: 2,
                    y: 3,
                    width: 4,
                    height: 2,
                }),
                EditOperation::RotateClockwise,
            ],
        );
        let mesh = transformed_image_mesh(egui::TextureId::Managed(0), rect, transform);
        assert_eq!(mesh.calc_bounds(), rect);
        assert!(
            mesh.vertices
                .iter()
                .all(|vertex| (2.5 / 8.0..=5.5 / 8.0).contains(&vertex.uv.x)
                    && (3.5 / 8.0..=4.5 / 8.0).contains(&vertex.uv.y))
        );
    }

    #[test]
    fn cancellation_discards_undrawn_presses_but_allows_subsequent_presses() {
        let Some(_root) = isolated_test_root(
            "tests::cancellation_discards_undrawn_presses_but_allows_subsequent_presses",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        app.media_kind = Some(MediaKind::Image);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 500.0));
        let start = egui::pos2(100.0, 100.0);
        let end = egui::pos2(300.0, 300.0);
        for button in [egui::PointerButton::Primary, egui::PointerButton::Secondary] {
            for interruption in 0..3 {
                let context = egui::Context::default();
                app.ui_state = Some(egui_winit::State::new(
                    context.clone(),
                    egui::ViewportId::ROOT,
                    &winit::raw_window_handle::DisplayHandle::windows(),
                    Some(1.0),
                    None,
                    None,
                ));
                let original = (interruption == 1).then_some(UnitRect {
                    min: UnitPoint { x: 0.4, y: 0.4 },
                    max: UnitPoint { x: 0.8, y: 0.8 },
                });
                app.image_view.selection = original;
                app.image_view.pan = (10.0, 20.0);
                let event = |pressed, pos| egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                };
                let pending = app.ui_state.as_mut().expect("input state").egui_input_mut();
                let retained = vec![
                    egui::Event::PointerMoved(start),
                    egui::Event::Key {
                        key: egui::Key::A,
                        physical_key: Some(egui::Key::A),
                        pressed: false,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, 1.0),
                        phase: egui::TouchPhase::Move,
                        modifiers: egui::Modifiers::NONE,
                    },
                    event(false, start),
                ];
                pending.events = retained.clone();
                pending.events.push(event(true, start));
                match interruption {
                    0 => assert!(app.cancel_view_drag()),
                    1 => {
                        app.fullscreen = true;
                        assert!(app.dismiss_overlay_or_fullscreen());
                        assert!(app.fullscreen);
                        app.fullscreen = false;
                    }
                    _ => app.dispatch(CommandId::ClearSelection),
                }
                let pending = app.ui_state.as_mut().expect("input state").egui_input_mut();
                assert_eq!(pending.events, retained);
                pending
                    .events
                    .extend([egui::Event::PointerMoved(end), event(false, end)]);
                let canceled = std::mem::take(&mut pending.events);
                for (index, events) in [
                    vec![egui::Event::PointerMoved(start)],
                    canceled,
                    vec![
                        event(true, start),
                        egui::Event::PointerMoved(end),
                        event(false, end),
                    ],
                ]
                .into_iter()
                .enumerate()
                {
                    let _ = context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(screen),
                            events,
                            ..Default::default()
                        },
                        |ui| {
                            let response = ui.interact(
                                screen,
                                "pending-drag".into(),
                                egui::Sense::click_and_drag(),
                            );
                            let pointer = ui.input(|input| input.pointer.hover_pos());
                            if button == egui::PointerButton::Primary {
                                app.update_selection(&response, screen, (500, 500), false, pointer);
                            } else {
                                app.update_pan(&response, pointer);
                            }
                        },
                    );
                    assert_eq!(
                        app.image_view.selection,
                        if index == 2 && button == egui::PointerButton::Primary {
                            Some(UnitRect {
                                min: UnitPoint { x: 0.2, y: 0.2 },
                                max: UnitPoint { x: 0.6, y: 0.6 },
                            })
                        } else {
                            original
                        }
                    );
                    assert_eq!(
                        app.image_view.pan,
                        if index == 2 && button == egui::PointerButton::Secondary {
                            (210.0, 220.0)
                        } else {
                            (10.0, 20.0)
                        }
                    );
                    assert!(app.view_drag.is_none());
                }
            }
        }
    }

    #[test]
    fn batched_view_drags_respect_press_ownership_and_release_position() {
        let Some(_root) = isolated_test_root(
            "tests::batched_view_drags_respect_press_ownership_and_release_position",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        app.media_kind = Some(MediaKind::Image);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 500.0));
        let start = egui::pos2(100.0, 100.0);
        let end = egui::pos2(300.0, 300.0);
        for button in [egui::PointerButton::Primary, egui::PointerButton::Secondary] {
            for blocked in 0..5 {
                let context = egui::Context::default();
                app.image_view.selection = None;
                app.image_view.pan = (10.0, 20.0);
                let event = |pressed, pos| egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                };
                for events in [
                    vec![egui::Event::PointerMoved(start)],
                    vec![
                        event(true, start),
                        egui::Event::PointerMoved(end),
                        event(false, end),
                        egui::Event::PointerMoved(egui::pos2(440.0, 440.0)),
                    ],
                ] {
                    let mut passes = 0;
                    let _ = context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(screen),
                            events,
                            ..Default::default()
                        },
                        |ui| {
                            if blocked == 3 {
                                egui::Area::new("drag-cover".into())
                                    .order(egui::Order::Foreground)
                                    .fixed_pos(egui::pos2(50.0, 50.0))
                                    .show(ui.ctx(), |ui| {
                                        ui.allocate_exact_size(
                                            egui::vec2(100.0, 100.0),
                                            egui::Sense::click_and_drag(),
                                        );
                                    });
                            }
                            if blocked == 1 {
                                ui.disable();
                            }
                            if blocked == 2 {
                                ui.set_clip_rect(egui::Rect::from_min_max(
                                    egui::pos2(150.0, 150.0),
                                    screen.max,
                                ));
                            }
                            let response = ui.interact(
                                screen,
                                "batched-surface".into(),
                                egui::Sense::click_and_drag(),
                            );
                            let pointer = ui.input(|input| input.pointer.hover_pos());
                            if blocked == 4 {
                                ui.ctx().set_dragged_id("another-widget".into());
                            }
                            if button == egui::PointerButton::Primary {
                                app.update_selection(&response, screen, (500, 500), false, pointer);
                            } else {
                                app.update_pan(&response, pointer);
                            }
                            passes += 1;
                            if passes == 1 {
                                ui.ctx()
                                    .request_discard("verify gesture is applied only once");
                            }
                        },
                    );
                    assert!(passes >= 2);
                }
                let accepted = blocked == 0;
                assert_eq!(
                    app.image_view.pan,
                    if accepted && button == egui::PointerButton::Secondary {
                        (210.0, 220.0)
                    } else {
                        (10.0, 20.0)
                    },
                    "{button:?}, blocked={blocked}"
                );
                assert_eq!(
                    app.image_view.selection,
                    (accepted && button == egui::PointerButton::Primary).then_some(UnitRect {
                        min: UnitPoint { x: 0.2, y: 0.2 },
                        max: UnitPoint { x: 0.6, y: 0.6 },
                    }),
                    "{button:?}, blocked={blocked}"
                );
                assert!(app.view_drag.is_none());
            }
        }
    }

    #[test]
    fn pan_commits_the_release_position_and_cancel_requires_a_new_press() {
        let Some(_root) = isolated_test_root(
            "tests::pan_commits_the_release_position_and_cancel_requires_a_new_press",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 500.0));
        let button = |pressed, pos| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        for interruption in 0..5 {
            let context = egui::Context::default();
            app.image_view.pan = (10.0, 20.0);
            let frame = |app: &mut Application<_>, events, focused| {
                let _ = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        events,
                        focused,
                        ..Default::default()
                    },
                    |ui| {
                        let response =
                            ui.interact(screen, "cancel-pan".into(), egui::Sense::click_and_drag());
                        let pointer = ui.input(|input| input.pointer.hover_pos());
                        app.update_pan(&response, pointer);
                    },
                );
            };
            let start = egui::pos2(100.0, 100.0);
            frame(&mut app, vec![egui::Event::PointerMoved(start)], true);
            frame(&mut app, vec![button(true, start)], true);
            frame(
                &mut app,
                vec![egui::Event::PointerMoved(egui::pos2(130.0, 140.0))],
                true,
            );
            assert_eq!(app.image_view.pan, (40.0, 60.0));
            match interruption {
                0 | 1 => {}
                2 => app.pending_guard = Some(GuardedAction::Exit),
                3 => {
                    app.fullscreen = true;
                    assert!(app.dismiss_overlay_or_fullscreen());
                    assert!(app.fullscreen);
                }
                _ => app.dispatch(CommandId::ToggleFullscreen),
            }
            frame(&mut app, vec![], interruption != 1);
            app.pending_guard = None;
            app.fullscreen = false;
            let end = egui::pos2(180.0, 190.0);
            frame(
                &mut app,
                vec![egui::Event::PointerMoved(end), button(false, end)],
                true,
            );
            assert_eq!(
                app.image_view.pan,
                if interruption == 0 {
                    (90.0, 110.0)
                } else {
                    (10.0, 20.0)
                }
            );
            assert!(app.view_drag.is_none());
            let before = app.image_view.pan;
            frame(&mut app, vec![button(true, end)], true);
            let next = end + egui::vec2(50.0, 30.0);
            frame(
                &mut app,
                vec![egui::Event::PointerMoved(next), button(false, next)],
                true,
            );
            assert_eq!(app.image_view.pan, (before.0 + 50.0, before.1 + 30.0));
            assert!(app.view_drag.is_none());
        }
    }

    #[test]
    fn selection_drag_cancels_without_resuming_after_focus_or_overlay_interruptions() {
        let Some(_root) = isolated_test_root(
            "tests::selection_drag_cancels_without_resuming_after_focus_or_overlay_interruptions",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        app.media_kind = Some(MediaKind::Image);
        let original = Some(UnitRect {
            min: UnitPoint { x: 0.2, y: 0.2 },
            max: UnitPoint { x: 0.8, y: 0.8 },
        });
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 500.0));
        let button = |pressed, x| egui::Event::PointerButton {
            pos: egui::pos2(x, 250.0),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        for interruption in 0..9 {
            let context = egui::Context::default();
            app.image_view.selection = original;
            let frame = |app: &mut Application<_>, events, focused| {
                let _ = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        events,
                        focused,
                        ..Default::default()
                    },
                    |ui| {
                        let response = ui.interact(
                            screen,
                            "cancel-selection".into(),
                            egui::Sense::click_and_drag(),
                        );
                        let pointer = ui.input(|input| input.pointer.hover_pos());
                        app.update_selection(&response, screen, (500, 500), false, pointer);
                    },
                );
            };
            frame(
                &mut app,
                vec![egui::Event::PointerMoved(egui::pos2(100.0, 250.0))],
                true,
            );
            frame(&mut app, vec![button(true, 100.0)], true);
            frame(
                &mut app,
                vec![egui::Event::PointerMoved(egui::pos2(150.0, 250.0))],
                true,
            );
            assert_ne!(app.image_view.selection, original);
            match interruption {
                0 => {}
                1 => app.pending_guard = Some(GuardedAction::Exit),
                2 => app.palette_open = true,
                3 => app.grid_open = true,
                4 => app.filmstrip_open = true,
                5 => app.pending_dialog = Some(DialogIntent::OpenFile),
                6 => {
                    app.fullscreen = true;
                    assert!(app.dismiss_overlay_or_fullscreen());
                    assert!(app.fullscreen);
                }
                7 => app.dispatch(CommandId::ToggleFullscreen),
                _ => egui::Popup::open_id(&context, "interrupt-drag".into()),
            }
            frame(&mut app, vec![], interruption != 0);
            assert_eq!(
                app.image_view.selection, original,
                "interruption {interruption}"
            );
            app.pending_guard = None;
            app.pending_dialog = None;
            app.palette_open = false;
            app.grid_open = false;
            app.filmstrip_open = false;
            app.fullscreen = false;
            egui::Popup::close_id(&context, "interrupt-drag".into());
            frame(
                &mut app,
                vec![egui::Event::PointerMoved(egui::pos2(300.0, 250.0))],
                true,
            );
            frame(&mut app, vec![button(false, 300.0)], true);
            assert_eq!(
                app.image_view.selection, original,
                "release after interruption {interruption}"
            );
            assert!(!app.image_view.crop_preview);
        }
    }

    #[test]
    fn selection_drag_uses_press_and_release_positions_with_sparse_events() {
        let Some(_root) = isolated_test_root(
            "tests::selection_drag_uses_press_and_release_positions_with_sparse_events",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 500.0));
        let image = egui::Rect::from_min_max(egui::pos2(50.0, 50.0), egui::pos2(450.0, 450.0));
        let rectangle = |x, y, width, height| PixelCrop {
            x,
            y,
            width,
            height,
        };
        for kind in [MediaKind::Image, MediaKind::Video] {
            for delivery in 0..4 {
                for (start, end, initial, expected) in [
                    (
                        egui::pos2(100.0, 100.0),
                        egui::pos2(300.0, 300.0),
                        None,
                        Some(rectangle(50, 50, 200, 200)),
                    ),
                    (
                        egui::pos2(300.0, 300.0),
                        egui::pos2(100.0, 100.0),
                        None,
                        Some(rectangle(50, 50, 200, 200)),
                    ),
                    (
                        egui::pos2(150.0, 250.0),
                        egui::pos2(190.0, 250.0),
                        Some(rectangle(100, 100, 200, 200)),
                        Some(rectangle(140, 100, 160, 200)),
                    ),
                    (egui::pos2(20.0, 20.0), egui::pos2(300.0, 300.0), None, None),
                    (
                        egui::pos2(48.0, 250.0),
                        egui::pos2(100.0, 250.0),
                        None,
                        None,
                    ),
                    (
                        egui::pos2(48.0, 250.0),
                        egui::pos2(100.0, 250.0),
                        Some(rectangle(0, 0, 400, 400)),
                        Some(rectangle(50, 0, 350, 400)),
                    ),
                    (
                        egui::pos2(452.0, 250.0),
                        egui::pos2(400.0, 250.0),
                        Some(rectangle(0, 0, 400, 400)),
                        Some(rectangle(0, 0, 350, 400)),
                    ),
                    (
                        egui::pos2(250.0, 48.0),
                        egui::pos2(250.0, 100.0),
                        Some(rectangle(0, 0, 400, 400)),
                        Some(rectangle(0, 50, 400, 350)),
                    ),
                    (
                        egui::pos2(250.0, 452.0),
                        egui::pos2(250.0, 400.0),
                        Some(rectangle(0, 0, 400, 400)),
                        Some(rectangle(0, 0, 400, 350)),
                    ),
                    (
                        egui::pos2(250.0, 250.0),
                        egui::pos2(250.0, 250.0),
                        Some(rectangle(100, 100, 200, 200)),
                        Some(rectangle(100, 100, 200, 200)),
                    ),
                ] {
                    let context = egui::Context::default();
                    app.media_kind = Some(kind);
                    app.view_drag = None;
                    app.image_view.selection = initial.map(|crop| crop.unit_rect((400, 400)));
                    app.image_view.crop_preview = false;
                    let button = |pressed, pos| egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    };
                    let mut frames = vec![vec![egui::Event::PointerMoved(start)]];
                    if delivery < 2 {
                        frames.push(vec![button(true, start)]);
                    }
                    if delivery == 0 {
                        frames.push(vec![egui::Event::PointerMoved(start.lerp(end, 0.5))]);
                    }
                    let mut completion = Vec::new();
                    if delivery >= 2 {
                        completion.push(button(true, start));
                    }
                    completion.extend([egui::Event::PointerMoved(end), button(false, end)]);
                    if delivery == 3 {
                        completion.push(egui::Event::PointerMoved(egui::pos2(440.0, 440.0)));
                    }
                    frames.push(completion);
                    for events in frames {
                        let _ = context.run_ui(
                            egui::RawInput {
                                screen_rect: Some(screen),
                                events,
                                ..Default::default()
                            },
                            |ui| {
                                let response = ui.interact(
                                    screen,
                                    "selection-sparse".into(),
                                    egui::Sense::click_and_drag(),
                                );
                                let pointer = ui.input(|input| input.pointer.hover_pos());
                                app.update_selection(&response, image, (400, 400), false, pointer);
                            },
                        );
                    }
                    let actual = app.image_view.selection.and_then(|selection| {
                        PixelCrop::from_selection(selection, (400, 400), kind)
                    });
                    assert_eq!(
                        actual, expected,
                        "{kind:?}, delivery={delivery}, {start:?} -> {end:?}"
                    );
                    assert!(app.view_drag.is_none());
                    assert_eq!(
                        app.image_view.crop_preview,
                        kind == MediaKind::Image && start == end
                    );
                }
            }
        }
    }

    #[test]
    fn zoomed_single_pixel_selection_survives_release_on_a_large_image() {
        let Some(_root) = isolated_test_root(
            "tests::zoomed_single_pixel_selection_survives_release_on_a_large_image",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless application");
        app.media_kind = Some(MediaKind::Image);
        let context = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 500.0));
        let button = |pressed, at| egui::Event::PointerButton {
            pos: egui::pos2(at, at),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        };
        for events in [
            vec![egui::Event::PointerMoved(egui::pos2(100.0, 100.0))],
            vec![button(true, 100.0)],
            vec![egui::Event::PointerMoved(egui::pos2(110.0, 110.0))],
            vec![egui::Event::PointerMoved(egui::pos2(174.0, 174.0))],
            vec![button(false, 174.0)],
        ] {
            let _ = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let response = ui.interact(
                        screen,
                        "selection-test".into(),
                        egui::Sense::click_and_drag(),
                    );
                    let pointer = ui.input(|input| input.pointer.hover_pos());
                    app.update_selection(
                        &response,
                        egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(16_384.0 * 64.0, 16_384.0 * 64.0),
                        ),
                        (16_384, 16_384),
                        false,
                        pointer,
                    );
                },
            );
        }
        let selection = app
            .image_view
            .selection
            .expect("one-pixel selection retained");
        let crop = PixelCrop::from_selection(selection, (16_384, 16_384), MediaKind::Image)
            .expect("pixel crop");
        assert_eq!((crop.width, crop.height), (1, 1));
    }

    #[test]
    fn unchanged_active_tab_keeps_playback_view_and_pending_load_state() {
        let Some(root) = isolated_test_root(
            "tests::unchanged_active_tab_keeps_playback_view_and_pending_load_state",
        ) else {
            return;
        };
        for kind in [MediaKind::Image, MediaKind::Video, MediaKind::Audio] {
            let mut app = Application::new(None, |_| {}).expect("headless app");
            let background = app
                .tabs
                .open_new(root.join("background.png"), MediaKind::Image);
            app.edits.insert(background, EditHistory::default());
            app.export_paths
                .insert(background, root.join("background-export.png"));
            let path = root.join(match kind {
                MediaKind::Image => "active.png",
                MediaKind::Video => "active.mp4",
                MediaKind::Audio => "active.wav",
            });
            let active = app.tabs.open_new(path.clone(), kind);
            app.path = Some(path.clone());
            app.media_kind = Some(kind);
            app.state = PlaybackState::Paused;
            let mut clock = PlaybackClock::new(media_time(Duration::from_secs(5)), 1.0);
            clock.set_paused(true);
            app.clock = Some(clock);
            app.media_duration = Some(Duration::from_secs(30));
            app.timeline_open = true;
            app.image_generation = 17;
            app.media_generation = 23;
            app.image_view.zoom = towavue_core::ZoomMode::Custom(2.5);
            app.image_view.pan = (20.0, -10.0);
            app.image_view.selection = Some(UnitRect::FULL);
            app.edits.entry(active).or_default().push(
                if kind == MediaKind::Audio {
                    EditOperation::SetVolume(0.4)
                } else {
                    EditOperation::RotateClockwise
                },
                kind,
            );
            let history = app.edits[&active].clone();
            let view = app.image_view;
            let position = app.current_position();
            for operation in 0..3 {
                match operation {
                    0 => app.close_tab_unchecked(background),
                    1 => app.activate_tab(active),
                    _ => app.cycle_tab(true),
                }
                assert_eq!(app.tabs.active().map(|tab| tab.id), Some(active));
                assert_eq!(app.path.as_ref(), Some(&path));
                assert_eq!(
                    app.state,
                    PlaybackState::Paused,
                    "{kind:?}, operation {operation}"
                );
                assert_eq!(app.current_position(), position);
                assert_eq!(app.media_duration, Some(Duration::from_secs(30)));
                assert!(app.timeline_open);
                assert_eq!((app.image_generation, app.media_generation), (17, 23));
                assert_eq!(app.image_view, view);
                assert_eq!(app.edits[&active], history);
                assert!(!app.edits.contains_key(&background));
                assert!(!app.export_paths.contains_key(&background));
            }
        }
    }

    #[test]
    fn last_tab_close_clears_media_state_before_welcome() {
        let Some(root) =
            isolated_test_root("tests::last_tab_close_clears_media_state_before_welcome")
        else {
            return;
        };
        for kind in [MediaKind::Image, MediaKind::Video, MediaKind::Audio] {
            let mut app = Application::new(None, |_| {}).expect("headless app");
            let path = root.join("closed-media");
            let tab = app.tabs.open_new(path.clone(), kind);
            app.path = Some(path.clone());
            app.media_kind = Some(kind);
            app.timeline_open = true;
            app.media_duration = Some(Duration::from_secs(30));
            app.clock = Some(PlaybackClock::new(media_time(Duration::from_secs(5)), 1.0));
            app.pending_time = Some(media_time(Duration::from_secs(6)));
            app.pending_seek_started = Some(Instant::now());
            app.decode_finished = true;
            app.audio_drained = false;
            app.seek_latencies.push(Duration::from_millis(20));
            app.drift_samples.push(Duration::from_millis(2));
            app.image_view.zoom = towavue_core::ZoomMode::Custom(2.5);
            let context = egui::Context::default();
            let texture = context.load_texture(
                "closed preview",
                egui::ColorImage::new([1, 1], vec![Color32::WHITE]),
                TextureOptions::LINEAR,
            );
            app.waveform = Some(texture.clone());
            app.hover_thumbnail = Some((2, texture));
            app.waveform_loading = true;
            app.thumbnail_loading = Some(3);
            app.failed_thumbnails.insert(1);
            app.set_status("Position 5.000s".into());
            app.close_tab_unchecked(tab);
            assert!(app.path.is_none());
            assert!(app.media_kind.is_none());
            assert!(!app.timeline_open);
            assert!(app.waveform.is_none());
            assert!(app.hover_thumbnail.is_none());
            assert!(app.media_duration.is_none());
            assert!(!app.waveform_loading);
            assert!(app.thumbnail_loading.is_none());
            assert!(app.failed_thumbnails.is_empty());
            assert!(app.clock.is_none());
            assert!(app.pending_time.is_none());
            assert!(app.pending_seek_started.is_none());
            assert!(!app.decode_finished);
            assert!(app.audio_drained);
            assert!(app.seek_latencies.is_empty());
            assert!(app.drift_samples.is_empty());
            assert_eq!(app.image_view, ImageViewState::default());
            assert_eq!(app.current_position(), MediaTime::ZERO);
            assert_eq!(app.state, PlaybackState::Paused);
            assert!(app.status_message.is_none());
            app.handle_app_event(AppEvent::Duration(
                path.clone(),
                app.media_generation,
                Ok(Duration::from_secs(30)),
            ));
            app.handle_app_event(AppEvent::Waveform(
                path.clone(),
                app.media_generation,
                Err("late waveform".into()),
            ));
            app.handle_app_event(AppEvent::Thumbnail(
                path,
                app.media_generation,
                3,
                Err("late thumbnail".into()),
            ));
            assert!(app.media_duration.is_none());
            assert!(app.status_message.is_none());
            app.timeline_open = true;
            let _ = context.run_ui(egui::RawInput::default(), |ui| {
                let available = ui.available_rect_before_wrap();
                app.draw_timeline(ui, &mut Vec::new());
                assert_eq!(ui.available_rect_before_wrap(), available);
            });
        }
    }

    #[test]
    fn duration_requests_do_not_start_more_workers_while_one_is_busy() {
        let Some(_root) = isolated_test_root(
            "tests::duration_requests_do_not_start_more_workers_while_one_is_busy",
        ) else {
            return;
        };
        let gate = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let worker_gate = Arc::clone(&gate);
        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = Application::new(None, move |event| {
            if let AppEvent::Duration(_, _, result) = event {
                let _ = tx.send(result.is_ok());
                let (mutex, ready) = &*worker_gate;
                drop(
                    ready
                        .wait_while(mutex.lock().expect("gate"), |released| !*released)
                        .expect("released"),
                );
            }
        })
        .expect("headless app");
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/m1/h264-aac.mp4");
        app.load_duration(path.clone());
        assert!(
            rx.recv_timeout(Duration::from_secs(5))
                .expect("first duration")
        );
        for _ in 0..8 {
            app.load_duration(path.clone());
        }
        let additional = rx.recv_timeout(Duration::from_secs(2));
        *gate.0.lock().expect("release gate") = true;
        gate.1.notify_all();
        assert!(
            additional.is_err(),
            "another duration worker ran while the first was busy"
        );
        assert!(
            rx.recv_timeout(Duration::from_secs(5))
                .expect("latest duration")
        );
    }

    #[test]
    fn reopened_path_rejects_old_duration_and_waveform_results() {
        let Some(root) =
            isolated_test_root("tests::reopened_path_rejects_old_duration_and_waveform_results")
        else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless app");
        let path = root.join("reopened.mp4");
        app.load_path(path.clone(), MediaKind::Video);
        let old = app.media_generation;
        app.load_path(path.clone(), MediaKind::Video);
        assert_ne!(app.media_generation, old);
        app.status_message = None;
        app.waveform_loading = true;
        app.ui_context = Some(egui::Context::default());
        let preview = towavue_runtime_windows::PreviewImage {
            width: 1,
            height: 1,
            rgba: vec![255; 4],
        };
        for result in [Ok(Duration::from_secs(99)), Err("old duration".into())] {
            app.handle_app_event(AppEvent::Duration(path.clone(), old, result));
            assert!(app.media_duration.is_none());
            assert!(app.status_message.is_none());
        }
        for result in [Ok(preview.clone()), Err("old waveform".into())] {
            app.handle_app_event(AppEvent::Waveform(path.clone(), old, result));
            assert!(app.waveform.is_none());
            assert!(app.waveform_loading);
            assert!(app.status_message.is_none());
        }
        app.handle_app_event(AppEvent::Duration(
            path.clone(),
            app.media_generation,
            Ok(Duration::from_secs(2)),
        ));
        app.handle_app_event(AppEvent::Waveform(path, app.media_generation, Ok(preview)));
        assert_eq!(app.media_duration, Some(Duration::from_secs(2)));
        assert!(app.waveform.is_some());
        assert!(!app.waveform_loading);
    }

    #[test]
    fn reopened_playback_rejects_events_from_the_previous_session() {
        let Some(root) =
            isolated_test_root("tests::reopened_playback_rejects_events_from_the_previous_session")
        else {
            return;
        };
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/m1/h264-aac.mp4");
        let path = root.join("video-only.mp4");
        let ffmpeg = PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg"))
            .join("bin/ffmpeg.exe");
        assert!(
            std::process::Command::new(ffmpeg)
                .args(["-v", "error", "-i"])
                .arg(source)
                .args(["-an", "-c:v", "copy"])
                .arg(&path)
                .status()
                .expect("video-only fixture")
                .success()
        );
        struct Trial(PathBuf);
        impl ApplicationHandler for Trial {
            fn resumed(&mut self, event_loop: &ActiveEventLoop) {
                let window = Arc::new(
                    event_loop
                        .create_window(Window::default_attributes().with_visible(false))
                        .expect("hidden playback test window"),
                );
                let renderer = match FrameRenderer::new(&window) {
                    Ok(renderer) => renderer,
                    Err(error) => {
                        eprintln!("SKIP reopened playback: D3D11 renderer unavailable: {error}");
                        event_loop.exit();
                        return;
                    }
                };
                let (tx, rx) = std::sync::mpsc::channel();
                let mut app = Application::new(None, move |event| {
                    let _ = tx.send(event);
                })
                .expect("test app");
                app.window = Some(window);
                app.renderer = Some(renderer);
                app.load_path(self.0.clone(), MediaKind::Video);
                assert_eq!(app.state, PlaybackState::Playing);
                let deadline = Instant::now() + Duration::from_secs(5);
                let (old_media, old_playback) = loop {
                    let event = rx
                        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                        .expect("first session playback callback");
                    if let AppEvent::Playback(media, event) = event {
                        break (media, event.generation());
                    }
                };
                app.load_path(self.0.clone(), MediaKind::Video);
                assert_eq!(app.state, PlaybackState::Playing);
                assert_ne!(app.media_generation, old_media);
                assert_eq!(
                    app.generation, old_playback,
                    "session-local generations overlap"
                );
                for event in [
                    PlaybackEvent::DecodeFinished(old_playback),
                    PlaybackEvent::Failed(old_playback, "old failure".into()),
                    PlaybackEvent::DeviceRemoved(old_playback, "old device".into()),
                    PlaybackEvent::VideoReady(old_playback),
                    PlaybackEvent::AudioReady(old_playback),
                ] {
                    app.handle_app_event(AppEvent::Playback(old_media, event));
                    assert_eq!(app.state, PlaybackState::Playing);
                    assert!(!app.decode_finished);
                    assert!(app.pending_time.is_none());
                    assert!(app.playback_error.is_none());
                }
                app.handle_app_event(AppEvent::Playback(
                    app.media_generation,
                    PlaybackEvent::DecodeFinished(app.generation.next()),
                ));
                assert!(!app.decode_finished, "seek generation is still checked");
                app.handle_app_event(AppEvent::Playback(
                    app.media_generation,
                    PlaybackEvent::DecodeFinished(app.generation),
                ));
                assert!(
                    app.decode_finished,
                    "current session notifications are accepted"
                );
                eprintln!("reopened playback: live session identity checks passed");
                event_loop.exit();
            }

            fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
        }
        use winit::platform::windows::EventLoopBuilderExtWindows;
        let mut builder = EventLoop::builder();
        builder.with_any_thread(true);
        builder
            .build()
            .expect("test event loop")
            .run_app(&mut Trial(path))
            .expect("playback trial");
    }

    #[test]
    fn playback_notifications_without_a_session_leave_welcome_unchanged() {
        let Some(_root) = isolated_test_root(
            "tests::playback_notifications_without_a_session_leave_welcome_unchanged",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless app");
        let state = app.state;
        for event in [
            PlaybackEvent::DecodeFinished(app.generation),
            PlaybackEvent::Failed(app.generation, "closed decoder failed".into()),
            PlaybackEvent::DeviceRemoved(app.generation, "closed device".into()),
        ] {
            app.handle_playback_event(event);
            assert!(!app.decode_finished);
            assert_eq!(app.state, state);
            assert!(app.playback_error.is_none());
            assert!(app.status_message.is_none());
        }
    }

    #[test]
    fn playback_failure_remains_visible_after_status_expires_and_clears_on_close() {
        let Some(root) = isolated_test_root(
            "tests::playback_failure_remains_visible_after_status_expires_and_clears_on_close",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("headless app");
        let path = root.join("unsupported.mp4");
        let tab = app.tabs.open_new(path.clone(), MediaKind::Video);
        app.path = Some(path);
        app.media_kind = Some(MediaKind::Video);
        app.fail("Unsupported orientation fixture".into());
        app.status_message = None;
        let context = egui::Context::default();
        for fullscreen in [false, true] {
            app.fullscreen = fullscreen;
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(480.0, 300.0),
                    )),
                    ..Default::default()
                },
                |ui| app.draw_ui(ui, &mut Vec::new()),
            );
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text().contains("Unsupported orientation fixture"))));
            assert_eq!(app.state, PlaybackState::Faulted);
        }
        app.close_tab_unchecked(tab);
        assert!(app.playback_error.is_none());
        assert!(app.path.is_none());
    }

    #[test]
    fn video_metadata_orientation_precedes_edits_and_matches_export() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("towavue-orientation-{unique}"));
        std::fs::create_dir(&root).expect("orientation fixtures");
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/m1/h264-aac.mp4");
        let ffmpeg = PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg"))
            .join("bin/ffmpeg.exe");
        let first_frame = |path: &Path| {
            let mut frame = None;
            towavue_runtime_windows::decode_file(path, |output| {
                if frame.is_none()
                    && let towavue_runtime_windows::DecodeOutput::Video(video) = output
                {
                    frame = Some(video);
                }
                true
            })
            .expect("decode orientation fixture");
            frame.expect("video frame")
        };
        for rotation in [0, 90, 180, 270] {
            for mirror in [false, true] {
                let input = root.join(format!("{rotation}-{mirror}.mp4"));
                let mut command = std::process::Command::new(&ffmpeg);
                command.args(["-v", "error", "-display_rotation", &rotation.to_string()]);
                if mirror {
                    command.arg("-display_hflip");
                }
                let remux = command
                    .arg("-i")
                    .arg(&source)
                    .args(["-map", "0", "-c", "copy"])
                    .arg(&input)
                    .output()
                    .expect("metadata remux");
                assert!(
                    remux.status.success(),
                    "{}",
                    String::from_utf8_lossy(&remux.stderr)
                );
                let original = first_frame(&input);
                assert_eq!(original.orientation.swaps_axes(), rotation % 180 != 0);
                for edited in [false, true] {
                    let operations = if edited {
                        vec![
                            EditOperation::Crop(PixelCrop {
                                x: 8,
                                y: 12,
                                width: 48,
                                height: 64,
                            }),
                            EditOperation::RotateClockwise,
                            EditOperation::FlipHorizontal,
                        ]
                    } else {
                        vec![]
                    };
                    let transform = ImageTransform::with_orientation(
                        (original.width, original.height),
                        original.orientation,
                        &operations,
                    );
                    assert_eq!(
                        transform.pixel_aspect(1.5),
                        if transform.uv[0].x == transform.uv[1].x {
                            1.0 / 1.5
                        } else {
                            1.5
                        }
                    );
                    let target = root.join(format!("out-{rotation}-{mirror}-{edited}.mp4"));
                    towavue_runtime_windows::export_media(&ExportRequest {
                        source: input.clone(),
                        target: target.clone(),
                        kind: MediaKind::Video,
                        operations,
                        hardware_encode: false,
                    })
                    .expect("orientation export");
                    let exported = first_frame(&target);
                    assert_eq!(
                        (exported.width as f32, exported.height as f32),
                        transform.size
                    );
                    assert_eq!(
                        exported.orientation,
                        towavue_runtime_windows::VideoOrientation::default(),
                        "export must not rotate again on reopen"
                    );
                    let (mut error, mut count) = (0_u64, 0_u64);
                    for y in 4..exported.height - 4 {
                        for x in 4..exported.width - 4 {
                            let uv = bilinear_uv(
                                transform.uv,
                                (x as f32 + 0.5) / exported.width as f32,
                                (y as f32 + 0.5) / exported.height as f32,
                            );
                            let original_index = (((uv.y * original.height as f32) as u32
                                * original.width
                                + (uv.x * original.width as f32) as u32)
                                * 4) as usize;
                            let output_index = ((y * exported.width + x) * 4) as usize;
                            for channel in 0..3 {
                                error += u64::from(
                                    original.rgba[original_index + channel]
                                        .abs_diff(exported.rgba[output_index + channel]),
                                );
                                count += 1;
                            }
                        }
                    }
                    assert!(
                        (error as f64 / count as f64) < 12.0,
                        "orientation {rotation}/{mirror}, edits {edited}: RGB mean error {}",
                        error as f64 / count as f64
                    );
                }
            }
        }
        std::fs::remove_dir_all(root).expect("remove owned orientation fixtures");
    }

    #[test]
    fn video_visual_edits_match_export_and_follow_undo_redo_and_active_tab() {
        let Some(root) = isolated_test_root(
            "tests::video_visual_edits_match_export_and_follow_undo_redo_and_active_tab",
        ) else {
            return;
        };
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/m1/h264-aac.mp4");
        let first_frame = |path: &Path| {
            let mut frame = None;
            towavue_runtime_windows::decode_file(path, |output| {
                if frame.is_none()
                    && let towavue_runtime_windows::DecodeOutput::Video(video) = output
                {
                    frame = Some(video);
                }
                true
            })
            .expect("decode comparison frame");
            frame.expect("fixture video frame")
        };
        let original = first_frame(&source);
        let size = (original.width, original.height);
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let tab = app.tabs.open_new(source.clone(), MediaKind::Video);
        app.media_kind = Some(MediaKind::Video);
        app.state = PlaybackState::Paused;
        let generation = app.generation;
        app.dispatch(CommandId::RotateClockwise);
        app.dispatch(CommandId::FlipHorizontal);
        app.image_view.selection = Some(UnitRect {
            min: UnitPoint { x: 0.25, y: 0.25 },
            max: UnitPoint { x: 0.75, y: 0.75 },
        });
        app.crop_selection(size);
        let transform = app.visual_transform(size);
        assert_eq!(transform.size, (48.0, 80.0));
        assert_eq!(transform.pixel_aspect(2.0), 0.5);
        assert!(app.image_view.selection.is_none());
        assert_eq!(app.state, PlaybackState::Paused);
        assert_eq!(app.generation, generation);
        app.dispatch(CommandId::Undo);
        assert_eq!(app.visual_transform(size).size, (96.0, 160.0));
        app.dispatch(CommandId::Redo);
        assert_eq!(app.visual_transform(size).uv, transform.uv);
        app.tabs
            .open_new(root.join("another.mp4"), MediaKind::Video);
        assert_eq!(
            app.visual_transform(size).uv,
            ImageTransform::new(size, &[]).uv
        );
        app.tabs.activate(tab);
        assert_eq!(app.visual_transform(size).uv, transform.uv);

        let target = root.join("edited.mp4");
        towavue_runtime_windows::export_media(&ExportRequest {
            source,
            target: target.clone(),
            kind: MediaKind::Video,
            operations: app
                .edits
                .get(&tab)
                .expect("edit history")
                .operations()
                .to_vec(),
            hardware_encode: false,
        })
        .expect("export visual edits");
        let exported = first_frame(&target);
        assert_eq!((exported.width, exported.height), (48, 80));
        let mut difference = 0_u64;
        let mut samples = 0;
        for y in 4..exported.height - 4 {
            for x in 4..exported.width - 4 {
                let uv = bilinear_uv(
                    transform.uv,
                    (x as f32 + 0.5) / exported.width as f32,
                    (y as f32 + 0.5) / exported.height as f32,
                );
                let source_index = (((uv.y * original.height as f32) as u32 * original.width
                    + (uv.x * original.width as f32) as u32)
                    * 4) as usize;
                let output_index = ((y * exported.width + x) * 4) as usize;
                for channel in 0..3 {
                    difference += u64::from(
                        original.rgba[source_index + channel]
                            .abs_diff(exported.rgba[output_index + channel]),
                    );
                    samples += 1;
                }
            }
        }
        let mean_error = difference as f64 / f64::from(samples);
        assert!(
            mean_error < 12.0,
            "preview/export RGB mean error: {mean_error}"
        );
    }

    #[test]
    fn visual_uv_composition_preserves_flips_rotations_and_pixel_aspect() {
        let full = ImageTransform::new((640, 360), &[]);
        for operation in [
            EditOperation::RotateClockwise,
            EditOperation::RotateCounterclockwise,
        ] {
            let transform = ImageTransform::new((640, 360), &[operation; 4]);
            assert_eq!(transform.uv, full.uv);
            assert_eq!(transform.size, full.size);
            assert_eq!(transform.pixel_aspect(2.0), 2.0);
        }
        for operation in [EditOperation::FlipHorizontal, EditOperation::FlipVertical] {
            let transform = ImageTransform::new((640, 360), &[operation; 2]);
            assert_eq!(transform.uv, full.uv);
            assert_eq!(transform.size, full.size);
        }
        let transform = ImageTransform::new((640, 360), &[EditOperation::RotateCounterclockwise]);
        assert_eq!(transform.uv[0], UnitPoint { x: 1.0, y: 0.0 });
        let rect = fitted_video_rect(
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 540.0)),
            (360, 640),
            transform.pixel_aspect(1.5),
        );
        assert!((rect.width() / rect.height() - 0.375).abs() < 0.001);
    }
}
