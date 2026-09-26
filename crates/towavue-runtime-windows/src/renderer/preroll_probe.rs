//! Explicit diagnostic pacing on the existing, multithread-protected device.
use super::GraphicsDevice;
use std::time::{Duration, Instant};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_ASYNC_GETDATA_DONOTFLUSH, D3D11_QUERY_DESC, D3D11_QUERY_EVENT, ID3D11DeviceContext,
    ID3D11Query,
};
use windows::core::BOOL;

pub(crate) struct PrerollProbe {
    context: ID3D11DeviceContext,
    query: ID3D11Query,
}

impl PrerollProbe {
    pub(crate) fn new(device: &GraphicsDevice) -> Self {
        // The owned context/query refer to the same protected device used by playback.
        // This helper stays on one decoder worker; no raw native handle escapes it.
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
                .expect("preroll probe query creation");
            Self {
                context: device
                    .device
                    .GetImmediateContext()
                    .expect("preroll probe context"),
                query: query.expect("preroll probe query"),
            }
        }
    }

    pub(crate) fn wait(&self, cancelled: &dyn Fn() -> bool) -> bool {
        // End submits an event after existing work. Flush once so DONOTFLUSH polling
        // cannot leave that event in an unsubmitted command buffer. Never hold an
        // application/FFmpeg device lock while polling or sleeping; each native call
        // alone uses the immediate context's existing multithread protection.
        unsafe {
            self.context.End(&self.query);
            self.context.Flush();
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if cancelled() {
                return false;
            }
            let mut complete = BOOL(0);
            // GetData only borrows this stack BOOL for the duration of the call.
            // S_FALSE is not an error, so inspect the result value as well.
            unsafe {
                self.context
                    .GetData(
                        &self.query,
                        Some((&mut complete as *mut BOOL).cast()),
                        size_of::<BOOL>() as u32,
                        D3D11_ASYNC_GETDATA_DONOTFLUSH.0 as u32,
                    )
                    .expect("preroll probe completion query");
            }
            if complete.as_bool() {
                return true;
            }
            assert!(
                Instant::now() < deadline,
                "preroll probe completion timeout"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
