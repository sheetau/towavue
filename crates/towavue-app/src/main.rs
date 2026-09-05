//! Application entry point for towavue.

#![forbid(unsafe_code)]

mod chrome;
mod grid;
mod palette;
mod seekbar;
mod shortcuts;

use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Align2, Color32, RichText, TextureHandle, TextureOptions};
use towavue_core::{
    CommandContext, CommandId, EditHistory, EditOperation, FolderSnapshot, FolderSnapshotSource,
    ImageViewState, Key, KeyStroke, MediaKind, MediaTime, Modifiers, PlaybackGeneration,
    PlaybackState, ReadingAxis, ReadingSettings, ShortcutBindings, ShortcutMatch, TabId, TabSet,
    TabTarget, UnitPoint, UnitRect, ZoomMode, command_definitions,
};
use towavue_runtime_windows::{
    AudioOutputEvent, DecodedImage, DialogError, ExportError, ExportEvent, ExportJob,
    ExportRequest, FileDialogKind, FolderOrderProvider, FolderWatcher, FrameRenderer, ImageLoader,
    PlaybackEvent, PlaybackSession, PreviewCache, RenderError, canonical_shell_path,
    cursor_position_in_window, pick_path,
};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

const AUDIO_EVENT_POLL_INTERVAL: Duration = Duration::from_millis(20);
const FOLDER_EVENT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const PREFIX_TIMEOUT: Duration = Duration::from_secs(1);
const STATUS_MESSAGE_DURATION: Duration = Duration::from_secs(4);
const VIDEO_EARLY_TOLERANCE: Duration = Duration::from_millis(5);
const KEYBOARD_SEEK_STEP: Duration = Duration::from_secs(5);
const VIDEO_LATE_TOLERANCE: Duration = Duration::from_millis(40);

fn main() -> Result<(), Box<dyn Error>> {
    let initial_path = parse_initial_path()?;
    let event_loop = EventLoop::<AppEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    let mut application = Application::new(initial_path, move |event| {
        let _ = proxy.send_event(event);
    })?;
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
    CloseTab(TabId),
    DetachTab(TabId),
    OpenMedia(PathBuf, bool),
    Seek(MediaTime),
    ResolveGuard(GuardDecision),
    CancelExport,
    DismissExportError,
}

enum AppEvent {
    ImagesReady,
    FolderReady,
    DialogFinished(Result<Option<PathBuf>, DialogError>),
    Export(ExportEvent),
    Playback(PlaybackEvent),
    Duration(PathBuf, Result<Duration, String>),
    Waveform(
        PathBuf,
        Result<towavue_runtime_windows::PreviewImage, String>,
    ),
    Thumbnail(
        PathBuf,
        u64,
        Result<towavue_runtime_windows::PreviewImage, String>,
    ),
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

struct ActiveExport {
    job: ExportJob,
    tab: TabId,
    request: ExportRequest,
    encoded: Duration,
    cancelling: bool,
    continuation: Option<GuardedAction>,
}

struct ImagePresentation {
    decoded: DecodedImage,
    texture: TextureHandle,
    frame_index: usize,
    next_frame_at: Option<Instant>,
}

impl ImagePresentation {
    fn from_decoded(
        context: &egui::Context,
        path: &Path,
        decoded: DecodedImage,
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
        while deadline <= now {
            self.frame_index = (self.frame_index + 1) % self.decoded.frames.len();
            deadline += self.decoded.frames[self.frame_index].delay;
        }
        self.texture.set(
            color_image(&self.decoded.frames[self.frame_index]),
            TextureOptions::LINEAR,
        );
        self.next_frame_at = Some(deadline);
        true
    }
}

fn color_image(frame: &towavue_runtime_windows::DecodedImageFrame) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied(
        [frame.width as usize, frame.height as usize],
        &frame.rgba,
    )
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
struct ImageTransform {
    uv: [UnitPoint; 4],
    size: (f32, f32),
}

impl ImageTransform {
    fn new(size: (u32, u32), operations: &[EditOperation]) -> Self {
        let mut transform = Self {
            uv: [
                UnitPoint { x: 0.0, y: 0.0 },
                UnitPoint { x: 1.0, y: 0.0 },
                UnitPoint { x: 1.0, y: 1.0 },
                UnitPoint { x: 0.0, y: 1.0 },
            ],
            size: (size.0 as f32, size.1 as f32),
        };
        for operation in operations {
            match *operation {
                EditOperation::Crop(region) => transform.crop(region),
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
        let previous = self.uv;
        self.uv = [
            bilinear_uv(previous, region.min.x, region.min.y),
            bilinear_uv(previous, region.max.x, region.min.y),
            bilinear_uv(previous, region.max.x, region.max.y),
            bilinear_uv(previous, region.min.x, region.max.y),
        ];
        self.size.0 = (self.size.0 * region.width()).max(1.0);
        self.size.1 = (self.size.1 * region.height()).max(1.0);
    }
}

struct Application<N> {
    initial_path: Option<PathBuf>,
    notify: Arc<N>,
    window: Option<Arc<Window>>,
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
    image_loader: ImageLoader,
    image_generation: u64,
    image_loading: bool,
    image_error: Option<String>,
    image_view: ImageViewState,
    selection_drag: Option<SelectionDrag>,
    reading_mode: bool,
    reading_settings: ReadingSettings,
    reading_pages: Vec<Result<ImagePresentation, String>>,
    preview_cache: PreviewCache,
    timeline_open: bool,
    waveform: Option<TextureHandle>,
    media_duration: Option<Duration>,
    hover_thumbnail: Option<(u64, TextureHandle)>,
    waveform_loading: bool,
    thumbnail_loading: Option<u64>,
    session: Option<PlaybackSession>,
    pending_time: Option<MediaTime>,
    video_rect: Option<egui::Rect>,
    ui_repaint_at: Option<Instant>,
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
    grid_open: bool,
    palette_open: bool,
    palette: palette::CommandPalette,
    status_message: Option<(String, Instant)>,
    pending_guard: Option<GuardedAction>,
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
        Ok(Self {
            initial_path,
            notify,
            window: None,
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
            image_loader,
            image_generation: 0,
            image_loading: false,
            image_error: None,
            image_view: ImageViewState::default(),
            selection_drag: None,
            reading_mode: false,
            reading_settings: ReadingSettings::default(),
            reading_pages: Vec::new(),
            preview_cache,
            timeline_open: false,
            waveform: None,
            media_duration: None,
            hover_thumbnail: None,
            waveform_loading: false,
            thumbnail_loading: None,
            session: None,
            pending_time: None,
            video_rect: None,
            ui_repaint_at: None,
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
            grid_open: false,
            palette_open: false,
            palette: palette::CommandPalette::default(),
            status_message: None,
            pending_guard: None,
            exit_requested: false,
            prefer_hardware_encode: false,
        })
    }

    fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Box<dyn Error>> {
        let attributes = Window::default_attributes()
            .with_title(self.title())
            .with_inner_size(LogicalSize::new(960, 576))
            .with_min_inner_size(LogicalSize::new(480, 300))
            .with_decorations(false);
        let window = Arc::new(event_loop.create_window(attributes)?);
        let mut renderer = FrameRenderer::new(&window)?;
        let size = window.inner_size();
        renderer.resize_surface(size.width, size.height)?;
        let context = egui::Context::default();
        context.set_visuals(egui::Visuals::dark());
        context.style_mut_of(egui::Theme::Dark, chrome::style);
        context.input_mut(|input| input.max_texture_side = renderer.max_texture_side());
        let state = egui_winit::State::new(
            context.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            window.theme(),
            Some(renderer.max_texture_side()),
        );
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

    fn open_folder_path(&mut self, folder: PathBuf) {
        let generation = self.folder_order.request(Some(folder));
        self.pending_folder = Some((generation, FolderIntent::Open));
        self.request_redraw();
    }

    fn load_path(&mut self, path: PathBuf, kind: MediaKind) {
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
        self.waveform_loading = false;
        self.thumbnail_loading = None;
        self.image_view = ImageViewState::default();
        self.selection_drag = None;
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
        match PlaybackSession::open(
            &path,
            graphics_device,
            self.edit_state().volume,
            self.edit_state().rate,
            move |event| notify(AppEvent::Playback(event)),
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
        self.waveform_loading = true;
        if let Err(error) = std::thread::Builder::new()
            .name("towavue-waveform".into())
            .spawn(move || {
                let result = cache
                    .waveform(&path, 640, 96)
                    .map_err(|error| error.to_string());
                notify(AppEvent::Waveform(path, result));
            })
        {
            self.waveform_loading = false;
            self.set_status(format!("Could not start waveform worker: {error}"));
        }
    }

    fn load_hover_thumbnail(&mut self, position: Duration, bucket: u64) {
        if self.thumbnail_loading.is_some() {
            return;
        }
        let Some(path) = self.path.clone() else {
            return;
        };
        let cache = self.preview_cache.clone();
        let notify = Arc::clone(&self.notify);
        self.thumbnail_loading = Some(bucket);
        if let Err(error) = std::thread::Builder::new()
            .name("towavue-thumbnail".into())
            .spawn(move || {
                let result = cache
                    .thumbnail(&path, position, 240)
                    .map_err(|error| error.to_string());
                notify(AppEvent::Thumbnail(path, bucket, result));
            })
        {
            self.thumbnail_loading = None;
            self.set_status(format!("Could not start thumbnail worker: {error}"));
        }
    }

    fn load_duration(&mut self, path: PathBuf) {
        let cache = self.preview_cache.clone();
        let notify = Arc::clone(&self.notify);
        if let Err(error) = std::thread::Builder::new()
            .name("towavue-duration".into())
            .spawn(move || {
                let result = cache.duration(&path).map_err(|error| error.to_string());
                notify(AppEvent::Duration(path, result));
            })
        {
            self.set_status(format!("Could not start duration worker: {error}"));
        }
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
            .and_then(|decoded| ImagePresentation::from_decoded(&context, &path, decoded))
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
                    .and_then(|decoded| ImagePresentation::from_decoded(&context, &path, decoded))
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
            AppEvent::ImagesReady => self.finish_image_load(),
            AppEvent::FolderReady => self.finish_folder_load(),
            AppEvent::DialogFinished(result) => self.finish_dialog(result),
            AppEvent::Export(event) => self.handle_export_event(event),
            AppEvent::Playback(event) => self.handle_playback_event(event),
            AppEvent::Duration(path, result) if self.path.as_ref() == Some(&path) => match result {
                Ok(duration) => {
                    self.media_duration = Some(duration);
                    self.request_redraw();
                }
                Err(error) => self.set_status(format!("Duration unavailable: {error}")),
            },
            AppEvent::Waveform(path, result) if self.path.as_ref() == Some(&path) => {
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
            AppEvent::Thumbnail(path, bucket, result) if self.path.as_ref() == Some(&path) => {
                if self.thumbnail_loading != Some(bucket) {
                    return;
                }
                self.thumbnail_loading = None;
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
            AppEvent::Duration(_, _) | AppEvent::Waveform(_, _) | AppEvent::Thumbnail(_, _, _) => {}
        }
    }

    fn handle_playback_event(&mut self, event: PlaybackEvent) {
        if event.generation() != self.generation {
            return;
        }
        match event {
            PlaybackEvent::VideoReady(_) => {
                if let Some(started) = self.pending_seek_started.take() {
                    let latency = started.elapsed();
                    self.seek_latencies.push(latency);
                    eprintln!(
                        "towavue: seek_latency_ms={:.3}",
                        latency.as_secs_f64() * 1000.0
                    );
                }
                self.load_next_frame();
            }
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
            let mut clock = PlaybackClock::new(presentation_time, self.playback_rate());
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
        let due = self.frame_is_due();
        if due {
            let video_time = self.pending_time.take().expect("due frame exists");
            if let Some(audio_time) = self.audio_master_position() {
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

    fn render_frame(&mut self) {
        self.advance_media();
        let (Some(window), Some(context)) = (self.window.as_ref(), self.ui_context.clone()) else {
            return;
        };
        let input = self
            .ui_state
            .as_mut()
            .expect("UI state exists")
            .take_egui_input(window);
        let mut actions = Vec::new();
        let output = context.run_ui(input, |ui| {
            self.draw_ui(ui, &mut actions);
        });
        self.ui_repaint_at = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .and_then(|viewport| Instant::now().checked_add(viewport.repaint_delay));
        let renderer = self.renderer.as_mut().expect("renderer exists");
        let media_result = renderer.clear([0.025, 0.025, 0.03, 1.0]).and_then(|()| {
            if let (Some(session), Some(rect)) = (&mut self.session, self.video_rect) {
                session.draw_current(renderer, rect * context.pixels_per_point())?;
            }
            Ok(())
        });
        if let Err(error) = media_result {
            self.handle_render_error(error);
            return;
        }
        self.check_eof();
        let platform_output = match self
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
        // Consumed keys can emit actions only in an earlier, discarded layout pass.
        for (index, action) in actions.iter().enumerate() {
            if !actions[..index].contains(action) {
                self.handle_ui_action(action.clone());
            }
        }
    }

    fn draw_ui(&mut self, root: &mut egui::Ui, actions: &mut Vec<UiAction>) {
        self.video_rect = None;
        let context = root.ctx().clone();
        self.draw_top_bar(root, actions);
        let status_rect = self.draw_status_bar(root, actions);
        self.draw_timeline(root, actions);
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(root, |ui| {
                if self.path.is_none() {
                    ui.vertical_centered(|ui| {
                        ui.add_space((ui.available_height() * 0.35).max(20.0));
                        ui.heading("towavue");
                        ui.label("Open a media file or a folder to begin.");
                        ui.horizontal(|ui| {
                            if ui.button("Open file").clicked() {
                                actions.push(UiAction::Command(CommandId::OpenFile));
                            }
                            if ui.button("Open folder").clicked() {
                                actions.push(UiAction::Command(CommandId::OpenFolder));
                            }
                        });
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
                    self.draw_video_edit_overlay(ui);
                }
            });
        self.draw_seek_bar(&context, status_rect, actions);
        if self.filmstrip_open {
            self.draw_filmstrip(&context, actions);
        }
        if self.palette_open {
            self.draw_command_palette(&context, actions);
        }
        self.draw_grid_menu(&context, actions);
        self.draw_export_status(&context, actions);
        if let Some(error) = &self.export_error {
            egui::Modal::new("export-error".into()).show(&context, |ui| {
                ui.set_max_width(520.0);
                ui.heading("Export failed");
                ui.label("Your edits and existing files have been kept.");
                egui::ScrollArea::vertical()
                    .max_height(220.0)
                    .show(ui, |ui| {
                        ui.label(error);
                    });
                if ui.button("OK").clicked() {
                    actions.push(UiAction::DismissExportError);
                }
            });
        } else if self.pending_guard.is_some() {
            self.draw_unsaved_guard(&context, actions);
        }
    }

    fn draw_export_status(&self, context: &egui::Context, actions: &mut Vec<UiAction>) {
        let Some(export) = &self.active_export else {
            return;
        };
        let mut contents = |ui: &mut egui::Ui| {
            ui.set_max_width(340.0);
            ui.label(display_name(&export.request.target));
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
                ui.heading("Exporting before continuing");
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
        egui::Modal::new("unsaved-edit-guard".into()).show(context, |ui| {
            ui.heading("Unsaved edits");
            ui.separator();
            ui.label(format!("Export edits to {name} before continuing?"));
            ui.label("The source file has not been changed.");
            ui.horizontal(|ui| {
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
        let operations = self
            .tabs
            .active()
            .and_then(|tab| self.edits.get(&tab.id))
            .map(|history| history.operations().to_vec())
            .unwrap_or_default();
        let mut transform = ImageTransform::new(image.dimensions(), &operations);
        let viewport = ui.max_rect().shrink(8.0);
        let region = self.image_view.preview_region();
        transform.crop(region);
        let image_size = (transform.size.0 as u32, transform.size.1 as u32);
        let scale = self
            .image_view
            .scale(image_size, (viewport.width(), viewport.height()));
        let displayed = egui::vec2(transform.size.0 * scale, transform.size.1 * scale);
        let center = viewport.center() + egui::vec2(self.image_view.pan.0, self.image_view.pan.1);
        let image_rect = egui::Rect::from_center_size(center, displayed);
        let response = ui.interact(
            viewport,
            ui.id().with("image-surface"),
            egui::Sense::click_and_drag(),
        );

        let (control, shift, scroll, pointer, pointer_delta) = ui.input(|input| {
            (
                input.modifiers.ctrl,
                input.modifiers.shift,
                input.smooth_scroll_delta.y,
                input.pointer.hover_pos(),
                input.pointer.delta(),
            )
        });
        if control && scroll != 0.0 && pointer.is_some_and(|point| viewport.contains(point)) {
            let old_scale = scale;
            self.image_view.zoom_by(
                if scroll > 0.0 { 1.1 } else { 1.0 / 1.1 },
                image_size,
                (viewport.width(), viewport.height()),
            );
            let new_scale = self
                .image_view
                .scale(image_size, (viewport.width(), viewport.height()));
            if let Some(pointer) = pointer {
                let from_center = pointer - center;
                let correction = from_center * (1.0 - new_scale / old_scale);
                self.image_view.pan.0 += correction.x;
                self.image_view.pan.1 += correction.y;
            }
        }
        if response.dragged_by(egui::PointerButton::Secondary) {
            self.image_view.pan.0 += pointer_delta.x;
            self.image_view.pan.1 += pointer_delta.y;
        }

        if !self.image_view.crop_preview {
            self.update_selection(&response, image_rect, image_size, shift, pointer);
        }

        let painter = ui.painter_at(viewport);
        paint_transformed_image(&painter, texture, image_rect, transform.uv);
        if !self.image_view.crop_preview
            && let Some(selection) = self.image_view.selection
        {
            paint_selection(&painter, image_rect, selection);
        }
    }

    fn draw_video_edit_overlay(&mut self, ui: &mut egui::Ui) {
        let Some((width, height, pixel_aspect)) = self
            .session
            .as_ref()
            .and_then(PlaybackSession::video_geometry)
        else {
            return;
        };
        let viewport = fitted_video_rect(ui.max_rect(), (width, height), pixel_aspect);
        self.video_rect = Some(viewport);
        let response = ui.interact(
            viewport,
            ui.id().with("video-edit-surface"),
            egui::Sense::click_and_drag(),
        );
        let (shift, pointer) = ui.input(|input| (input.modifiers.shift, input.pointer.hover_pos()));
        self.update_selection(&response, viewport, (width, height), shift, pointer);
        if let Some(selection) = self.image_view.selection {
            paint_selection(&ui.painter_at(viewport), viewport, selection);
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
        let Some(pointer) = pointer else { return };
        let point = unit_point(pointer, image_rect);
        if response.drag_started_by(egui::PointerButton::Primary) && image_rect.contains(pointer) {
            self.selection_drag = self
                .image_view
                .selection
                .and_then(|selection| selection_edge(pointer, image_rect, selection))
                .or_else(|| {
                    self.image_view
                        .selection
                        .filter(|selection| selection.contains(point))
                        .map(|_| SelectionDrag::New(point))
                })
                .or(Some(SelectionDrag::New(point)));
        }
        if response.dragged_by(egui::PointerButton::Primary) {
            match self.selection_drag {
                Some(SelectionDrag::New(start)) => {
                    self.image_view.selection =
                        Some(UnitRect::from_drag(start, point, image_size, square));
                }
                Some(edge) => self.resize_selection(edge, point, square, image_size),
                None => {}
            }
        }
        if response.drag_stopped_by(egui::PointerButton::Primary) {
            self.selection_drag = None;
            if self
                .image_view
                .selection
                .is_some_and(|selection| !selection.is_visible())
            {
                self.image_view.selection = None;
            }
        }
        if self.media_kind == Some(MediaKind::Image)
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
        let pixel_ratio = selection.width() * image_size.0 as f32
            / (selection.height() * image_size.1 as f32).max(1.0);
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
                    let height = selection.width() * image_size.0 as f32
                        / pixel_ratio.max(f32::EPSILON)
                        / image_size.1 as f32;
                    let center = (selection.min.y + selection.max.y) * 0.5;
                    selection.min.y = (center - height * 0.5).max(0.0);
                    selection.max.y = (center + height * 0.5).min(1.0);
                }
                SelectionDrag::Top | SelectionDrag::Bottom => {
                    let width = selection.height() * image_size.1 as f32 * pixel_ratio
                        / image_size.0 as f32;
                    let center = (selection.min.x + selection.max.x) * 0.5;
                    selection.min.x = (center - width * 0.5).max(0.0);
                    selection.max.x = (center + width * 0.5).min(1.0);
                }
                SelectionDrag::New(_) => {}
            }
        }
        self.image_view.selection = Some(selection);
    }

    fn draw_reading_pages(&self, ui: &mut egui::Ui) {
        let viewport = ui.max_rect().shrink(8.0);
        let first = self
            .image
            .as_ref()
            .map(Ok)
            .or_else(|| self.image_error.as_ref().map(Err));
        let count = self.reading_pages.len() + usize::from(first.is_some());
        if count == 0 {
            ui.centered_and_justified(|ui| ui.label("No image pages available"));
            return;
        }
        let gap = 8.0;
        let painter = ui.painter_at(viewport);
        for (index, image) in first
            .into_iter()
            .chain(self.reading_pages.iter().map(|page| page.as_ref()))
            .enumerate()
        {
            let index = if self.reading_settings.reversed {
                count - 1 - index
            } else {
                index
            };
            let page = reading_page_rect(viewport, count, index, self.reading_settings.axis, gap);
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
            let dimensions = image.dimensions();
            let scale = towavue_core::fit_scale(dimensions, (page.width(), page.height()));
            let size = egui::vec2(dimensions.0 as f32 * scale, dimensions.1 as f32 * scale);
            painter.image(
                image.texture.id(),
                egui::Rect::from_center_size(page.center(), size),
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }

    fn draw_top_bar(&self, root: &mut egui::Ui, actions: &mut Vec<UiAction>) {
        let window_rect = root.max_rect();
        egui::Panel::top("tabs")
            .exact_size(32.0)
            .frame(chrome::bar())
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    ui.visuals_mut().widgets.inactive.weak_bg_fill = chrome::BACKGROUND;
                    let menu = ui.menu_button("    ", |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(500.0)
                            .show(ui, |ui| {
                                for definition in command_definitions() {
                                    let shortcut = self
                                        .shortcuts
                                        .get(definition.id)
                                        .map(ToString::to_string)
                                        .unwrap_or_default();
                                    let label = format!("{}    {}", definition.title, shortcut);
                                    if ui
                                        .add_enabled(
                                            definition.is_enabled(self.command_context()),
                                            egui::Button::new(label),
                                        )
                                        .clicked()
                                    {
                                        actions.push(UiAction::Command(definition.id));
                                        ui.close();
                                    }
                                }
                            });
                    });
                    chrome::logo(ui, menu.response.rect);
                    menu.response.on_hover_text("towavue menu");

                    let controls_width = 98.0;
                    let strip_width = (ui.available_width() - controls_width - 56.0).max(80.0);
                    let width = chrome::tab_width(strip_width, self.tabs.tabs().len());
                    egui::ScrollArea::horizontal()
                        .id_salt("tab-strip")
                        .max_width(strip_width)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for tab in self.tabs.tabs() {
                                    let active =
                                        self.tabs.active().is_some_and(|item| item.id == tab.id);
                                    let dirty =
                                        self.edits.get(&tab.id).is_some_and(EditHistory::is_dirty);
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(width, 26.0),
                                        egui::Sense::hover(),
                                    );
                                    if active {
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
                                    let response = ui
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
                                    if response.clicked_by(egui::PointerButton::Middle) {
                                        actions.push(UiAction::CloseTab(tab.id));
                                    }
                                    if response.drag_stopped()
                                        && response
                                            .interact_pointer_pos()
                                            .is_some_and(|position| !window_rect.contains(position))
                                    {
                                        actions.push(UiAction::DetachTab(tab.id));
                                    }
                                    let close_rect = egui::Rect::from_min_max(
                                        egui::pos2(rect.right() - 24.0, rect.top()),
                                        rect.max,
                                    );
                                    if ui
                                        .put(close_rect, egui::Button::new("×").frame(false))
                                        .on_hover_text("Close tab")
                                        .clicked()
                                    {
                                        actions.push(UiAction::CloseTab(tab.id));
                                    }
                                }
                            });
                        });
                    let (drag_rect, response) = ui.allocate_exact_size(
                        egui::vec2((ui.available_width() - controls_width).max(20.0), 26.0),
                        egui::Sense::click_and_drag(),
                    );
                    if self.tabs.tabs().is_empty() {
                        ui.painter().text(
                            drag_rect.left_center(),
                            Align2::LEFT_CENTER,
                            "towavue",
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

    fn draw_grid_menu(&self, context: &egui::Context, actions: &mut Vec<UiAction>) {
        let opacity =
            context.animate_bool_with_time("grid-menu-animation".into(), self.grid_open, 0.12);
        if opacity <= 0.0 {
            return;
        }
        context.request_repaint();
        let Some(kind) = self.media_kind else { return };
        let commands = self.grid_layouts.get(kind);
        egui::Area::new("grid-menu".into())
            .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .order(egui::Order::Foreground)
            .show(context, |ui| {
                ui.set_opacity(opacity);
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    egui::Grid::new("command-grid")
                        .spacing([6.0, 6.0])
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
                                if ui
                                    .add_enabled(
                                        enabled,
                                        egui::Button::new(label).min_size([110.0, 52.0].into()),
                                    )
                                    .clicked()
                                {
                                    actions.push(UiAction::Command(command));
                                }
                                if index % 4 == 3 {
                                    ui.end_row();
                                }
                            }
                        });
                    ui.weak(format!("Edit {}", self.grid_path.display()));
                });
            });
    }

    fn draw_status_bar(&self, root: &mut egui::Ui, actions: &mut Vec<UiAction>) -> egui::Rect {
        egui::Panel::bottom("status")
            .exact_size(30.0)
            .frame(chrome::bar())
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if self.media_kind.is_some_and(|kind| kind != MediaKind::Image) {
                        let playing = self.state == PlaybackState::Playing;
                        if chrome::button(
                            ui,
                            if playing { "Ⅱ" } else { "▶" },
                            if playing {
                                "Pause (Space)"
                            } else {
                                "Play / replay (Space)"
                            },
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
                        if chrome::button(ui, "≋", "Waveform timeline (T)").clicked() {
                            actions.push(UiAction::Command(CommandId::ToggleTimeline));
                        }
                    } else if self.media_kind == Some(MediaKind::Image) {
                        if chrome::button(ui, "◫", "Reading mode (B)").clicked() {
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
                            ZoomMode::Custom(scale) => format!("{:.0}%", scale * 100.0),
                        };
                        details.push(zoom);
                        details.push(format!("{} {width}×{height}", image.decoded.format));
                    } else if self.session.is_some() {
                        let edit = self.edit_state();
                        details.push(format!("{:.0}%  {:.2}×", edit.volume * 100.0, edit.rate));
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
            let response = seekbar::show(context, status, progress);
            if let Some(pointer) = response.interact_pointer_pos().or(response.hover_pos()) {
                let target =
                    seekbar::item_index(seekbar::ratio(response.rect, pointer.x), images.len());
                response.clone().on_hover_text(format!(
                    "{} / {}  {}",
                    target + 1,
                    images.len(),
                    display_name(&images[target].path)
                ));
                if (response.clicked() || response.drag_stopped()) && target != index {
                    actions.push(UiAction::OpenMedia(images[target].path.clone(), false));
                }
            }
            return;
        }
        if self.timeline_open
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
        let response = seekbar::show(context, status, progress);
        if let Some(pointer) = response.interact_pointer_pos().or(response.hover_pos()) {
            let ratio = seekbar::ratio(response.rect, pointer.x);
            self.draw_seek_preview(&response, ratio, duration);
            if response.clicked() || response.drag_stopped() {
                actions.push(UiAction::Seek(media_time(duration.mul_f32(ratio))));
            }
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
        response.clone().on_hover_ui(|ui| {
            if let Some((cached, texture)) = &self.hover_thumbnail
                && self.media_kind == Some(MediaKind::Video)
                && *cached == bucket
            {
                ui.image((texture.id(), texture.size_vec2()));
            }
            ui.monospace(format_time(media_time(duration.mul_f32(ratio))));
        });
    }

    fn draw_timeline(&mut self, root: &mut egui::Ui, actions: &mut Vec<UiAction>) {
        if !self.timeline_open || self.media_kind == Some(MediaKind::Image) {
            return;
        }
        egui::Panel::bottom("timeline")
            .exact_size(96.0)
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
                let Some(position) = response.interact_pointer_pos().or(response.hover_pos())
                else {
                    return;
                };
                let ratio = ((position.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                let target = duration.mul_f32(ratio);
                self.draw_seek_preview(&response, ratio, duration);
                if response.clicked() || response.drag_stopped() {
                    actions.push(UiAction::Seek(media_time(target)));
                }
            });
    }

    fn draw_audio_playlist(&self, ui: &mut egui::Ui, actions: &mut Vec<UiAction>) {
        ui.heading("Folder playlist");
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            if let Some(snapshot) = &self.folder_snapshot {
                for item in snapshot.items_of_kind(MediaKind::Audio) {
                    if ui
                        .selectable_label(
                            self.path.as_deref() == Some(item.path.as_path()),
                            display_name(&item.path),
                        )
                        .clicked()
                    {
                        actions.push(UiAction::OpenMedia(item.path.clone(), false));
                    }
                }
            }
        });
    }

    fn draw_filmstrip(&self, context: &egui::Context, actions: &mut Vec<UiAction>) {
        egui::Area::new("filmstrip".into())
            .anchor(Align2::CENTER_BOTTOM, [0.0, -36.0])
            .show(context, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    egui::ScrollArea::horizontal()
                        .max_width((context.content_rect().width() - 32.0).max(120.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if let Some(snapshot) = &self.folder_snapshot {
                                    for item in &snapshot.items {
                                        let response = ui.selectable_label(
                                            self.path.as_deref() == Some(item.path.as_path()),
                                            display_name(&item.path),
                                        );
                                        if response.clicked() {
                                            actions.push(UiAction::OpenMedia(
                                                item.path.clone(),
                                                false,
                                            ));
                                        }
                                        if response.middle_clicked() {
                                            actions
                                                .push(UiAction::OpenMedia(item.path.clone(), true));
                                        }
                                    }
                                }
                            });
                        });
                });
            });
    }

    fn draw_command_palette(&mut self, context: &egui::Context, actions: &mut Vec<UiAction>) {
        let commands = self.command_context();
        let (chosen, close) = self.palette.show(context, commands, &self.shortcuts);
        if let Some(command) = chosen {
            actions.push(UiAction::Command(command));
        }
        if close {
            self.palette_open = false;
        }
    }

    fn handle_ui_action(&mut self, action: UiAction) {
        if self.pending_dialog.is_some() {
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
        if self.pending_dialog.is_some() {
            return;
        }
        self.palette_open = false;
        match command {
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
            CommandId::PreviousSameKind => self.navigate(false, true),
            CommandId::NextSameKind => self.navigate(true, true),
            CommandId::ToggleFilmstrip => {
                if !self.filmstrip_open {
                    self.refresh_folder_snapshot();
                }
                self.filmstrip_open = !self.filmstrip_open;
                self.request_redraw();
            }
            CommandId::ToggleCommandPalette => {
                self.palette_open = !self.palette_open;
                self.palette.reset();
                self.request_redraw();
            }
            CommandId::ToggleGridMenu => {
                self.grid_open = !self.grid_open;
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
                if let Some(selection) = self.image_view.selection {
                    self.push_edit(EditOperation::Crop(selection));
                    self.image_view.selection = None;
                    self.image_view.crop_preview = false;
                    self.image_view.fit();
                } else {
                    self.set_status("Drag on the image to create a crop selection".into());
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
                let volume = if self.edit_state().volume == 0.0 {
                    1.0
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
                self.timeline_open = !self.timeline_open;
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

    fn push_visual_edit(&mut self, operation: EditOperation) {
        self.push_edit(operation);
        self.image_view.selection = None;
        self.image_view.crop_preview = false;
        self.image_view.fit();
    }

    fn push_edit(&mut self, operation: EditOperation) {
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
                _ => "Edit added (source unchanged)".into(),
            });
            self.refresh_title();
            self.request_redraw();
        }
    }

    fn undo_edit(&mut self, redo: bool) {
        let Some(id) = self.tabs.active().map(|tab| tab.id) else {
            return;
        };
        let changed = self
            .edits
            .get_mut(&id)
            .is_some_and(|history| if redo { history.redo() } else { history.undo() });
        if changed {
            self.sync_playback_edits();
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
            if session.rate() != state.rate {
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
        if self.pending_dialog.is_some() {
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

    fn finish_dialog(&mut self, result: Result<Option<PathBuf>, DialogError>) {
        if let (Some(window), Some(state)) = (&self.window, &mut self.ui_state)
            && let Some((x, y)) = cursor_position_in_window(window)
        {
            // The native dialog can consume CursorEntered; a stationary click still needs a position.
            let _ = state.on_window_event(
                window,
                &WindowEvent::CursorMoved {
                    device_id: winit::event::DeviceId::dummy(),
                    position: winit::dpi::PhysicalPosition::new(f64::from(x), f64::from(y)),
                },
            );
        }
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
                            "Exported {} ({encoder} encode)",
                            export.request.target.display()
                        ));
                        self.refresh_title();
                        if let Some(action) = export.continuation {
                            self.request_guarded(action);
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
        self.request_redraw();
    }

    fn zoom_image(&mut self, factor: f32) {
        let Some(image) = &self.image else { return };
        let size = image.dimensions();
        self.image_view.zoom_by(factor, size, (960.0, 576.0));
        self.request_redraw();
    }

    fn activate_tab(&mut self, id: TabId) {
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
        if self.pending_dialog.is_some() {
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
        if self.tabs.close(id).is_none() {
            return;
        }
        self.edits.remove(&id);
        self.export_paths.remove(&id);
        if let Some((path, kind)) = self.tabs.active().map(|tab| {
            (
                tab.target.current_path().to_owned(),
                tab.target.media_kind(),
            )
        }) {
            self.load_path(path, kind);
        } else {
            self.session.take();
            self.image_generation = self.image_loader.request(Vec::new());
            self.image_loading = false;
            self.image_error = None;
            self.image = None;
            self.reading_pages.clear();
            self.path = None;
            self.media_kind = None;
            self.folder_snapshot = None;
            self.folder_watcher = None;
            self.folder_order.request(None);
            self.pending_folder = None;
            self.pending_time = None;
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
        if self.state == PlaybackState::Ended {
            self.seek_to(MediaTime::ZERO);
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
        if !matches!(self.state, PlaybackState::Playing | PlaybackState::Paused)
            || self.session.is_none()
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
        let rate = self.edit_state().rate;
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if self.state == PlaybackState::Ended {
            if let Err(error) = session.set_paused(true) {
                self.fail(error.to_string());
                return;
            }
            self.state = PlaybackState::Paused;
        }
        match session.seek_at_rate(target, rate) {
            Ok(generation) => {
                self.generation = generation;
                self.pending_time = None;
                self.clock = None;
                self.decode_finished = false;
                self.audio_drained = !session.has_audio();
                self.metrics_recorded = false;
                self.pending_seek_started = Some(Instant::now());
                self.set_status(format!("Position {:.0}s", target.as_seconds_f64()));
                self.refresh_title();
            }
            Err(error) => self.fail(error.to_string()),
        }
    }

    fn recover_graphics_device(&mut self, position: MediaTime) {
        let Some(window) = &self.window else {
            self.fail("window was unavailable during graphics recovery".to_owned());
            return;
        };
        let mut renderer = match FrameRenderer::new(window) {
            Ok(renderer) => renderer,
            Err(error) => {
                self.fail(format!("D3D11 device recovery failed: {error}"));
                return;
            }
        };
        let size = window.inner_size();
        if let Err(error) = renderer.resize_surface(size.width, size.height) {
            self.fail(format!("D3D11 surface recovery failed: {error}"));
            return;
        }
        let graphics_device = renderer.graphics_device();
        let Some(session) = self.session.as_mut() else {
            self.renderer = Some(renderer);
            return;
        };
        match session.replace_graphics_device(graphics_device, position) {
            Ok(generation) => {
                self.renderer = Some(renderer);
                self.generation = generation;
                self.pending_time = None;
                self.clock = None;
                self.decode_finished = false;
                self.audio_drained = !session.has_audio();
                self.metrics_recorded = false;
                self.pending_seek_started = Some(Instant::now());
            }
            Err(error) => self.fail(format!("D3D11 pipeline recovery failed: {error}")),
        }
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
            Some(AudioOutputEvent::EndpointChanged) => self.seek_to(self.current_position()),
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
        self.audio_master_position()
            .or_else(|| self.clock.as_ref().map(PlaybackClock::position))
            .or_else(|| {
                self.session
                    .as_ref()
                    .and_then(PlaybackSession::audio_position)
            })
            .or(self.pending_time)
            .unwrap_or(MediaTime::ZERO)
    }

    fn check_eof(&mut self) {
        if self.decode_finished && self.pending_time.is_none() && self.audio_drained {
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
        }
    }

    fn handle_render_error(&mut self, error: RenderError) {
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
        eprintln!("towavue: {error}");
        self.set_status(error);
        self.state = PlaybackState::Faulted;
        self.refresh_title();
    }

    fn set_status(&mut self, message: String) {
        self.status_message = Some((message, Instant::now()));
        self.request_redraw();
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
        format!("{name} — towavue ({:?})", self.state)
    }

    fn command_context(&self) -> CommandContext {
        CommandContext {
            media_kind: self.media_kind,
            palette_open: self.palette_open,
            filmstrip_open: self.filmstrip_open,
            reading_mode: self.reading_mode,
        }
    }

    fn process_key(&mut self, event: &KeyEvent) {
        if self.pending_dialog.is_some()
            || self.pending_guard.is_some()
            || self.export_error.is_some()
            || self
                .active_export
                .as_ref()
                .is_some_and(|export| export.continuation.is_some())
        {
            return;
        }
        if event.state != ElementState::Pressed || event.repeat {
            return;
        }
        if event.logical_key == WinitKey::Named(NamedKey::Escape)
            && (self.palette_open || self.filmstrip_open || self.grid_open)
        {
            self.palette_open = false;
            self.filmstrip_open = false;
            self.grid_open = false;
            self.request_redraw();
            return;
        }
        if self.grid_open
            && let WinitKey::Character(value) = &event.logical_key
            && let Some(character) = value.chars().next().map(|value| value.to_ascii_lowercase())
            && let Some(index) = grid::KEYS.iter().position(|key| *key == character)
            && let Some(kind) = self.media_kind
        {
            let command = self.grid_layouts.get(kind)[index];
            let enabled = command_definitions()
                .iter()
                .find(|definition| definition.id == command)
                .is_some_and(|definition| definition.is_enabled(self.command_context()));
            if enabled {
                self.grid_open = false;
                self.dispatch(command);
            }
            return;
        }
        if self.filmstrip_open && event.logical_key == WinitKey::Named(NamedKey::Tab) {
            self.navigate(!self.modifiers.shift_key(), false);
            return;
        }
        let Some(stroke) = self.key_stroke(event) else {
            return;
        };
        if self
            .prefix_started
            .is_some_and(|started| started.elapsed() >= PREFIX_TIMEOUT)
        {
            self.entered_shortcut.clear();
        }
        self.entered_shortcut.push(stroke.clone());
        match self
            .shortcuts
            .resolve(&self.entered_shortcut, self.command_context())
        {
            ShortcutMatch::Command(command) => {
                self.entered_shortcut.clear();
                self.prefix_started = None;
                self.dispatch(command);
            }
            ShortcutMatch::Prefix => {
                self.prefix_started = Some(Instant::now());
                self.set_status(format!(
                    "{} …",
                    self.entered_shortcut
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(" ")
                ));
            }
            ShortcutMatch::None => {
                self.entered_shortcut.clear();
                self.entered_shortcut.push(stroke);
                let retry = self
                    .shortcuts
                    .resolve(&self.entered_shortcut, self.command_context());
                self.entered_shortcut.clear();
                self.prefix_started = None;
                if let ShortcutMatch::Command(command) = retry {
                    self.dispatch(command);
                }
            }
        }
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
            event_loop.exit();
            return;
        }
        self.poll_audio();
        let now = Instant::now();
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
        if self
            .prefix_started
            .is_some_and(|started| started.elapsed() >= PREFIX_TIMEOUT)
        {
            self.entered_shortcut.clear();
            self.prefix_started = None;
        }
        if self.state == PlaybackState::Playing {
            if let Some(audio_position) = self.audio_master_position() {
                let cutoff = audio_position.saturating_sub(VIDEO_LATE_TOLERANCE);
                if let Some(session) = self.session.as_mut()
                    && session.drop_video_before(cutoff) > 0
                {
                    self.pending_time = session.pending_video_time();
                }
            }
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
                self.request_redraw();
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    self.ui_repaint_at
                        .map_or(Instant::now() + AUDIO_EVENT_POLL_INTERVAL, |ui| {
                            ui.min(Instant::now() + AUDIO_EVENT_POLL_INTERVAL)
                        }),
                ));
                return;
            }
        }
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
        if self.folder_watcher.is_some()
            || next_image_frame.is_some()
            || self.ui_repaint_at.is_some()
        {
            let folder_poll = self
                .folder_watcher
                .is_some()
                .then(|| Instant::now() + FOLDER_EVENT_POLL_INTERVAL);
            let deadline = [folder_poll, next_image_frame, self.ui_repaint_at]
                .into_iter()
                .flatten()
                .min()
                .expect("at least one scheduled wakeup exists");
            event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }

    fn request_redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
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

fn paint_transformed_image(
    painter: &egui::Painter,
    texture: egui::TextureId,
    rect: egui::Rect,
    uv: [UnitPoint; 4],
) {
    let mut mesh = egui::Mesh::with_texture(texture);
    for (position, uv) in [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ]
    .into_iter()
    .zip(uv)
    {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: position,
            uv: egui::pos2(uv.x, uv.y),
            color: Color32::WHITE,
        });
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    painter.add(egui::Shape::mesh(mesh));
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

fn reading_page_rect(
    viewport: egui::Rect,
    count: usize,
    index: usize,
    axis: ReadingAxis,
    gap: f32,
) -> egui::Rect {
    let count = count.max(1) as f32;
    match axis {
        ReadingAxis::Horizontal => {
            let width = (viewport.width() - gap * (count - 1.0)).max(1.0) / count;
            let left = viewport.left() + index as f32 * (width + gap);
            egui::Rect::from_min_size(
                egui::pos2(left, viewport.top()),
                egui::vec2(width, viewport.height()),
            )
        }
        ReadingAxis::Vertical => {
            let height = (viewport.height() - gap * (count - 1.0)).max(1.0) / count;
            let top = viewport.top() + index as f32 * (height + gap);
            egui::Rect::from_min_size(
                egui::pos2(viewport.left(), top),
                egui::vec2(viewport.width(), height),
            )
        }
    }
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
        let event_response = match (self.window.as_ref(), self.ui_state.as_mut()) {
            (Some(window), Some(state)) => Some(state.on_window_event(window, &event)),
            _ => None,
        };
        let (consumed, repaint) = event_response
            .map(|response| (response.consumed, response.repaint))
            .unwrap_or_default();
        if repaint {
            self.request_redraw();
        }
        match event {
            WindowEvent::CloseRequested => self.request_guarded(GuardedAction::Exit),
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

    fn isolated_test_root(test_name: &str) -> Option<PathBuf> {
        const TEST_ROOT: &str = "TOWAVUE_APP_TEST_ROOT";
        let Some(root) = std::env::var_os(TEST_ROOT) else {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time is after the epoch")
                .as_nanos();
            let root = std::env::temp_dir().join(format!("towavue-app-test-{unique}"));
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
            ImagePresentation::from_decoded(&context, Path::new("wide.png"), decoded.clone())
                .is_err()
        );
        context.input_mut(|input| input.max_texture_side = 8192);
        let image = ImagePresentation::from_decoded(&context, Path::new("wide.png"), decoded)
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
    fn reading_page_geometry_follows_the_selected_axis() {
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(300.0, 200.0));

        let horizontal = reading_page_rect(viewport, 2, 1, ReadingAxis::Horizontal, 8.0);
        let vertical = reading_page_rect(viewport, 2, 1, ReadingAxis::Vertical, 8.0);

        assert_eq!(horizontal.left(), 154.0);
        assert_eq!(horizontal.width(), 146.0);
        assert_eq!(vertical.top(), 104.0);
        assert_eq!(vertical.height(), 96.0);
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
                EditOperation::Crop(UnitRect {
                    min: UnitPoint { x: 0.25, y: 0.0 },
                    max: UnitPoint { x: 0.75, y: 1.0 },
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
}
