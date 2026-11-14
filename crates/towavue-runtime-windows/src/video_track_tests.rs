use super::*;
use towavue_core::{MediaTime, VideoTrackId};

#[test]
fn video_tracks_keep_decode_previews_frame_steps_and_exports_on_the_selected_stream() {
    let root = std::env::temp_dir().join(format!(
        "towavue-video-tracks-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("video track verification")
            .as_nanos()
    ));
    fs::create_dir(&root).expect("video track verification");
    let source = root.join("tracks.mkv");
    let output = hidden_command(
        &tool_path("ffmpeg.exe").expect("video track verification"),
        [
            "-v",
            "error",
            "-n",
            "-f",
            "lavfi",
            "-i",
            "color=red:size=64x48:rate=10:duration=2",
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=48000:cl=stereo:d=2",
            "-f",
            "lavfi",
            "-i",
            "color=blue:size=80x64:rate=25:duration=2",
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "2:v",
            "-c:v",
            "libopenh264",
            "-threads",
            "1",
            "-c:a",
            "pcm_s16le",
            "-disposition:v:0",
            "0",
            "-disposition:v:1",
            "default",
            "-metadata:s:v:0",
            "title=Red",
            "-metadata:s:v:1",
            "title=Blue",
        ],
    )
    .arg(&source)
    .output()
    .expect("video track verification");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let input = ffmpeg_next::format::input(&source).expect("video track verification");
    let catalog = crate::decode::video_tracks::catalog(&input);
    assert_eq!(
        catalog
            .tracks
            .iter()
            .map(|entry| entry.id.index())
            .collect::<Vec<_>>(),
        [0, 2]
    );
    assert_eq!(catalog.preferred, Some(VideoTrackId::from_index(2)));
    assert_eq!(catalog.tracks[0].title.as_deref(), Some("Red"));
    assert_eq!(
        catalog.next_track(catalog.preferred),
        Some(VideoTrackId::from_index(0))
    );
    for index in [1, 99] {
        assert!(matches!(
            crate::decode::video_tracks::selected_stream(
                &input,
                Some(VideoTrackId::from_index(index))
            ),
            Err(crate::DecodeError::VideoTrackUnavailable)
        ));
    }
    drop(input);
    let cache = PreviewCache::new(root.join("cache")).expect("video track verification");
    let mut steps = crate::FrameStepCache::default();
    let layout = VideoSheetLayout::for_position(Duration::from_secs(2), Duration::ZERO)
        .expect("video track verification");
    for (index, size, fps, previous, dominant) in [
        (0, (64, 48), 10.0, 100_000_000, 0),
        (2, (80, 64), 25.0, 160_000_000, 2),
        (0, (64, 48), 10.0, 100_000_000, 0),
    ] {
        let track = Some(VideoTrackId::from_index(index));
        let formats = crate::decode::probe_playback_formats_with_video(&source, None, track)
            .expect("video track verification");
        assert_eq!(formats.source_video_frame_rate, Some(fps));
        assert_eq!(
            steps
                .adjacent_for_track(
                    &source,
                    track,
                    MediaTime::from_nanoseconds(200_000_000),
                    false,
                    None,
                    &|| false
                )
                .expect("video track verification"),
            Some(MediaTime::from_nanoseconds(previous))
        );
        let thumbnail = cache
            .thumbnail_for_track(&source, track, Duration::from_millis(700), 80)
            .expect("video track verification");
        assert!(thumbnail.rgba[dominant] > 200 && thumbnail.rgba[2 - dominant] < 30);
        let sheet = cache
            .video_sheet_for_track(&source, track, layout)
            .expect("video track verification");
        assert!(sheet.image.rgba[dominant] > 200 && sheet.image.rgba[2 - dominant] < 30);
        let cached = cache
            .cached_video_sheet_for_track(&source, track, layout)
            .expect("video track verification")
            .expect("video track verification");
        assert_eq!(cached.image.rgba, sheet.image.rgba);
        let cli = frame_preview_for_track(
            &source,
            track,
            Duration::from_millis(700),
            "format=rgba",
            None,
        )
        .expect("video track verification");
        let cli = image::load_from_memory(&cli)
            .expect("video track verification")
            .into_rgba8();
        assert_eq!(cli.dimensions(), size);
        assert!(cli.as_raw()[dominant] > 200 && cli.as_raw()[2 - dominant] < 30);
        let png = crate::decode::frame_png::edited_video_track_frame_png(
            &source,
            track,
            MediaTime::from_nanoseconds(800_000_000),
            &[],
            &|| false,
        )
        .expect("video track verification");
        assert_eq!(
            image::load_from_memory(&png)
                .expect("video track verification")
                .into_rgba8()
                .dimensions(),
            size
        );
        let target = root.join(format!("selected-{index}.mkv"));
        if !target.exists() {
            crate::export_media(&crate::ExportRequest {
                source: source.clone(),
                target: target.clone(),
                kind: MediaKind::Video,
                operations: vec![towavue_core::EditOperation::FlipHorizontal],
                hardware_encode: false,
                video_track: track,
            })
            .expect("video track verification");
            let result = ffmpeg_next::format::input(&target).expect("video track verification");
            let streams: Vec<_> = result
                .streams()
                .filter(|stream| stream.parameters().medium() == ffmpeg_next::media::Type::Video)
                .collect();
            assert_eq!(streams.len(), 1);
            let decoder =
                ffmpeg_next::codec::context::Context::from_parameters(streams[0].parameters())
                    .expect("video track verification")
                    .decoder()
                    .video()
                    .expect("video track verification");
            assert_eq!((decoder.width(), decoder.height()), size);
        }
    }
    assert!(
        cache
            .thumbnail_for_track(
                &source,
                Some(VideoTrackId::from_index(1)),
                Duration::ZERO,
                80
            )
            .is_err()
    );
    let cover = root.join("cover.png");
    image::RgbaImage::from_pixel(32, 32, image::Rgba([240, 10, 10, 255]))
        .save(&cover)
        .expect("video track verification");
    let covered = root.join("cover.m4a");
    let result = hidden_command(
        &tool_path("ffmpeg.exe").expect("video track verification"),
        ["-v", "error", "-n", "-i"],
    )
    .arg(&cover)
    .args([
        "-f",
        "lavfi",
        "-i",
        "anullsrc=r=48000:cl=stereo:d=1",
        "-map",
        "0:v",
        "-map",
        "1:a",
        "-c:v",
        "copy",
        "-c:a",
        "aac",
        "-disposition:v:0",
        "attached_pic",
    ])
    .arg(&covered)
    .output()
    .expect("video track verification");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let input = ffmpeg_next::format::input(&covered).expect("video track verification");
    let cover_stream = input
        .streams()
        .find(|stream| stream.parameters().medium() == ffmpeg_next::media::Type::Video)
        .expect("video track verification");
    assert!(
        crate::decode::video_tracks::catalog(&input)
            .tracks
            .is_empty()
    );
    assert!(
        crate::decode::video_tracks::selected_stream(
            &input,
            Some(VideoTrackId::from_index(cover_stream.index()))
        )
        .is_err()
    );
    drop(input);
    drop(cache);
    fs::remove_dir_all(root).expect("video track verification");
}
