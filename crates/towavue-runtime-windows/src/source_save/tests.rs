use super::*;
use std::sync::mpsc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use towavue_core::{EditOperation, MediaKind};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "towavue-source-save-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir(&root).expect("owned fixture");
        Self(root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(
            self.0
                .file_name()
                .expect("owned fixture")
                .to_string_lossy()
                .starts_with("towavue-source-save-")
        );
        if std::thread::panicking() {
            eprintln!("Retained failed source-save fixture: {}", self.0.display());
            return;
        }
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn bitmap(path: &Path) {
    let mut bytes = vec![0_u8; 62];
    bytes[..2].copy_from_slice(b"BM");
    bytes[2..6].copy_from_slice(&62_u32.to_le_bytes());
    bytes[10..14].copy_from_slice(&54_u32.to_le_bytes());
    bytes[14..18].copy_from_slice(&40_u32.to_le_bytes());
    bytes[18..22].copy_from_slice(&2_u32.to_le_bytes());
    bytes[22..26].copy_from_slice(&1_u32.to_le_bytes());
    bytes[26..28].copy_from_slice(&1_u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&24_u16.to_le_bytes());
    bytes[54..60].copy_from_slice(&[0, 0, 255, 255, 0, 0]);
    fs::write(path, bytes).expect("owned bitmap");
}
fn prepare(path: &Path) -> PreparedSourceSave {
    prepare_source_save(
        FileOperationSource::capture(path).expect("source identity"),
        ExportRequest {
            source: path.to_owned(),
            target: path.to_owned(),
            kind: MediaKind::Image,
            operations: vec![EditOperation::FlipHorizontal],
            hardware_encode: false,
        },
        ExportOptions::default(),
        &AtomicBool::new(false),
        &|_| {},
        &|_| {},
    )
    .unwrap_or_else(|error| panic!("prepare: {error}"))
}
fn finish(prepared: PreparedSourceSave) -> Result<SavedSource, SourceSaveError> {
    let (send, receive) = mpsc::channel();
    commit_source_save(prepared, move |result| {
        send.send(result).ok();
    })
    .expect("publication worker");
    let result = receive
        .recv_timeout(Duration::from_secs(5))
        .expect("one publication result");
    assert!(matches!(
        receive.recv_timeout(Duration::from_secs(1)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
    result
}
fn wait_removed(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while path.exists() {
        assert!(Instant::now() < deadline, "cleanup {}", path.display());
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn staged_save_preserves_original_until_publication_and_retains_read_only_undo_source() {
    let fixture = Fixture::new();
    let path = fixture.0.join("image.bmp");
    bitmap(&path);
    let original = fs::read(&path).expect("original");
    let created = fs::metadata(&path)
        .expect("source metadata")
        .created()
        .expect("source creation");
    fs::write(
        path.with_file_name("image.bmp:towavue-test"),
        b"owned named stream",
    )
    .expect("owned ADS");
    let prepared = prepare(&path);
    let directory = prepared.files.directory.clone();
    let edited = fs::read(&prepared.files.prepared).expect("candidate");
    assert_ne!(edited, original);
    assert_eq!(fs::read(&path).expect("unchanged source"), original);
    let saved = finish(prepared).unwrap_or_else(|error| panic!("publish: {error}"));
    assert_eq!(fs::read(&path).expect("saved output"), edited);
    assert_eq!(
        fs::metadata(&path)
            .expect("saved metadata")
            .created()
            .expect("saved creation"),
        created
    );
    assert_eq!(
        crate::decode_image(&path).expect("saved pixels").frames[0].rgba,
        [0, 0, 255, 255, 255, 0, 0, 255]
    );
    assert_eq!(
        fs::read(saved.original_path()).expect("retained original"),
        original
    );
    assert_eq!(
        fs::read(path.with_file_name("image.bmp:towavue-test")).expect("preserved ADS"),
        b"owned named stream"
    );
    assert!(
        OpenOptions::new()
            .write(true)
            .open(saved.original_path())
            .is_err()
    );
    saved
        .current_source()
        .verify()
        .expect("new expected source");
    let clone = saved.clone();
    drop(saved);
    assert!(directory.exists());
    drop(clone);
    wait_removed(&directory);
    assert_eq!(
        fs::read(&path).expect("saved output survives cleanup"),
        edited
    );
}

#[test]
fn preparation_job_cancels_without_publication_and_delivers_one_terminal_result() {
    let fixture = Fixture::new();
    let path = fixture.0.join("image.bmp");
    bitmap(&path);
    let original = fs::read(&path).expect("original");
    let (send, receive) = mpsc::channel();
    let job = SourceSaveJob::start(
        FileOperationSource::capture(&path).expect("identity"),
        ExportRequest {
            source: path.clone(),
            target: path.clone(),
            kind: MediaKind::Image,
            operations: vec![EditOperation::FlipHorizontal],
            hardware_encode: false,
        },
        ExportOptions::default(),
        move |event| {
            send.send(event).ok();
        },
    )
    .expect("preparation worker");
    job.cancel();
    let candidate = loop {
        match receive
            .recv_timeout(Duration::from_secs(5))
            .expect("preparation callback")
        {
            SourceSaveEvent::Prepared(result) => break result,
            SourceSaveEvent::Progress(_) | SourceSaveEvent::AnalyzingAudio(_) => {}
        }
    };
    drop(job);
    match candidate {
        Ok(prepared) => {
            let directory = prepared.files.directory.clone();
            drop(prepared);
            wait_removed(&directory);
        }
        Err(SourceSaveError::Export(ExportError::Cancelled)) => {}
        Err(error) => panic!("unexpected preparation failure: {error}"),
    }
    assert!(matches!(
        receive.recv_timeout(Duration::from_secs(1)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
    assert_eq!(fs::read(path).expect("unpublished source"), original);
}

#[test]
fn cancelled_stale_and_tampered_candidates_never_replace_the_source() {
    let fixture = Fixture::new();
    let path = fixture.0.join("image.bmp");
    bitmap(&path);
    let original = fs::read(&path).expect("original");
    let request = ExportRequest {
        source: path.clone(),
        target: path.clone(),
        kind: MediaKind::Image,
        operations: vec![EditOperation::FlipHorizontal],
        hardware_encode: false,
    };
    assert!(matches!(
        prepare_source_save(
            FileOperationSource::capture(&path).expect("identity"),
            request,
            ExportOptions::default(),
            &AtomicBool::new(true),
            &|_| {},
            &|_| {}
        ),
        Err(SourceSaveError::Export(ExportError::Cancelled))
    ));
    let prepared = prepare(&path);
    let directory = prepared.files.directory.clone();
    drop(prepared);
    wait_removed(&directory);
    assert_eq!(fs::read(&path).expect("cancelled source"), original);
    let prepared = prepare(&path);
    let directory = prepared.files.directory.clone();
    fs::write(&path, b"external replacement content").expect("external edit");
    assert!(matches!(
        finish(prepared),
        Err(SourceSaveError::Source(FileOperationError::SourceChanged))
    ));
    wait_removed(&directory);
    assert_eq!(
        fs::read(&path).expect("external content"),
        b"external replacement content"
    );
    bitmap(&path);
    let prepared = prepare(&path);
    let directory = prepared.files.directory.clone();
    fs::write(&prepared.files.prepared, b"tampered").expect("candidate alteration");
    assert!(matches!(
        finish(prepared),
        Err(SourceSaveError::Source(FileOperationError::SourceChanged))
    ));
    wait_removed(&directory);
    assert_eq!(fs::read(&path).expect("source protected"), original);
}

#[test]
fn publication_failure_restores_only_missing_targets_and_preserves_ambiguous_recovery_files() {
    let fixture = Fixture::new();
    let path = fixture.0.join("image.bmp");
    bitmap(&path);
    let original = fs::read(&path).expect("original");
    let prepared = prepare(&path);
    let directory = prepared.files.directory.clone();
    // Controlled documented partial state, not a claim that the OS emitted 1177.
    fs::rename(&path, &prepared.files.original).expect("old source moved to backup");
    prepared.files.preserve.store(true, Ordering::Relaxed);
    assert!(matches!(
        finish_publication(
            prepared,
            Err(io::Error::other("injected partial replacement"))
        ),
        Err(SourceSaveError::Io(_))
    ));
    assert_eq!(fs::read(&path).expect("restored original"), original);
    wait_removed(&directory);
    let prepared = prepare(&path);
    let directory = prepared.files.directory.clone();
    let backup = prepared.files.original.clone();
    fs::rename(&path, &backup).expect("old source moved");
    fs::write(&path, b"concurrent new file").expect("external target");
    prepared.files.preserve.store(true, Ordering::Relaxed);
    assert!(matches!(
        finish_publication(
            prepared,
            Err(io::Error::other("injected partial replacement"))
        ),
        Err(SourceSaveError::RecoveryRequired { .. })
    ));
    assert_eq!(
        fs::read(&path).expect("external target untouched"),
        b"concurrent new file"
    );
    assert_eq!(
        fs::read(&backup).expect("recovery original retained"),
        original
    );
    assert!(directory.exists());
}

#[test]
fn locked_publication_preserves_source_and_second_save_can_render_from_the_retained_original() {
    let fixture = Fixture::new();
    let path = fixture.0.join("image.bmp");
    bitmap(&path);
    let original = fs::read(&path).expect("original");
    let prepared = prepare(&path);
    let directory = prepared.files.directory.clone();
    let blocker = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .open(&path)
        .expect("another reader denies replacement");
    assert!(finish(prepared).is_err());
    drop(blocker);
    wait_removed(&directory);
    assert_eq!(fs::read(&path).expect("busy source retained"), original);
    let saved = finish(prepare(&path)).unwrap_or_else(|error| panic!("first save: {error}"));
    let second = prepare_source_save(
        saved.current_source().clone(),
        ExportRequest {
            source: saved.original_path().to_owned(),
            target: path.clone(),
            kind: MediaKind::Image,
            operations: Vec::new(),
            hardware_encode: false,
        },
        ExportOptions::default(),
        &AtomicBool::new(false),
        &|_| {},
        &|_| {},
    )
    .unwrap_or_else(|error| panic!("undo prepare: {error}"));
    let expected = fs::read(&second.files.prepared).expect("undo candidate");
    let next = finish(second).unwrap_or_else(|error| panic!("undo publish: {error}"));
    assert_eq!(fs::read(&path).expect("undo save"), expected);
    assert_eq!(
        crate::decode_image(&path).expect("undo pixels").frames[0].rgba,
        [255, 0, 0, 255, 0, 0, 255, 255]
    );
    assert_eq!(
        fs::read(saved.original_path()).expect("initial original remains"),
        original
    );
    drop(next);
    drop(saved);
}

#[test]
fn audio_and_video_saves_keep_original_bytes_and_publish_readable_edited_media() {
    let fixture = Fixture::new();
    let executable = crate::media_tools::tool_path("ffmpeg.exe").expect("fixed FFmpeg");
    for (kind, name, input, encoding) in [
        (
            MediaKind::Audio,
            "audio.wav",
            "sine=frequency=440:sample_rate=48000:duration=0.1",
            vec!["-c:a", "pcm_s16le"],
        ),
        (
            MediaKind::Video,
            "video.mp4",
            "testsrc=size=64x48:rate=4:duration=1",
            vec!["-pix_fmt", "yuv420p", "-c:v", "mpeg4", "-q:v", "2"],
        ),
    ] {
        let path = fixture.0.join(name);
        let output = std::process::Command::new(&executable)
            .args(["-v", "error", "-f", "lavfi", "-i", input])
            .args(encoding)
            .arg(&path)
            .output()
            .expect("generate owned fixture");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let original = fs::read(&path).expect("original media");
        let mut before_samples = Vec::new();
        let before = crate::decode::decode_file(&path, |item| {
            if let crate::DecodeOutput::Audio(chunk) = item {
                before_samples.extend(
                    chunk
                        .bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|sample| f32::from_le_bytes(*sample)),
                );
            }
            true
        })
        .expect("original decode");
        let operations = if kind == MediaKind::Audio {
            vec![EditOperation::SetVolume(0.5)]
        } else {
            vec![EditOperation::Crop(towavue_core::PixelCrop {
                x: 0,
                y: 0,
                width: 32,
                height: 48,
            })]
        };
        let prepared = prepare_source_save(
            FileOperationSource::capture(&path).expect("identity"),
            ExportRequest {
                source: path.clone(),
                target: path.clone(),
                kind,
                operations,
                hardware_encode: false,
            },
            ExportOptions::default(),
            &AtomicBool::new(false),
            &|_| {},
            &|_| {},
        )
        .unwrap_or_else(|error| panic!("prepare {kind:?}: {error}"));
        assert_eq!(
            fs::read(&path).expect("preparation preserves source"),
            original
        );
        let saved = finish(prepared).unwrap_or_else(|error| panic!("save {kind:?}: {error}"));
        assert_eq!(
            fs::read(saved.original_path()).expect("retained original media"),
            original
        );
        let mut samples = Vec::new();
        let after = crate::decode::decode_file(&path, |item| {
            match item {
                crate::DecodeOutput::Video(frame) => {
                    assert_eq!((frame.width, frame.height), (32, 48))
                }
                crate::DecodeOutput::Audio(chunk) => samples.extend(
                    chunk
                        .bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|sample| f32::from_le_bytes(*sample)),
                ),
            }
            true
        })
        .expect("saved decode");
        assert_eq!(after, before);
        if kind == MediaKind::Audio {
            assert_eq!(samples.len(), before_samples.len());
            assert!(
                samples
                    .iter()
                    .zip(&before_samples)
                    .all(|(actual, original)| (*actual - *original * 0.5).abs() < 0.00004)
            );
        }
        let directory = saved.original._files.directory.clone();
        drop(saved);
        wait_removed(&directory);
    }
}

#[test]
fn encoder_failure_keeps_the_source_and_cleans_the_unpublished_candidate() {
    let fixture = Fixture::new();
    let path = fixture.0.join("image.bmp");
    bitmap(&path);
    let original = fs::read(&path).expect("original");
    let result = prepare_source_save(
        FileOperationSource::capture(&path).expect("identity"),
        ExportRequest {
            source: path.clone(),
            target: path.clone(),
            kind: MediaKind::Image,
            operations: vec![EditOperation::Crop(towavue_core::PixelCrop {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            })],
            hardware_encode: false,
        },
        ExportOptions::default(),
        &AtomicBool::new(false),
        &|_| {},
        &|_| {},
    );
    assert!(matches!(result, Err(SourceSaveError::Export(_))));
    assert_eq!(
        fs::read(path).expect("source survives encoder failure"),
        original
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while fs::read_dir(&fixture.0)
        .expect("owned directory")
        .filter_map(Result::ok)
        .any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".towavue-save-")
        })
    {
        assert!(Instant::now() < deadline, "unpublished staging cleanup");
        std::thread::sleep(Duration::from_millis(2));
    }
}
