use super::*;
use crate::*;
use std::sync::mpsc;
use winit::platform::windows::EventLoopBuilderExtWindows;

#[test]
fn native_time_notification_refreshes_same_source_and_rejects_old_results() {
    let Some(root) = crate::tests::isolated_test_root(
        "status_file_details::native_tests::native_time_notification_refreshes_same_source_and_rejects_old_results",
    ) else {
        return;
    };
    struct Trial(PathBuf);
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = Arc::new(
                event_loop
                    .create_window(Window::default_attributes().with_visible(false))
                    .expect("hidden time notification window"),
            );
            let caption = NativeCaption::new(window).expect("caption");
            let path = self.0.join("source.bin");
            std::fs::write(&path, [0; 1024]).expect("source fixture");
            let (tx, rx) = mpsc::channel();
            let mut app = Application::new(None, move |event| {
                if matches!(event, AppEvent::StatusFileDetails(..)) {
                    let _ = tx.send(event);
                }
            })
            .expect("app");
            app.native_caption = Some(caption);
            let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
            app.path = Some(path.clone());
            app.media_kind = Some(MediaKind::Image);
            app.displayed_tab = Some(tab);
            app.refresh_status_file_details();
            app.handle_app_event(
                rx.recv_timeout(Duration::from_secs(5))
                    .expect("initial details"),
            );
            let original = app
                .status_file_details
                .get(app.status_file_source())
                .cloned()
                .expect("cache");
            let old_ticket = app.status_file_details.ticket;
            let original_source = app.status_file_source();
            let context = fonts::test_context();
            app.image = Some(
                ImagePresentation::from_decoded(
                    &context,
                    &path,
                    Arc::new(DecodedImage {
                        animation_plays: 0,
                        format: "test",
                        frames: vec![towavue_runtime_windows::DecodedImageFrame {
                            width: 2,
                            height: 2,
                            rgba: vec![255; 16],
                            delay: Duration::ZERO,
                        }],
                    }),
                )
                .expect("held image"),
            );
            app.image_handoff = app.take_navigation_handoff(MediaKind::Image);
            assert!(app.image_handoff.is_some());

            // A changed fixture makes a fresh query observable without changing
            // the machine's clock or time zone. Native conversion has separate
            // UTC/Tokyo/Pacific historical-DST controls.
            std::fs::write(&path, [0; 2048]).expect("changed size");
            std::fs::File::options()
                .write(true)
                .open(&path)
                .expect("source")
                .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(90_000))
                .expect("changed date");
            let expected = FileDetails::read(&path).expect("new details");
            assert_ne!(original.modified_local, expected.modified_local);
            for _ in 0..3 {
                app.native_caption
                    .as_ref()
                    .expect("caption")
                    .verification_time_settings_changed();
            }
            // A pre-notification completion can arrive before the requested paint.
            app.handle_app_event(AppEvent::StatusFileDetails(
                old_ticket,
                Some(original.clone()),
            ));
            assert_eq!(
                app.status_file_source(),
                original_source,
                "same source identity"
            );
            assert!(
                app.status_file_details
                    .get(app.status_file_source())
                    .is_none()
            );
            let held = app
                .image_handoff
                .as_ref()
                .expect("handoff")
                .file_details
                .as_ref()
                .expect("held size");
            assert_eq!(held.bytes, original.bytes);
            assert!(
                held.modified_local.is_none(),
                "no stale local date on held pixels"
            );
            let fresh_ticket = app.status_file_details.ticket;
            for _ in 0..20 {
                app.refresh_status_file_details();
                assert_eq!(
                    app.status_file_details.ticket, fresh_ticket,
                    "no per-frame query"
                );
            }
            app.handle_app_event(
                rx.recv_timeout(Duration::from_secs(5))
                    .expect("refreshed details"),
            );
            assert_eq!(
                app.status_file_details.get(app.status_file_source()),
                Some(&expected)
            );
            assert!(rx.try_recv().is_err(), "one coalesced request");
            app.image_handoff = None;
            assert!(
                app.status_details()
                    .iter()
                    .any(|field| field.contains(expected.modified_local.as_ref().expect("date")))
            );

            // A second notification expires the accepted result as well. Closing
            // before its completion must not resurrect the old source or date.
            app.native_caption
                .as_ref()
                .expect("caption")
                .verification_time_settings_changed();
            app.refresh_status_file_details();
            let pending = rx
                .recv_timeout(Duration::from_secs(5))
                .expect("second refresh");
            app.path = None;
            app.handle_app_event(pending);
            assert!(app.status_file_details.get(None).is_none());
            app.native_caption
                .as_ref()
                .expect("caption")
                .verification_time_settings_changed();
            app.refresh_status_file_details();
            assert!(rx.try_recv().is_err(), "no metadata query without a source");
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let event_loop = winit::event_loop::EventLoop::builder()
        .with_any_thread(true)
        .build()
        .expect("event loop");
    event_loop
        .run_app(&mut Trial(root))
        .expect("native notification trial");
}
