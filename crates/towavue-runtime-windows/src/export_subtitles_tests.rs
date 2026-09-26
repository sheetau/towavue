use super::*;
use towavue_core::{SubtitleCue, SubtitleTimeline, TimeRange, TimelineEdit};

fn ms(value: i64) -> MediaTime {
    MediaTime::from_nanoseconds(value * 1_000_000)
}
fn range(a: i64, b: i64) -> TimeRange {
    TimeRange::new(ms(a), ms(b)).expect("range")
}
fn edits() -> Vec<EditOperation> {
    vec![
        EditOperation::SetTrimStart(ms(1000)),
        EditOperation::SetTrimEnd(ms(7000)),
        EditOperation::Timeline(TimelineEdit::Delete(range(1000, 2000))),
        EditOperation::Timeline(TimelineEdit::Stretch(range(0, 1000), ms(2000))),
        EditOperation::SetRate(2.0),
    ]
}

#[test]
fn projection_clips_surviving_spans_and_flattens_overlap_at_absolute_millisecond_boundaries() {
    let document = SubtitleTimeline::new(vec![
        SubtitleCue::new(ms(500), ms(2500), SubtitleContent::Text("A & <B>".into())).expect("cue"),
        SubtitleCue::new(ms(2000), ms(4000), SubtitleContent::Text("Overlap".into())).expect("cue"),
        SubtitleCue::new(ms(6000), ms(7000), SubtitleContent::Text("Tail".into())).expect("cue"),
    ]);
    let timeline = EditTimeline::from_operations(ms(8000), &edits()).expect("timeline");
    let result =
        project(&document, &timeline, 2.0, true, &AtomicBool::new(false)).expect("projection");
    assert_eq!(
        result,
        "1\n00:00:00,000 --> 00:00:01,000\nA &amp; &lt;B&gt;\n\n2\n00:00:01,000 --> 00:00:01,500\nOverlap\n\n3\n00:00:02,500 --> 00:00:03,000\nTail\n\n"
    );
    let timeline = EditTimeline::from_operations(ms(8000), &[]).expect("uncut");
    let result =
        project(&document, &timeline, 1.0, true, &AtomicBool::new(false)).expect("overlap");
    assert!(result.contains("00:00:02,000 --> 00:00:02,500\nA &amp; &lt;B&gt;\nOverlap\n"));
    assert!(matches!(
        project(&document, &timeline, 1.0, true, &AtomicBool::new(true)),
        Err(ExportError::Cancelled)
    ));
}

fn fixture(name: &str) -> (PathBuf, PathBuf) {
    let root = super::super::audio_tests::root(name);
    let first = root.join("first.srt");
    let second = root.join("second.srt");
    fs::write(&first, "1\n00:00:00,500 --> 00:00:02,500\n日本語 &amp; &lt;tag&gt; &amp;lt;\n\n2\n00:00:02,000 --> 00:00:04,000\nOverlap\n\n3\n00:00:06,000 --> 00:00:07,000\nTail\n").expect("first SRT");
    fs::write(&second, "1\n00:00:01,000 --> 00:00:05,000\nSecond\n").expect("second SRT");
    let source = root.join("source.mkv");
    let result =
        crate::hidden_test_command(crate::media_tools::tool_path("ffmpeg.exe").expect("FFmpeg"))
            .args([
                "-v",
                "error",
                "-n",
                "-f",
                "lavfi",
                "-i",
                "color=size=64x48:rate=20:duration=8",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=44100:cl=stereo:d=8",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=48000:cl=stereo:d=8",
            ])
            .arg("-i")
            .arg(first)
            .arg("-i")
            .arg(second)
            .args([
                "-map",
                "0:v",
                "-map",
                "1:a",
                "-map",
                "2:a",
                "-map",
                "3:s",
                "-map",
                "4:s",
                "-c:v",
                "ffv1",
                "-c:a",
                "pcm_s16le",
                "-c:s",
                "srt",
                "-metadata:s:s:0",
                "language=jpn",
                "-metadata:s:s:0",
                "title=Main captions",
                "-metadata:s:s:1",
                "language=eng",
                "-disposition:s:0",
                "forced",
                "-disposition:s:1",
                "0",
                "-output_ts_offset",
                "5",
            ])
            .arg(&source)
            .output()
            .expect("generated captioned video");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    (root, source)
}

fn request(source: &Path, target: PathBuf, operations: Vec<EditOperation>) -> ExportRequest {
    ExportRequest {
        source: source.to_owned(),
        target,
        kind: MediaKind::Video,
        operations,
        hardware_encode: false,
    }
}

fn read(path: &Path) -> Vec<SubtitleDocument> {
    let input = ffmpeg::format::input(path).expect("output probe");
    let tracks = crate::subtitles::catalog(&input);
    assert_eq!(tracks.len(), 2, "{path:?}");
    assert_eq!(tracks[0].language.as_deref(), Some("jpn"));
    assert_eq!(tracks[1].language.as_deref(), Some("eng"));
    let subtitles = input
        .streams()
        .filter(|stream| stream.parameters().medium() == ffmpeg::media::Type::Subtitle)
        .collect::<Vec<_>>();
    // FFmpeg writes ISO track-kind roles in MP4, but not QuickTime MOV/3GP.
    // Those containers still retain text and language; do not invent a flag.
    if !matches!(
        path.extension().and_then(|value| value.to_str()),
        Some("mov" | "3gp")
    ) {
        assert!(
            subtitles[0]
                .disposition()
                .contains(ffmpeg::format::stream::Disposition::FORCED),
            "{path:?}"
        );
    }
    assert!(
        !subtitles[1]
            .disposition()
            .contains(ffmpeg::format::stream::Disposition::DEFAULT)
    );
    tracks
        .into_iter()
        .map(|track| {
            crate::read_subtitles(
                &MediaInput::new(path.to_owned()),
                Some(track.id),
                &crate::Cancellation::default(),
            )
            .expect("saved caption")
        })
        .collect()
}

fn active(document: &SubtitleDocument, time: i64) -> String {
    document
        .active(ms(time), SubtitleDelay::default())
        .filter_map(|(_, cue)| match cue.content() {
            SubtitleContent::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_boundaries(path: &Path, document: &SubtitleDocument, expected: &[(i64, i64)]) {
    use ffmpeg::Rescale;
    let mut input = ffmpeg::format::input(path).expect("video clock probe");
    let origin = crate::decode::input_origin(&input);
    let first = input
        .packets()
        .find_map(|(stream, packet)| {
            (stream.parameters().medium() == ffmpeg::media::Type::Video).then(|| {
                packet
                    .pts()
                    .expect("video PTS")
                    .rescale(stream.time_base(), (1, 1_000_000_000))
                    - origin * 1000
            })
        })
        .expect("video origin");
    assert_eq!(document.cues().len(), expected.len(), "{path:?}");
    for (cue, &(start, end)) in document.cues().iter().zip(expected) {
        for (actual, expected) in [(cue.start(), start), (cue.end(), end)] {
            let relative = actual.as_nanoseconds() - first;
            assert!(
                (relative - expected * 1_000_000).abs() <= 1_000_000,
                "subtitle/video boundary at {path:?}: {relative} ns vs {expected} ms"
            );
        }
    }
}

#[test]
fn common_containers_keep_two_text_tracks_origin_metadata_overlap_and_audio() {
    let (root, source) = fixture("subtitle-formats");
    for extension in ["mkv", "mp4", "mov", "webm", "3gp"] {
        let request = request(&source, root.join(format!("output.{extension}")), vec![]);
        export_media(&request).expect("caption export");
        let documents = read(&request.target);
        assert_boundaries(
            &request.target,
            &documents[0],
            &[(500, 2000), (2000, 2500), (2500, 4000), (6000, 7000)],
        );
        for (time, expected) in [
            (250, ""),
            (750, "日本語 & <tag> &lt;"),
            (2250, "日本語 & <tag> &lt;\nOverlap"),
            (3000, "Overlap"),
            (5000, ""),
            (6500, "Tail"),
            (7100, ""),
        ] {
            assert_eq!(
                active(&documents[0], time),
                expected,
                "{extension}: {time} ms"
            );
        }
        assert_eq!(active(&documents[1], 1500), "Second");
        assert_eq!(active(&documents[1], 5100), "");
        assert_eq!(
            crate::probe_audio_tracks(&request.target)
                .expect("audio retained")
                .tracks
                .len(),
            2
        );
    }
    assert!(!fs::read_dir(&root).expect("owned root").any(|entry| {
        entry
            .expect("entry")
            .file_name()
            .to_string_lossy()
            .starts_with(".towavue-export-")
    }));
    fs::remove_dir_all(root).expect("owned fixtures released");
}

#[test]
fn edited_exports_keep_only_surviving_cues_with_trim_delete_stretch_and_global_rate() {
    let (root, source) = fixture("subtitle-edits");
    for extension in ["mkv", "mp4"] {
        let request = request(&source, root.join(format!("timeline.{extension}")), edits());
        export_media(&request).expect("edited captions");
        let documents = read(&request.target);
        assert_boundaries(
            &request.target,
            &documents[0],
            &[(0, 1000), (1000, 1500), (2500, 3000)],
        );
        for (time, expected) in [
            (500, "日本語 & <tag> &lt;"),
            (1250, "Overlap"),
            (1600, ""),
            (2750, "Tail"),
            (3100, ""),
        ] {
            assert_eq!(
                active(&documents[0], time),
                expected,
                "{extension}: {time} ms"
            );
        }
        assert_eq!(active(&documents[1], 1500), "Second");
        assert_eq!(active(&documents[1], 2100), "");
        let request = self::request(
            &source,
            root.join(format!("trim.{extension}")),
            vec![
                EditOperation::SetTrimStart(ms(1000)),
                EditOperation::SetTrimEnd(ms(7000)),
                EditOperation::SetRate(2.0),
            ],
        );
        export_media(&request).expect("simple trimmed captions");
        let documents = read(&request.target);
        assert_eq!(active(&documents[0], 250), "日本語 & <tag> &lt;");
        assert_eq!(active(&documents[0], 625), "日本語 & <tag> &lt;\nOverlap");
        assert_eq!(active(&documents[0], 1000), "Overlap");
        assert_eq!(active(&documents[0], 2750), "Tail");
    }
    let request = request(
        &source,
        root.join("empty-captions.mkv"),
        vec![EditOperation::SetTrimStart(ms(7500))],
    );
    export_media(&request).expect("valid video with no surviving subtitle cues");
    assert!(
        crate::subtitles::catalog(&ffmpeg::format::input(&request.target).expect("video output"))
            .is_empty()
    );
    fs::remove_dir_all(root).expect("owned fixtures released");
}

#[test]
fn unsupported_containers_and_cancelled_preparation_do_not_leave_sidecars() {
    let (root, source) = fixture("subtitle-cleanup");
    let request = request(&source, root.join("output.avi"), vec![]);
    let streams = ExportStreams::probe(&request).expect("probe");
    let staging = StagedExport::new(&request.target).expect("staging");
    assert!(
        Prepared::prepare(&request, &streams, &staging, &AtomicBool::new(false))
            .expect("preview-only format")
            .tracks
            .is_none()
    );
    let request = ExportRequest {
        target: root.join("output.mkv"),
        ..request
    };
    assert!(matches!(
        Prepared::prepare(&request, &streams, &staging, &AtomicBool::new(true)),
        Err(ExportError::Cancelled)
    ));
    assert_eq!(
        fs::read_dir(&staging.directory)
            .expect("empty staging")
            .count(),
        0
    );
    let files;
    {
        let prepared = Prepared::prepare(&request, &streams, &staging, &AtomicBool::new(false))
            .expect("prepare");
        files = prepared.paths.clone();
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|path| path.exists()));
    }
    assert!(files.iter().all(|path| !path.exists()));
    drop(staging);
    fs::remove_dir_all(root).expect("owned fixtures released");
}

#[test]
fn bitmap_tracks_remain_preview_only_while_neighboring_text_tracks_are_retained() {
    let (root, source) = fixture("subtitle-bitmap-policy");
    let bitmap = root.join("captions.sup");
    crate::subtitles::tests::pgs(&bitmap, true);
    let mixed = root.join("mixed.mkv");
    let output =
        crate::hidden_test_command(crate::media_tools::tool_path("ffmpeg.exe").expect("FFmpeg"))
            .args(["-v", "error", "-n", "-copyts", "-i"])
            .arg(&source)
            .args(["-itsoffset", "5", "-i"])
            .arg(&bitmap)
            .args(["-map", "0", "-map", "1:s", "-c", "copy"])
            .arg(&mixed)
            .output()
            .expect("bitmap mux");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let input = ffmpeg::format::input(&mixed).expect("mixed probe");
    let tracks = crate::subtitles::catalog(&input);
    assert_eq!(tracks.len(), 3);
    let captions = crate::read_subtitles(
        &MediaInput::new(mixed.clone()),
        Some(tracks[2].id),
        &crate::Cancellation::default(),
    )
    .expect("bitmap preview");
    assert!(matches!(
        captions.cues()[0].content(),
        SubtitleContent::Bitmap(_)
    ));
    drop(input);
    let request = request(&mixed, root.join("output.mkv"), edits());
    export_media(&request).expect("text retention with bitmap neighbor");
    let captions = read(&request.target);
    assert_eq!(active(&captions[0], 500), "日本語 & <tag> &lt;");
    assert_eq!(active(&captions[1], 1500), "Second");
    fs::remove_dir_all(root).expect("owned mixed fixture released");
}
