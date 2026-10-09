use super::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn failed_worker_creation_is_immediately_available_to_every_read() {
    let (_send, receive) = mpsc::sync_channel(1);
    let job = StartupDevice::from_spawn(
        receive,
        Err(std::io::Error::other("injected thread creation failure")),
    );
    for wait in [false, true, false] {
        let error = job
            .get(wait)
            .expect("terminal job")
            .err()
            .expect("failed creation");
        assert!(
            error
                .to_string()
                .contains("injected thread creation failure")
        );
    }
    assert!(job.inner.lock().expect("job state").thread.is_none());
}

#[test]
fn device_result_is_cached_only_after_the_worker_is_joined() {
    let (send, receive) = mpsc::sync_channel(1);
    let completed = Arc::new(AtomicBool::new(false));
    let worker_completed = completed.clone();
    let thread = std::thread::spawn(move || {
        assert!(send.send(Err(RenderError::SurfaceNotSized)).is_ok());
        worker_completed.store(true, Ordering::Release);
    });
    let job = StartupDevice::from_spawn(receive, Ok(thread));
    assert!(matches!(
        job.get(true),
        Some(Err(RenderError::SurfaceNotSized))
    ));
    assert!(completed.load(Ordering::Acquire));
    assert!(job.inner.lock().expect("job state").thread.is_none());
    assert!(matches!(
        job.get(false),
        Some(Err(RenderError::SurfaceNotSized))
    ));
}

#[test]
fn disconnected_worker_becomes_a_stable_failure() {
    let (send, receive) = mpsc::sync_channel(1);
    let thread = std::thread::spawn(move || drop(send));
    let job = StartupDevice::from_spawn(receive, Ok(thread));
    for wait in [true, false] {
        let error = job
            .get(wait)
            .expect("terminal job")
            .err()
            .expect("disconnected worker");
        assert!(
            error
                .to_string()
                .contains("Startup device worker disconnected")
        );
    }
    assert!(job.inner.lock().expect("job state").thread.is_none());
}

#[test]
fn cpu_frame_size_rejects_overflow_before_allocating_or_casting() {
    assert_eq!(
        cpu::frame_byte_len([800, 650]).expect("client size"),
        2_080_000
    );
    assert_eq!(cpu::frame_byte_len([0, 650]).expect("minimized width"), 0);
    for size in [
        [usize::MAX, 1],
        [i32::MAX as usize + 1, 1],
        [i32::MAX as usize; 2],
    ] {
        assert!(cpu::frame_byte_len(size).is_err());
    }
}
