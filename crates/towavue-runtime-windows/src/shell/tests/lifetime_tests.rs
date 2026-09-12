use super::*;

struct TrialWorker {
    provider: FolderOrderProvider,
    stopped: mpsc::Receiver<()>,
    thread: thread::JoinHandle<()>,
}

impl TrialWorker {
    fn start() -> Self {
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
            // Establish a deterministic first STA. The extra balanced OLE reference
            // lives on this same worker and does not change its apartment identity.
            let apartment = ShellApartment::new();
            assert!(apartment.0, "test STA initialization");
            initialized.send(()).expect("ready observer");
            shell_worker(shared, || {});
            drop(apartment);
            assert_eq!(SHELL_INITIALIZATIONS.get(), SHELL_UNINITIALIZATIONS.get());
            finished.send(()).expect("shutdown observer");
        });
        ready
            .recv_timeout(Duration::from_secs(10))
            .expect("STA ready");
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
