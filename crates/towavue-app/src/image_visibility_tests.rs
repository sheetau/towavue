use super::*;
use winit::platform::windows::EventLoopBuilderExtWindows;

#[test]
fn hidden_windows_suspend_image_animation_work() {
    visibility_trial(false);
}

#[test]
#[ignore = "briefly shows and minimizes an owned native window; no input injection or media playback"]
fn hidden_and_minimized_windows_suspend_image_animation_work() {
    visibility_trial(true);
}

fn visibility_trial(show: bool) {
    let test_name = if show {
        "image_visibility_tests::hidden_and_minimized_windows_suspend_image_animation_work"
    } else {
        "image_visibility_tests::hidden_windows_suspend_image_animation_work"
    };
    let Some(_root) = crate::tests::isolated_test_root(test_name) else {
        return;
    };
    struct Trial {
        show: bool,
    }
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = Arc::new(
                event_loop
                    .create_window(
                        Window::default_attributes()
                            .with_title("towavue animation visibility test")
                            .with_visible(false)
                            .with_active(false)
                            .with_inner_size(winit::dpi::PhysicalSize::new(320, 200)),
                    )
                    .expect("owned window"),
            );
            let mut app = Application::new(None, |_: AppEvent| {}).expect("app");
            let context = fonts::test_context();
            app.ui_context = Some(context.clone());
            app.window = Some(Arc::clone(&window));
            app.media_kind = Some(MediaKind::Image);
            app.state = PlaybackState::Paused;
            let decoded = Arc::new(DecodedImage {
                format: "test",
                frames: [10, 30]
                    .into_iter()
                    .map(|delay| towavue_runtime_windows::DecodedImageFrame {
                        width: 1,
                        height: 1,
                        rgba: vec![delay, 0, 0, 255],
                        delay: Duration::from_millis(u64::from(delay)),
                    })
                    .collect(),
            });
            let start = Instant::now() - Duration::from_secs(60);
            let mut image = ImagePresentation::from_decoded(
                &context,
                Path::new("animation.png"),
                Arc::clone(&decoded),
            )
            .expect("image");
            image.next_frame_at = Some(start + Duration::from_millis(10));
            let mut page = ImagePresentation::from_decoded(
                &context,
                Path::new("page.png"),
                Arc::clone(&decoded),
            )
            .expect("page");
            page.next_frame_at = image.next_frame_at;
            app.image = Some(image);
            app.reading_pages = vec![Ok(page)];
            let _ = context.tex_manager().write().take_delta();
            for minimized in [false, true] {
                if minimized && !self.show {
                    break;
                }
                if minimized {
                    window.set_visible(true);
                    window.set_minimized(true);
                    assert_eq!(window.is_minimized(), Some(true));
                } else {
                    assert_eq!(window.is_visible(), Some(false));
                }
                let deadline = app.image.as_ref().expect("image").next_frame_at;
                app.schedule();
                assert_eq!(
                    app.image.as_ref().expect("image").next_frame_at,
                    deadline,
                    "hidden animation must not advance"
                );
                assert_eq!(
                    app.reading_pages[0].as_ref().expect("page").next_frame_at,
                    deadline
                );
                assert!(
                    context.tex_manager().write().take_delta().set.is_empty(),
                    "no invisible texture updates"
                );
                assert!(
                    app.idle_wakeup(Instant::now()).is_none(),
                    "no invisible animation timer"
                );
            }
            // Visual suspension must not remove unrelated status/background deadlines.
            let now = Instant::now();
            app.status_message = Some(("status".into(), now));
            assert_eq!(app.idle_wakeup(now), Some(now + STATUS_MESSAGE_DURATION));
            app.status_message = None;
            if !self.show {
                event_loop.exit();
                return;
            }
            window.set_minimized(false);
            assert_eq!(window.is_visible(), Some(true));
            assert_eq!(window.is_minimized(), Some(false));
            let now = Instant::now();
            app.schedule();
            for image in app
                .image
                .iter()
                .chain(app.reading_pages.iter().filter_map(|p| p.as_ref().ok()))
            {
                let deadline = image.next_frame_at.expect("resumed animation");
                assert!(deadline > now);
                assert_eq!(
                    (deadline - start).as_millis() % 40,
                    if image.frame_index == 0 { 10 } else { 0 },
                    "retain the original animation phase"
                );
                assert!(Arc::ptr_eq(&image.decoded, &decoded), "no source reload");
            }
            assert!(app.idle_wakeup(now).is_some());
            window.set_visible(false);
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    EventLoop::builder()
        .with_any_thread(true)
        .build()
        .expect("event loop")
        .run_app(&mut Trial { show })
        .expect("visibility trial");
}
