use super::*;
use crate::update::crypto::{TestSigner, sha256};
use towavue_core::release::ReleaseManifest;

pub(in crate::update) struct Fixture {
    pub(in crate::update) store: UpdateStore,
    signer: TestSigner,
}

impl Fixture {
    pub(in crate::update) fn new() -> Self {
        let signer = TestSigner::new();
        // Hosted Windows may expose TEMP through an 8.3 alias. The production
        // helper requires normalized long paths, as supplied by installation discovery.
        let temporary = crate::shell::canonical_shell_path(&std::env::temp_dir())
            .expect("canonical fixture parent");
        let root = temporary.join(format!(
            "towavue-update-{}",
            stage_name().expect("owned fixture")
        ));
        let mut store = UpdateStore::new(root);
        store.test_key = Some(signer.public.clone());
        Self { store, signer }
    }

    fn signed(&self, version: u32, payload: &[u8]) -> SignedUpdate {
        let hash = sha256(payload)
            .expect("owned fixture")
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let bytes = format!(
            "towavue-update-v1\n1.0.{version}\nwindows-x64\n{}\n{hash}\n",
            payload.len()
        )
        .into_bytes();
        SignedUpdate::authenticate_with_key(
            ReleaseManifest::parse(&bytes).expect("owned fixture"),
            &bytes,
            &self.signer.sign(&bytes),
            &self.signer.public,
        )
        .expect("owned fixture")
    }

    pub(in crate::update) fn stage(&self, version: u32) -> CachedUpdate {
        self.store
            .prepare(self.signed(version, b"test payload"), |f| {
                f.write_all(b"test payload")
            })
            .expect("owned fixture")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Only this test's freshly generated path. Normal product code never
        // recursively removes cache content or an installation directory.
        let root = &self.store.root;
        let temporary = crate::shell::canonical_shell_path(&std::env::temp_dir())
            .expect("canonical fixture parent");
        assert_eq!(root.parent(), Some(temporary.as_path()));
        assert!(
            root.file_name()
                .expect("owned fixture")
                .to_string_lossy()
                .starts_with("towavue-update-stage-")
        );
        let _ = fs::remove_dir_all(root);
    }
}

#[test]
fn cache_reauthenticates_and_retains_payload_and_directory_locks() {
    let fixture = Fixture::new();
    assert!(fixture.store.load().expect("owned fixture").is_none());
    let mut cached = fixture.stage(1);
    assert_eq!(cached.version(), ReleaseVersion(1, 0, 1));
    assert!(
        OpenOptions::new()
            .write(true)
            .open(cached.directory.join(SETUP))
            .is_err()
    );
    assert!(fs::rename(&cached.directory, fixture.store.root.join("renamed")).is_err());
    assert_eq!(
        fixture
            .store
            .load()
            .expect("load cache")
            .expect("cached update")
            .phase(),
        UpdatePhase::Ready
    );
    fixture
        .store
        .transition(&mut cached, UpdatePhase::NextLaunch)
        .expect("owned fixture");
    drop(cached);
    let mut cached = fixture
        .store
        .load()
        .expect("load cache")
        .expect("cached update");
    assert_eq!(cached.phase(), UpdatePhase::NextLaunch);
    assert!(
        fixture
            .store
            .transition(&mut cached, UpdatePhase::Ready)
            .is_err()
    );
    fixture
        .store
        .transition(&mut cached, UpdatePhase::Installing)
        .expect("owned fixture");
    fixture
        .store
        .transition(&mut cached, UpdatePhase::Failed)
        .expect("owned fixture");
    assert_eq!(
        fixture
            .store
            .load()
            .expect("load cache")
            .expect("cached update")
            .phase(),
        UpdatePhase::Failed
    );
    let directory = cached.directory.clone();
    drop(cached);
    fs::write(directory.join(SETUP), b"TEST payload").expect("owned fixture");
    assert!(
        fixture.store.load().is_err(),
        "same-size tampering must fail"
    );
    fs::write(directory.join(SETUP), b"test payload").expect("owned fixture");
    let signature = fs::read(directory.join(SIGNATURE)).expect("owned fixture");
    let mut changed = signature.clone();
    changed[0] ^= 1;
    fs::write(directory.join(SIGNATURE), changed).expect("owned fixture");
    assert!(
        fixture.store.load().is_err(),
        "signature tampering must fail"
    );
    fs::write(directory.join(SIGNATURE), signature).expect("owned fixture");
    fs::write(directory.join(MANIFEST), vec![b'a'; 257]).expect("owned fixture");
    assert!(fixture.store.load().is_err(), "oversize metadata must fail");
}

#[test]
fn cache_refuses_stale_decisions_and_preserves_scheduled_updates() {
    let fixture = Fixture::new();
    let mut first = fixture.stage(1);
    let mut second = fixture.stage(2);
    assert!(
        fixture
            .store
            .transition(&mut first, UpdatePhase::NextLaunch)
            .is_err()
    );
    let old_state = fs::read(fixture.store.root.join(STATE)).expect("owned fixture");
    assert!(
        fixture
            .store
            .prepare(fixture.signed(1, b"old"), |f| f.write_all(b"old"))
            .is_err()
    );
    assert_eq!(
        fs::read(fixture.store.root.join(STATE)).expect("owned fixture"),
        old_state
    );
    fixture
        .store
        .transition(&mut second, UpdatePhase::NextLaunch)
        .expect("owned fixture");
    let scheduled = fs::read(fixture.store.root.join(STATE)).expect("owned fixture");
    assert!(
        fixture
            .store
            .prepare(fixture.signed(3, b"new"), |f| f.write_all(b"new"))
            .is_err()
    );
    assert_eq!(
        fs::read(fixture.store.root.join(STATE)).expect("owned fixture"),
        scheduled
    );
    let _lock = fixture.store.lock().expect("owned fixture");
    assert!(
        fixture.store.load().is_err(),
        "concurrent transition must not race"
    );
}

#[test]
fn failed_or_cancelled_downloads_leave_previous_state_and_unrelated_files_intact() {
    let fixture = Fixture::new();
    let _cached = fixture.stage(1);
    let previous = fs::read(fixture.store.root.join(STATE)).expect("owned fixture");
    let sentinel = fixture.store.root.join("user-file.txt");
    fs::write(&sentinel, b"preserve me").expect("owned fixture");
    let count = fs::read_dir(&fixture.store.root)
        .expect("owned fixture")
        .count();
    assert!(
        fixture
            .store
            .prepare(fixture.signed(2, b"new"), |f| {
                f.write_all(b"n")?;
                Err(io::ErrorKind::Interrupted.into())
            })
            .is_err()
    );
    assert!(
        fixture
            .store
            .prepare(fixture.signed(2, b"new"), |f| f.write_all(b"bad"))
            .is_err()
    );
    assert_eq!(
        fs::read_dir(&fixture.store.root)
            .expect("owned fixture")
            .count(),
        count
    );
    assert_eq!(
        fs::read(fixture.store.root.join(STATE)).expect("owned fixture"),
        previous
    );
    assert_eq!(fs::read(sentinel).expect("owned fixture"), b"preserve me");
}

#[test]
fn state_refuses_traversal_unknown_phases_and_ambiguous_encodings() {
    let stage = stage_name().expect("owned fixture");
    for phase in [
        UpdatePhase::Ready,
        UpdatePhase::NextLaunch,
        UpdatePhase::Installing,
        UpdatePhase::Failed,
    ] {
        let state = State {
            stage: stage.clone(),
            phase,
        };
        assert_eq!(State::parse(&state.bytes()).expect("owned fixture"), state);
    }
    for bytes in [
        "towavue-update-state-v1\n../escape\nready\n".into(),
        format!("towavue-update-state-v1\n{stage}\nunknown\n"),
        format!("towavue-update-state-v1\r\n{stage}\r\nready\r\n"),
        format!("towavue-update-state-v1\n{stage}\nready"),
        format!("towavue-update-state-v1\n{stage}\nready\nextra\n"),
    ] {
        assert!(State::parse(bytes.as_bytes()).is_err());
    }
    assert!(Directories::lock(Path::new("relative"), true).is_err());
    assert!(Directories::lock(Path::new(r"C:\..\escape"), true).is_err());
    assert!(Directories::lock(Path::new(r"\\server\share\cache"), true).is_err());
}

#[test]
fn startup_preserves_next_launch_and_does_not_repeat_an_interrupted_install() {
    let fixture = Fixture::new();
    let current = ReleaseVersion(1, 0, 0);
    assert!(matches!(
        fixture.store.startup(current).expect("inspect startup"),
        StartupUpdate::None
    ));
    let mut cached = fixture.stage(1);
    fixture
        .store
        .transition(&mut cached, UpdatePhase::NextLaunch)
        .expect("owned fixture");
    drop(cached);
    let StartupUpdate::Cached(mut cached) =
        fixture.store.startup(current).expect("inspect startup")
    else {
        panic!("scheduled");
    };
    assert_eq!(cached.phase(), UpdatePhase::NextLaunch);
    fixture
        .store
        .transition(&mut cached, UpdatePhase::Installing)
        .expect("owned fixture");
    drop(cached);
    let helper = OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(fixture.store.root.join("handoff.lock"))
        .expect("owned fixture");
    assert!(matches!(
        fixture.store.startup(current).expect("inspect startup"),
        StartupUpdate::Installing
    ));
    drop(helper);
    let StartupUpdate::Cached(cached) = fixture.store.startup(current).expect("inspect startup")
    else {
        panic!("failure");
    };
    assert_eq!(cached.phase(), UpdatePhase::Failed);
    drop(cached);
    let StartupUpdate::Cached(cached) = fixture.store.startup(current).expect("inspect startup")
    else {
        panic!("retained failure");
    };
    assert_eq!(cached.phase(), UpdatePhase::Failed);
    let directory = cached.directory.clone();
    fs::write(directory.join("user-file.txt"), b"preserve").expect("owned fixture");
    drop(cached);
    assert!(matches!(
        fixture
            .store
            .startup(ReleaseVersion(1, 0, 1))
            .expect("owned fixture"),
        StartupUpdate::None
    ));
    assert!(!fixture.store.root.join(STATE).exists());
    assert!(!directory.join(SETUP).exists());
    assert_eq!(
        fs::read(directory.join("user-file.txt")).expect("owned fixture"),
        b"preserve"
    );
}

#[test]
fn runtime_handoff_refuses_cancellation_and_a_parent_outside_the_registered_application() {
    let fixture = Fixture::new();
    let cached = fixture.stage(1);
    let installation = std::env::current_exe()
        .expect("owned fixture")
        .parent()
        .expect("owned fixture")
        .to_owned();
    let cancelled = Cancellation::default();
    cancelled.cancel();
    let result = fixture
        .store
        .start_handoff(cached, &installation, None, &cancelled);
    assert!(matches!(result, Err(error) if error.kind() == io::ErrorKind::Interrupted));
    let cached = fixture
        .store
        .load()
        .expect("load cache")
        .expect("cached update");
    assert_eq!(cached.phase(), UpdatePhase::Ready);
    assert_eq!(
        fs::read_dir(cached.directory())
            .expect("stage entries")
            .count(),
        3
    );
    let result = fixture
        .store
        .start_handoff(cached, &installation, None, &Cancellation::default());
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("A test runner must not become an update parent"),
    };
    assert!(
        error.to_string().contains("parent process identity"),
        "{error}"
    );
    assert_eq!(
        fixture
            .store
            .load()
            .expect("load cache")
            .expect("cached update")
            .phase(),
        UpdatePhase::Failed
    );
}
