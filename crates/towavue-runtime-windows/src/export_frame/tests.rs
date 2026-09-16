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
