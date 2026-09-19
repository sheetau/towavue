use super::*;

fn request(expected: &FileOperationSource, original: &RetainedSource) -> ExportRequest {
    ExportRequest {
        source: original.original_path().to_owned(),
        target: expected.path().to_owned(),
        kind: MediaKind::Image,
        operations: vec![EditOperation::FlipHorizontal],
        hardware_encode: false,
    }
}

fn candidate(expected: &FileOperationSource, original: &RetainedSource) -> PreparedSourceSave {
    prepare_source_recreation(
        expected.clone(),
        original.clone(),
        request(expected, original),
        ExportOptions::default(),
        &AtomicBool::new(false),
        &|_| {},
        &|_| {},
    )
    .expect("prepare recreation")
}

#[test]
fn retained_copy_keeps_bytes_streams_and_read_only_ownership_after_removal() {
    let fixture = Fixture::new();
    let path = fixture.0.join("image.bmp");
    bitmap(&path);
    let bytes = fs::read(&path).expect("original bytes");
    fs::write(path.with_file_name("image.bmp:towavue-test"), b"owned ADS").expect("stream");
    let expected = FileOperationSource::capture(&path).expect("version");
    let original = RetainedSource::capture(&expected).expect("retained copy");
    let directory = original.0._files.directory.clone();
    assert_ne!(original.original_path(), path);
    assert_eq!(fs::read(original.original_path()).expect("copy"), bytes);
    assert_eq!(
        fs::read(
            original
                .original_path()
                .with_file_name("original.bmp:towavue-test")
        )
        .expect("copied stream"),
        b"owned ADS"
    );
    expected.verify().expect("source not mutated by retention");
    assert!(
        OpenOptions::new()
            .write(true)
            .open(original.original_path())
            .is_err()
    );
    assert!(fs::rename(original.original_path(), fixture.0.join("stolen.bmp")).is_err());
    fs::remove_file(&path).expect("remove owned source");
    let input = crate::MediaInput::retained(path.clone(), original.clone());
    drop(original);
    assert_eq!(input.logical_path(), path);
    assert_eq!(
        crate::decode_image(input.path())
            .expect("retained decode")
            .frames[0]
            .rgba,
        [255, 0, 0, 255, 0, 0, 255, 255]
    );
    let clone = input.clone();
    drop(input);
    assert!(directory.exists());
    drop(clone);
    wait_removed(&directory);
}

#[test]
fn retention_rejects_stale_or_written_sources_and_cleans_read_only_copies() {
    let fixture = Fixture::new();
    let path = fixture.0.join("image.bmp");
    bitmap(&path);
    let expected = FileOperationSource::capture(&path).expect("version");
    let writer = OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("competing writer");
    assert!(RetainedSource::capture(&expected).is_err());
    drop(writer);
    fs::write(&path, b"changed source").expect("change source");
    assert!(matches!(
        RetainedSource::capture(&expected),
        Err(FileOperationError::SourceChanged)
    ));
    bitmap(&path);
    let original_permissions = fs::metadata(&path).expect("metadata").permissions();
    let mut permissions = original_permissions.clone();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions).expect("read-only source");
    let expected = FileOperationSource::capture(&path).expect("read-only version");
    let original = RetainedSource::capture(&expected).expect("read-only retention");
    assert!(
        fs::metadata(&path)
            .expect("source permissions")
            .permissions()
            .readonly()
    );
    let directory = original.0._files.directory.clone();
    drop(original);
    wait_removed(&directory);
    fs::set_permissions(&path, original_permissions).expect("owned fixture cleanup");
}

#[test]
fn deleted_audio_and_video_recreate_edits_and_video_restarts_from_retained_bytes() {
    let fixture = Fixture::new();
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
        let output = std::process::Command::new(
            crate::media_tools::tool_path("ffmpeg.exe").expect("FFmpeg"),
        )
        .args(["-v", "error", "-f", "lavfi", "-i", input])
        .args(encoding)
        .arg(&path)
        .output()
        .expect("owned media generator");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let expected = FileOperationSource::capture(&path).expect("version");
        let original = RetainedSource::capture(&expected).expect("retain media");
        let original_bytes = fs::read(&path).expect("original bytes");
        let mut before_samples = Vec::new();
        let before = crate::decode::decode_file(&path, |item| {
            if let crate::DecodeOutput::Audio(chunk) = item {
                before_samples.extend(
                    chunk
                        .bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|p| f32::from_le_bytes(*p)),
                );
            }
            true
        })
        .expect("original decode");
        fs::remove_file(&path).expect("remove owned source");
        let unchanged = finish(
            prepare_source_recreation(
                expected.clone(),
                original.clone(),
                ExportRequest {
                    source: original.original_path().to_owned(),
                    target: path.clone(),
                    kind,
                    operations: Vec::new(),
                    hardware_encode: true,
                },
                ExportOptions::default(),
                &AtomicBool::new(false),
                &|_| {},
                &|_| {},
            )
            .expect("prepare untouched media"),
        )
        .expect("recreate without compression");
        assert_eq!(
            fs::read(&path).expect("exact original media"),
            original_bytes
        );
        assert!(!unchanged.outcome.used_hardware_encoder);
        drop(unchanged);
        fs::remove_file(&path).expect("remove owned recreated file for edited control");
        let directory = original.0._files.directory.clone();
        if kind == MediaKind::Video {
            let mut session = crate::PlaybackSession::open_input(
                crate::MediaInput::retained(path.clone(), original.clone()),
                crate::GraphicsDevice::warp_for_test().expect("WARP"),
                0.0,
                1.0,
                Default::default(),
                true,
                |_| {},
            )
            .expect("deleted video playback");
            for milliseconds in [0, 500] {
                let position = towavue_core::MediaTime::from_nanoseconds(milliseconds * 1_000_000);
                if milliseconds != 0 {
                    session
                        .set_rate_at(position, 1.0, false)
                        .expect("restart deleted video");
                }
                let deadline = Instant::now() + Duration::from_secs(5);
                while session.pending_video_time().is_none() {
                    assert!(Instant::now() < deadline, "retained frame deadline");
                    std::thread::sleep(Duration::from_millis(2));
                }
                assert!(session.advance_pending());
                assert_eq!(session.video_geometry(), Some((64, 48, 1.0)));
                assert_eq!(session.current_video_time(), Some(position));
            }
        }
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
        let saved = finish(
            prepare_source_recreation(
                expected,
                original.clone(),
                ExportRequest {
                    source: original.original_path().to_owned(),
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
            .expect("prepare deleted media"),
        )
        .expect("recreate edited media");
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
                        .map(|p| f32::from_le_bytes(*p)),
                ),
            }
            true
        })
        .expect("recreated decode");
        assert_eq!(before, after);
        if kind == MediaKind::Audio {
            assert_eq!(samples.len(), before_samples.len());
            assert!(
                samples
                    .iter()
                    .zip(&before_samples)
                    .all(|(actual, before)| (*actual - before * 0.5).abs() < 0.00004)
            );
        }
        assert_eq!(
            fs::read(original.original_path()).expect("original media preserved"),
            original_bytes
        );
        drop(original);
        drop(saved);
        wait_removed(&directory);
    }
}

#[test]
fn recreation_retains_the_earliest_original_for_subsequent_save_and_undo() {
    let fixture = Fixture::new();
    let path = fixture.0.join("image.bmp");
    bitmap(&path);
    let expected = FileOperationSource::capture(&path).expect("version");
    let original = RetainedSource::capture(&expected).expect("retain");
    let original_bytes = fs::read(&path).expect("original bytes");
    let original_path = original.original_path().to_owned();
    let directory = original.0._files.directory.clone();
    fs::remove_file(&path).expect("remove owned source");
    let prepared = candidate(&expected, &original);
    assert!(!path.exists(), "preparation cannot recreate early");
    let staging = prepared.files.directory.clone();
    let saved = finish(prepared).expect("recreate Save");
    wait_removed(&staging);
    assert_eq!(saved.original_path(), original_path);
    saved.current_source().verify().expect("published identity");
    assert_eq!(
        crate::decode_image(&path).expect("saved pixels").frames[0].rgba,
        [0, 0, 255, 255, 255, 0, 0, 255]
    );
    let mut undo = request(saved.current_source(), &original);
    undo.operations.clear();
    let reverted = finish(
        prepare_source_save(
            saved.current_source().clone(),
            undo,
            ExportOptions::default(),
            &AtomicBool::new(false),
            &|_| {},
            &|_| {},
        )
        .expect("prepare Undo save"),
    )
    .expect("publish Undo save");
    assert_eq!(
        crate::decode_image(&path).expect("Undo pixels").frames[0].rgba,
        crate::decode_image(&original_path)
            .expect("original pixels")
            .frames[0]
            .rgba
    );
    assert_eq!(
        fs::read(&original_path).expect("earliest original"),
        original_bytes
    );
    drop(original);
    let input = crate::MediaInput::retained(path, saved.retained_source());
    drop(saved);
    assert!(directory.exists());
    drop(input);
    wait_removed(&directory);
    let second_directory = reverted.original._files.directory.clone();
    drop(reverted);
    wait_removed(&second_directory);
}

#[test]
fn untouched_recreation_keeps_the_exact_file_and_named_streams() {
    let fixture = Fixture::new();
    let path = fixture.0.join("image.bmp");
    bitmap(&path);
    fs::write(
        path.with_file_name("image.bmp:towavue-test"),
        b"retained stream",
    )
    .expect("ADS");
    let bytes = fs::read(&path).expect("original bytes");
    let expected = FileOperationSource::capture(&path).expect("source identity");
    let original = RetainedSource::capture(&expected).expect("retain");
    fs::remove_file(&path).expect("remove owned source");
    let mut request = request(&expected, &original);
    request.operations.clear();
    let saved = finish(
        prepare_source_recreation(
            expected,
            original.clone(),
            request,
            ExportOptions::default(),
            &AtomicBool::new(false),
            &|_| {},
            &|_| {},
        )
        .expect("prepare exact recreation"),
    )
    .expect("publish exact recreation");
    assert_eq!(fs::read(&path).expect("restored bytes"), bytes);
    assert_eq!(
        fs::read(path.with_file_name("image.bmp:towavue-test")).expect("restored ADS"),
        b"retained stream"
    );
    assert!(!saved.outcome.used_hardware_encoder);
    let directory = original.0._files.directory.clone();
    drop(saved);
    drop(original);
    wait_removed(&directory);
}

#[test]
fn failed_recycling_preserves_the_copy_only_when_the_original_is_uncertain() {
    // Inject completion states, not a native Shell failure. The separate opt-in
    // covers actual recycling; this verifies recovery ownership after an error.
    let fixture = Fixture::new();
    for state in ["unchanged", "missing", "replaced"] {
        let path = fixture.0.join(format!("{state}.bmp"));
        bitmap(&path);
        let bytes = fs::read(&path).expect("original bytes");
        let expected = FileOperationSource::capture(&path).expect("source version");
        let original = RetainedSource::capture(&expected).expect("retain source");
        let backup = original.original_path().to_owned();
        let directory = original.0._files.directory.clone();
        if state != "unchanged" {
            fs::remove_file(&path).expect("simulate completed removal");
            if state == "replaced" {
                fs::write(&path, b"external replacement").expect("new owner");
            }
        }
        let result = crate::file_operation::finish_retained_recycle(
            &expected,
            Some(&original),
            Err(FileOperationError::NotCompleted),
        );
        if state == "unchanged" {
            assert!(matches!(result, Err(FileOperationError::NotCompleted)));
            drop(original);
            wait_removed(&directory);
            assert_eq!(fs::read(&path).expect("original kept"), bytes);
        } else {
            assert!(
                matches!(result, Err(FileOperationError::RecoveryRequired { directory: ref preserved, .. }) if preserved == &directory)
            );
            drop(original);
            assert_eq!(
                fs::read(&backup).expect("recovery bytes survive last owner"),
                bytes
            );
            if state == "replaced" {
                assert_eq!(
                    fs::read(&path).expect("replacement unchanged"),
                    b"external replacement"
                );
            } else {
                assert!(!path.exists());
            }
            // Remove only the two known owned fixture entries, never a tree.
            assert!(
                directory
                    .file_name()
                    .expect("owned directory")
                    .to_string_lossy()
                    .starts_with(".towavue-retained-")
            );
            fs::remove_file(backup).expect("owned recovery-file cleanup");
            fs::remove_dir(directory).expect("owned empty recovery-directory cleanup");
        }
    }
}

#[test]
fn recreation_rejects_collisions_tampering_cancellation_and_unrelated_inputs() {
    let fixture = Fixture::new();
    let path = fixture.0.join("image.bmp");
    bitmap(&path);
    let expected = FileOperationSource::capture(&path).expect("version");
    let original = RetainedSource::capture(&expected).expect("retain");
    fs::remove_file(&path).expect("remove owned source");
    let prepare = |request, cancelled| {
        prepare_source_recreation(
            expected.clone(),
            original.clone(),
            request,
            ExportOptions::default(),
            &AtomicBool::new(cancelled),
            &|_| {},
            &|_| {},
        )
    };
    assert!(matches!(
        prepare(request(&expected, &original), true),
        Err(SourceSaveError::Export(ExportError::Cancelled))
    ));
    let mut unrelated = request(&expected, &original);
    unrelated.source = fixture.0.join("unrelated.bmp");
    assert!(matches!(
        prepare(unrelated, false),
        Err(SourceSaveError::InvalidRequest)
    ));
    fs::write(&path, b"external occupant").expect("collision");
    assert!(matches!(prepare(request(&expected, &original), false),
        Err(SourceSaveError::Io(error)) if error.kind() == io::ErrorKind::AlreadyExists));
    assert_eq!(
        fs::read(&path).expect("external file"),
        b"external occupant"
    );
    fs::remove_file(&path).expect("remove owned collision");
    let prepared = candidate(&expected, &original);
    let staging = prepared.files.directory.clone();
    fs::write(&path, b"late external occupant").expect("late collision");
    // Exercise the native no-replacement primitive independently of the earlier
    // missing-path check: an entry created in that gap still cannot be replaced.
    assert!(restore_missing(&prepared.files.prepared, &path).is_err());
    assert!(prepared.files.prepared.is_file());
    assert!(matches!(finish(prepared), Err(SourceSaveError::Io(_))));
    wait_removed(&staging);
    assert_eq!(
        fs::read(&path).expect("late file"),
        b"late external occupant"
    );
    fs::remove_file(&path).expect("remove owned collision");
    let prepared = candidate(&expected, &original);
    let staging = prepared.files.directory.clone();
    fs::write(&prepared.files.prepared, b"tampered candidate").expect("tamper");
    assert!(matches!(finish(prepared), Err(SourceSaveError::Source(_))));
    wait_removed(&staging);
    assert!(!path.exists());
    let prepared = candidate(&expected, &original);
    let staging = prepared.files.directory.clone();
    drop(prepared);
    wait_removed(&staging);
    assert!(!path.exists(), "dropping a candidate cancels recreation");
    assert!(crate::decode_image(original.original_path()).is_ok());
    let directory = original.0._files.directory.clone();
    drop(original);
    wait_removed(&directory);
}

#[test]
fn recreation_job_delivers_one_candidate_and_cancellation_keeps_the_path_missing() {
    let fixture = Fixture::new();
    let path = fixture.0.join("image.bmp");
    bitmap(&path);
    let expected = FileOperationSource::capture(&path).expect("version");
    let original = RetainedSource::capture(&expected).expect("retain");
    fs::remove_file(&path).expect("remove owned source");
    for cancel in [false, true] {
        let (send, receive) = mpsc::channel();
        let job = SourceSaveJob::start_recreating(
            expected.clone(),
            original.clone(),
            request(&expected, &original),
            ExportOptions::default(),
            move |event| {
                send.send(event).expect("preparation receiver");
            },
        )
        .expect("recreation worker");
        if cancel {
            job.cancel();
        }
        let prepared = loop {
            if let SourceSaveEvent::Prepared(result) = receive
                .recv_timeout(Duration::from_secs(5))
                .expect("terminal result")
            {
                break result;
            }
        };
        drop(job);
        assert!(matches!(
            receive.recv_timeout(Duration::from_secs(1)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
        match prepared {
            Ok(candidate) => {
                let staging = candidate.files.directory.clone();
                drop(candidate);
                wait_removed(&staging);
            }
            Err(SourceSaveError::Export(ExportError::Cancelled)) if cancel => {}
            Err(error) => panic!("unexpected preparation result: {error}"),
        }
        assert!(!path.exists());
    }
    let directory = original.0._files.directory.clone();
    drop(original);
    wait_removed(&directory);
}

#[test]
#[ignore = "recycles one owned bitmap and recreates it through native source publication"]
fn native_recycle_retains_a_document_and_recreation_preserves_other_files() {
    let fixture = Fixture::new();
    let path = fixture.0.join("image.bmp");
    let other = fixture.0.join("other.bmp");
    bitmap(&path);
    fs::write(&other, b"untouched neighbor").expect("neighbor");
    let bytes = fs::read(&path).expect("original bytes");
    let expected = FileOperationSource::capture(&path).expect("source version");
    let (send, receive) = mpsc::channel();
    crate::start_file_recycling_retaining_source(expected.clone(), move |result| {
        send.send(result).expect("recycle receiver");
    })
    .expect("native recycle worker");
    let report = receive
        .recv_timeout(Duration::from_secs(10))
        .expect("recycle result")
        .expect("native recycle");
    assert!(matches!(
        receive.recv_timeout(Duration::from_secs(1)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
    assert!(!path.exists());
    assert!(report.before.items.iter().any(|item| item.path == path));
    assert!(
        report
            .after
            .as_ref()
            .expect("refreshed Shell order")
            .items
            .iter()
            .all(|item| item.path != path)
    );
    let original = report.retained_source.expect("owned retained source");
    assert_eq!(
        fs::read(original.original_path()).expect("retained bytes"),
        bytes
    );
    let directory = original.0._files.directory.clone();
    let saved = finish(candidate(&expected, &original)).expect("recreate deleted source");
    assert_eq!(
        crate::decode_image(&path).expect("edited pixels").frames[0].rgba,
        [0, 0, 255, 255, 255, 0, 0, 255]
    );
    assert_eq!(
        fs::read(other).expect("neighbor preserved"),
        b"untouched neighbor"
    );
    drop(original);
    drop(saved);
    wait_removed(&directory);
    eprintln!(
        "PASS retained native recycle: original bytes kept, edited source recreated without replacing a neighbor, final reader cleanup verified; owned fixture only"
    );
}
