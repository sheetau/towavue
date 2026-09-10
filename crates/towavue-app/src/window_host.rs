use super::*;

#[cfg(test)]
#[path = "window_host_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "window_graphics_tests.rs"]
mod graphics_tests;

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
    Window(WindowKey, AppEvent),
    Accessibility(accesskit_winit::Event),
}

impl From<accesskit_winit::Event> for Event {
    fn from(event: accesskit_winit::Event) -> Self {
        Self::Accessibility(event)
    }
}

type WindowApplication = Application<Box<dyn Fn(AppEvent) + Send + Sync>>;

pub(crate) struct WindowHost {
    windows: BTreeMap<WindowKey, WindowApplication>,
    proxy: Option<EventLoopProxy<Event>>,
    next_key: u64,
}

impl WindowHost {
    pub(crate) fn new(
        initial_path: Option<PathBuf>,
        proxy: Option<EventLoopProxy<Event>>,
    ) -> Result<Self, Box<dyn Error>> {
        let mut host = Self {
            windows: BTreeMap::new(),
            proxy,
            next_key: 1,
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
        let notify: Box<dyn Fn(AppEvent) + Send + Sync> = Box::new(move |event| {
            if let Some(proxy) = &proxy {
                let _ = proxy.send_event(Event::Window(key, event));
            }
        });
        let mut app = Application::new(initial_path, notify)?;
        app.event_loop_proxy = self.proxy.clone();
        app.hosted_graphics = true;
        self.windows.insert(key, app);
        Ok(key)
    }

    fn start_pending(&mut self, event_loop: &ActiveEventLoop, visible: bool) {
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
                }
                Err(error) => {
                    eprintln!("towavue: could not create window: {error}");
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

    fn route(&mut self, event: Event) {
        match event {
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
        self.recover_pending_graphics();
    }

    fn recover_pending_graphics(&mut self) {
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
        self.recover_pending_graphics();
        self.remove_closed();
        let mut wait = ControlFlow::Wait;
        for app in self.windows.values_mut() {
            wait = earliest_wait(wait, app.schedule());
        }
        self.recover_pending_graphics();
        self.remove_closed();
        if self.windows.is_empty() {
            ControlFlow::Poll
        } else {
            wait
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

    fn user_event(&mut self, _: &ActiveEventLoop, event: Event) {
        self.route(event);
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
        event_loop.set_control_flow(self.prepare_wait());
        if self.windows.is_empty() {
            // Windows winit waits once after AboutToWait; prepare_wait selects Poll.
            event_loop.exit();
        }
    }
}
