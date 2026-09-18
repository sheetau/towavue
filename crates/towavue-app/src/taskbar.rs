use crate::*;
use towavue_runtime_windows::{TaskbarAction, TaskbarIcons, TaskbarTransport};

#[derive(Default)]
pub(super) struct State {
    icon_size: Option<u32>,
    owner: Option<(TabId, u64)>,
    revision: u64,
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    fn taskbar_owner(&self) -> Option<(TabId, u64)> {
        matches!(self.media_kind, Some(MediaKind::Audio | MediaKind::Video))
            .then(|| {
                self.tabs
                    .active_id()
                    .map(|tab| (tab, self.media_generation))
            })
            .flatten()
    }

    fn taskbar_transport(&self) -> Option<TaskbarTransport> {
        self.taskbar_owner()?;
        let enabled = !self.modal_input_blocked() && !self.exit_requested;
        let navigate = enabled
            && self.folder_snapshot.as_ref().is_some_and(|folder| {
                folder.items.len() > 1
                    && self
                        .path
                        .as_ref()
                        .is_some_and(|path| folder.items.iter().any(|item| &item.path == path))
            });
        Some(TaskbarTransport {
            context: self.taskbar_ui.revision,
            previous: navigate,
            play_pause: enabled && !self.command_context().playback_blocked,
            next: navigate,
            playing: self.state == PlaybackState::Playing,
        })
    }

    pub(super) fn sync_taskbar_transport(&mut self) {
        self.prepare_taskbar_icons();
        let owner = self.taskbar_owner();
        if owner != self.taskbar_ui.owner {
            self.taskbar_ui.owner = owner;
            self.taskbar_ui.revision = self.taskbar_ui.revision.wrapping_add(1);
        }
        let transport = self.taskbar_transport();
        if let Some(taskbar) = &mut self.native_taskbar
            && let Err(error) = taskbar.set_transport(transport)
        {
            eprintln!("Could not update taskbar transport: {error}");
        }
    }

    pub(super) fn handle_taskbar_click(&mut self, revision: u64, action: TaskbarAction) {
        if revision != self.taskbar_ui.revision || self.taskbar_ui.owner != self.taskbar_owner() {
            return;
        }
        let Some(transport) = self.taskbar_transport() else {
            return;
        };
        let (enabled, command) = match action {
            TaskbarAction::Previous => (transport.previous, CommandId::PreviousMedia),
            TaskbarAction::PlayPause => (transport.play_pause, CommandId::TogglePause),
            TaskbarAction::Next => (transport.next, CommandId::NextMedia),
        };
        if enabled {
            self.dispatch(command);
        }
    }

    fn prepare_taskbar_icons(&mut self) {
        if self.native_taskbar.is_none() || self.taskbar_owner().is_none() {
            return;
        }
        let Some(window) = &self.window else { return };
        let size = (32.0 * window.scale_factor()).round().clamp(16.0, 256.0) as u32;
        if self.taskbar_ui.icon_size == Some(size) {
            return;
        }
        // Build once per density, even before the first draw or while minimized.
        let images = icon_pixels(size);
        match TaskbarIcons::new(size, images.each_ref().map(|image| image.as_slice())) {
            Ok(icons) => self
                .native_taskbar
                .as_mut()
                .expect("taskbar")
                .set_icons(icons),
            Err(error) => eprintln!("Taskbar icons unavailable: {error}"),
        }
        self.taskbar_ui.icon_size = Some(size);
    }
}

fn icon_pixels(size: u32) -> [Vec<u8>; 4] {
    use crate::lucide::{Kind, render};
    [Kind::Previous, Kind::Play, Kind::Pause, Kind::Next].map(|kind| {
        let mut pixels = render(kind, size, 32.0)
            .expect("bounded taskbar icon")
            .data()
            .to_vec();
        let side = size as usize;
        // A black outline behind the white fill remains visible against
        // both light and dark Shell previews without OS theme overrides.
        let fill = pixels.clone();
        let radius = side.div_ceil(32);
        for y in 0..side {
            for x in 0..side {
                let mut alpha = 0;
                for row in y.saturating_sub(radius)..=(y + radius).min(side - 1) {
                    for column in x.saturating_sub(radius)..=(x + radius).min(side - 1) {
                        alpha = alpha.max(fill[(row * side + column) * 4 + 3]);
                    }
                }
                pixels[(y * side + x) * 4 + 3] = alpha;
            }
        }
        pixels
    })
}

#[cfg(test)]
pub(crate) mod tests;
