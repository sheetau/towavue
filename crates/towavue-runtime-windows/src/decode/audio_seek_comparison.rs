use super::*;
use std::os::windows::process::CommandExt;
use std::time::Instant;

struct Samples {
    values: Vec<f32>,
    rate: u32,
    first_ms: f64,
    total_ms: f64,
}

fn read(path: &Path, target: MediaTime, approximate: bool) -> Samples {
    let mut input = ParallelInput::open(path, &|| false).expect("read-only audio input");
    let config = best_stream_config(&input.input, Type::Audio).expect("audio stream");
    assert!(
        matches!(
            config.sample_preroll(target),
            Some((_, PrerollSamples::AacLc(_)))
        ),
        "coarse AAC-LC reference required"
    );
    let mut values = Vec::new();
    let mut rate = 0;
    let mut first_ms = None;
    let started = Instant::now();
    let end = target.saturating_add(Duration::from_secs(1));
    let emit = |output| {
        if let ParallelSoftwareDecodeOutput::Item(DecodeOutput::Audio(mut chunk)) = output {
            clip_audio_chunk(&mut chunk, target, Some(end));
            if chunk.frames > 0 {
                assert_eq!(chunk.format.channels, 2);
                first_ms.get_or_insert_with(|| started.elapsed().as_secs_f64() * 1000.0);
                if rate != 0 {
                    assert_eq!(rate, chunk.format.sample_rate);
                }
                rate = chunk.format.sample_rate;
                values.extend(
                    chunk
                        .bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|sample| f32::from_ne_bytes(*sample)),
                );
            }
        }
        true
    };
    let cancelled = || started.elapsed() > Duration::from_secs(120);
    if approximate {
        input.decode_playback_audio(target, Some(end), &cancelled, emit)
    } else {
        input.decode_software(
            target,
            Some(end),
            Some(DecodeStream::Audio),
            &cancelled,
            emit,
        )
    }
    .expect("bounded audio comparison");
    assert!(values.iter().all(|value| value.is_finite()));
    assert!(
        values.len() >= rate as usize,
        "at least half a second of stereo output"
    );
    Samples {
        values,
        rate,
        first_ms: first_ms.expect("first audio"),
        total_ms: started.elapsed().as_secs_f64() * 1000.0,
    }
}

pub(crate) fn error_at(fast: &[f32], exact: &[f32], offset: i32, stride: usize) -> (f64, f64) {
    let fast = &fast[offset.max(0) as usize * 2..];
    let exact = &exact[(-offset).max(0) as usize * 2..];
    let mut squared = 0.0;
    let mut peak = 0.0_f64;
    let mut count = 0;
    for (fast, exact) in fast.iter().zip(exact).step_by(stride) {
        let error = f64::from(*fast) - f64::from(*exact);
        squared += error * error;
        peak = peak.max(error.abs());
        count += 1;
    }
    assert!(count > 0);
    ((squared / count as f64).sqrt(), peak)
}

pub(crate) fn alignment(fast: &[f32], exact: &[f32], bound: i32) -> i32 {
    (-bound..=bound)
        .min_by(|a, b| {
            error_at(fast, exact, *a, 16)
                .0
                .total_cmp(&error_at(fast, exact, *b, 16).0)
                .then_with(|| a.abs().cmp(&b.abs()))
        })
        .expect("alignment range")
}

fn report(path: &Path, targets: &[MediaTime], require_aligned_equality: bool) {
    let stamp = || {
        let metadata = std::fs::metadata(path).expect("source metadata");
        (
            metadata.len(),
            metadata.modified().expect("source timestamp"),
        )
    };
    let before = stamp();
    for (index, target) in targets.iter().copied().enumerate() {
        let (exact, fast) = if index % 2 == 0 {
            let exact = read(path, target, false);
            (exact, read(path, target, true))
        } else {
            let fast = read(path, target, true);
            (read(path, target, false), fast)
        };
        assert_eq!(fast.rate, exact.rate);
        let bound = exact.rate as i32 / 100;
        let offset = alignment(&fast.values, &exact.values, bound);
        let (raw_rms, raw_peak) = error_at(&fast.values, &exact.values, 0, 1);
        let (aligned_rms, aligned_peak) = error_at(&fast.values, &exact.values, offset, 1);
        let aligned_equal = aligned_rms == 0.0;
        if require_aligned_equality {
            assert!(
                aligned_equal,
                "PNS-disabled waveform changed after alignment: offset={offset}, rms={aligned_rms}, peak={aligned_peak}"
            );
        }
        let signal_rms = (exact
            .values
            .iter()
            .map(|value| f64::from(*value).powi(2))
            .sum::<f64>()
            / exact.values.len() as f64)
            .sqrt();
        println!(
            "AAC_SEEK index={index} target_ns={} rate={} exact_first_ms={:.3} fast_first_ms={:.3} exact_total_ms={:.3} fast_total_ms={:.3} exact_frames={} fast_frames={} fast_early_frames={offset} offset_ms={:.6} alignment_at_bound={} signal_rms={signal_rms:.9} raw_rms={raw_rms:.9} raw_peak={raw_peak:.9} aligned_rms={aligned_rms:.9} aligned_peak={aligned_peak:.9} aligned_equal={aligned_equal}",
            target.as_nanoseconds(),
            exact.rate,
            exact.first_ms,
            fast.first_ms,
            exact.total_ms,
            fast.total_ms,
            exact.values.len() / 2,
            fast.values.len() / 2,
            f64::from(offset) * 1000.0 / f64::from(exact.rate),
            offset.abs() == bound
        );
        assert_eq!(
            stamp(),
            before,
            "reference bytes length/timestamp unchanged"
        );
    }
}

#[test]
fn alignment_detects_known_stereo_offsets_and_retains_waveform_error() {
    let mut seed = 173_u32;
    let values: Vec<_> = (0..4096)
        .map(|_| {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 16) as f32 / 65536.0
        })
        .collect();
    for offset in [0, 3, -5] {
        let fast = &values[(-offset).max(0) as usize * 2..];
        let exact = &values[offset.max(0) as usize * 2..];
        assert_eq!(alignment(fast, exact, 10), offset);
        assert_eq!(error_at(fast, exact, offset, 1), (0.0, 0.0));
        let changed: Vec<_> = fast.iter().map(|value| value + 0.125).collect();
        assert_eq!(alignment(&changed, exact, 10), offset);
        let (rms, peak) = error_at(&changed, exact, offset, 1);
        assert!((rms - 0.125).abs() < 1e-8 && (peak - 0.125).abs() < 1e-8);
    }
}

#[test]
#[ignore = "Release timing; generated coarse AAC at two sample rates, no playback"]
fn generated_aac_reports_fast_seek_alignment() {
    if cfg!(debug_assertions) {
        panic!("use Release for timing");
    }
    for rate in [44_100, 48_000] {
        let (root, source, _) = audio_seek_tests::fixture(rate, 2, "pcm_f32le", 12, 1024);
        let executable =
            std::path::PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg"))
                .join("bin/ffmpeg.exe");
        for pns in ["0", "1"] {
            let encoded = root.join(format!("aac-pns-{pns}.mkv"));
            let output = std::process::Command::new(&executable)
                .creation_flags(0x0800_0000)
                .args(["-v", "error", "-n", "-i"])
                .arg(&source)
                .args(["-c:a", "aac", "-aac_pns", pns])
                .arg(&encoded)
                .output()
                .expect("owned AAC fixture");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            println!("AAC_SOURCE rate={rate} pns={pns}");
            report(
                &encoded,
                &[617_000_000, 6_137_000_000, 9_731_000_000].map(MediaTime::from_nanoseconds),
                pns == "0",
            );
        }
        std::fs::remove_dir_all(root).expect("remove owned fixture");
    }
}

#[test]
#[ignore = "Release read-only owner reference; compares PCM in memory without playing or saving audio"]
fn reference_aac_reports_fast_seek_alignment() {
    if cfg!(debug_assertions) {
        panic!("use Release for timing");
    }
    let path = std::path::PathBuf::from(
        std::env::var_os("TOWAVUE_SEEK_REFERENCE_SOURCE").expect("explicit reference"),
    );
    let input = format::input(&path).expect("read-only duration");
    let duration = input.duration();
    assert!(duration > 4_000_000, "reference duration");
    drop(input);
    report(
        &path,
        &[2, 3, 1, 2].map(|part| MediaTime::from_nanoseconds(duration * 1000 * part / 4)),
        false,
    );
}
