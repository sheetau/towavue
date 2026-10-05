//! Optional completion pacing during seek preroll on the shared D3D11 device.
use super::GraphicsDevice;
use std::time::{Duration, Instant};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_ASYNC_GETDATA_DONOTFLUSH, D3D11_FENCE_FLAG_NONE, D3D11_QUERY_DESC, D3D11_QUERY_EVENT,
    ID3D11Device5, ID3D11DeviceContext, ID3D11DeviceContext4, ID3D11Fence, ID3D11Query,
};
use windows::core::{BOOL, Interface};

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

#[cfg(test)]
pub(crate) static FORCE_QUERY: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
pub(crate) static FENCE_SUBMITTED: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

enum Marker {
    Query(ID3D11Query),
    Fence {
        context: ID3D11DeviceContext4,
        fence: ID3D11Fence,
        timeline: FenceTimeline,
    },
}

pub(crate) struct PrerollPacing {
    context: ID3D11DeviceContext,
    marker: Marker,
    packet_interval: u64,
}

// A window of two four-packet groups keeps at most eight packets ahead.
// Retirement advances only after the required older marker actually completes.
struct FenceTimeline {
    submitted: u64,
    retired: u64,
    window: u64,
}

impl FenceTimeline {
    fn next_marker(&self) -> Option<(u64, u64)> {
        let next = self.submitted.checked_add(1)?;
        if next == u64::MAX || next.checked_sub(self.retired)? > self.window {
            return None;
        }
        Some((next, next.saturating_sub(self.window - 1)))
    }
}

fn fence_ready(completed: u64, target: u64) -> Result<bool, ()> {
    if completed == u64::MAX {
        Err(())
    } else {
        Ok(completed >= target)
    }
}

impl PrerollPacing {
    pub(crate) fn new(device: &GraphicsDevice, batch_limit: u64) -> Option<Self> {
        // All interfaces belong to the existing protected device and stay on
        // one decoder worker. No event HANDLE or borrowed CPU address is registered.
        let context = unsafe { device.device.GetImmediateContext().ok()? };
        let use_fence = true;
        #[cfg(test)]
        let use_fence = use_fence && !FORCE_QUERY.load(std::sync::atomic::Ordering::Relaxed);
        #[cfg(feature = "presentation-verification")]
        let use_fence =
            use_fence && std::env::var("TOWAVUE_VERIFY_PREROLL_QUERY").as_deref() != Ok("1");
        if use_fence {
            let native = (|| {
                let device5 = device.device.cast::<ID3D11Device5>().ok()?;
                let context4 = context.cast::<ID3D11DeviceContext4>().ok()?;
                let mut fence = None;
                unsafe {
                    device5
                        .CreateFence(0, D3D11_FENCE_FLAG_NONE, &mut fence)
                        .ok()?;
                }
                Some(Marker::Fence {
                    context: context4,
                    fence: fence?,
                    timeline: FenceTimeline {
                        submitted: 0,
                        retired: 0,
                        window: if batch_limit == 8 { 2 } else { 1 },
                    },
                })
            })();
            if let Some(marker) = native {
                return Some(Self {
                    context,
                    marker,
                    packet_interval: if batch_limit == 8 { 4 } else { batch_limit },
                });
            }
        }
        // Older interfaces or unsupported fence creation retain EVENT queries.
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
                context,
                marker: Marker::Query(query?),
                packet_interval: batch_limit,
            })
        }
    }

    pub(crate) fn packet_interval(&self) -> u64 {
        self.packet_interval
    }

    pub(crate) fn wait(&mut self, cancelled: &dyn Fn() -> bool) -> Completion {
        if cancelled() {
            return Completion::Cancelled;
        }
        let deadline = Instant::now() + Duration::from_millis(100);
        // Each marker follows preceding decode work on the same immediate
        // context. Flush once; existing multithread protection serializes native
        // calls, and no application/FFmpeg lock spans polling or sleeping.
        // The deadline bounds polling, not a driver's native call. The caller
        // retires this helper after cancellation, failure or an incomplete wait.
        match &mut self.marker {
            Marker::Query(query) => {
                unsafe {
                    self.context.End(&*query);
                    self.context.Flush();
                }
                #[cfg(test)]
                SUBMITTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                poll_until(deadline, cancelled, || {
                    let mut complete = BOOL(0);
                    // GetData borrows the BOOL synchronously. S_FALSE keeps it
                    // false and must not be interpreted as marker completion.
                    unsafe {
                        self.context
                            .GetData(
                                &*query,
                                Some((&mut complete as *mut BOOL).cast()),
                                size_of::<BOOL>() as u32,
                                D3D11_ASYNC_GETDATA_DONOTFLUSH.0 as u32,
                            )
                            .map_err(|_| ())?;
                    }
                    Ok(complete.as_bool())
                })
            }
            Marker::Fence {
                context,
                fence,
                timeline,
            } => {
                // Never wrap or submit the device-removal sentinel. Each
                // helper exclusively owns its monotonic fence timeline.
                let Some((next, target)) = timeline.next_marker() else {
                    return Completion::Unavailable;
                };
                unsafe {
                    if context.Signal(&*fence, next).is_err() {
                        return Completion::Unavailable;
                    }
                    self.context.Flush();
                }
                timeline.submitted = next;
                #[cfg(test)]
                {
                    SUBMITTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    FENCE_SUBMITTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                let result = poll_until(deadline, cancelled, || {
                    // Read only the owned fence; no immediate-context query or
                    // event registration is needed. Removed devices cannot pass.
                    fence_ready(unsafe { fence.GetCompletedValue() }, target)
                });
                if result == Completion::Ready {
                    timeline.retired = target;
                }
                result
            }
        }
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
    fn incomplete_markers_bound_submission_and_never_wrap() {
        let mut timeline = FenceTimeline {
            submitted: 0,
            retired: 0,
            window: 2,
        };
        for completed_group in 0..100 {
            let (next, target) = timeline.next_marker().expect("space after retirement");
            assert!(
                (next - timeline.retired) * 4 <= 8,
                "maximum GPU packet lead"
            );
            assert_eq!(target, completed_group);
            timeline.submitted = next;
            if next > 1 {
                assert!(
                    timeline.next_marker().is_none(),
                    "uncompleted older marker must bound submission"
                );
            }
            timeline.retired = target;
        }
        timeline.submitted = u64::MAX - 1;
        timeline.retired = u64::MAX - 2;
        assert!(timeline.next_marker().is_none(), "reserved removal value");
        timeline.submitted = u64::MAX;
        assert!(timeline.next_marker().is_none(), "overflow");
        let fallback = FenceTimeline {
            submitted: 1,
            retired: 0,
            window: 1,
        };
        assert!(
            fallback.next_marker().is_none(),
            "nonoverlapped policy waits for every marker"
        );
    }

    #[test]
    fn removed_fence_does_not_report_completion() {
        assert_eq!(fence_ready(u64::MAX, 1), Err(()));
        assert_eq!(fence_ready(0, 1), Ok(false));
        assert_eq!(fence_ready(1, 1), Ok(true));
        assert_eq!(fence_ready(2, 1), Ok(true));
    }

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
