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
    // Codepoints from the bundled Monaco/Codicon mapping: chevron-left, play,
    // debug-pause, chevron-right. Retain the existing artwork attribution.
    // A bounded, icon-only font collection avoids requiring a UI pass or copying
    // the window's potentially large text atlas. It is dropped after this batch.
    let mut definitions = egui::FontDefinitions::empty();
    definitions.font_data.insert(
        "codicon".into(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/codicon.ttf")).into(),
    );
    definitions
        .families
        .insert(fonts::icon_font().family, vec!["codicon".into()]);
    let mut collection = egui::epaint::text::Fonts::new(Default::default(), definitions);
    let glyphs = {
        let mut fonts = collection.with_pixels_per_point(size as f32 / 32.0);
        ['\u{eab5}', '\u{eb2c}', '\u{ead1}', '\u{eab6}'].map(|glyph| {
            let galley = fonts.layout_no_wrap(
                glyph.to_string(),
                egui::FontId::new(32.0, fonts::icon_font().family),
                egui::Color32::WHITE,
            );
            galley.rows[0].glyphs[0].uv_rect
        })
    };
    let atlas = collection.texture_atlas().image();
    glyphs.map(|uv| {
        let width = usize::from(uv.max[0] - uv.min[0]);
        let height = usize::from(uv.max[1] - uv.min[1]);
        let side = size as usize;
        let mut pixels = vec![0; side * side * 4];
        for y in 0..height.min(side) {
            for x in 0..width.min(side) {
                let alpha = atlas[(usize::from(uv.min[0]) + x, usize::from(uv.min[1]) + y)].a();
                let target = ((y + side.saturating_sub(height) / 2) * side
                    + x
                    + side.saturating_sub(width) / 2)
                    * 4;
                pixels[target..target + 4].fill(alpha);
            }
        }
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
