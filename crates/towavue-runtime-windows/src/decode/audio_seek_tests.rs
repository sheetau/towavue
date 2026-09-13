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

fn native_compressed_fixture(directory: &Path, rate: u32, codec: &str, extension: &str) -> PathBuf {
    let native = directory.join(format!("native.{extension}"));
    let executable =
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg")).join("bin/ffmpeg.exe");
    let generated = std::process::Command::new(executable)
        .creation_flags(0x0800_0000)
        .args([
            "-v",
            "error",
            "-n",
            "-f",
            "lavfi",
            "-i",
            &format!("anoisesrc=sample_rate={rate}:duration=4:seed=173"),
            "-ac",
            "2",
            "-c:a",
            codec,
        ])
        .arg(&native)
        .output()
        .expect("native container fixture");
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    native
}

#[test]
fn coarse_audio_seek_preserves_sequential_samples_without_decoding_the_prefix() {
    for (codec, rate, channels) in [
        ("flac", 32_000, 1),
        ("flac", 44_100, 2),
        ("flac", 48_000, 2),
        ("flac", 96_000, 2),
        ("libvorbis", 32_000, 1),
        ("libvorbis", 44_100, 2),
        ("libvorbis", 48_000, 2),
        ("libvorbis", 96_000, 2),
    ] {
        let (directory, source, _) = fixture(rate, channels, codec, 4, 1001);
        let mut input = ParallelInput::open(&source, &|| false).expect("reused compressed input");
        for ns in [
            2_917_000_000,
            17_000_000,
            617_000_000,
            1_017_000_000,
            0,
            3_917_000_000,
            4_000_000_000,
        ] {
            let target = MediaTime::from_nanoseconds(ns);
            let end = target.saturating_add(Duration::from_millis(250));
            let expected = samples(&source, target, end, MediaTime::ZERO).0;
            let (actual, summary) = collect(&mut input, target, end, target);
            assert_eq!(
                actual.len(),
                expected.len(),
                "{codec}/{rate}/{channels} at {ns}: sample count"
            );
            assert!(
                actual == expected,
                "{codec}/{rate}/{channels} at {ns}: sequential samples differ"
            );
            assert!(
                summary.audio_frames < u64::from(rate),
                "{codec}/{rate}/{channels} at {ns}: {} prefix samples were decoded",
                summary.audio_frames
            );
        }
        let checks = AtomicUsize::new(0);
        let result = input.decode_software(
            MediaTime::from_nanoseconds(3_917_000_000),
            None,
            Some(DecodeStream::Audio),
            &|| checks.fetch_add(1, Ordering::Relaxed) > 10,
            |_| true,
        );
        assert!(matches!(result, Err(DecodeError::ConsumerClosed)));
        let target = MediaTime::from_nanoseconds(17_000_000);
        let end = target.saturating_add(Duration::from_millis(250));
        assert!(
            collect(&mut input, target, end, target).0
                == samples(&source, target, end, MediaTime::ZERO).0
        );
        drop(input);
        fs::remove_dir_all(directory).expect("remove owned fixtures");
    }
}

#[test]
fn flac_prefix_headers_preserve_variable_counts_gaps_and_invalid_header_fallback() {
    let (directory, source, _) = fixture(48_000, 2, "flac", 1, 1001);
    let mut input = format::input(&source).expect("FLAC input");
    let config = best_stream_config(&input, Type::Audio).expect("FLAC config");
    let mut pipeline = create_audio_pipeline(&input)
        .expect("pipeline")
        .expect("audio");
    pipeline.sample_preroll = config.sample_preroll(MediaTime::from_nanoseconds(2_000_000_000));
    pipeline.packet_samples = PacketSamples::new(codec::Id::FLAC);
    let mut sequential = create_audio_pipeline(&input)
        .expect("sequential pipeline")
        .expect("audio");
    let mut summary = DecodeSummary::default();
    let mut counts = std::collections::BTreeSet::new();
    let mut first = None;
    for (index, (_, mut packet)) in input.packets().enumerate() {
        if index >= 3 {
            packet.set_pts(packet.pts().map(|pts| pts + 100));
        }
        first.get_or_insert_with(|| ffmpeg::Packet::copy(packet.data().expect("packet")));
        let before = summary.audio_frames;
        sequential
            .decoder
            .send_packet(&packet)
            .expect("decode packet");
        sequential
            .receive(&mut |_| true, &mut summary)
            .expect("decode samples");
        counts.insert(summary.audio_frames - before);
        assert!(pipeline.skip_sample_preroll(&packet).expect("FLAC count"));
        assert_eq!(
            pipeline.next_sample, sequential.next_sample,
            "header and decoded sample axis"
        );
    }
    assert!(counts.len() >= 2, "fixture includes a short final block");
    let first = first.expect("first packet");
    let parser = pipeline.packet_samples.as_mut().expect("FLAC parser");
    assert!(parser.samples(&mut pipeline.decoder, &first).is_some());
    assert!(
        parser
            .samples(&mut pipeline.decoder, &ffmpeg::Packet::copy(&[0; 15]))
            .is_none()
    );
    let mut damaged = ffmpeg::Packet::copy(first.data().expect("packet"));
    damaged.data_mut().expect("owned packet")[0] = 0;
    assert!(
        parser.samples(&mut pipeline.decoder, &damaged).is_none(),
        "invalid header cannot reuse the preceding duration"
    );
    let previous = pipeline.next_sample;
    assert!(
        !pipeline
            .skip_sample_preroll(&damaged)
            .expect("decoder fallback")
    );
    assert!(pipeline.sample_preroll.is_none());
    assert_eq!(pipeline.next_sample, previous);
    drop(config);
    drop(input);
    fs::remove_dir_all(directory).expect("remove owned fixtures");
}

#[test]
fn vorbis_prefix_counts_match_decoder_priming_and_preserve_timestamp_gaps() {
    let (directory, source, _) = fixture(48_000, 2, "libvorbis", 1, 1001);
    let mut input = format::input(&source).expect("Vorbis input");
    let config = best_stream_config(&input, Type::Audio).expect("Vorbis config");
    let mut pipeline = create_audio_pipeline(&input)
        .expect("pipeline")
        .expect("audio");
    pipeline.sample_preroll = config.sample_preroll(MediaTime::from_nanoseconds(2_000_000_000));
    pipeline.packet_samples = PacketSamples::new(codec::Id::VORBIS);
    let mut sequential = create_audio_pipeline(&input)
        .expect("sequential pipeline")
        .expect("audio");
    let mut summary = DecodeSummary::default();
    let mut counts = std::collections::BTreeSet::new();
    for (index, (_, mut packet)) in input.packets().enumerate() {
        if index >= 3 {
            packet.set_pts(packet.pts().map(|pts| pts + 100));
        }
        if index == 0 {
            assert!(
                packet
                    .side_data()
                    .any(|side| side.kind() == ffmpeg::codec::packet::side_data::Type::SkipSamples)
            );
        }
        let before = summary.audio_frames;
        sequential
            .decoder
            .send_packet(&packet)
            .expect("decode packet");
        sequential
            .receive(&mut |_| true, &mut summary)
            .expect("decode samples");
        counts.insert(summary.audio_frames - before);
        assert!(pipeline.skip_sample_preroll(&packet).expect("Vorbis count"));
        assert_eq!(
            pipeline.next_sample, sequential.next_sample,
            "header and decoded sample axis"
        );
    }
    assert!(
        counts.contains(&0),
        "initial packet is decoder priming only"
    );
    assert!(
        counts.iter().filter(|count| **count > 0).count() >= 2,
        "varying overlap lengths"
    );
    let previous = pipeline.next_sample;
    assert!(
        !pipeline
            .skip_sample_preroll(&ffmpeg::Packet::copy(&[0xff; 16]))
            .expect("invalid-header fallback")
    );
    assert!(pipeline.sample_preroll.is_none());
    assert_eq!(pipeline.next_sample, previous);
    drop(config);
    drop(input);
    fs::remove_dir_all(directory).expect("remove owned fixtures");
}

#[test]
fn compressed_audio_preroll_keeps_seek_samples_and_rewinds_reused_inputs() {
    for (codec, extension, rate) in [
        ("libmp3lame", "mp3", 48_000),
        ("libmp3lame", "mp3", 44_100),
        ("aac", "m4a", 48_000),
        ("libopus", "opus", 48_000),
        ("libvorbis", "ogg", 48_000),
    ] {
        let (directory, _, _) = fixture(rate, 2, codec, 4, 1001);
        let source = native_compressed_fixture(&directory, rate, codec, extension);
        let mut input = ParallelInput::open(&source, &|| false).expect("reused input");
        for ns in [
            2_917_000_000,
            17_000_000,
            617_000_000,
            1_017_000_000,
            0,
            3_917_000_000,
            4_000_000_000,
            2_917_000_000,
        ] {
            let target = MediaTime::from_nanoseconds(ns);
            let end = target.saturating_add(Duration::from_millis(250));
            let expected = samples(&source, target, end, MediaTime::ZERO).0;
            let (actual, summary) = collect(&mut input, target, end, target);
            assert_eq!(
                actual.len(),
                expected.len(),
                "{codec}/{rate} at {ns}: requested sample count differs"
            );
            if matches!(codec, "libmp3lame" | "libvorbis") || ns < 250_000_000 {
                assert!(
                    actual == expected,
                    "{codec}/{rate} at {ns}: sequential samples differ"
                );
            }
            // Ogg may decode an additional page to restore its granule anchor.
            let decoded_seconds = if codec == "libvorbis" { 3 } else { 2 };
            assert!(
                summary.audio_frames < u64::from(rate) * decoded_seconds,
                "unexpected full-prefix decode"
            );
        }
        let checks = AtomicUsize::new(0);
        assert!(matches!(
            input.decode_software(
                MediaTime::from_nanoseconds(2_917_000_000),
                None,
                Some(DecodeStream::Audio),
                &|| checks.fetch_add(1, Ordering::Relaxed) > 3,
                |_| true,
            ),
            Err(DecodeError::ConsumerClosed)
        ));
        let target = MediaTime::from_nanoseconds(17_000_000);
        let end = target.saturating_add(Duration::from_millis(250));
        assert!(
            collect(&mut input, target, end, target).0
                == samples(&source, target, end, MediaTime::ZERO).0
        );
        drop(input);
        fs::remove_dir_all(directory).expect("remove owned fixtures");
    }
}

#[test]
#[ignore = "diagnostic comparison of compressed audio seek output with sequential decoding"]
fn compressed_audio_seek_reports_sequential_alignment() {
    for (codec, extension) in [
        ("flac", "flac"),
        ("aac", "m4a"),
        ("libmp3lame", "mp3"),
        ("libopus", "opus"),
        ("libvorbis", "ogg"),
    ] {
        let (directory, source, _) = fixture(48_000, 2, codec, 4, 1001);
        let native = native_compressed_fixture(&directory, 48_000, codec, extension);
        for source in [&source, &native] {
            for ns in [17_000_000, 617_000_000, 1_017_000_000, 2_917_000_000] {
                let target = MediaTime::from_nanoseconds(ns);
                let end = target.saturating_add(Duration::from_millis(250));
                let expected = samples(source, target, end, MediaTime::ZERO).0;
                let actual = samples(source, target, end, target).0;
                let floats = |bytes: &[u8]| {
                    bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|bytes| f32::from_ne_bytes(*bytes))
                        .collect::<Vec<_>>()
                };
                let expected = floats(&expected);
                let actual = floats(&actual);
                let error = |skip: usize| {
                    expected
                        .iter()
                        .zip(&actual)
                        .skip(skip)
                        .map(|(a, b)| (a - b).abs())
                        .fold(0.0f32, f32::max)
                };
                let (lag, _) = (-64i32..=64)
                    .map(|lag| {
                        let error = (0..128)
                            .map(|index| {
                                let sample = 3000 + index * 31;
                                let a = expected[sample * 2];
                                let b = actual[((sample as i32 + lag) * 2) as usize];
                                (a - b) * (a - b)
                            })
                            .sum::<f32>();
                        (lag, error)
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .expect("alignment probe");
                eprintln!(
                    "{codec}/{} at {ns} ns: samples {}/{}, max {}, tail max {}, best lag {lag}",
                    source.extension().expect("extension").to_string_lossy(),
                    actual.len() / 2,
                    expected.len() / 2,
                    error(0),
                    error(9600)
                );
            }
        }
        fs::remove_dir_all(directory).expect("remove owned fixtures");
    }
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
            .sample_preroll(MediaTime::from_nanoseconds(50_000_000))
            .is_none()
    );
    let config = best_stream_config(&input, Type::Audio).expect("PCM config");
    assert!(config.sample_preroll(MediaTime::ZERO).is_none());
    let mut pipeline = create_audio_pipeline(&input)
        .expect("pipeline")
        .expect("audio");
    pipeline.sample_preroll = config.sample_preroll(MediaTime::from_nanoseconds(150_000_000));
    assert!(pipeline.sample_preroll.is_some());
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
        assert_eq!(
            pipeline.skip_sample_preroll(&packet).expect("PCM count"),
            skip
        );
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
    assert!(pipeline.sample_preroll.is_none(), "target packet reached");
    pipeline.sample_preroll = config.sample_preroll(MediaTime::from_nanoseconds(150_000_000));
    assert!(
        pipeline
            .skip_sample_preroll(&ffmpeg::Packet::copy(&[0; 3]))
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
            .skip_sample_preroll(&packet)
            .expect("side data fallback")
    );
    assert!(pipeline.sample_preroll.is_none());
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
