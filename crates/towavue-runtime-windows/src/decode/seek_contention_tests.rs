use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use towavue_core::{MediaTime, PlaybackRange};

use crate::{LatestTask, PlaybackEvent, PlaybackSession, PreviewCache, VideoSheetLayout};

#[test]
#[ignore = "Release read-only reference demux/index stage timings; no playback or pixels"]
fn reference_seek_reports_demux_index_growth() {
    if cfg!(debug_assertions) {
        panic!("use Release for timing");
    }
    let path = PathBuf::from(
        std::env::var_os("TOWAVUE_SEEK_REFERENCE_SOURCE").expect("explicit reference"),
    );
    let stamp = || {
        let metadata = std::fs::metadata(&path).expect("source");
        (metadata.len(), metadata.modified().expect("mtime"))
    };
    let before = stamp();
    for trial in 0..2 {
        let mut input = super::format::input(&path).expect("read-only input");
        let duration = input.duration();
        assert!(duration > 0);
        let index_state = |input: &super::format::context::Input| {
            let stream = input.streams().best(super::Type::Video).expect("video");
            // SAFETY: the input is borrowed exclusively by this test thread;
            // the public accessors read indexes and no entry pointer escapes.
            unsafe {
                let count = ffmpeg_next::ffi::avformat_index_get_entries_count(stream.as_ptr());
                let last = if count > 0 {
                    let entry = ffmpeg_next::ffi::avformat_index_get_entry(
                        stream.as_ptr().cast_mut(),
                        count - 1,
                    );
                    assert!(!entry.is_null());
                    Some((*entry).timestamp)
                } else {
                    None
                };
                (count, last)
            }
        };
        for numerator in [2, 3, 1, 2, 3] {
            let target = MediaTime::from_nanoseconds(duration * 1000 * numerator / 4);
            let before_index = index_state(&input);
            let started = Instant::now();
            let current = || started.elapsed() > Duration::from_secs(90);
            super::seek_input(&mut input, target, true, &current).expect("production seek");
            let seek_ms = started.elapsed().as_secs_f64() * 1000.0;
            let after_index = index_state(&input);
            let video = input
                .streams()
                .best(super::Type::Video)
                .expect("video")
                .index();
            let packet = input
                .packets()
                .find(|(stream, _)| stream.index() == video)
                .map(|(stream, packet)| {
                    (
                        stream.time_base(),
                        packet.pts(),
                        packet.dts(),
                        packet.is_key(),
                    )
                })
                .expect("first video packet");
            println!(
                "SEEK_DEMUX trial={trial} fraction={numerator}/4 seek_ms={seek_ms:.3} packet_ms={:.3} before_index={before_index:?} after_index={after_index:?} first_packet={packet:?}",
                started.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
    assert_eq!(stamp(), before);
}

// Keep this separate from ordinary regressions: it reads an explicitly selected
// owner reference and reports measurements, without machine-dependent speed limits.
#[test]
#[ignore = "Release read-only reference with isolated preview cache and concurrent native workers"]
fn reference_seek_reports_preview_worker_contention() {
    if cfg!(debug_assertions) {
        panic!("use Release for timing");
    }
    let path = PathBuf::from(
        std::env::var_os("TOWAVUE_SEEK_REFERENCE_SOURCE").expect("explicit reference"),
    );
    let stamp = || {
        let metadata = std::fs::metadata(&path).expect("source");
        (metadata.len(), metadata.modified().expect("mtime"))
    };
    let before = stamp();
    let input = super::format::input(&path).expect("read-only duration probe");
    let microseconds = input.duration();
    assert!(microseconds > 0);
    let duration = Duration::from_micros(microseconds as u64);
    drop(input);
    let root = std::env::temp_dir().join(format!(
        "towavue-seek-contention-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("current time")
            .as_nanos()
    ));
    std::fs::create_dir(&root).expect("unique cache root");
    let device = super::GraphicsDevice::hardware_for_test().expect("hardware device");
    let (notify, events) = mpsc::channel();
    let mut session = PlaybackSession::open_paused(
        &path,
        device,
        0.0,
        1.0,
        PlaybackRange::default(),
        move |event| {
            let _ = notify.send(event);
        },
    )
    .expect("muted paused playback");

    // Reverse the workload order in the second half to expose cache/order bias.
    // Each case has its own empty app cache; the OS cache is deliberately untouched.
    for (trial, mode) in [0, 1, 2, 2, 1, 0].into_iter().enumerate() {
        for (sample, fraction) in [0.5, 0.75, 0.25].into_iter().enumerate() {
            let target_duration = duration.mul_f64(fraction);
            let target = MediaTime::from_nanoseconds(target_duration.as_nanos() as i64);
            let cache =
                PreviewCache::new(root.join(format!("{trial}-{sample}"))).expect("isolated cache");
            let mut workers = Vec::new();
            let (started_tx, started_rx) = mpsc::channel();
            let (result_tx, result_rx) = mpsc::channel();
            for kind in 0..mode {
                let worker = LatestTask::new("seek-contention-reference").expect("worker");
                let source = path.clone();
                let cache = cache.clone();
                let started_tx = started_tx.clone();
                let result_tx = result_tx.clone();
                worker.submit(move |cancel| {
                    let cache = cache.cancellable(cancel.clone());
                    let started = Instant::now();
                    started_tx.send(()).expect("start observation");
                    let result = if kind == 0 {
                        cache
                            .video_sheet(
                                &source,
                                VideoSheetLayout::for_position(duration, target_duration)
                                    .expect("layout"),
                            )
                            .map(|_| ())
                    } else {
                        cache.waveform(&source, 640, 96).map(|_| ())
                    };
                    result_tx
                        .send((kind, started.elapsed(), cancel.is_cancelled(), result))
                        .expect("result observation");
                });
                workers.push(worker);
            }
            for _ in &workers {
                started_rx
                    .recv_timeout(Duration::from_secs(10))
                    .expect("background task entered");
            }
            if mode != 0 {
                std::thread::sleep(Duration::from_millis(50));
            }
            let active_before: Vec<_> = workers.iter().map(|w| !w.is_idle()).collect();
            let started = Instant::now();
            session.seek(target).expect("seek");
            let command_ms = started.elapsed().as_secs_f64() * 1000.0;
            loop {
                if let Some(time) = session.pending_video_time() {
                    assert!(
                        time >= target && time <= target.saturating_add(Duration::from_secs(1))
                    );
                    assert!(session.advance_pending());
                    break;
                }
                assert!(started.elapsed() < Duration::from_secs(90), "seek deadline");
                if let Ok(event) = events.recv_timeout(Duration::from_millis(1))
                    && event.generation() == session.generation()
                {
                    assert!(
                        !matches!(
                            event,
                            PlaybackEvent::Failed(..)
                                | PlaybackEvent::VideoFailed(..)
                                | PlaybackEvent::DeviceRemoved(..)
                        ),
                        "{event:?}"
                    );
                }
            }
            let ready_ms = started.elapsed().as_secs_f64() * 1000.0;
            let active_after: Vec<_> = workers.iter().map(|w| !w.is_idle()).collect();
            println!(
                "SEEK_CONTENTION trial={trial} mode={mode} fraction={fraction} command_ms={command_ms:.3} ready_ms={ready_ms:.3} active_before={active_before:?} active_after={active_after:?} stages={:?} hardware={} transfers={}",
                session.seek_stage_ms(),
                session.metrics().hardware_frame_count,
                session.metrics().cpu_transfer_count
            );
            for worker in &workers {
                worker.clear();
            }
            for _ in &workers {
                let (kind, elapsed, cancelled, result) = result_rx
                    .recv_timeout(Duration::from_secs(90))
                    .expect("background completion");
                println!(
                    "SEEK_BACKGROUND kind={kind} elapsed_ms={:.3} cancelled={cancelled} result={result:?}",
                    elapsed.as_secs_f64() * 1000.0
                );
                assert!(cancelled || result.is_ok(), "background workload failed");
            }
            let deadline = Instant::now() + Duration::from_secs(10);
            while workers.iter().any(|worker| !worker.is_idle()) {
                assert!(Instant::now() < deadline, "native readers released");
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
    drop(session);
    assert_eq!(stamp(), before);
    let resolved = std::fs::canonicalize(&root).expect("owned root");
    assert!(resolved.starts_with(std::fs::canonicalize(std::env::temp_dir()).expect("temp")));
    assert!(
        resolved
            .file_name()
            .expect("name")
            .to_string_lossy()
            .starts_with("towavue-seek-contention-")
    );
    std::fs::remove_dir_all(resolved).expect("owned cache cleanup");
}
