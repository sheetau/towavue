//! Owned CPU presentation and the handoff to the shared D3D11 device.
use super::{FrameRenderer, GraphicsDevice, RenderError};
use crate::caption::CaptionSurface;
use std::{
    cell::RefCell,
    sync::{Mutex, mpsc},
    thread::JoinHandle,
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    Graphics::Gdi::*,
    UI::{
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};
mod cpu;
#[cfg(test)]
mod tests;
const PAINT_SUBCLASS: usize = 0x745643505550524f;

/// One host-owned creation job. Safe handles can be shared before completion.
/// No HWND or immediate-context drawing is used on this owned worker.
pub struct StartupDevice {
    inner: Mutex<DeviceState>,
    #[cfg(feature = "render-verification")]
    gate: Mutex<Option<mpsc::Sender<bool>>>,
}
struct DeviceState {
    ready: mpsc::Receiver<Result<GraphicsDevice, RenderError>>,
    result: Option<Result<GraphicsDevice, RenderError>>,
    thread: Option<JoinHandle<()>>,
}
impl StartupDevice {
    pub fn spawn(notify: impl Fn() + Send + 'static) -> Self {
        let (send, ready) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("startup-graphics".into())
            .spawn(move || {
                let result = GraphicsDevice::hardware_for_startup();
                if send.send(result).is_ok() {
                    notify();
                }
            });
        Self::from_spawn(ready, thread)
    }

    fn from_spawn(
        ready: mpsc::Receiver<Result<GraphicsDevice, RenderError>>,
        thread: std::io::Result<JoinHandle<()>>,
    ) -> Self {
        // The first window paint already checks this job. A failure to start the
        // thread is immediately available through the same explicit Retry path;
        // it must not panic or leave the UI waiting for a nonexistent worker.
        let (thread, result) = match thread {
            Ok(thread) => (Some(thread), None),
            Err(error) => (
                None,
                Some(Err(RenderError::D3d11(windows::core::Error::new(
                    windows::Win32::Foundation::E_FAIL,
                    format!("Could not start graphics worker: {error}"),
                )))),
            ),
        };
        Self {
            inner: Mutex::new(DeviceState {
                ready,
                result,
                thread,
            }),
            #[cfg(feature = "render-verification")]
            gate: Mutex::new(None),
        }
    }

    #[cfg(feature = "render-verification")]
    pub fn controlled(notify: impl Fn() + Send + 'static) -> Self {
        Self::controlled_result(notify, false)
    }

    #[cfg(feature = "render-verification")]
    pub fn controlled_failure(notify: impl Fn() + Send + 'static) -> Self {
        Self::controlled_result(notify, true)
    }

    #[cfg(feature = "render-verification")]
    fn controlled_result(notify: impl Fn() + Send + 'static, fail: bool) -> Self {
        let (release, gate) = mpsc::channel();
        let (send, ready) = mpsc::sync_channel(1);
        let thread = std::thread::spawn(move || {
            if gate.recv() == Ok(true) {
                let result = if fail {
                    Err(RenderError::D3d11(windows::core::Error::new(
                        windows::Win32::Foundation::E_FAIL,
                        "Injected startup device creation failure",
                    )))
                } else {
                    GraphicsDevice::hardware_for_startup()
                };
                if send.send(result).is_ok() {
                    notify();
                }
            }
        });
        Self {
            inner: Mutex::new(DeviceState {
                ready,
                result: None,
                thread: Some(thread),
            }),
            gate: Mutex::new(Some(release)),
        }
    }

    #[cfg(feature = "render-verification")]
    pub fn release(&self) {
        if let Some(gate) = self.gate.lock().expect("startup gate").take() {
            let _ = gate.send(true);
        }
    }

    #[cfg(feature = "render-verification")]
    pub fn matches(&self, device: &GraphicsDevice) -> bool {
        self.get(false)
            .is_some_and(|result| result.is_ok_and(|own| own.same_device(device)))
    }

    pub fn get(&self, wait: bool) -> Option<Result<GraphicsDevice, RenderError>> {
        let mut state = self.inner.lock().expect("startup result lock");
        if state.result.is_none() {
            let received = if wait {
                state
                    .ready
                    .recv()
                    .map_err(|_| mpsc::TryRecvError::Disconnected)
            } else {
                state.ready.try_recv()
            };
            let result = match received {
                Ok(result) => result,
                Err(mpsc::TryRecvError::Empty) => return None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Err(RenderError::D3d11(windows::core::Error::new(
                        windows::Win32::Foundation::E_FAIL,
                        "Startup device worker disconnected",
                    )))
                }
            };
            if let Some(thread) = state.thread.take() {
                let _ = thread.join();
            }
            state.result = Some(result);
        }
        state.result.clone()
    }
}
impl Drop for StartupDevice {
    fn drop(&mut self) {
        // A running driver call cannot be interrupted. The last owner joins;
        // the bounded channel never blocks and the worker borrows no UI state.
        #[cfg(feature = "render-verification")]
        if let Some(gate) = self.gate.get_mut().expect("startup gate").take() {
            let _ = gate.send(false);
        }
        if let Some(thread) = self
            .inner
            .get_mut()
            .expect("startup result lock")
            .thread
            .take()
        {
            let _ = thread.join();
        }
    }
}

struct Frame {
    bgra: Vec<u8>,
    width: usize,
    height: usize,
}
#[derive(Default)]
struct PaintState {
    frame: RefCell<Option<Frame>>,
}

impl PaintState {
    fn paint(&self, dc: HDC) {
        // The callback executes only on the owner UI thread. A reentrant native
        // callback must not unwind across FFI if a frame update is in progress.
        let Ok(borrow) = self.frame.try_borrow() else {
            return;
        };
        let Some(frame) = borrow.as_ref() else {
            return;
        };
        let mut info = BITMAPINFO::default();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = frame.width as i32;
        info.bmiHeader.biHeight = -(frame.height as i32);
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB.0;
        // Frame bytes and description are borrowed only for this synchronous GDI
        // transfer. This HWND never owns a DXGI swap chain, including after handoff.
        unsafe {
            let _ = SetDIBitsToDevice(
                dc,
                0,
                0,
                frame.width as u32,
                frame.height as u32,
                0,
                0,
                0,
                frame.height as u32,
                frame.bgra.as_ptr().cast(),
                &info,
                DIB_RGB_COLORS,
            );
        }
    }
}

unsafe extern "system" fn paint_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    data: usize,
) -> LRESULT {
    // Box<PaintState> has a stable address, is UI-thread owned, and remains alive
    // until this exact subclass is removed before destroying the child HWND.
    let state = unsafe { &*(data as *const PaintState) };
    match message {
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            // Begin/EndPaint pair owns the borrowed DC only inside this callback.
            unsafe {
                let dc = BeginPaint(hwnd, &mut paint);
                if !dc.is_invalid() {
                    state.paint(dc);
                }
                let _ = EndPaint(hwnd, &paint);
            }
            LRESULT(0)
        }
        WM_PRINTCLIENT => {
            state.paint(HDC(wparam.0 as *mut _));
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        _ => unsafe { DefSubclassProc(hwnd, message, wparam, lparam) },
    }
}

/// UI-thread-owned CPU child and retained texture state until GPU publication.
/// The child retains its parent window and never receives a DXGI swap chain.
pub struct StartupSurface {
    surface: CaptionSurface,
    paint: Box<PaintState>,
    cpu: cpu::CpuUiRenderer,
    last: Option<egui::FullOutput>,
    size: [usize; 2],
}
impl StartupSurface {
    #[cfg(feature = "render-verification")]
    pub fn verification_rgba(&self) -> Vec<u8> {
        let mut pixels = self
            .paint
            .frame
            .borrow()
            .as_ref()
            .expect("CPU frame")
            .bgra
            .clone();
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
        pixels
    }
    #[cfg(feature = "render-verification")]
    pub fn verification_resize_hidden_parent(&self, size: [i32; 2]) -> Result<(), RenderError> {
        // SAFETY: CaptionSurface retains the parent owner on this same UI thread.
        // Scalar sizing parameters apply only to this test's hidden native parent.
        unsafe {
            let parent = GetParent(self.surface.handle)?;
            assert!(
                !IsWindowVisible(parent).as_bool(),
                "hidden test parent required"
            );
            assert!(size[0] >= 0 && size[1] >= 0);
            SetWindowPos(
                parent,
                None,
                0,
                0,
                size[0],
                size[1],
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            )?;
        }
        Ok(())
    }

    pub fn new(caption: &crate::NativeCaption, size: [usize; 2]) -> Result<Self, RenderError> {
        cpu::frame_byte_len(size).map_err(|_| RenderError::InvalidFrame)?;
        let surface = CaptionSurface::new(caption)?;
        surface.resize(size[0] as u32, size[1] as u32)?;
        let paint = Box::<PaintState>::default();
        // Stable Box, installed/removed on the parent UI thread; original caption
        // subclass remains responsible for HTTRANSPARENT and native frame routing.
        if !unsafe {
            SetWindowSubclass(
                surface.handle,
                Some(paint_proc),
                PAINT_SUBCLASS,
                paint.as_ref() as *const PaintState as usize,
            )
        }
        .as_bool()
        {
            return Err(windows::core::Error::from_thread().into());
        }
        Ok(Self {
            surface,
            paint,
            cpu: cpu::CpuUiRenderer::default(),
            last: None,
            size,
        })
    }
    pub fn resize(&mut self, size: [usize; 2]) -> Result<(), RenderError> {
        cpu::frame_byte_len(size).map_err(|_| RenderError::InvalidFrame)?;
        self.surface.resize(size[0] as u32, size[1] as u32)?;
        self.size = size;
        Ok(())
    }
    pub fn draw(
        &mut self,
        context: &egui::Context,
        output: egui::FullOutput,
    ) -> Result<egui::PlatformOutput, RenderError> {
        let mut bgra = self
            .cpu
            .render(context, &output, self.size)
            .map_err(|message| {
                RenderError::D3d11(windows::core::Error::new(
                    windows::Win32::Foundation::E_FAIL,
                    message,
                ))
            })?;
        for pixel in bgra.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
        self.paint.frame.replace(Some(Frame {
            bgra,
            width: self.size[0],
            height: self.size[1],
        }));
        let platform = output.platform_output.clone();
        self.last = Some(output);
        Ok(platform)
    }

    pub fn present(&self) -> Result<(), RenderError> {
        self.surface
            .refresh_clip(self.size[0] as u32, self.size[1] as u32)?;
        // Synchronously submit the cached complete frame on its owner thread.
        // The caller uses the same post-presentation completion boundary as GPU.
        unsafe {
            let _ = InvalidateRect(Some(self.surface.handle), None, false);
            let _ = UpdateWindow(self.surface.handle);
        }
        Ok(())
    }
    pub fn prepare_handoff(
        &mut self,
        caption: &crate::NativeCaption,
        context: &egui::Context,
        device: GraphicsDevice,
    ) -> Result<FrameRenderer, RenderError> {
        let gpu_surface = CaptionSurface::new_hidden(caption)?;
        let gpu_hwnd = gpu_surface.handle;
        let mut renderer = FrameRenderer::for_handle(gpu_hwnd, Some(gpu_surface), Some(device))?;
        // Explicit playback can need a device while the parent is minimized.
        // Keep the GPU child hidden and give its initial back buffer a valid size;
        // the ordinary resize path applies the restored client before publication.
        renderer.resize_surface(self.size[0].max(1) as u32, self.size[1].max(1) as u32)?;
        renderer.clear([0.0, 0.0, 0.0, 1.0])?;
        if let Some(last) = &self.last {
            let mut output = last.clone();
            // Snapshot precedes after-paint frees, including last-frame frees.
            output.textures_delta = self.cpu.replay();
            renderer.render_ui(context, output)?;
        }
        Ok(renderer)
    }

    pub fn publish(&self, renderer: &FrameRenderer) -> Result<(), RenderError> {
        let gpu_hwnd = renderer
            .caption_surface
            .as_ref()
            .expect("caption surface")
            .handle;
        // Both children retain the same native-caption region. Complete the native
        // visibility batch before retiring the GDI child; no HWND changes its API.
        unsafe {
            let batch = BeginDeferWindowPos(2)?;
            let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
            let batch = DeferWindowPos(
                batch,
                gpu_hwnd,
                Some(HWND_TOP),
                0,
                0,
                0,
                0,
                flags | SWP_SHOWWINDOW,
            )?;
            let batch = DeferWindowPos(
                batch,
                self.surface.handle,
                None,
                0,
                0,
                0,
                0,
                flags | SWP_NOZORDER | SWP_HIDEWINDOW,
            )?;
            if let Err(error) = EndDeferWindowPos(batch) {
                let _ = ShowWindow(gpu_hwnd, SW_HIDE);
                let _ = ShowWindow(self.surface.handle, SW_SHOWNOACTIVATE);
                return Err(error.into());
            }
        }
        Ok(())
    }
}
impl Drop for StartupSurface {
    fn drop(&mut self) {
        // Detach the callback before Box<PaintState> or CaptionSurface destruction.
        unsafe {
            let _ = RemoveWindowSubclass(self.surface.handle, Some(paint_proc), PAINT_SUBCLASS);
        }
    }
}
