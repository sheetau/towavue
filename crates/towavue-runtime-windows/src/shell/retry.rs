use super::*;

const ATTEMPTS: usize = 3;
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg(test)]
thread_local! {
    pub(super) static FAIL_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(super) fn fail_read() -> bool {
    FAIL_READS.with(|remaining| {
        let count = remaining.get();
        remaining.set(count.saturating_sub(1));
        count != 0
    })
}

/// Retry only failed reads. A ready native view returns without any settling delay.
/// Each attempt owns and drops its COM objects before the next one starts. The
/// caller must check both its generation and the supplied deadline during waits.
pub(super) fn read<T>(
    current: &impl Fn() -> bool,
    mut attempt: impl FnMut(Instant) -> Option<T>,
) -> Option<T> {
    for index in 0..ATTEMPTS {
        if !current() {
            return None;
        }
        if let Some(value) = attempt(Instant::now() + ATTEMPT_TIMEOUT) {
            return current().then_some(value);
        }
        if !current() {
            return None;
        }
        crate::diagnostic!(
            "towavue: Shell order read failed (attempt {}/{ATTEMPTS})",
            index + 1
        );
        if index + 1 < ATTEMPTS {
            let deadline = Instant::now() + Duration::from_millis(50 * (index as u64 + 1));
            while current() && Instant::now() < deadline {
                // SAFETY: this helper runs on the owning STA, without mailbox
                // locks. Dispatch COM messages during backoff and bound the wait
                // so cancellation/drop does not wait for the backoff to expire.
                unsafe {
                    pump_messages();
                    MsgWaitForMultipleObjectsEx(None, 10, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn transient_failures_retry_but_success_and_cancellation_stop_immediately() {
        let calls = Cell::new(0);
        let result = read(&|| true, |_| {
            calls.set(calls.get() + 1);
            (calls.get() == 3).then_some("native order")
        });
        assert_eq!(result, Some("native order"));
        assert_eq!(calls.get(), 3);
        calls.set(0);
        assert_eq!(
            read(&|| true, |_| {
                calls.set(calls.get() + 1);
                Some(7)
            }),
            Some(7)
        );
        assert_eq!(calls.get(), 1);
        let current = Cell::new(true);
        calls.set(0);
        assert!(
            read(&|| current.get(), |_| {
                calls.set(calls.get() + 1);
                current.set(false);
                None::<()>
            })
            .is_none()
        );
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn persistent_failures_end_without_manufacturing_an_empty_or_name_ordered_result() {
        let calls = Cell::new(0);
        assert!(
            read(&|| true, |_| {
                calls.set(calls.get() + 1);
                None::<FolderSnapshot>
            })
            .is_none()
        );
        assert_eq!(calls.get(), ATTEMPTS);
        assert!(
            read(&|| false, |_| -> Option<()> {
                panic!("cancelled request must not start")
            })
            .is_none()
        );
    }
}
