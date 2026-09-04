//! Application entry point for towavue.

#![forbid(unsafe_code)]

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
    AudioOutputEvent, DecodedImage, ExportRequest, FolderOrderProvider, FolderWatcher,
    FrameRenderer, PlaybackEvent, PlaybackSession, RenderError, decode_image, export_media,
    pick_export_file, pick_folder, pick_media_file,
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
    let event_loop = EventLoop::<PlaybackEvent>::with_user_event().build()?;
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
}

impl PlaybackClock {
    fn new(media_anchor: MediaTime) -> Self {
        Self {
            media_anchor,
            wall_anchor: Instant::now(),
            paused_at: None,
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
        self.wall_anchor + Duration::from_nanos(delta.max(0) as u64)
    }

    fn position(&self) -> MediaTime {
        let now = self.paused_at.unwrap_or_else(Instant::now);
        self.media_anchor
            .saturating_add(now.saturating_duration_since(self.wall_anchor))
    }
}

enum UiAction {
    Command(CommandId),
    ActivateTab(TabId),
    CloseTab(TabId),
    OpenMedia(PathBuf, bool),
    ResolveGuard(GuardDecision),
}

#[derive(Clone)]
enum GuardedAction {
    CloseTab(TabId),
    Navigate(PathBuf),
    Exit,
}

#[derive(Clone, Copy)]
enum GuardDecision {
    Save,
    Discard,
    Cancel,
}

struct ImagePresentation {
    decoded: DecodedImage,
    texture: TextureHandle,
    frame_index: usize,
    next_frame_at: Option<Instant>,
}

impl ImagePresentation {
    fn load(context: &egui::Context, path: &Path) -> Result<Self, String> {
        let decoded = decode_image(path).map_err(|error| error.to_string())?;
        let first = decoded
            .frames
            .first()
            .ok_or_else(|| "decoded image contained no frames".to_owned())?;
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
    window: Option<Window>,
    renderer: Option<FrameRenderer>,
    ui_context: Option<egui::Context>,
    ui_state: Option<egui_winit::State>,
    folder_order: FolderOrderProvider,
    folder_snapshot: Option<FolderSnapshot>,
    folder_watcher: Option<(PathBuf, FolderWatcher)>,
    tabs: TabSet,
    edits: BTreeMap<TabId, EditHistory>,
    export_paths: BTreeMap<TabId, PathBuf>,
    path: Option<PathBuf>,
    media_kind: Option<MediaKind>,
    image: Option<ImagePresentation>,
    image_view: ImageViewState,
    selection_drag: Option<SelectionDrag>,
    reading_mode: bool,
    reading_settings: ReadingSettings,
    reading_pages: Vec<ImagePresentation>,
    session: Option<PlaybackSession>,
    pending_time: Option<MediaTime>,
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
    palette_open: bool,
    palette_query: String,
    status_message: Option<(String, Instant)>,
    pending_guard: Option<GuardedAction>,
    exit_requested: bool,
}

impl<N> Application<N>
where
    N: Fn(PlaybackEvent) + Send + Sync + 'static,
{
    fn new(initial_path: Option<PathBuf>, notify: N) -> Result<Self, Box<dyn Error>> {
        let (shortcuts, shortcut_path) = shortcuts::load()
            .map_err(|error| format!("could not load keyboard shortcuts: {error}"))?;
        Ok(Self {
            initial_path,
            notify: Arc::new(notify),
            window: None,
            renderer: None,
            ui_context: None,
            ui_state: None,
            folder_order: FolderOrderProvider::new()?,
            folder_snapshot: None,
            folder_watcher: None,
            tabs: TabSet::default(),
            edits: BTreeMap::new(),
            export_paths: BTreeMap::new(),
            path: None,
            media_kind: None,
            image: None,
            image_view: ImageViewState::default(),
            selection_drag: None,
            reading_mode: false,
            reading_settings: ReadingSettings::default(),
            reading_pages: Vec::new(),
            session: None,
            pending_time: None,
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
            palette_open: false,
            palette_query: String::new(),
            status_message: None,
            pending_guard: None,
            exit_requested: false,
        })
    }

    fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Box<dyn Error>> {
        let attributes = Window::default_attributes()
            .with_title(self.title())
            .with_inner_size(LogicalSize::new(960, 576));
        let window = event_loop.create_window(attributes)?;
        let mut renderer = FrameRenderer::new(&window)?;
        let size = window.inner_size();
        renderer.resize_surface(size.width, size.height)?;
        let context = egui::Context::default();
        context.set_visuals(egui::Visuals::dark());
        let state = egui_winit::State::new(
            context.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            window.theme(),
            Some(4096),
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
        let Some(kind) = MediaKind::from_path(&path) else {
            self.set_status(format!("Unsupported media: {}", path.display()));
            return;
        };
        let folder = path.parent().unwrap_or_else(|| Path::new(""));
        let dirty_playlist = kind == MediaKind::Audio
            && self.tabs.tabs().iter().any(|tab| {
                matches!(&tab.target, TabTarget::AudioFolder { folder: open, .. } if open == folder)
                    && self.edits.get(&tab.id).is_some_and(EditHistory::is_dirty)
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
        match self.folder_order.snapshot(&folder) {
            Ok(snapshot) => {
                let first = snapshot.items.first().map(|item| item.path.clone());
                self.folder_snapshot = Some(snapshot);
                if let Some(path) = first {
                    self.open_external(path, false);
                } else {
                    self.set_status(format!("No supported media in {}", folder.display()));
                }
            }
            Err(error) => self.set_status(error.to_string()),
        }
    }

    fn load_path(&mut self, path: PathBuf, kind: MediaKind) {
        self.session.take();
        self.image = None;
        self.reading_pages.clear();
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
            let Some(context) = self.ui_context.as_ref() else {
                self.fail("UI context is unavailable".to_owned());
                return;
            };
            match ImagePresentation::load(context, &path) {
                Ok(image) => {
                    let (width, height) = image.dimensions();
                    let format = image.decoded.format;
                    let frames = image.decoded.frames.len();
                    self.image = Some(image);
                    self.state = PlaybackState::Paused;
                    self.set_status(format!(
                        "{format} · {width} × {height} · {frames} frame{}",
                        if frames == 1 { "" } else { "s" }
                    ));
                    if self.reading_mode {
                        self.rebuild_reading_pages();
                    }
                }
                Err(error) => self.fail(error),
            }
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
        match PlaybackSession::open(&path, graphics_device, move |event| notify(event)) {
            Ok(session) => {
                self.generation = session.generation();
                self.audio_drained = !session.has_audio();
                self.session = Some(session);
                self.state = PlaybackState::Playing;
            }
            Err(error) => self.fail(error.to_string()),
        }
        self.refresh_title();
        self.request_redraw();
    }

    fn refresh_folder_snapshot(&mut self) {
        let folder = self
            .path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_owned);
        let Some(folder) = folder else { return };
        self.watch_folder(&folder);
        match self.folder_order.snapshot(&folder) {
            Ok(snapshot) => {
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
                }
            }
            Err(error) => self.set_status(error.to_string()),
        }
        if self.reading_mode && self.image.is_some() {
            self.rebuild_reading_pages();
        }
    }

    fn rebuild_reading_pages(&mut self) {
        self.reading_pages.clear();
        let (Some(context), Some(snapshot), Some(path)) = (
            self.ui_context.as_ref(),
            self.folder_snapshot.as_ref(),
            self.path.as_deref(),
        ) else {
            return;
        };
        let paths = snapshot
            .reading_items(
                path,
                self.reading_settings.page_count,
                self.reading_settings.reversed,
            )
            .into_iter()
            .map(|item| item.path.clone())
            .collect::<Vec<_>>();
        let context = context.clone();
        for path in paths {
            match ImagePresentation::load(&context, &path) {
                Ok(image) => self.reading_pages.push(image),
                Err(error) => {
                    self.set_status(format!("Could not load {}: {error}", path.display()));
                    break;
                }
            }
        }
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
            self.check_eof();
            return;
        };
        if self.clock.is_none() {
            self.clock = Some(PlaybackClock::new(presentation_time));
        }
        self.pending_time = Some(presentation_time);
    }

    fn frame_is_due(&self) -> bool {
        let Some(presentation_time) = self.pending_time else {
            return false;
        };
        if let Some(audio_position) = self.audio_master_position() {
            presentation_time <= audio_position.saturating_add(VIDEO_EARLY_TOLERANCE)
        } else {
            self.clock
                .as_ref()
                .is_none_or(|clock| clock.due_at(presentation_time) <= Instant::now())
        }
    }

    fn draw_media(&mut self) -> Result<(), RenderError> {
        let due = self.frame_is_due();
        self.renderer
            .as_mut()
            .ok_or(RenderError::SurfaceNotSized)?
            .clear([0.025, 0.025, 0.03, 1.0])?;
        if due {
            let video_time = self.pending_time.take().expect("due frame exists");
            if let Some(audio_time) = self.audio_master_position() {
                self.drift_samples.push(Duration::from_nanos(
                    video_time
                        .as_nanoseconds()
                        .abs_diff(audio_time.as_nanoseconds()),
                ));
            }
            if let (Some(session), Some(renderer)) = (self.session.as_mut(), self.renderer.as_mut())
            {
                session.present_pending(renderer)?;
            }
            self.load_next_frame();
            self.check_eof();
        } else if let (Some(session), Some(renderer)) =
            (self.session.as_ref(), self.renderer.as_mut())
        {
            session.draw_current(renderer)?;
        }
        Ok(())
    }

    fn render_frame(&mut self) {
        if let Err(error) = self.draw_media() {
            self.handle_render_error(error);
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
        let output = context.run_ui(input, |ui| self.draw_ui(ui, &mut actions));
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
        for action in actions {
            self.handle_ui_action(action);
        }
    }

    fn draw_ui(&mut self, root: &mut egui::Ui, actions: &mut Vec<UiAction>) {
        let context = root.ctx().clone();
        self.draw_top_bar(root, actions);
        self.draw_status_bar(root, actions);
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
                } else if self.media_kind == Some(MediaKind::Video) {
                    self.draw_video_edit_overlay(ui);
                }
            });
        if self.filmstrip_open {
            self.draw_filmstrip(&context, actions);
        }
        if self.palette_open {
            self.draw_command_palette(&context, actions);
        }
        if self.pending_guard.is_some() {
            self.draw_unsaved_guard(&context, actions);
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
                if ui.button("Export and continue").clicked() {
                    actions.push(UiAction::ResolveGuard(GuardDecision::Save));
                }
                if ui.button("Discard edits").clicked() {
                    actions.push(UiAction::ResolveGuard(GuardDecision::Discard));
                }
                if ui.button("Cancel").clicked() {
                    actions.push(UiAction::ResolveGuard(GuardDecision::Cancel));
                }
            });
        });
    }

    fn draw_image(&mut self, ui: &mut egui::Ui) {
        if self.reading_mode {
            self.draw_reading_pages(ui);
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
        let viewport = ui.max_rect().shrink(8.0);
        let response = ui.interact(
            viewport,
            ui.id().with("video-edit-surface"),
            egui::Sense::click_and_drag(),
        );
        let (shift, pointer) = ui.input(|input| (input.modifiers.shift, input.pointer.hover_pos()));
        self.update_selection(
            &response,
            viewport,
            (viewport.width() as u32, viewport.height() as u32),
            shift,
            pointer,
        );
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
        let count = self.reading_pages.len();
        if count == 0 {
            ui.centered_and_justified(|ui| ui.label("No image pages available"));
            return;
        }
        let gap = 8.0;
        let painter = ui.painter_at(viewport);
        for (index, image) in self.reading_pages.iter().enumerate() {
            let page = reading_page_rect(viewport, count, index, self.reading_settings.axis, gap);
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
        egui::Panel::top("tabs").exact_size(32.0).show(root, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button(RichText::new("towavue").strong(), |ui| {
                    for definition in command_definitions() {
                        let shortcut = self
                            .shortcuts
                            .get(definition.id)
                            .map(ToString::to_string)
                            .unwrap_or_default();
                        let label = if shortcut.is_empty() {
                            definition.title.to_owned()
                        } else {
                            format!("{}    {}", definition.title, shortcut)
                        };
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
                egui::ScrollArea::horizontal()
                    .id_salt("tab-strip")
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for tab in self.tabs.tabs() {
                                let active =
                                    self.tabs.active().is_some_and(|item| item.id == tab.id);
                                if ui
                                    .selectable_label(
                                        active,
                                        format!(
                                            "{}{}",
                                            display_name(tab.target.current_path()),
                                            if self
                                                .edits
                                                .get(&tab.id)
                                                .is_some_and(EditHistory::is_dirty)
                                            {
                                                " *"
                                            } else {
                                                ""
                                            }
                                        ),
                                    )
                                    .clicked()
                                {
                                    actions.push(UiAction::ActivateTab(tab.id));
                                }
                                if ui.small_button("×").clicked() {
                                    actions.push(UiAction::CloseTab(tab.id));
                                }
                            }
                        });
                    });
            });
        });
    }

    fn draw_status_bar(&self, root: &mut egui::Ui, actions: &mut Vec<UiAction>) {
        egui::Panel::bottom("status")
            .exact_size(30.0)
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    if self.media_kind.is_some_and(|kind| kind != MediaKind::Image) {
                        let label = if self.state == PlaybackState::Playing {
                            "Pause"
                        } else {
                            "Play"
                        };
                        if ui.small_button(label).clicked() {
                            actions.push(UiAction::Command(CommandId::TogglePause));
                        }
                        ui.monospace(format_time(self.current_position()));
                    }
                    if self.media_kind == Some(MediaKind::Image) {
                        if ui
                            .selectable_label(self.reading_mode, "Reading")
                            .on_hover_text("Toggle reading mode (B)")
                            .clicked()
                        {
                            actions.push(UiAction::Command(CommandId::ToggleReadingMode));
                        }
                        if self.image_view.selection.is_some()
                            && ui
                                .selectable_label(self.image_view.crop_preview, "Crop preview")
                                .on_hover_text("Toggle crop preview (Ctrl+Y)")
                                .clicked()
                        {
                            actions.push(UiAction::Command(CommandId::ToggleCropPreview));
                        }
                    }
                    if let Some((message, _)) = &self.status_message {
                        ui.colored_label(Color32::LIGHT_YELLOW, message);
                    } else if let Some(path) = &self.path {
                        ui.label(display_name(path));
                        ui.weak(
                            path.parent()
                                .unwrap_or_else(|| Path::new(""))
                                .display()
                                .to_string(),
                        );
                        if let Some(index) = self
                            .folder_snapshot
                            .as_ref()
                            .and_then(|snapshot| snapshot.item_index(path))
                        {
                            let count = self
                                .folder_snapshot
                                .as_ref()
                                .map_or(0, |snapshot| snapshot.items.len());
                            ui.monospace(format!("{} / {}", index + 1, count));
                        }
                        if let Ok(metadata) = path.metadata() {
                            ui.monospace(format_size(metadata.len()));
                        }
                        if let Some(image) = &self.image {
                            let (width, height) = image.dimensions();
                            ui.monospace(format!("{width} × {height}"));
                            match self.image_view.zoom {
                                ZoomMode::Fit => ui.monospace("Fit"),
                                ZoomMode::Actual => ui.monospace("100%"),
                                ZoomMode::Custom(scale) => {
                                    ui.monospace(format!("{:.0}%", scale * 100.0))
                                }
                            };
                            if self.reading_mode {
                                ui.weak(format!(
                                    "Reading · {} pages · {:?}",
                                    self.reading_pages.len(),
                                    self.reading_settings.axis
                                ));
                            }
                        }
                        if let Some(snapshot) = &self.folder_snapshot {
                            ui.weak(snapshot_source(snapshot.source));
                        }
                        if self
                            .tabs
                            .active()
                            .and_then(|tab| self.edits.get(&tab.id))
                            .is_some_and(EditHistory::is_dirty)
                        {
                            let edit = self.edit_state();
                            ui.colored_label(Color32::LIGHT_YELLOW, "Unsaved");
                            if self.media_kind.is_some_and(|kind| kind != MediaKind::Image) {
                                ui.monospace(format!(
                                    "Volume {:.0}% · Rate {:.2}×",
                                    edit.volume * 100.0,
                                    edit.rate
                                ));
                            }
                        }
                    } else {
                        ui.weak(format!("Shortcuts: {}", self.shortcut_path.display()));
                    }
                });
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
        let command_context = self.command_context();
        egui::Window::new("Command palette")
            .id("command-palette".into())
            .anchor(Align2::CENTER_TOP, [0.0, 48.0])
            .collapsible(false)
            .resizable(false)
            .default_width(520.0)
            .show(context, |ui| {
                ui.text_edit_singleline(&mut self.palette_query)
                    .request_focus();
                let query = self.palette_query.to_ascii_lowercase();
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        for definition in command_definitions().iter().filter(|definition| {
                            query.is_empty()
                                || definition.title.to_ascii_lowercase().contains(&query)
                        }) {
                            let shortcut = self
                                .shortcuts
                                .get(definition.id)
                                .map(ToString::to_string)
                                .unwrap_or_default();
                            if ui
                                .add_enabled(
                                    definition.is_enabled(command_context),
                                    egui::Button::new(format!(
                                        "{}    {}",
                                        definition.title, shortcut
                                    ))
                                    .min_size(egui::vec2(ui.available_width(), 24.0)),
                                )
                                .clicked()
                            {
                                actions.push(UiAction::Command(definition.id));
                            }
                        }
                    });
            });
    }

    fn handle_ui_action(&mut self, action: UiAction) {
        match action {
            UiAction::Command(command) => self.dispatch(command),
            UiAction::ActivateTab(id) => self.activate_tab(id),
            UiAction::CloseTab(id) => self.request_guarded(GuardedAction::CloseTab(id)),
            UiAction::OpenMedia(path, force_new) => {
                if force_new {
                    self.open_external(path, true);
                } else {
                    self.request_guarded(GuardedAction::Navigate(path));
                }
            }
            UiAction::ResolveGuard(decision) => self.resolve_guard(decision),
        }
    }

    fn dispatch(&mut self, command: CommandId) {
        self.palette_open = false;
        match command {
            CommandId::OpenFile => match pick_media_file() {
                Ok(Some(path)) => self.open_external(path, false),
                Ok(None) => {}
                Err(error) => self.set_status(error.to_string()),
            },
            CommandId::OpenFolder => match pick_folder() {
                Ok(Some(path)) => self.open_folder_path(path),
                Ok(None) => {}
                Err(error) => self.set_status(error.to_string()),
            },
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
                self.palette_query.clear();
                self.request_redraw();
            }
            CommandId::ReloadShortcuts => match shortcuts::load() {
                Ok((bindings, path)) => {
                    self.shortcuts = bindings;
                    self.shortcut_path = path;
                    self.set_status("Keyboard shortcuts reloaded".into());
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
                if self.reading_mode {
                    self.rebuild_reading_pages();
                } else {
                    self.reading_pages.clear();
                }
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
                self.rebuild_reading_pages();
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
                self.export_current(false);
            }
            CommandId::ExportAs => {
                self.export_current(true);
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
            self.set_status("Edit added (source unchanged)".into());
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
            self.image_view.selection = None;
            self.image_view.crop_preview = false;
            self.image_view.fit();
            self.refresh_title();
            self.request_redraw();
        }
    }

    fn edit_state(&self) -> towavue_core::EditState {
        self.tabs
            .active()
            .and_then(|tab| self.edits.get(&tab.id))
            .map(EditHistory::state)
            .unwrap_or_default()
    }

    fn export_current(&mut self, force_dialog: bool) -> bool {
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
        let target = match target {
            Some(target) => target,
            None => {
                let suggested = export_name(&source);
                match pick_export_file(&suggested) {
                    Ok(Some(target)) => target,
                    Ok(None) => return false,
                    Err(error) => {
                        self.set_status(error.to_string());
                        return false;
                    }
                }
            }
        };
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
        };
        match export_media(&request) {
            Ok(()) => {
                self.export_paths.insert(id, target.clone());
                if let Some(history) = self.edits.get_mut(&id) {
                    history.mark_saved();
                }
                self.set_status(format!("Exported {}", target.display()));
                self.refresh_title();
                self.request_redraw();
                true
            }
            Err(error) => {
                self.set_status(error.to_string());
                false
            }
        }
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
        let dirty = match action {
            GuardedAction::CloseTab(id) => self
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
                if self.export_current(false) {
                    self.request_guarded(action);
                } else {
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
            GuardedAction::Navigate(path) => self.navigate_to_unchecked(path),
            GuardedAction::Exit => {
                self.exit_requested = true;
                self.request_redraw();
            }
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
            self.image = None;
            self.reading_pages.clear();
            self.path = None;
            self.media_kind = None;
            self.folder_snapshot = None;
            self.folder_watcher = None;
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
        let paused = match self.state {
            PlaybackState::Playing => true,
            PlaybackState::Paused if self.session.is_some() => false,
            _ => return,
        };
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
        let Some(session) = self.session.as_mut() else {
            return;
        };
        match session.seek(target) {
            Ok(generation) => {
                self.generation = generation;
                self.pending_time = None;
                self.clock = None;
                self.decode_finished = false;
                self.audio_drained = !session.has_audio();
                self.metrics_recorded = false;
                self.pending_seek_started = Some(Instant::now());
                self.set_status(format!("Position {:.0}s", target.as_seconds_f64()));
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
        self.session
            .as_ref()
            .and_then(PlaybackSession::audio_position)
            .or(self.pending_time)
            .or_else(|| self.clock.as_ref().map(PlaybackClock::position))
            .unwrap_or(MediaTime::ZERO)
    }

    fn check_eof(&mut self) {
        if self.decode_finished && self.pending_time.is_none() && self.audio_drained {
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
        if event.state != ElementState::Pressed || event.repeat {
            return;
        }
        if event.logical_key == WinitKey::Named(NamedKey::Escape)
            && (self.palette_open || self.filmstrip_open)
        {
            self.palette_open = false;
            self.filmstrip_open = false;
            self.request_redraw();
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
        let mut image_changed = self
            .image
            .as_mut()
            .is_some_and(|image| image.advance_animation(now));
        for image in &mut self.reading_pages {
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
                    Instant::now() + Duration::from_nanos(wait).min(AUDIO_EVENT_POLL_INTERVAL)
                } else if let Some(clock) = &self.clock {
                    clock.due_at(presentation_time)
                } else {
                    Instant::now()
                };
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
                    Instant::now() + AUDIO_EVENT_POLL_INTERVAL,
                ));
                return;
            }
        }
        let next_image_frame = self
            .image
            .iter()
            .chain(self.reading_pages.iter())
            .filter_map(|image| image.next_frame_at)
            .min();
        if self.folder_watcher.is_some() || next_image_frame.is_some() {
            let folder_poll = self
                .folder_watcher
                .is_some()
                .then(|| Instant::now() + FOLDER_EVENT_POLL_INTERVAL);
            let deadline = [folder_poll, next_image_frame]
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

impl<N> ApplicationHandler<PlaybackEvent> for Application<N>
where
    N: Fn(PlaybackEvent) + Send + Sync + 'static,
{
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none()
            && let Err(error) = self.start(event_loop)
        {
            eprintln!("towavue: {error}");
            event_loop.exit();
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: PlaybackEvent) {
        self.handle_playback_event(event);
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.window.as_ref().map(Window::id) != Some(window_id) {
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
