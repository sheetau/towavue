use super::*;

#[derive(Default)]
pub(super) struct Layout {
    ids: Vec<TabId>,
    width: f32,
    available: f32,
    gap: f32,
    region: Option<egui::Rect>,
    pending: Option<TabId>,
    frozen: bool,
}

impl Layout {
    pub(super) fn request(&mut self, id: TabId, context: Option<&egui::Context>) {
        self.pending = context
            .filter(|context| {
                context.input(|input| {
                    input.pointer.any_released()
                        && input
                            .pointer
                            .hover_pos()
                            .is_some_and(|pos| self.region.is_some_and(|rect| rect.contains(pos)))
                })
            })
            .map(|_| id);
    }

    pub(super) fn width(
        &mut self,
        ids: Vec<TabId>,
        region: egui::Rect,
        gap: f32,
        pointer: Option<egui::Pos2>,
    ) -> f32 {
        let available = region.width();
        let hovering = pointer.is_some_and(|pos| region.contains(pos));
        if !hovering || self.available != available || self.gap != gap {
            self.frozen = false;
            self.pending = None;
        }
        // An unchanged set on the next layout means the close was refused or
        // deferred by a guard. Do not let that request affect a later key close.
        if ids == self.ids {
            self.pending = None;
        }
        if ids != self.ids {
            if let Some(closed) = self.pending.take()
                && self.ids.contains(&closed)
                && ids
                    .iter()
                    .copied()
                    .eq(self.ids.iter().copied().filter(|id| *id != closed))
                && !ids.is_empty()
            {
                if self.ids.last() == Some(&closed) {
                    let total = self.width * self.ids.len() as f32
                        + gap * self.ids.len().saturating_sub(1) as f32;
                    self.width =
                        (total - gap * ids.len().saturating_sub(1) as f32) / ids.len() as f32;
                }
                self.frozen = true;
            } else {
                self.frozen = false;
            }
        }
        if !self.frozen {
            self.width = chrome::tab_width(available, ids.len(), gap);
        }
        self.ids = ids;
        self.available = available;
        self.gap = gap;
        self.region = Some(region);
        self.width
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_middle_keeps_width_and_closing_last_keeps_total_until_pointer_leaves() {
        let mut tabs = towavue_core::TabSet::default();
        let ids: Vec<_> = (0..4)
            .map(|i| tabs.open_new(PathBuf::from(format!("{i}.png")), MediaKind::Image))
            .collect();
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420.0, 32.0));
        let mut layout = Layout::default();
        let pointer = Some(rect.center());
        let original = layout.width(ids.clone(), rect, 4.0, pointer);
        layout.pending = Some(ids[1]);
        let remaining = vec![ids[0], ids[2], ids[3]];
        assert_eq!(
            layout.width(remaining.clone(), rect, 4.0, pointer),
            original
        );
        layout.pending = Some(ids[3]);
        let remaining = vec![ids[0], ids[2]];
        let expanded = layout.width(remaining.clone(), rect, 4.0, pointer);
        assert_eq!(expanded * 2.0 + 4.0, original * 3.0 + 8.0);
        assert_eq!(
            layout.width(remaining.clone(), rect, 4.0, None),
            chrome::tab_width(420.0, 2, 4.0)
        );
        layout.pending = Some(ids[0]);
        layout.width(remaining.clone(), rect, 4.0, pointer);
        assert!(!layout.frozen, "canceled close cannot freeze width");
        assert!(layout.pending.is_none());
        layout.width(ids.clone(), rect, 4.0, pointer);
        assert!(!layout.frozen, "new or reordered tabs reset closing state");
        layout.width(vec![ids[0]], rect, 4.0, pointer);
        assert!(!layout.frozen, "keyboard close has no pointer request");
    }
    #[test]
    fn pointer_close_keeps_next_close_button_under_the_cursor_at_three_densities() {
        let Some(root) = crate::tests::isolated_test_root(
            "tab_close_layout::tests::pointer_close_keeps_next_close_button_under_the_cursor_at_three_densities",
        ) else {
            return;
        };
        for density in [1.0, 1.25, 2.0] {
            for trailing in [false, true] {
                let mut app = Application::new(None, |_| {}).expect("app");
                let context = fonts::test_context();
                context.enable_accesskit();
                context.set_pixels_per_point(density);
                app.ui_context = Some(context.clone());
                for i in 0..5 {
                    app.tabs
                        .open_new(root.join(format!("{i}.png")), MediaKind::Image);
                }
                app.tabs
                    .close_gallery(app.tabs.gallery().expect("initial Gallery"));
                app.media_kind = Some(MediaKind::Image);
                app.state = PlaybackState::Paused;
                let frame = |app: &mut Application<_>, events| {
                    let mut actions = Vec::new();
                    let output = context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(960.0, 576.0),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ui| app.draw_ui(ui, &mut actions),
                    );
                    for action in actions {
                        app.handle_ui_action(action);
                    }
                    output.platform_output.accesskit_update.expect("tree")
                };
                let close_center = |tree: &egui::accesskit::TreeUpdate, label: &str| {
                    let rect = tree
                        .nodes
                        .iter()
                        .find(|(_, node)| node.label() == Some(label))
                        .expect("close button")
                        .1
                        .bounds()
                        .expect("bounds");
                    egui::pos2(
                        (rect.x0 + rect.x1) as f32 * 0.5,
                        (rect.y0 + rect.y1) as f32 * 0.5,
                    )
                };
                frame(&mut app, vec![]);
                let tree = frame(&mut app, vec![]);
                let index = if trailing { 4 } else { 1 };
                let point = close_center(&tree, &format!("Close tab: {index}.png"));
                frame(&mut app, vec![egui::Event::PointerMoved(point)]);
                for pressed in [true, false] {
                    frame(
                        &mut app,
                        vec![egui::Event::PointerButton {
                            pos: point,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        }],
                    );
                }
                assert_eq!(app.tabs.len(), 4, "density={density} trailing={trailing}");
                frame(&mut app, vec![]);
                let tree = frame(&mut app, vec![]);
                let next = if trailing { 3 } else { 2 };
                let after = close_center(&tree, &format!("Close tab: {next}.png"));
                assert!(
                    (after.x - point.x).abs() <= 1.0,
                    "density={density} trailing={trailing}: {point:?} -> {after:?}"
                );
                assert!(app.tab_close_layout.frozen);
                frame(
                    &mut app,
                    vec![egui::Event::PointerMoved(egui::pos2(20.0, 400.0))],
                );
                assert!(!app.tab_close_layout.frozen);
            }
        }
    }
}
