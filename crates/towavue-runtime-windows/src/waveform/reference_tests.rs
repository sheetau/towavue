//! Explicit, read-only source assessment; never run in the ordinary suite.

use super::{DisplayEnvelope, native, timeline_waveform};
use crate::{Cancellation, decode, playback::timeline};
use std::{path::PathBuf, sync::atomic::AtomicBool, time::Instant};
use towavue_core::{EditTimeline, MediaTime, TimeRange, TimelineEdit};
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessIoCounters, IO_COUNTERS};

fn read_bytes() -> u64 {
    let mut counters = IO_COUNTERS::default();
    // The pseudo-handle needs no close. Windows writes only this live local
    // structure, retaining no pointer and changing no process state.
    unsafe { GetProcessIoCounters(GetCurrentProcess(), &mut counters) }.expect("process I/O");
    counters.ReadTransferCount
}

#[test]
#[ignore = "Release read-only reference: overview and exact edited waveform costs"]
fn reference_waveform_reports_decode_and_envelope_cost() {
    if cfg!(debug_assertions) {
        panic!("use Release for timing");
    }
    let path = PathBuf::from(
        std::env::var_os("TOWAVUE_SEEK_REFERENCE_SOURCE").expect("explicit reference source"),
    );
    let stamp = || {
        let metadata = std::fs::metadata(&path).expect("source metadata");
        (metadata.len(), metadata.modified().expect("source mtime"))
    };
    let before = stamp();
    ffmpeg_next::init().expect("FFmpeg initialization");
    let input = ffmpeg_next::format::input(&path).expect("source duration");
    let duration_ns = input.duration().checked_mul(1000).expect("duration bounds");
    assert!(
        duration_ns > 20_000_000_000,
        "reference must exceed 20 seconds"
    );
    drop(input);
    let duration = MediaTime::from_nanoseconds(duration_ns);
    let started = Instant::now();
    let format = decode::probe_audio_format(&path)
        .expect("audio probe")
        .expect("reference audio");
    println!(
        "WAVEFORM_PROBE ms={:.3}",
        started.elapsed().as_secs_f64() * 1000.0
    );

    let io_before = read_bytes();
    let started = Instant::now();
    let overview =
        native::decode(&path, 1024, 64, &Cancellation::default()).expect("native source overview");
    let elapsed = started.elapsed();
    let bytes = read_bytes() - io_before;
    assert_eq!(overview.dimensions(), (1024, 64));
    assert!(
        overview.pixels().any(|pixel| pixel[3] != 0),
        "non-silent source"
    );
    println!(
        "WAVEFORM_OVERVIEW ms={:.3} read_bytes={bytes}",
        elapsed.as_secs_f64() * 1000.0
    );

    for (label, interior, rate) in [
        ("whole", false, 1.0),
        ("middle", true, 1.0),
        ("retimed", true, 1.1),
    ] {
        let mut plan = EditTimeline::new(duration, Default::default()).expect("whole source plan");
        if interior {
            let begin = MediaTime::from_nanoseconds(duration_ns / 2);
            let end = MediaTime::from_nanoseconds(duration_ns / 2 + 8_000_000_000);
            assert!(plan.apply(TimelineEdit::Keep(
                TimeRange::new(begin, end).expect("middle range")
            )));
        }
        let expected_frames = crate::tempo::output_sample_boundary(
            plan.duration().as_nanoseconds(),
            format.sample_rate,
            rate,
        );
        let mut envelope = DisplayEnvelope::new(1024, expected_frames);
        let mut envelope_time = std::time::Duration::ZERO;
        let mut frames = 0_u64;
        let cancelled = AtomicBool::new(false);
        let io_before = read_bytes();
        let started = Instant::now();
        timeline::decode_audio(
            &path,
            &plan,
            MediaTime::ZERO,
            None,
            rate,
            format,
            &cancelled,
            |chunk| {
                let collect = Instant::now();
                assert!(envelope.push_stereo(&chunk.bytes, 1.0));
                envelope_time += collect.elapsed();
                frames += u64::try_from(chunk.frames).expect("frame count bounds");
                true
            },
        )
        .expect("exact timeline audio");
        let elapsed = started.elapsed();
        let bytes = read_bytes() - io_before;
        assert_eq!(frames, expected_frames, "exact output frame count");
        let profiled = envelope.finish().expect("complete bounded envelope");
        let io_before = read_bytes();
        let started = Instant::now();
        let public = timeline_waveform(&path, &plan, rate, 1.0, 1024, &Cancellation::default())
            .expect("public detailed waveform");
        let public_elapsed = started.elapsed();
        let public_bytes = read_bytes() - io_before;
        assert_eq!(profiled, public, "instrumentation retains every column");
        assert_eq!(stamp(), before, "source remains unchanged");
        println!(
            "WAVEFORM_DETAIL case={label} frames={frames} total_ms={:.3} envelope_ms={:.3} read_bytes={bytes} public_ms={:.3} public_read_bytes={public_bytes} columns_equal=true",
            elapsed.as_secs_f64() * 1000.0,
            envelope_time.as_secs_f64() * 1000.0,
            public_elapsed.as_secs_f64() * 1000.0,
        );
    }
    assert_eq!(stamp(), before, "read-only source assessment");
}
