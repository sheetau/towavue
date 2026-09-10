use super::*;

#[derive(Clone)]
pub(super) struct DetachRequest {
    pub tab: TabId,
    path: PathBuf,
    instance: u64,
}

pub(super) struct PlaybackTransfer {
    target: towavue_core::TabTarget,
    playback: playback_tab::RetainedPlaybackTab,
    edits: Option<EditHistory>,
    export_path: Option<PathBuf>,
    audio_options: Option<AudioExportOptions>,
    metadata_options: Option<MetadataExportOptions>,
    audio_queue: Option<audio_playback::AudioTab>,
    focus: Option<egui::Id>,
    timeline: Option<egui::containers::panel::PanelState>,
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn request_tab_detach(&mut self, id: TabId) {
        if !self.hosted_graphics
            || self
                .tabs
                .tabs()
                .iter()
                .find(|tab| tab.id == id)
                .is_some_and(|tab| tab.target.media_kind() == MediaKind::Image)
        {
            self.request_guarded(GuardedAction::DetachTab(id));
            return;
        }
        match self.playback_detach_request(id) {
            Ok(request) => self.pending_tab_detach = Some(request),
            Err(error) => self.set_status(format!("Could not detach tab: {error}")),
        }
    }

    pub(super) fn validate_transfer_window(&self) -> Result<(), String> {
        if self.exit_requested || self.modal_input_blocked() {
            return Err("close the dialog before moving a tab".into());
        }
        if self.renderer.is_none()
            || self.ui_context.is_none()
            || self.graphics_recovery_request.is_some()
        {
            return Err("wait for the window's graphics device to become available".into());
        }
        Ok(())
    }

    pub(super) fn playback_detach_request(&self, id: TabId) -> Result<DetachRequest, String> {
        self.validate_transfer_window()?;
        let tab = self
            .tabs
            .tabs()
            .iter()
            .find(|tab| tab.id == id)
            .ok_or("the tab is no longer open")?;
        if !matches!(tab.target.media_kind(), MediaKind::Audio | MediaKind::Video) {
            return Err("live transfer requires an audio or video tab".into());
        }
        if self
            .active_export
            .as_ref()
            .is_some_and(|export| export.tab == id)
        {
            return Err("wait for this tab's export to finish".into());
        }
        let path = tab.target.current_path();
        let instance = if self.displayed_tab == Some(id) && self.path.as_deref() == Some(path) {
            self.media_generation
        } else {
            self.retained_playback
                .get(&id)
                .filter(|saved| saved.path == path)
                .ok_or("the tab's playback state is unavailable")?
                .instance
        };
        Ok(DetachRequest {
            tab: id,
            path: path.to_owned(),
            instance,
        })
    }

    pub(super) fn validate_playback_transfer(&self, request: &DetachRequest) -> Result<(), String> {
        let current = self.playback_detach_request(request.tab)?;
        if current.path != request.path || current.instance != request.instance {
            return Err("the tab changed before it could be moved".into());
        }
        Ok(())
    }

    pub(super) fn take_playback_transfer(&mut self, request: &DetachRequest) -> PlaybackTransfer {
        let id = request.tab;
        let target = self
            .tabs
            .tabs()
            .iter()
            .find(|tab| tab.id == id)
            .expect("validated tab")
            .target
            .clone();
        let mut playback = if self.displayed_tab == Some(id) {
            self.cancel_hold_speed();
            self.cancel_frame_steps();
            self.take_playback_tab_state()
        } else {
            self.retained_playback
                .remove(&id)
                .expect("validated playback")
        };
        // egui texture and widget IDs belong to the source context, unlike the
        // runtime frame, which remains on the host's shared D3D11 device.
        playback.waveform = None;
        playback.playlist.detach_context();
        self.duration_workers.remove(&playback.instance);
        let focus = self
            .ui_context
            .as_ref()
            .and_then(|context| tab_focus::take(context, id));
        let timeline = self.ui_context.as_ref().and_then(|context| {
            context.data_mut(|data| {
                data.get_persisted::<egui::containers::panel::PanelState>(egui::Id::new((
                    "timeline", id,
                )))
            })
        });
        let transfer = PlaybackTransfer {
            target,
            playback,
            edits: self.edits.remove(&id),
            export_path: self.export_paths.remove(&id),
            audio_options: self.audio_export_settings.remove(&id),
            metadata_options: self.metadata_export_settings.remove(&id),
            audio_queue: self.audio_queues.remove(&id),
            focus,
            timeline,
        };
        self.remove_tab(id, false);
        transfer
    }

    pub(super) fn accept_playback_transfer(
        &mut self,
        mut transfer: PlaybackTransfer,
        gap: usize,
    ) -> TabId {
        let path = transfer.target.current_path().to_owned();
        let kind = transfer.target.media_kind();
        let id = self.tabs.open_new(path.clone(), kind);
        self.tabs.get_mut(id).expect("new tab").target = transfer.target;
        self.tabs.reorder(id, gap);
        // Reserve an identity without changing the still-displayed tab's instance;
        // load_path must retain that tab under its existing worker identity.
        self.media_sequence = self
            .media_sequence
            .max(self.media_generation)
            .wrapping_add(1);
        let old_instance = transfer.playback.instance;
        transfer.playback.instance = self.media_sequence;
        transfer.playback.graphics_epoch = self.graphics_epoch;
        if let Some(edits) = transfer.edits {
            self.edits.insert(id, edits);
        }
        if let Some(path) = transfer.export_path {
            self.export_paths.insert(id, path);
        }
        if let Some(options) = transfer.audio_options {
            self.audio_export_settings.insert(id, options);
        }
        if let Some(options) = transfer.metadata_options {
            self.metadata_export_settings.insert(id, options);
        }
        if let Some(mut queue) = transfer.audio_queue {
            let notify = Arc::clone(&self.notify);
            queue.transfer(old_instance, self.media_sequence, move || {
                notify(AppEvent::FolderReady)
            });
            self.audio_queues.insert(id, queue);
        }
        self.retained_playback.insert(id, transfer.playback);
        self.load_path_with_transfer(path, kind, true);
        if let Some(context) = &self.ui_context {
            if let Some(focus) = transfer.focus {
                tab_focus::adopt(context, id, focus);
            }
            if let Some(timeline) = transfer.timeline {
                context.data_mut(|data| {
                    data.insert_persisted(egui::Id::new(("timeline", id)), timeline)
                });
            }
        }
        id
    }
}
