use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use towavue_runtime_windows::{FileDetails, LatestTask};

use crate::AppEvent;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub path: PathBuf,
    pub instance: u64,
    pub snapshot: Option<(u64, SystemTime)>,
}

pub struct FileDetailsCache {
    worker: LatestTask,
    source: Option<Source>,
    ticket: u64,
    details: Option<FileDetails>,
}

impl FileDetailsCache {
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            worker: LatestTask::new("towavue-status-file-details")?,
            source: None,
            ticket: 0,
            details: None,
        })
    }

    pub fn update(&mut self, source: Option<Source>, notify: Arc<dyn Fn(AppEvent) + Send + Sync>) {
        if self.source == source {
            return;
        }
        self.ticket = self.ticket.wrapping_add(1);
        self.details = None;
        self.source = source;
        let Some(source) = &self.source else {
            self.worker.clear();
            return;
        };
        let path = source.path.clone();
        let ticket = self.ticket;
        self.worker.submit(move |cancellation| {
            if cancellation.is_cancelled() {
                return;
            }
            // Filesystem metadata may block; the runtime worker owns that wait, never the UI.
            let details = FileDetails::read(&path).ok();
            if !cancellation.is_cancelled() {
                notify(AppEvent::StatusFileDetails(ticket, details));
            }
        });
    }

    pub fn finish(&mut self, ticket: u64, details: Option<FileDetails>) -> bool {
        if self.source.is_none() || ticket != self.ticket {
            return false;
        }
        self.details = details;
        true
    }

    pub fn get(&self, source: Option<Source>) -> Option<&FileDetails> {
        (self.source == source)
            .then_some(self.details.as_ref())
            .flatten()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;
    use crate::{Application, fonts, format_size};

    #[test]
    fn reads_once_per_source_and_refreshes_failures_without_accepting_stale_results() {
        let Some(root) = crate::tests::isolated_test_root(
            "status_file_details::tests::reads_once_per_source_and_refreshes_failures_without_accepting_stale_results",
        ) else {
            return;
        };
        let path = root.join("source.bin");
        std::fs::write(&path, [0; 1024]).expect("source fixture");
        let mut source = Source {
            path: path.clone(),
            instance: 1,
            snapshot: Some((1, SystemTime::UNIX_EPOCH)),
        };
        let (tx, rx) = mpsc::channel();
        let notify: Arc<dyn Fn(AppEvent) + Send + Sync> = Arc::new(move |event| {
            let _ = tx.send(event);
        });
        let mut size = FileDetailsCache::new().expect("worker");
        let receive = || match rx.recv_timeout(Duration::from_secs(5)).expect("file size") {
            AppEvent::StatusFileDetails(ticket, bytes) => (ticket, bytes),
            _ => panic!("unexpected event"),
        };
        size.update(Some(source.clone()), Arc::clone(&notify));
        assert_eq!(
            size.get(Some(source.clone())).map(|details| details.bytes),
            None
        );
        let (ticket, bytes) = receive();
        assert_eq!(bytes.as_ref().map(|details| details.bytes), Some(1024));
        assert_eq!(
            bytes,
            Some(FileDetails::read(&path).expect("source details"))
        );
        let original_date = bytes.as_ref().expect("details").modified_local.clone();
        assert!(size.finish(ticket, bytes));
        std::fs::write(&path, [0; 2048]).expect("changed fixture");
        std::fs::File::options()
            .write(true)
            .open(&path)
            .expect("owned source")
            .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(90_000))
            .expect("changed file date");
        for _ in 0..20 {
            size.update(Some(source.clone()), Arc::clone(&notify));
            assert_eq!(size.ticket, ticket, "no per-frame query");
            assert_eq!(
                size.get(Some(source.clone())).map(|details| details.bytes),
                Some(1024)
            );
        }
        assert!(rx.try_recv().is_err());
        // Snapshot capture time matters even when the provider generation repeats.
        source.snapshot = Some((1, SystemTime::UNIX_EPOCH + Duration::from_secs(1)));
        assert_eq!(
            size.get(Some(source.clone())).map(|details| details.bytes),
            None
        );
        size.update(Some(source.clone()), Arc::clone(&notify));
        assert!(!size.finish(
            ticket,
            Some(FileDetails {
                bytes: 999,
                modified_local: Some("stale".into())
            })
        ));
        let (ticket, bytes) = receive();
        assert_eq!(bytes.as_ref().map(|details| details.bytes), Some(2048));
        assert_ne!(
            bytes.as_ref().expect("new details").modified_local,
            original_date
        );
        assert!(size.finish(ticket, bytes));

        source.path = root.join("missing.bin");
        size.update(Some(source.clone()), Arc::clone(&notify));
        let (missing_ticket, bytes) = receive();
        assert_eq!(bytes, None);
        assert!(size.finish(missing_ticket, bytes));
        size.update(Some(source.clone()), Arc::clone(&notify));
        assert_eq!(size.ticket, missing_ticket, "remember failed reads");
        std::fs::write(&source.path, []).expect("new empty file");
        source.instance += 1;
        size.update(Some(source.clone()), Arc::clone(&notify));
        let (ticket, bytes) = receive();
        assert_eq!(
            bytes.as_ref().map(|details| details.bytes),
            Some(0),
            "empty file is not an unknown size"
        );
        assert!(size.finish(ticket, bytes));
        size.update(None, Arc::clone(&notify));
        assert!(!size.finish(
            ticket,
            Some(FileDetails {
                bytes: 999,
                modified_local: Some("stale".into())
            })
        ));
        size.update(Some(source.clone()), Arc::clone(&notify));
        assert!(
            !size.finish(
                ticket,
                Some(FileDetails {
                    bytes: 999,
                    modified_local: Some("stale".into())
                })
            ),
            "close/reopen rejects old ticket"
        );
        let (ticket, bytes) = receive();
        assert!(size.finish(ticket, bytes));
        assert_eq!(size.get(Some(source)).map(|details| details.bytes), Some(0));
    }

    #[test]
    fn status_draw_uses_cached_size_and_event_delivery_checks_the_current_source() {
        let Some(root) = crate::tests::isolated_test_root(
            "status_file_details::tests::status_draw_uses_cached_size_and_event_delivery_checks_the_current_source",
        ) else {
            return;
        };
        let path = root.join("source.bin");
        std::fs::write(&path, [0; 4096]).expect("source fixture");
        let (tx, rx) = mpsc::channel();
        let mut app = Application::new(None, move |event| {
            if matches!(event, AppEvent::StatusFileDetails(..)) {
                let _ = tx.send(event);
            }
        })
        .expect("headless app");
        app.tabs
            .open_new(path.clone(), towavue_core::MediaKind::Image);
        app.path = Some(path.clone());
        app.media_kind = Some(towavue_core::MediaKind::Image);
        app.refresh_status_file_details();
        app.handle_app_event(rx.recv_timeout(Duration::from_secs(5)).expect("size event"));
        let original = app
            .status_file_details
            .get(app.status_file_source())
            .cloned()
            .expect("details");
        assert!(original.modified_local.is_some());
        std::fs::write(&path, [0; 8192]).expect("changed fixture");
        let context = fonts::test_context();
        let ticket = app.status_file_details.ticket;
        let output = context.run_ui(egui::RawInput::default(), |ui| {
            app.draw_ui(ui, &mut Vec::new());
        });
        assert_eq!(app.status_file_details.ticket, ticket);
        assert_eq!(
            app.status_file_details.get(app.status_file_source()),
            Some(&original)
        );
        assert!(
            output.shapes.iter().any(|shape| matches!(
                &shape.shape,
                egui::Shape::Text(text) if text.galley.text().contains(&format_size(4096))
            )),
            "rendering must not reread the changed file"
        );

        let old_ticket = app.status_file_details.ticket;
        app.media_generation += 1;
        app.handle_app_event(AppEvent::StatusFileDetails(
            old_ticket,
            Some(FileDetails {
                bytes: 999,
                modified_local: Some("stale".into()),
            }),
        ));
        assert_eq!(
            app.status_file_details
                .get(app.status_file_source())
                .map(|details| details.bytes),
            None
        );
        app.handle_app_event(
            rx.recv_timeout(Duration::from_secs(5))
                .expect("fresh size event"),
        );
        assert_eq!(
            app.status_file_details
                .get(app.status_file_source())
                .map(|details| details.bytes),
            Some(8192)
        );
        app.path = None;
        app.handle_app_event(AppEvent::StatusFileDetails(
            old_ticket,
            Some(FileDetails {
                bytes: 999,
                modified_local: Some("stale".into()),
            }),
        ));
        assert_eq!(
            app.status_file_details
                .get(app.status_file_source())
                .map(|details| details.bytes),
            None
        );
    }

    #[test]
    fn modified_date_is_available_for_all_media_and_stays_with_the_held_image() {
        let Some(root) = crate::tests::isolated_test_root(
            "status_file_details::tests::modified_date_is_available_for_all_media_and_stays_with_the_held_image",
        ) else {
            return;
        };
        use towavue_core::MediaKind;
        let context = fonts::test_context();
        let mut app = Application::new(None, |_| {}).expect("app");
        let path = root.join("original.png");
        let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
        app.path = Some(path.clone());
        app.displayed_tab = Some(tab);
        app.media_kind = Some(MediaKind::Image);
        app.refresh_status_file_details();
        let original = FileDetails {
            bytes: 4096,
            modified_local: Some("2024-02-29 12:34:56".into()),
        };
        assert!(
            app.status_file_details
                .finish(app.status_file_details.ticket, Some(original.clone()))
        );
        for kind in [MediaKind::Image, MediaKind::Video, MediaKind::Audio] {
            app.media_kind = Some(kind);
            for density in [1.0, 1.25, 2.0] {
                for width in [320.0, 480.0, 960.0] {
                    let context = fonts::test_context();
                    context.set_pixels_per_point(density);
                    // Applying a new zoom factor uses the previous viewport size for one pass.
                    let _ = context.run_ui(egui::RawInput::default(), |_| {});
                    let output = context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 320.0),
                            )),
                            ..Default::default()
                        },
                        |ui| {
                            assert_eq!(ui.ctx().viewport_rect().size(), egui::vec2(width, 320.0));
                            assert_eq!(ui.ctx().pixels_per_point(), density);
                            app.draw_status_bar(ui, &mut Vec::new(), &mut Vec::new());
                        },
                    );
                    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                        egui::Shape::Text(text) if text.galley.text().contains("Modified (local): 2024-02-29 12:34:56"))), "{kind:?}, {density}, {width}");
                    {
                        let position = output
                            .shapes
                            .iter()
                            .find_map(|shape| match &shape.shape {
                                egui::Shape::Text(text)
                                    if text.galley.text().contains("Modified (local)") =>
                                {
                                    if width == 320.0 {
                                        assert!(
                                            text.galley.elided,
                                            "exercise automatic truncation help"
                                        );
                                    } else if width == 960.0 {
                                        assert!(
                                            !text.galley.elided,
                                            "also exercise untruncated help"
                                        );
                                    }
                                    Some(
                                        text.visual_bounding_rect()
                                            .intersect(shape.clip_rect)
                                            .center(),
                                    )
                                }
                                _ => None,
                            })
                            .expect("file details label");
                        context.global_style_mut(|style| {
                            style.interaction.tooltip_delay = 0.0;
                            style.interaction.show_tooltips_only_when_still = false;
                        });
                        let mut copies = 0;
                        for time in [1.0, 1.1, 1.2] {
                            let hovered = context.run_ui(
                                egui::RawInput {
                                    time: Some(time),
                                    screen_rect: Some(egui::Rect::from_min_size(
                                        egui::Pos2::ZERO,
                                        egui::vec2(width, 320.0),
                                    )),
                                    events: vec![egui::Event::PointerMoved(position)],
                                    ..Default::default()
                                },
                                |ui| {
                                    app.draw_status_bar(ui, &mut Vec::new(), &mut Vec::new());
                                },
                            );
                            copies = hovered.shapes.iter().filter(|shape| matches!(&shape.shape,
                                egui::Shape::Text(text) if text.galley.text().contains("2024-02-29 12:34:56"))).count();
                        }
                        assert_eq!(copies, 2, "one status label and one complete tooltip");
                    }
                }
            }
        }
        app.media_kind = Some(MediaKind::Image);
        app.image = Some(
            crate::ImagePresentation::from_decoded(
                &context,
                &path,
                Arc::new(towavue_runtime_windows::DecodedImage {
                    animation_plays: 0,
                    format: "test",
                    frames: vec![towavue_runtime_windows::DecodedImageFrame {
                        width: 1,
                        height: 1,
                        rgba: vec![0, 0, 0, 255],
                        delay: Duration::ZERO,
                    }],
                }),
            )
            .expect("image"),
        );
        let held = app
            .take_navigation_handoff(MediaKind::Image)
            .expect("held original");
        assert_eq!(held.file_details, Some(original));
        app.image_handoff = Some(held);
        app.path = Some(root.join("next.png"));
        app.refresh_status_file_details();
        assert!(app.status_file_details.finish(
            app.status_file_details.ticket,
            Some(FileDetails {
                bytes: 8192,
                modified_local: Some("2026-09-14 00:00:00".into()),
            })
        ));
        let output = context.run_ui(egui::RawInput::default(), |ui| {
            app.draw_status_bar(ui, &mut Vec::new(), &mut Vec::new());
        });
        let text = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.text()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("2024-02-29 12:34:56"));
        assert!(!text.contains("2026-09-14 00:00:00"));
    }
}
