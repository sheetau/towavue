use super::*;

#[path = "window_drop.rs"]
mod dropping;

#[cfg(test)]
#[path = "window_host_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "window_graphics_tests.rs"]
mod graphics_tests;

#[cfg(test)]
#[path = "window_transfer_tests.rs"]
mod transfer_tests;

#[cfg(test)]
#[path = "resume/host_tests.rs"]
mod resume_tests;

#[cfg(test)]
#[path = "window_open_tests.rs"]
mod opening_tests;

#[cfg(test)]
#[path = "window_launch_tests.rs"]
mod launch_tests;

mod file_operations;
mod idle_graphics;
mod source_save;
mod updates;

// Never reused, even after the native HWND or a window-local session ID is reused.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct WindowKey(u64);

#[derive(Clone, Copy)]
pub(crate) struct GraphicsRecoveryRequest {
    pub position: MediaTime,
    pub state: PlaybackState,
    pub retry: bool,
}

pub(crate) enum Event {
    Launch(towavue_runtime_windows::LaunchRequest),
    Update(u64, towavue_runtime_windows::update::UpdateEvent),
    PlaybackVolumePreferenceFailed(String),
    Window(WindowKey, AppEvent),
    Accessibility(accesskit_winit::Event),
}

impl From<accesskit_winit::Event> for Event {
    fn from(event: accesskit_winit::Event) -> Self {
        Self::Accessibility(event)
    }
}

type WindowApplication = Application<Box<dyn Fn(AppEvent) + Send + Sync>>;

#[cfg(test)]
type CapturedEvents = Arc<std::sync::Mutex<Option<VecDeque<Event>>>>;

pub(crate) struct WindowHost {
    updates: updates::Updates,
    file_operation: Option<file_operations::Transaction>,
    source_save: Option<source_save::Publication>,
    delete_confirmation_suppressed: bool,
    delete_preference_path: Option<PathBuf>,
    windows: BTreeMap<WindowKey, WindowApplication>,
    proxy: Option<EventLoopProxy<Event>>,
    next_key: u64,
    pending_launches: Vec<towavue_runtime_windows::LaunchRequest>,
    preview_cache: PreviewCache,
    last_playback_volume: Arc<std::sync::Mutex<playback_volume::PlaybackVolume>>,
    playback_volume_preferences: Option<Arc<towavue_runtime_windows::PlaybackVolumePreferences>>,
    video_export_quality: Arc<std::sync::Mutex<towavue_runtime_windows::VideoExportQuality>>,
    idle_graphics: Option<idle_graphics::IdleGraphics>,
    tab_cursor_owner: Option<WindowKey>,
    tab_badge: Option<tab_drag::badge::Badge>,
    tab_badge_failed: bool,
    #[cfg(test)]
    captured_events: CapturedEvents,
}

impl WindowHost {
    pub(crate) fn new(
        initial_path: Option<PathBuf>,
        proxy: Option<EventLoopProxy<Event>>,
    ) -> Result<Self, Box<dyn Error>> {
        let delete_preference_path = crate::file_operations::preferences::path();
        let delete_confirmation_suppressed = delete_preference_path
            .as_deref()
            .is_some_and(crate::file_operations::preferences::suppressed);
        let (volume, playback_volume_preferences) =
            playback_volume::open_preferences(proxy.clone());
        let mut host = Self {
            updates: updates::Updates::default(),
            delete_confirmation_suppressed,
            delete_preference_path,
            file_operation: None,
            source_save: None,
            windows: BTreeMap::new(),
            proxy,
            next_key: 1,
            pending_launches: Vec::new(),
            preview_cache: PreviewCache::local()?,
            last_playback_volume: Arc::new(std::sync::Mutex::new(volume)),
            playback_volume_preferences,
            video_export_quality: Arc::default(),
            idle_graphics: None,
            tab_cursor_owner: None,
            tab_badge: None,
            tab_badge_failed: false,
            #[cfg(test)]
            captured_events: Arc::default(),
        };
        host.add_application(initial_path)?;
        Ok(host)
    }

    fn add_application(
        &mut self,
        initial_path: Option<PathBuf>,
    ) -> Result<WindowKey, Box<dyn Error>> {
        let key = WindowKey(self.next_key);
        self.next_key = self
            .next_key
            .checked_add(1)
            .expect("window identity exhausted");
        let proxy = self.proxy.clone();
        #[cfg(test)]
        let captured_events = self.captured_events.clone();
        let notify: Box<dyn Fn(AppEvent) + Send + Sync> = Box::new(move |event| {
            #[cfg(test)]
            if matches!(
                event,
                AppEvent::VideoExportQualityChanged
                    | AppEvent::Update(_)
                    | AppEvent::VideoResume(_)
                    | AppEvent::FolderReady
                    | AppEvent::FileOperationSource(..)
                    | AppEvent::FileOperationFinished(..)
                    | AppEvent::SourceSave(..)
                    | AppEvent::SourceSavePublished(..)
                    | AppEvent::SaveAs(..)
                    | AppEvent::SaveAsPublished(..)
                    | AppEvent::FileDeleteConfirmed(..)
            ) && let Some(queue) = captured_events.lock().expect("test events").as_mut()
            {
                queue.push_back(Event::Window(key, event));
                return;
            }
            if let Some(proxy) = &proxy {
                let _ = proxy.send_event(Event::Window(key, event));
            }
        });
        let mut app =
            Application::new_with_preview_cache(initial_path, notify, self.preview_cache.clone())?;
        app.last_playback_volume = Arc::clone(&self.last_playback_volume);
        app.playback_volume_preferences = self.playback_volume_preferences.clone();
        app.video_export_quality = Arc::clone(&self.video_export_quality);
        app.event_loop_proxy = self.proxy.clone();
        app.window_key = Some(key);
        app.hosted_graphics = true;
        self.windows.insert(key, app);
        Ok(key)
    }

    fn add_transfer_application(&mut self) -> Result<WindowKey, Box<dyn Error>> {
        let key = self.add_application(None)?;
        let app = self.windows.get_mut(&key).expect("new window");
        // This hidden destination will receive media or a transferred Gallery.
        // Do not manufacture an additional start tab while staging the handoff.
        app.tabs
            .take_gallery(app.tabs.gallery().expect("initial Gallery"));
        Ok(key)
    }

    fn start_pending(&mut self, event_loop: &ActiveEventLoop, visible: bool) {
        if self.updates.startup {
            return;
        }
        let mut device = self
            .windows
            .values()
            .find_map(|app| app.renderer.as_ref().map(FrameRenderer::graphics_device));
        for app in self
            .windows
            .values_mut()
            .filter(|app| app.window.is_none() && !app.exit_requested)
        {
            match app.start_on_device(event_loop, device.clone(), visible) {
                Ok(()) => {
                    device = app.renderer.as_ref().map(FrameRenderer::graphics_device);
                    if visible {
                        app.render_ready_launch_image();
                    }
                }
                Err(error) => {
                    towavue_runtime_windows::diagnostic!(
                        "towavue: could not create window: {error}"
                    );
                    app.exit_requested = true;
                }
            }
        }
    }

    fn native_window(&mut self, id: WindowId) -> Option<&mut WindowApplication> {
        self.windows.values_mut().find(|app| {
            !app.exit_requested && app.window.as_ref().is_some_and(|window| window.id() == id)
        })
    }

    fn open_pending_launches(&mut self, event_loop: &ActiveEventLoop, visible: bool) {
        if self.updates.startup
            || self.update_committing()
            || self.file_operation.is_some()
            || self.source_save.is_some()
        {
            return;
        }
        let local: Vec<_> = self
            .windows
            .iter_mut()
            .flat_map(|(key, app)| {
                std::mem::take(&mut app.pending_window_launches)
                    .into_iter()
                    .map(|path| (*key, path))
            })
            .collect();
        for (source, path) in local {
            let result = self.open_launched_window_with(Some(path), visible, |app, device| {
                app.start_on_device(event_loop, device, false)
                    .map_err(|error| error.to_string())
            });
            if let Err(error) = result
                && let Some(app) = self.windows.get_mut(&source)
            {
                app.set_status(format!("Could not open new window: {error}"));
            }
        }
        for request in std::mem::take(&mut self.pending_launches) {
            let result =
                self.open_launched_window_with(request.path.clone(), visible, |app, device| {
                    app.start_on_device(event_loop, device, false)
                        .map_err(|error| error.to_string())
                });
            if let Err(error) = &result {
                towavue_runtime_windows::diagnostic!(
                    "towavue: could not open launched window: {error}"
                );
            }
            request.acknowledge(result.is_ok());
        }
    }

    fn open_launched_window_with(
        &mut self,
        path: Option<PathBuf>,
        visible: bool,
        start: impl FnOnce(
            &mut WindowApplication,
            Option<towavue_runtime_windows::GraphicsDevice>,
        ) -> Result<(), String>,
    ) -> Result<WindowKey, String> {
        let device = self
            .windows
            .values()
            .find_map(|app| app.renderer.as_ref().map(FrameRenderer::graphics_device));
        let key = self
            .add_application(path)
            .map_err(|error| error.to_string())?;
        let app = self.windows.get_mut(&key).expect("new window");
        if let Err(error) = start(app, device) {
            let mut app = self.windows.remove(&key).expect("new window");
            if let Some(renderer) = app.renderer.take() {
                renderer.release_surface();
            }
            return Err(error);
        }
        let window = app.window.as_ref().expect("started window");
        #[cfg(feature = "presentation-verification")]
        let show_started = Instant::now();
        window.set_visible(visible);
        #[cfg(feature = "presentation-verification")]
        let shown = Instant::now();
        if visible {
            let _ = towavue_runtime_windows::activate_window(window.as_ref());
        }
        #[cfg(feature = "presentation-verification")]
        {
            app.image_launch_activation =
                Some((shown.duration_since(show_started), shown.elapsed()));
        }
        if visible {
            app.render_ready_launch_image();
        }
        Ok(key)
    }

    fn move_tab(
        &mut self,
        source: WindowKey,
        destination: WindowKey,
        request: &tab_transfer::DetachRequest,
        gap: usize,
    ) -> Result<TabId, String> {
        if source == destination {
            return Err("choose another window".into());
        }
        self.windows
            .get(&source)
            .ok_or("source window is closed")?
            .validate_tab_transfer(request)?;
        let target = self
            .windows
            .get(&destination)
            .ok_or("destination window is closed")?;
        target.validate_transfer_window()?;
        if gap > target.tabs.len() {
            return Err("the destination tab strip changed".into());
        }
        let stage = self.windows[&source].prepare_image_transfer(
            request.tab,
            target.ui_context.as_ref().expect("validated context"),
        )?;
        // All fallible validation is complete before extracting any user state.
        let transfer = self
            .windows
            .get_mut(&source)
            .expect("validated source")
            .take_tab_transfer(request, stage);
        Ok(self
            .windows
            .get_mut(&destination)
            .expect("validated destination")
            .accept_tab_transfer(transfer, gap))
    }

    fn detach_tab(
        &mut self,
        event_loop: &ActiveEventLoop,
        source: WindowKey,
        request: &tab_transfer::DetachRequest,
        visible: bool,
        client_position: winit::dpi::PhysicalPosition<i32>,
        anchor: egui::Vec2,
    ) -> Result<WindowKey, String> {
        self.detach_tab_with(source, request, visible, |app, device| {
            app.start_on_device(event_loop, Some(device), false)
                .map_err(|error| error.to_string())?;
            app.position_window_at_drop(client_position, anchor)
        })
    }

    fn detach_tab_with(
        &mut self,
        source: WindowKey,
        request: &tab_transfer::DetachRequest,
        visible: bool,
        start: impl FnOnce(
            &mut WindowApplication,
            towavue_runtime_windows::GraphicsDevice,
        ) -> Result<(), String>,
    ) -> Result<WindowKey, String> {
        let app = self.windows.get(&source).ok_or("source window is closed")?;
        app.validate_tab_transfer(request)?;
        let device = app
            .renderer
            .as_ref()
            .expect("validated renderer")
            .graphics_device();
        let destination = self
            .add_transfer_application()
            .map_err(|error| error.to_string())?;
        // Keep the empty HWND hidden until both startup and transfer succeed.
        let started = start(
            self.windows.get_mut(&destination).expect("new window"),
            device,
        );
        let moved = started.and_then(|()| self.move_tab(source, destination, request, 0));
        match moved {
            Ok(_) => {
                self.windows[&destination]
                    .window
                    .as_ref()
                    .expect("started window")
                    .set_visible(visible);
                Ok(destination)
            }
            Err(error) => {
                let mut app = self.windows.remove(&destination).expect("new window");
                if let Some(renderer) = app.renderer.take() {
                    renderer.release_surface();
                }
                Err(error)
            }
        }
    }

    fn route(&mut self, event: Event) {
        match event {
            Event::Launch(request) => {
                if self.update_committing() {
                    request.acknowledge(false);
                } else {
                    self.cancel_update("Update cancelled because another launch was requested.");
                    self.updates.startup = false;
                    self.pending_launches.push(request);
                }
            }
            Event::Update(epoch, event) => {
                if epoch == self.updates.worker_epoch {
                    self.update_event(event);
                }
            }
            Event::Window(origin, AppEvent::Update(action)) => self.update_choice(origin, action),
            Event::PlaybackVolumePreferenceFailed(error) => {
                if let Some(app) = self.windows.values_mut().find(|app| !app.exit_requested) {
                    app.set_status(format!("Could not save playback volume: {error}"));
                    app.request_redraw();
                }
            }
            Event::Window(origin, AppEvent::Playback(instance, event)) => {
                if let Some((owner, instance)) = self.playback_owner((origin, instance)) {
                    self.windows
                        .get_mut(&owner)
                        .expect("live playback owner")
                        .handle_app_event(AppEvent::Playback(instance, event));
                }
            }
            Event::Window(_, AppEvent::VideoExportQualityChanged) => {
                for app in self.windows.values_mut().filter(|app| !app.exit_requested) {
                    app.handle_app_event(AppEvent::VideoExportQualityChanged);
                }
            }
            Event::Window(_, AppEvent::ShortcutsChanged(bindings)) => {
                for app in self.windows.values_mut().filter(|app| !app.exit_requested) {
                    app.handle_app_event(AppEvent::ShortcutsChanged(bindings.clone()));
                }
            }
            Event::Window(key, AppEvent::FileDeleteConfirmed(serial, result)) => {
                self.finish_delete_confirmation(key, serial, result)
            }
            Event::Window(key, AppEvent::FileOperationFinished(serial, result)) => {
                self.finish_host_file_operation(key, serial, result)
            }
            Event::Window(key, AppEvent::SaveAsPublished(serial, result)) => {
                self.finish_save_as_publication(key, serial, result)
            }
            Event::Window(key, AppEvent::SourceSavePublished(serial, result)) => {
                self.finish_source_publication(key, serial, result)
            }
            Event::Window(key, event) => {
                if let Some(app) = self.windows.get_mut(&key).filter(|app| !app.exit_requested) {
                    app.handle_app_event(event);
                }
            }
            Event::Accessibility(event) => {
                if let Some(app) = self.native_window(event.window_id) {
                    app.handle_app_event(AppEvent::Accessibility(event));
                }
            }
        }
        self.clear_stale_update_guards();
        self.recover_pending_graphics();
    }

    fn open_pending_windows(&mut self, event_loop: &ActiveEventLoop, visible: bool) {
        self.open_pending_windows_with(event_loop, visible, Self::window_at_drop);
    }

    fn open_pending_windows_with(
        &mut self,
        event_loop: &ActiveEventLoop,
        visible: bool,
        pick: impl Fn(&Self, WindowKey, egui::Pos2) -> Option<(WindowKey, egui::Pos2)>,
    ) {
        let requests: Vec<_> = self
            .windows
            .iter_mut()
            .filter_map(|(key, app)| {
                app.pending_window_open
                    .take()
                    .map(|request| (*key, request))
            })
            .collect();
        for (source, request) in requests {
            let target = self
                .windows
                .get(&source)
                .and_then(|app| app.ui_context.as_ref())
                .filter(|context| context.content_rect().contains(request.point))
                .map(|_| (source, request.point))
                .or_else(|| pick(self, source, request.point));
            let result = if let Some((target, point)) = target {
                self.open_filmstrip_tab(source, &request, target, point)
                    .map(|opened| {
                        if opened
                            && visible
                            && let Some(window) = self
                                .windows
                                .get(&target)
                                .and_then(|app| app.window.as_ref())
                        {
                            let _ = towavue_runtime_windows::activate_window(window.as_ref());
                        }
                    })
            } else {
                self.open_filmstrip_window_with(source, &request, visible, |app, device| {
                    app.start_on_device(event_loop, Some(device), false)
                        .map_err(|error| error.to_string())
                })
                .map(|_| ())
            };
            if let Err(error) = result
                && let Some(app) = self.windows.get_mut(&source)
            {
                app.set_status(format!("Could not open dragged media: {error}"));
            }
        }
    }

    fn open_filmstrip_tab(
        &mut self,
        source: WindowKey,
        request: &window_open::Request,
        target: WindowKey,
        point: egui::Pos2,
    ) -> Result<bool, String> {
        let Some(app) = self
            .windows
            .get(&source)
            .filter(|app| app.window_open_request_is_current(request))
        else {
            return Ok(false);
        };
        app.validate_transfer_window()?;
        let source_tab = app.tabs.active().map(|tab| tab.id);
        let gap = self
            .windows
            .get(&target)
            .and_then(|app| app.incoming_gap(point))
            .ok_or("drop on an available window")?;
        let path = canonical_shell_path(&request.path).map_err(|error| error.to_string())?;
        if MediaKind::from_path(&path).is_none() {
            return Err(format!("Unsupported media: {}", path.display()));
        }
        let app = self.windows.get_mut(&target).expect("destination");
        let count = app.tabs.tabs().len();
        app.open_external(path, true);
        if app.tabs.tabs().len() == count {
            return Err("The media could not be opened".into());
        }
        app.tabs
            .reorder(app.tabs.active().expect("new tab").id, gap);
        app.request_redraw();
        let app = self.windows.get_mut(&source).expect("source");
        if request.is_gallery() {
            app.filmstrip.cancel_drag();
        } else if source == target {
            // Opening a local tab has already retained the source view. Close only its
            // saved overlay, without activating it again or copying its edits.
            if let Some(id) = source_tab {
                if let Some(saved) = app.retained_images.get_mut(&id) {
                    saved.filmstrip_open = false;
                }
                if let Some(saved) = app.retained_playback.get_mut(&id) {
                    saved.filmstrip_open = false;
                }
            }
        } else {
            app.close_filmstrip();
        }
        Ok(true)
    }

    fn open_filmstrip_window_with(
        &mut self,
        source: WindowKey,
        request: &window_open::Request,
        visible: bool,
        start: impl FnOnce(
            &mut WindowApplication,
            towavue_runtime_windows::GraphicsDevice,
        ) -> Result<(), String>,
    ) -> Result<Option<WindowKey>, String> {
        let Some(app) = self
            .windows
            .get(&source)
            .filter(|app| app.window_open_request_is_current(request))
        else {
            return Ok(None);
        };
        app.validate_transfer_window()?;
        let position = self.source_client_position(source, request.point)?;
        let path = canonical_shell_path(&request.path).map_err(|error| error.to_string())?;
        if MediaKind::from_path(&path).is_none() {
            return Err(format!("Unsupported media: {}", path.display()));
        }
        let device = app
            .renderer
            .as_ref()
            .expect("validated renderer")
            .graphics_device();
        let destination = self
            .add_transfer_application()
            .map_err(|error| error.to_string())?;
        let app = self.windows.get_mut(&destination).expect("new window");
        let opened = start(app, device).and_then(|()| {
            app.position_window_at_drop(position, request.anchor)?;
            app.open_external(path, true);
            if app.path.is_none() {
                return Err(app.status_message.as_ref().map_or_else(
                    || "The media could not be opened".into(),
                    |(text, _)| text.clone(),
                ));
            }
            Ok(())
        });
        if let Err(error) = opened {
            let mut app = self.windows.remove(&destination).expect("new window");
            if let Some(renderer) = app.renderer.take() {
                renderer.release_surface();
            }
            return Err(error);
        }
        // Window startup is acknowledged here. Image/decode errors remain in the
        // independent child, just as for ordinary Open; no source edits are copied.
        self.windows[&destination]
            .window
            .as_ref()
            .expect("started window")
            .set_visible(visible);
        let app = self.windows.get_mut(&source).expect("source window");
        if request.is_gallery() {
            app.filmstrip.cancel_drag();
        } else {
            app.close_filmstrip();
        }
        Ok(Some(destination))
    }

    fn playback_owner(&self, origin: (WindowKey, u64)) -> Option<(WindowKey, u64)> {
        // The session keeps its immutable callback origin through any number of
        // moves. Resolve against live owners, without forwarding chains or tombstones.
        self.windows
            .iter()
            .filter(|(_, app)| !app.exit_requested)
            .find_map(|(key, app)| {
                if app.playback_origin == Some(origin) {
                    Some((*key, app.media_generation))
                } else {
                    app.retained_playback
                        .values()
                        .find(|saved| saved.origin == Some(origin))
                        .map(|saved| (*key, saved.instance))
                }
            })
    }

    fn recover_pending_graphics(&mut self) {
        if self.file_operation.is_some() || self.source_save.is_some() {
            return;
        }
        self.recover_pending_graphics_with(|app, device| app.create_graphics_surface(device));
    }

    fn recover_pending_graphics_with(
        &mut self,
        mut create: impl FnMut(
            &WindowApplication,
            Option<towavue_runtime_windows::GraphicsDevice>,
        ) -> Result<FrameRenderer, String>,
    ) {
        let requests: BTreeMap<_, _> = self
            .windows
            .iter_mut()
            .filter_map(|(key, app)| {
                app.graphics_recovery_request
                    .take()
                    .map(|request| (*key, request))
            })
            .collect();
        if requests.is_empty() {
            return;
        }
        let lost = requests.values().any(|request| !request.retry)
            || self
                .windows
                .values()
                .filter_map(|app| app.renderer.as_ref())
                .any(|renderer| renderer.device_removed_reason().is_some());
        let targets: BTreeMap<_, _> = self
            .windows
            .iter()
            .filter(|(key, app)| {
                !app.exit_requested
                    && app.window.is_some()
                    && (requests.contains_key(key) || (lost && app.renderer.is_some()))
            })
            .map(|(key, app)| {
                (
                    *key,
                    requests
                        .get(key)
                        .copied()
                        .unwrap_or(GraphicsRecoveryRequest {
                            position: app.current_position(),
                            state: app.state,
                            retry: false,
                        }),
                )
            })
            .collect();
        if targets.is_empty() {
            return;
        }
        let mut device = if lost {
            None
        } else {
            self.windows
                .iter()
                .filter(|(key, app)| !targets.contains_key(key) && !app.exit_requested)
                .find_map(|(_, app)| app.renderer.as_ref().map(FrameRenderer::graphics_device))
        };
        // Quiesce every affected session before allocating any replacement surface.
        for key in targets.keys() {
            self.windows
                .get_mut(key)
                .expect("target window")
                .prepare_graphics_recovery();
        }
        let mut surfaces = BTreeMap::new();
        for key in targets.keys() {
            match create(&self.windows[key], device.clone()) {
                Ok(renderer) => {
                    device = Some(renderer.graphics_device());
                    surfaces.insert(*key, renderer);
                }
                Err(error) => {
                    // Never resume one window against a partially created device group.
                    for renderer in surfaces.into_values() {
                        renderer.release_surface();
                    }
                    for (key, point) in targets {
                        self.windows
                            .get_mut(&key)
                            .expect("target window")
                            .fail_graphics_recovery(point.position, point.state, error.clone());
                    }
                    return;
                }
            }
        }
        for (key, renderer) in surfaces {
            let point = targets[&key];
            self.windows
                .get_mut(&key)
                .expect("target window")
                .restore_graphics_surface(renderer, point.position, point.state);
        }
    }

    fn prepare_wait(&mut self) -> ControlFlow {
        self.clear_stale_update_guards();
        self.advance_update();
        self.advance_source_save();
        self.start_pending_file_operation();
        self.advance_file_operation();
        self.recover_pending_graphics();
        self.remove_closed();
        let mut wait = self.update_wait();
        for app in self.windows.values_mut() {
            wait = earliest_wait(wait, app.schedule());
        }
        self.recover_pending_graphics();
        self.remove_closed();
        if self.windows.is_empty() {
            exit_wait(
                towavue_runtime_windows::shell_workers_pending() || self.update_worker_pending(),
            )
        } else {
            earliest_wait(wait, self.trim_idle_graphics(Instant::now()))
        }
    }

    fn remove_closed(&mut self) {
        // Only the existing per-window close/save guard may approve this removal.
        // Release bound/deferred swap-chain references while the HWND is still owned.
        self.windows.retain(|_, app| {
            if app.exit_requested {
                if let Some(renderer) = app.renderer.take() {
                    renderer.release_surface();
                }
                false
            } else {
                true
            }
        });
    }
}

// Keep winit's main STA alive while detached Shell workers retire. Poll only
// during final shutdown, without blocking UI messages or unrelated windows.
fn exit_wait(shell_pending: bool) -> ControlFlow {
    if shell_pending {
        ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(10))
    } else {
        ControlFlow::Poll
    }
}

fn earliest_wait(first: ControlFlow, second: ControlFlow) -> ControlFlow {
    match (first, second) {
        (ControlFlow::Poll, _) | (_, ControlFlow::Poll) => ControlFlow::Poll,
        (ControlFlow::WaitUntil(a), ControlFlow::WaitUntil(b)) => ControlFlow::WaitUntil(a.min(b)),
        (ControlFlow::Wait, other) | (other, ControlFlow::Wait) => other,
    }
}

impl ApplicationHandler<Event> for WindowHost {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.start_pending(event_loop, true);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Event) {
        self.route(event);
        self.start_pending(event_loop, true);
        self.open_pending_launches(event_loop, true);
        self.open_pending_windows(event_loop, true);
        self.update_tab_drops(event_loop, true);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if let Some(app) = self.native_window(window_id) {
            app.window_event(event_loop, window_id, event);
        }
        self.recover_pending_graphics();
        self.open_pending_windows(event_loop, true);
        self.update_tab_drops(event_loop, true);
    }

    fn device_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        id: winit::event::DeviceId,
        event: winit::event::DeviceEvent,
    ) {
        // Each window cancels stale raw drag on focus loss and accepts motion only
        // while focused. Deliver to all so a nonfocused window also releases capture.
        for app in self.windows.values_mut().filter(|app| !app.exit_requested) {
            app.device_event(event_loop, id, event.clone());
        }
        self.recover_pending_graphics();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.start_pending(event_loop, true);
        self.open_pending_launches(event_loop, true);
        self.open_pending_windows(event_loop, true);
        self.update_tab_drops(event_loop, true);
        event_loop.set_control_flow(self.prepare_wait());
        if self.windows.is_empty()
            && !towavue_runtime_windows::shell_workers_pending()
            && !self.update_worker_pending()
        {
            // A worker can finish between prepare_wait and this check. Windows
            // winit still waits once after AboutToWait, so clear any old deadline.
            event_loop.set_control_flow(ControlFlow::Poll);
            event_loop.exit();
        }
    }
}
