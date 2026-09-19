use super::*;
use crate::export::audio_tests::{ffmpeg, pcm, root};

fn request(source: &Path, target: &Path) -> ExportRequest {
    ExportRequest {
        source: source.into(),
        target: target.into(),
        kind: MediaKind::Audio,
        operations: vec![],
        hardware_encode: false,
    }
}

fn options(target: LoudnessTarget) -> ExportOptions {
    ExportOptions {
        audio: AudioExportOptions {
            normalization: AudioNormalization::Loudness(target),
            channels: AudioChannels::Keep,
        },
        ..Default::default()
    }
}

fn measurement(path: &Path, target: LoudnessTarget) -> Measurement {
    let request = request(path, &path.with_extension("unused.wav"));
    let streams = ExportStreams::probe(&request).expect("probe");
    let staging = StagedExport::new(&request.target).expect("owned measurement staging");
    measure(
        target,
        &request,
        &streams,
        &staging,
        &crate::media_tools::tool_path("ffmpeg.exe").expect("fixed tool"),
        &AtomicBool::new(false),
        &|_| {},
    )
    .expect("actual encoded measurement")
}

fn tone(path: &Path, filter: &str) {
    ffmpeg(&["-f", "lavfi", "-i", filter, "-c:a", "pcm_f64le"], path);
}

fn samples(path: &Path) -> Vec<f32> {
    pcm(path)
        .as_chunks::<4>()
        .0
        .iter()
        .map(|value| f32::from_le_bytes(*value))
        .collect()
}

#[test]
fn loudness_targets_validate_and_silence_short_and_gated_audio_fail_without_publication() {
    for target in [
        LoudnessTarget {
            integrated_tenths: -701,
            ..Default::default()
        },
        LoudnessTarget {
            true_peak_tenths: 1,
            ..Default::default()
        },
    ] {
        assert!(target.validate().is_err());
    }
    let root = root("loudness-unmeasurable");
    for (name, filter, expected) in [
        ("silence", "anullsrc=r=48000:cl=mono:d=1", "Silent audio"),
        ("short", "sine=sample_rate=48000:duration=0.1", "too short"),
        (
            "gate",
            "aevalsrc=0.00000001*sin(2*PI*431*t):s=48000:d=1",
            "gated integrated",
        ),
    ] {
        let source = root.join(format!("{name}.wav"));
        tone(&source, filter);
        let target = root.join(format!("{name}-output.wav"));
        fs::write(&target, b"preserve existing target").expect("owned loudness fixture operation");
        let result = export_media_with_options(
            &request(&source, &target),
            options(LoudnessTarget::default()),
        );
        assert!(
            result
                .as_ref()
                .is_err_and(|error| error.to_string().contains(expected)),
            "{name}: {result:?}"
        );
        assert_eq!(
            fs::read(&target).expect("owned loudness fixture operation"),
            b"preserve existing target"
        );
    }
    assert!(
        !fs::read_dir(&root)
            .expect("owned loudness fixture operation")
            .any(|entry| {
                entry
                    .expect("owned loudness fixture operation")
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".towavue")
            })
    );
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn loudness_linear_gain_preserves_wide_dynamics_and_encoded_formats_meet_targets() {
    let root = root("loudness-linear");
    let source = root.join("source.wav");
    tone(
        &source,
        r"aevalsrc=0.025*sin(2*PI*431*t)*if(lt(t\,8)\,1\,4):s=48000:d=16",
    );
    let target = LoudnessTarget {
        integrated_tenths: -230,
        ..Default::default()
    };
    let input = measurement(&source, target);
    assert!(
        input.range > 7.0,
        "exercise a range above loudnorm's implicit default: {input:?}"
    );
    let plan = Plan {
        target,
        input,
        correction: 0.0,
        peak_margin: 0.1,
        sample_rate: 48000,
    };
    assert!(plan.filters().expect("normalization filters")[0].starts_with("volume="));
    let original = samples(&source);
    for extension in ["wav", "flac", "mp3", "m4a", "aac", "ogg", "opus"] {
        let output = root.join(format!("output.{extension}"));
        export_media_with_options(&request(&source, &output), options(target))
            .unwrap_or_else(|error| panic!("{extension}: {error}"));
        let measured = measurement(&output, target);
        eprintln!("PASS loudness {extension}: {measured:?}");
        assert!((measured.integrated - target.integrated()).abs() <= 0.100001);
        assert!(measured.true_peak + 0.005 <= target.true_peak());
        if extension == "wav" {
            let decoded = samples(&output);
            assert_eq!(decoded.len(), original.len());
            let gain = 10_f64.powf((target.integrated() - input.integrated) / 20.0);
            assert!(
                original
                    .iter()
                    .zip(&decoded)
                    .all(|(a, b)| (f64::from(*a) * gain - f64::from(*b)).abs() < 0.00004)
            );
            assert!((input.range - measured.range).abs() <= 0.1);
        }
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn loudness_peak_limiting_verifies_lossy_candidates_and_unreachable_target_is_not_published() {
    let root = root("loudness-limiter");
    let source = root.join("source.wav");
    tone(
        &source,
        r"aevalsrc=(0.04+0.7*between(mod(t\,1)\,0.49\,0.50))*sin(2*PI*431*t):s=48000:d=8",
    );
    let target = LoudnessTarget::default();
    let input = measurement(&source, target);
    let plan = Plan {
        target,
        input,
        correction: 0.0,
        peak_margin: 0.1,
        sample_rate: 48000,
    };
    assert!(
        plan.filters().expect("normalization filters")[0].starts_with("loudnorm="),
        "{input:?}"
    );
    for extension in ["wav", "mp3", "m4a", "opus"] {
        let output = root.join(format!("output.{extension}"));
        export_media_with_options(&request(&source, &output), options(target))
            .unwrap_or_else(|error| panic!("{extension}: {error}"));
        let measured = measurement(&output, target);
        eprintln!("PASS peak-limited {extension}: {measured:?}");
        assert!((measured.integrated - target.integrated()).abs() <= 0.100001);
        assert!(measured.true_peak + 0.005 <= target.true_peak());
    }
    let impossible = LoudnessTarget {
        integrated_tenths: -50,
        true_peak_tenths: -90,
    };
    let output = root.join("impossible.wav");
    fs::write(&output, b"old destination").expect("owned loudness fixture operation");
    let result = export_media_with_options(&request(&source, &output), options(impossible));
    assert!(
        result
            .as_ref()
            .is_err_and(|error| error.to_string().contains("did not meet")),
        "{result:?}"
    );
    assert_eq!(
        fs::read(&output).expect("destination bytes"),
        b"old destination"
    );
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn loudness_analysis_uses_edited_channel_converted_audio_and_cancellation_preserves_destination() {
    let root = root("loudness-edits");
    let source = root.join("source.wav");
    tone(
        &source,
        r"aevalsrc=0.05*sin(2*PI*431*t)|0.1*sin(2*PI*631*t):s=48000:d=3",
    );
    let output = root.join("mono.wav");
    let mut request = request(&source, &output);
    let time = |ms: i64| towavue_core::MediaTime::from_nanoseconds(ms * 1_000_000);
    let range = |start, end| {
        towavue_core::TimeRange::new(time(start), time(end))
            .expect("owned loudness fixture operation")
    };
    request.operations = vec![
        EditOperation::SetVolume(0.4),
        EditOperation::Timeline(towavue_core::TimelineEdit::Delete(range(500, 1000))),
        EditOperation::Timeline(towavue_core::TimelineEdit::SetVolume(range(0, 500), 0.5)),
        EditOperation::SetRate(1.5),
    ];
    let mut options = options(LoudnessTarget::default());
    options.audio.channels = AudioChannels::Mono;
    export_media_with_options(&request, options.clone())
        .expect("post edit/channel normalized output");
    let measured = measurement(&output, LoudnessTarget::default());
    assert!((measured.integrated + 14.0).abs() <= 0.100001);
    assert_eq!(
        ExportStreams::probe(&self::request(&output, &output))
            .expect("owned loudness fixture operation")
            .audio_channels,
        Some(1)
    );
    assert_eq!(
        samples(&output).len(),
        80000,
        "edited 2.5 seconds / 1.5 rate at 48 kHz, mono"
    );
    let saved = fs::read(&output).expect("destination bytes");
    let cancelled = AtomicBool::new(false);
    let result = export_options_cancellable(&request, options, &cancelled, &|_| {}, &|time| {
        if !time.is_zero() {
            cancelled.store(true, Ordering::Relaxed);
        }
    });
    assert!(matches!(result, Err(ExportError::Cancelled)), "{result:?}");
    assert_eq!(fs::read(&output).expect("destination bytes"), saved);
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn loudness_final_verification_cancels_and_rejects_source_mutation_without_publication() {
    let root = root("loudness-final-guards");
    let source = root.join("source.wav");
    tone(&source, "sine=sample_rate=48000:duration=2");
    let output = root.join("destination.wav");
    fs::write(&output, b"existing destination").expect("owned loudness fixture operation");
    for mutate in [false, true] {
        let cancelled = AtomicBool::new(false);
        let encoded = AtomicBool::new(false);
        let visited = AtomicBool::new(false);
        let result = export_options_cancellable(
            &request(&source, &output),
            options(LoudnessTarget::default()),
            &cancelled,
            &|time| {
                if !time.is_zero() {
                    encoded.store(true, Ordering::Relaxed);
                }
            },
            &|_| {
                if encoded.load(Ordering::Relaxed) && !visited.swap(true, Ordering::Relaxed) {
                    if mutate {
                        let file = fs::OpenOptions::new()
                            .write(true)
                            .open(&source)
                            .expect("owned loudness fixture operation");
                        file.set_modified(
                            file.metadata()
                                .expect("owned loudness fixture operation")
                                .modified()
                                .expect("owned loudness fixture operation")
                                + Duration::from_secs(1),
                        )
                        .expect("owned loudness fixture operation");
                    } else {
                        cancelled.store(true, Ordering::Relaxed);
                    }
                }
            },
        );
        assert!(
            visited.load(Ordering::Relaxed),
            "must reach encoded-candidate verification"
        );
        if mutate {
            assert!(
                result
                    .expect_err("source mutation must fail")
                    .to_string()
                    .contains("source changed")
            );
        } else {
            assert!(matches!(result, Err(ExportError::Cancelled)));
        }
        assert_eq!(
            fs::read(&output).expect("destination bytes"),
            b"existing destination"
        );
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn loudness_video_audio_only_and_source_save_share_verified_output() {
    let root = root("loudness-video-save");
    let source = root.join("source.mkv");
    crate::export::audio_tests::fixture(&source);
    for (extension, audio_only, hardware) in [
        ("mp4", false, false),
        ("mp4", false, true),
        ("wav", true, false),
    ] {
        let output = root.join(format!("output-{audio_only}-{hardware}.{extension}"));
        let mut request = request(&source, &output);
        request.kind = MediaKind::Video;
        request.hardware_encode = hardware;
        let mut options = options(LoudnessTarget::default());
        if audio_only {
            options.output = ExportOutput::AudioOnly;
        }
        let outcome =
            export_media_with_options(&request, options).expect("verified video/audio-only output");
        eprintln!(
            "PASS loudness video hardware requested={hardware} used={}",
            outcome.used_hardware_encoder
        );
        let measured = measurement(&output, LoudnessTarget::default());
        assert!((measured.integrated + 14.0).abs() <= 0.100001);
        assert!(measured.true_peak < -1.0);
        let probe = ExportStreams::probe(&ExportRequest {
            source: output.clone(),
            ..request
        })
        .expect("owned loudness fixture operation");
        assert_eq!(probe.video.is_some(), !audio_only);
    }
    let audio = root.join("save.wav");
    tone(&audio, "sine=sample_rate=44100:duration=2");
    let original = fs::read(&audio).expect("source bytes");
    let candidate = crate::prepare_source_save(
        crate::FileOperationSource::capture(&audio).expect("owned loudness fixture operation"),
        request(&audio, &audio),
        options(LoudnessTarget::default()),
        &AtomicBool::new(false),
        &|_| {},
        &|_| {},
    )
    .expect("verified unpublished candidate");
    assert_eq!(fs::read(&audio).expect("source bytes"), original);
    let (tx, rx) = std::sync::mpsc::channel();
    crate::commit_source_save(candidate, move |result| {
        tx.send(result).expect("owned loudness fixture operation");
    })
    .expect("owned loudness fixture operation");
    let saved = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("owned loudness fixture operation")
        .expect("native source publication");
    assert!((measurement(&audio, LoudnessTarget::default()).integrated + 14.0).abs() <= 0.100001);
    assert_ne!(fs::read(&audio).expect("source bytes"), original);
    drop(saved);
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn loudness_parser_requires_unique_finite_named_statistics() {
    let statistics = "[astats@towavue_loudness_samples @ 123] Number of samples: 48000\n[astats@towavue_loudness_samples @ 123] Number of NaNs: 0\n[astats@towavue_loudness_samples @ 123] Number of Infs: 0\n[astats@towavue_loudness_samples @ 123] Peak level dB: -4.2\n";
    let json = "[loudnorm@towavue_loudness @ 456]\n{\n\"input_i\" : \"-18.5\",\n\"input_tp\" : \"-4.19\",\n\"input_lra\" : \"10.1\",\n\"input_thresh\" : \"-28.5\",\n\"target_offset\" : \"0.02\"\n}\n";
    let valid = format!("{statistics}{json}");
    assert_eq!(parse(&valid, 48000).expect("valid statistics").range, 10.1);
    for invalid in [
        valid.repeat(2),
        valid.replace("\"-18.5\"", "\"NaN\""),
        valid.replace("\"-4.19\"", "\"inf\""),
        valid.replace("Number of NaNs: 0", "Number of NaNs: 1"),
        valid.replace("towavue_loudness @", "foreign @"),
        format!("{statistics}{statistics}{json}"),
    ] {
        assert!(parse(&invalid, 48000).is_err());
    }
}

#[test]
fn loudness_dynamic_pass_uses_measured_range_above_seven_and_retries_original_input() {
    let root = root("loudness-wide-limiter");
    let source = root.join("source.wav");
    tone(
        &source,
        r"aevalsrc=(0.015*if(lt(t\,8)\,1\,4)+0.7*between(mod(t\,1)\,0.49\,0.4901))*sin(2*PI*431*t):s=48000:d=16",
    );
    let target = LoudnessTarget::default();
    let input = measurement(&source, target);
    let plan = Plan {
        target,
        input,
        correction: 0.0,
        peak_margin: 0.1,
        sample_rate: 48000,
    };
    assert!(input.range > 7.0, "{input:?}");
    let filter = &plan.filters().expect("normalization filters")[0];
    assert!(filter.starts_with("loudnorm="));
    assert!(filter.contains(&format!(":LRA={:.2}:", input.range)));
    let output = root.join("output.m4a");
    let encoded = AtomicBool::new(false);
    let passes = std::sync::atomic::AtomicUsize::new(0);
    export_options_cancellable(
        &request(&source, &output),
        options(target),
        &AtomicBool::new(false),
        &|_| {
            if !encoded.swap(true, Ordering::Relaxed) {
                passes.fetch_add(1, Ordering::Relaxed);
            }
        },
        &|_| {
            encoded.store(false, Ordering::Relaxed);
        },
    )
    .expect("wide-range peak-limited output");
    let measured = measurement(&output, target);
    eprintln!(
        "PASS wide limiter input={input:?} output={measured:?} original-input encodes={}",
        passes.load(Ordering::Relaxed)
    );
    assert!((measured.integrated + 14.0).abs() <= 0.100001);
    assert!(measured.true_peak < -1.0);
    assert!((1..=MAX_ATTEMPTS).contains(&passes.load(Ordering::Relaxed)));
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}
