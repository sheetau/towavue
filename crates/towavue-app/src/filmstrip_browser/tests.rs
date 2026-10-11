use super::*;
use towavue_core::{FolderMediaItem, FolderSnapshotSource, ShellIdentity};

fn snapshot(folder: &Path) -> FolderSnapshot {
    FolderSnapshot {
        folder_identity: ShellIdentity::new(vec![42]),
        folder_path: folder.to_owned(),
        items: ["z10.jpg", "alpha.mp4", "z2.jpg", "sound.wav"]
            .into_iter()
            .enumerate()
            .map(|(index, name)| FolderMediaItem {
                identity: ShellIdentity::new(vec![index as u8]),
                path: folder.join(name),
                kind: MediaKind::from_path(Path::new(name)).expect("kind"),
            })
            .collect(),
        sort_columns: Vec::new(),
        source: FolderSnapshotSource::PersistedShellView,
        generation: 7,
        captured_at: SystemTime::UNIX_EPOCH,
    }
}

fn context(density: f32) -> Context {
    let context = crate::fonts::test_context();
    context.enable_accesskit();
    context.set_pixels_per_point(density);
    context.global_style_mut(|style| {
        chrome::style(style);
        style.animation_time = 0.0;
    });
    context
}

fn frame(
    context: &Context,
    browser: &mut Browser,
    size: Vec2,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, Option<PathBuf>) {
    let mut target = None;
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ui| {
            target = browser
                .show(ui.ctx(), ui.ctx().content_rect(), None, true)
                .or(target.take());
        },
    );
    (output, target)
}

fn node(output: &egui::FullOutput, label: &str) -> Rect {
    let (_, node) = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("tree")
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some(label))
        .unwrap_or_else(|| {
            panic!(
                "node {label}; available: {:?}",
                output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .expect("tree")
                    .nodes
                    .iter()
                    .filter_map(|(_, node)| node.label())
                    .collect::<Vec<_>>()
            )
        });
    let bounds = node.bounds().expect("bounds");
    Rect::from_min_max(
        egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
        egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
    )
}

fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}
fn click(
    context: &Context,
    browser: &mut Browser,
    size: Vec2,
    pos: egui::Pos2,
) -> (egui::FullOutput, Option<PathBuf>) {
    frame(
        context,
        browser,
        size,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    frame(
        context,
        browser,
        size,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    )
}

#[test]
fn browser_projection_preserves_shell_identity_order_and_caches_between_paints() {
    let folder = Path::new("C:/pictures/alpha");
    let source = snapshot(folder);
    let mut browser = Browser::default();
    browser.sync_folder(folder, &context(1.0));
    browser.query = "z".into();
    browser.filter = Some(MediaKind::Image);
    let (projected, changed) = browser.project(Some(&source));
    assert!(changed);
    let projected = projected.expect("projection");
    assert_eq!(
        projected.items,
        [source.items[0].clone(), source.items[2].clone()]
    );
    assert_eq!(projected.folder_identity, source.folder_identity);
    assert_eq!(
        projected.generation, source.generation,
        "drag ownership stays tied to the Shell snapshot"
    );
    assert!(Arc::ptr_eq(
        &projected,
        &browser.project(Some(&source)).0.expect("cached")
    ));
    browser.query = "alpha".into();
    assert!(
        browser
            .project(Some(&source))
            .0
            .expect("filename-only search")
            .items
            .is_empty()
    );
    assert_eq!(source.items.len(), 4);
    browser.sync_folder(Path::new("C:/other"), &context(1.0));
    assert!(browser.query.is_empty());
    assert_eq!(browser.filter, Some(MediaKind::Image));
    assert!(
        browser.worker.is_none(),
        "projection never creates a directory worker"
    );
}

fn cached_listing(
    browser: &mut Browser,
    path: &Path,
    folders: Vec<towavue_runtime_windows::BrowserFolder>,
) {
    browser.cache.push(Arc::new(FolderListing {
        path: path.to_owned(),
        folders,
        truncated: false,
        unavailable: false,
    }));
}

#[test]
fn browser_long_child_name_truncates_without_raising_or_separating_the_cascade() {
    let context = context(1.0);
    let mut browser = Browser::default();
    let folder = Path::new("C:/Parent/Current");
    browser.sync_folder(folder, &context);
    let branch = folder.join("branch");
    let long = branch.join("A very long child name ".repeat(50));
    cached_listing(
        &mut browser,
        folder,
        vec![towavue_runtime_windows::BrowserFolder {
            path: branch.clone(),
            has_children: true,
        }],
    );
    cached_listing(
        &mut browser,
        &branch,
        vec![towavue_runtime_windows::BrowserFolder {
            path: long.clone(),
            has_children: false,
        }],
    );
    let size = egui::vec2(480.0, 220.0);
    let mut output = egui::FullOutput::default();
    for _ in 0..4 {
        output = frame(&context, &mut browser, size, vec![]).0;
    }
    let opener = node(&output, "Subfolders: Current");
    click(&context, &mut browser, size, opener.center());
    for _ in 0..4 {
        output = frame(&context, &mut browser, size, vec![]).0;
    }
    let arrow = node(&output, "Subfolders: branch");
    click(&context, &mut browser, size, arrow.center());
    for _ in 0..4 {
        output = frame(&context, &mut browser, size, vec![]).0;
    }
    let child = node(&output, &crate::folder_navigation::folder_name(&long));
    assert!(child.top() >= opener.bottom());
    assert!(
        (child.top() - arrow.top()).abs() < 1.0,
        "one-row child stays row-aligned"
    );
    assert!(child.width() < size.x * 0.75);
    let text = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text.starts_with("A very long child") => {
                Some(text)
            }
            _ => None,
        })
        .expect("long child text");
    assert!(text.galley.elided);
    let parent = node(&output, "branch").union(arrow);
    let margin = egui::Frame::menu(&context.global_style())
        .total_margin()
        .sum()
        .x;
    let separation = if child.left() >= parent.right() {
        child.left() - parent.right()
    } else {
        parent.left() - child.right()
    };
    assert!(
        (separation - (margin - 1.0)).abs() < 1.0,
        "menu borders overlap by one point: {parent:?} {child:?}"
    );
}

#[test]
#[ignore = "hidden D3D11/WASAPI navigation control; generated silent media only"]
fn browser_native_navigation_stays_paused_until_explicit_play() {
    use crate::*;
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let Some(root) = tests::isolated_test_root(
        "filmstrip_browser::tests::browser_native_navigation_stays_paused_until_explicit_play",
    ) else {
        return;
    };
    let ffmpeg =
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg")).join("bin/ffmpeg.exe");
    for (name, source, codec) in [
        ("silent.wav", "anullsrc=r=48000:cl=stereo", "pcm_s16le"),
        ("silent.mp4", "color=red:s=64x64:r=30", "mpeg4"),
    ] {
        assert!(
            tests::hidden_command(&ffmpeg)
                .args([
                    "-v", "error", "-f", "lavfi", "-i", source, "-t", "2", "-c", codec
                ])
                .arg(root.join(name))
                .status()
                .expect("fixture")
                .success()
        );
    }
    struct Trial {
        root: PathBuf,
        complete: bool,
    }
    impl winit::application::ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
            let (send, receive) = std::sync::mpsc::channel();
            let mut app = Application::new(None, move |event| {
                let _ = send.send(event);
            })
            .expect("app");
            let window = Arc::new(
                event_loop
                    .create_window(winit::window::Window::default_attributes().with_visible(false))
                    .expect("hidden window"),
            );
            app.renderer = Some(FrameRenderer::new(window.as_ref()).expect("D3D11"));
            app.window = Some(window);
            app.ui_context = Some(fonts::test_context());
            tab_transfer::tests::install(
                &mut app,
                self.root.join("origin.bmp"),
                tab_transfer::tests::decoded(false),
            );
            for name in ["silent.wav", "silent.mp4"] {
                app.filmstrip_open = true;
                app.navigate_to_unchecked(self.root.join(name));
                let deadline = Instant::now() + Duration::from_secs(10);
                while app.session.is_none() {
                    while let Ok(event) = receive.try_recv() {
                        app.handle_app_event(event);
                    }
                    app.finish_folder_load();
                    assert!(
                        Instant::now() < deadline,
                        "session: {:?}",
                        app.playback_error
                    );
                    std::thread::sleep(Duration::from_millis(2));
                }
                assert!(app.filmstrip_open);
                assert_eq!(app.state, PlaybackState::Paused);
                let position = app.current_position();
                let until = Instant::now() + Duration::from_millis(200);
                while Instant::now() < until {
                    while let Ok(event) = receive.try_recv() {
                        app.handle_app_event(event);
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                assert_eq!(app.state, PlaybackState::Paused);
                assert_eq!(app.current_position(), position, "paused clock: {name}");
                app.close_filmstrip();
                app.toggle_pause();
                assert_eq!(app.state, PlaybackState::Playing, "explicit play: {name}");
            }
            self.complete = true;
            event_loop.exit();
        }
        fn window_event(
            &mut self,
            _: &winit::event_loop::ActiveEventLoop,
            _: winit::window::WindowId,
            _: winit::event::WindowEvent,
        ) {
        }
    }
    let mut trial = Trial {
        root,
        complete: false,
    };
    winit::event_loop::EventLoop::builder()
        .with_any_thread(true)
        .build()
        .expect("event loop")
        .run_app(&mut trial)
        .expect("trial");
    assert!(trial.complete);
}

#[test]
fn browser_menus_expand_after_loading_align_labels_and_block_address_input() {
    let context = context(1.0);
    let mut browser = Browser::default();
    let folder = Path::new("C:/Ancestor/Parent/Current");
    browser.sync_folder(folder, &context);
    browser.worker_failed = true;
    cached_listing(&mut browser, folder, vec![]);
    let size = egui::vec2(960.0, 600.0);
    let mut output = egui::FullOutput::default();
    for _ in 0..4 {
        output = frame(&context, &mut browser, size, vec![]).0;
    }
    assert!(
        !output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("tree")
            .nodes
            .iter()
            .any(|(_, node)| node.label() == Some("Subfolders: Current")),
        "leaf breadcrumb has no arrow"
    );
    let opener = node(&output, "Subfolders: Parent");
    let current = node(&output, "Current");
    let blank = egui::pos2(size.x - 260.0, current.center().y);
    click(&context, &mut browser, size, opener.center());
    for _ in 0..3 {
        frame(&context, &mut browser, size, vec![]);
    }
    cached_listing(
        &mut browser,
        folder.parent().expect("parent"),
        (0..30)
            .map(|index| towavue_runtime_windows::BrowserFolder {
                path: folder
                    .parent()
                    .expect("parent")
                    .join(format!("folder{index:02}")),
                has_children: index == 0,
            })
            .collect(),
    );
    for _ in 0..5 {
        output = frame(&context, &mut browser, size, vec![]).0;
    }
    let first = node(&output, "folder00");
    let last = node(&output, "folder23");
    assert!(
        last.bottom() - first.top() > 500.0,
        "loading height must not constrain the menu"
    );
    assert!(
        last.width() < 160.0,
        "short names produce a compact menu: {last:?}"
    );
    let text_x = |label: &str| {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == label => Some(text.pos.x),
                _ => None,
            })
            .expect("menu text")
    };
    assert!(
        (text_x("folder00") - text_x("folder01")).abs() < 0.1,
        "branch and leaf text align left"
    );
    output = frame(
        &context,
        &mut browser,
        size,
        vec![egui::Event::PointerMoved(current.center())],
    )
    .0;
    let current_node = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("tree")
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("Current"))
        .expect("current");
    assert!(
        current_node.1.is_disabled(),
        "open menu blocks breadcrumb hover and clicks"
    );
    assert!(
        click(&context, &mut browser, size, current.center())
            .1
            .is_none()
    );
    assert!(
        !egui::Popup::is_any_open(&context),
        "outside breadcrumb click dismisses the menu"
    );
    // The dismissing release is still owned by the popup. Let egui rebuild
    // enabled hit regions before starting the next independent gesture.
    for _ in 0..3 {
        frame(&context, &mut browser, size, vec![]);
    }
    click(&context, &mut browser, size, opener.center());
    frame(&context, &mut browser, size, vec![]);
    assert!(
        egui::Popup::is_any_open(&context),
        "menu reopened before outside click"
    );
    assert!(click(&context, &mut browser, size, blank).1.is_none());
    for _ in 0..3 {
        output = frame(&context, &mut browser, size, vec![]).0;
    }
    let filter = node(&output, localization::text(&context, Text::GalleryFilter));
    click(&context, &mut browser, size, filter.center());
    for _ in 0..4 {
        output = frame(&context, &mut browser, size, vec![]).0;
    }
    let all = node(&output, localization::text(&context, Text::GalleryAll));
    assert_eq!(
        context.layer_id_at(all.center()).expect("menu layer").order,
        egui::Order::Foreground
    );
    let dim = output
        .shapes
        .iter()
        .position(|shape| {
            matches!(&shape.shape,
        egui::Shape::Rect(rect) if rect.fill == Color32::from_black_alpha(191))
        })
        .expect("header dim");
    let menu_text = output.shapes.iter().position(|shape| matches!(&shape.shape,
        egui::Shape::Text(text) if text.galley.job.text == localization::text(&context, Text::GalleryAll))).expect("menu text");
    assert!(
        dim < menu_text,
        "header dim must paint beneath the filter popup"
    );
}

#[test]
fn browser_elided_ancestors_are_ordered_menu_targets_and_gutters_change_continuously() {
    let context = context(1.0);
    let mut browser = Browser::default();
    let folder = Path::new("C:/very-long-ancestor/another-long-parent/near-parent/Current");
    browser.sync_folder(folder, &context);
    cached_listing(&mut browser, folder, vec![]);
    let size = egui::vec2(480.0, 600.0);
    let mut output = egui::FullOutput::default();
    for _ in 0..4 {
        output = frame(&context, &mut browser, size, vec![]).0;
    }
    click(&context, &mut browser, size, node(&output, "…").center());
    for _ in 0..4 {
        output = frame(&context, &mut browser, size, vec![]).0;
    }
    let root = node(&output, "C:");
    let ancestor = node(&output, "very-long-ancestor");
    assert!(
        ancestor.top() >= HEADER_HEIGHT - 8.0 && root.top() >= ancestor.bottom(),
        "each parent appears below its child, with the root last"
    );
    assert_eq!(
        click(&context, &mut browser, size, ancestor.center()).1,
        Some(PathBuf::from("C:/very-long-ancestor"))
    );
    assert!(!egui::Popup::is_any_open(&context));
    let mut previous: Option<Rect> = None;
    for width in 280..1000 {
        output = frame(
            &context,
            &mut browser,
            egui::vec2(width as f32, 600.0),
            vec![],
        )
        .0;
        let address = node(&output, "Search Current");
        if let Some(previous) = previous {
            assert!((address.left() - previous.left()).abs() <= 1.01);
            assert!((address.width() - previous.width()).abs() <= 1.01);
        }
        previous = Some(address);
    }
}

#[test]
fn browser_header_has_only_search_input_and_preserves_breadcrumbs_on_blank_click() {
    for density in [1.0, 1.25, 2.0] {
        for width in [240.0, 480.0, 960.0] {
            let context = context(density);
            let mut browser = Browser::default();
            browser.sync_folder(Path::new("C:/Pictures"), &context);
            let size = egui::vec2(width, 400.0);
            let mut output = egui::FullOutput::default();
            for _ in 0..4 {
                output = frame(&context, &mut browser, size, vec![]).0;
            }
            let search = node(&output, "Search Pictures");
            assert!(search.width() <= 250.0);
            assert!(search.width() > 32.0);
            let inputs = output
                .platform_output
                .accesskit_update
                .as_ref()
                .expect("tree")
                .nodes
                .iter()
                .filter(|(_, node)| node.role() == egui::accesskit::Role::TextInput)
                .count();
            assert_eq!(inputs, 1, "only the search is editable");
            assert!(node(&output, "Pictures").left() < search.left());
            let blank = egui::pos2(search.left() - 6.0, search.center().y);
            assert!(click(&context, &mut browser, size, blank).1.is_none());
            assert!(
                frame(
                    &context,
                    &mut browser,
                    size,
                    vec![
                        egui::Event::Text("C:/new-place".into()),
                        key(egui::Key::Enter)
                    ]
                )
                .1
                .is_none()
            );
            assert_eq!(browser.folder, Path::new("C:/Pictures"));
            click(&context, &mut browser, size, search.center());
            for _ in 0..3 {
                frame(&context, &mut browser, size, vec![]);
            }
            frame(
                &context,
                &mut browser,
                size,
                vec![egui::Event::Text("mountain".into())],
            );
            assert_eq!(browser.query, "mountain");
            frame(&context, &mut browser, size, vec![key(egui::Key::Escape)]);
            assert!(browser.query.is_empty());
        }
    }
}

#[test]
fn browser_lazy_cascade_reaches_descendants_without_opening_empty_parent() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip_browser::tests::browser_lazy_cascade_reaches_descendants_without_opening_empty_parent",
    ) else {
        return;
    };
    std::fs::create_dir_all(root.join("empty/deep")).expect("nested folders");
    let context = context(1.0);
    let mut browser = Browser::default();
    browser.sync_folder(&root, &context);
    let size = egui::vec2(1160.0, 600.0);
    let mut output = egui::FullOutput::default();
    for _ in 0..4 {
        output = frame(&context, &mut browser, size, vec![]).0;
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !browser.cache.iter().any(|listing| listing.path == root) {
        assert!(
            std::time::Instant::now() < deadline,
            "initial arrow metadata"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
        output = frame(&context, &mut browser, size, vec![]).0;
    }
    let root_arrow = node(
        &output,
        &format!(
            "Subfolders: {}",
            crate::folder_navigation::folder_name(&root)
        ),
    );
    assert!(
        click(&context, &mut browser, size, root_arrow.center())
            .1
            .is_none()
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        output = frame(&context, &mut browser, size, vec![]).0;
        if browser.cache.iter().any(|listing| listing.path == root) {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "directory listing");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    for _ in 0..4 {
        output = frame(&context, &mut browser, size, vec![]).0;
    }
    let arrow = node(&output, "Subfolders: empty");
    assert!(
        click(&context, &mut browser, size, arrow.center())
            .1
            .is_none(),
        "arrow explores without replacing media"
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        output = frame(&context, &mut browser, size, vec![]).0;
        if browser
            .cache
            .iter()
            .any(|listing| listing.path == root.join("empty"))
        {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "nested listing");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    for _ in 0..4 {
        output = frame(&context, &mut browser, size, vec![]).0;
    }
    let deep = node(&output, "deep");
    assert_eq!(
        click(&context, &mut browser, size, deep.center()).1,
        Some(root.join("empty/deep"))
    );
    browser.hide(Some(&context));
    assert!(browser.cache.is_empty());
    assert!(
        browser
            .worker
            .as_ref()
            .expect("lazy worker")
            .result()
            .is_none()
    );
}

#[test]
fn browser_navigation_retains_tab_filmstrip_dirty_guards_and_rejects_stale_actions() {
    use crate::*;
    let Some(root) = tests::isolated_test_root(
        "filmstrip_browser::tests::browser_navigation_retains_tab_filmstrip_dirty_guards_and_rejects_stale_actions",
    ) else {
        return;
    };
    let a = root.join("a/first.bmp");
    let b = root.join("b/second.bmp");
    for path in [&a, &b] {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("folder");
        tab_transfer::tests::bitmap(path);
    }
    std::fs::create_dir(root.join("empty")).expect("empty");
    let mut app = Application::new(None, |_| {}).expect("app");
    app.ui_context = Some(context(1.0));
    let tab =
        tab_transfer::tests::install(&mut app, a.clone(), tab_transfer::tests::decoded(false));
    app.filmstrip_open = true;
    let settle = |app: &mut Application<_>| {
        let deadline = Instant::now() + Duration::from_secs(30);
        while app.pending_folder.is_some() {
            app.finish_folder_load();
            assert!(Instant::now() < deadline, "folder delivery");
            std::thread::sleep(Duration::from_millis(2));
        }
    };
    let pending_before = app
        .pending_folder
        .as_ref()
        .map(|(generation, _)| *generation);
    app.handle_ui_action(UiAction::FilmstripFolder(
        Some(tab),
        app.media_generation.wrapping_sub(1),
        root.join("b"),
    ));
    assert_eq!(
        app.pending_folder
            .as_ref()
            .map(|(generation, _)| *generation),
        pending_before
    );
    app.handle_ui_action(UiAction::FilmstripFolder(
        Some(tab),
        app.media_generation,
        root.join("empty"),
    ));
    settle(&mut app);
    assert_eq!(app.path.as_ref(), Some(&a));
    assert!(app.filmstrip_open);
    app.push_visual_edit(EditOperation::RotateClockwise);
    app.handle_ui_action(UiAction::FilmstripFolder(
        Some(tab),
        app.media_generation,
        root.join("b"),
    ));
    settle(&mut app);
    assert!(matches!(
        app.pending_guard,
        Some(GuardedAction::NavigateFromFolder(..))
    ));
    app.resolve_guard(GuardDecision::Cancel);
    assert_eq!(app.path.as_ref(), Some(&a));
    app.handle_ui_action(UiAction::FilmstripFolder(
        Some(tab),
        app.media_generation,
        root.join("b"),
    ));
    settle(&mut app);
    app.resolve_guard(GuardDecision::Discard);
    assert_eq!(app.path.as_ref(), Some(&b));
    assert_eq!(app.tabs.active_id(), Some(tab));
    assert!(app.filmstrip_open);
}

#[test]
#[ignore = "requires hidden hardware D3D11; optional Filmstrip browser readbacks"]
fn browser_header_reaches_gpu_at_supported_densities() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let Some(_root) = crate::tests::isolated_test_root(
        "filmstrip_browser::tests::browser_header_reaches_gpu_at_supported_densities",
    ) else {
        return;
    };
    struct Trial {
        complete: bool,
    }
    impl winit::application::ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
            let window = event_loop
                .create_window(winit::window::Window::default_attributes().with_visible(false))
                .expect("hidden window");
            let mut renderer = towavue_runtime_windows::FrameRenderer::new(&window).expect("D3D11");
            for density in [1.0, 1.25, 2.0] {
                for width in [480.0, 960.0] {
                    let context = context(density);
                    let size = egui::vec2(width, 220.0);
                    let pixels_w = (size.x * density) as u32;
                    let pixels_h = (size.y * density) as u32;
                    renderer
                        .resize_surface(pixels_w, pixels_h)
                        .expect("surface");
                    let mut browser = Browser::default();
                    browser
                        .sync_folder(Path::new("C:/Users/Viewer/Pictures/Travel 2026"), &context);
                    browser.cache.push(Arc::new(FolderListing {
                        path: browser.folder.clone(),
                        folders: vec![towavue_runtime_windows::BrowserFolder {
                            path: browser.folder.join("Mountains"),
                            has_children: false,
                        }],
                        truncated: false,
                        unavailable: false,
                    }));
                    for mode in 0..8 {
                        if mode == 5 {
                            browser.cache.clear();
                            let folder = browser.folder.clone();
                            cached_listing(
                                &mut browser,
                                &folder,
                                (0..30)
                                    .map(|index| towavue_runtime_windows::BrowserFolder {
                                        path: folder.join(format!("Folder {index:02}")),
                                        has_children: index == 0,
                                    })
                                    .collect(),
                            );
                        } else if mode == 6 {
                            browser.sync_folder(Path::new("C:/very-long-ancestor-with-an-extra-long-name/another-long-parent-with-an-extra-long-name/near-parent/Current"), &context);
                            let folder = browser.folder.clone();
                            cached_listing(&mut browser, &folder, vec![]);
                        } else if mode == 7 {
                            browser.sync_folder(
                                Path::new("C:/Users/Viewer/Pictures/Travel 2026"),
                                &context,
                            );
                            let folder = browser.folder.clone();
                            cached_listing(&mut browser, &folder, vec![]);
                        }
                        browser.query = if mode == 2 {
                            "mountain".into()
                        } else {
                            String::new()
                        };
                        browser.filter = (mode == 2).then_some(MediaKind::Image);
                        let initial =
                            frame(&context, &mut browser, size, vec![egui::Event::PointerGone]).0;
                        let hover = match mode {
                            3 => Some(node(&initial, "Travel 2026").center()),
                            4 => Some(node(&initial, "Subfolders: Travel 2026").center()),
                            _ => None,
                        };
                        renderer
                            .clear([0.18, 0.25, 0.32, 1.0])
                            .expect("initial background");
                        renderer
                            .render_ui(&context, initial)
                            .expect("initial UI textures");
                        if mode == 1 {
                            let output = frame(&context, &mut browser, size, vec![]).0;
                            let search = node(&output, "Search Travel 2026");
                            renderer
                                .render_ui(&context, output)
                                .expect("search textures");
                            let output = click(&context, &mut browser, size, search.center()).0;
                            renderer
                                .render_ui(&context, output)
                                .expect("focused search");
                        }
                        if mode >= 5 {
                            let (output, _) = frame(&context, &mut browser, size, vec![]);
                            let label = match mode {
                                5 => "Subfolders: Travel 2026",
                                6 => "…",
                                _ => localization::text(&context, Text::GalleryFilter),
                            };
                            // AccessKit bounds and injected events both use
                            // logical points; only the render target uses pixels.
                            let pos = node(&output, label).center();
                            renderer
                                .render_ui(&context, output)
                                .expect("menu anchor textures");
                            // The filter opens on press, while breadcrumbs open
                            // on release. Upload atlas deltas from both frames.
                            for pressed in [true, false] {
                                let (output, _) = frame(
                                    &context,
                                    &mut browser,
                                    size,
                                    vec![
                                        egui::Event::PointerMoved(pos),
                                        egui::Event::PointerButton {
                                            pos,
                                            button: egui::PointerButton::Primary,
                                            pressed,
                                            modifiers: egui::Modifiers::NONE,
                                        },
                                    ],
                                );
                                renderer.render_ui(&context, output).expect("menu textures");
                            }
                        }
                        for pass in 0..5 {
                            let output = frame(
                                &context,
                                &mut browser,
                                size,
                                hover
                                    .map(|pos| vec![egui::Event::PointerMoved(pos)])
                                    .unwrap_or_default(),
                            )
                            .0;
                            renderer.clear([0.18, 0.25, 0.32, 1.0]).expect("background");
                            renderer.render_ui(&context, output).expect("UI");
                            if pass == 4 {
                                if mode >= 5 {
                                    assert!(
                                        egui::Popup::is_any_open(&context),
                                        "menu {mode} at density {density}/{width}"
                                    );
                                }
                                let pixels =
                                    renderer.verification_surface_rgba().expect("readback");
                                assert_eq!(pixels.len(), (pixels_w * pixels_h * 4) as usize);
                                if let Some(directory) =
                                    std::env::var_os("TOWAVUE_REFERENCE_RENDER_DIR")
                                {
                                    std::fs::write(
                                        PathBuf::from(directory).join(format!(
                                            "browser-{mode}-{pixels_w}x{pixels_h}.rgba"
                                        )),
                                        pixels,
                                    )
                                    .expect("readback file");
                                }
                            }
                            renderer.present_surface().expect("Present");
                        }
                    }
                }
            }
            self.complete = true;
            event_loop.exit();
        }
        fn window_event(
            &mut self,
            _: &winit::event_loop::ActiveEventLoop,
            _: winit::window::WindowId,
            _: winit::event::WindowEvent,
        ) {
        }
    }
    let mut trial = Trial { complete: false };
    let mut builder = winit::event_loop::EventLoop::builder();
    builder.with_any_thread(true);
    builder
        .build()
        .expect("event loop")
        .run_app(&mut trial)
        .expect("trial");
    assert!(trial.complete);
}

#[test]
fn browser_search_retains_media_focus_and_filtered_tab_traversal_uses_paths() {
    use crate::*;
    let Some(root) = tests::isolated_test_root(
        "filmstrip_browser::tests::browser_search_retains_media_focus_and_filtered_tab_traversal_uses_paths",
    ) else {
        return;
    };
    let a = root.join("z10.bmp");
    let b = root.join("z2.bmp");
    tab_transfer::tests::bitmap(&a);
    tab_transfer::tests::bitmap(&b);
    let mut app = Application::new(None, |_| {}).expect("app");
    let context = context(1.0);
    app.ui_context = Some(context.clone());
    let tab =
        tab_transfer::tests::install(&mut app, a.clone(), tab_transfer::tests::decoded(false));
    app.filmstrip_open = true;
    let mut source = snapshot(&root);
    source.items[0].path = a.clone();
    source.items[2].path = b.clone();
    app.folder_snapshot = Some(source.clone());
    let size = egui::vec2(960.0, 400.0);
    let draw = |app: &mut Application<_>, events| {
        let mut actions = Vec::new();
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)),
                events,
                ..Default::default()
            },
            |ui| app.draw_filmstrip(ui.ctx(), ui.ctx().content_rect(), &mut actions),
        );
        assert!(actions.is_empty());
        output
    };
    let mut output = egui::FullOutput::default();
    for _ in 0..4 {
        output = draw(&mut app, vec![]);
    }
    assert!(
        (node(&output, "z10.bmp").center().y - size.y * 0.5).abs() < 1.0,
        "header must not move the strip below the media center"
    );
    let search = node(
        &output,
        &format!("Search {}", folder_navigation::folder_name(&root)),
    );
    let pos = search.center();
    for pressed in [true, false] {
        draw(
            &mut app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    draw(&mut app, vec![egui::Event::Text("z2".into())]);
    assert_eq!(app.filmstrip_browser.query, "z2");
    assert!(
        context.text_edit_focused(),
        "filtering must not steal editor focus"
    );
    assert_eq!(app.path.as_ref(), Some(&a));
    assert_eq!(
        app.filmstrip_browser
            .snapshot()
            .expect("projection")
            .items
            .iter()
            .map(|item| &item.path)
            .collect::<Vec<_>>(),
        vec![&b]
    );
    draw(&mut app, vec![key(egui::Key::Escape)]);
    assert!(app.filmstrip_browser.query.is_empty());
    assert!(app.filmstrip_open);
    app.filmstrip_browser.query = "z".into();
    app.filmstrip_browser.filter = Some(MediaKind::Image);
    draw(&mut app, vec![]);
    context.memory_mut(|memory| {
        if let Some(id) = memory.focused() {
            memory.surrender_focus(id);
        }
    });
    assert!(app.navigate_filmstrip_tab(&"Tab".parse().expect("key")));
    assert_eq!(app.path.as_ref(), Some(&b));
    assert_eq!(app.tabs.active_id(), Some(tab));
    assert!(app.filmstrip_open);
    app.folder_snapshot = Some(source.clone());
    app.filmstrip_browser.query = "z2".into();
    draw(&mut app, vec![]);
    let generation = app.media_generation;
    assert!(app.navigate_filmstrip_tab(&"Tab".parse().expect("key")));
    assert_eq!(
        app.media_generation, generation,
        "a single filtered item must not reload"
    );
    app.filmstrip_browser.query = "no-match".into();
    app.folder_snapshot = Some(source);
    draw(&mut app, vec![]);
    assert!(
        app.filmstrip_browser
            .snapshot()
            .expect("empty projection")
            .items
            .is_empty()
    );
    assert!(app.navigate_filmstrip_tab(&"Tab".parse().expect("key")));
    assert_eq!(app.path.as_ref(), Some(&b));
}
#[test]
fn folder_status_menus_size_to_their_current_message() {
    for density in [1.0, 1.25, 2.0] {
        let context = context(density);
        for label in ["Loading folders...", "No subfolders", "Folder unavailable"] {
            let mut measured = (0.0, 0.0);
            for _ in 0..4 {
                let _ = context.run_ui(egui::RawInput::default(), |ui| {
                    let opener = ui.button("Open");
                    let popup = egui::Popup::menu(&opener)
                        .style(chrome::menu_style)
                        .open(true)
                        .show(|ui| {
                            menu_message(ui, label);
                            measured.1 = egui::WidgetText::from(label)
                                .into_galley(
                                    ui,
                                    Some(egui::TextWrapMode::Extend),
                                    f32::INFINITY,
                                    egui::TextStyle::Button,
                                )
                                .size()
                                .x;
                        })
                        .expect("test fixture");
                    measured.0 = popup.response.rect.width();
                });
            }
            assert!(
                (measured.0 - measured.1 - 22.0).abs() <= 2.0 / density,
                "message width plus row padding and frame: {measured:?}"
            );
        }
    }
}
