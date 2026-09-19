use super::*;
use std::borrow::Cow;
use std::fs;
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn raw() -> arboard::ImageData<'static> {
    arboard::ImageData {
        width: 2,
        height: 2,
        bytes: Cow::Owned(vec![
            255, 123, 12, 0, 200, 99, 3, 1, 17, 55, 33, 127, 1, 2, 3, 255,
        ]),
    }
}

fn wait_removed(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while path.exists() {
        assert!(Instant::now() < deadline, "cleanup {}", path.display());
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn pasted_pixels_keep_transparency_and_original_lifetime_through_edit_export() {
    let raw = raw();
    let bytes = raw.bytes.as_ref().to_vec();
    let pasted = prepare(raw, &AtomicBool::new(false)).expect("prepare image");
    assert_eq!(pasted.image().frames[0].rgba, bytes);
    let original = pasted.original().clone();
    let path = original.original_path().to_owned();
    let directory = path.parent().expect("owned directory").to_owned();
    assert_eq!(
        crate::decode_image(&path).expect("lossless PNG").frames[0].rgba,
        bytes
    );
    let another_window = pasted.clone();
    drop(pasted);
    assert!(fs::OpenOptions::new().write(true).open(&path).is_err());
    assert!(fs::rename(&path, directory.join("stolen.png")).is_err());
    let target = directory.join("edited.png");
    crate::export_media_with_options(
        &crate::ExportRequest {
            source: path.clone(),
            target: target.clone(),
            kind: towavue_core::MediaKind::Image,
            operations: vec![towavue_core::EditOperation::FlipHorizontal],
            hardware_encode: false,
        },
        crate::ExportOptions::default(),
    )
    .expect("edited PNG export");
    assert_eq!(
        crate::decode_image(&target).expect("edited output").frames[0].rgba,
        [&bytes[4..8], &bytes[0..4], &bytes[12..16], &bytes[8..12]].concat()
    );
    fs::remove_file(target).expect("remove owned export");
    original
        .original_source()
        .verify()
        .expect("immutable source");
    drop(original);
    assert_eq!(another_window.image().frames[0].rgba, bytes);
    assert!(path.is_file());
    drop(another_window);
    wait_removed(&directory);
}

#[test]
fn paste_rejects_empty_malformed_oversized_and_cancelled_input() {
    let cancel = AtomicBool::new(false);
    for (width, height, bytes) in [
        (0, 0, vec![]),
        (1, 1, vec![]),
        (1, 1, vec![0; 5]),
        (usize::MAX, usize::MAX, vec![]),
        (u32::MAX as usize, 2, vec![]),
        (16384, 16384, vec![]),
    ] {
        assert!(
            prepare(
                arboard::ImageData {
                    width,
                    height,
                    bytes: Cow::Owned(bytes),
                },
                &cancel,
            )
            .is_err()
        );
    }
    cancel.store(true, Ordering::Relaxed);
    assert_eq!(
        prepare(raw(), &cancel).expect_err("cancelled"),
        "Image paste cancelled"
    );
}

#[test]
fn paste_worker_delivers_owned_result_once_and_reports_reader_failure() {
    let (send, receive) = mpsc::channel();
    let job = ImagePasteJob::start_with_reader(
        || Ok(raw()),
        move |result| {
            send.send(result).expect("receiver");
        },
    )
    .expect("worker");
    let pasted = receive
        .recv_timeout(Duration::from_secs(5))
        .expect("notification")
        .expect("pasted image");
    drop(job);
    assert!(matches!(
        receive.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
    let directory = pasted
        .original()
        .original_path()
        .parent()
        .expect("directory")
        .to_owned();
    assert_eq!(pasted.image().dimensions(), (2, 2));
    assert!(pasted.original().original_path().is_file());
    drop(pasted);
    wait_removed(&directory);

    let (send, receive) = mpsc::channel();
    let job = ImagePasteJob::start_with_reader(
        || Err("No image available".into()),
        move |result| {
            send.send(result).expect("receiver");
        },
    )
    .expect("worker");
    assert_eq!(
        receive
            .recv_timeout(Duration::from_secs(5))
            .expect("notification")
            .expect_err("failure"),
        "No image available"
    );
    drop(job);
    assert!(matches!(
        receive.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
}

#[test]
fn paste_cancelled_during_clipboard_read_never_delivers_an_image() {
    let (started, ready) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let (send, receive) = mpsc::channel();
    let job = ImagePasteJob::start_with_reader(
        move || {
            started.send(()).expect("reader entered");
            blocked
                .recv_timeout(Duration::from_secs(5))
                .expect("release reader");
            Ok(raw())
        },
        move |result| {
            send.send(result).expect("receiver");
        },
    )
    .expect("worker");
    ready
        .recv_timeout(Duration::from_secs(5))
        .expect("reader entry");
    job.cancel();
    release.send(()).expect("release");
    assert_eq!(
        receive
            .recv_timeout(Duration::from_secs(5))
            .expect("notification")
            .expect_err("cancelled"),
        "Image paste cancelled"
    );
    drop(job);
    assert!(matches!(
        receive.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
}
