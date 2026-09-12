use super::*;
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

fn fixture(
    rate: u32,
    channels: u16,
    codec: &str,
    seconds: u32,
    packet_samples: u32,
) -> (PathBuf, PathBuf, PathBuf) {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!("towavue-pcm-anchor-{unique}"));
    fs::create_dir(&directory).expect("owned fixture directory");
    let source = directory.join("source.mkv");
    let control = directory.join("control.nut");
    let executable =
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg")).join("bin/ffmpeg.exe");
    let generated = std::process::Command::new(&executable)
        .creation_flags(0x0800_0000)
        .args([
            "-v",
            "error",
            "-n",
            "-f",
            "lavfi",
            "-i",
            &format!("anoisesrc=sample_rate={rate}:duration={seconds}:seed=173"),
            "-af",
            &format!("asetnsamples=n={packet_samples}:p=0"),
            "-ac",
            &channels.to_string(),
            "-c:a",
            codec,
        ])
        .arg(&source)
        .output()
        .expect("generate owned PCM");
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let remuxed = std::process::Command::new(executable)
        .creation_flags(0x0800_0000)
        .args(["-v", "error", "-n", "-i"])
        .arg(&source)
        .args(["-c:a", "copy"])
        .arg(&control)
        .output()
        .expect("sample-time-base control");
    assert!(
        remuxed.status.success(),
        "{}",
        String::from_utf8_lossy(&remuxed.stderr)
    );
    (directory, source, control)
}

fn collect(
    input: &mut ParallelInput,
    target: MediaTime,
    end: MediaTime,
    decode_start: MediaTime,
) -> (Vec<u8>, DecodeSummary) {
    let mut bytes = Vec::new();
    let start = Instant::now();
    let summary = input
        .decode_software(
            decode_start,
            Some(end),
            Some(DecodeStream::Audio),
            &|| start.elapsed() > Duration::from_secs(60),
            |output| {
                if let ParallelSoftwareDecodeOutput::Item(DecodeOutput::Audio(mut chunk)) = output {
                    clip_audio_chunk(&mut chunk, target, Some(end));
                    bytes.extend(chunk.bytes);
                }
                true
            },
        )
        .expect("bounded audio decode");
    (bytes, summary)
}

fn samples(
    path: &Path,
    target: MediaTime,
    end: MediaTime,
    decode_start: MediaTime,
) -> (Vec<u8>, DecodeSummary) {
    collect(
        &mut ParallelInput::open(path, &|| false).expect("input"),
        target,
        end,
        decode_start,
    )
}

#[test]
fn coarse_pcm_seek_preserves_samples_across_formats_reuse_and_cancellation() {
    for (rate, channels, codec, packet_samples) in [
        (48_000, 1, "pcm_s16le", 1001),
        (44_100, 2, "pcm_s24le", 997),
        (96_000, 2, "pcm_f32le", 1024),
        (48_000, 3, "pcm_s32le", 511),
        (48_000, 1, "pcm_s16be", 1001),
        (48_000, 2, "pcm_f64le", 1001),
        (48_000, 1, "pcm_u8", 997),
    ] {
        let (directory, source, control) = fixture(rate, channels, codec, 2, packet_samples);
        let full_end = MediaTime::from_nanoseconds(2_000_000_000);
        assert_eq!(
            samples(&source, MediaTime::ZERO, full_end, MediaTime::ZERO).0,
            samples(&control, MediaTime::ZERO, full_end, MediaTime::ZERO).0,
            "identical sequential {codec}/{rate}/{channels} PCM"
        );
        let mut reused = ParallelInput::open(&source, &|| false).expect("reused input");
        for ns in [
            1_017_000_000,
            17_000_000,
            1_917_010_417,
            0,
            617_000_000,
            2_000_000_000,
        ] {
            let target = MediaTime::from_nanoseconds(ns);
            let end = target.saturating_add(Duration::from_millis(50));
            let expected = samples(&control, target, end, target).0;
            let (actual, summary) = collect(&mut reused, target, end, target);
            assert_eq!(actual, expected, "{codec}/{rate}/{channels} at {ns} ns");
            assert!(
                summary.audio_frames <= u64::from(rate / 20 + packet_samples * 2),
                "the prefix must not pass through the decoder"
            );
        }
        let checks = AtomicUsize::new(0);
        let result = reused.decode_software(
            MediaTime::from_nanoseconds(1_900_000_000),
            None,
            Some(DecodeStream::Audio),
            &|| checks.fetch_add(1, Ordering::Relaxed) > 10,
            |_| panic!("cancel during prefix traversal, before the target"),
        );
        assert!(
            matches!(result, Err(DecodeError::ConsumerClosed)),
            "cancel prefix scan"
        );
        let target = MediaTime::from_nanoseconds(17_000_000);
        let end = target.saturating_add(Duration::from_millis(50));
        assert_eq!(
            collect(&mut reused, target, end, target).0,
            samples(&control, target, end, target).0,
            "seek after cancellation"
        );
        drop(reused);
        fs::remove_dir_all(directory).expect("remove owned fixtures");
    }
}

#[test]
fn pcm_preroll_counts_variable_packets_and_keeps_sequential_gap_rules() {
    let (directory, source, control) = fixture(48_000, 1, "pcm_s16le", 1, 1001);
    let input = format::input(&source).expect("input");
    let precise = format::input(&control).expect("precise input");
    let precise_config = best_stream_config(&precise, Type::Audio).expect("precise config");
    assert!(
        precise_config
            .pcm_preroll(MediaTime::from_nanoseconds(50_000_000))
            .is_none()
    );
    let config = best_stream_config(&input, Type::Audio).expect("PCM config");
    assert!(config.pcm_preroll(MediaTime::ZERO).is_none());
    let mut pipeline = create_audio_pipeline(&input)
        .expect("pipeline")
        .expect("audio");
    pipeline.pcm_preroll = config.pcm_preroll(MediaTime::from_nanoseconds(150_000_000));
    assert!(pipeline.pcm_preroll.is_some());
    let mut sequential = create_audio_pipeline(&input)
        .expect("sequential pipeline")
        .expect("audio");
    let mut sample = 0_i64;
    for (index, count) in [37_usize, 1024, 17, 2111, 513, 1024]
        .into_iter()
        .enumerate()
    {
        if index == 3 {
            sample += 4800;
        }
        let mut packet = ffmpeg::Packet::copy(&vec![0; count * 2]);
        packet.set_pts(Some((sample * 1000 + 24_000) / 48_000));
        let previous = sequential.next_sample;
        sequential.sample_presentation_time(packet.pts(), count);
        let skip = sequential.next_sample <= 7200;
        assert_eq!(pipeline.skip_pcm_preroll(&packet).expect("PCM count"), skip);
        if !skip {
            assert_eq!(
                pipeline.next_sample, previous,
                "target packet restores its predecessor"
            );
            break;
        }
        assert_eq!(pipeline.next_sample, sequential.next_sample);
        sample += count as i64;
    }
    assert!(pipeline.pcm_preroll.is_none(), "target packet reached");
    pipeline.pcm_preroll = config.pcm_preroll(MediaTime::from_nanoseconds(150_000_000));
    assert!(
        pipeline
            .skip_pcm_preroll(&ffmpeg::Packet::copy(&[0; 3]))
            .is_err()
    );
    let mut packet = ffmpeg::Packet::copy(&[0; 200]);
    let previous = pipeline.next_sample;
    // The exclusively owned packet retains this allocation. Initialize its full
    // ten-byte skip-sample payload; neither native pointer escapes the block.
    unsafe {
        use ffmpeg::codec::packet::Mut;
        let data = ffmpeg::ffi::av_packet_new_side_data(
            packet.as_mut_ptr(),
            ffmpeg::ffi::AVPacketSideDataType::AV_PKT_DATA_SKIP_SAMPLES,
            10,
        );
        assert!(!data.is_null(), "skip-sample side data");
        std::slice::from_raw_parts_mut(data, 10).fill(0);
        *data = 1;
    }
    assert!(
        !pipeline
            .skip_pcm_preroll(&packet)
            .expect("side data fallback")
    );
    assert!(pipeline.pcm_preroll.is_none());
    assert_eq!(
        pipeline.next_sample, previous,
        "decoder owns side-data counting"
    );
    drop((pipeline, sequential, precise_config, config, input, precise));
    fs::remove_dir_all(directory).expect("remove owned fixtures");
}

#[test]
#[ignore = "generates 30 minutes of PCM and compares prefix scanning with decoding"]
fn pcm_packet_scan_reports_exact_seek_anchor_cost() {
    let (directory, source, control) = fixture(48_000, 1, "pcm_s16le", 1800, 1001);
    for target_ms in [617_i64, 60_017, 900_017, 1_799_017] {
        let target = MediaTime::from_nanoseconds(target_ms * 1_000_000);
        let end = target.saturating_add(Duration::from_millis(50));
        let reference = samples(&control, target, end, target).0;
        let scan_start = Instant::now();
        let (scanned, summary) = samples(&source, target, end, target);
        let scan_elapsed = scan_start.elapsed();
        let prefix_start = Instant::now();
        let prefix = samples(&source, target, end, MediaTime::ZERO).0;
        let prefix_elapsed = prefix_start.elapsed();
        println!(
            "PCM_ANCHOR target_ms={target_ms} scan_ms={:.3} decode_prefix_ms={:.3} decoded_samples={} scan_exact={} prefix_exact={}",
            scan_elapsed.as_secs_f64() * 1000.0,
            prefix_elapsed.as_secs_f64() * 1000.0,
            summary.audio_frames,
            scanned == reference,
            prefix == reference
        );
        assert_eq!(scanned, reference);
        assert_eq!(prefix, reference);
        assert!(summary.audio_frames <= 5005, "prefix must not be decoded");
    }
    fs::remove_dir_all(directory).expect("remove owned fixtures");
}
