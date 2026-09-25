use super::*;

#[test]
#[ignore = "briefly shows owned D3D11 windows; verifies placement startup and guarded multi-window shutdown"]
fn native_host_restores_only_initial_window_and_remembers_accepted_visible_closes() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::placement::tests::native_host_restores_only_initial_window_and_remembers_accepted_visible_closes",
    ) else {
        return;
    };
    use winit::platform::windows::EventLoopBuilderExtWindows;
    struct Trial {
        root: PathBuf,
        proxy: EventLoopProxy<Event>,
    }
    impl ApplicationHandler<Event> for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            for mode in ["normal", "maximized"] {
                let path = self.root.join(format!("{mode}-placement.conf"));
                std::fs::write(
                    &path,
                    format!("towavue window placement v1\n40\n40\n736\n488\n96\n{mode}\n"),
                )
                .expect("owned placement fixture");
                let mut host = WindowHost::new(None, Some(self.proxy.clone()))
                    .expect("owned placement fixture");
                assert!(
                    host.window_placement.is_none(),
                    "fixture never opens owner preferences"
                );
                host.window_placement = Some(
                    WindowPlacementPreferences::open(path.clone())
                        .expect("owned placement fixture"),
                );
                let first = *host.windows.keys().next().expect("owned placement fixture");
                host.restore_initial_placement(first);
                assert!(host.windows[&first].initial_window_placement.is_some());
                let second = host.add_application(None).expect("owned placement fixture");
                assert!(host.windows[&second].initial_window_placement.is_none());
                host.windows
                    .get_mut(&first)
                    .expect("owned placement fixture")
                    .start_on_device(event_loop, None, true)
                    .expect("owned placement fixture");
                let device = host.windows[&first]
                    .renderer
                    .as_ref()
                    .expect("owned placement fixture")
                    .graphics_device();
                host.windows
                    .get_mut(&second)
                    .expect("owned placement fixture")
                    .start_on_device(event_loop, Some(device), false)
                    .expect("owned placement fixture");
                let window = host.windows[&first]
                    .window
                    .as_ref()
                    .expect("owned placement fixture")
                    .clone();
                assert_eq!(window.is_visible(), Some(true));
                assert_eq!(window.is_maximized(), mode == "maximized");
                let saved = host.windows[&first]
                    .native_caption
                    .as_ref()
                    .expect("owned placement fixture")
                    .saved_placement()
                    .expect("owned placement fixture");
                assert_eq!(saved.maximized(), mode == "maximized");
                assert_eq!(
                    Some(saved),
                    host.window_placement
                        .as_ref()
                        .expect("owned placement fixture")
                        .initial()
                );
                assert_eq!(host.windows[&first].window_size, Some(window.inner_size()));
                assert_eq!(
                    host.windows[&first].surface_resize_pending,
                    mode == "maximized"
                );
                let app = host.windows.get_mut(&first).expect("visible owner");
                let tab = app
                    .tabs
                    .open_new(self.root.join("memory-only.png"), MediaKind::Image);
                let mut history = EditHistory::default();
                history.push(EditOperation::FlipHorizontal, MediaKind::Image);
                app.edits.insert(tab, history);
                app.request_guarded(GuardedAction::Exit);
                assert!(app.pending_guard.is_some() && !app.exit_requested);
                host.remove_closed();
                assert_eq!(host.windows.len(), 2);
                let app = host.windows.get_mut(&first).expect("guarded owner");
                app.resolve_guard(GuardDecision::Cancel);
                assert!(!app.exit_requested);
                app.edits.remove(&tab);
                let before = std::fs::read(&path).expect("owned placement fixture");
                host.remove_closed();
                assert_eq!(
                    host.windows.len(),
                    2,
                    "unapproved close cannot remove an owner"
                );
                assert_eq!(
                    std::fs::read(&path).expect("owned placement fixture"),
                    before
                );
                // A hidden failed destination must not supersede the accepted
                // visible owner, even when it has a newer key and was last active.
                host.last_active_window = Some(second);
                host.windows
                    .get_mut(&second)
                    .expect("owned placement fixture")
                    .exit_requested = true;
                host.windows
                    .get_mut(&first)
                    .expect("owned placement fixture")
                    .request_guarded(GuardedAction::Exit);
                assert!(host.windows[&first].exit_requested);
                host.remove_closed();
                assert!(host.windows.is_empty());
                drop(host);
                assert_eq!(
                    WindowPlacementPreferences::open(path)
                        .expect("owned placement fixture")
                        .initial(),
                    Some(saved)
                );
            }

            for simultaneous in [true, false] {
                let path = self.root.join(format!("multiple-{simultaneous}.conf"));
                let mut host = WindowHost::new(None, Some(self.proxy.clone()))
                    .expect("owned placement fixture");
                host.window_placement = Some(
                    WindowPlacementPreferences::open(path.clone())
                        .expect("owned placement fixture"),
                );
                let first = *host.windows.keys().next().expect("owned placement fixture");
                let second = host.add_application(None).expect("owned placement fixture");
                host.start_pending(event_loop, true);
                assert_eq!(host.windows.len(), 2);
                host.last_active_window = Some(first);
                let window = host.windows[&second]
                    .window
                    .as_ref()
                    .expect("owned placement fixture");
                let _ = window.request_inner_size(LogicalSize::new(540, 350));
                let first_saved = host.windows[&first]
                    .native_caption
                    .as_ref()
                    .expect("owned placement fixture")
                    .saved_placement()
                    .expect("owned placement fixture");
                let second_saved = host.windows[&second]
                    .native_caption
                    .as_ref()
                    .expect("owned placement fixture")
                    .saved_placement()
                    .expect("owned placement fixture");
                assert_ne!(first_saved, second_saved);
                host.windows
                    .get_mut(&first)
                    .expect("owned placement fixture")
                    .request_guarded(GuardedAction::Exit);
                if simultaneous {
                    host.windows
                        .get_mut(&second)
                        .expect("owned placement fixture")
                        .request_guarded(GuardedAction::Exit);
                }
                host.remove_closed();
                if !simultaneous {
                    assert_eq!(host.windows.len(), 1);
                    host.windows
                        .get_mut(&second)
                        .expect("owned placement fixture")
                        .request_guarded(GuardedAction::Exit);
                    host.remove_closed();
                }
                drop(host);
                assert_eq!(
                    WindowPlacementPreferences::open(path)
                        .expect("owned placement fixture")
                        .initial(),
                    Some(if simultaneous {
                        first_saved
                    } else {
                        second_saved
                    })
                );
            }
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let mut builder = EventLoop::<Event>::with_user_event();
    builder.with_any_thread(true);
    configure_mouse_input(&mut builder);
    let event_loop = builder.build().expect("owned placement fixture");
    let mut trial = Trial {
        root,
        proxy: event_loop.create_proxy(),
    };
    event_loop
        .run_app(&mut trial)
        .expect("owned placement fixture");
}
