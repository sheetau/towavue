//! Application entry point for towavue.

#![forbid(unsafe_code)]

mod shortcuts;

use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Align2, Color32, RichText};
use towavue_core::{
    CommandContext, CommandId, FolderSnapshot, FolderSnapshotSource, Key, KeyStroke, MediaKind,
    MediaTime, Modifiers, PlaybackGeneration, PlaybackState, ShortcutBindings, ShortcutMatch,
    TabId, TabSet, command_definitions,
};
use towavue_runtime_windows::{
    AudioOutputEvent, FolderOrderProvider, FolderWatcher, FrameRenderer, PlaybackEvent,
    PlaybackSession, RenderError, pick_folder, pick_media_file,
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
    path: Option<PathBuf>,
    media_kind: Option<MediaKind>,
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
            path: None,
            media_kind: None,
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
        if force_new_tab {
            self.tabs.open_new(path.clone(), kind);
        } else {
            self.tabs.open_external(path.clone(), kind);
        }
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
            self.state = PlaybackState::Paused;
            self.set_status("Image rendering is scheduled for M5".to_owned());
            self.refresh_title();
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
                    ui.centered_and_justified(|ui| ui.label("Image rendering begins in M5"));
                }
            });
        if self.filmstrip_open {
            self.draw_filmstrip(&context, actions);
        }
        if self.palette_open {
            self.draw_command_palette(&context, actions);
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
                                        display_name(tab.target.current_path()),
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
                        if let Some(snapshot) = &self.folder_snapshot {
                            ui.weak(snapshot_source(snapshot.source));
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
            UiAction::CloseTab(id) => self.close_tab(id),
            UiAction::OpenMedia(path, force_new) => {
                if force_new {
                    self.open_external(path, true);
                } else {
                    self.navigate_to(path);
                }
            }
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
                    self.close_tab(id);
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
        }
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

    fn close_tab(&mut self, id: TabId) {
        if self.tabs.close(id).is_none() {
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
        self.navigate_to(paths[index].clone());
    }

    fn navigate_to(&mut self, path: PathBuf) {
        let Some(kind) = MediaKind::from_path(&path) else {
            return;
        };
        if let Some(tab) = self.tabs.active_mut() {
            tab.target.set_current_path(path.clone(), kind);
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
        let name = self
            .path
            .as_deref()
            .map(display_name)
            .unwrap_or_else(|| "Welcome".to_owned());
        format!("{name} — towavue ({:?})", self.state)
    }

    fn command_context(&self) -> CommandContext {
        CommandContext {
            media_kind: self.media_kind,
            palette_open: self.palette_open,
            filmstrip_open: self.filmstrip_open,
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
        let key = match &event.logical_key {
            WinitKey::Character(value) if value.chars().count() == 1 => {
                Key::Character(value.chars().next()?.to_ascii_lowercase())
            }
            WinitKey::Named(NamedKey::Space) => Key::Space,
            WinitKey::Named(NamedKey::ArrowLeft) => Key::ArrowLeft,
            WinitKey::Named(NamedKey::ArrowRight) => Key::ArrowRight,
            WinitKey::Named(NamedKey::Tab) => Key::Tab,
            WinitKey::Named(NamedKey::Escape) => Key::Escape,
            _ => return None,
        };
        Some(KeyStroke {
            modifiers: Modifiers {
                control: self.modifiers.control_key(),
                alt: self.modifiers.alt_key(),
                shift: self.modifiers.shift_key(),
                logo: self.modifiers.super_key(),
            },
            key,
        })
    }

    fn schedule(&mut self, event_loop: &ActiveEventLoop) {
        self.poll_audio();
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
        if self.folder_watcher.is_some() {
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + FOLDER_EVENT_POLL_INTERVAL,
            ));
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

fn display_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
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
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.window.as_ref().map(Window::id) != Some(window_id) {
            return;
        }
        let consumed = match (self.window.as_ref(), self.ui_state.as_mut()) {
            (Some(window), Some(state)) => state.on_window_event(window, &event).consumed,
            _ => false,
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
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
}
