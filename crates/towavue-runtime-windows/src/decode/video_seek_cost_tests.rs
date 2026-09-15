use super::*;
use std::time::Instant;

// Isolate avoidable preroll conversion before changing worker/lifetime contracts.
// This does not reuse decoder state or skip reference-picture decoding.
fn first_frame(
    path: &Path,
    target: MediaTime,
    skip_conversion: bool,
) -> (VideoFrame, usize, usize) {
    let mut input = format::input(path).expect("owned input");
    let origin = input_origin(&input);
    let mut video = create_video_pipeline(&input)
        .expect("pipeline")
        .expect("video");
    seek_input(&mut input, target, true, &|| false).expect("same GOP seek");
    let mut decoded_count = 0;
    let mut converted_count = 0;
    for (stream, mut packet) in input.packets() {
        if stream.index() != video.stream_index {
            continue;
        }
        normalize_packet_time(&mut packet, stream.time_base(), origin);
        video.decoder.send_packet(&packet).expect("packet");
        loop {
            let mut decoded = frame::Video::empty();
            match video.decoder.receive_frame(&mut decoded) {
                Ok(()) => {
                    decoded_count += 1;
                    let time = timestamp_to_media_time(decoded.timestamp(), video.time_base);
                    if skip_conversion && time < target {
                        continue;
                    }
                    let mut rgba = frame::Video::empty();
                    video.scaler.run(&decoded, &mut rgba).expect("RGBA");
                    let output =
                        copy_video_frame(&decoded, &rgba, video.time_base, video.orientation)
                            .expect("owned frame");
                    converted_count += 1;
                    if time >= target {
                        return (output, decoded_count, converted_count);
                    }
                }
                Err(error) if decoder_is_drained(error) => break,
                Err(error) => panic!("decode: {error}"),
            }
        }
    }
    panic!("fixture target must precede decoder drain; not an EOF prototype");
}

#[test]
#[ignore = "Release-only generated short/long-GOP software preroll conversion comparison"]
fn preroll_conversion_reports_first_frame_cost() {
    if cfg!(debug_assertions) {
        panic!("use Release");
    }
    ffmpeg::init().expect("FFmpeg");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "towavue-seek-conversion-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&root).expect("owned fixtures");
    let executable =
        std::path::PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg"))
            .join("bin/ffmpeg.exe");
    for gop in [30, 180] {
        let path = root.join(format!("gop-{gop}.mp4"));
        let generated = std::process::Command::new(&executable)
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=1920x1080:rate=30:duration=6",
                "-c:v",
                "mpeg4",
                "-q:v",
                "5",
                "-g",
                &gop.to_string(),
                "-bf",
                "2",
                "-an",
            ])
            .arg(&path)
            .output()
            .expect("generate");
        assert!(
            generated.status.success(),
            "{}",
            String::from_utf8_lossy(&generated.stderr)
        );
        let bytes = std::fs::read(&path).expect("owned bytes");
        let stamp = std::fs::metadata(&path)
            .expect("metadata")
            .modified()
            .expect("mtime");
        for milliseconds in [550, 4550] {
            let target = MediaTime::from_nanoseconds(milliseconds * 1_000_000);
            let mut expected = None;
            let result =
                decode_file_parallel(&path, target, None, Some(DecodeStream::Video), |output| {
                    if let ParallelSoftwareDecodeOutput::Item(DecodeOutput::Video(frame)) = output {
                        expected = Some(frame);
                        false
                    } else {
                        true
                    }
                });
            assert!(matches!(result, Err(DecodeError::ConsumerClosed)));
            let expected = expected.expect("ordinary parallel path's target frame");
            let mut expected_decoded = None;
            for skip in [false, true, true, false] {
                let mut times = Vec::new();
                let mut converted = 0;
                let mut decoded = 0;
                for _ in 0..3 {
                    let start = Instant::now();
                    let (actual, received, scaled) = first_frame(&path, target, skip);
                    times.push(start.elapsed().as_secs_f64() * 1000.0);
                    assert_eq!(actual.presentation_time, expected.presentation_time);
                    assert_eq!(actual.duration, expected.duration);
                    assert_eq!(
                        (actual.width, actual.height),
                        (expected.width, expected.height)
                    );
                    assert_eq!(actual.pixel_aspect, expected.pixel_aspect);
                    assert_eq!(actual.orientation, expected.orientation);
                    assert!(actual.rgba == expected.rgba, "exact selected RGBA");
                    assert_eq!(*expected_decoded.get_or_insert(received), received);
                    assert_eq!(scaled, if skip { 1 } else { received });
                    (decoded, converted) = (received, scaled);
                }
                let raw = times.clone();
                times.sort_by(f64::total_cmp);
                println!(
                    "SEEK_CONVERSION gop={gop} target_ms={milliseconds} skip={skip} median_ms={:.4} decoded={decoded} converted={converted} raw={raw:?}",
                    times[1]
                );
            }
        }
        assert_eq!(std::fs::read(&path).expect("unchanged source"), bytes);
        assert_eq!(
            std::fs::metadata(&path)
                .expect("metadata")
                .modified()
                .expect("mtime"),
            stamp
        );
        std::fs::remove_file(path).expect("owned fixture cleanup");
    }
    std::fs::remove_dir(root).expect("empty owned fixture directory");
    println!(
        "SEEK_CONVERSION_CHECKS cases=48 exact_selected_frames=true unchanged_decode_count=true source_bytes_stamps=true"
    );
}
