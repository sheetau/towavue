//! Opt-in libpng 1.6.58 simplified API probe, not a production decoder.
//! ABI source: https://github.com/pnggroup/libpng/blob/v1.6.58/png.h

use std::ffi::{c_char, c_int, c_void};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use towavue_runtime_windows::DecodedImageFrame;
use windows::Win32::Foundation::{FreeLibrary, HMODULE};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
    LoadLibraryExW,
};
use windows::core::{PCWSTR, s};

// png_image version 1 from the pinned header. No classic libpng callbacks or
// setjmp/longjmp cross Rust frames: simplified API error recovery stays in C.
#[repr(C)]
struct PngImage {
    opaque: *mut c_void,
    version: u32,
    width: u32,
    height: u32,
    format: u32,
    flags: u32,
    colormap_entries: u32,
    warning_or_error: u32,
    message: [c_char; 64],
}

type Begin = unsafe extern "C" fn(*mut PngImage, *const c_void, usize) -> c_int;
type Finish =
    unsafe extern "C" fn(*mut PngImage, *const c_void, *mut c_void, i32, *mut c_void) -> c_int;
type Free = unsafe extern "C" fn(*mut PngImage);

struct Library(HMODULE);

impl Drop for Library {
    fn drop(&mut self) {
        // Every image borrows Decoder; no image/function call outlives this DLL.
        let _ = unsafe { FreeLibrary(self.0) };
    }
}

pub(super) struct Decoder {
    begin: Begin,
    finish: Finish,
    free: Free,
    _library: Library,
}

struct Image<'a> {
    raw: PngImage,
    decoder: &'a Decoder,
}

impl Drop for Image<'_> {
    fn drop(&mut self) {
        // Safe after begin/finish failures, or a successful finish, which clears
        // opaque itself. This guard is destroyed before its borrowed input data.
        unsafe { (self.decoder.free)(&mut self.raw) };
    }
}

impl Decoder {
    pub(super) fn load() -> Self {
        assert_eq!(std::mem::size_of::<usize>(), 8, "x64 benchmark only");
        assert_eq!(std::mem::size_of::<PngImage>(), 104);
        assert_eq!(std::mem::offset_of!(PngImage, message), 36);
        let path = PathBuf::from(
            std::env::var_os("TOWAVUE_LIBPNG_DLL")
                .expect("explicit trusted libpng 1.6.58 DLL required"),
        );
        assert!(path.is_absolute(), "absolute trusted DLL path required");
        let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // Search dependencies only beside the explicitly supplied DLL and in
        // standard safe directories, never the working directory or PATH.
        let library = Library(unsafe {
            LoadLibraryExW(
                PCWSTR(name.as_ptr()),
                None,
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
            )
            .expect("load trusted libpng")
        });
        macro_rules! symbol {
            ($name:literal, $ty:ty) => {{
                // Signatures are the exported PNGAPI (__cdecl) declarations in
                // the pinned header; library ownership keeps their code loaded.
                unsafe {
                    std::mem::transmute::<unsafe extern "system" fn() -> isize, $ty>(
                        GetProcAddress(library.0, s!($name)).expect("libpng symbol"),
                    )
                }
            }};
        }
        let version = symbol!("png_access_version_number", unsafe extern "C" fn() -> u32);
        assert_eq!(unsafe { version() }, 10658, "pinned libpng required");
        Self {
            begin: symbol!("png_image_begin_read_from_memory", Begin),
            finish: symbol!("png_image_finish_read", Finish),
            free: symbol!("png_image_free", Free),
            _library: library,
        }
    }

    pub(super) fn decode(&self, path: &Path) -> DecodedImageFrame {
        // Rust opens Unicode paths. Compressed input allocation/read/release is
        // part of the timing, and is extra memory beyond the RGBA canvas limit.
        let encoded = std::fs::read(path).expect("PNG read failed");
        let mut image = Image {
            // All fields permit zero; the API requires an initially zeroed image
            // with version set to PNG_IMAGE_VERSION before the first call.
            raw: unsafe { std::mem::zeroed() },
            decoder: self,
        };
        image.raw.version = 1;
        assert_ne!(
            unsafe { (self.begin)(&mut image.raw, encoded.as_ptr().cast(), encoded.len()) },
            0,
            "libpng begin failed"
        );
        let (width, height) = (image.raw.width, image.raw.height);
        let bytes = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .expect("RGBA size");
        assert!(width > 0 && height > 0 && bytes <= 512 * 1024 * 1024);
        image.raw.format = 3; // PNG_FORMAT_RGBA: 8-bit, straight alpha, no colormap.
        let mut rgba = vec![0; bytes];
        assert_ne!(
            unsafe {
                (self.finish)(
                    &mut image.raw,
                    std::ptr::null(),
                    rgba.as_mut_ptr().cast(),
                    0, // libpng's tightly packed, positive row stride.
                    std::ptr::null_mut(),
                )
            },
            0,
            "libpng finish failed"
        );
        DecodedImageFrame {
            width,
            height,
            rgba,
            delay: Duration::ZERO,
        }
    }
}
