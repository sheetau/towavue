use super::*;
use std::hash::BuildHasher;
use towavue_core::{AudioQueue, RepeatMode};

pub(super) struct AudioTab {
    pub order: AudioQueue,
    folder: PathBuf,
    provider: Option<FolderOrderProvider>,
    snapshot: Option<FolderSnapshot>,
    handled_eof: Option<u64>,
    requested_eof: Option<(u64, PlaybackGeneration)>,
    refreshing: bool,
}

impl AudioTab {
    pub(super) fn transfer(
        &mut self,
        old: u64,
        new: u64,
        notify: impl Fn() + Send + Sync + 'static,
    ) {
        self.handled_eof = self
            .handled_eof
            .filter(|instance| *instance == old)
            .map(|_| new);
        self.requested_eof = self
            .requested_eof
            .filter(|(instance, _)| *instance == old)
            .map(|(_, generation)| (new, generation));
        // Folder completion must wake the new owner even after the old window closes.
        self.provider = match FolderOrderProvider::with_notify(notify) {
            Ok(provider) => {
                provider.request(Some(self.folder.clone()));
                Some(provider)
            }
            Err(error) => {
                eprintln!("towavue: audio order unavailable after tab transfer: {error}");
                None
            }
        };
        self.refreshing = self.provider.is_some();
    }

    fn accept_snapshot(&mut self, snapshot: FolderSnapshot) {
        if snapshot.folder_path == self.folder {
            self.order.set_items(
                snapshot
                    .items_of_kind(MediaKind::Audio)
                    .map(|item| item.path.clone())
                    .collect(),
            );
            self.snapshot = Some(snapshot);
        }
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn draw_audio_mode_buttons(&self, ui: &mut egui::Ui, actions: &mut Vec<UiAction>) {
        let (repeat, shuffled) = self.audio_mode();
        let (icon, label) = match repeat {
            RepeatMode::Off => (chrome::AudioIcon::Repeat, "Repeat off"),
            RepeatMode::All => (chrome::AudioIcon::Repeat, "Repeat all"),
            RepeatMode::One => (chrome::AudioIcon::RepeatOne, "Repeat one"),
        };
        if chrome::audio_button(
            ui,
            icon,
            repeat != RepeatMode::Off,
            &self.command_hint(CommandId::CycleAudioRepeat, label),
        )
        .clicked()
        {
            actions.push(UiAction::Command(CommandId::CycleAudioRepeat));
        }
        if chrome::audio_button(
            ui,
            chrome::AudioIcon::Shuffle,
            shuffled,
            &self.command_hint(
                CommandId::ToggleAudioShuffle,
                if shuffled {
                    "Shuffle on"
                } else {
                    "Shuffle off"
                },
            ),
        )
        .clicked()
        {
            actions.push(UiAction::Command(CommandId::ToggleAudioShuffle));
        }
    }

    pub(super) fn ensure_audio_queue(&mut self) {
        let Some(id) = self.tabs.active().map(|tab| tab.id) else {
            return;
        };
        if self.media_kind != Some(MediaKind::Audio) {
            self.audio_queues.remove(&id);
            return;
        }
        let Some(folder) = self
            .path
            .as_ref()
            .and_then(|path| path.parent())
            .map(Path::to_owned)
        else {
            return;
        };
        if self
            .audio_queues
            .get(&id)
            .is_some_and(|queue| queue.folder == folder)
        {
            return;
        }
        let notify = Arc::clone(&self.notify);
        let provider = match FolderOrderProvider::with_notify(move || notify(AppEvent::FolderReady))
        {
            Ok(provider) => {
                provider.request(Some(folder.clone()));
                Some(provider)
            }
            Err(error) => {
                self.set_status(format!("Audio order unavailable: {error}"));
                None
            }
        };
        let mut queue = AudioTab {
            order: AudioQueue::default(),
            folder,
            provider,
            snapshot: None,
            handled_eof: None,
            requested_eof: None,
            refreshing: false,
        };
        if let Some(snapshot) = &self.folder_snapshot {
            queue.accept_snapshot(snapshot.clone());
        }
        self.audio_queues.insert(id, queue);
    }

    pub(super) fn finish_audio_folder_loads(&mut self) {
        for queue in self.audio_queues.values_mut() {
            if let Some(snapshot) = queue
                .provider
                .as_ref()
                .and_then(FolderOrderProvider::take_completed)
            {
                queue.accept_snapshot(snapshot);
                queue.refreshing = false;
            }
        }
    }

    pub(super) fn sync_audio_snapshot(&mut self, snapshot: &FolderSnapshot) {
        if let Some(queue) = self
            .tabs
            .active()
            .and_then(|tab| self.audio_queues.get_mut(&tab.id))
        {
            queue.accept_snapshot(snapshot.clone());
        }
    }

    pub(super) fn audio_mode(&self) -> (RepeatMode, bool) {
        self.tabs
            .active()
            .and_then(|tab| self.audio_queues.get(&tab.id))
            .map_or((RepeatMode::Off, false), |queue| {
                (queue.order.repeat(), queue.order.shuffled())
            })
    }

    pub(super) fn change_audio_mode(&mut self, shuffle: bool) {
        self.ensure_audio_queue();
        if let Some(queue) = self
            .tabs
            .active()
            .and_then(|tab| self.audio_queues.get_mut(&tab.id))
        {
            if shuffle {
                let seed = std::collections::hash_map::RandomState::new().hash_one(Instant::now());
                if let Some(path) = &self.path {
                    queue.order.toggle_shuffle(path, seed);
                }
            } else {
                queue.order.cycle_repeat();
            }
        }
        self.request_redraw();
    }

    pub(super) fn navigate_audio(&mut self, forward: bool) {
        let Some(queue) = self
            .tabs
            .active()
            .and_then(|tab| self.audio_queues.get(&tab.id))
        else {
            return;
        };
        let Some(current) = &self.path else { return };
        let next = if forward {
            queue.order.next(current, false)
        } else {
            queue.order.previous(current)
        };
        if let Some(path) = next {
            if &path == current {
                self.state = PlaybackState::Ended;
                self.toggle_pause();
            } else {
                self.request_guarded(GuardedAction::Navigate(path));
            }
        }
    }

    pub(super) fn suppress_paused_audio_eof(&mut self) {
        if let Some(queue) = self
            .tabs
            .active()
            .and_then(|tab| self.audio_queues.get_mut(&tab.id))
        {
            queue.handled_eof = Some(self.media_generation);
        }
    }

    pub(super) fn arm_audio_queue(&mut self, id: TabId) {
        if let Some(queue) = self.audio_queues.get_mut(&id) {
            queue.handled_eof = None;
            queue.requested_eof = None;
        }
    }

    pub(super) fn advance_audio_queues(&mut self) {
        let active = self.tabs.active().map(|tab| tab.id);
        let ids: Vec<_> = self.audio_queues.keys().copied().collect();
        for id in ids {
            let selected = if active == Some(id) {
                self.playback_selection.is_some()
            } else {
                self.retained_playback
                    .get(&id)
                    .is_some_and(|saved| saved.playback_selection.is_some())
            };
            let (state, instance, generation, path) = if active == Some(id) {
                if self.media_kind != Some(MediaKind::Audio) || self.modal_input_blocked() {
                    continue;
                }
                (
                    self.state,
                    self.media_generation,
                    self.generation,
                    self.path.clone(),
                )
            } else if let Some(saved) = self.retained_playback.get(&id) {
                (
                    saved.state,
                    saved.instance,
                    saved
                        .session
                        .as_ref()
                        .map_or(PlaybackGeneration::INITIAL, PlaybackSession::generation),
                    Some(saved.path.clone()),
                )
            } else {
                continue;
            };
            if state == PlaybackState::Playing {
                let queue = self.audio_queues.get_mut(&id).expect("audio queue");
                queue.handled_eof = None;
                queue.requested_eof = None;
            }
            if state != PlaybackState::Ended {
                continue;
            }
            let Some(path) = path else { continue };
            let queue = self.audio_queues.get_mut(&id).expect("audio queue");
            // Device replacement alone must not restart an already stopped queue.
            if queue.handled_eof == Some(instance) {
                continue;
            }
            if selected {
                queue.handled_eof = Some(instance);
                if queue.order.repeat() != RepeatMode::Off {
                    if active == Some(id) {
                        self.toggle_pause();
                    } else {
                        self.advance_background_audio(id, path);
                    }
                }
                continue;
            }
            if queue.order.repeat() != RepeatMode::One {
                if queue.requested_eof != Some((instance, generation)) {
                    queue.requested_eof = Some((instance, generation));
                    if let Some(provider) = &queue.provider {
                        provider.request(Some(queue.folder.clone()));
                        queue.refreshing = true;
                    }
                }
                if queue.refreshing {
                    continue;
                }
            }
            if queue.snapshot.is_none() && queue.order.repeat() != RepeatMode::One {
                continue;
            }
            queue.handled_eof = Some(instance);
            let Some(next) = queue.order.next(&path, true) else {
                continue;
            };
            if next != path
                && (self.edits.get(&id).is_some_and(EditHistory::is_dirty)
                    || self
                        .active_export
                        .as_ref()
                        .is_some_and(|export| export.tab == id))
            {
                let message =
                    "Automatic next track stopped to preserve edits or an active export".to_owned();
                if active == Some(id) {
                    self.set_status(message);
                } else if let Some(saved) = self.retained_playback.get_mut(&id) {
                    saved.status = Some((message, Instant::now()));
                }
                continue;
            }
            if active == Some(id) {
                if next == path {
                    self.toggle_pause();
                } else {
                    self.navigate_to_unchecked(next);
                }
            } else {
                self.advance_background_audio(id, next);
            }
        }
    }

    fn advance_background_audio(&mut self, id: TabId, path: PathBuf) {
        let Some(mut saved) = self.retained_playback.remove(&id) else {
            return;
        };
        if saved.path == path {
            saved.restart();
        } else if let Some(renderer) = &self.renderer {
            saved.session.take();
            self.duration_workers.remove(&saved.instance);
            self.media_sequence = self
                .media_sequence
                .max(self.media_generation)
                .wrapping_add(1);
            saved.instance = self.media_sequence;
            saved.path = path.clone();
            saved.clock = None;
            saved.duration = None;
            saved.time_selection = None;
            saved.playback_selection = None;
            saved.waveform = None;
            saved.pending_time = None;
            saved.audio_drained = true;
            saved.decode_finished = false;
            saved.error = None;
            saved.status = None;
            saved.view = ImageViewState::default();
            saved.playlist.clear();
            saved.metrics_recorded = false;
            saved.seek_latencies.clear();
            saved.drift_samples.clear();
            saved.folder_snapshot = self
                .audio_queues
                .get(&id)
                .and_then(|queue| queue.snapshot.clone());
            if let Some(tab) = self.tabs.get_mut(id) {
                tab.target.set_current_path(path.clone(), MediaKind::Audio);
            }
            self.edits.insert(id, EditHistory::default());
            self.export_paths.remove(&id);
            self.audio_export_settings.remove(&id);
            self.metadata_export_settings.remove(&id);
            let instance = saved.instance;
            saved.origin = self.window_key.map(|key| (key, instance));
            let notify = Arc::clone(&self.notify);
            match PlaybackSession::open(
                &path,
                renderer.graphics_device(),
                1.0,
                1.0,
                Default::default(),
                move |event| notify(AppEvent::Playback(instance, event)),
            ) {
                Ok(session) => {
                    saved.audio_drained = !session.has_audio();
                    saved.session = Some(session);
                    saved.state = PlaybackState::Playing;
                }
                Err(error) => saved.fail(error.to_string()),
            }
            self.load_duration_for(path.clone(), instance);
            if let Some(recent) = &self.recent_files {
                recent.record(path);
            }
        }
        if saved.state == PlaybackState::Playing {
            self.arm_audio_queue(id);
        }
        self.retained_playback.insert(id, saved);
        self.request_redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use winit::platform::windows::EventLoopBuilderExtWindows;
    use winit::window::WindowAttributes;

    #[test]
    fn transferred_audio_queue_rebinds_wakeup_and_remaps_eof_identity() {
        let Some(root) = crate::tests::isolated_test_root(
            "audio_playback::tests::transferred_audio_queue_rebinds_wakeup_and_remaps_eof_identity",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("app");
        let path = root.join("routing-only.wav");
        let id = app.tabs.open_new(path.clone(), MediaKind::Audio);
        app.path = Some(path);
        app.media_kind = Some(MediaKind::Audio);
        app.ensure_audio_queue();
        let queue = app.audio_queues.get_mut(&id).expect("queue");
        queue.order.cycle_repeat();
        queue.handled_eof = Some(7);
        queue.requested_eof = Some((7, PlaybackGeneration::INITIAL));
        let (sent, received) = mpsc::channel();
        queue.transfer(7, 19, move || {
            let _ = sent.send(());
        });
        assert_eq!(queue.handled_eof, Some(19));
        assert_eq!(queue.requested_eof, Some((19, PlaybackGeneration::INITIAL)));
        assert_eq!(queue.order.repeat(), RepeatMode::All);
        received
            .recv_timeout(Duration::from_secs(5))
            .expect("new owner's folder wakeup");
        assert!(
            queue
                .provider
                .as_ref()
                .expect("rebound provider")
                .take_completed()
                .is_some()
        );
        queue.transfer(20, 31, || {});
        assert_eq!(queue.handled_eof, None);
        assert_eq!(queue.requested_eof, None);
    }

    fn advance<N: Fn(AppEvent) + Send + Sync + 'static>(
        app: &mut Application<N>,
        events: &mpsc::Receiver<AppEvent>,
    ) {
        app.advance_audio_queues();
        wait(app, events, |app| {
            app.audio_queues.values().all(|queue| !queue.refreshing)
        });
        app.advance_audio_queues();
    }

    fn wait<N: Fn(AppEvent) + Send + Sync + 'static>(
        app: &mut Application<N>,
        events: &mpsc::Receiver<AppEvent>,
        done: impl Fn(&Application<N>) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            for event in events.try_iter() {
                app.handle_app_event(event);
            }
            app.poll_audio();
            app.load_next_frame();
            app.check_eof();
            for saved in app.retained_playback.values_mut() {
                saved.poll();
            }
            if done(app) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "audio queue deadline: {:?} {:?}",
                app.state,
                app.playback_error
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn audio_modes_are_tab_local_non_editing_and_disabled_for_visual_media() {
        let Some(root) = crate::tests::isolated_test_root(
            "audio_playback::tests::audio_modes_are_tab_local_non_editing_and_disabled_for_visual_media",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("app");
        let first = app.tabs.open_new(root.join("first.wav"), MediaKind::Audio);
        app.path = Some(root.join("first.wav"));
        app.media_kind = Some(MediaKind::Audio);
        app.ensure_audio_queue();
        app.dispatch(CommandId::CycleAudioRepeat);
        app.dispatch(CommandId::ToggleAudioShuffle);
        assert_eq!(app.audio_mode(), (RepeatMode::All, true));
        assert!(!app.command_context().has_unsaved_edits);
        let second = app.tabs.open_new(root.join("second.wav"), MediaKind::Audio);
        app.path = Some(root.join("second.wav"));
        app.ensure_audio_queue();
        assert_eq!(app.audio_mode(), (RepeatMode::Off, false));
        app.tabs.activate(first);
        assert_eq!(app.audio_mode(), (RepeatMode::All, true));
        for kind in [MediaKind::Image, MediaKind::Video] {
            app.media_kind = Some(kind);
            app.dispatch(CommandId::CycleAudioRepeat);
            app.dispatch(CommandId::ToggleAudioShuffle);
            assert_eq!(app.audio_mode(), (RepeatMode::All, true));
        }
        app.media_kind = Some(MediaKind::Audio);
        app.audio_queues
            .get_mut(&first)
            .expect("first queue")
            .order
            .cycle_repeat();
        app.state = PlaybackState::Ended;
        app.suppress_paused_audio_eof();
        app.generation = app.generation.next();
        app.advance_audio_queues();
        assert_eq!(
            app.state,
            PlaybackState::Ended,
            "device generation alone does not restart stopped playback"
        );
        app.state = PlaybackState::Playing;
        app.advance_audio_queues();
        assert!(
            app.audio_queues[&first].handled_eof.is_none(),
            "explicit replay enables the next natural EOF"
        );
        app.remove_tab(second, false);
        assert!(!app.audio_queues.contains_key(&second));
    }

    #[test]
    fn audio_buttons_fit_small_windows_and_accessibility_invokes_shared_commands() {
        let Some(root) = crate::tests::isolated_test_root(
            "audio_playback::tests::audio_buttons_fit_small_windows_and_accessibility_invokes_shared_commands",
        ) else {
            return;
        };
        for width in [240.0, 480.0, 960.0] {
            let mut app = Application::new(None, |_| {}).expect("app");
            let path = root.join("audio.wav");
            app.tabs.open_new(path.clone(), MediaKind::Audio);
            app.path = Some(path);
            app.media_kind = Some(MediaKind::Audio);
            app.ensure_audio_queue();
            let mut clock = PlaybackClock::new(media_time(Duration::from_secs(359999)), 1.0);
            clock.set_paused(true);
            app.clock = Some(clock);
            let context = fonts::test_context();
            context.enable_accesskit();
            let frame = |app: &mut Application<_>, events| {
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
                    |ui| {
                        app.draw_status_bar(ui, &mut actions, &mut Vec::new());
                    },
                );
                (output, actions)
            };
            for (label, command) in [
                ("Repeat off", CommandId::CycleAudioRepeat),
                ("Shuffle off", CommandId::ToggleAudioShuffle),
            ] {
                frame(&mut app, vec![]);
                let output = frame(&mut app, vec![]).0;
                let update = output
                    .platform_output
                    .accesskit_update
                    .expect("accessibility tree");
                let (id, node) = update
                    .nodes
                    .iter()
                    .find(|(_, node)| node.label().is_some_and(|name| name.starts_with(label)))
                    .expect("audio mode button");
                let bounds = node.bounds().expect("button bounds");
                assert!(
                    bounds.x0 >= 0.0 && bounds.x1 <= width as f64,
                    "{label} outside {width}: {bounds:?}"
                );
                let actions = frame(
                    &mut app,
                    vec![egui::Event::AccessKitActionRequest(
                        egui::accesskit::ActionRequest {
                            action: egui::accesskit::Action::Click,
                            target_tree: egui::accesskit::TreeId::ROOT,
                            target_node: *id,
                            data: None,
                        },
                    )],
                )
                .1;
                assert!(matches!(actions.as_slice(), [UiAction::Command(id)] if *id == command));
                app.dispatch(command);
            }
            assert_eq!(app.audio_mode(), (RepeatMode::All, true));
            assert!(!app.command_context().has_unsaved_edits);
            app.media_kind = Some(MediaKind::Video);
            context.global_style_mut(chrome::style);
            for density in [1.0, 1.25, 2.0] {
                context.set_pixels_per_point(density);
                for (label, repeated) in [("Video repeat off", true), ("Video repeat on", false)] {
                    frame(&mut app, vec![]);
                    let output = frame(&mut app, vec![]).0;
                    let update = output.platform_output.accesskit_update.expect("video UIA");
                    let (id, node) = update
                        .nodes
                        .iter()
                        .find(|(_, node)| node.label().is_some_and(|name| name.starts_with(label)))
                        .expect("video repeat button");
                    let bounds = node.bounds().expect("repeat bounds");
                    assert!(bounds.x0 >= 0.0 && bounds.x1 <= f64::from(width));
                    assert!(!node.is_disabled());
                    let (_, actions) = frame(
                        &mut app,
                        vec![egui::Event::AccessKitActionRequest(
                            egui::accesskit::ActionRequest {
                                action: egui::accesskit::Action::Click,
                                target_tree: egui::accesskit::TreeId::ROOT,
                                target_node: *id,
                                data: None,
                            },
                        )],
                    );
                    assert!(matches!(
                        actions.as_slice(),
                        [UiAction::Command(CommandId::ToggleVideoRepeat)]
                    ));
                    for action in actions {
                        app.handle_ui_action(action);
                    }
                    assert_eq!(app.video_repeat, repeated);
                    assert!(!app.command_context().has_unsaved_edits);
                }
            }
            for kind in [MediaKind::Image, MediaKind::Audio] {
                app.media_kind = Some(kind);
                app.dispatch(CommandId::ToggleVideoRepeat);
                assert!(
                    !app.video_repeat,
                    "video command is unavailable for other media"
                );
            }
        }
    }

    #[test]
    #[ignore = "requires Windows D3D11 and a shared WASAPI endpoint; only generated silence is played"]
    fn audio_queue_advances_and_repeats_without_activating_background_tabs() {
        let Some(root) = crate::tests::isolated_test_root(
            "audio_playback::tests::audio_queue_advances_and_repeats_without_activating_background_tabs",
        ) else {
            return;
        };
        let ffmpeg = PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg"))
            .join("bin/ffmpeg.exe");
        assert!(
            std::process::Command::new(&ffmpeg)
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "anullsrc=r=48000:cl=stereo",
                    "-t",
                    "0.15",
                    "-c:a",
                    "pcm_s16le"
                ])
                .arg(root.join("01.wav"))
                .status()
                .expect("silence")
                .success()
        );
        for name in ["02.wav", "03.wav"] {
            std::fs::copy(root.join("01.wav"), root.join(name)).expect("copy owned silence");
        }
        assert!(
            std::process::Command::new(&ffmpeg)
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "color=red:s=16x16",
                    "-frames:v",
                    "1"
                ])
                .arg(root.join("image.bmp"))
                .status()
                .expect("image")
                .success()
        );
        struct Trial(PathBuf);
        impl ApplicationHandler for Trial {
            fn resumed(&mut self, event_loop: &ActiveEventLoop) {
                let window = Arc::new(
                    event_loop
                        .create_window(WindowAttributes::default().with_visible(false))
                        .expect("owned hidden window"),
                );
                let renderer = match FrameRenderer::new(window.as_ref()) {
                    Ok(renderer) => renderer,
                    Err(error) => {
                        eprintln!("SKIP audio queue: D3D11 unavailable: {error}");
                        event_loop.exit();
                        return;
                    }
                };
                let (sender, events) = mpsc::channel();
                let mut app = Application::new(None, move |event| {
                    let _ = sender.send(event);
                })
                .expect("app");
                app.window = Some(window);
                app.renderer = Some(renderer);
                app.ui_context = Some(fonts::test_context());
                app.open_external(self.0.join("01.wav"), true);
                if app
                    .playback_error
                    .as_ref()
                    .is_some_and(|error| error.starts_with("audio output failed:"))
                {
                    eprintln!(
                        "SKIP audio queue: {}",
                        app.playback_error.as_ref().expect("audio error")
                    );
                    event_loop.exit();
                    return;
                }
                let audio = app.tabs.active().expect("audio tab").id;
                wait(&mut app, &events, |app| {
                    app.state == PlaybackState::Ended && app.audio_queues[&audio].snapshot.is_some()
                });
                let paths: Vec<_> = app.audio_queues[&audio]
                    .snapshot
                    .as_ref()
                    .expect("Shell snapshot")
                    .items_of_kind(MediaKind::Audio)
                    .map(|item| item.path.clone())
                    .collect();
                assert_eq!(paths.len(), 3);
                wait(&mut app, &events, |app| app.media_duration.is_some());
                let audition_path = app.path.clone();
                let audition_instance = app.media_generation;
                let selected = towavue_core::TimeRange::new(
                    media_time(Duration::from_millis(20)),
                    media_time(Duration::from_millis(80)),
                )
                .expect("audition range");
                for (ms, inside) in [
                    (0, false),
                    (20, true),
                    (40, true),
                    (80, false),
                    (100, false),
                ] {
                    app.set_time_selection(None);
                    app.seek_to(media_time(Duration::from_millis(ms)));
                    app.set_time_selection(Some(selected));
                    app.toggle_pause();
                    assert_eq!(app.state, PlaybackState::Playing);
                    assert_eq!(app.playback_selection, inside.then_some(selected));
                    assert_eq!(
                        app.session.as_ref().expect("session").range_end(),
                        inside.then_some(selected.end())
                    );
                    app.toggle_pause();
                }
                app.set_time_selection(Some(selected));
                for repeat in [RepeatMode::Off, RepeatMode::All, RepeatMode::One] {
                    assert_eq!(app.audio_mode().0, repeat);
                    app.process_shortcut("Shift+Space".parse().expect("play selected time"));
                    wait(&mut app, &events, |app| app.state == PlaybackState::Ended);
                    let generation = app.generation;
                    advance(&mut app, &events);
                    assert_eq!(
                        app.path, audition_path,
                        "selection EOF must not advance to another file"
                    );
                    assert_eq!(app.media_generation, audition_instance);
                    if repeat == RepeatMode::Off {
                        assert_eq!(app.state, PlaybackState::Ended);
                        assert_eq!(app.current_position(), selected.end());
                        assert_eq!(app.generation, generation);
                    } else {
                        assert_eq!(app.state, PlaybackState::Playing);
                        assert_ne!(app.generation, generation);
                        assert_eq!(
                            app.session.as_ref().expect("session").target(),
                            selected.start()
                        );
                        wait(&mut app, &events, |app| app.state == PlaybackState::Ended);
                    }
                    app.dispatch(CommandId::CycleAudioRepeat);
                }
                app.dispatch(CommandId::CycleAudioRepeat);
                app.play_time_selection();
                app.open_external(self.0.join("image.bmp"), true);
                let audition_image = app.tabs.active().expect("image").id;
                wait(&mut app, &events, |app| {
                    app.retained_playback[&audio].state == PlaybackState::Ended
                });
                for _ in 0..2 {
                    let generation = app.retained_playback[&audio]
                        .session
                        .as_ref()
                        .expect("session")
                        .generation();
                    advance(&mut app, &events);
                    let saved = &app.retained_playback[&audio];
                    assert_eq!(saved.path, audition_path.clone().expect("audio path"));
                    assert_eq!(saved.state, PlaybackState::Playing);
                    assert_ne!(
                        saved.session.as_ref().expect("session").generation(),
                        generation
                    );
                    assert_eq!(
                        saved.session.as_ref().expect("session").target(),
                        selected.start()
                    );
                    assert_eq!(
                        app.tabs.active().expect("image remains active").id,
                        audition_image
                    );
                    // Arm the next EOF while the background session is playing.
                    app.advance_audio_queues();
                    wait(&mut app, &events, |app| {
                        app.retained_playback[&audio].state == PlaybackState::Ended
                    });
                }
                app.activate_tab(audio);
                app.dispatch(CommandId::CycleAudioRepeat);
                app.dispatch(CommandId::CycleAudioRepeat);
                app.process_shortcut("Escape".parse().expect("leave audition"));
                assert!(app.playback_selection.is_none());
                assert!(!app.command_context().has_unsaved_edits);
                app.request_guarded(GuardedAction::CloseTab(audition_image));
                app.navigate_to_unchecked(paths[0].clone());
                wait(&mut app, &events, |app| app.state == PlaybackState::Ended);
                let old_instance = app.media_generation;
                let old_generation = app.generation;
                let mut metadata = MetadataExportOptions::default();
                metadata
                    .set(
                        towavue_runtime_windows::MetadataField::Title,
                        Some("Previous track".into()),
                    )
                    .expect("metadata setting");
                app.metadata_export_settings.insert(audio, metadata.clone());
                app.audio_export_settings.insert(
                    audio,
                    AudioExportOptions {
                        normalize_peak: true,
                        ..Default::default()
                    },
                );
                advance(&mut app, &events);
                assert_eq!(app.path.as_ref(), Some(&paths[1]));
                assert!(!app.audio_export_settings.contains_key(&audio));
                assert!(!app.metadata_export_settings.contains_key(&audio));
                assert_eq!(app.tabs.active().expect("same tab").id, audio);
                assert!(app.media_generation > old_instance);
                app.handle_app_event(AppEvent::Playback(
                    old_instance,
                    PlaybackEvent::Failed(old_generation, "stale EOF track".into()),
                ));
                assert!(app.playback_error.is_none());
                wait(&mut app, &events, |app| app.state == PlaybackState::Ended);
                app.dispatch(CommandId::CycleAudioRepeat);
                app.dispatch(CommandId::CycleAudioRepeat);
                let generation = app.generation;
                app.advance_audio_queues();
                assert!(
                    app.audio_queues[&audio].handled_eof.is_none(),
                    "repeat is armed before the next scheduling tick"
                );
                assert_eq!(app.path.as_ref(), Some(&paths[1]));
                assert_ne!(app.generation, generation);
                assert_eq!(app.state, PlaybackState::Playing);
                wait(&mut app, &events, |app| app.state == PlaybackState::Ended);
                app.state = PlaybackState::Paused;
                app.check_eof();
                let generation = app.generation;
                advance(&mut app, &events);
                assert_eq!(
                    app.generation, generation,
                    "paused EOF is not automatic playback"
                );
                app.dispatch(CommandId::CycleAudioRepeat);
                app.navigate_to_unchecked(paths[0].clone());
                wait(&mut app, &events, |app| app.state == PlaybackState::Ended);
                app.edits
                    .entry(audio)
                    .or_default()
                    .push(EditOperation::SetVolume(0.5), MediaKind::Audio);
                advance(&mut app, &events);
                assert_eq!(app.path.as_ref(), Some(&paths[0]));
                assert!(app.edits[&audio].is_dirty());
                assert!(
                    app.status_message
                        .as_ref()
                        .expect("guard status")
                        .0
                        .contains("preserve edits")
                );
                app.dispatch(CommandId::NextSameKind);
                assert!(app.pending_guard.is_some(), "manual Next uses dirty guard");
                app.resolve_guard(GuardDecision::Cancel);
                assert_eq!(app.path.as_ref(), Some(&paths[0]));
                assert_eq!(
                    app.audio_queues[&audio].order.next(&paths[0], false),
                    Some(paths[1].clone())
                );
                app.edits.insert(audio, EditHistory::default());
                app.navigate_to_unchecked(paths[0].clone());
                app.open_external(self.0.join("image.bmp"), true);
                let image = app.tabs.active().expect("image tab").id;
                wait(&mut app, &events, |app| {
                    app.image.is_some()
                        && app.retained_playback[&audio].state == PlaybackState::Ended
                });
                let image_instance = app.media_generation;
                app.metadata_export_settings.insert(audio, metadata);
                app.audio_export_settings.insert(
                    audio,
                    AudioExportOptions {
                        normalize_peak: true,
                        ..Default::default()
                    },
                );
                advance(&mut app, &events);
                assert_eq!(app.retained_playback[&audio].path, paths[1]);
                assert!(!app.audio_export_settings.contains_key(&audio));
                assert!(!app.metadata_export_settings.contains_key(&audio));
                assert_eq!(app.tabs.active().expect("image stays active").id, image);
                assert_eq!(app.media_generation, image_instance);
                assert_eq!(
                    app.tabs
                        .tabs()
                        .iter()
                        .find(|tab| tab.id == audio)
                        .expect("audio tab")
                        .target
                        .current_path(),
                    paths[1]
                );
                wait(&mut app, &events, |app| {
                    app.retained_playback[&audio].state == PlaybackState::Ended
                });
                app.edits
                    .entry(audio)
                    .or_default()
                    .push(EditOperation::SetVolume(0.5), MediaKind::Audio);
                advance(&mut app, &events);
                assert_eq!(
                    app.retained_playback[&audio].path, paths[1],
                    "background edits block automatic navigation"
                );
                assert!(app.edits[&audio].is_dirty());
                app.edits.insert(audio, EditHistory::default());
                app.audio_queues.get_mut(&audio).expect("queue").handled_eof = None;
                advance(&mut app, &events);
                assert_eq!(app.retained_playback[&audio].path, paths[2]);
                wait(&mut app, &events, |app| {
                    app.retained_playback[&audio].state == PlaybackState::Ended
                });
                advance(&mut app, &events);
                assert_eq!(
                    app.retained_playback[&audio].state,
                    PlaybackState::Ended,
                    "Repeat off stops at end"
                );
                let queue = app.audio_queues.get_mut(&audio).expect("queue");
                queue.order.cycle_repeat();
                queue.handled_eof = None;
                advance(&mut app, &events);
                assert_eq!(
                    app.retained_playback[&audio].path, paths[0],
                    "Repeat all wraps in background"
                );
                wait(&mut app, &events, |app| {
                    app.retained_playback[&audio].state == PlaybackState::Ended
                });
                let generation = app.retained_playback[&audio]
                    .session
                    .as_ref()
                    .expect("session")
                    .generation();
                app.audio_queues
                    .get_mut(&audio)
                    .expect("queue")
                    .order
                    .cycle_repeat();
                advance(&mut app, &events);
                assert_eq!(app.retained_playback[&audio].path, paths[0]);
                assert_ne!(
                    app.retained_playback[&audio]
                        .session
                        .as_ref()
                        .expect("session")
                        .generation(),
                    generation
                );
                assert_eq!(app.tabs.active().expect("still image").id, image);
                wait(&mut app, &events, |app| {
                    app.retained_playback[&audio].state == PlaybackState::Ended
                });
                let queue = app.audio_queues.get_mut(&audio).expect("queue");
                queue.order.cycle_repeat();
                queue
                    .order
                    .set_items(vec![paths[0].clone(), self.0.join("missing.wav")]);
                // Model disappearance after enumeration, before the next open.
                queue.provider = None;
                advance(&mut app, &events);
                assert_eq!(app.retained_playback[&audio].state, PlaybackState::Faulted);
                assert!(
                    app.playback_error.is_none(),
                    "background failure is isolated"
                );
                app.remove_tab(audio, false);
                assert!(app.audio_queues.is_empty() && app.retained_playback.is_empty());
                let early_folder = self.0.join("early");
                std::fs::create_dir(&early_folder).expect("owned early folder");
                for name in ["01.wav", "02.wav"] {
                    std::fs::copy(&paths[0], early_folder.join(name)).expect("owned early audio");
                }
                app.open_external(early_folder.join("01.wav"), true);
                let early = app.tabs.active().expect("early tab").id;
                assert!(app.audio_queues[&early].snapshot.is_none());
                app.activate_tab(image);
                wait(&mut app, &events, |app| {
                    app.retained_playback[&early].state == PlaybackState::Ended
                        && app.audio_queues[&early].snapshot.is_some()
                });
                app.audio_queues
                    .get_mut(&early)
                    .expect("early queue")
                    .order
                    .cycle_repeat();
                let next = app.audio_queues[&early]
                    .order
                    .next(&early_folder.join("01.wav"), true)
                    .expect("next early track");
                advance(&mut app, &events);
                assert_eq!(app.retained_playback[&early].path, next);
                assert_eq!(app.tabs.active().expect("image active").id, image);
                app.remove_tab(early, false);
                assert!(app.audio_queues.is_empty());
                app.remove_tab(image, false);
                drop(app);
                eprintln!(
                    "PASS audio queue: real silent EOF/next/repeat, paused/dirty guard, background identity and failure isolation"
                );
                event_loop.exit();
            }
            fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
        }
        EventLoop::builder()
            .with_any_thread(true)
            .build()
            .expect("event loop")
            .run_app(&mut Trial(root))
            .expect("audio trial");
    }
}
