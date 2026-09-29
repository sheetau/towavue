//! Opt-in, read-only Shell folder-navigation latency probe. No media is decoded.
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use towavue_core::FolderNavigation;
use towavue_runtime_windows::FolderOrderProvider;

fn main() {
    let origin = PathBuf::from(
        std::env::var_os("TOWAVUE_FOLDER_REFERENCE_ORIGIN").expect("explicit reference folder"),
    );
    assert!(
        origin.is_absolute() && origin.is_dir(),
        "existing absolute folder"
    );
    let direction = match std::env::args().nth(1).as_deref() {
        Some("parent") => Some(FolderNavigation::Parent),
        Some("next") => Some(FolderNavigation::Next),
        Some("previous") => Some(FolderNavigation::Previous),
        Some("child") => Some(FolderNavigation::FirstChild),
        Some("open") => None,
        _ => panic!("expected open, parent, next, previous or child"),
    };
    let (send, receive) = mpsc::channel();
    let provider = FolderOrderProvider::with_notify(move || {
        let _ = send.send(());
    })
    .expect("Shell worker");
    let started = Instant::now();
    let mut generation = match direction {
        Some(direction) => provider.request_navigation(origin, direction),
        None => provider.request(Some(origin)),
    };
    let mut discovery_ms = None;
    let (folder, found) = loop {
        if let Some(result) = provider.take_navigation() {
            assert_eq!(result.generation, generation, "current destination");
            discovery_ms = Some(started.elapsed().as_secs_f64() * 1000.0);
            if let Some(target) = result.target {
                // The app passes the selected path to its ordinary Open folder route.
                generation = provider.request(Some(target));
                continue;
            }
            break (result.searched_folder, false);
        }
        if let Some(result) = provider.take_completed() {
            assert_eq!(result.generation, generation, "current listing");
            break (result.folder_path, !result.items.is_empty());
        }
        receive
            .recv_timeout(Duration::from_secs(120))
            .expect("folder delivery");
    };
    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
    let mut digest = std::hash::DefaultHasher::new();
    folder.hash(&mut digest);
    println!(
        "direction={direction:?} ms={elapsed:.3} discovery_ms={discovery_ms:?} found={found} folder={:016x}",
        digest.finish()
    );
}
