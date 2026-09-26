use super::*;
use crate::decode::audio_seek_comparison::{alignment, error_at};
use std::fs;
use std::os::windows::process::CommandExt;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use towavue_core::TimelineEdit;

struct Case {
    name: &'static str,
    plan: EditTimeline,
    target: MediaTime,
    end: MediaTime,
    rate: f32,
}

fn cases(duration: MediaTime, numerator: i64) -> Vec<Case> {
    let nanos = MediaTime::from_nanoseconds;
    let base = duration.as_nanoseconds() * numerator / 8;
    let range = |a, b| TimeRange::new(nanos(a), nanos(b)).expect("case range");
    let full = EditTimeline::new(duration, PlaybackRange::default()).expect("timeline");
    let mut result = Vec::new();
    for (name, edit) in [
        (
            "keep",
            TimelineEdit::Keep(range(base - 1_000_000_000, base + 3_000_000_000)),
        ),
        (
            "join",
            TimelineEdit::Delete(range(base + 300_000_000, base + 1_300_000_000)),
        ),
        (
            "gain",
            TimelineEdit::SetVolume(range(base + 250_000_000, base + 500_000_000), 0.25),
        ),
        (
            "stretch",
            TimelineEdit::Stretch(
                range(base + 300_000_000, base + 1_300_000_000),
                nanos(1_500_000_000),
            ),
        ),
    ] {
        let mut plan = full.clone();
        assert!(plan.apply(edit));
        let target = plan
            .edited_time(nanos(base + 137_000_000))
            .expect("kept target");
        result.push(Case {
            name,
            plan,
            target,
            end: target.saturating_add(Duration::from_millis(1200)),
            rate: 1.0,
        });
    }
    for (name, rate) in [("slow", 0.5), ("rate", 1.5), ("fast", 2.0)] {
        result.push(Case {
            name,
            plan: full.clone(),
            target: nanos(base + 137_000_000),
            end: nanos(base + 737_000_000),
            rate,
        });
    }
    let mut tail = full.clone();
    assert!(tail.apply(TimelineEdit::Keep(range(
        base - 1_000_000_000,
        base + 1_000_000_000
    ))));
    result.push(Case {
        name: "tail",
        target: tail.duration().saturating_sub(Duration::from_millis(200)),
        end: tail.duration(),
        plan: tail,
        rate: 1.0,
    });
    result.push(Case {
        name: "origin",
        plan: full,
        target: MediaTime::ZERO,
        end: nanos(500_000_000),
        rate: 1.0,
    });
    result
}

struct Samples {
    values: Vec<f32>,
    first_ms: f64,
    total_ms: f64,
}

fn read(path: &Path, case: &Case, format: AudioFormat, listening: bool) -> Samples {
    let mut values = Vec::new();
    let mut first_ms = None;
    let started = Instant::now();
    let cancelled = AtomicBool::new(false);
    let boundary = |time: MediaTime| {
        crate::tempo::output_sample_boundary(time.as_nanoseconds(), format.sample_rate, case.rate)
    };
    let expected = boundary(case.end) - boundary(case.target);
    let emit = |chunk: decode::AudioChunk| {
        assert_eq!(chunk.format, format);
        assert!(chunk.frames > 0 && chunk.frames <= 1024);
        assert_eq!(chunk.bytes.len(), chunk.frames * 8);
        let offset = ((values.len() / 2) as f64 * 1_000_000_000.0 * f64::from(case.rate)
            / f64::from(format.sample_rate)) as i64;
        assert_eq!(
            chunk.presentation_time.as_nanoseconds(),
            case.target.as_nanoseconds() + offset
        );
        first_ms.get_or_insert_with(|| started.elapsed().as_secs_f64() * 1000.0);
        values.extend(
            chunk
                .bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|sample| f32::from_le_bytes(*sample)),
        );
        assert!(values.len() / 2 <= expected as usize);
        assert!(
            started.elapsed() < Duration::from_secs(120),
            "bounded audio comparison"
        );
        true
    };
    if listening {
        timeline::decode_listening_audio(
            path,
            &case.plan,
            case.target,
            Some(case.end),
            case.rate,
            format,
            &cancelled,
            emit,
        )
    } else {
        timeline::decode_audio(
            path,
            &case.plan,
            case.target,
            Some(case.end),
            case.rate,
            format,
            &cancelled,
            emit,
        )
    }
    .expect("edited audio comparison");
    assert_eq!(
        values.len() / 2,
        expected as usize,
        "{} output count",
        case.name
    );
    assert!(values.iter().all(|value| value.is_finite()));
    Samples {
        values,
        first_ms: first_ms.expect("first audio"),
        total_ms: started.elapsed().as_secs_f64() * 1000.0,
    }
}

fn interior_error(
    fast: &[f32],
    exact: &[f32],
    offset: i32,
    case: &Case,
    format: AudioFormat,
) -> (f64, f64, usize) {
    // Gain/cut boundaries stay on the requested timeline in both outputs.
    // Align the underlying signal without comparing across shifted edit edges.
    let boundary = |time: MediaTime| {
        crate::tempo::output_sample_boundary(time.as_nanoseconds(), format.sample_rate, case.rate)
    };
    let origin = boundary(case.target);
    let mut boundaries = vec![0, (boundary(case.end) - origin) as usize];
    let mut position = MediaTime::ZERO;
    for span in case.plan.spans() {
        position =
            position.saturating_add(Duration::from_nanos(span.duration().as_nanoseconds() as u64));
        if position > case.target && position < case.end {
            boundaries.push((boundary(position) - origin) as usize);
        }
    }
    let margin = offset.unsigned_abs() as usize + 2;
    let fast = &fast[offset.max(0) as usize * 2..];
    let exact_start = (-offset).max(0) as usize;
    let exact = &exact[exact_start * 2..];
    let mut squared = 0.0;
    let mut peak = 0.0_f64;
    let mut count = 0;
    let mut excluded = 0;
    for (index, (fast, exact)) in fast
        .as_chunks::<2>()
        .0
        .iter()
        .zip(exact.as_chunks::<2>().0.iter())
        .enumerate()
    {
        if boundaries
            .iter()
            .any(|boundary| (index + exact_start).abs_diff(*boundary) <= margin)
        {
            excluded += 1;
            continue;
        }
        for (fast, exact) in fast.iter().zip(exact) {
            let error = f64::from(*fast) - f64::from(*exact);
            squared += error * error;
            peak = peak.max(error.abs());
            count += 1;
        }
    }
    assert!(count > 0);
    ((squared / count as f64).sqrt(), peak, excluded)
}

fn compare(path: &Path, exact_equality: bool, exact_interiors: bool, numerator: i64) {
    let stamp = || {
        let metadata = fs::metadata(path).expect("source");
        (metadata.len(), metadata.modified().expect("mtime"))
    };
    let before = stamp();
    let format = decode::probe_playback_formats(path)
        .expect("format")
        .0
        .expect("audio");
    assert_eq!(format.channels, 2);
    let input = ffmpeg_next::format::input(path).expect("duration");
    let duration =
        MediaTime::from_nanoseconds(input.duration().checked_mul(1000).expect("duration range"));
    drop(input);
    assert!(duration > MediaTime::from_nanoseconds(8_000_000_000));
    for (index, case) in cases(duration, numerator).into_iter().enumerate() {
        let (exact, fast) = if index % 2 == 0 {
            let exact = read(path, &case, format, false);
            (exact, read(path, &case, format, true))
        } else {
            let fast = read(path, &case, format, true);
            (read(path, &case, format, false), fast)
        };
        let unit_rate = case.rate == 1.0
            && case
                .plan
                .spans()
                .iter()
                .all(|span| span.duration() == span.source().duration());
        if exact_equality || !unit_rate {
            assert!(
                fast.values == exact.values,
                "{} exact fallback PCM",
                case.name
            );
        }
        let bound = format.sample_rate as i32 / 100;
        let offset = alignment(&fast.values, &exact.values, bound);
        let (raw_rms, raw_peak) = error_at(&fast.values, &exact.values, 0, 1);
        let (aligned_rms, aligned_peak) = error_at(&fast.values, &exact.values, offset, 1);
        let (interior_rms, interior_peak, excluded_frames) =
            interior_error(&fast.values, &exact.values, offset, &case, format);
        let interior_equal = interior_peak == 0.0;
        let pcm_equal = fast.values == exact.values;
        if exact_interiors {
            assert_eq!(
                interior_peak, 0.0,
                "{} aligned non-boundary waveform",
                case.name
            );
        }
        assert!(
            offset.abs() <= (format.sample_rate / 1000 + 1) as i32,
            "{} sub-millisecond sample phase",
            case.name
        );
        let signal_rms = (exact
            .values
            .iter()
            .map(|value| f64::from(*value).powi(2))
            .sum::<f64>()
            / exact.values.len() as f64)
            .sqrt();
        println!(
            "EDITED_AUDIO case={} fraction={numerator}/8 rate={} sample_rate={} exact_first_ms={:.3} listening_first_ms={:.3} exact_total_ms={:.3} listening_total_ms={:.3} frames={} lag_samples={offset} lag_ms={:.6} at_bound={} signal_rms={signal_rms:.9} raw_rms={raw_rms:.9} raw_peak={raw_peak:.9} aligned_rms={aligned_rms:.9} aligned_peak={aligned_peak:.9} interior_rms={interior_rms:.9} interior_peak={interior_peak:.9} excluded_frames={excluded_frames} interior_equal={interior_equal} pcm_equal={pcm_equal}",
            case.name,
            case.rate,
            format.sample_rate,
            exact.first_ms,
            fast.first_ms,
            exact.total_ms,
            fast.total_ms,
            exact.values.len() / 2,
            f64::from(offset) * 1000.0 / f64::from(format.sample_rate),
            offset.abs() == bound
        );
    }
    assert_eq!(stamp(), before);
    println!(
        "EDITED_AUDIO_CHECKS cases=9 counts_pts_bounds=true source_stamp_unchanged=true playback=false"
    );
}

fn fixture(rate: u32, codec: &str, pns: bool) -> (PathBuf, PathBuf) {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "towavue-edited-listening-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&root).expect("owned fixture");
    let path = root.join("source.mkv");
    let executable =
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg")).join("bin/ffmpeg.exe");
    let mut command = std::process::Command::new(executable);
    command.creation_flags(0x0800_0000).args(["-v", "error", "-n", "-f", "lavfi", "-i"])
        .arg(format!("aevalsrc=0.15*sin(2*PI*(271*t+17*t*t))+0.07*sin(2*PI*997*t)|0.13*sin(2*PI*(419*t+23*t*t))+0.05*sin(2*PI*733*t):s={rate}:d=12"))
        .args(["-c:a", codec]);
    if codec == "aac" {
        command.args(["-b:a", "192k", "-aac_pns", if pns { "1" } else { "0" }]);
    }
    let generated = command.arg(&path).output().expect("generate fixture");
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    (root, path)
}

#[test]
fn aligned_interior_check_excludes_only_the_bounded_edit_edges() {
    let nanos = MediaTime::from_nanoseconds;
    let mut plan = EditTimeline::new(nanos(1_000_000_000), PlaybackRange::default()).expect("plan");
    assert!(plan.apply(TimelineEdit::SetVolume(
        TimeRange::new(nanos(5_000_000), nanos(10_000_000)).expect("gain range"),
        0.5
    )));
    let case = Case {
        name: "control",
        plan,
        target: MediaTime::ZERO,
        end: nanos(20_000_000),
        rate: 1.0,
    };
    let format = AudioFormat {
        sample_rate: 48000,
        channels: 2,
    };
    let exact: Vec<_> = (0..1920).map(|index| (index % 37) as f32 / 37.0).collect();
    let mut fast = vec![0.0; 6];
    fast.extend_from_slice(&exact);
    fast[(240 + 3) * 2] += 1.0;
    let (rms, peak, excluded) = interior_error(&fast, &exact, 3, &case, format);
    assert_eq!((rms, peak), (0.0, 0.0));
    assert!(excluded > 0 && excluded < 40);
    fast[(64 + 3) * 2] += 0.25;
    let (rms, peak, _) = interior_error(&fast, &exact, 3, &case, format);
    assert!(rms > 0.0 && (peak - 0.25).abs() < 1e-7);
}

#[test]
fn listening_timeline_preserves_exact_pcm_edits_and_cancellation() {
    let (root, path) = fixture(48000, "pcm_f32le", false);
    compare(&path, true, true, 3);
    let plan = EditTimeline::new(
        MediaTime::from_nanoseconds(12_000_000_000),
        PlaybackRange::default(),
    )
    .expect("timeline");
    let result = timeline::decode_listening_audio(
        &path,
        &plan,
        MediaTime::ZERO,
        None,
        1.0,
        AudioFormat {
            sample_rate: 48000,
            channels: 2,
        },
        &AtomicBool::new(true),
        |_| panic!("cancelled output"),
    );
    assert!(matches!(result, Err(decode::DecodeError::ConsumerClosed)));
    fs::remove_dir_all(root).expect("owned fixture");
}

#[test]
fn unit_rate_edited_aac_preserves_interior_waveform_and_tempo_fallback() {
    for rate in [44100, 48000] {
        let (root, path) = fixture(rate, "aac", false);
        compare(&path, false, true, 3);
        fs::remove_dir_all(root).expect("owned fixture");
    }
}

#[test]
#[ignore = "Release generated edited AAC comparison; no playback or UI"]
fn generated_edited_aac_reports_listening_alignment() {
    if cfg!(debug_assertions) {
        panic!("use Release for timing");
    }
    for rate in [44100, 48000] {
        for pns in [false, true] {
            let (root, path) = fixture(rate, "aac", pns);
            println!("EDITED_AUDIO_SOURCE sample_rate={rate} pns={pns}");
            compare(&path, false, !pns, 3);
            fs::remove_dir_all(root).expect("owned fixture");
        }
    }
}

#[test]
#[ignore = "Release read-only owner reference: edited listening PCM comparison without playback"]
fn reference_edited_audio_reports_listening_alignment() {
    if cfg!(debug_assertions) {
        panic!("use Release for timing");
    }
    let path = PathBuf::from(
        std::env::var_os("TOWAVUE_SEEK_REFERENCE_SOURCE").expect("explicit reference"),
    );
    for numerator in [2, 4, 6] {
        compare(&path, false, false, numerator);
    }
    println!("EDITED_AUDIO_REFERENCE_CHECKS positions=3 cases=27 playback=false");
}
