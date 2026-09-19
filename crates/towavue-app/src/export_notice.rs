use super::*;

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    /// The target travels with its timed notification, never with the displayed
    /// source path or edited-save cursor (audio/frame derivatives have neither).
    pub(super) fn export_notice_target(&self, shown: Instant) -> Option<&Path> {
        let (stamp, path) = self.export_notice.as_ref()?;
        let (message, current) = self.status_message.as_ref()?;
        if *stamp != shown
            || *current != shown
            || shown.elapsed() >= STATUS_MESSAGE_DURATION
            || self.modal_input_blocked()
            || self.palette_open
            || self.grid_open
            || self
                .ui_context
                .as_ref()
                .is_some_and(egui::Popup::is_any_open)
            || self.status_notice().as_deref() != Some(message.as_str())
        {
            return None;
        }
        Some(path)
    }

    pub(super) fn export_notice_open_target(&self, shown: Instant) -> Option<&Path> {
        let target = self.export_notice_target(shown)?;
        // Source Save and rename/move notices already show this file. Compare the
        // current document as well as a held image during an asynchronous switch.
        (self.path.as_deref() != Some(target)
            && self.displayed_image_path().map(PathBuf::as_path) != Some(target))
        .then_some(target)
    }

    /// Keep the full notice for its tooltip and reveal identity; omit only the
    /// exact destination from the compact status label.
    pub(super) fn compact_export_notice(&self, message: &str) -> Option<String> {
        let (shown, target) = self.export_notice.as_ref()?;
        let (_, current) = self.status_message.as_ref()?;
        if shown != current {
            return None;
        }
        let target = target.display().to_string();
        let (before, after) = message.split_once(&target)?;
        let before = before.trim_end_matches([' ', ':']);
        Some(if after.trim().is_empty() {
            before.into()
        } else {
            format!("{before} {}", after.trim_start())
        })
    }

    pub(super) fn reveal_path(&mut self, path: PathBuf) {
        let notify = Arc::clone(&self.notify);
        if let Err(error) = reveal_file(path, move |result| notify(AppEvent::FileRevealed(result)))
        {
            self.set_status(format!("Could not reveal file: {error}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(
        app: &mut Application<impl Fn(AppEvent) + Send + Sync + 'static>,
        path: &Path,
    ) -> Instant {
        app.set_status(format!("Exported: {}", path.display()));
        let shown = app.status_message.as_ref().expect("notice").1;
        app.export_notice = Some((shown, path.into()));
        shown
    }

    fn paint(
        app: &Application<impl Fn(AppEvent) + Send + Sync + 'static>,
        width: f32,
        events: Vec<egui::Event>,
    ) -> (egui::FullOutput, Vec<UiAction>) {
        let mut actions = Vec::new();
        let output = app.ui_context.as_ref().expect("context").run_ui(
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
    }

    fn link(output: &egui::FullOutput) -> Option<(egui::accesskit::NodeId, egui::Rect)> {
        output
            .platform_output
            .accesskit_update
            .as_ref()?
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Show exported file in Explorer"))
            .map(|(id, node)| {
                assert_eq!(node.role(), egui::accesskit::Role::Button);
                let bounds = node.bounds().expect("link bounds");
                (
                    *id,
                    egui::Rect::from_min_max(
                        egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                        egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
                    ),
                )
            })
    }

    #[test]
    fn status_link_pointer_and_accessibility_use_the_export_target() {
        let Some(root) = crate::tests::isolated_test_root(
            "export_notice::tests::status_link_pointer_and_accessibility_use_the_export_target",
        ) else {
            return;
        };
        let target = root
            .join("export folder")
            .join("画像 long exported file name.png");
        let mut app = Application::new(None, |_| {}).expect("app");
        app.path = Some(root.join("different source.jpg"));
        for density in [1.0, 1.25, 2.0] {
            for width in [240.0, 640.0] {
                let context = fonts::test_context();
                context.set_pixels_per_point(density);
                context.global_style_mut(|style| style.interaction.tooltip_delay = 0.0);
                context.enable_accesskit();
                app.ui_context = Some(context);
                let shown = notice(&mut app, &target);
                paint(&app, width, vec![]);
                let (output, _) = paint(&app, width, vec![]);
                assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                    egui::Shape::Text(text) if text.galley.text() == "Exported")));
                assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
                    egui::Shape::Text(text) if text.galley.text().contains("long exported file name"))));
                let (id, rect) = link(&output).expect("export link");
                let pos = rect.center();
                let (hover, _) = paint(&app, width, vec![egui::Event::PointerMoved(pos)]);
                assert_eq!(
                    hover.platform_output.cursor_icon,
                    egui::CursorIcon::PointingHand
                );
                let (help, _) = paint(&app, width, vec![egui::Event::PointerMoved(pos)]);
                assert!(help.shapes.iter().any(|shape| matches!(&shape.shape,
                    egui::Shape::Text(text) if text.galley.text().contains(&target.display().to_string()))), "full path stays in hover help");
                let button = |pressed| egui::Event::PointerButton {
                    pos,
                    pressed,
                    button: egui::PointerButton::Primary,
                    modifiers: egui::Modifiers::NONE,
                };
                paint(&app, width, vec![button(true)]);
                let (_, actions) = paint(&app, width, vec![button(false)]);
                assert!(actions == vec![UiAction::RevealExport(shown)]);
                let middle = |pressed| egui::Event::PointerButton {
                    pos,
                    pressed,
                    button: egui::PointerButton::Middle,
                    modifiers: egui::Modifiers::NONE,
                };
                paint(&app, width, vec![middle(true)]);
                let (_, actions) = paint(&app, width, vec![middle(false)]);
                assert!(actions == vec![UiAction::OpenExport(shown)]);
                let source = app.path.replace(target.clone());
                paint(&app, width, vec![middle(true)]);
                let (_, actions) = paint(&app, width, vec![middle(false)]);
                assert!(
                    actions.is_empty(),
                    "current document never opens a duplicate"
                );
                app.path = source;
                let (_, actions) = paint(
                    &app,
                    width,
                    vec![egui::Event::AccessKitActionRequest(
                        egui::accesskit::ActionRequest {
                            action: egui::accesskit::Action::Click,
                            target_tree: egui::accesskit::TreeId::ROOT,
                            target_node: id,
                            data: None,
                        },
                    )],
                );
                assert!(actions == vec![UiAction::RevealExport(shown)]);
                assert_eq!(app.export_notice_target(shown), Some(target.as_path()));
                // Do not dispatch a valid reveal: these synthetic controls must
                // not open Explorer or change the operator's foreground window.
            }
        }
    }

    #[test]
    fn middle_click_export_notice_opens_owned_output_in_a_new_tab() {
        let Some(root) = crate::tests::isolated_test_root(
            "export_notice::tests::middle_click_export_notice_opens_owned_output_in_a_new_tab",
        ) else {
            return;
        };
        let target = root.join("result.bmp");
        let mut bytes = vec![0_u8; 62];
        bytes[..2].copy_from_slice(b"BM");
        bytes[2..6].copy_from_slice(&62_u32.to_le_bytes());
        bytes[10..14].copy_from_slice(&54_u32.to_le_bytes());
        bytes[14..18].copy_from_slice(&40_u32.to_le_bytes());
        bytes[18..22].copy_from_slice(&2_u32.to_le_bytes());
        bytes[22..26].copy_from_slice(&1_u32.to_le_bytes());
        bytes[26..28].copy_from_slice(&1_u16.to_le_bytes());
        bytes[28..30].copy_from_slice(&24_u16.to_le_bytes());
        std::fs::write(&target, &bytes).expect("owned bitmap");
        let mut app = Application::new(None, |_| {}).expect("app");
        let source = root.join("source.png");
        let original = app.tabs.open_new(source.clone(), MediaKind::Image);
        app.path = Some(source);
        app.media_kind = Some(MediaKind::Image);
        app.edits
            .entry(original)
            .or_default()
            .push(EditOperation::FlipHorizontal, MediaKind::Image);
        let count = app.tabs.len();
        let shown = notice(&mut app, &target);
        app.handle_ui_action(UiAction::OpenExport(shown));
        assert_eq!(app.tabs.len(), count + 1);
        let opened = app.tabs.active().expect("opened tab");
        assert_ne!(opened.id, original);
        assert_eq!(
            opened.target.current_path().expect("file-backed tab"),
            canonical_shell_path(&target).expect("path")
        );
        assert!(app.edits.get(&original).expect("old edits").is_dirty());
        assert_eq!(std::fs::read(&target).expect("output preserved"), bytes);
        let shown = notice(&mut app, &target);
        app.handle_ui_action(UiAction::OpenExport(shown));
        assert_eq!(app.tabs.len(), count + 1, "current path stays in this tab");
    }

    #[test]
    fn stale_expired_and_blocked_notices_cannot_reveal() {
        let Some(root) = crate::tests::isolated_test_root(
            "export_notice::tests::stale_expired_and_blocked_notices_cannot_reveal",
        ) else {
            return;
        };
        let (send, events) = std::sync::mpsc::channel();
        let mut app = Application::new(None, move |event| {
            let _ = send.send(event);
        })
        .expect("app");
        let context = fonts::test_context();
        context.enable_accesskit();
        app.ui_context = Some(context);
        let target = root.join("result.png");
        let stale = notice(&mut app, &target);
        let message = app.status_message.as_ref().expect("notice").0.clone();
        app.set_status(message);
        assert!(app.export_notice.is_none());
        app.handle_ui_action(UiAction::RevealExport(stale));
        assert!(link(&paint(&app, 640.0, vec![]).0).is_none());
        for blocked in 0..4 {
            let shown = notice(&mut app, &target);
            match blocked {
                0 => {
                    let old = shown - STATUS_MESSAGE_DURATION - Duration::from_secs(1);
                    app.status_message.as_mut().expect("notice").1 = old;
                    app.export_notice.as_mut().expect("target").0 = old;
                    assert!(app.export_notice_target(old).is_none());
                }
                1 => app.palette_open = true,
                2 => app.grid_open = true,
                _ => app.export_error = Some("fixture failure".into()),
            }
            assert!(app.export_notice_target(shown).is_none());
            assert!(link(&paint(&app, 640.0, vec![]).0).is_none());
            let count = app.tabs.len();
            app.handle_ui_action(UiAction::RevealExport(shown));
            app.handle_ui_action(UiAction::OpenExport(shown));
            assert_eq!(app.tabs.len(), count);
            app.palette_open = false;
            app.grid_open = false;
            app.export_error = None;
        }
        assert!(
            !events
                .try_iter()
                .any(|event| matches!(event, AppEvent::FileRevealed(_)))
        );
    }

    #[test]
    fn image_and_playback_tab_retention_keep_notice_and_target_together() {
        let Some(root) = crate::tests::isolated_test_root(
            "export_notice::tests::image_and_playback_tab_retention_keep_notice_and_target_together",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("app");
        let target = root.join("result.png");
        app.path = Some(root.join("source.jpg"));
        app.media_kind = Some(MediaKind::Image);
        let shown = notice(&mut app, &target);
        let saved = app.take_image_tab_state();
        assert!(app.status_message.is_none() && app.export_notice.is_none());
        app.restore_image_tab(saved);
        assert_eq!(app.export_notice_target(shown), Some(target.as_path()));
        app.media_kind = Some(MediaKind::Video);
        app.media_duration = Some(Duration::from_secs(1));
        app.state = PlaybackState::Paused;
        let saved = app.take_playback_tab_state();
        assert!(app.status_message.is_none() && app.export_notice.is_none());
        app.restore_playback_tab(saved, false);
        assert_eq!(app.export_notice_target(shown), Some(target.as_path()));
    }
}
