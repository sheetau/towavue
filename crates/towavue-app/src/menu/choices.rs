use super::*;
use towavue_core::RepeatMode;
use towavue_runtime_windows::VideoExportQuality;

pub(crate) struct Choices {
    pub volume_step: u8,
    pub video_quality: VideoExportQuality,
    pub audio_repeat: RepeatMode,
    pub folder_loop: bool,
}

impl Default for Choices {
    fn default() -> Self {
        Self {
            volume_step: 2,
            video_quality: VideoExportQuality::High,
            audio_repeat: RepeatMode::Off,
            folder_loop: true,
        }
    }
}

type Options = (&'static str, &'static [(CommandId, &'static str)]);

pub(super) fn options(command: CommandId) -> Option<Options> {
    Some(match command {
        ExportQualityHigh => (
            "Export quality",
            &[
                (ExportQualityHigh, "High quality"),
                (ExportQualityBalanced, "Balanced"),
                (ExportQualitySmaller, "Smaller file"),
            ],
        ),
        CycleVolumeStep => (
            "Listening volume step",
            &[
                (VolumeStepTwo, "2%"),
                (VolumeStepFive, "5%"),
                (VolumeStepTen, "10%"),
            ],
        ),
        CycleAudioRepeat => (
            "Audio repeat",
            &[
                (AudioRepeatOff, "Off"),
                (AudioRepeatAll, "All"),
                (AudioRepeatOne, "One"),
            ],
        ),
        FolderNavigationStop => (
            "Folder navigation",
            &[
                (FolderNavigationStop, "Stop at ends"),
                (FolderNavigationLoop, "Loop at ends"),
            ],
        ),
        _ => return None,
    })
}

fn row_width(ui: &egui::Ui, rows: &[(CommandId, &str)]) -> f32 {
    rows.iter()
        .map(|(_, label)| {
            egui::WidgetText::from(*label)
                .into_galley(
                    ui,
                    Some(egui::TextWrapMode::Extend),
                    f32::INFINITY,
                    egui::TextStyle::Button,
                )
                .size()
                .x
        })
        .fold(0.0, f32::max)
        + ui.spacing().icon_width
        + ui.spacing().icon_spacing
        + 2.0 * ui.spacing().button_padding.x
}

pub(super) fn reserve_cascade(
    ui: &mut egui::Ui,
    groups: &[&[CommandId]],
    context: CommandContext,
    ancestor: Option<egui::Rect>,
) {
    let child = groups
        .iter()
        .flat_map(|group| group.iter())
        .filter(|id| {
            **id != ExportQualityHigh || context.media_kind == Some(towavue_core::MediaKind::Video)
        })
        .filter_map(|id| options(*id))
        .map(|(_, rows)| row_width(ui, rows))
        .fold(0.0, f32::max);
    if child == 0.0 {
        return;
    }
    let frame = egui::Frame::popup(ui.style()).total_margin().sum().x;
    // A parent anchored near either screen edge can flip its alignment. Reserve
    // a child on both sides so neither alignment can strand a truncated submenu.
    let available = ui.ctx().content_rect().width()
        - ancestor.map_or(0.0, |rect| rect.width())
        - 2.0 * (child + frame + 2.0)
        - frame
        - 8.0;
    ui.set_max_width(ui.available_width().min(available.max(1.0)));
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
}

pub(super) fn submenu(
    ui: &mut egui::Ui,
    command: CommandId,
    context: CommandContext,
    choices: &Choices,
    requested: Option<egui::Id>,
    ancestor: Option<egui::Rect>,
) -> Option<(egui::Response, Option<CommandId>)> {
    let (title, rows) = options(command)?;
    let selected = match command {
        ExportQualityHigh => match choices.video_quality {
            VideoExportQuality::High => ExportQualityHigh,
            VideoExportQuality::Balanced => ExportQualityBalanced,
            VideoExportQuality::Smaller => ExportQualitySmaller,
        },
        CycleVolumeStep => match choices.volume_step {
            5 => VolumeStepFive,
            10 => VolumeStepTen,
            _ => VolumeStepTwo,
        },
        CycleAudioRepeat => match choices.audio_repeat {
            RepeatMode::Off => AudioRepeatOff,
            RepeatMode::All => AudioRepeatAll,
            RepeatMode::One => AudioRepeatOne,
        },
        FolderNavigationStop => {
            if choices.folder_loop {
                FolderNavigationLoop
            } else {
                FolderNavigationStop
            }
        }
        _ => return None,
    };
    let enabled = command_definitions()
        .iter()
        .find(|definition| definition.id == command)
        .expect("choice command")
        .is_enabled(context);
    let root = egui::containers::menu::find_menu_root(ui);
    let parent = ui.ctx().read_response(root.id).expect("parent menu").rect;
    let screen = ui.ctx().content_rect();
    let right = screen.right() - parent.right();
    let left = parent.left() - screen.left();
    let side = if ancestor.is_some_and(|root| root.right() <= parent.left()) {
        right
    } else {
        right.max(left)
    };
    let available = side - 2.0 - 1.0 / ui.ctx().pixels_per_point();
    let menu = ui
        .add_enabled_ui(enabled, |ui| {
            let category = ui.next_auto_id();
            if enabled && requested == Some(category) {
                let id = egui::containers::menu::SubMenu::id_from_widget_id(category);
                egui::containers::menu::MenuState::mark_shown(ui.ctx(), id);
                egui::containers::menu::MenuState::from_ui(ui, |state, _| {
                    state.open_item = Some(id)
                });
            }
            ui.menu_button(title, |ui| {
                let keyboard = MenuKeyboard::begin(ui);
                let back = keyboard.left;
                let frame = egui::Frame::popup(ui.style()).total_margin().sum().x;
                let width = row_width(ui, rows);
                // Child menus size from their own rows and the free cascade side,
                // independently of a directly opened or nested parent menu.
                ui.set_width(width.min((available - frame).max(1.0)));
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                let mut chosen = None;
                let mut items = Vec::new();
                for (id, label) in rows {
                    let mut checked = *id == selected;
                    let response = ui.checkbox(&mut checked, *label);
                    items.push(response.id);
                    if response.clicked() {
                        chosen = Some(*id);
                        ui.close();
                    }
                }
                keyboard.finish(ui, items);
                (chosen, back)
            })
        })
        .inner;
    let (chosen, back) = menu.inner.unwrap_or_default();
    if back {
        egui::containers::menu::MenuState::from_ui(ui, |state, _| state.open_item = None);
        menu.response.request_focus();
    }
    Some((menu.response, chosen))
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::accesskit::{Action, ActionRequest, TreeId};

    #[test]
    fn video_quality_submenu_does_not_reserve_or_show_controls_for_other_media() {
        for kind in [
            None,
            Some(towavue_core::MediaKind::Image),
            Some(towavue_core::MediaKind::Audio),
        ] {
            let context = crate::fonts::test_context();
            context.enable_accesskit();
            context.global_style_mut(crate::chrome::style);
            let frame = |events| {
                context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(480.0, 1100.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        ui.menu_button("Menu", |ui| {
                            show_section_with_recent(
                                ui,
                                CommandContext {
                                    media_kind: kind,
                                    ..Default::default()
                                },
                                &crate::shortcuts::defaults(),
                                Some(Section::File),
                                &mut MenuData::default(),
                            );
                        });
                    },
                )
            };
            let output = frame(vec![]);
            let id = output
                .platform_output
                .accesskit_update
                .as_ref()
                .expect("tree")
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some("Menu"))
                .expect("menu")
                .0;
            frame(vec![egui::Event::AccessKitActionRequest(ActionRequest {
                action: Action::Click,
                target_tree: TreeId::ROOT,
                target_node: id,
                data: None,
            })]);
            for _ in 0..4 {
                frame(vec![]);
            }
            let output = frame(vec![]);
            assert!(
                !output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .expect("tree")
                    .nodes
                    .iter()
                    .any(|(_, node)| node
                        .label()
                        .is_some_and(|text| text.contains("Export quality")))
            );
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::Text(text) if text.galley.text() == "Export current frame (PNG)" && !text.galley.elided)),
                "a hidden video submenu must not shrink the File menu");
        }
    }

    #[test]
    fn choice_submenus_check_current_values_dispatch_and_size_independently() {
        for density in [1.0, 1.25, 2.0] {
            for width in [480.0, 1200.0] {
                for (nested, right_edge) in [(false, false), (false, true), (true, false)] {
                    for (section, title, labels, selected, target, expected, kind) in [
                        (
                            Section::File,
                            "Export quality",
                            vec!["High quality", "Balanced", "Smaller file"],
                            "Balanced",
                            "Smaller file",
                            ExportQualitySmaller,
                            towavue_core::MediaKind::Video,
                        ),
                        (
                            Section::Edit,
                            "Listening volume step",
                            vec!["2%", "5%", "10%"],
                            "5%",
                            "10%",
                            VolumeStepTen,
                            towavue_core::MediaKind::Audio,
                        ),
                        (
                            Section::View,
                            "Audio repeat",
                            vec!["Off", "All", "One"],
                            "One",
                            "All",
                            AudioRepeatAll,
                            towavue_core::MediaKind::Audio,
                        ),
                        (
                            Section::View,
                            "Folder navigation",
                            vec!["Stop at ends", "Loop at ends"],
                            "Stop at ends",
                            "Loop at ends",
                            FolderNavigationLoop,
                            towavue_core::MediaKind::Video,
                        ),
                    ] {
                        let context = crate::fonts::test_context();
                        context.enable_accesskit();
                        context.set_pixels_per_point(density);
                        context.global_style_mut(crate::chrome::style);
                        let parent = std::cell::Cell::new(egui::Rect::NOTHING);
                        let frame = |events| {
                            let mut chosen = Vec::new();
                            let output = context.run_ui(
                                egui::RawInput {
                                    screen_rect: Some(egui::Rect::from_min_size(
                                        egui::Pos2::ZERO,
                                        egui::vec2(width, 1100.0),
                                    )),
                                    events,
                                    ..Default::default()
                                },
                                |ui| {
                                    ui.horizontal(|ui| {
                                        if right_edge {
                                            ui.add_space(width - 120.0);
                                        }
                                        ui.menu_button("Menu", |ui| {
                                            let mut data = MenuData {
                                                choices: Choices {
                                                    video_quality: VideoExportQuality::Balanced,
                                                    volume_step: 5,
                                                    audio_repeat: RepeatMode::One,
                                                    folder_loop: false,
                                                },
                                                ..Default::default()
                                            };
                                            let commands = CommandContext {
                                                media_kind: Some(kind),
                                                ..Default::default()
                                            };
                                            if let Some(command) = show_section_with_recent(
                                                ui,
                                                commands,
                                                &crate::shortcuts::defaults(),
                                                (!nested).then_some(section),
                                                &mut data,
                                            ) {
                                                chosen.push(command);
                                            }
                                        });
                                    });
                                },
                            );
                            if let Some(tree) = &output.platform_output.accesskit_update
                                && let Some((_, node)) = tree.nodes.iter().find(|(_, node)| {
                                    node.label().is_some_and(|label| {
                                        label.trim_end_matches('\u{23f5}').trim() == title
                                    })
                                })
                                && let Some(bounds) = node.bounds()
                            {
                                parent.set(egui::Rect::from_min_max(
                                    egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                                    egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
                                ));
                            }
                            (output, chosen)
                        };
                        let find = |output: &egui::FullOutput, label: &str| {
                            output
                                .platform_output
                                .accesskit_update
                                .as_ref()
                                .expect("tree")
                                .nodes
                                .iter()
                                .find(|(_, node)| {
                                    node.label().is_some_and(|text| {
                                        text.trim_end_matches('\u{23f5}').trim() == label
                                    })
                                })
                                .unwrap_or_else(|| {
                                    panic!(
                                        "missing {label}, {title}, width={width}, nested={nested}"
                                    )
                                })
                                .0
                        };
                        let click = |id| {
                            egui::Event::AccessKitActionRequest(ActionRequest {
                                action: Action::Click,
                                target_tree: TreeId::ROOT,
                                target_node: id,
                                data: None,
                            })
                        };
                        let mut triggers = vec!["Menu"];
                        if nested {
                            triggers.push(section.title());
                        }
                        triggers.push(title);
                        for label in triggers {
                            for _ in 0..4 {
                                assert!(frame(vec![]).1.is_empty());
                            }
                            let output = frame(vec![]).0;
                            let id = find(&output, label);
                            assert!(frame(vec![click(id)]).1.is_empty());
                        }
                        for _ in 0..4 {
                            frame(vec![]);
                        }
                        let output = frame(vec![]).0;
                        let tree = output
                            .platform_output
                            .accesskit_update
                            .as_ref()
                            .expect("choices");
                        for label in labels {
                            let node = &tree
                                .nodes
                                .iter()
                                .find(|(_, node)| node.label() == Some(label))
                                .expect("choice")
                                .1;
                            assert_eq!(
                                node.toggled(),
                                Some(if label == selected {
                                    egui::accesskit::Toggled::True
                                } else {
                                    egui::accesskit::Toggled::False
                                })
                            );
                            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                                egui::Shape::Text(text) if text.galley.text() == label && !text.galley.elided)),
                                "short choice label must be fully readable: {label}, width={width}, nested={nested}, right={right_edge}, parent={:?}, bounds={:?}, texts={:?}", parent.get(), node.bounds(), output.shapes.iter().filter_map(|shape| match &shape.shape { egui::Shape::Text(t) if t.galley.text().contains("ends") => Some((t.galley.text(),t.galley.size(),t.galley.elided)), _ => None }).collect::<Vec<_>>());
                            let bounds = node.bounds().expect("choice bounds");
                            assert!(
                                bounds.x0 >= 0.0 && bounds.x1 <= f64::from(width) + 1.0,
                                "choice stays in viewport: {bounds:?}"
                            );
                            let parent = parent.get();
                            assert!(
                                bounds.x1 <= f64::from(parent.left())
                                    || bounds.x0 >= f64::from(parent.right()),
                                "child cannot overlap parent at {width}/{density}/{nested}/{right_edge}: {parent:?}, {bounds:?}"
                            );
                            assert!(
                                bounds.width() < 180.0,
                                "short choices cannot inherit parent width"
                            );
                        }
                        let id = find(&output, target);
                        assert_eq!(frame(vec![click(id)]).1, vec![expected]);
                        assert!(frame(vec![]).1.is_empty(), "selection dispatches once");
                    }
                }
            }
        }
    }
}
