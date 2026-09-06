use std::path::{Path, PathBuf};

use egui::{Color32, RichText};
use towavue_core::{FolderSnapshot, MediaKind};

pub fn show(
    ui: &mut egui::Ui,
    snapshot: Option<&FolderSnapshot>,
    current: Option<&Path>,
) -> Option<PathBuf> {
    let mut chosen = None;
    let items: Vec<_> = snapshot
        .into_iter()
        .flat_map(|snapshot| snapshot.items_of_kind(MediaKind::Audio))
        .collect();
    egui::Frame::new().inner_margin(8).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        egui::ScrollArea::vertical()
            .id_salt("audio_playlist")
            .auto_shrink([false, false])
            .show_rows(ui, 32.0, items.len(), |ui, rows| {
                for index in rows {
                    let item = items[index];
                    let selected = current == Some(item.path.as_path());
                    let name = crate::display_name(&item.path);
                    let text = RichText::new(format!("{}. {name}", index + 1))
                        .size(14.0)
                        .color(Color32::from_gray(if selected { 240 } else { 150 }));
                    if ui
                        .add_sized(
                            egui::vec2(ui.available_width(), 32.0),
                            egui::Button::selectable(selected, (text, egui::Atom::grow()))
                                .truncate(),
                        )
                        .on_hover_ui(|ui| {
                            ui.set_max_width(
                                (ui.ctx().viewport_rect().width() - 32.0).clamp(1.0, 400.0),
                            );
                            ui.add(egui::Label::new(name).wrap());
                        })
                        .clicked()
                    {
                        chosen = Some(item.path.clone());
                    }
                }
            });
    });
    chosen
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use egui::{Event, Pos2, Rect, Shape, Vec2};
    use towavue_core::{FolderMediaItem, FolderSnapshotSource, ShellIdentity};

    use super::*;

    #[test]
    fn playlist_rows_preserve_order_and_offer_full_width_targets() {
        for width in [240.0, 960.0] {
            let context = egui::Context::default();
            context.global_style_mut(|style| style.interaction.tooltip_delay = 0.0);
            let mut snapshot = snapshot(10_000);
            snapshot.items[0].path = PathBuf::from("z-first.wav");
            snapshot.items[1].path = PathBuf::from("a-current.wav");
            snapshot.items[2].path = PathBuf::from(format!("{}.wav", "long-name-".repeat(40)));
            snapshot.items.insert(
                1,
                FolderMediaItem {
                    identity: ShellIdentity::new(vec![255]),
                    path: PathBuf::from("skip.png"),
                    kind: MediaKind::Image,
                },
            );
            let current = snapshot.items[2].path.as_path();
            let time = std::cell::Cell::new(0.0);
            let frame = |events| {
                time.set(time.get() + 0.2);
                let mut chosen = None;
                let output = context.run_ui(
                    egui::RawInput {
                        time: Some(time.get()),
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, 240.0))),
                        events,
                        ..Default::default()
                    },
                    |ui| chosen = show(ui, Some(&snapshot), Some(current)),
                );
                (output, chosen)
            };
            for _ in 0..4 {
                frame(vec![]);
            }
            let (output, chosen) = frame(vec![]);
            assert!(chosen.is_none());
            let texts = texts(&output);
            assert!(texts.len() < 12, "only visible rows should be laid out");
            assert_eq!(texts[0].galley.job.text, "1. z-first.wav");
            assert_eq!(texts[1].galley.job.text, "2. a-current.wav");
            assert_eq!(texts[1].pos.y - texts[0].pos.y, 32.0);
            assert_eq!(
                texts[0].galley.job.sections[0].format.color,
                Color32::from_gray(150)
            );
            assert_eq!(
                texts[1].galley.job.sections[0].format.color,
                Color32::from_gray(240)
            );
            assert_eq!(texts[2].galley.rows.len(), 1);
            assert!(texts[2].galley.elided);
            for text in &texts {
                assert!(text.pos.x < 30.0, "left aligned");
                assert!(text.pos.x + text.galley.size().x <= width - 8.0);
            }
            let pos = egui::pos2(width - 32.0, texts[1].pos.y + 7.0);
            frame(vec![Event::PointerMoved(pos)]);
            let mut actions = Vec::new();
            for pressed in [true, false] {
                actions.extend(
                    frame(vec![Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    }])
                    .1,
                );
            }
            assert_eq!(actions, [current.to_path_buf()]);
            let pos = egui::pos2(width - 32.0, texts[2].pos.y + 7.0);
            frame(vec![Event::PointerMoved(pos)]);
            let mut output = frame(vec![]).0;
            for _ in 0..5 {
                output = frame(vec![]).0;
            }
            let tooltip = self::texts(&output)
                .into_iter()
                .find(|text| text.galley.job.text == crate::display_name(&snapshot.items[3].path))
                .expect("tooltip contains the entire filename");
            assert!(tooltip.pos.x >= 0.0);
            assert!(tooltip.pos.x + tooltip.galley.size().x <= width);
            frame(vec![Event::PointerGone]);
            frame(vec![Event::PointerMoved(egui::pos2(width / 2.0, 30.0))]);
            frame(vec![Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -1600.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            }]);
            for _ in 0..40 {
                frame(vec![]);
            }
            let output = frame(vec![Event::PointerGone]).0;
            let texts = self::texts(&output);
            assert!(texts.len() < 12);
            let row = texts
                .iter()
                .find(|text| text.pos.y > 20.0 && text.pos.y < 180.0)
                .expect("scrolled row");
            let number: usize = row
                .galley
                .job
                .text
                .split('.')
                .next()
                .expect("row number prefix")
                .parse()
                .expect("numeric row prefix");
            assert!(number > 40, "scroll reaches later audio rows");
            let pos = egui::pos2(width - 32.0, row.pos.y + 7.0);
            frame(vec![Event::PointerMoved(pos)]);
            for pressed in [true, false] {
                if let Some(path) = frame(vec![Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }])
                .1
                {
                    assert_eq!(
                        path,
                        snapshot
                            .items_of_kind(MediaKind::Audio)
                            .nth(number - 1)
                            .expect("clicked audio item")
                            .path
                    );
                    actions.push(path);
                }
            }
            assert_eq!(actions.len(), 2);
        }
    }

    #[test]
    fn empty_playlist_has_no_rows_or_actions() {
        for snapshot in [None, Some(snapshot(0))] {
            let context = egui::Context::default();
            let output = context.run_ui(egui::RawInput::default(), |ui| {
                assert!(show(ui, snapshot.as_ref(), None).is_none());
            });
            assert!(texts(&output).is_empty());
        }
    }

    fn snapshot(count: usize) -> FolderSnapshot {
        FolderSnapshot {
            folder_identity: ShellIdentity::new(vec![1]),
            folder_path: PathBuf::from("playlist"),
            items: (0..count)
                .map(|index| FolderMediaItem {
                    identity: ShellIdentity::new(index.to_le_bytes().to_vec()),
                    path: PathBuf::from(format!("track-{index}.wav")),
                    kind: MediaKind::Audio,
                })
                .collect(),
            sort_columns: Vec::new(),
            source: FolderSnapshotSource::LiveExplorerView,
            generation: 1,
            captured_at: SystemTime::now(),
        }
    }

    fn texts(output: &egui::FullOutput) -> Vec<&egui::epaint::TextShape> {
        fn visit<'a>(shape: &'a Shape, result: &mut Vec<&'a egui::epaint::TextShape>) {
            match shape {
                Shape::Text(text) => result.push(text),
                Shape::Vec(shapes) => shapes.iter().for_each(|shape| visit(shape, result)),
                _ => {}
            }
        }
        let mut result = Vec::new();
        for shape in &output.shapes {
            visit(&shape.shape, &mut result);
        }
        result
    }
}
