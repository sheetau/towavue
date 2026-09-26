//! Optional completion pacing during seek preroll on the shared D3D11 device.
use super::GraphicsDevice;
use std::time::{Duration, Instant};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_ASYNC_GETDATA_DONOTFLUSH, D3D11_QUERY_DESC, D3D11_QUERY_EVENT, ID3D11DeviceContext,
    ID3D11Query,
};
use windows::core::BOOL;

#[cfg(test)]
pub(crate) static BATCH_OVERRIDE: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(8);
#[cfg(test)]
pub(crate) static SUBMITTED: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

pub(crate) fn batch_size() -> u64 {
    #[cfg(feature = "presentation-verification")]
    if let Ok(value) = std::env::var("TOWAVUE_PREROLL_BATCH") {
        return value.parse().expect("preroll batch integer");
    }
    #[cfg(test)]
    return BATCH_OVERRIDE.load(std::sync::atomic::Ordering::Relaxed);
    #[cfg(not(test))]
    8
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Completion {
    Ready,
    Cancelled,
    Unavailable,
}

pub(crate) struct PrerollPacing {
    context: ID3D11DeviceContext,
    query: ID3D11Query,
}

impl PrerollPacing {
    pub(crate) fn new(device: &GraphicsDevice) -> Option<Self> {
        // Both owned interfaces belong to the existing protected device. This
        // helper stays on one decoder worker. Query support is an optimization,
        // not a prerequisite for decoding or for the normal device-loss path.
        unsafe {
            let mut query = None;
            device
                .device
                .CreateQuery(
                    &D3D11_QUERY_DESC {
                        Query: D3D11_QUERY_EVENT,
                        MiscFlags: 0,
                    },
                    Some(&mut query),
                )
                .ok()?;
            Some(Self {
                context: device.device.GetImmediateContext().ok()?,
                query: query?,
            })
        }
    }

    pub(crate) fn wait(&self, cancelled: &dyn Fn() -> bool) -> Completion {
        if cancelled() {
            return Completion::Cancelled;
        }
        let deadline = Instant::now() + Duration::from_millis(100);
        // End marks preceding work. Flush once before DONOTFLUSH polling so the
        // marker cannot remain unsubmitted. Each native call uses the context's
        // existing multithread protection; no application/FFmpeg lock spans a
        // poll or sleep. The deadline bounds polling, not a driver's native call.
        unsafe {
            self.context.End(&self.query);
            self.context.Flush();
        }
        #[cfg(test)]
        SUBMITTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        poll_until(deadline, cancelled, || {
            let mut complete = BOOL(0);
            // The query borrows this BOOL only during GetData. S_FALSE is not an
            // error: its false value must keep waiting rather than reuse the event.
            unsafe {
                self.context
                    .GetData(
                        &self.query,
                        Some((&mut complete as *mut BOOL).cast()),
                        size_of::<BOOL>() as u32,
                        D3D11_ASYNC_GETDATA_DONOTFLUSH.0 as u32,
                    )
                    .map_err(|_| ())?;
            }
            Ok(complete.as_bool())
        })
    }
}

fn poll_until(
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
    mut poll: impl FnMut() -> Result<bool, ()>,
) -> Completion {
    loop {
        if cancelled() {
            return Completion::Cancelled;
        }
        match poll() {
            Ok(true) => return Completion::Ready,
            Err(()) => return Completion::Unavailable,
            Ok(false) if Instant::now() >= deadline => return Completion::Unavailable,
            Ok(false) => std::thread::sleep(Duration::from_millis(1)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn pending_query_cancels_before_a_second_poll_and_errors_do_not_complete() {
        let polls = Cell::new(0);
        let result = poll_until(
            Instant::now() + Duration::from_secs(1),
            &|| polls.get() > 0,
            || {
                polls.set(polls.get() + 1);
                Ok(false)
            },
        );
        assert_eq!(result, Completion::Cancelled);
        assert_eq!(polls.get(), 1);
        assert_eq!(
            poll_until(Instant::now(), &|| false, || Err(())),
            Completion::Unavailable
        );
        assert_eq!(
            poll_until(Instant::now(), &|| false, || Ok(false)),
            Completion::Unavailable
        );
        assert_eq!(
            poll_until(Instant::now(), &|| false, || Ok(true)),
            Completion::Ready
        );
        assert_eq!(
            poll_until(Instant::now(), &|| true, || panic!(
                "cancelled query polled"
            )),
            Completion::Cancelled
        );
    }
}
