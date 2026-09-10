use super::*;
use crate::export::audio_tests::{ffmpeg, fixture, pcm, root};
use std::os::windows::process::CommandExt;
use towavue_core::{MediaTime, TimeRange, TimelineEdit};

fn samples(path: &Path) -> Vec<f32> {
    pcm(path)
        .as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| f32::from_le_bytes(*bytes))
        .collect()
}

fn peak(samples: &[f32]) -> f64 {
    samples
        .iter()
        .map(|value| f64::from(value.abs()))
        .fold(0.0, f64::max)
}

fn request(source: &Path, target: PathBuf) -> ExportRequest {
    ExportRequest {
        source: source.to_owned(),
        target,
        kind: MediaKind::Audio,
        operations: vec![],
        hardware_encode: false,
    }
}

fn options(normalize_peak: bool, channels: AudioChannels) -> ExportOptions {
    ExportOptions {
        output: ExportOutput::Media,
        audio: AudioExportOptions {
            normalize_peak,
            channels,
        },
        ..Default::default()
    }
}

fn assert_near(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1.0 / 32768.0,
        "{actual} vs {expected}"
    );
}

#[test]
fn audio_options_have_explicit_channel_rules_and_reject_unreliable_statistics() {
    assert!(
        AudioExportOptions::default()
            .filters(None)
            .expect("disabled")
            .is_empty()
    );
    for channels in [None, Some(0), Some(6)] {
        assert!(
            options(false, AudioChannels::Mono)
                .audio
                .filters(channels)
                .is_err()
        );
        assert!(
            options(false, AudioChannels::Stereo)
                .audio
                .filters(channels)
                .is_err()
        );
    }
    assert!(
        options(true, AudioChannels::Keep)
            .audio
            .filters(Some(6))
            .is_ok()
    );
    let stats = |peak, samples, nans, infs| {
        format!(
            "[astats@towavue_peak @ 1234] Peak level dB: {peak}\n[astats@towavue_peak @ 1234] Number of samples: {samples}\n[astats@towavue_peak @ 1234] Number of NaNs: {nans}\n[astats@towavue_peak @ 1234] Number of Infs: {infs}\n"
        )
    };
    assert_eq!(
        gain_from_statistics(&stats("-inf", "20", "0", "0")).expect("silence"),
        1.0
    );
    assert_eq!(
        gain_from_statistics(&stats("-1", "20", "0", "0")).expect("already normalized"),
        1.0
    );
    for log in [
        String::new(),
        stats("nan", "20", "0", "0"),
        stats("inf", "20", "0", "0"),
        stats("-3", "0", "0", "0"),
        stats("-3", "nan", "0", "0"),
        stats("-3", "20", "1", "0"),
        stats("-3", "20", "0", "1"),
        stats("-3", "20", "0", "0").repeat(2),
        stats("-3", "20", "0", "0").replace("towavue_peak", "foreign"),
    ] {
        assert!(
            gain_from_statistics(&log).is_err(),
            "invalid statistics: {log}"
        );
    }
}

#[test]
fn audio_channels_average_stereo_duplicate_mono_and_normalize_after_the_mix() {
    let root = root("channel-options");
    let target_peak = 10_f64.powf(-1.0 / 20.0);
    for (index, expression, channels, expected) in [
        (0, "0.125|0.375", AudioChannels::Mono, vec![0.25]),
        (1, "0.125|-0.375", AudioChannels::Mono, vec![-0.125]),
        (2, "0.125|-0.125", AudioChannels::Mono, vec![0.0]),
        (3, "0.25", AudioChannels::Stereo, vec![0.25, 0.25]),
        (4, "0.125|0.375", AudioChannels::Stereo, vec![0.125, 0.375]),
        (5, "0.25", AudioChannels::Mono, vec![0.25]),
    ] {
        let source = root.join(format!("source-{index}.wav"));
        ffmpeg(
            &[
                "-f",
                "lavfi",
                "-i",
                &format!("aevalsrc={expression}:s=48000:d=0.1"),
                "-c:a",
                "pcm_f64le",
            ],
            &source,
        );
        let original = fs::read(&source).expect("source");
        for normalize in [false, true] {
            let request = request(
                &source,
                root.join(format!("output-{index}-{normalize}.wav")),
            );
            export_media_with_options(&request, options(normalize, channels))
                .expect("channel export");
            let actual = samples(&request.target);
            assert_eq!(actual.len(), 4800 * expected.len());
            let maximum = expected.iter().copied().map(f64::abs).fold(0.0, f64::max);
            let gain = if normalize && maximum > 0.0 {
                target_peak / maximum
            } else {
                1.0
            };
            for frame in actual.chunks_exact(expected.len()) {
                for (sample, expected) in frame.iter().zip(&expected) {
                    assert_near(f64::from(*sample), expected * gain);
                }
            }
        }
        assert_eq!(fs::read(&source).expect("unchanged source"), original);
    }
    fs::remove_dir_all(&root).expect("owned fixture cleanup");
}

#[test]
fn peak_normalization_preserves_dynamics_silence_quiet_and_overfull_float_samples() {
    let root = root("peak-options");
    let target_peak = 10_f64.powf(-1.0 / 20.0);
    for (index, expression, expected) in [
        (0, "0", vec![0.0]),
        (
            1,
            "0.000001|0.0000005",
            vec![target_peak, target_peak / 2.0],
        ),
        (2, "1.5|-0.75", vec![target_peak, -target_peak / 2.0]),
        (3, "if(lt(t\\,0.05)\\,0.25\\,0.0625)", vec![target_peak]),
    ] {
        let source = root.join(format!("source-{index}.wav"));
        ffmpeg(
            &[
                "-f",
                "lavfi",
                "-i",
                &format!("aevalsrc={expression}:s=48000:d=0.1"),
                "-c:a",
                "pcm_f64le",
            ],
            &source,
        );
        let mut request = request(&source, root.join(format!("output-{index}.flac")));
        request.operations.push(EditOperation::SetVolume(2.0));
        export_media_with_options(&request, options(true, AudioChannels::Keep))
            .expect("peak normalization");
        let actual = samples(&request.target);
        assert_eq!(actual.len(), 4800 * expected.len());
        for (frame_index, frame) in actual.chunks_exact(expected.len()).enumerate() {
            for (sample, expected) in frame.iter().zip(&expected) {
                let expected = if index == 3 && frame_index >= 2400 {
                    expected / 4.0
                } else {
                    *expected
                };
                assert_near(f64::from(*sample), expected);
            }
        }
    }
    fs::remove_dir_all(&root).expect("owned fixture cleanup");
}

#[test]
fn normalization_progress_and_cancellation_keep_existing_target_and_detect_source_changes() {
    let root = root("normalization-protection");
    let source = root.join("source.mkv");
    fixture(&source);
    let mut request = request(&source, root.join("existing.wav"));
    request.kind = MediaKind::Video;
    let options = ExportOptions {
        output: ExportOutput::AudioOnly,
        ..options(true, AudioChannels::Stereo)
    };
    let existing = b"existing target";
    fs::write(&request.target, existing).expect("owned target");
    for phase in 0..3 {
        let cancelled = AtomicBool::new(false);
        let seen_analysis = AtomicBool::new(false);
        let seen_encode = AtomicBool::new(false);
        let error = export_options_cancellable(
            &request,
            options.clone(),
            &cancelled,
            &|time| {
                assert!(
                    seen_analysis.load(Ordering::Relaxed),
                    "analysis precedes encoding"
                );
                seen_encode.store(true, Ordering::Relaxed);
                if phase == 1 && time > Duration::ZERO {
                    cancelled.store(true, Ordering::Relaxed);
                }
                if phase == 2 && time == Duration::ZERO {
                    let file = fs::OpenOptions::new()
                        .write(true)
                        .open(&source)
                        .expect("owned source after analysis");
                    let modified = file
                        .metadata()
                        .expect("metadata")
                        .modified()
                        .expect("mtime");
                    file.set_modified(modified + Duration::from_secs(1))
                        .expect("simulate external source change");
                }
            },
            &|time| {
                seen_analysis.store(true, Ordering::Relaxed);
                if phase == 0 && time > Duration::ZERO {
                    cancelled.store(true, Ordering::Relaxed);
                }
            },
        )
        .expect_err("cancel or source change");
        if phase < 2 {
            assert!(matches!(error, ExportError::Cancelled));
        } else {
            assert!(error.to_string().contains("source changed"), "{error}");
        }
        assert!(seen_analysis.load(Ordering::Relaxed));
        assert_eq!(seen_encode.load(Ordering::Relaxed), phase != 0);
        assert_eq!(
            fs::read(&request.target).expect("target retained"),
            existing
        );
    }
    fs::remove_dir_all(&root).expect("owned fixture cleanup");
}

fn video_pixels(path: &Path) -> Vec<u8> {
    let output = Command::new(crate::media_tools::tool_path("ffmpeg.exe").expect("fixed FFmpeg"))
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-v", "error", "-i"])
        .arg(path)
        .args([
            "-map", "0:v:0", "-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1",
        ])
        .output()
        .expect("decode video only");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

#[test]
fn normalization_uses_edited_best_audio_and_matches_video_and_audio_only_without_changing_video() {
    let root = root("normalized-timeline");
    let source = root.join("source.mkv");
    fixture(&source);
    let original = fs::read(&source).expect("original");
    let time = |ms: i64| MediaTime::from_nanoseconds(ms * 1_000_000);
    let range = |start, end| TimeRange::new(time(start), time(end)).expect("range");
    let mut request = request(&source, root.join("baseline.avi"));
    request.kind = MediaKind::Video;
    request.operations = vec![
        EditOperation::SetTrimStart(time(200)),
        EditOperation::SetTrimEnd(time(1000)),
        EditOperation::Timeline(TimelineEdit::Delete(range(200, 400))),
        EditOperation::Timeline(TimelineEdit::Stretch(range(0, 200), time(400))),
        EditOperation::Timeline(TimelineEdit::SetVolume(range(0, 400), 0.25)),
        EditOperation::SetVolume(0.5),
        EditOperation::SetRate(2.0),
        EditOperation::RotateClockwise,
    ];
    export_media(&request).expect("baseline video");
    let baseline = video_pixels(&request.target);
    assert!(!baseline.is_empty());
    request.target = root.join("normalized.avi");
    export_media_with_options(&request, options(true, AudioChannels::Stereo))
        .expect("normalized video");
    assert_eq!(
        video_pixels(&request.target),
        baseline,
        "normalization does not change video pixels/frame count"
    );
    let video_audio = samples(&request.target);
    request.target = root.join("normalized.wav");
    let derivative = ExportOptions {
        output: ExportOutput::AudioOnly,
        ..options(true, AudioChannels::Stereo)
    };
    export_media_with_options(&request, derivative).expect("normalized derivative");
    assert_eq!(
        samples(&request.target),
        video_audio,
        "same edited samples in both export modes"
    );
    let reference = root.join("unscaled-reference.wav");
    let graph = "[0:2]asplit=2[a][b];[a]atrim=start_pts=9600:end_pts=19200,asetpts=PTS-STARTPTS,aformat=sample_fmts=flt,volume=0.125,apad=whole_len=9600,atrim=end_sample=9600[x];[b]atrim=start_pts=28800:end_pts=48000,asetpts=PTS-STARTPTS,aformat=sample_fmts=flt,atempo=2,volume=0.5,apad=whole_len=9600,atrim=end_sample=9600[y];[x][y]concat=n=2:v=0:a=1[out]";
    ffmpeg(
        &[
            "-i",
            source.to_str().expect("path"),
            "-copyts",
            "-start_at_zero",
            "-filter_complex",
            graph,
            "-map",
            "[out]",
            "-c:a",
            "pcm_f64le",
        ],
        &reference,
    );
    let reference = samples(&reference);
    assert_eq!(reference.len(), 19200);
    assert_eq!(video_audio.len(), reference.len() * 2);
    let target_peak = 10_f64.powf(-1.0 / 20.0);
    let gain = target_peak / peak(&reference);
    for (frame, reference) in video_audio.as_chunks::<2>().0.iter().zip(reference) {
        assert_eq!(frame[0], frame[1], "mono duplication");
        assert_near(f64::from(frame[0]), f64::from(reference) * gain);
    }
    assert_near(peak(&video_audio), target_peak);
    // Both actual encode branches receive the same post-edit channel/gain chain.
    let mut streams = ExportStreams::probe(&request).expect("streams");
    streams.audio_post_filters = options(true, AudioChannels::Stereo)
        .audio
        .filters(streams.audio_channels)
        .expect("post filters");
    streams
        .audio_post_filters
        .push(format!("volume={gain:.17e}:precision=double"));
    let hardware = ffmpeg_arguments(&request, true, &streams);
    let software = ffmpeg_arguments(&request, false, &streams);
    let filter = |arguments: Vec<String>| {
        arguments[arguments
            .iter()
            .position(|argument| argument == "-af")
            .expect("audio chain")
            + 1]
        .clone()
    };
    assert_eq!(filter(hardware), filter(software));
    assert_eq!(fs::read(&source).expect("original retained"), original);
    fs::remove_dir_all(&root).expect("owned fixture cleanup");
}

#[test]
fn audio_options_reject_nonfinite_empty_absent_and_multichannel_conversion_before_replacement() {
    let root = root("invalid-audio-options");
    let source = root.join("source.wav");
    let mut request = request(&source, root.join("existing.flac"));
    let existing = b"existing output";
    for expression in ["0/0", "1/0", "-1/0"] {
        ffmpeg(
            &[
                "-f",
                "lavfi",
                "-i",
                &format!("aevalsrc={expression}:s=48000:d=0.1"),
                "-c:a",
                "pcm_f64le",
            ],
            &source,
        );
        fs::write(&request.target, existing).expect("existing target");
        assert!(
            export_media_with_options(&request, options(true, AudioChannels::Keep)).is_err(),
            "invalid float audio"
        );
        assert_eq!(
            fs::read(&request.target).expect("target retained"),
            existing
        );
    }
    ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            "aevalsrc=0.125|0.25|0.375|0.5|0.625|0.75:c=5.1:s=48000:d=0.1",
            "-c:a",
            "pcm_f64le",
        ],
        &source,
    );
    export_media_with_options(&request, options(true, AudioChannels::Keep))
        .expect("keep six channels");
    let actual = samples(&request.target);
    assert_eq!(actual.len(), 4800 * 6);
    for frame in actual.as_chunks::<6>().0 {
        for (index, sample) in frame.iter().enumerate() {
            assert_near(
                f64::from(*sample),
                10_f64.powf(-0.05) * (index + 1) as f64 / 6.0,
            );
        }
    }
    for channels in [AudioChannels::Mono, AudioChannels::Stereo] {
        fs::write(&request.target, existing).expect("restore owned target");
        assert!(
            export_media_with_options(&request, options(true, channels))
                .expect_err("explicit downmix rejection")
                .to_string()
                .contains("mono or stereo source")
        );
        assert_eq!(
            fs::read(&request.target).expect("target retained"),
            existing
        );
    }
    request.operations = vec![EditOperation::SetTrimStart(MediaTime::from_nanoseconds(
        1_000_000_000,
    ))];
    assert!(
        export_media_with_options(&request, options(true, AudioChannels::Keep)).is_err(),
        "empty analysis cannot publish"
    );
    request.operations.clear();
    request.kind = MediaKind::Image;
    assert!(export_media_with_options(&request, options(true, AudioChannels::Keep)).is_err());
    request.kind = MediaKind::Video;
    request.source = root.join("video-only.mkv");
    ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            "color=size=64x48:rate=4:duration=0.1",
            "-c:v",
            "ffv1",
        ],
        &request.source,
    );
    assert!(
        export_media_with_options(&request, options(true, AudioChannels::Keep))
            .expect_err("no audio")
            .to_string()
            .contains("audio stream")
    );
    assert_eq!(
        fs::read(&request.target).expect("target retained"),
        existing
    );
    assert!(!fs::read_dir(&root).expect("directory").any(|entry| {
        entry
            .expect("entry")
            .file_name()
            .to_string_lossy()
            .starts_with(".towavue-export-")
    }));
    fs::remove_dir_all(&root).expect("owned fixture cleanup");
}

#[test]
fn normalization_job_reports_analysis_then_encoding_and_finishes_once() {
    let root = root("normalization-job");
    let source = root.join("source.mkv");
    fixture(&source);
    let mut request = request(&source, root.join("result.wav"));
    request.kind = MediaKind::Video;
    let (sender, receiver) = std::sync::mpsc::channel();
    let job = ExportJob::start_with_options(
        request.clone(),
        ExportOptions {
            output: ExportOutput::AudioOnly,
            ..options(true, AudioChannels::Mono)
        },
        move |event| {
            sender.send(event).expect("event receiver");
        },
    )
    .expect("export job");
    let mut analysis = 0;
    let mut encoding = 0;
    loop {
        match receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("event")
        {
            ExportEvent::AnalyzingAudio(_) => {
                assert_eq!(encoding, 0);
                analysis += 1;
            }
            ExportEvent::Progress(_) => {
                assert!(analysis > 0);
                encoding += 1;
            }
            ExportEvent::Finished(result) => {
                assert!(result.is_ok(), "{result:?}");
                break;
            }
        }
    }
    drop(job);
    assert!(
        analysis > 1 && encoding > 1,
        "start and measured progress in both phases"
    );
    assert!(receiver.try_recv().is_err(), "only one completion");
    assert!(!samples(&request.target).is_empty());
    fs::remove_dir_all(&root).expect("owned fixture cleanup");
}
