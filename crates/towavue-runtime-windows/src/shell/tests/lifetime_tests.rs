use super::*;

struct TrialWorker {
    provider: FolderOrderProvider,
    stopped: mpsc::Receiver<()>,
    thread: thread::JoinHandle<()>,
}

impl TrialWorker {
    fn start() -> Self {
        Self::spawn(true)
    }

    fn spawn(initialize_before_ready: bool) -> Self {
        let shared = Arc::new((
            Mutex::new(Mailbox::default()),
            ShellWake::new().expect("wake"),
        ));
        let provider = FolderOrderProvider {
            shared: Arc::clone(&shared),
        };
        let (initialized, ready) = mpsc::channel();
        let (finished, stopped) = mpsc::channel();
        let thread = thread::spawn(move || {
            // The overlap trial establishes an STA before readiness; restart trials
            // retain production's lazy initialization without an extra OLE reference.
            let apartment = initialize_before_ready.then(ShellApartment::new);
            assert!(apartment.as_ref().is_none_or(|apartment| apartment.0));
            initialized.send(()).expect("ready observer");
            shell_worker(shared, || {});
            drop(apartment);
            assert_eq!(SHELL_INITIALIZATIONS.get(), SHELL_UNINITIALIZATIONS.get());
            finished.send(()).expect("shutdown observer");
        });
        ready
            .recv_timeout(Duration::from_secs(10))
            .expect("worker ready");
        Self {
            provider,
            stopped,
            thread,
        }
    }

    fn close(self) -> (mpsc::Receiver<()>, thread::JoinHandle<()>) {
        drop(self.provider);
        (self.stopped, self.thread)
    }
}

#[test]
#[ignore = "native Shell teardown/restart stress; run alone"]
fn restarting_shell_worker_during_teardown_preserves_native_snapshots() {
    let root = test_directory("restarting-lifetimes");
    fs::create_dir(&root).expect("owned fixture folder");
    fs::write(root.join("item.jpg"), []).expect("owned file");
    eprintln!("SHELL_RESTART fixture={}", root.display());
    let snapshot = |provider: &FolderOrderProvider| {
        let (reply, response) = mpsc::channel();
        provider.enqueue(Some(root.clone()), Some(reply));
        response
            .recv_timeout(Duration::from_secs(10))
            .expect("native snapshot")
    };
    for round in 0..48 {
        // Unlike the overlapping-lifetime trial, do not establish a secondary
        // apartment before closing the old provider, or add an extra OLE reference.
        let phase = ["warmed", "pending", "parsed-only"][round % 3];
        let stopped = if phase == "parsed-only" {
            let folder = root.clone();
            let (parsed, ready) = mpsc::channel();
            let (finished, stopped) = mpsc::channel();
            let worker = thread::spawn(move || {
                let apartment = ShellApartment::new();
                assert!(apartment.0);
                // Isolate cancellation after path parsing but before live-view
                // lookup or creation of an IExplorerBrowser changes COM state.
                drop(parse_path(&folder).expect("parsed folder"));
                parsed.send(()).expect("parse observer");
                drop(apartment);
                assert_eq!(SHELL_INITIALIZATIONS.get(), SHELL_UNINITIALIZATIONS.get());
                finished.send(()).expect("shutdown observer");
            });
            ready.recv_timeout(Duration::from_secs(10)).expect("parsed");
            (stopped, worker)
        } else {
            let first = TrialWorker::spawn(false);
            if phase == "warmed" {
                let snapshot = snapshot(&first.provider);
                assert_ne!(snapshot.source, FolderSnapshotSource::NaturalNameFallback);
            } else {
                first.provider.request(Some(root.clone()));
                thread::sleep(Duration::from_millis([0, 1, 5, 10][round / 3 % 4]));
            }
            first.close()
        };
        let delay = [0, 1, 5, 10][round / 12];
        thread::sleep(Duration::from_millis(delay));
        let next = TrialWorker::spawn(false);
        let snapshot = snapshot(&next.provider);
        assert_ne!(snapshot.source, FolderSnapshotSource::NaturalNameFallback);
        assert_eq!(snapshot.items.len(), 1);
        assert_eq!(snapshot.items[0].path, root.join("item.jpg"));
        join(stopped);
        join(next.close());
        eprintln!(
            "SHELL_RESTART round={} phase={phase} delay_ms={delay} completed",
            round + 1
        );
    }
    fs::remove_dir_all(root).expect("remove owned fixture after workers exit");
}

fn join((stopped, thread): (mpsc::Receiver<()>, thread::JoinHandle<()>)) {
    stopped
        .recv_timeout(Duration::from_secs(10))
        .expect("Shell thread shutdown");
    thread.join().expect("Shell worker panic");
}

#[test]
#[ignore = "native Shell lifetime stress; run alone, optionally TOWAVUE_SHELL_LIFETIME_ANCHOR=keep"]
fn overlapping_shell_worker_lifetimes_preserve_native_snapshots() {
    let keep_first = std::env::var("TOWAVUE_SHELL_LIFETIME_ANCHOR").as_deref() == Ok("keep");
    let root = test_directory("overlapping-lifetimes");
    fs::create_dir(&root).expect("owned fixture folder");
    fs::write(root.join("item2.jpg"), []).expect("owned file");
    fs::write(root.join("item10.jpg"), []).expect("owned file");
    eprintln!(
        "SHELL_LIFETIME keep_first={keep_first} fixture={}",
        root.display()
    );
    for round in 0..32 {
        let mut first = TrialWorker::start();
        let phase = ["warmed", "pending", "initialized-only"][round % 3];
        let mut expected = if phase == "warmed" {
            let initial = first.provider.snapshot(&root).expect("initial native view");
            assert_ne!(initial.source, FolderSnapshotSource::NaturalNameFallback);
            Some(
                initial
                    .items
                    .into_iter()
                    .map(|item| (item.path, item.kind))
                    .collect::<Vec<_>>(),
            )
        } else {
            if phase == "pending" {
                first.provider.request(Some(root.clone()));
                thread::sleep(Duration::from_millis([0, 1, 5, 10][round / 3 % 4]));
            }
            None
        };
        let workers: Vec<_> = (0..4).map(|_| TrialWorker::start()).collect();
        let replies: Vec<_> = workers
            .iter()
            .map(|worker| {
                let (reply, response) = mpsc::channel();
                worker.provider.enqueue(Some(root.clone()), Some(reply));
                response
            })
            .collect();
        let deadline = Instant::now() + Duration::from_secs(10);
        while workers.iter().any(|worker| {
            worker
                .provider
                .shared
                .0
                .lock()
                .expect("mailbox")
                .pending
                .is_some()
        }) {
            assert!(Instant::now() < deadline, "secondary requests must start");
            thread::yield_now();
        }
        let mut first = Some(first);
        if !keep_first {
            // Confirm the first STA has actually uninitialized while all secondary
            // providers remain alive, not merely that its close signal was sent.
            join(first.take().expect("first worker").close());
        }
        for response in replies {
            let snapshot = response
                .recv_timeout(Duration::from_secs(10))
                .expect("native snapshot");
            assert_ne!(snapshot.source, FolderSnapshotSource::NaturalNameFallback);
            // PIDLs can carry changing provider metadata across distinct views.
            // Compare the observable file order, not raw PIDL byte equality.
            let items: Vec<_> = snapshot
                .items
                .into_iter()
                .map(|item| (item.path, item.kind))
                .collect();
            assert_eq!(items.len(), 2);
            assert!(
                items
                    .iter()
                    .any(|(path, _)| path == &root.join("item2.jpg"))
            );
            assert!(
                items
                    .iter()
                    .any(|(path, _)| path == &root.join("item10.jpg"))
            );
            if let Some(expected) = &expected {
                assert_eq!(&items, expected);
            } else {
                expected = Some(items);
            }
        }
        for worker in workers {
            join(worker.close());
        }
        if let Some(first) = first {
            join(first.close());
        }
        eprintln!("SHELL_LIFETIME round={} phase={phase} completed", round + 1);
    }
    fs::remove_dir_all(root).expect("remove owned fixture after workers exit");
}
