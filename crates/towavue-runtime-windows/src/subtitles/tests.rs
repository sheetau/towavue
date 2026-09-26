use super::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "towavue-subtitles-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::create_dir(&root).expect("owned root");
    root
}

fn srt(path: &Path) {
    fs::write(path, "1\n00:00:02,250 --> 00:00:04,000\n<i>Hello</i>, 世界\nSecond line\n\n2\n00:00:03,500 --> 00:00:05,125\nOverlapping\n").expect("SRT");
}

fn read(path: &Path, track: Option<SubtitleTrackId>) -> SubtitleDocument {
    read_subtitles(
        &MediaInput::new(path.to_owned()),
        track,
        &Cancellation::default(),
    )
    .expect("subtitles")
}

fn assert_text(document: &SubtitleDocument) {
    let cues = document.cues();
    assert_eq!(cues.len(), 2);
    assert_eq!(cues[0].start().as_nanoseconds(), 2_250_000_000);
    assert_eq!(cues[0].end().as_nanoseconds(), 4_000_000_000);
    assert_eq!(cues[1].start().as_nanoseconds(), 3_500_000_000);
    assert_eq!(cues[1].end().as_nanoseconds(), 5_125_000_000);
    let SubtitleContent::Text(text) = cues[0].content() else {
        panic!("text cue");
    };
    assert_eq!(text, "Hello, 世界\nSecond line");
}

#[test]
fn external_srt_vtt_and_ass_keep_authored_times_unicode_overlap_and_plain_text() {
    let root = root("external");
    let srt_path = root.join("external.srt");
    srt(&srt_path);
    assert_text(&read(&srt_path, None));
    let vtt = root.join("external.vtt");
    fs::write(&vtt, "WEBVTT\n\nfirst\n00:02.250 --> 00:04.000 align:start\n<i>Hello</i>, 世界\nSecond line\n\n00:03.500 --> 00:05.125\nOverlapping\n").expect("VTT");
    assert_text(&read(&vtt, None));
    let ass = root.join("external.ass");
    let output =
        crate::hidden_test_command(crate::media_tools::tool_path("ffmpeg.exe").expect("FFmpeg"))
            .args(["-v", "error", "-n", "-i"])
            .arg(&srt_path)
            .arg(&ass)
            .output()
            .expect("ASS conversion");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let decoded = read(&ass, None);
    assert_eq!(decoded.cues()[0].start().as_nanoseconds(), 2_250_000_000);
    let SubtitleContent::Text(text) = decoded.cues()[0].content() else {
        panic!("ASS text");
    };
    assert_eq!(text, "Hello, 世界\nSecond line");
    assert_eq!(
        super::text::plain(
            "0,0,Default,,0,0,0,,{\\pos(1,2)\\i1}Text, comma\\N{\\p1}m 0 0 l 1 1{\\p0}after\\hword",
            true
        ),
        "Text, comma\nafter word"
    );
    fs::remove_dir_all(root).expect("all native readers released");
}

#[test]
fn embedded_tracks_follow_video_origin_and_reject_missing_or_non_subtitle_ids() {
    let root = root("embedded");
    let first = root.join("first.srt");
    let second = root.join("second.srt");
    srt(&first);
    fs::write(
        &second,
        "1\n00:00:01,000 --> 00:00:02,000\nOther language\n",
    )
    .expect("second track");
    let video = root.join("video.mkv");
    let output =
        crate::hidden_test_command(crate::media_tools::tool_path("ffmpeg.exe").expect("FFmpeg"))
            .args([
                "-v",
                "error",
                "-n",
                "-f",
                "lavfi",
                "-i",
                "color=size=64x48:rate=10:duration=8",
                "-i",
            ])
            .arg(&first)
            .arg("-i")
            .arg(&second)
            .args([
                "-map",
                "0:v",
                "-map",
                "1:s",
                "-map",
                "2:s",
                "-c:v",
                "ffv1",
                "-c:s",
                "srt",
                "-output_ts_offset",
                "5",
                "-metadata:s:s:0",
                "title=Primary",
                "-metadata:s:s:0",
                "language=jpn",
                "-metadata:s:s:1",
                "language=eng",
            ])
            .arg(&video)
            .output()
            .expect("subtitle video");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let original = fs::read(&video).expect("video bytes");
    let tracks = crate::decode::probe_playback_formats(&video, None)
        .expect("initial probe catalog")
        .subtitle_tracks;
    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[0].title.as_deref(), Some("Primary"));
    assert_eq!(tracks[0].language.as_deref(), Some("jpn"));
    let session = crate::PlaybackSession::open_paused(
        &video,
        crate::GraphicsDevice::warp_for_test().expect("WARP"),
        0.0,
        1.0,
        towavue_core::PlaybackRange::default(),
        |_| {},
    )
    .expect("paused video session");
    assert_eq!(session.subtitle_tracks(), tracks);
    drop(session);
    assert_text(&read(&video, Some(tracks[0].id)));
    let other = read(&video, Some(tracks[1].id));
    assert_eq!(other.cues()[0].start().as_nanoseconds(), 1_000_000_000);
    for id in [0, 99] {
        assert!(matches!(
            read_subtitles(
                &MediaInput::new(video.clone()),
                Some(SubtitleTrackId::from_index(id)),
                &Cancellation::default()
            ),
            Err(SubtitleError::Message(Text::SubtitleTrackUnavailable))
        ));
    }
    let cancelled = Cancellation::default();
    cancelled.cancel();
    assert!(matches!(
        read_subtitles(
            &MediaInput::new(video.clone()),
            Some(tracks[0].id),
            &cancelled
        ),
        Err(SubtitleError::Cancelled)
    ));
    assert_eq!(fs::read(video).expect("unmodified video"), original);
    fs::remove_dir_all(root).expect("native inputs closed");
}

#[test]
fn bitmap_palette_runs_preserve_alpha_and_clear_events_close_the_active_cue() {
    let mut owned = Decoded::new();
    owned.0.set_pts(Some(2_000_000));
    owned.0.set_end(u32::MAX);
    let rect = owned.0.add_rect(ffmpeg::subtitle::Type::Bitmap);
    // This test exclusively owns FFmpeg-allocated buffers. Decoded frees them
    // through the same avsubtitle_free path used after real decoder output.
    unsafe {
        let raw = &mut *rect.as_ptr().cast_mut();
        raw.w = 4;
        raw.h = 2;
        raw.x = 20;
        raw.y = 40;
        raw.nb_colors = 3;
        raw.linesize[0] = 4;
        raw.data[0] = ffmpeg::ffi::av_malloc(8).cast();
        raw.data[1] = ffmpeg::ffi::av_malloc(12).cast();
        assert!(!raw.data[0].is_null() && !raw.data[1].is_null());
        std::ptr::copy_nonoverlapping([0u8, 1, 1, 0, 0, 2, 2, 0].as_ptr(), raw.data[0], 8);
        let palette = [0u32, 0x80ffffff, 0xff000000];
        std::ptr::copy_nonoverlapping(palette.as_ptr().cast::<u8>(), raw.data[1], 12);
    }
    let mut builder = Builder::default();
    builder
        .append(&owned.0, None, None, 0, Some((64, 48)), &|| Ok(()))
        .expect("bitmap event");
    let mut clear = Decoded::new();
    clear.0.set_pts(Some(3_000_000));
    builder
        .append(&clear.0, None, None, 0, None, &|| Ok(()))
        .expect("clear event");
    let document = builder.finish().expect("closed bitmap timeline");
    let cue = &document.cues()[0];
    assert_eq!(cue.end().as_nanoseconds(), 3_000_000_000);
    let SubtitleContent::Bitmap(images) = cue.content() else {
        panic!("bitmap cue");
    };
    assert_eq!(images[0].size(), (4, 2));
    assert_eq!(images[0].canvas(), Some((64, 48)));
    assert_eq!((images[0].x, images[0].y), (20, 40));
    assert_eq!(
        images[0].rgba(),
        [
            [0, 0, 0, 0],
            [255, 255, 255, 128],
            [255, 255, 255, 128],
            [0, 0, 0, 0],
            [0, 0, 0, 0],
            [0, 0, 0, 255],
            [0, 0, 0, 255],
            [0, 0, 0, 0],
        ]
        .concat()
    );
    let mut refused = Builder::default();
    assert!(matches!(
        refused.append(&owned.0, None, None, 0, None, &|| Err(
            SubtitleError::Cancelled
        )),
        Err(SubtitleError::Cancelled)
    ));

    // A clear event supersedes even a declared display timeout.
    owned.0.set_end(5000);
    let mut finite = Builder::default();
    finite
        .append(&owned.0, None, None, 0, None, &|| Ok(()))
        .expect("finite bitmap");
    finite
        .append(&clear.0, None, None, 0, None, &|| Ok(()))
        .expect("early clear");
    assert_eq!(
        finite.finish().expect("timeline").cues()[0]
            .end()
            .as_nanoseconds(),
        3_000_000_000
    );

    let mut limited = Builder {
        bytes: MAX_BYTES,
        ..Builder::default()
    };
    assert!(matches!(
        limited.append(&owned.0, None, None, 0, None, &|| Ok(())),
        Err(SubtitleError::Message(Text::SubtitleTooLarge))
    ));
}

/// Tiny authored PGS display sets: one 4x2 white object at (20, 40) on a
/// 64x48 canvas, and optionally a later clear. These exercise the real SUP
/// demuxer and PGS decoder rather than just constructing decoder output.
pub(crate) fn pgs(path: &Path, clear: bool) {
    fn segment(output: &mut Vec<u8>, time: u32, kind: u8, data: &[u8]) {
        output.extend_from_slice(b"PG");
        output.extend_from_slice(&(time * 90_000).to_be_bytes());
        output.extend_from_slice(&0u32.to_be_bytes());
        output.push(kind);
        output.extend_from_slice(&(data.len() as u16).to_be_bytes());
        output.extend_from_slice(data);
    }
    let mut output = Vec::new();
    segment(
        &mut output,
        2,
        0x16,
        &[
            0, 64, 0, 48, 0x10, 0, 0, 0x80, 0, 0, 1, 0, 0, 0, 0, 0, 20, 0, 40,
        ],
    );
    segment(&mut output, 2, 0x17, &[1, 0, 0, 0, 0, 0, 0, 64, 0, 48]);
    segment(&mut output, 2, 0x14, &[0, 0, 1, 235, 128, 128, 255]);
    segment(
        &mut output,
        2,
        0x15,
        &[
            0, 0, 0, 0xc0, 0, 0, 16, 0, 4, 0, 2, 1, 1, 1, 1, 0, 0, 1, 1, 1, 1, 0, 0,
        ],
    );
    segment(&mut output, 2, 0x80, &[]);
    if clear {
        segment(
            &mut output,
            3,
            0x16,
            &[0, 64, 0, 48, 0x10, 0, 1, 0, 0, 0, 0],
        );
        segment(&mut output, 3, 0x80, &[]);
    }
    fs::write(path, output).expect("owned PGS fixture");
}

#[test]
fn srt_and_vtt_character_references_are_decoded_once_and_ass_keeps_literal_entities() {
    let root = root("entities");
    for (extension, contents) in [
        (
            "srt",
            "1\n00:00:01,000 --> 00:00:02,000\n&amp; &lt;tag&gt; &amp;lt; 日本語\n",
        ),
        (
            "vtt",
            "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\n&amp; &lt;tag&gt; &amp;lt; 日本語\n",
        ),
    ] {
        let path = root.join(format!("captions.{extension}"));
        fs::write(&path, contents).expect("entity fixture");
        let document = read(&path, None);
        let SubtitleContent::Text(text) = document.cues()[0].content() else {
            panic!("text")
        };
        assert_eq!(text, "& <tag> &lt; 日本語", "{extension}");
    }
    assert_eq!(
        super::text::plain("0,0,Default,,0,0,0,,&amp;lt;", true),
        "&amp;lt;"
    );
    fs::remove_dir_all(root).expect("owned readers released");
}

#[test]
fn pgs_demux_decode_and_embedded_timing_preserve_canvas_pixels_and_clear_events() {
    let root = root("pgs");
    let source = root.join("captions.sup");
    pgs(&source, true);
    let verify = |document: &SubtitleDocument, end: i64| {
        assert_eq!(document.cues().len(), 1);
        let cue = &document.cues()[0];
        assert_eq!(cue.start().as_nanoseconds(), 2_000_000_000);
        assert_eq!(cue.end().as_nanoseconds(), end);
        let SubtitleContent::Bitmap(images) = cue.content() else {
            panic!("PGS bitmap")
        };
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].canvas(), Some((64, 48)));
        assert_eq!(images[0].size(), (4, 2));
        assert_eq!((images[0].x, images[0].y), (20, 40));
        let rgba = images[0].rgba();
        assert_eq!(rgba.len(), 32);
        for pixel in rgba.as_chunks::<4>().0 {
            assert!(
                pixel[..3].iter().all(|value| *value >= 254),
                "white pixel {pixel:?}"
            );
            assert_eq!(pixel[3], 255);
        }
    };
    verify(&read(&source, None), 3_000_000_000);
    let video = root.join("video.mkv");
    let output =
        crate::hidden_test_command(crate::media_tools::tool_path("ffmpeg.exe").expect("FFmpeg"))
            .args([
                "-v",
                "error",
                "-n",
                "-copyts",
                "-f",
                "lavfi",
                "-i",
                "color=size=64x48:rate=10:duration=8",
                "-i",
            ])
            .arg(&source)
            .args([
                "-map",
                "0:v",
                "-map",
                "1:s",
                "-c:v",
                "ffv1",
                "-c:s",
                "copy",
                "-output_ts_offset",
                "5",
            ])
            .arg(&video)
            .output()
            .expect("PGS mux");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let track = crate::decode::probe_playback_formats(&video, None)
        .expect("probe")
        .subtitle_tracks[0]
        .id;
    verify(&read(&video, Some(track)), 3_000_000_000);
    let unclosed = root.join("unclosed.sup");
    pgs(&unclosed, false);
    verify(&read(&unclosed, None), i64::MAX);
    fs::remove_dir_all(root).expect("all PGS inputs released");
}

#[test]
fn malformed_and_missing_subtitles_fail_without_retaining_the_file() {
    let root = root("errors");
    let path = root.join("broken.srt");
    fs::write(&path, "this is not a subtitle\n").expect("malformed fixture");
    for path in [&path, &root.join("missing.vtt")] {
        assert!(
            read_subtitles(
                &MediaInput::new(path.to_owned()),
                None,
                &Cancellation::default()
            )
            .is_err()
        );
    }
    let error = SubtitleError::Message(Text::SubtitleInvalidData);
    assert_ne!(
        error.message(Language::English),
        error.message(Language::Japanese)
    );
    fs::remove_dir_all(root).expect("failure releases input");
}

#[test]
fn mp4_timed_text_preserves_millisecond_intervals_and_unicode_lines() {
    let root = root("mov-text");
    let source = root.join("captions.srt");
    fs::write(
        &source,
        "1\n00:00:02,125 --> 00:00:03,625\n<i>日本語</i>\nSecond line\n",
    )
    .expect("SRT");
    let video = root.join("video.mp4");
    let output =
        crate::hidden_test_command(crate::media_tools::tool_path("ffmpeg.exe").expect("FFmpeg"))
            .args([
                "-v",
                "error",
                "-n",
                "-f",
                "lavfi",
                "-i",
                "color=size=64x48:rate=10:duration=5",
                "-i",
            ])
            .arg(&source)
            .args([
                "-map", "0:v", "-map", "1:s", "-c:v", "mpeg4", "-c:s", "mov_text",
            ])
            .arg(&video)
            .output()
            .expect("MP4 subtitles");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let track = crate::decode::probe_playback_formats(&video, None)
        .expect("probe")
        .subtitle_tracks[0]
        .id;
    let document = read(&video, Some(track));
    assert_eq!(document.cues().len(), 1);
    let cue = &document.cues()[0];
    assert_eq!(cue.start().as_nanoseconds(), 2_125_000_000);
    assert_eq!(cue.end().as_nanoseconds(), 3_625_000_000);
    let SubtitleContent::Text(text) = cue.content() else {
        panic!("timed text")
    };
    assert_eq!(text, "日本語\nSecond line");
    fs::remove_dir_all(root).expect("MP4 readers released");
}
