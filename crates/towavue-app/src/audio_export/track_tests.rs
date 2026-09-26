use super::*;
use towavue_core::AudioTrackId;

fn tracks() -> Vec<AudioTrack> {
    ["Main", "Commentary"]
        .into_iter()
        .enumerate()
        .map(|(index, title)| AudioTrack {
            id: AudioTrackId::from_index(index + 1),
            title: Some(title.into()),
            language: None,
        })
        .collect()
}

#[test]
fn output_track_checkboxes_allow_subset_none_and_all_at_each_density() {
    use crate::localization::test_ui as ui;
    for density in [1.0, 1.25, 2.0] {
        for japanese in [false, true] {
            let context = if japanese {
                ui::japanese_context(density)
            } else {
                let context = fonts::test_context();
                context.set_pixels_per_point(density);
                context.enable_accesskit();
                context
            };
            let mut tabs = TabSet::default();
            let source = PathBuf::from("video.mkv");
            let mut dialog = AudioExportDialog {
                token: 1,
                tab: tabs.open_new(source.clone(), MediaKind::Video),
                source,
                kind: MediaKind::Video,
                generation: 1,
                options: AudioExportOptions::default(),
                tracks: Some(tracks()),
                retention: AudioTrackRetention::All,
                first_frame: true,
                focused_option: None,
            };
            let size = egui::vec2(640.0, 800.0);
            for (index, expected) in [
                (
                    0,
                    AudioTrackRetention::Selected(vec![AudioTrackId::from_index(2)]),
                ),
                (1, AudioTrackRetention::Selected(vec![])),
                (
                    0,
                    AudioTrackRetention::Selected(vec![AudioTrackId::from_index(1)]),
                ),
                (1, AudioTrackRetention::All),
            ] {
                let output = ui::settle(&context, size, |context| dialog.show(context));
                let label = audio_preview::track_label(language(&context), index, &tracks()[index]);
                ui::frame(
                    &context,
                    size,
                    vec![ui::action(&output, &label, None)],
                    |context| dialog.show(context),
                );
                assert_eq!(dialog.retention, expected);
            }
            let compact = egui::vec2(320.0, 240.0);
            let output = ui::settle(&context, compact, |context| dialog.show(context));
            ui::visible_button(
                &output,
                Text::ApplyOptions.in_language(language(&context)),
                compact,
                true,
            );
            ui::visible_button(
                &output,
                Text::Cancel.in_language(language(&context)),
                compact,
                true,
            );
        }
    }
}

pub(crate) fn choose<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    retention: AudioTrackRetention,
) {
    app.open_audio_export_options();
    let dialog = app.audio_export_dialog.as_mut().expect("options");
    dialog.tracks = Some(tracks());
    dialog.retention = retention;
    let token = dialog.token;
    app.finish_audio_export_options(token, Some(AudioExportOptions::default()));
}

#[test]
fn retained_original_restores_excluded_tracks_after_save_as_and_source_save() {
    let Some(root) = crate::tests::isolated_test_root(
        "audio_export::track_tests::retained_original_restores_excluded_tracks_after_save_as_and_source_save",
    ) else {
        return;
    };
    let source = root.join("source.mkv");
    let ffmpeg =
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg")).join("bin/ffmpeg.exe");
    let generated = crate::tests::hidden_command(ffmpeg)
        .args([
            "-v",
            "error",
            "-n",
            "-f",
            "lavfi",
            "-i",
            "color=size=64x48:rate=20:duration=1",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=300:duration=1",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=600:duration=1",
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "2:a",
            "-c:v",
            "ffv1",
            "-c:a",
            "pcm_s16le",
            "-metadata:s:a:0",
            "title=Main",
            "-metadata:s:a:1",
            "title=Commentary",
        ])
        .arg(&source)
        .output()
        .expect("generated tracks");
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let original = std::fs::read(&source).expect("original");
    let (sender, events) = std::sync::mpsc::channel();
    let mut app = Application::new(None, move |event| {
        let _ = sender.send(event);
    })
    .expect("app");
    let tab = app.tabs.open_new(source.clone(), MediaKind::Video);
    crate::source_save::tests::loaded(&mut app, tab, &source);
    app.path = Some(source.clone());
    app.displayed_tab = Some(tab);
    app.media_kind = Some(MediaKind::Video);
    app.state = PlaybackState::Paused;
    let subset = AudioTrackRetention::Selected(vec![AudioTrackId::from_index(2)]);
    choose(&mut app, subset.clone());
    assert_eq!(
        app.audio_selection(),
        towavue_core::AudioTrackSelection::Default
    );
    let target = root.join("saved.mkv");
    app.pending_dialog = Some(DialogIntent::Export {
        tab,
        source: source.clone(),
        kind: MediaKind::Video,
        generation: app.media_generation,
        output: ExportOutput::Media,
        continuation: None,
    });
    app.finish_test_dialog(Ok(Some(target.clone())));
    assert_eq!(
        app.active_export
            .as_ref()
            .expect("Save as job")
            .options
            .audio_tracks,
        subset
    );
    crate::audio_export_tests::drain_export(&mut app, &events);
    assert!(app.export_error.is_none(), "{:?}", app.export_error);
    let catalog = towavue_runtime_windows::probe_audio_tracks(&target).expect("saved catalog");
    assert_eq!(catalog.tracks.len(), 1);
    assert_eq!(catalog.tracks[0].title.as_deref(), Some("Commentary"));
    assert_eq!(app.audio_retention_for(tab, &target), subset);
    assert_eq!(
        app.audio_retention_for(tab, &source),
        AudioTrackRetention::All
    );
    for (selection, count) in [
        (AudioTrackRetention::All, 2),
        (AudioTrackRetention::Selected(vec![]), 0),
        (subset.clone(), 1),
    ] {
        choose(&mut app, selection.clone());
        assert!(app.save_source(None));
        assert_eq!(
            app.active_export
                .as_ref()
                .expect("Save job")
                .options
                .audio_tracks,
            selection
        );
        crate::audio_export_tests::drain_export(&mut app, &events);
        assert!(app.export_error.is_none(), "{:?}", app.export_error);
        assert_eq!(
            towavue_runtime_windows::probe_audio_tracks(&target)
                .expect("saved tracks")
                .tracks
                .len(),
            count
        );
        assert!(!app.edits[&tab].is_dirty());
    }
    app.open_audio_export_options();
    let dialog = app
        .audio_export_dialog
        .as_mut()
        .expect("cancelled selection");
    dialog.retention = AudioTrackRetention::All;
    let token = dialog.token;
    app.finish_audio_export_options(token, None);
    assert_eq!(app.audio_retention_for(tab, &target), subset);
    app.open_audio_export_options();
    let dialog = app.audio_export_dialog.as_mut().expect("stale selection");
    dialog.retention = AudioTrackRetention::All;
    dialog.generation = dialog.generation.wrapping_add(1);
    let token = dialog.token;
    app.finish_audio_export_options(token, Some(AudioExportOptions::default()));
    assert_eq!(app.audio_retention_for(tab, &target), subset);
    app.close_tab_unchecked(tab);
    assert!(!app.audio_export_tracks.contains_key(&tab));
    assert_eq!(std::fs::read(&source).expect("untouched source"), original);
}
