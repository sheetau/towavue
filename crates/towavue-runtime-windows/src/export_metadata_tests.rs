use super::*;
use crate::export::audio_tests::{ffmpeg, fixture, pcm, root};
use std::os::windows::process::CommandExt;

fn edits(field: MetadataField, value: &str) -> MetadataExportOptions {
    let mut options = MetadataExportOptions::default();
    options.set(field, Some(value.into())).expect("valid text");
    options
}

fn request(source: &Path, target: &Path, kind: MediaKind) -> ExportRequest {
    ExportRequest {
        source: source.into(),
        target: target.into(),
        kind,
        operations: vec![],
        hardware_encode: false,
    }
}

#[test]
fn metadata_options_are_bounded_transactional_and_pass_literal_arguments() {
    let text = "日本語 = ; # \\\"\n-next-option";
    let mut options = edits(MetadataField::Title, text);
    assert_eq!(options.get(MetadataField::Title), Some(text));
    assert_eq!(
        options.arguments(),
        vec![
            "-metadata:g",
            &format!("title={text}"),
            "-metadata:s",
            &format!("title={text}")
        ]
    );
    for bad in ["x\0y".into(), "x".repeat(1025), "音".repeat(342)] {
        assert!(options.set(MetadataField::Title, Some(bad)).is_err());
        assert_eq!(options.get(MetadataField::Title), Some(text));
    }
    options
        .set(MetadataField::Title, Some(String::new()))
        .expect("remove");
    assert_eq!(options.get(MetadataField::Title), Some(""));
    options.set(MetadataField::Title, None).expect("keep");
    assert!(options.is_empty());
    for field in MetadataField::ALL.into_iter().take(4) {
        options
            .set(field, Some("x".repeat(1024)))
            .expect("bounded total");
    }
    let before = options.clone();
    assert!(options.set(MetadataField::Genre, Some("x".into())).is_err());
    assert_eq!(options, before);
    options
        .set(MetadataField::Title, None)
        .expect("restore keep frees space");
    options
        .set(MetadataField::Genre, Some("x".into()))
        .expect("space reused");
}

#[test]
fn metadata_title_set_remove_keep_round_trips_audio_formats_and_preserves_source() {
    let root = root("metadata-formats");
    let source = root.join("source.mkv");
    fixture(&source);
    let original = fs::read(&source).expect("original");
    let title = "試験 = \"title\" ; # \\ literal";
    for extension in ["wav", "flac", "mp3", "m4a", "ogg", "opus"] {
        let target = root.join(format!("set.{extension}"));
        let options = ExportOptions {
            output: ExportOutput::AudioOnly,
            metadata: edits(MetadataField::Title, title),
            ..Default::default()
        };
        let export = request(&source, &target, MediaKind::Video);
        export_media_with_options(&export, options.clone())
            .unwrap_or_else(|error| panic!("{extension}: {error}"));
        options
            .metadata
            .verify(&target)
            .expect("explicit title retained");
        let removed = root.join(format!("removed.{extension}"));
        let remove = edits(MetadataField::Title, "");
        export_media_with_options(
            &request(&target, &removed, MediaKind::Audio),
            ExportOptions {
                metadata: remove.clone(),
                ..Default::default()
            },
        )
        .expect("remove title");
        remove
            .verify(&removed)
            .expect("no title in container or streams");
        if matches!(extension, "wav" | "flac") {
            let baseline = root.join(format!("keep.{extension}"));
            export_media_with_output(
                &request(&source, &baseline, MediaKind::Video),
                ExportOutput::AudioOnly,
            )
            .expect("default copy");
            edits(MetadataField::Title, "Audio derivative fixture")
                .verify(&baseline)
                .expect("keep original title");
            assert_eq!(pcm(&target), pcm(&baseline), "metadata does not change PCM");
            assert_eq!(pcm(&removed), pcm(&baseline));
        }
    }
    assert_eq!(fs::read(&source).expect("original unchanged"), original);
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn metadata_all_fields_and_control_characters_round_trip_without_changing_video() {
    let root = root("metadata-fields");
    let source = root.join("source.mkv");
    fixture(&source);
    let target = root.join("set.mkv");
    let baseline = root.join("baseline.mkv");
    let mut options = MetadataExportOptions::default();
    for field in MetadataField::ALL {
        let value = match field {
            MetadataField::Track => "3/12".into(),
            MetadataField::Date => "2026-09-10".into(),
            _ => format!("{} 日本語\n\"\\ ;=#", field.key()),
        };
        options.set(field, Some(value)).expect("field value");
    }
    export_media_with_options(
        &request(&source, &target, MediaKind::Video),
        ExportOptions {
            metadata: options.clone(),
            ..Default::default()
        },
    )
    .expect("all fields");
    options.verify(&target).expect("all field values exact");
    export_media(&request(&source, &baseline, MediaKind::Video)).expect("baseline");
    let pixels = |path: &Path| {
        let output =
            Command::new(crate::media_tools::tool_path("ffmpeg.exe").expect("fixed FFmpeg"))
                .creation_flags(CREATE_NO_WINDOW)
                .args(["-v", "error", "-i"])
                .arg(path)
                .args([
                    "-map", "0:v:0", "-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1",
                ])
                .output()
                .expect("decode video");
        assert!(output.status.success());
        output.stdout
    };
    assert_eq!(pixels(&target), pixels(&baseline));
    assert_eq!(pcm(&target), pcm(&baseline));
    let mut remove = MetadataExportOptions::default();
    for field in MetadataField::ALL {
        remove
            .set(field, Some(String::new()))
            .expect("remove field");
    }
    let removed = root.join("removed.mkv");
    export_media_with_options(
        &request(&target, &removed, MediaKind::Video),
        ExportOptions {
            metadata: remove.clone(),
            ..Default::default()
        },
    )
    .expect("remove all selected fields");
    remove
        .verify(&removed)
        .expect("removed global and stream fields");
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn metadata_unsupported_output_cancel_and_image_rejection_preserve_existing_targets() {
    let root = root("metadata-protection");
    let source = root.join("source.mkv");
    fixture(&source);
    let original = fs::read(&source).expect("original");
    let target = root.join("existing.aac");
    let existing = b"existing target must survive";
    fs::write(&target, existing).expect("owned target");
    let options = ExportOptions {
        output: ExportOutput::AudioOnly,
        metadata: edits(MetadataField::Title, "must not be silently lost"),
        ..Default::default()
    };
    let request = request(&source, &target, MediaKind::Video);
    let error =
        export_media_with_options(&request, options.clone()).expect_err("ADTS cannot retain title");
    assert!(error.to_string().contains("did not retain"), "{error}");
    assert_eq!(fs::read(&target).expect("target"), existing);
    for (extension, field, value) in [
        ("wav", MetadataField::AlbumArtist, "unsupported RIFF tag"),
        ("m4a", MetadataField::Track, "003/012"),
    ] {
        let mut unsupported = request.clone();
        unsupported.target = root.join(format!("existing.{extension}"));
        fs::write(&unsupported.target, existing).expect("owned existing target");
        let error = export_media_with_options(
            &unsupported,
            ExportOptions {
                output: ExportOutput::AudioOnly,
                metadata: edits(field, value),
                ..Default::default()
            },
        )
        .expect_err("lost or reformatted requested value must not publish");
        assert!(
            error.to_string().contains("did not retain"),
            "{extension}: {error}"
        );
        assert_eq!(fs::read(&unsupported.target).expect("target"), existing);
    }
    for cancel_after_progress in [false, true] {
        let cancelled = AtomicBool::new(!cancel_after_progress);
        let error = export_options_cancellable(
            &request,
            options.clone(),
            &cancelled,
            &|_| {
                cancelled.store(true, Ordering::Relaxed);
            },
            &|_| {},
        )
        .expect_err("cancel");
        assert!(matches!(error, ExportError::Cancelled), "{error}");
        assert_eq!(fs::read(&target).expect("target"), existing);
    }
    let mut same = request.clone();
    same.target = source.clone();
    assert!(matches!(
        export_media_with_options(&same, options),
        Err(ExportError::SameAsSource)
    ));
    let image = root.join("source.png");
    ffmpeg(
        &["-f", "lavfi", "-i", "color=size=16x16", "-frames:v", "1"],
        &image,
    );
    let mut image_request = request.clone();
    image_request.source = image;
    image_request.kind = MediaKind::Image;
    assert!(
        export_media_with_options(
            &image_request,
            ExportOptions {
                metadata: edits(MetadataField::Title, "not connected"),
                ..Default::default()
            }
        )
        .is_err()
    );
    assert_eq!(fs::read(&target).expect("target"), existing);
    assert_eq!(fs::read(&source).expect("source"), original);
    assert!(fs::read_dir(&root).expect("owned root").all(|entry| {
        !entry
            .expect("entry")
            .file_name()
            .to_string_lossy()
            .starts_with(".towavue-export-")
    }));
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn metadata_composes_with_timeline_normalization_worker_and_selected_stream_overrides() {
    use towavue_core::{MediaTime, TimeRange, TimelineEdit};
    let root = root("metadata-composed");
    let raw = root.join("raw.mkv");
    fixture(&raw);
    let source = root.join("source.mkv");
    ffmpeg(
        &[
            "-i",
            raw.to_str().expect("fixture path"),
            "-map",
            "0",
            "-c",
            "copy",
            "-metadata",
            "artist=Keep this artist",
            "-metadata:s:a:1",
            "title=Selected audio title",
        ],
        &source,
    );
    let original = fs::read(&source).expect("original");
    assert!(
        edits(MetadataField::Title, "Audio derivative fixture")
            .verify(&source)
            .is_err(),
        "a matching global title cannot hide a conflicting stream title"
    );
    assert!(
        edits(MetadataField::Title, "").verify(&source).is_err(),
        "removal must reject a retained title"
    );
    assert!(
        edits(MetadataField::Album, "missing")
            .verify(&source)
            .is_err(),
        "a missing requested tag is not success"
    );
    let time = |ms: i64| MediaTime::from_nanoseconds(ms * 1_000_000);
    let range = |start, end| TimeRange::new(time(start), time(end)).expect("range");
    let mut export = request(&source, &root.join("baseline.avi"), MediaKind::Video);
    export.operations = vec![
        EditOperation::SetTrimStart(time(200)),
        EditOperation::SetTrimEnd(time(1000)),
        EditOperation::Timeline(TimelineEdit::Delete(range(200, 400))),
        EditOperation::Timeline(TimelineEdit::Stretch(range(0, 200), time(400))),
        EditOperation::Timeline(TimelineEdit::SetVolume(range(0, 400), 0.25)),
        EditOperation::SetVolume(0.5),
        EditOperation::SetRate(2.0),
        EditOperation::RotateClockwise,
    ];
    let mut options = ExportOptions {
        audio: AudioExportOptions {
            normalize_peak: true,
            channels: AudioChannels::Stereo,
        },
        ..Default::default()
    };
    export_media_with_options(&export, options.clone()).expect("normalized baseline");
    let baseline = pcm(&export.target);
    options.metadata = edits(MetadataField::Title, "One edited title");
    export.target = root.join("edited.avi");
    export_media_with_options(&export, options.clone()).expect("video metadata and timeline");
    assert_eq!(pcm(&export.target), baseline);
    edits(MetadataField::Artist, "Keep this artist")
        .verify(&export.target)
        .expect("unmodified field kept");
    options
        .metadata
        .verify(&export.target)
        .expect("global/stream title override");
    let mut streams = ExportStreams::probe(&export).expect("selected streams");
    streams.metadata = options.metadata.clone();
    for hardware in [false, true] {
        let args = ffmpeg_arguments(&export, hardware, &streams);
        for pair in options.metadata.arguments().as_chunks::<2>().0 {
            assert!(
                args.windows(2).any(|args| args == pair),
                "both encoders receive metadata"
            );
        }
    }
    options.output = ExportOutput::AudioOnly;
    export.target = root.join("derivative.wav");
    let (sender, receiver) = std::sync::mpsc::channel();
    let job = ExportJob::start_with_options(export.clone(), options.clone(), move |event| {
        sender.send(event).expect("receiver");
    })
    .expect("worker");
    let mut analyzed = false;
    let mut encoded = false;
    loop {
        match receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("export event")
        {
            ExportEvent::AnalyzingAudio(_) => {
                assert!(!encoded);
                analyzed = true;
            }
            ExportEvent::Progress(_) => {
                assert!(analyzed);
                encoded = true;
            }
            ExportEvent::Finished(result) => {
                result.expect("published derivative");
                break;
            }
        }
    }
    assert!(analyzed && encoded);
    drop(job);
    assert!(receiver.try_recv().is_err(), "one completion");
    assert_eq!(pcm(&export.target), baseline);
    options
        .metadata
        .verify(&export.target)
        .expect("derivative metadata");
    assert_eq!(fs::read(&source).expect("source"), original);
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}
