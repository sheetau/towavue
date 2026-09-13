use super::*;
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub(super) fn fixture(
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

pub(super) fn collect(
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

#[test]
fn retained_audio_intervals_skip_decoding_deleted_packets_without_changing_samples() {
    for (rate, channels, codec, packet_frames) in [
        (44100, 1, "pcm_s16le", 1001),
        (48000, 2, "pcm_f32le", 997),
        (96000, 4, "pcm_s24le", 1001),
        (44100, 2, "flac", 1001),
        (48000, 1, "flac", 997),
        (48000, 2, "aac", 1001),
    ] {
        let (directory, source, _) = fixture(rate, channels, codec, 4, packet_frames);
        let native = directory.join(if codec == "flac" {
            "native.flac"
        } else if codec == "aac" {
            "native.m4a"
        } else {
            "native.wav"
        });
        let executable = PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg"))
            .join("bin/ffmpeg.exe");
        let remuxed = std::process::Command::new(executable)
            .creation_flags(0x0800_0000)
            .args(["-v", "error", "-n", "-i"])
            .arg(&source)
            .args(["-c:a", "copy"])
            .arg(&native)
            .output()
            .expect("native-container control");
        assert!(
            remuxed.status.success(),
            "{}",
            String::from_utf8_lossy(&remuxed.stderr)
        );
        let native_input = format::input(&native).expect("native input");
        assert!(
            best_stream_config(&native_input, Type::Audio)
                .expect("native audio")
                .has_sample_timestamps()
        );
        drop(native_input);
        for path in [&source, &native] {
            for start in [0, 2_130_000_001] {
                let nanos = MediaTime::from_nanoseconds;
                let intervals = [
                    TimeRange::new(nanos(17_000_001), nanos(91_000_001)).expect("first"),
                    TimeRange::new(nanos(2_117_000_001), nanos(2_189_000_001)).expect("second"),
                    TimeRange::new(nanos(3_703_000_001), nanos(3_817_000_001)).expect("third"),
                ];
                // Drain to natural EOF so counters include all decoded output, not only
                // chunks observed before a bounded consumer closes the worker queue.
                let end = nanos(5_000_000_000);
                let collect = |count_deleted: bool| {
                    let mut output = Vec::new();
                    let summary = decode_audio_intervals_cancellable(
                        path,
                        nanos(start),
                        end,
                        if count_deleted { &intervals } else { &[] },
                        &|| false,
                        |chunk| {
                            for range in intervals {
                                let (bounds, _) =
                                    clip_audio_bounds(&chunk, range.start(), Some(range.end()));
                                output.extend_from_slice(
                                    &chunk.bytes[bounds.start * 8..bounds.end * 8],
                                );
                            }
                            true
                        },
                    )
                    .expect("bounded interval decode");
                    (output, summary.audio_frames)
                };
                let (reference, all_frames) = collect(false);
                let (retained, decoded_frames) = collect(true);
                assert!(!reference.is_empty());
                assert!(
                    retained == reference,
                    "PCM mismatch: {rate}/{channels}/{codec}/{path:?} at {start}"
                );
                if codec == "aac" {
                    assert_eq!(
                        decoded_frames, all_frames,
                        "stateful decoder must stay continuous"
                    );
                } else {
                    if start == 0 {
                        assert_eq!(all_frames, u64::from(rate) * 4, "full source frame count");
                    }
                    assert!(
                        decoded_frames < all_frames / if start == 0 { 4 } else { 2 },
                        "deleted samples were still decoded: {rate}/{channels}/{codec}/{path:?} at {start} {decoded_frames}/{all_frames}"
                    );
                }
            }
        }
        fs::remove_dir_all(directory).expect("remove owned fixtures");
    }
}

pub(super) fn samples(
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
fn aac_prefix_requires_lc_frame_length_and_matching_packet_duration() {
    let (directory, source, _) = fixture(44_100, 2, "aac", 1, 1001);
    let mut input = format::input(&source).expect("AAC input");
    let mut config = best_stream_config(&input, Type::Audio).expect("AAC config");
    config.parameters = config.parameters.clone();
    let target = MediaTime::from_nanoseconds(2_000_000_000);
    assert!(matches!(
        config.sample_preroll(target),
        Some((_, PrerollSamples::AacLc(1024)))
    ));
    for (profile, length, accepted) in [
        (ffmpeg::ffi::AV_PROFILE_AAC_HE, 1024, false),
        (ffmpeg::ffi::AV_PROFILE_AAC_LOW, 0, false),
        (ffmpeg::ffi::AV_PROFILE_AAC_LOW, 2048, false),
        (ffmpeg::ffi::AV_PROFILE_AAC_LOW, 960, true),
        (ffmpeg::ffi::AV_PROFILE_AAC_LOW, 1024, true),
    ] {
        // This cloned allocation is exclusively test-owned, not the live stream's
        // parameters. No decoder or concurrent thread can observe these mutations.
        unsafe {
            (*config.parameters.as_mut_ptr()).profile = profile;
            (*config.parameters.as_mut_ptr()).frame_size = length;
        }
        assert_eq!(config.sample_preroll(target).is_some(), accepted);
    }
    config.time_base = Rational(1, 44_100);
    assert!(
        config.sample_preroll(target).is_none(),
        "sample-precision input keeps direct seeking"
    );
    config.time_base = Rational(1, 1000);
    let mut pipeline = create_audio_pipeline(&input)
        .expect("pipeline")
        .expect("audio");
    pipeline.sample_preroll = config.sample_preroll(target);
    let mut summary = DecodeSummary::default();
    let mut packets = input.packets();
    while pipeline.next_sample == ffmpeg::ffi::AV_NOPTS_VALUE {
        let (_, packet) = packets.next().expect("AAC priming packet");
        assert!(
            !pipeline
                .skip_sample_preroll(&packet)
                .expect("decode priming")
        );
        pipeline
            .decoder
            .send_packet(&packet)
            .expect("decode initial packet");
        pipeline
            .receive(&mut |_| true, &mut summary)
            .expect("initial samples");
    }
    let (_, mut packet) = packets.next().expect("countable AAC packet");
    let packet_duration = packet.duration();
    assert!(
        pipeline
            .skip_sample_preroll(&packet)
            .expect("declared frame count")
    );
    let previous = pipeline.next_sample;
    for duration in [0, 1, 100] {
        pipeline.sample_preroll = config.sample_preroll(target);
        packet.set_duration(duration);
        assert!(
            !pipeline
                .skip_sample_preroll(&packet)
                .expect("duration mismatch fallback")
        );
        assert!(pipeline.sample_preroll.is_none());
        assert_eq!(
            pipeline.next_sample, previous,
            "fallback cannot advance the sample axis"
        );
    }
    pipeline.sample_preroll = config.sample_preroll(target);
    packet.set_duration(packet_duration);
    packet.data_mut().expect("owned AAC packet")[0] = 6 << 5;
    assert!(
        !pipeline
            .skip_sample_preroll(&packet)
            .expect("metadata-leading packet fallback")
    );
    assert!(pipeline.sample_preroll.is_none());
    assert_eq!(pipeline.next_sample, previous);
    drop(config);
    drop(input);
    fs::remove_dir_all(directory).expect("remove owned fixtures");
}

#[test]
fn aac_seek_without_noise_substitution_matches_sequential_samples() {
    compare_aac_noise_substitution(false);
}

#[test]
#[ignore = "diagnostic separation of AAC noise synthesis and coarse container timestamps"]
fn aac_noise_substitution_reports_container_seek_differences() {
    compare_aac_noise_substitution(true);
}

fn compare_aac_noise_substitution(report_alignment: bool) {
    for rate in [44_100, 48_000] {
        let (directory, source, _) = fixture(rate, 2, "pcm_f32le", 4, 1001);
        let executable =
            PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg")).join("bin/ffmpeg.exe");
        for pns in ["0", "1"] {
            let encoded = directory.join(format!("pns-{pns}.m4a"));
            let result = std::process::Command::new(&executable)
                .creation_flags(0x0800_0000)
                .args(["-v", "error", "-n", "-i"])
                .arg(&source)
                .args(["-c:a", "aac", "-aac_pns", pns])
                .arg(&encoded)
                .output()
                .expect("AAC control fixture");
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            let remuxed = directory.join(format!("pns-{pns}.mkv"));
            let result = std::process::Command::new(&executable)
                .creation_flags(0x0800_0000)
                .args(["-v", "error", "-n", "-i"])
                .arg(&encoded)
                .args(["-c:a", "copy"])
                .arg(&remuxed)
                .output()
                .expect("AAC container control");
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            for encoded in [encoded, remuxed] {
                let extension = encoded.extension().expect("container").to_string_lossy();
                let mut input = ParallelInput::open(&encoded, &|| false).expect("reused AAC input");
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
                    let expected = samples(&encoded, target, end, MediaTime::ZERO).0;
                    let (actual, summary) = collect(&mut input, target, end, target);
                    assert!(
                        summary.audio_frames < u64::from(rate),
                        "AAC/{rate}/{extension} at {ns}: {} decoded frames",
                        summary.audio_frames
                    );
                    assert_eq!(
                        actual.len(),
                        expected.len(),
                        "AAC/{rate}/{extension}/PNS={pns} at {ns}: sample count"
                    );
                    if pns == "0" {
                        assert!(
                            actual == expected,
                            "AAC/{rate}/{extension} at {ns}: samples differ without PNS"
                        );
                    }
                    assert!(
                        actual == samples(&encoded, target, end, target).0,
                        "identical seek starts must be deterministic"
                    );
                    let error = actual
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .zip(expected.as_chunks::<4>().0)
                        .map(|(a, b)| (f32::from_ne_bytes(*a) - f32::from_ne_bytes(*b)).abs())
                        .fold(0.0f32, f32::max);
                    eprintln!(
                        "AAC/{rate}/{extension}/PNS={pns} at {ns}: samples {}/{}, maximum sample error {error}",
                        actual.len() / 8,
                        expected.len() / 8
                    );
                    if report_alignment
                        && extension == "mkv"
                        && pns == "0"
                        && actual.len() > 32 * 8
                        && expected.len() > 32 * 8
                    {
                        let (offset, residual) = (-32_i32..=32)
                            .map(|offset| {
                                let a = &actual[(offset.max(0) as usize * 8)..];
                                let b = &expected[((-offset).max(0) as usize * 8)..];
                                let error = a
                                    .as_chunks::<4>()
                                    .0
                                    .iter()
                                    .zip(b.as_chunks::<4>().0)
                                    .map(|(a, b)| {
                                        (f32::from_ne_bytes(*a) - f32::from_ne_bytes(*b)).abs()
                                    })
                                    .fold(0.0f32, f32::max);
                                (offset, error)
                            })
                            .min_by(|a, b| a.1.total_cmp(&b.1))
                            .expect("alignment comparison");
                        eprintln!(
                            "AAC/{rate}/{extension} at {ns}: best sample offset {offset}, full-overlap residual {residual}"
                        );
                    }
                }
                let checks = AtomicUsize::new(0);
                assert!(matches!(
                    input.decode_software(
                        MediaTime::from_nanoseconds(2_917_000_000),
                        None,
                        Some(DecodeStream::Audio),
                        &|| checks.fetch_add(1, Ordering::Relaxed) > 10,
                        |_| true,
                    ),
                    Err(DecodeError::ConsumerClosed)
                ));
                let target = MediaTime::from_nanoseconds(17_000_000);
                let end = target.saturating_add(Duration::from_millis(250));
                assert!(
                    collect(&mut input, target, end, target).0
                        == samples(&encoded, target, end, MediaTime::ZERO).0
                );
            }
        }
        fs::remove_dir_all(directory).expect("remove owned fixtures");
    }
}

#[test]
fn opus_prefix_counts_match_decoder_priming_gaps_and_invalid_header_fallback() {
    let (directory, source, _) = fixture(48_000, 2, "libopus", 1, 1001);
    let mut input = format::input(&source).expect("Opus input");
    let config = best_stream_config(&input, Type::Audio).expect("Opus config");
    let mut pipeline = create_audio_pipeline(&input)
        .expect("pipeline")
        .expect("audio");
    assert_eq!(pipeline.decoder.packet_time_base(), config.time_base);
    pipeline.sample_preroll = config.sample_preroll(MediaTime::from_nanoseconds(2_000_000_000));
    pipeline.packet_samples = PacketSamples::new(codec::Id::OPUS);
    let mut sequential = create_audio_pipeline(&input)
        .expect("pipeline")
        .expect("audio");
    let mut summary = DecodeSummary::default();
    let mut first = None;
    let mut counted = 0;
    for (index, (_, mut packet)) in input.packets().enumerate() {
        if index >= 3 {
            packet.set_pts(packet.pts().map(|pts| pts + 100));
        }
        first.get_or_insert_with(|| packet.clone());
        sequential
            .decoder
            .send_packet(&packet)
            .expect("sequential decode");
        sequential
            .receive(&mut |_| true, &mut summary)
            .expect("samples");
        if pipeline.skip_sample_preroll(&packet).expect("Opus prefix") {
            counted += 1;
        } else {
            pipeline
                .decoder
                .send_packet(&packet)
                .expect("decode priming or side data");
            pipeline
                .receive(&mut |_| true, &mut DecodeSummary::default())
                .expect("samples");
        }
        assert_eq!(
            pipeline.next_sample, sequential.next_sample,
            "sample anchor including gap/priming"
        );
    }
    assert!(
        counted > 30,
        "prefix uses packet headers, not full decoding"
    );
    pipeline.sample_preroll = config.sample_preroll(MediaTime::from_nanoseconds(2_000_000_000));
    let previous = pipeline.next_sample;
    assert!(
        !pipeline
            .skip_sample_preroll(&ffmpeg::Packet::copy(&[0x03, 0]))
            .expect("invalid Opus fallback")
    );
    assert!(pipeline.sample_preroll.is_none());
    assert_eq!(pipeline.next_sample, previous);
    pipeline.sample_preroll = config.sample_preroll(MediaTime::from_nanoseconds(2_000_000_000));
    let first = first.expect("first packet with skip samples");
    assert!(first.side_data().next().is_some());
    assert!(
        !pipeline
            .skip_sample_preroll(&first)
            .expect("side data fallback")
    );
    assert!(pipeline.sample_preroll.is_none());
    assert_eq!(pipeline.next_sample, previous);
    drop(config);
    drop(input);
    fs::remove_dir_all(directory).expect("remove owned fixtures");
}

#[test]
fn opus_seek_preserves_sample_phase_across_packet_durations() {
    let (directory, source, _) = fixture(48_000, 2, "pcm_f32le", 4, 1001);
    let executable =
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg")).join("bin/ffmpeg.exe");
    for (duration, application, bitrate) in [
        ("2.5", "audio", "128k"),
        ("5", "audio", "128k"),
        ("10", "audio", "128k"),
        ("20", "audio", "128k"),
        ("40", "audio", "128k"),
        ("60", "audio", "128k"),
        ("20", "voip", "24k"),
        ("60", "voip", "24k"),
    ] {
        let encoded = directory.join(format!("{duration}-{application}.opus"));
        let remuxed = encoded.with_extension("mkv");
        for (input, output, args) in [
            (
                &source,
                &encoded,
                vec![
                    "-c:a",
                    "libopus",
                    "-frame_duration",
                    duration,
                    "-application",
                    application,
                    "-b:a",
                    bitrate,
                ],
            ),
            (&encoded, &remuxed, vec!["-c:a", "copy"]),
        ] {
            let result = std::process::Command::new(&executable)
                .creation_flags(0x0800_0000)
                .args(["-v", "error", "-n", "-i"])
                .arg(input)
                .args(args)
                .arg(output)
                .output()
                .expect("Opus fixture");
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
        for encoded in [&encoded, &remuxed] {
            let mut input = ParallelInput::open(encoded, &|| false).expect("reused Opus input");
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
                let expected = samples(encoded, target, end, MediaTime::ZERO).0;
                let (actual, summary) = collect(&mut input, target, end, target);
                let requested_frames =
                    (4_000_000_000_i64 - ns).clamp(0, 250_000_000) * 48_000 / 1_000_000_000;
                assert_eq!(
                    expected.len(),
                    requested_frames as usize * 8,
                    "priming/EOF sample count"
                );
                assert_eq!(actual.len(), expected.len(), "requested sample count");
                let native = encoded.extension().expect("container") == "opus";
                // Ogg may need the preceding page to restore an EOS granule anchor.
                assert!(
                    summary.audio_frames < if native { 144_000 } else { 48_000 },
                    "Opus/{duration}/{application} at {ns}: {} decoded frames",
                    summary.audio_frames
                );
                assert!(
                    actual == samples(encoded, target, end, target).0,
                    "deterministic reused seek"
                );
                if native || matches!(duration, "2.5" | "5") || ns < 250_000_000 {
                    assert!(
                        actual == expected,
                        "Opus/{duration}/{application}: sample phase at {ns}"
                    );
                }
                if actual.len() <= 64 * 8 {
                    continue;
                }
                let (offset, error) = (-32_i32..=32)
                    .map(|offset| {
                        let a = &actual[offset.max(0) as usize * 8..];
                        let b = &expected[(-offset).max(0) as usize * 8..];
                        let error = a
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .zip(b.as_chunks::<4>().0)
                            .map(|(a, b)| (f32::from_ne_bytes(*a) - f32::from_ne_bytes(*b)).abs())
                            .fold(0.0f32, f32::max);
                        (offset, error)
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .expect("full overlap alignment");
                assert_eq!(
                    offset, 0,
                    "Opus/{duration}/{application}: sample phase at {ns}"
                );
                eprintln!(
                    "OPUS/{duration}/{application}/{} at {ns}: frames {}/{}, offset {offset}, residual {error}, decoded {}",
                    encoded.extension().expect("container").to_string_lossy(),
                    actual.len() / 8,
                    expected.len() / 8,
                    summary.audio_frames
                );
            }
            let checks = AtomicUsize::new(0);
            assert!(matches!(
                input.decode_software(
                    MediaTime::from_nanoseconds(2_917_000_000),
                    None,
                    Some(DecodeStream::Audio),
                    &|| checks.fetch_add(1, Ordering::Relaxed) > 10,
                    |_| true,
                ),
                Err(DecodeError::ConsumerClosed)
            ));
            let target = MediaTime::from_nanoseconds(17_000_000);
            let end = target.saturating_add(Duration::from_millis(250));
            assert!(
                collect(&mut input, target, end, target).0
                    == samples(encoded, target, end, MediaTime::ZERO).0
            );
        }
    }
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
            let decoded_seconds = if matches!(codec, "libvorbis" | "libopus") {
                3
            } else {
                2
            };
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
#[ignore = "generates 30 minutes of PCM and compares prefix scanning, decoding and checkpoint reuse"]
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
        let mut reused = ParallelInput::open(&source, &|| false).expect("benchmark input");
        // Same owner/native indexes, discard/retain/retain/discard order. Clearing
        // only our scalar bookmarks is the full-prefix control, not a cold disk.
        for retain in [false, true, true, false] {
            let mut elapsed = Vec::new();
            for _ in 0..3 {
                if !retain {
                    reused.audio_checkpoints.discard_for_comparison();
                }
                let before = reused.audio_checkpoints.resumes;
                let start = Instant::now();
                let actual = collect(&mut reused, target, end, target).0;
                elapsed.push(start.elapsed().as_secs_f64() * 1000.0);
                assert!(actual == reference, "benchmark preserves exact PCM");
                assert_eq!(reused.audio_checkpoints.resumes > before, retain);
            }
            elapsed.sort_by(f64::total_cmp);
            println!(
                "PCM_CHECKPOINT target_ms={target_ms} retain={retain} median_ms={:.3} probe_packets={} worker_packets={}",
                elapsed[1],
                reused.audio_checkpoints.probe_packets,
                reused.audio_checkpoints.worker_packets
            );
        }
        let mut played = ParallelInput::open(&source, &|| false).expect("playback benchmark input");
        assert!(collect(&mut played, target, end, MediaTime::ZERO).0 == reference);
        let decoded_packets = played.audio_checkpoints.worker_packets;
        let mut elapsed = Vec::new();
        for _ in 0..3 {
            let before = played.audio_checkpoints.resumes;
            let start = Instant::now();
            let actual = collect(&mut played, target, end, target).0;
            elapsed.push(start.elapsed().as_secs_f64() * 1000.0);
            assert!(
                actual == reference,
                "ordinary playback bookmark preserves PCM"
            );
            assert_eq!(played.audio_checkpoints.resumes, before + 1);
        }
        elapsed.sort_by(f64::total_cmp);
        println!(
            "PCM_PLAYED_CHECKPOINT target_ms={target_ms} median_ms={:.3} decoded_packets={decoded_packets} probe_packets={} worker_packets={}",
            elapsed[1],
            played.audio_checkpoints.probe_packets,
            played.audio_checkpoints.worker_packets
        );
    }
    fs::remove_dir_all(directory).expect("remove owned fixtures");
}

#[test]
fn counted_packet_checkpoints_restore_exact_pcm_and_flac_seek_phase() {
    for (codec, rate, channels, packet_samples, variant) in [
        ("pcm_s16le", 48_000, 1, 1001, ""),
        ("pcm_s24le", 44_100, 2, 997, ""),
        ("pcm_f32le", 96_000, 2, 1024, ""),
        ("flac", 44_100, 2, 1001, ""),
        ("pcm_s24le", 44_100, 2, 997, "interleaved"),
        ("pcm_s24le", 44_100, 2, 997, "origin"),
        ("flac", 44_100, 2, 1001, "gap"),
    ] {
        let (directory, mut source, control) = fixture(rate, channels, codec, 12, packet_samples);
        if !variant.is_empty() {
            let output = directory.join("variant.mkv");
            let executable = PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg"))
                .join("bin/ffmpeg.exe");
            let mut command = std::process::Command::new(executable);
            command
                .creation_flags(0x0800_0000)
                .args(["-v", "error", "-n", "-i"])
                .arg(&source);
            match variant {
                "interleaved" => {
                    command.args([
                        "-f",
                        "lavfi",
                        "-i",
                        "color=size=16x16:rate=2:duration=12",
                        "-map",
                        "0:a:0",
                        "-map",
                        "1:v:0",
                        "-c:a",
                        "copy",
                        "-c:v",
                        "ffv1",
                    ]);
                }
                "origin" => {
                    command.args(["-c:a", "copy", "-output_ts_offset", "7.003"]);
                }
                "gap" => {
                    command.args(["-af", "asetpts=PTS+2/TB*gte(T\\,5)", "-c:a", codec]);
                }
                _ => unreachable!(),
            }
            let generated = command.arg(&output).output().expect("variant fixture");
            assert!(
                generated.status.success(),
                "{}",
                String::from_utf8_lossy(&generated.stderr)
            );
            source = output;
        }
        let mut input = ParallelInput::open(&source, &|| false).expect("reused checkpoint input");
        if variant == "origin" {
            assert!(
                input_origin(&input.input) >= 7_000_000,
                "nonzero input origin"
            );
        } else if variant == "interleaved" {
            assert!(input.input.streams().best(Type::Video).is_some());
        }
        let mut full_prefix_packets = 0;
        for (index, target_ms) in [
            11_017_i64, 3017, 9017, 617, 11_917, 12_000, 0, 5017, 6017, 7017, 11_017,
        ]
        .into_iter()
        .enumerate()
        {
            let target = MediaTime::from_nanoseconds(target_ms * 1_000_000);
            let end = target.saturating_add(Duration::from_millis(50));
            let resumes = input.audio_checkpoints.resumes;
            let actual = collect(&mut input, target, end, target).0;
            // A FLAC-copy NUT retains rounded source PTS, not an exact direct-seek
            // oracle. Compare full-prefix decoding for every codec and variant.
            let expected = samples(&source, target, end, MediaTime::ZERO).0;
            if variant == "gap" && target_ms == 6017 {
                assert!(expected.is_empty(), "fixture has a real timestamp gap");
            }
            assert_eq!(
                actual.len(),
                expected.len(),
                "{codec}/{variant} at {target_ms}: length"
            );
            assert!(
                actual == expected,
                "{codec}/{variant} at {target_ms}: exact PCM"
            );
            if codec != "flac" && variant.is_empty() {
                assert!(
                    expected == samples(&control, target, end, target).0,
                    "{codec}: independent sample-time-base control"
                );
            }
            if index == 0 {
                assert_eq!(
                    input.audio_checkpoints.resumes, 0,
                    "first request counts the prefix"
                );
                full_prefix_packets = input.audio_checkpoints.worker_packets;
            } else if target_ms >= 9000 {
                assert_eq!(
                    input.audio_checkpoints.resumes,
                    resumes + 1,
                    "{codec}/{variant}: production path reuses the checkpoint"
                );
                let read =
                    input.audio_checkpoints.probe_packets + input.audio_checkpoints.worker_packets;
                assert!(
                    read < full_prefix_packets / 2,
                    "{codec}/{variant} at {target_ms}: read {read}/{full_prefix_packets} packets"
                );
            }
        }
        // Cancel inside the native checkpoint probe, then reuse the same input.
        let checks = AtomicUsize::new(0);
        let result = input.decode_software(
            MediaTime::from_nanoseconds(11_017_000_000),
            None,
            Some(DecodeStream::Audio),
            &|| checks.fetch_add(1, Ordering::Relaxed) >= 3,
            |_| panic!("cancel checkpoint probe before emitting"),
        );
        assert!(matches!(result, Err(DecodeError::ConsumerClosed)));
        assert!(
            input.audio_checkpoints.probe_packets > 0,
            "probe started before cancellation"
        );
        let target = MediaTime::from_nanoseconds(9_017_000_000);
        let end = target.saturating_add(Duration::from_millis(50));
        assert!(
            collect(&mut input, target, end, target).0
                == samples(&source, target, end, MediaTime::ZERO).0,
            "reuse after probe cancellation"
        );
        drop(input);
        let mut played = ParallelInput::open(&source, &|| false).expect("normal playback input");
        collect(
            &mut played,
            MediaTime::ZERO,
            MediaTime::from_nanoseconds(10_000_000_000),
            MediaTime::ZERO,
        );
        let played_packets = played.audio_checkpoints.worker_packets;
        assert_eq!(played.audio_checkpoints.resumes, 0, "no initial seek");
        let actual = collect(&mut played, target, end, target).0;
        assert!(actual == samples(&source, target, end, MediaTime::ZERO).0);
        assert_eq!(
            played.audio_checkpoints.resumes, 1,
            "{codec}/{variant}: reuse normal playback"
        );
        assert!(played.audio_checkpoints.worker_packets < played_packets / 2);
        drop(played);
        if variant == "interleaved" {
            let mut combined = ParallelInput::open(&source, &|| false).expect("combined input");
            combined
                .decode_software(target, Some(end), None, &|| false, |_| true)
                .expect("video-GOP audio start");
            assert!(
                collect(&mut combined, target, end, target).0
                    == samples(&source, target, end, MediaTime::ZERO).0
            );
            assert_eq!(
                combined.audio_checkpoints.resumes, 0,
                "an approximate video-GOP audio axis must not seed checkpoints"
            );
        }
        fs::remove_dir_all(directory).expect("remove owned fixtures");
    }
}
