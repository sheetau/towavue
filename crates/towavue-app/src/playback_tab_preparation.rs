use crate::*;
use towavue_runtime_windows::VideoResumeSource;

#[cfg(test)]
mod tests;

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn prepare_playback_tab(&mut self, id: TabId) {
        if self.displayed_tab == Some(id) || self.retained_playback.contains_key(&id) {
            return;
        }
        let Some(tab) = self.tabs.tabs().iter().find(|tab| tab.id == id) else {
            return;
        };
        let kind = tab.target.media_kind();
        if !matches!(kind, MediaKind::Video | MediaKind::Audio) {
            return;
        }
        let path = tab.target.current_path().to_owned();
        self.seed_playback_volume(id);
        self.media_sequence = self
            .media_sequence
            .max(self.media_generation)
            .wrapping_add(1);
        let instance = self.media_sequence;
        self.retained_playback.insert(
            id,
            playback_tab::RetainedPlaybackTab {
                prepared_only: true,
                path: path.clone(),
                kind,
                instance,
                origin: None,
                resume: None,
                session: None,
                clock: None,
                state: PlaybackState::Loading,
                audio_drained: true,
                decode_finished: false,
                pending_time: None,
                duration: None,
                waveform: None,
                waveform_detail: waveform_detail::Detail::default(),
                view: ImageViewState::default(),
                timeline_open: false,
                time_selection: None,
                playback_selection: None,
                video_repeat: false,
                filmstrip_open: false,
                filmstrip_view: filmstrip::View::default(),
                playlist: playlist::Playlist::default(),
                folder_snapshot: self
                    .folder_snapshot
                    .as_ref()
                    .filter(|snapshot| Some(snapshot.folder_path.as_path()) == path.parent())
                    .cloned(),
                error: None,
                status: None,
                export_notice: None,
                seek_latencies: Vec::new(),
                drift_samples: Vec::new(),
                metrics_recorded: false,
                graphics_epoch: self.graphics_epoch,
                recovery_position: None,
                video_suspended: false,
            },
        );
        if kind == MediaKind::Audio {
            self.ensure_audio_queue_for(id, &path);
        }
        let worker = match LatestTask::new("towavue-tab-metadata") {
            Ok(worker) => worker,
            Err(error) => {
                self.retained_playback
                    .get_mut(&id)
                    .expect("prepared tab")
                    .status = Some((format!("Tab metadata unavailable: {error}"), Instant::now()));
                return;
            }
        };
        let notify = Arc::clone(&self.notify);
        let cache = self.preview_cache.clone();
        let input = self.media_input_for(Some(id), &path);
        worker.submit(move |cancellation| {
            let cache = cache.cancellable(cancellation);
            // Capture source identity on this worker too. Merely hovering never
            // writes resume history; it only enables a later explicit transport.
            let source = (kind == MediaKind::Video)
                .then(|| VideoResumeSource::capture(&path).ok())
                .flatten();
            let duration = cache
                .duration(input.path())
                .map_err(|error| error.to_string());
            notify(AppEvent::PreparedPlayback(
                id, instance, path, duration, source,
            ));
        });
        self.duration_workers.insert(instance, worker);
        self.request_redraw();
    }

    pub(super) fn finish_playback_tab_preparation(
        &mut self,
        id: TabId,
        instance: u64,
        path: PathBuf,
        duration: Result<Duration, String>,
        source: Option<VideoResumeSource>,
    ) {
        self.duration_workers.remove(&instance);
        if !self
            .tabs
            .tabs()
            .iter()
            .any(|tab| tab.id == id && tab.target.current_path() == path)
        {
            return;
        }
        let Some(saved) = self.retained_playback.get_mut(&id).filter(|saved| {
            saved.instance == instance && saved.path == path && saved.prepared_only
        }) else {
            return;
        };
        match duration {
            Ok(duration) => {
                saved.duration = Some(duration);
                saved.resume = source.map(resume::Owner::for_preview);
                self.edits
                    .entry(id)
                    .or_default()
                    .set_source_duration(Some(media_time(duration)));
            }
            Err(error) => {
                saved.status = Some((format!("Tab metadata unavailable: {error}"), Instant::now()))
            }
        }
        self.request_redraw();
    }

    pub(super) fn start_prepared_playback(&mut self, id: TabId) -> bool {
        let Some(saved) = self.retained_playback.get(&id) else {
            return false;
        };
        if !saved.prepared_only {
            return saved.session.is_some();
        }
        if saved.duration.is_none() || saved.recovery_position.is_some() {
            return false;
        }
        let Some(renderer) = &self.renderer else {
            return false;
        };
        let device = renderer.graphics_device();
        let path = saved.path.clone();
        let instance = saved.instance;
        let edit = self
            .edits
            .get(&id)
            .map(EditHistory::state)
            .unwrap_or_default();
        let volume = self.playback_volume_for(id) * edit.volume;
        let notify = Arc::clone(&self.notify);
        let input = self.media_input_for(Some(id), &path);
        let result = PlaybackSession::open_input(
            input,
            device,
            volume,
            edit.rate,
            edit.playback_range(),
            true,
            move |event| notify(AppEvent::Playback(instance, event)),
        );
        let saved = self
            .retained_playback
            .get_mut(&id)
            .expect("validated prepared tab");
        saved.prepared_only = false;
        match result {
            Ok(session) => {
                self.source_versions
                    .entry(id)
                    .or_insert_with(|| session.source().cloned());
                saved.origin = self.window_key.map(|key| (key, instance));
                saved.audio_drained = !session.has_audio();
                saved.clock = Some(PlaybackClock::paused(session.target(), session.rate()));
                saved.session = Some(session);
                saved.state = PlaybackState::Paused;
                saved.suspend_video_if_bounded();
                true
            }
            Err(error) => {
                saved.fail(error.to_string());
                self.request_redraw();
                false
            }
        }
    }
}
