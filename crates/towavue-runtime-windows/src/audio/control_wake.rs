//! Wake the existing auto-reset WASAPI event after publishing a control command.
use std::os::windows::io::{AsRawHandle, BorrowedHandle, OwnedHandle};
use std::sync::OnceLock;
use windows::Win32::{Foundation::HANDLE, System::Threading::SetEvent};

#[derive(Default)]
pub(super) struct ControlWake {
    event: OnceLock<OwnedHandle>,
}

impl ControlWake {
    pub(super) fn install(&self, event: BorrowedHandle<'_>) -> std::io::Result<()> {
        // Independent ownership keeps control signals valid even after the
        // native worker closes its original handle. OnceLock publishes once;
        // no lock spans a wait, a signal, or joining the worker.
        let duplicate = event.try_clone_to_owned()?;
        let _ = self.event.set(duplicate);
        Ok(())
    }

    pub(super) fn signal(&self) {
        if let Some(event) = self.event.get() {
            // SAFETY: this owned duplicate stays live through SetEvent and is
            // never replaced. The PCM/control channels publish data; this
            // auto-reset event only wakes the sole native audio consumer.
            // If signaling fails, its existing finite wait still reads control.
            let _ = unsafe { SetEvent(HANDLE(event.as_raw_handle())) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::*;
    use std::os::windows::io::{AsHandle, FromRawHandle};
    use windows::Win32::{
        Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
        System::Threading::{CreateEventW, WaitForSingleObject},
    };

    fn event() -> OwnedHandle {
        // SAFETY: create one unnamed auto-reset event with exclusive ownership;
        // transfer its successful non-null handle exactly once to OwnedHandle.
        unsafe {
            let event = CreateEventW(None, false, false, None).expect("event");
            OwnedHandle::from_raw_handle(event.0)
        }
    }

    fn wait(event: &OwnedHandle, milliseconds: u32) -> windows::Win32::Foundation::WAIT_EVENT {
        // SAFETY: the owned event remains live for the complete synchronous wait.
        unsafe { WaitForSingleObject(HANDLE(event.as_raw_handle()), milliseconds) }
    }

    #[test]
    fn wake_survives_original_close_and_keeps_auto_reset_semantics() {
        let wake = ControlWake::default();
        wake.signal(); // A command during initialization has no native wait yet.
        let original = event();
        let waiter = original.try_clone().expect("independent waiter");
        wake.install(original.as_handle()).expect("install");
        drop(original);
        assert_eq!(wait(&waiter, 0), WAIT_TIMEOUT);
        wake.signal();
        assert_eq!(wait(&waiter, 0), WAIT_OBJECT_0);
        assert_eq!(wait(&waiter, 0), WAIT_TIMEOUT);
        drop(waiter);
        wake.signal(); // Only the control duplicate remains after worker exit.
    }

    #[test]
    fn pause_and_shutdown_interrupt_native_wait_with_commands_already_published() {
        for before_install in [false, true] {
            let wake = Arc::new(ControlWake::default());
            let (control_tx, control_rx) = mpsc::channel();
            let (audio_tx, _audio_rx) = mpsc::sync_channel(1);
            let (_event_tx, event_rx) = mpsc::channel();
            let (entered_tx, entered_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let (paused_tx, paused_rx) = mpsc::channel();
            let thread_wake = Arc::clone(&wake);
            let thread = thread::spawn(move || {
                let original = event();
                if !before_install {
                    thread_wake.install(original.as_handle()).expect("install");
                }
                entered_tx.send(()).expect("entered");
                release_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("release");
                if before_install {
                    thread_wake
                        .install(original.as_handle())
                        .expect("late install");
                } else {
                    assert_eq!(wait(&original, 5_000), WAIT_OBJECT_0);
                }
                assert!(matches!(
                    control_rx.try_recv(),
                    Ok(AudioControl::SetPaused(true))
                ));
                paused_tx.send(()).expect("pause consumed");
                assert_eq!(wait(&original, 5_000), WAIT_OBJECT_0);
                assert!(matches!(control_rx.try_recv(), Ok(AudioControl::Shutdown)));
            });
            let output = AudioOutput {
                format: AudioFormat {
                    sample_rate: 48_000,
                    channels: 2,
                },
                audio_tx,
                control_tx,
                control_wake: wake,
                initial_audio_queued: Arc::new(AtomicBool::new(false)),
                event_rx,
                position_nanoseconds: Arc::new(AtomicI64::new(0)),
                volume: Arc::new(AtomicU32::new(0)),
                drained: Arc::new(AtomicBool::new(false)),
                thread: Some(thread),
            };
            entered_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("worker entered");
            output.set_paused(true).expect("publish pause");
            release_tx.send(()).expect("release worker");
            paused_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("pause woke worker");
            // Drop joins the worker, which cannot finish without a shutdown wake.
            // Preserve its result because AudioOutput intentionally ignores panics.
            let mut output = output;
            let thread = output.thread.take().expect("worker");
            drop(output);
            thread.join().expect("shutdown consumed after native wake");
        }
    }
}
