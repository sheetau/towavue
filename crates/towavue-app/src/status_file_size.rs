use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use towavue_runtime_windows::LatestTask;

use crate::AppEvent;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub path: PathBuf,
    pub instance: u64,
    pub snapshot: Option<(u64, SystemTime)>,
}

pub struct FileSize {
    worker: LatestTask,
    source: Option<Source>,
    ticket: u64,
    bytes: Option<u64>,
}

impl FileSize {
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            worker: LatestTask::new("towavue-status-file-size")?,
            source: None,
            ticket: 0,
            bytes: None,
        })
    }

    pub fn update(&mut self, source: Option<Source>, notify: Arc<dyn Fn(AppEvent) + Send + Sync>) {
        if self.source == source {
            return;
        }
        self.ticket = self.ticket.wrapping_add(1);
        self.bytes = None;
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
            let bytes = path.metadata().ok().map(|metadata| metadata.len());
            if !cancellation.is_cancelled() {
                notify(AppEvent::StatusFileSize(ticket, bytes));
            }
        });
    }

    pub fn finish(&mut self, ticket: u64, bytes: Option<u64>) -> bool {
        if self.source.is_none() || ticket != self.ticket {
            return false;
        }
        self.bytes = bytes;
        true
    }

    pub fn bytes(&self, source: Option<Source>) -> Option<u64> {
        (self.source == source).then_some(self.bytes).flatten()
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
            "status_file_size::tests::reads_once_per_source_and_refreshes_failures_without_accepting_stale_results",
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
        let mut size = FileSize::new().expect("worker");
        let receive = || match rx.recv_timeout(Duration::from_secs(5)).expect("file size") {
            AppEvent::StatusFileSize(ticket, bytes) => (ticket, bytes),
            _ => panic!("unexpected event"),
        };
        size.update(Some(source.clone()), Arc::clone(&notify));
        assert_eq!(size.bytes(Some(source.clone())), None);
        let (ticket, bytes) = receive();
        assert_eq!(bytes, Some(1024));
        assert!(size.finish(ticket, bytes));
        std::fs::write(&path, [0; 2048]).expect("changed fixture");
        for _ in 0..20 {
            size.update(Some(source.clone()), Arc::clone(&notify));
            assert_eq!(size.ticket, ticket, "no per-frame query");
            assert_eq!(size.bytes(Some(source.clone())), Some(1024));
        }
        assert!(rx.try_recv().is_err());
        // Snapshot capture time matters even when the provider generation repeats.
        source.snapshot = Some((1, SystemTime::UNIX_EPOCH + Duration::from_secs(1)));
        assert_eq!(size.bytes(Some(source.clone())), None);
        size.update(Some(source.clone()), Arc::clone(&notify));
        assert!(!size.finish(ticket, Some(999)));
        let (ticket, bytes) = receive();
        assert_eq!(bytes, Some(2048));
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
        assert_eq!(bytes, Some(0), "empty file is not an unknown size");
        assert!(size.finish(ticket, bytes));
        size.update(None, Arc::clone(&notify));
        assert!(!size.finish(ticket, Some(999)));
        size.update(Some(source.clone()), Arc::clone(&notify));
        assert!(
            !size.finish(ticket, Some(999)),
            "close/reopen rejects old ticket"
        );
        let (ticket, bytes) = receive();
        assert!(size.finish(ticket, bytes));
        assert_eq!(size.bytes(Some(source)), Some(0));
    }

    #[test]
    fn status_draw_uses_cached_size_and_event_delivery_checks_the_current_source() {
        let Some(root) = crate::tests::isolated_test_root(
            "status_file_size::tests::status_draw_uses_cached_size_and_event_delivery_checks_the_current_source",
        ) else {
            return;
        };
        let path = root.join("source.bin");
        std::fs::write(&path, [0; 4096]).expect("source fixture");
        let (tx, rx) = mpsc::channel();
        let mut app = Application::new(None, move |event| {
            if matches!(event, AppEvent::StatusFileSize(..)) {
                let _ = tx.send(event);
            }
        })
        .expect("headless app");
        app.tabs
            .open_new(path.clone(), towavue_core::MediaKind::Image);
        app.path = Some(path.clone());
        app.media_kind = Some(towavue_core::MediaKind::Image);
        app.refresh_status_file_size();
        app.handle_app_event(rx.recv_timeout(Duration::from_secs(5)).expect("size event"));
        std::fs::write(&path, [0; 8192]).expect("changed fixture");
        let context = fonts::test_context();
        let ticket = app.status_file_size.ticket;
        let output = context.run_ui(egui::RawInput::default(), |ui| {
            app.draw_ui(ui, &mut Vec::new());
        });
        assert_eq!(app.status_file_size.ticket, ticket);
        assert!(
            output.shapes.iter().any(|shape| matches!(
                &shape.shape,
                egui::Shape::Text(text) if text.galley.text().contains(&format_size(4096))
            )),
            "rendering must not reread the changed file"
        );

        let old_ticket = app.status_file_size.ticket;
        app.media_generation += 1;
        app.handle_app_event(AppEvent::StatusFileSize(old_ticket, Some(999)));
        assert_eq!(app.status_file_size.bytes(app.status_file_source()), None);
        app.handle_app_event(
            rx.recv_timeout(Duration::from_secs(5))
                .expect("fresh size event"),
        );
        assert_eq!(
            app.status_file_size.bytes(app.status_file_source()),
            Some(8192)
        );
        app.path = None;
        app.handle_app_event(AppEvent::StatusFileSize(old_ticket, Some(999)));
        assert_eq!(app.status_file_size.bytes(app.status_file_source()), None);
    }
}
