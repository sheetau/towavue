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

#[test]
fn empty_startup_surface_paints_black_before_the_first_frame() {
    use windows::core::w;
    // SAFETY: every native resource is owned by this test on this thread. The
    // window stays hidden; the memory DC never targets the user's desktop.
    unsafe {
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!(""),
            WS_POPUP,
            0,
            0,
            8,
            8,
            None,
            None,
            None,
            None,
        )
        .expect("hidden window");
        let dc = CreateCompatibleDC(None);
        assert!(!dc.is_invalid());
        let mut info = BITMAPINFO::default();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = 8;
        info.bmiHeader.biHeight = -8;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        let mut bits = std::ptr::null_mut();
        let bitmap =
            CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).expect("DIB");
        let old = SelectObject(dc, bitmap.into());
        let bounds = RECT {
            left: 0,
            top: 0,
            right: 8,
            bottom: 8,
        };
        FillRect(dc, &bounds, HBRUSH(GetStockObject(WHITE_BRUSH).0));
        let state = PaintState::default();
        state.paint(dc, hwnd);
        let empty = GetPixel(dc, 4, 4);
        state.frame.replace(Some(Frame {
            bgra: [0, 0, 255, 255].repeat(64),
            width: 8,
            height: 8,
        }));
        state.paint(dc, hwnd);
        let ready = GetPixel(dc, 4, 4);
        let hidden = !IsWindowVisible(hwnd).as_bool();
        SelectObject(dc, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
        DestroyWindow(hwnd).expect("retire hidden window");
        assert!(hidden);
        assert_eq!(empty.0, 0, "uninitialized surface is black");
        assert_eq!(ready.0, 255, "complete frame replaces the black background");
    }
}
