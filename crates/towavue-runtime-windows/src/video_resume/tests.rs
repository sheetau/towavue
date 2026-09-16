use super::*;

fn root() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "towavue-resume-test-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&root).expect("fixture directory");
    root
}

#[test]
fn resume_round_trip_resets_reject_stale_writers_and_preserve_source_bytes() {
    let root = root();
    let media = root.join("日本語 ' video.mkv");
    fs::write(&media, b"owned fixture source").expect("fixture");
    let history = root.join("config/video-resume.txt");
    let initial = load_video_resume(&history, &media).expect("new history");
    assert_eq!(initial.position, None);
    let stamp = fs::metadata(&media)
        .expect("source")
        .modified()
        .expect("mtime");
    let observed = UNIX_EPOCH + Duration::from_secs(100);
    let position = Duration::from_nanos(12_345_678_901);
    remember_video_resume(&history, &initial.source, position, observed).expect("record");
    assert_eq!(
        load_video_resume(&history, &media)
            .expect("restart")
            .position,
        Some(position)
    );
    remember_video_resume(
        &history,
        &initial.source,
        Duration::ZERO,
        observed + Duration::from_secs(1),
    )
    .expect("EOF reset");
    remember_video_resume(&history, &initial.source, position, observed).expect("delayed write");
    assert_eq!(
        load_video_resume(&history, &media)
            .expect("reset persists")
            .position,
        None
    );
    assert_eq!(
        fs::read(&media).expect("source bytes"),
        b"owned fixture source"
    );
    assert_eq!(
        fs::metadata(&media)
            .expect("source")
            .modified()
            .expect("mtime"),
        stamp
    );
    fs::write(&media, b"replaced video with another length").expect("replacement");
    remember_video_resume(
        &history,
        &initial.source,
        position,
        observed + Duration::from_secs(2),
    )
    .expect("reject replaced source");
    assert_eq!(
        load_video_resume(&history, &media)
            .expect("replacement position")
            .position,
        None
    );
    assert_eq!(read(&history).expect("record")[0].source, initial.source);
    fs::remove_dir_all(root).expect("remove owned fixtures");
}

#[test]
fn concurrent_resume_writers_merge_and_keep_only_the_newest_two_hundred() {
    let root = root();
    let media = root.join("video.mkv");
    fs::write(&media, b"fixture").expect("source");
    let history = root.join("resume.txt");
    let source = load_video_resume(&history, &media)
        .expect("source stamp")
        .source;
    let entries: Vec<_> = (0..LIMIT)
        .map(|index| {
            let mut source = source.clone();
            source.path = root.join(format!("{index}.mkv"));
            Entry {
                source,
                position: index as u64,
                observed: index as u128,
            }
        })
        .collect();
    write(&history, &entries).expect("bounded initial history");
    std::thread::scope(|scope| {
        for stamp in [400, 300, 500, 200] {
            let history = &history;
            let source = &source;
            scope.spawn(move || {
                remember_video_resume(
                    history,
                    source,
                    Duration::from_secs(stamp),
                    UNIX_EPOCH + Duration::from_secs(stamp),
                )
                .expect("concurrent record")
            });
        }
    });
    assert_eq!(
        load_video_resume(&history, &media)
            .expect("merged")
            .position,
        Some(Duration::from_secs(500))
    );
    let entries = read(&history).expect("bounded history");
    assert_eq!(entries.len(), LIMIT);
    assert!(
        !entries
            .iter()
            .any(|entry| entry.source.path == root.join("0.mkv"))
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry.source.path == root.join("199.mkv"))
    );
    fs::remove_dir_all(root).expect("remove owned fixtures");
}

#[test]
fn malformed_history_and_invalid_positions_never_overwrite_existing_data() {
    let root = root();
    let media = root.join("video.mkv");
    fs::write(&media, b"fixture").expect("source");
    let history = root.join("resume.txt");
    let source = load_video_resume(&history, &media).expect("source").source;
    for contents in [
        "unknown version".to_owned(),
        format!("{HEADER}\ninvalid\n"),
        format!(
            "{HEADER}\n1\t1\t1\t9223372036854775808\t{}\n",
            media.display()
        ),
        "x".repeat(MAX_BYTES as usize + 1),
    ] {
        fs::write(&history, &contents).expect("malformed fixture");
        assert!(load_video_resume(&history, &media).is_err());
        assert!(
            remember_video_resume(&history, &source, Duration::from_secs(1), SystemTime::now())
                .is_err()
        );
        assert_eq!(
            fs::read_to_string(&history).expect("preserved history"),
            contents
        );
    }
    write(&history, &[]).expect("valid empty history");
    let bytes = fs::read(&history).expect("history");
    assert!(remember_video_resume(&history, &source, Duration::MAX, SystemTime::now()).is_err());
    assert_eq!(fs::read(&history).expect("unchanged history"), bytes);
    fs::remove_dir_all(root).expect("remove owned fixtures");
}

#[test]
fn resume_worker_keeps_latest_lookup_orders_writes_and_flushes_on_drop() {
    let root = root();
    let media = root.join("video.mkv");
    fs::write(&media, b"fixture").expect("source");
    let history = root.join("resume.txt");
    let (events_tx, events_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let worker = VideoResumeHistory::new(history.clone(), move |event| {
        let first = matches!(event, VideoResumeEvent::Loaded { token: 1, .. });
        events_tx.send(event).expect("event");
        if first {
            release_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("release first lookup");
        }
    })
    .expect("worker");
    worker.load(1, media.clone());
    let VideoResumeEvent::Loaded { result, .. } = events_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("lookup")
    else {
        panic!("load result")
    };
    let source = result.expect("stamp").source;
    worker.load(2, root.join("obsolete.mkv"));
    worker.load(3, media.clone());
    worker.remember(
        source.clone(),
        Duration::from_secs(20),
        UNIX_EPOCH + Duration::from_secs(200),
    );
    worker.remember(
        source.clone(),
        Duration::from_secs(10),
        UNIX_EPOCH + Duration::from_secs(100),
    );
    release_tx.send(()).expect("continue worker");
    let VideoResumeEvent::Loaded {
        token,
        path,
        result,
    } = events_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("latest lookup")
    else {
        panic!("load result")
    };
    assert_eq!(token, 3);
    assert_eq!(path, media);
    assert_eq!(
        result.expect("loaded after writes").position,
        Some(Duration::from_secs(20))
    );
    assert!(events_rx.try_recv().is_err());
    worker.remember(
        source,
        Duration::ZERO,
        UNIX_EPOCH + Duration::from_secs(300),
    );
    drop(worker);
    assert_eq!(
        load_video_resume(&history, &media)
            .expect("drop drained reset")
            .position,
        None
    );
    fs::remove_dir_all(root).expect("remove owned fixtures");
}

#[test]
fn same_length_revision_changes_and_failed_worker_writes_keep_the_old_history() {
    let root = root();
    let media = root.join("video.mkv");
    fs::write(&media, b"fixture").expect("source");
    let history = root.join("resume.txt");
    let source = load_video_resume(&history, &media).expect("stamp").source;
    remember_video_resume(&history, &source, Duration::from_secs(2), SystemTime::now())
        .expect("record");
    let bytes = fs::read(&history).expect("history");
    File::options()
        .write(true)
        .open(&media)
        .expect("fixture handle")
        .set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(10)))
        .expect("different modification time");
    assert_eq!(
        load_video_resume(&history, &media)
            .expect("changed source")
            .position,
        None
    );
    remember_video_resume(&history, &source, Duration::from_secs(5), SystemTime::now())
        .expect("stale source ignored");
    assert_eq!(fs::read(&history).expect("unchanged history"), bytes);
    let source = load_video_resume(&history, &media)
        .expect("fresh stamp")
        .source;
    fs::write(&history, b"preserve corrupt history").expect("corrupt fixture");
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = VideoResumeHistory::new(history.clone(), move |event| {
        tx.send(event).expect("event");
    })
    .expect("worker");
    worker.remember(source, Duration::from_secs(5), SystemTime::now());
    drop(worker);
    assert!(matches!(
        rx.recv_timeout(Duration::from_secs(5))
            .expect("failure event"),
        VideoResumeEvent::SaveFailed(_)
    ));
    assert_eq!(
        fs::read(&history).expect("preserved"),
        b"preserve corrupt history"
    );
    fs::remove_dir_all(root).expect("remove owned fixtures");
}
