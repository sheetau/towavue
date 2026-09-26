use super::*;
use std::sync::mpsc;
use towavue_core::{SubtitleCue, SubtitleTimeline};
use towavue_runtime_windows::SubtitleContent;

type TestApp = Application<Box<dyn Fn(AppEvent) + Send + Sync>>;

fn app(path: &Path) -> (TestApp, mpsc::Receiver<AppEvent>) {
    let (sender, receiver) = mpsc::channel();
    let mut app = Application::new(
        None,
        Box::new(move |event| {
            let _ = sender.send(event);
        }) as Box<dyn Fn(AppEvent) + Send + Sync>,
    )
    .expect("application");
    let tab = app.tabs.open_new(path.to_owned(), MediaKind::Video);
    app.path = Some(path.to_owned());
    app.media_kind = Some(MediaKind::Video);
    app.displayed_tab = Some(tab);
    app.state = PlaybackState::Paused;
    app.ui_context = Some(fonts::test_context());
    (app, receiver)
}

fn text_document(text: &str) -> Arc<SubtitleDocument> {
    Arc::new(SubtitleTimeline::new(vec![
        SubtitleCue::new(
            MediaTime::from_nanoseconds(1_000_000_000),
            MediaTime::from_nanoseconds(3_000_000_000),
            SubtitleContent::Text(text.into()),
        )
        .expect("cue"),
    ]))
}

fn complete(app: &mut TestApp, receiver: &mpsc::Receiver<AppEvent>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.subtitles.pending.is_some() {
        let event = receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("subtitle event");
        app.handle_app_event(event);
    }
}

#[test]
fn external_drop_loads_in_the_active_video_and_leaves_edits_transport_and_tabs_unchanged() {
    let Some(root) = crate::tests::isolated_test_root(
        "subtitles::tests::external_drop_loads_in_the_active_video_and_leaves_edits_transport_and_tabs_unchanged",
    ) else {
        return;
    };
    let path = root.join("captions.SRT");
    std::fs::write(&path, "1\n00:00:01,000 --> 00:00:03,000\nHello 世界\n").expect("SRT");
    let (mut app, receiver) = app(&root.join("video.mp4"));
    let tab = app.displayed_tab.expect("tab");
    let history = app.edits.clone();
    app.open_dropped_path(path.clone());
    assert!(app.subtitles.pending.is_some());
    complete(&mut app, &receiver);
    assert_eq!(app.tabs.tabs().len(), 1);
    assert_eq!(app.displayed_tab, Some(tab));
    assert_eq!(app.state, PlaybackState::Paused);
    let document = app
        .subtitle_choice()
        .expect("choice")
        .document
        .clone()
        .expect("decoded");
    assert_eq!(document.cues().len(), 1);
    let SubtitleContent::Text(text) = document.cues()[0].content() else {
        panic!("text")
    };
    assert_eq!(text, "Hello 世界");
    app.apply_subtitle_action(Action::Delay(SubtitleDelay::from_tenths(-3)));
    app.apply_subtitle_action(Action::Show(false));
    assert!(!app.subtitle_settings().visible);
    app.apply_subtitle_action(Action::Show(true));
    assert!(Arc::ptr_eq(
        &document,
        app.subtitle_choice()
            .expect("test fixture state")
            .document
            .as_ref()
            .expect("test fixture state")
    ));
    assert_eq!(app.subtitle_settings().delay.tenths(), -3);
    assert_eq!(app.edits, history);
    assert_eq!(app.state, PlaybackState::Paused);
    assert!(app.active_export.is_none());
    assert_eq!(
        std::fs::read_to_string(&path).expect("original caption"),
        "1\n00:00:01,000 --> 00:00:03,000\nHello 世界\n"
    );
    app.media_kind = Some(MediaKind::Image);
    app.open_dropped_path(path);
    assert_eq!(app.tabs.tabs().len(), 1);
    assert!(
        app.status_message
            .as_ref()
            .expect("test fixture state")
            .0
            .contains("video")
    );
}

#[test]
fn stale_picker_and_worker_deliveries_cannot_attach_to_another_tab_or_source() {
    let Some(root) = crate::tests::isolated_test_root(
        "subtitles::tests::stale_picker_and_worker_deliveries_cannot_attach_to_another_tab_or_source",
    ) else {
        return;
    };
    let (mut app, _) = app(&root.join("first.mp4"));
    let owner = app.subtitle_owner().expect("test fixture state");
    app.ensure_subtitle_choice(&owner).settings.selection = Selection::External;
    app.subtitles.pending = Some((7, owner.clone()));
    app.finish_subtitle_read(6, Ok(text_document("wrong revision")));
    assert!(
        app.subtitle_choice()
            .expect("test fixture state")
            .document
            .is_none()
    );
    app.media_generation += 1;
    app.finish_subtitle_read(7, Ok(text_document("wrong source generation")));
    assert!(
        app.subtitle_choice()
            .expect("test fixture state")
            .document
            .is_none()
    );
    app.finish_subtitle_picker(owner, Ok(Some(root.join("never-open.srt"))));
    assert!(app.subtitle_settings().external.is_none());
    app.update_subtitle_read();
    assert!(app.subtitles.pending.is_none());
    let second = app.tabs.open_new(root.join("second.mp4"), MediaKind::Video);
    app.displayed_tab = Some(second);
    app.path = Some(root.join("second.mp4"));
    assert_eq!(app.subtitle_settings(), Settings::default());
    app.finish_subtitle_read(
        7,
        Err(SubtitleError::Message(
            localization::Text::SubtitleInvalidData,
        )),
    );
    assert!(app.status_message.is_none());
    app.about_open = true;
    app.open_dropped_path(root.join("guarded.srt"));
    assert!(!app.subtitles.choices.contains_key(&second));
}

#[test]
fn subtitle_readers_quiesce_and_transferred_state_releases_its_pending_owner() {
    let Some(root) = crate::tests::isolated_test_root(
        "subtitles::tests::subtitle_readers_quiesce_and_transferred_state_releases_its_pending_owner",
    ) else {
        return;
    };
    let video = root.join("video.mp4");
    let (mut app, _) = app(&video);
    let owner = app.subtitle_owner().expect("test fixture state");
    let document = text_document("retained");
    let choice = app.ensure_subtitle_choice(&owner);
    choice.document = Some(Arc::clone(&document));
    choice.settings.delay = SubtitleDelay::from_tenths(4);
    let worker = LatestTask::new("subtitle-drain-test").expect("worker");
    let (ready_tx, ready_rx) = mpsc::channel();
    worker.submit(move |cancellation| {
        ready_tx.send(()).expect("test fixture state");
        let limit = Instant::now() + Duration::from_secs(5);
        while !cancellation.is_cancelled() && Instant::now() < limit {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            cancellation.is_cancelled(),
            "source relocation must cancel subtitle reads"
        );
    });
    ready_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("live reader");
    app.subtitles.worker = Some(worker);
    app.subtitles.pending = Some((2, owner.clone()));
    assert!(!app.source_readers_idle());
    app.quiesce_file_relocation(&video);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !app.subtitles.is_idle() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(app.source_readers_idle());
    assert!(app.subtitles.pending.is_none());
    let mut moved = app.subtitles.take_choice(owner.tab).expect("state payload");
    assert!(!app.subtitles.choices.contains_key(&owner.tab));
    moved.relocate(&video, &root.join("renamed.mp4"));
    assert_eq!(moved.path, root.join("renamed.mp4"));
    assert_eq!(moved.settings.delay.tenths(), 4);
    assert!(Arc::ptr_eq(
        moved.document.as_ref().expect("test fixture state"),
        &document
    ));
}

#[test]
fn subtitle_commands_are_video_only_and_allow_custom_shortcuts_without_editing() {
    for id in [CommandId::LoadSubtitles, CommandId::ToggleSubtitles] {
        let definition = towavue_core::command_definitions()
            .iter()
            .find(|definition| definition.id == id)
            .expect("command");
        for kind in [MediaKind::Image, MediaKind::Audio, MediaKind::Video] {
            let context = towavue_core::CommandContext {
                media_kind: Some(kind),
                ..Default::default()
            };
            assert_eq!(definition.is_enabled(context), kind == MediaKind::Video);
            assert!(!definition.is_enabled(towavue_core::CommandContext {
                playback_blocked: true,
                ..context
            }));
        }
    }
}
