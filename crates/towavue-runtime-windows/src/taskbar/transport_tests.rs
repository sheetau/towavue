use super::*;
use std::cell::RefCell;
use std::time::{Duration, Instant};
use windows::Win32::UI::WindowsAndMessaging::SendMessageW;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::platform::windows::EventLoopBuilderExtWindows;

fn icons() -> TaskbarIcons {
    let image = vec![255; 32 * 32 * 4];
    TaskbarIcons::new(32, [&image; 4]).expect("generated icons")
}

#[test]
fn button_ids_tooltips_visibility_and_enabled_flags_follow_transport() {
    let icons = icons();
    let active = TaskbarTransport {
        context: 42,
        previous: false,
        play_pause: true,
        next: true,
        playing: false,
    };
    for transport in [
        None,
        Some(active),
        Some(TaskbarTransport {
            playing: true,
            ..active
        }),
    ] {
        let rows = buttons(transport, &icons);
        for (index, row) in rows.iter().enumerate() {
            assert_eq!(row.iId, BUTTON_BASE + index as u32);
            assert_eq!(row.dwMask, THB_ICON | THB_TOOLTIP | THB_FLAGS);
            assert_eq!(
                row.dwFlags,
                if transport.is_none() {
                    THBF_HIDDEN
                } else if index == 0 {
                    THBF_DISABLED
                } else {
                    THBF_ENABLED
                }
            );
            let title = String::from_utf16_lossy(&row.szTip)
                .trim_end_matches('\0')
                .to_owned();
            assert_eq!(
                title,
                match index {
                    0 => "Previous",
                    2 => "Next",
                    _ if transport.is_some_and(|t| t.playing) => "Pause",
                    _ => "Play",
                }
            );
        }
        assert_eq!(
            rows[1].hIcon,
            icons.handle(if transport.is_some_and(|t| t.playing) {
                2
            } else {
                1
            })
        );
    }
    assert!(TaskbarIcons::new(0, [&[]; 4]).is_err());
    assert!(TaskbarIcons::new(257, [&[]; 4]).is_err());
    assert!(TaskbarIcons::new(32, [&[]; 4]).is_err());
}

#[test]
#[ignore = "briefly shows an owned blank window; requires Explorer thumbnail toolbar support"]
fn visible_taskbar_registers_once_and_routes_only_current_enabled_buttons() {
    if std::env::var_os("TOWAVUE_TASKBAR_TRANSPORT_CHILD").is_none() {
        let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .args(["--exact", "taskbar::transport_tests::visible_taskbar_registers_once_and_routes_only_current_enabled_buttons", "--ignored", "--nocapture"])
            .env("TOWAVUE_TASKBAR_TRANSPORT_CHILD", "1").output().expect("isolated trial");
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    struct Trial {
        taskbar: Option<NativeTaskbar>,
        events: Rc<RefCell<Vec<TaskbarEvent>>>,
        deadline: Instant,
        completed: bool,
    }
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = Arc::new(
                event_loop
                    .create_window(
                        Window::default_attributes()
                            .with_title("towavue taskbar regression - no media")
                            .with_inner_size(winit::dpi::LogicalSize::new(320, 200))
                            .with_visible(false),
                    )
                    .expect("owned window"),
            );
            let events = self.events.clone();
            let mut taskbar =
                NativeTaskbar::new(window.clone(), move |event| events.borrow_mut().push(event))
                    .expect("taskbar");
            taskbar.set_icons(icons());
            self.taskbar = Some(taskbar);
            window.set_visible(true);
        }
        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            let taskbar = self.taskbar.as_mut().expect("taskbar");
            if taskbar.state.epoch.get() == 0 {
                // Exit normally on timeout: unwinding through an active native
                // event callback can leave winit processing its teardown messages.
                if Instant::now() >= self.deadline {
                    event_loop.exit();
                    return;
                }
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    Instant::now() + Duration::from_millis(16),
                ));
                return;
            }
            let mut transport = TaskbarTransport {
                context: 10,
                previous: false,
                play_pause: true,
                next: true,
                playing: false,
            };
            taskbar
                .set_transport(Some(transport))
                .expect("native registration");
            assert!(taskbar.buttons_added);
            let send = |handle, id, code| {
                // SAFETY: synchronous scalar message to this fixture's retained HWND.
                unsafe {
                    SendMessageW(
                        handle,
                        WM_COMMAND,
                        Some(WPARAM(((code as usize) << 16) | id as usize)),
                        None,
                    )
                };
            };
            for id in BUTTON_BASE..=BUTTON_BASE + 3 {
                send(taskbar.handle, id, THBN_CLICKED);
            }
            send(taskbar.handle, BUTTON_BASE + 1, 0);
            let clicks = || {
                self.events
                    .borrow()
                    .iter()
                    .copied()
                    .filter(|event| matches!(event, TaskbarEvent::Click { .. }))
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                clicks(),
                [
                    TaskbarEvent::Click {
                        context: 10,
                        action: TaskbarAction::PlayPause
                    },
                    TaskbarEvent::Click {
                        context: 10,
                        action: TaskbarAction::Next
                    }
                ]
            );
            transport.context = 11;
            transport.previous = true;
            transport.playing = true;
            taskbar
                .set_transport(Some(transport))
                .expect("native state/icon update");
            taskbar
                .set_transport(Some(transport))
                .expect("unchanged state");
            send(taskbar.handle, BUTTON_BASE, THBN_CLICKED);
            assert_eq!(
                clicks().last(),
                Some(&TaskbarEvent::Click {
                    context: 11,
                    action: TaskbarAction::Previous
                })
            );
            taskbar.set_transport(None).expect("hide for nonmedia tab");
            for id in BUTTON_BASE..=BUTTON_BASE + 2 {
                send(taskbar.handle, id, THBN_CLICKED);
            }
            assert_eq!(clicks().len(), 3);
            taskbar
                .set_transport(Some(transport))
                .expect("show through update, not second registration");
            taskbar
                .set_progress(TaskbarProgress::Fraction(500))
                .expect("export coexists with buttons");
            taskbar
                .set_progress(TaskbarProgress::Hidden)
                .expect("clear export");
            self.completed = true;
            self.taskbar = None;
            event_loop.exit();
        }
        fn window_event(
            &mut self,
            _: &ActiveEventLoop,
            _: winit::window::WindowId,
            _: WindowEvent,
        ) {
        }
    }
    let event_loop = EventLoop::builder()
        .with_any_thread(true)
        .build()
        .expect("event loop");
    let mut trial = Trial {
        taskbar: None,
        events: Rc::new(RefCell::new(Vec::new())),
        deadline: Instant::now() + Duration::from_secs(10),
        completed: false,
    };
    event_loop.run_app(&mut trial).expect("native event loop");
    assert!(trial.completed);
}
