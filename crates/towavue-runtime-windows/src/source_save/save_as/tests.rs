use super::*;
use crate::source_save::tests::{Fixture, bitmap, wait_removed};
use std::sync::mpsc;
use towavue_core::{EditOperation, MediaKind};

fn request(source: &Path, target: &Path, kind: MediaKind) -> ExportRequest {
    ExportRequest {
        source: source.to_owned(),
        target: target.to_owned(),
        kind,
        operations: vec![EditOperation::FlipHorizontal],
        hardware_encode: false,
    }
}
fn prepare_save_as(
    input: MediaInput,
    source_version: Option<&FileOperationSource>,
    export: ExportRequest,
    options: ExportOptions,
    cancelled: &AtomicBool,
    progress: &(impl Fn(Duration) + Sync),
    analyzing: &(impl Fn(Duration) + Sync),
) -> Result<PreparedSaveAs, SourceSaveError> {
    let target = SaveAsTarget::capture(&export.target)?;
    super::prepare_save_as(
        SaveAsRequest {
            input,
            source_version: source_version.cloned(),
            target,
            export,
            options,
        },
        cancelled,
        progress,
        analyzing,
    )
}
fn prepare_new(source: &Path, target: &Path) -> PreparedSaveAs {
    let expected = FileOperationSource::capture(source).expect("loaded source");
    prepare_save_as(
        MediaInput::new(source.to_owned()),
        Some(&expected),
        request(source, target, MediaKind::Image),
        ExportOptions::default(),
        &AtomicBool::new(false),
        &|_| {},
        &|_| {},
    )
    .expect("prepare Save as")
}
fn finish(prepared: PreparedSaveAs) -> Result<SavedAsSource, SourceSaveError> {
    let (send, receive) = mpsc::channel();
    commit_save_as(prepared, move |result| {
        send.send(result).ok();
    })
    .expect("publication worker");
    let result = receive
        .recv_timeout(Duration::from_secs(10))
        .expect("one publication result");
    assert!(matches!(
        receive.recv_timeout(Duration::from_secs(1)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
    result
}

#[test]
fn new_destination_keeps_an_immutable_undo_original_after_source_removal() {
    let fixture = Fixture::new();
    let source = fixture.0.join("source.bmp");
    let target = fixture.0.join("saved.png");
    bitmap(&source);
    let original = fs::read(&source).expect("original");
    let prepared = prepare_new(&source, &target);
    let staging = prepared.files().directory.clone();
    assert!(prepared.expected_target().is_none());
    assert_eq!(prepared.target(), target);
    assert!(!target.exists());
    assert_eq!(fs::read(&source).expect("unpublished source"), original);
    fs::remove_file(&source).expect("source is independent after capture");
    let saved = finish(prepared).expect("publish new path");
    assert_eq!(saved.current_source().path(), target);
    assert!(saved.replaced_source().is_none());
    let retained = saved.retained_source();
    assert_eq!(
        fs::read(retained.original_path()).expect("undo bytes"),
        original
    );
    assert!(fs::write(retained.original_path(), b"cannot mutate undo").is_err());
    assert_eq!(
        crate::decode_image(&target).expect("saved pixels").frames[0].rgba,
        [0, 0, 255, 255, 255, 0, 0, 255]
    );
    assert_eq!(
        crate::decode_image(retained.original_path())
            .expect("undo pixels")
            .frames[0]
            .rgba,
        [255, 0, 0, 255, 0, 0, 255, 255]
    );
    wait_removed(&staging);
    let undo_directory = retained
        .original_path()
        .parent()
        .expect("undo directory")
        .to_owned();
    drop(saved);
    assert!(
        retained.original_path().exists(),
        "another owner retains Undo"
    );
    drop(retained);
    wait_removed(&undo_directory);
}

#[test]
fn replacement_retains_destination_bytes_separately_and_repeated_save_as_reuses_document_original()
{
    let fixture = Fixture::new();
    let source = fixture.0.join("source.bmp");
    let target = fixture.0.join("saved.bmp");
    bitmap(&source);
    bitmap(&target);
    let original = fs::read(&source).expect("source");
    let mut previous = fs::read(&target).expect("target");
    previous[54..60].copy_from_slice(&[0, 255, 0, 255, 255, 255]);
    fs::write(&target, &previous).expect("distinct existing destination");
    let prepared = prepare_new(&source, &target);
    assert_eq!(
        prepared.expected_target().expect("target snapshot").path(),
        target
    );
    assert_eq!(fs::read(&target).expect("unpublished"), previous);
    let saved = finish(prepared).expect("replacement");
    let document = saved.retained_source();
    assert_eq!(
        fs::read(document.original_path()).expect("document original"),
        original
    );
    let replaced = saved
        .replaced_source()
        .expect("other target documents")
        .retained_source();
    assert_eq!(
        fs::read(replaced.original_path()).expect("old destination"),
        previous
    );
    assert!(!Arc::ptr_eq(&document.0, &replaced.0));
    fs::remove_file(&source).expect("old source no longer needed");
    fs::remove_file(&target).expect("saved path can also disappear");
    let next = fixture.0.join("again.png");
    let input = MediaInput::retained(target.clone(), document.clone());
    let prepared = prepare_save_as(
        input,
        None,
        request(&target, &next, MediaKind::Image),
        ExportOptions::default(),
        &AtomicBool::new(false),
        &|_| {},
        &|_| {},
    )
    .expect("retained deleted input");
    let again = finish(prepared).expect("repeat Save as");
    assert!(
        Arc::ptr_eq(&document.0, &again.retained_source().0),
        "reuse the first original, never the last encoded output"
    );
    assert_eq!(
        crate::decode_image(&next)
            .expect("repeated saved pixels")
            .frames[0]
            .rgba,
        [0, 0, 255, 255, 255, 0, 0, 255]
    );
}

#[test]
fn late_new_target_collisions_and_changed_existing_targets_preserve_competing_data() {
    let fixture = Fixture::new();
    let source = fixture.0.join("source.bmp");
    bitmap(&source);
    for kind in 0..3 {
        let target = fixture.0.join(format!("target-{kind}.bmp"));
        if kind == 2 {
            bitmap(&target);
        }
        let prepared = prepare_new(&source, &target);
        let staging = prepared.files().directory.clone();
        if kind == 1 {
            fs::create_dir(&target).expect("late directory");
        } else {
            fs::write(&target, b"competing data must survive").expect("external write");
        }
        assert!(finish(prepared).is_err());
        if kind == 1 {
            assert!(target.is_dir());
        } else {
            assert_eq!(
                fs::read(&target).expect("competitor"),
                b"competing data must survive"
            );
        }
        wait_removed(&staging);
    }
}

#[test]
fn stale_unbacked_inputs_invalid_outputs_and_abandoned_candidates_never_publish() {
    let fixture = Fixture::new();
    let source = fixture.0.join("source.bmp");
    bitmap(&source);
    let target = fixture.0.join("saved.png");
    let expected = FileOperationSource::capture(&source).expect("loaded identity");
    for (version, output) in [
        (None, ExportOutput::Media),
        (Some(&expected), ExportOutput::AudioOnly),
        (Some(&expected), ExportOutput::VideoFrame),
    ] {
        assert!(
            prepare_save_as(
                MediaInput::new(source.clone()),
                version,
                request(&source, &target, MediaKind::Image),
                ExportOptions {
                    output,
                    ..Default::default()
                },
                &AtomicBool::new(false),
                &|_| {},
                &|_| {}
            )
            .is_err()
        );
        assert!(!target.exists());
    }
    assert!(
        prepare_save_as(
            MediaInput::new(source.clone()),
            Some(&expected),
            request(&source, &source, MediaKind::Image),
            ExportOptions::default(),
            &AtomicBool::new(false),
            &|_| {},
            &|_| {}
        )
        .is_err()
    );
    let prepared = prepare_new(&source, &target);
    let staging = prepared.files().directory.clone();
    let original_directory = prepared
        .original
        .original_path()
        .parent()
        .expect("private original")
        .to_owned();
    drop(prepared);
    wait_removed(&staging);
    wait_removed(&original_directory);
    assert!(!target.exists());
    fs::write(&source, b"changed loaded source").expect("external mutation");
    assert!(
        prepare_save_as(
            MediaInput::new(source.clone()),
            Some(&expected),
            request(&source, &target, MediaKind::Image),
            ExportOptions::default(),
            &AtomicBool::new(false),
            &|_| {},
            &|_| {}
        )
        .is_err()
    );
    assert!(!target.exists());
    assert_eq!(
        fs::read(&source).expect("source remains external version"),
        b"changed loaded source"
    );
}

#[test]
fn retained_pasted_input_can_save_without_any_logical_source_file() {
    let fixture = Fixture::new();
    let logical = fixture.0.join("no-file-for-untitled.png");
    let frame = crate::DecodedImageFrame {
        width: 2,
        height: 1,
        rgba: vec![193, 29, 71, 0, 10, 211, 33, 255],
        delay: Duration::ZERO,
    };
    let original = RetainedSource::from_pasted_frame(&frame, &|| Ok(())).expect("pasted original");
    let target = fixture.0.join("pasted.png");
    let prepared = prepare_save_as(
        MediaInput::retained(logical.clone(), original.clone()),
        None,
        request(&logical, &target, MediaKind::Image),
        ExportOptions::default(),
        &AtomicBool::new(false),
        &|_| {},
        &|_| {},
    )
    .expect("untitled Save as");
    let saved = finish(prepared).expect("saved untitled input");
    assert!(!logical.exists());
    assert!(Arc::ptr_eq(&original.0, &saved.retained_source().0));
    assert_eq!(
        crate::decode_image(original.original_path())
            .expect("untouched paste")
            .frames[0]
            .rgba,
        frame.rgba
    );
    let pixels = crate::decode_image(&target).expect("saved paste");
    assert_eq!(&pixels.frames[0].rgba[..4], &[10, 211, 33, 255]);
    assert_eq!(pixels.frames[0].rgba[7], 0);
}

#[test]
fn preparation_worker_reports_once_and_cancellation_keeps_both_files() {
    let fixture = Fixture::new();
    let source = fixture.0.join("source.bmp");
    let target = fixture.0.join("saved.bmp");
    bitmap(&source);
    bitmap(&target);
    let before = fs::read(&target).expect("existing destination");
    let expected = FileOperationSource::capture(&source).expect("loaded source");
    for cancel in [false, true] {
        let (send, receive) = mpsc::channel();
        let job = SaveAsJob::start(
            SaveAsRequest {
                input: MediaInput::new(source.clone()),
                source_version: Some(expected.clone()),
                target: SaveAsTarget::capture(&target).expect("selected destination"),
                export: request(&source, &target, MediaKind::Image),
                options: ExportOptions::default(),
            },
            move |event| {
                if let SaveAsEvent::Prepared(result) = event {
                    send.send(result).ok();
                }
            },
        )
        .expect("worker");
        if cancel {
            job.cancel();
        }
        let result = receive
            .recv_timeout(Duration::from_secs(10))
            .expect("one prepared result");
        match result {
            Ok(prepared) => {
                let staging = prepared.files().directory.clone();
                drop(prepared);
                wait_removed(&staging);
            }
            Err(SourceSaveError::Export(ExportError::Cancelled)) if cancel => {}
            Err(error) => panic!("unexpected preparation failure: {error}"),
        }
        drop(job);
        assert!(matches!(
            receive.recv_timeout(Duration::from_secs(1)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
        assert_eq!(fs::read(&target).expect("unpublished target"), before);
        assert_eq!(fs::read(&source).expect("unchanged source"), before);
    }
    let result = prepare_save_as(
        MediaInput::new(source.clone()),
        Some(&expected),
        request(&source, &target, MediaKind::Image),
        ExportOptions::default(),
        &AtomicBool::new(true),
        &|_| {},
        &|_| {},
    );
    assert!(matches!(
        result,
        Err(SourceSaveError::Export(ExportError::Cancelled))
    ));
}

#[test]
fn destination_changes_after_selection_cannot_be_reauthorized_by_preparation() {
    let fixture = Fixture::new();
    let source = fixture.0.join("source.bmp");
    bitmap(&source);
    for existing in [false, true] {
        let path = fixture.0.join(format!("target-{existing}.bmp"));
        if existing {
            bitmap(&path);
        }
        let selected = SaveAsTarget::capture(&path).expect("selected destination");
        fs::write(&path, b"arrived after destination selection").expect("external file");
        let result = super::prepare_save_as(
            SaveAsRequest {
                input: MediaInput::new(source.clone()),
                source_version: Some(FileOperationSource::capture(&source).expect("loaded source")),
                target: selected,
                export: request(&source, &path, MediaKind::Image),
                options: ExportOptions::default(),
            },
            &AtomicBool::new(false),
            &|_| {},
            &|_| {},
        );
        assert!(result.is_err());
        assert_eq!(
            fs::read(&path).expect("competitor preserved"),
            b"arrived after destination selection"
        );
    }
}

#[test]
fn audio_video_and_repeated_edits_keep_the_first_original_and_publish_readable_destinations() {
    let fixture = Fixture::new();
    let executable = crate::media_tools::tool_path("ffmpeg.exe").expect("fixed FFmpeg");
    for (kind, name, input, encoding, operations) in [
        (
            MediaKind::Audio,
            "audio.wav",
            "sine=frequency=440:sample_rate=48000:duration=0.1",
            vec!["-c:a", "pcm_s16le"],
            vec![EditOperation::SetVolume(0.5)],
        ),
        (
            MediaKind::Video,
            "video.mp4",
            "testsrc=size=64x48:rate=4:duration=1",
            vec!["-pix_fmt", "yuv420p", "-c:v", "mpeg4", "-q:v", "2"],
            vec![EditOperation::Crop(towavue_core::PixelCrop {
                x: 0,
                y: 0,
                width: 32,
                height: 48,
            })],
        ),
    ] {
        let source = fixture.0.join(name);
        let output = std::process::Command::new(&executable)
            .args(["-v", "error", "-f", "lavfi", "-i", input])
            .args(encoding)
            .arg(&source)
            .output()
            .expect("generated media");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let original_bytes = fs::read(&source).expect("original bytes");
        let version = FileOperationSource::capture(&source).expect("loaded version");
        let target = fixture.0.join(format!("saved-{name}"));
        let export = ExportRequest {
            source: source.clone(),
            target: target.clone(),
            kind,
            operations: operations.clone(),
            hardware_encode: false,
        };
        let prepared = prepare_save_as(
            MediaInput::new(source.clone()),
            Some(&version),
            export,
            ExportOptions::default(),
            &AtomicBool::new(false),
            &|_| {},
            &|_| {},
        )
        .expect("media Save as");
        let saved = finish(prepared).expect("publish media");
        let original = saved.retained_source();
        assert_eq!(
            fs::read(original.original_path()).expect("undo source"),
            original_bytes
        );
        assert_eq!(
            fs::read(&source).expect("original file unchanged"),
            original_bytes
        );
        let output = std::process::Command::new(&executable)
            .args(["-v", "error", "-i"])
            .arg(&target)
            .args(["-f", "null", "-"])
            .output()
            .expect("read saved output");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        // Saving again with no operations models Undo back to the original: the
        // second encode must read the retained first input, not the cropped/gained file.
        let next = fixture.0.join(format!("restored-{name}"));
        let export = ExportRequest {
            source: target.clone(),
            target: next.clone(),
            kind,
            operations: vec![],
            hardware_encode: false,
        };
        let prepared = prepare_save_as(
            MediaInput::retained(target.clone(), original.clone()),
            None,
            export,
            ExportOptions::default(),
            &AtomicBool::new(false),
            &|_| {},
            &|_| {},
        )
        .expect("save undone plan");
        let restored = finish(prepared).expect("restored publication");
        assert!(Arc::ptr_eq(&original.0, &restored.retained_source().0));
        let mut edited_samples = Vec::new();
        let mut restored_samples = Vec::new();
        let mut edited_size = None;
        let mut restored_size = None;
        for (path, samples, size) in [
            (&target, &mut edited_samples, &mut edited_size),
            (&next, &mut restored_samples, &mut restored_size),
        ] {
            crate::decode::decode_file(path, |item| {
                match item {
                    crate::DecodeOutput::Audio(chunk) => samples.extend(
                        chunk
                            .bytes
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .map(|s| f32::from_le_bytes(*s)),
                    ),
                    crate::DecodeOutput::Video(frame) => {
                        *size = Some((frame.width, frame.height));
                    }
                }
                true
            })
            .expect("decoded saved media");
        }
        if kind == MediaKind::Audio {
            assert_eq!(edited_samples.len(), restored_samples.len());
            let peak = |samples: &[f32]| samples.iter().copied().map(f32::abs).fold(0.0, f32::max);
            assert!((peak(&edited_samples) / peak(&restored_samples) - 0.5).abs() < 0.01);
        } else {
            assert_eq!(edited_size, Some((32, 48)));
            assert_eq!(restored_size, Some((64, 48)));
        }
    }
}

#[test]
fn recovery_retains_the_document_original_as_well_as_the_destination_artifacts() {
    let fixture = Fixture::new();
    let source = fixture.0.join("original.bmp");
    bitmap(&source);
    let bytes = fs::read(&source).expect("original bytes");
    let original =
        RetainedSource::capture(&FileOperationSource::capture(&source).expect("version"))
            .expect("document original");
    let path = original.original_path().to_owned();
    let directory = path.parent().expect("original directory").to_owned();
    fs::remove_file(source).expect("source removed after preparation");
    let destination_recovery = fixture.0.join("destination-recovery");
    fs::create_dir(&destination_recovery).expect("owned recovery fixture");
    let backup = destination_recovery.join("old-destination.bmp");
    fs::write(&backup, b"different document").expect("old destination bytes");
    // Inject the native replacement's typed uncertain result, without claiming
    // to induce an OS partial replacement error. The worker uses this same guard.
    let result = retain_recovery_original(
        &original,
        Err(SourceSaveError::RecoveryRequired {
            message: "injected partial replacement".into(),
            directory: destination_recovery.clone(),
        }),
    );
    let Err(SourceSaveError::RecoveryRequired {
        message,
        directory: reported,
    }) = result
    else {
        panic!("uncertain publication remains an error");
    };
    assert_eq!(reported, destination_recovery);
    assert!(message.contains(&directory.display().to_string()));
    drop(original);
    assert_eq!(
        fs::read(&path).expect("document recovery survives last owner"),
        bytes
    );
    assert_eq!(
        fs::read(&backup).expect("destination recovery untouched"),
        b"different document"
    );
    fs::remove_file(path).expect("owned original cleanup");
    fs::remove_dir(directory).expect("owned empty recovery cleanup");
}
