use super::*;
use std::sync::mpsc;
use std::time::{SystemTime, UNIX_EPOCH};

struct Fixture {
    root: PathBuf,
    source: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "towavue-frame-publish-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir(&root).expect("owned fixture directory");
        let source = root.join("source.mkv");
        assert!(
            Command::new(crate::media_tools::tool_path("ffmpeg.exe").expect("FFmpeg"))
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc=size=32x24:rate=2:duration=1",
                    "-c:v",
                    "ffv1"
                ])
                .arg(&source)
                .status()
                .expect("fixture encoder")
                .success()
        );
        Self { root, source }
    }
    fn snapshot(&self) -> VideoFrameSnapshot {
        let input = crate::decode::ParallelInput::open_tracked_video(&self.source, &|| false)
            .expect("tracked decoder");
        VideoFrameSnapshot {
            source: input.frame_source.expect("verified file identity"),
            time: MediaTime::ZERO,
        }
    }
    fn assert_no_staging(&self) {
        assert!(
            fs::read_dir(&self.root)
                .expect("fixture entries")
                .all(|entry| !entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".towavue-export-"))
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // This path was exclusively created by Fixture::new; it contains only
        // generated media, aliases of that media, and this test's export targets.
        fs::remove_dir_all(&self.root).expect("remove owned fixture");
    }
}

#[test]
fn frame_export_worker_publishes_edited_png_without_changing_source() {
    let fixture = Fixture::new();
    let original = fs::read(&fixture.source).expect("source bytes");
    let snapshot = fixture.snapshot();
    let target = fixture.root.join("output.png");
    fs::write(&target, b"existing target").expect("old target");
    let operations = vec![
        EditOperation::Crop(towavue_core::PixelCrop {
            x: 2,
            y: 4,
            width: 16,
            height: 18,
        }),
        EditOperation::RotateClockwise,
    ];
    let (tx, rx) = mpsc::channel();
    let job = ExportJob::start_video_frame(
        snapshot.clone(),
        target.clone(),
        operations.clone(),
        move |event| {
            tx.send(event).expect("export event");
        },
    )
    .expect("export worker");
    let event = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("finished export");
    assert!(matches!(
        event,
        ExportEvent::Finished(Ok(ExportOutcome {
            used_hardware_encoder: false
        }))
    ));
    drop(job);
    assert!(rx.try_recv().is_err(), "one terminal event");
    let encoded = fs::read(&target).expect("saved PNG");
    let expected =
        crate::edited_video_frame_png(&fixture.source, MediaTime::ZERO, &operations, &|| false)
            .expect("expected pixels");
    assert_eq!(encoded, expected);
    let image = image::load_from_memory(&encoded).expect("PNG readback");
    assert_eq!((image.width(), image.height()), (18, 16));
    assert_eq!(
        fs::read(&fixture.source).expect("source after export"),
        original
    );
    assert_eq!(fixture.snapshot().source.stamp, snapshot.source.stamp);
    fixture.assert_no_staging();
}

#[test]
fn frame_export_rejects_replacements_restored_timestamps_and_source_aliases() {
    let fixture = Fixture::new();
    let snapshot = fixture.snapshot();
    let target = fixture.root.join("output.png");
    fs::write(&target, b"keep target").expect("target");
    let bytes = fs::read(&fixture.source).expect("source");
    let original_time = fs::metadata(&fixture.source)
        .expect("metadata")
        .modified()
        .expect("modified");
    let replacement = fixture.root.join("replacement.mkv");
    fs::write(&replacement, &bytes).expect("replacement");
    File::options()
        .write(true)
        .open(&replacement)
        .expect("replacement times")
        .set_times(fs::FileTimes::new().set_modified(original_time))
        .expect("restore timestamp");
    fs::rename(&replacement, &fixture.source).expect("replace same-named source");
    assert!(matches!(
        export_video_frame(&snapshot, &target, &[]),
        Err(ExportError::Failed(_))
    ));
    assert_eq!(fs::read(&target).expect("preserved target"), b"keep target");
    let snapshot = fixture.snapshot();
    // Separate writes beyond the host's timestamp granularity. A metadata
    // fingerprint cannot detect a same-tick write with restored modification time.
    std::thread::sleep(Duration::from_millis(20));
    let mut modified = bytes.clone();
    modified[0] ^= 1;
    fs::write(&fixture.source, &modified).expect("same-length write");
    File::options()
        .write(true)
        .open(&fixture.source)
        .expect("source times")
        .set_times(fs::FileTimes::new().set_modified(original_time))
        .expect("restore timestamp");
    let stamp = SourceLease::open(&fixture.source)
        .expect("changed identity")
        .source;
    assert_eq!(snapshot.source.stamp.id, stamp.stamp.id);
    assert_eq!(snapshot.source.stamp.written, stamp.stamp.written);
    assert_ne!(
        snapshot.source.stamp, stamp.stamp,
        "change time detects restored mtime"
    );
    assert!(matches!(
        export_video_frame(&snapshot, &target, &[]),
        Err(ExportError::Failed(_))
    ));
    fs::write(&fixture.source, &bytes).expect("restore valid fixture");
    let alias = fixture.root.join("source-alias.png");
    fs::hard_link(&fixture.source, &alias).expect("owned hard link");
    let snapshot = fixture.snapshot();
    assert!(matches!(
        export_video_frame(&snapshot, &alias, &[]),
        Err(ExportError::SameAsSource)
    ));
    assert!(matches!(
        export_video_frame(&snapshot, &fixture.source, &[]),
        Err(ExportError::SameAsSource)
    ));
    assert_eq!(fs::read(&target).expect("preserved target"), b"keep target");
    assert_eq!(fs::read(&fixture.source).expect("source"), bytes);
    fixture.assert_no_staging();
}

#[test]
fn frame_source_lease_blocks_changes_but_does_not_pin_playback_source() {
    let fixture = Fixture::new();
    let lease = SourceLease::open(&fixture.source).expect("read lease");
    assert!(
        OpenOptions::new()
            .write(true)
            .open(&fixture.source)
            .is_err()
    );
    assert!(fs::rename(&fixture.source, fixture.root.join("moved.mkv")).is_err());
    lease.verify_path().expect("stable identity");
    drop(lease);
    let snapshot = fixture.snapshot();
    // Neither the snapshot nor a retained decoder identity carries a file lock.
    let writer = OpenOptions::new()
        .write(true)
        .open(&fixture.source)
        .expect("not pinned");
    let input = crate::decode::ParallelInput::open_tracked_video(&fixture.source, &|| false)
        .expect("ordinary playback can still open a writable source");
    assert!(
        input.frame_source.is_none(),
        "unverifiable source is not exportable"
    );
    assert_eq!(snapshot.source_path(), fixture.source);
    drop(input);
    drop(writer);
}

#[test]
fn frame_export_cancellation_decode_and_publish_failures_preserve_targets() {
    let fixture = Fixture::new();
    let mut snapshot = fixture.snapshot();
    let target = fixture.root.join("output.png");
    fs::write(&target, b"preserve").expect("target");
    assert!(matches!(
        export_cancellable(&snapshot, &target, &[], &AtomicBool::new(true)),
        Err(ExportError::Cancelled)
    ));
    snapshot.time = MediaTime::from_nanoseconds(1);
    assert!(export_video_frame(&snapshot, &target, &[]).is_err());
    assert_eq!(fs::read(&target).expect("old target"), b"preserve");
    snapshot.time = MediaTime::ZERO;
    let mut permissions = fs::metadata(&target)
        .expect("target metadata")
        .permissions();
    let original_permissions = permissions.clone();
    permissions.set_readonly(true);
    fs::set_permissions(&target, permissions.clone()).expect("read-only target");
    assert!(export_video_frame(&snapshot, &target, &[]).is_err());
    // Restore only this generated target's original writable state for cleanup.
    fs::set_permissions(&target, original_permissions).expect("restore fixture permissions");
    assert_eq!(fs::read(&target).expect("old target"), b"preserve");
    assert!(export_video_frame(&snapshot, &fixture.root.join("wrong.jpg"), &[]).is_err());
    fixture.assert_no_staging();
}

#[test]
fn duplicate_frame_times_across_keyframes_and_at_eof_never_replace_targets() {
    use std::os::windows::process::CommandExt;
    for gop in [1, 2, 12] {
        for origin in [0, 3] {
            for duplicate_count in [2, 3] {
                let fixture = Fixture::new();
                let filter = format!(
                    "setpts=if(lt(N\\,3)\\,N\\,if(lt(N\\,{})\\,3\\,if(lt(N\\,20)\\,N-{}\\,{})))+{origin}/TB",
                    3 + duplicate_count,
                    duplicate_count - 1,
                    21 - duplicate_count
                );
                let output =
                    Command::new(crate::media_tools::tool_path("ffmpeg.exe").expect("FFmpeg"))
                        .creation_flags(0x0800_0000)
                        .args([
                            "-v",
                            "error",
                            "-y",
                            "-f",
                            "lavfi",
                            "-i",
                            "testsrc=size=32x24:rate=8:duration=3",
                            "-vf",
                            &filter,
                            "-fps_mode",
                            "passthrough",
                            "-c:v",
                            "ffv1",
                            "-g",
                            &gop.to_string(),
                        ])
                        .arg(&fixture.source)
                        .output()
                        .expect("duplicate PTS fixture");
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let original = fs::read(&fixture.source).expect("source bytes");
                let mut snapshot = fixture.snapshot();
                let stamp = fixture.snapshot();
                let target = fixture.root.join("output.png");
                let mut frames = Vec::new();
                crate::decode::decode_file(&fixture.source, |output| {
                    if let crate::DecodeOutput::Video(frame) = output {
                        frames.push(frame);
                    }
                    true
                })
                .expect("sequential reference");
                assert_eq!(frames.len(), 24);
                for group in [&frames[3..3 + duplicate_count], &frames[20..24]] {
                    assert!(
                        group
                            .iter()
                            .all(|frame| frame.presentation_time == group[0].presentation_time)
                    );
                    assert!(
                        group.windows(2).any(|pair| pair[0].rgba != pair[1].rgba),
                        "distinct images share a timestamp"
                    );
                    snapshot.time = group[0].presentation_time;
                    for existing in [false, true] {
                        if existing {
                            fs::write(&target, b"preserve existing output").expect("target");
                        }
                        let before = existing.then(|| {
                            fs::metadata(&target)
                                .expect("target metadata")
                                .modified()
                                .expect("mtime")
                        });
                        let (tx, rx) = mpsc::channel();
                        let job = ExportJob::start_video_frame(
                            snapshot.clone(),
                            target.clone(),
                            vec![],
                            move |event| {
                                tx.send(event).expect("export event");
                            },
                        )
                        .expect("frame export worker");
                        let result = rx
                            .recv_timeout(Duration::from_secs(10))
                            .expect("terminal export event");
                        assert!(
                            matches!(result, ExportEvent::Finished(Err(ExportError::Failed(ref error))) if error.contains("ambiguous duplicate")),
                            "gop={gop} origin={origin} time={:?}: {result:?}",
                            snapshot.time
                        );
                        drop(job);
                        assert!(rx.try_recv().is_err(), "one terminal event");
                        if let Some(before) = before {
                            assert_eq!(
                                fs::read(&target).expect("target"),
                                b"preserve existing output"
                            );
                            assert_eq!(
                                fs::metadata(&target)
                                    .expect("target metadata")
                                    .modified()
                                    .expect("mtime"),
                                before
                            );
                            fs::remove_file(&target).expect("remove owned target for next case");
                        } else {
                            assert!(!target.exists(), "failure cannot create a PNG");
                        }
                        fixture.assert_no_staging();
                    }
                }
                // Nearby unique timestamps must still save their own exact image.
                for index in [0, 2, 6, 19] {
                    snapshot.time = frames[index].presentation_time;
                    export_video_frame(&snapshot, &target, &[]).unwrap_or_else(|error| {
                        panic!(
                            "gop={gop} origin={origin} frame={index} time={:?}: {error}",
                            snapshot.time
                        )
                    });
                    assert_eq!(
                        image::open(&target).expect("PNG").to_rgba8().as_raw(),
                        &frames[index].rgba,
                        "gop={gop} origin={origin} frame={index}"
                    );
                }
                assert_eq!(fs::read(&fixture.source).expect("source"), original);
                assert_eq!(fixture.snapshot().source.stamp, stamp.source.stamp);
                fixture.assert_no_staging();
            }
        }
    }
}

#[test]
fn cancellation_after_the_last_pre_encode_check_discards_png_and_preserves_targets() {
    use std::{cell::RefCell, rc::Rc};
    for pixel in ["yuv420p", "yuv420p10le"] {
        let fixture = Fixture::new();
        let encoded = Command::new(crate::media_tools::tool_path("ffmpeg.exe").expect("FFmpeg"))
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=32x24:rate=2:duration=1",
                "-pix_fmt",
                pixel,
                "-c:v",
                "ffv1",
            ])
            .arg(&fixture.source)
            .output()
            .expect("depth fixture");
        assert!(
            encoded.status.success(),
            "{}",
            String::from_utf8_lossy(&encoded.stderr)
        );
        let snapshot = fixture.snapshot();
        let source_bytes = fs::read(&fixture.source).expect("source bytes");
        for existing in [false, true] {
            let target = fixture
                .root
                .join(if existing { "existing.png" } else { "new.png" });
            if existing {
                fs::write(&target, b"preserve this target").expect("old target");
            }
            let old_modified = fs::metadata(&target)
                .ok()
                .map(|m| m.modified().expect("target time"));
            let cancelled = Arc::new(AtomicBool::new(false));
            let observed = Rc::new(RefCell::new(Vec::new()));
            let result = crate::decode::with_encoder_observer(
                {
                    let cancelled = Arc::clone(&cancelled);
                    let observed = Rc::clone(&observed);
                    move |bytes| {
                        if let Some(bytes) = bytes {
                            assert!(bytes > 8, "the native encoder really produced a PNG packet");
                            assert!(cancelled.load(Ordering::Relaxed));
                        } else {
                            assert!(!cancelled.swap(true, Ordering::Relaxed));
                        }
                        observed.borrow_mut().push(bytes);
                    }
                },
                || {
                    export_cancellable(
                        &snapshot,
                        &target,
                        &[EditOperation::FlipHorizontal],
                        &cancelled,
                    )
                },
            );
            assert!(matches!(result, Err(ExportError::Cancelled)));
            let observed = observed.borrow();
            assert_eq!(observed.len(), 2);
            assert_eq!(observed[0], None);
            assert!(observed[1].is_some());
            if existing {
                assert_eq!(
                    fs::read(&target).expect("old target"),
                    b"preserve this target"
                );
                assert_eq!(
                    fs::metadata(&target)
                        .expect("target metadata")
                        .modified()
                        .ok(),
                    old_modified
                );
            } else {
                assert!(!target.exists());
            }
            fixture.assert_no_staging();
            assert_eq!(
                fs::read(&fixture.source).expect("source after cancel"),
                source_bytes
            );
            assert_eq!(fixture.snapshot().source.stamp, snapshot.source.stamp);
            // The observer must have left the thread and all native/file leases
            // must have been released: the same target can now be published.
            export_video_frame(&snapshot, &target, &[EditOperation::FlipHorizontal])
                .expect("retry after late cancellation");
            let png = fs::read(&target).expect("retry PNG");
            let image = image::load_from_memory(&png).expect("retry pixel readback");
            assert_eq!((image.width(), image.height()), (32, 24));
            assert_eq!(
                source_bytes,
                fs::read(&fixture.source).expect("unchanged source")
            );
            assert_eq!(fixture.snapshot().source.stamp, snapshot.source.stamp);
            fixture.assert_no_staging();
        }
    }
}
