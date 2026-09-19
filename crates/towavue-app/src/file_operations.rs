use crate::*;
use towavue_runtime_windows::{
    FileOperationAction, FileOperationOutcome, FileOperationSource, VideoResumeSource,
};

pub(crate) mod preferences;
mod recycling;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Delete,
    Rename,
    Move,
}

pub(super) struct Pending {
    pub serial: u64,
    pub tab: TabId,
    pub path: PathBuf,
    pub origin_path: PathBuf,
    pub instance: u64,
    pub kind: Kind,
    pub source: Option<FileOperationSource>,
}

#[derive(Default)]
pub(super) struct State {
    pub serial: u64,
    pub pending: Option<Pending>,
    pub ready: Option<(FileOperationSource, FileOperationAction)>,
    pub locked: bool,
    pub position: Option<MediaTime>,
}

impl State {
    pub fn busy(&self) -> bool {
        self.locked || self.pending.is_some()
    }
}

#[derive(Clone)]
pub(super) struct RelocatedVersions {
    pub original: FileOperationSource,
    pub current: Option<FileOperationSource>,
}

#[derive(Clone)]
pub(super) struct Completed {
    pub versions: Option<Box<RelocatedVersions>>,
    pub outcome: FileOperationOutcome,
    pub resume: Option<VideoResumeSource>,
    pub recycle: Option<Box<towavue_runtime_windows::FileRecycleReport>>,
    pub preference_warning: Option<String>,
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn begin_file_relocation(&mut self, kind: Kind) {
        if self.current_source_deleted() || (kind == Kind::Delete && self.timeline_open) {
            return;
        }
        let Some(path) = self.path.clone() else {
            return;
        };
        self.begin_file_relocation_at(kind, path);
    }

    pub(super) fn begin_file_relocation_at(&mut self, kind: Kind, path: PathBuf) {
        if self.path.as_ref() == Some(&path) && self.current_source_deleted() {
            return;
        }
        if self.modal_input_blocked()
            || self.active_export.is_some()
            || self.image_loading
            || self.image_edit_pending
            || self.state == PlaybackState::Loading
        {
            self.set_status("Wait for the current operation before changing the file.".into());
            return;
        }
        let Some(tab) = self.tabs.active() else {
            return;
        };
        let origin_path = tab.target.current_path().to_owned();
        self.file_operations.serial = self.file_operations.serial.wrapping_add(1);
        let serial = self.file_operations.serial;
        self.file_operations.pending = Some(Pending {
            serial,
            tab: tab.id,
            path: path.clone(),
            origin_path,
            instance: self.media_generation,
            kind,
            source: None,
        });
        self.cancel_shortcut_prefix();
        self.cancel_view_drag();
        self.cancel_hold_speed();
        let notify = Arc::clone(&self.notify);
        if let Err(error) =
            towavue_runtime_windows::inspect_file_operation_source(path, move |result| {
                notify(AppEvent::FileOperationSource(
                    serial,
                    result.map_err(|error| error.to_string()),
                ));
            })
        {
            self.file_operations.pending = None;
            self.set_status(error.to_string());
        }
        self.request_redraw();
    }

    pub(super) fn file_relocation_is_current(&self) -> bool {
        self.file_operations
            .pending
            .as_ref()
            .is_some_and(|pending| {
                pending.instance == self.media_generation
                    && self.tabs.active().is_some_and(|tab| {
                        tab.id == pending.tab && tab.target.current_path() == pending.origin_path
                    })
            })
    }

    pub(super) fn finish_file_source(
        &mut self,
        serial: u64,
        result: Result<FileOperationSource, String>,
    ) {
        if self
            .file_operations
            .pending
            .as_ref()
            .is_none_or(|pending| pending.serial != serial)
        {
            return;
        }
        if !self.file_relocation_is_current() {
            self.file_operations.pending = None;
            return;
        }
        match result {
            Ok(source) => {
                if self
                    .file_operations
                    .pending
                    .as_ref()
                    .is_none_or(|pending| source.path() != pending.path)
                {
                    self.file_operations.pending = None;
                    self.set_status("The inspected file does not match this operation.".into());
                    return;
                }
                let pending = self
                    .file_operations
                    .pending
                    .as_mut()
                    .expect("current request");
                if pending.kind == Kind::Delete {
                    self.file_operations.ready = Some((source, FileOperationAction::Recycle));
                    self.request_redraw();
                    return;
                }
                let kind = match pending.kind {
                    Kind::Delete => unreachable!("handled above"),
                    Kind::Rename => FileDialogKind::RenameFile {
                        source: source.path().to_owned(),
                    },
                    Kind::Move => FileDialogKind::MoveFile {
                        source: source.path().to_owned(),
                    },
                };
                pending.source = Some(source);
                if !self.begin_dialog(kind, DialogIntent::RelocateFile) {
                    self.file_operations.pending = None;
                }
            }
            Err(error) => {
                self.file_operations.pending = None;
                self.set_status(error);
            }
        }
        self.request_redraw();
    }

    pub(super) fn finish_file_destination(&mut self, result: Result<Option<PathBuf>, DialogError>) {
        if !self.file_relocation_is_current() {
            self.file_operations.pending = None;
            self.set_status("The source changed while choosing a path; nothing was moved.".into());
            return;
        }
        match result {
            Ok(Some(target)) => {
                let pending = self
                    .file_operations
                    .pending
                    .as_mut()
                    .expect("current request");
                if let Some(source) = pending.source.take() {
                    let action = match pending.kind {
                        Kind::Delete => unreachable!("delete has no destination dialog"),
                        Kind::Rename => FileOperationAction::RenameToPath(target),
                        Kind::Move => FileOperationAction::MoveToFolder(target),
                    };
                    self.file_operations.ready = Some((source, action));
                }
            }
            Ok(None) => {
                self.file_operations.pending = None;
            }
            Err(error) => {
                self.file_operations.pending = None;
                self.set_status(error.to_string());
            }
        }
        self.request_redraw();
    }

    pub(super) fn quiesce_file_relocation(&mut self, source: &Path) {
        self.filmstrip
            .preserve_after_file_operation(self.ui_context.as_ref(), source, None);
        self.file_operations.locked = true;
        self.cancel_hold_speed();
        self.cancel_frame_steps();
        self.cancel_view_drag();
        self.cancel_shortcut_prefix();
        resume::record(self, true);
        if self.path.as_deref() == Some(source) {
            let position = self.current_position();
            if let Some(session) = &mut self.session {
                session.suspend_for_file_operation(position);
                self.file_operations.position = Some(position);
                self.clock = Some(PlaybackClock::paused(position, self.playback_rate()));
            }
            self.resume_open = None;
            self.waveform_worker.clear();
            self.waveform_loading = false;
            self.thumbnail_loading = None;
            self.thumbnail_worker.clear();
            self.video_sheets.clear();
        }
        for saved in self
            .retained_playback
            .values_mut()
            .filter(|saved| saved.path == source)
        {
            let position = saved.position();
            saved.recovery_position = Some(position);
            if let Some(session) = &mut saved.session {
                session.suspend_for_file_operation(position);
            }
            let rate = saved.session.as_ref().map_or(1.0, PlaybackSession::rate);
            saved.clock = Some(PlaybackClock::paused(position, rate));
        }
        self.tab_preview.clear();
        self.request_redraw();
    }

    pub(super) fn finish_file_relocation(&mut self, source: &Path, completed: Option<&Completed>) {
        let target = completed.and_then(|result| match &result.outcome {
            FileOperationOutcome::Moved(path) => Some(path.as_path()),
            _ => None,
        });
        if let Some(target) = target {
            self.filmstrip.preserve_after_file_operation(
                self.ui_context.as_ref(),
                source,
                Some(target),
            );
            let changed = self.tabs.relocate_file(source, target);
            for id in &changed {
                if let Some(version) = self.source_versions.get_mut(id) {
                    // Moving a newer externally replaced file must not rebase old
                    // edits. Only a matching loaded version adopts the moved stamp.
                    *version = completed
                        .and_then(|result| result.versions.as_ref())
                        .filter(|result| version.as_ref() == Some(&result.original))
                        .and_then(|result| result.current.clone());
                }
                self.media_sequence = self
                    .media_sequence
                    .max(self.media_generation)
                    .wrapping_add(1);
                let instance = self.media_sequence;
                if self.displayed_tab == Some(*id) {
                    self.duration_workers.remove(&self.media_generation);
                    self.media_generation = instance;
                }

                self.viewed_media.begin(*id, target);
                if let Some(saved) = self.retained_images.get_mut(id) {
                    self.duration_workers.remove(&saved.instance);
                    saved.instance = instance;
                    saved.path = target.to_owned();
                    saved.filmstrip_view.relocate(source, target);
                    saved.folder_snapshot = None;
                    saved.previews.clear();
                    if let Some(focus) = &mut saved.reading_focus
                        && focus.path == source
                    {
                        focus.path = target.to_owned();
                    }
                }
                if let Some(saved) = self.retained_playback.get_mut(id) {
                    self.duration_workers.remove(&saved.instance);
                    saved.instance = instance;
                    saved.path = target.to_owned();
                    saved.filmstrip_view.relocate(source, target);
                    saved.folder_snapshot = None;
                    if let (Some(owner), Some(stamp)) = (
                        &mut saved.resume,
                        completed.and_then(|result| result.resume.as_ref()),
                    ) {
                        owner.relocate(stamp.clone());
                    }
                }
                if let Some(queue) = self.audio_queues.get_mut(id) {
                    queue.relocate(source, target);
                }
            }
            if self.path.as_deref() == Some(source) {
                self.path = Some(target.to_owned());
                self.image_previews.clear();
                if let Some(focus) = &mut self.reading_focus
                    && focus.path == source
                {
                    focus.path = target.to_owned();
                }
                if let (Some(owner), Some(stamp)) = (
                    &mut self.resume_owner,
                    completed.and_then(|result| result.resume.as_ref()),
                ) {
                    owner.relocate(stamp.clone());
                }
                self.refresh_folder_snapshot();
                self.refresh_status_file_details();
            }
            for closed in &mut self.closed_tabs {
                if let closed_tabs::ClosedTab::Media(path, _) = closed
                    && path == source
                {
                    *path = target.to_owned();
                }
            }
            self.tab_preview.clear();
        }
        let path = target.unwrap_or(source);
        let input = self.media_input(self.path.as_deref().unwrap_or(path));
        if let Some(position) = self.file_operations.position.take() {
            if let Some(session) = &mut self.session {
                match session.resume_after_file_operation_input(input, position) {
                    Ok(generation) => {
                        self.generation = generation;
                        self.audio_drained = !session.has_audio();
                        self.decode_finished = false;
                        self.pending_time = None;
                    }
                    Err(error) => self.fail(error.to_string()),
                }
            }
            self.clock = Some(if self.state == PlaybackState::Playing {
                PlaybackClock::new(position, self.playback_rate())
            } else {
                PlaybackClock::paused(position, self.playback_rate())
            });
        }
        let inputs: BTreeMap<_, _> = self
            .retained_playback
            .iter()
            .filter(|(_, saved)| saved.path == path)
            .map(|(id, _)| (*id, self.media_input_for(Some(*id), path)))
            .collect();
        for (id, saved) in self
            .retained_playback
            .iter_mut()
            .filter(|(_, saved)| saved.path == path)
        {
            if let Some(position) = saved.recovery_position.take() {
                if let Some(session) = &mut saved.session {
                    if let Err(error) =
                        session.resume_after_file_operation_input(inputs[id].clone(), position)
                    {
                        saved.fail(error.to_string());
                    } else {
                        saved.audio_drained = !session.has_audio();
                        saved.pending_time = None;
                        saved.decode_finished = false;
                    }
                }
                let rate = saved.session.as_ref().map_or(1.0, PlaybackSession::rate);
                saved.clock = Some(if saved.state == PlaybackState::Playing {
                    PlaybackClock::new(position, rate)
                } else {
                    PlaybackClock::paused(position, rate)
                });
            }
        }
        self.file_operations.locked = false;
        if self.path.as_deref() == Some(path) {
            if self.timeline_open
                && self.waveform.is_none()
                && self.media_kind != Some(MediaKind::Image)
            {
                self.load_waveform();
            }
            if self.media_duration.is_none() && self.media_kind != Some(MediaKind::Image) {
                self.load_duration(path.to_owned());
            }
        }

        // Folder notifications arriving inside the mutation were deliberately held.
        // Re-enumerate with final paths rather than accepting an intermediate listing.
        if completed.is_some_and(|result| matches!(result.outcome, FileOperationOutcome::Moved(_)))
        {
            // Keep the old view while the replacement Shell snapshot is pending.
            // Clearing it here would reset an open filmstrip's scroll position.
            self.refresh_folder_snapshot();
            for queue in self.audio_queues.values_mut() {
                queue.refresh_after_relocation();
            }
        } else {
            self.finish_folder_load();
            self.finish_audio_folder_loads();
        }

        self.refresh_title();
        self.request_redraw();
    }
}
