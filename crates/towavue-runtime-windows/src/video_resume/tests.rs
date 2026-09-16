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
fn resume_clear_failure_reports_without_removing_existing_data() {
    let root = root();
    let history = root.join("not-a-history-file");
    fs::create_dir(&history).expect("owned obstruction");
    fs::write(history.join("keep.txt"), b"keep").expect("owned data");
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = VideoResumeHistory::new(history.clone(), move |event| {
        tx.send(event).expect("clear error");
    })
    .expect("worker");
    worker.clear(SystemTime::now());
    assert!(matches!(
        rx.recv_timeout(Duration::from_secs(5))
            .expect("clear error event"),
        VideoResumeEvent::ClearFailed(_)
    ));
    drop(worker);
    assert_eq!(
        fs::read(history.join("keep.txt")).expect("retained data"),
        b"keep"
    );
    fs::remove_dir_all(root).expect("remove owned fixtures");
}

#[test]
fn resume_clear_migrates_legacy_and_rejects_delayed_writes_across_restarts() {
    let root = root();
    let media = root.join("source.mkv");
    let history = root.join("resume.txt");
    fs::write(&media, b"owned video").expect("fixture");
    let source = VideoResumeSource::capture(&media).expect("source");
    let legacy = format!(
        "{LEGACY_HEADER}\n100\t{}\t{}\t5000000000\t{}\n",
        source.length,
        source.modified,
        media.display()
    );
    fs::write(&history, &legacy).expect("legacy fixture");
    assert_eq!(
        load_video_resume(&history, &media)
            .expect("legacy read")
            .position,
        Some(Duration::from_secs(5))
    );
    assert_eq!(
        fs::read_to_string(&history).expect("unchanged legacy"),
        legacy
    );
    let at = |n| UNIX_EPOCH + Duration::from_nanos(n);
    clear_video_resume(&history, at(200)).expect("clear");
    let cleared = fs::read_to_string(&history).expect("cleared file");
    assert_eq!(cleared, format!("{HEADER}\ncleared\t200\n"));
    for time in [100, 199, 200] {
        remember_video_resume(&history, &source, Duration::from_secs(8), at(time))
            .expect("stale writer");
        assert_eq!(
            fs::read_to_string(&history).expect("cutoff persists"),
            cleared
        );
    }
    remember_video_resume(&history, &source, Duration::from_secs(9), at(300))
        .expect("new activity");
    clear_video_resume(&history, at(150)).expect("older delayed clear");
    assert_eq!(
        load_video_resume(&history, &media)
            .expect("new activity survives")
            .position,
        Some(Duration::from_secs(9))
    );
    assert_eq!(read(&history).expect("cutoff").cleared, 200);
    // Explicit clearing repairs corrupt history; ordinary writes must not.
    fs::write(&history, b"invalid resume data").expect("corrupt fixture");
    assert!(remember_video_resume(&history, &source, Duration::ZERO, at(400)).is_err());
    clear_video_resume(&history, at(400)).expect("explicit repair");
    assert_eq!(
        load_video_resume(&history, &media)
            .expect("cleared corrupt history")
            .position,
        None
    );
    assert_eq!(
        VideoResumeSource::capture(&media).expect("unchanged source"),
        source
    );
    assert_eq!(fs::read(&media).expect("source bytes"), b"owned video");
    fs::remove_dir_all(root).expect("remove owned fixtures");
}

#[test]
fn resume_worker_clear_orders_pending_loads_writes_drop_and_other_workers() {
    let root = root();
    let media = root.join("source.mkv");
    let history = root.join("resume.txt");
    fs::write(&media, b"owned source").expect("fixture");
    let (tx, rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let worker = VideoResumeHistory::new(history.clone(), move |event| {
        let first = matches!(event, VideoResumeEvent::Loaded { token: 1, .. });
        tx.send(event).expect("event");
        if first {
            release_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("release");
        }
    })
    .expect("worker");
    worker.load(1, media.clone());
    let VideoResumeEvent::Loaded { result, .. } =
        rx.recv_timeout(Duration::from_secs(5)).expect("load")
    else {
        panic!("loaded event")
    };
    let source = result.expect("source").source;
    let at = |n| UNIX_EPOCH + Duration::from_secs(n);
    worker.remember(source.clone(), Duration::from_secs(10), at(10));
    worker.clear(at(20));
    worker.remember(source.clone(), Duration::from_secs(15), at(15));
    worker.load(2, media.clone());
    release_tx.send(()).expect("continue");
    let VideoResumeEvent::Loaded { token, result, .. } = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("post-clear load")
    else {
        panic!("loaded event")
    };
    assert_eq!(token, 2);
    assert_eq!(result.expect("post-clear result").position, None);
    let other = VideoResumeHistory::new(history.clone(), |_| {}).expect("other worker");
    other.remember(source.clone(), Duration::from_secs(19), at(19));
    drop(other);
    assert_eq!(
        load_video_resume(&history, &media)
            .expect("other worker cannot resurrect")
            .position,
        None
    );
    worker.remember(source, Duration::from_secs(30), at(30));
    worker.clear(at(25));
    drop(worker); // Clear and later writes drain even without another lookup.
    assert_eq!(
        load_video_resume(&history, &media)
            .expect("new write survives clear")
            .position,
        Some(Duration::from_secs(30))
    );
    fs::remove_dir_all(root).expect("remove owned fixtures");
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
    assert_eq!(
        read(&history).expect("record").entries[0].source,
        initial.source
    );
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
    write(
        &history,
        &History {
            entries,
            cleared: 0,
        },
    )
    .expect("bounded initial history");
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
    let entries = read(&history).expect("bounded history").entries;
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
            "{HEADER}\ncleared\t0\n1\t1\t1\t9223372036854775808\t{}\n",
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
    write(&history, &History::default()).expect("valid empty history");
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
