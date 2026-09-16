use super::*;
use std::{ffi::c_void, ptr};

// Public libavcodec/exif.h API from the pinned FFmpeg 9 build. ffmpeg-sys-next
// does not include this header. Only the top-level owning IFD is described here;
// entries stay opaque and libavcodec alone allocates, walks and frees them.
#[repr(C)]
struct ExifMetadata {
    entries: *mut c_void,
    count: u32,
    size: u32,
}

unsafe extern "C" {
    fn av_exif_parse_buffer(
        log: *mut c_void,
        data: *const u8,
        size: usize,
        metadata: *mut ExifMetadata,
        header_mode: i32,
    ) -> i32;
    fn av_exif_write(
        log: *mut c_void,
        metadata: *const ExifMetadata,
        buffer: *mut *mut ffmpeg::ffi::AVBufferRef,
        header_mode: i32,
    ) -> i32;
    fn av_exif_free(metadata: *mut ExifMetadata);
}

struct OwnedExif(ExifMetadata);
impl Drop for OwnedExif {
    fn drop(&mut self) {
        // SAFETY: constructed only after successful parsing; this is the sole
        // owner of the native entries and is released exactly once.
        unsafe { av_exif_free(&mut self.0) };
    }
}

struct OwnedBuffer(*mut ffmpeg::ffi::AVBufferRef);
impl Drop for OwnedBuffer {
    fn drop(&mut self) {
        // SAFETY: write returns a caller-owned reference (or NULL on failure).
        unsafe { ffmpeg::ffi::av_buffer_unref(&mut self.0) };
    }
}

fn thumbnail_link(data: &[u8]) -> Option<usize> {
    let little = match data.get(..4)? {
        b"II\x2a\0" => true,
        b"MM\0\x2a" => false,
        _ => return None,
    };
    let long = |at: usize| -> Option<u32> {
        let bytes = data.get(at..at.checked_add(4)?)?.try_into().ok()?;
        Some(if little {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        })
    };
    let root = long(4)? as usize;
    if root < 8 {
        return None;
    }
    let bytes = data.get(root..root.checked_add(2)?)?.try_into().ok()?;
    let count = if little {
        u16::from_le_bytes(bytes)
    } else {
        u16::from_be_bytes(bytes)
    };
    let link = root
        .checked_add(2)?
        .checked_add(usize::from(count).checked_mul(12)?)?;
    (long(link)? != 0).then_some(link)
}

/// Drop standard IFD1 thumbnails, including their orphaned pixel bytes, while
/// retaining the parsed IFD0 descriptive fields. A reflected/cropped frame must
/// not carry the source's unchanged miniature. The native PNG encoder already
/// normalizes orientation/dimensions, but can reuse raw EXIF for same-size edits.
pub(super) fn remove_thumbnail(frame: &mut frame::Video) -> Result<(), DecodeError> {
    use ffmpeg::util::frame::side_data::Type;
    let Some(side) = frame.side_data(Type::EXIF) else {
        return Ok(());
    };
    let data = side.data();
    let Some(link) = thumbnail_link(data) else {
        // Leave thumbnail-free or unrecognized profiles to the existing encoder.
        return Ok(());
    };
    // Sever the next-IFD chain on a private copy before parsing. The native
    // parser otherwise retains extra IFDs as synthetic entries for roundtrips.
    // Rewriting only the reachable metadata also drops orphaned thumbnail bytes.
    let mut data = data.to_vec();
    data[link..link + 4].fill(0);
    let mut metadata = ExifMetadata {
        entries: ptr::null_mut(),
        count: 0,
        size: 0,
    };
    // SAFETY: input remains borrowed/live through parsing. Header mode 0 is
    // AV_EXIF_TIFF_HEADER. On failure the parser frees its partial allocations;
    // only success transfers ownership into OwnedExif below.
    let result = unsafe {
        av_exif_parse_buffer(ptr::null_mut(), data.as_ptr(), data.len(), &mut metadata, 0)
    };
    if result < 0 {
        return Err(ffmpeg::Error::from(result).into());
    }
    let metadata = OwnedExif(metadata);
    let mut buffer = OwnedBuffer(ptr::null_mut());
    // SAFETY: the parsed IFD remains alive, output starts NULL and is owned by
    // buffer. Only IFD0 and its tagged sub-IFDs are reachable after severing the
    // next-IFD link; serialization discards unreferenced thumbnail payload.
    let result = unsafe { av_exif_write(ptr::null_mut(), &metadata.0, &mut buffer.0, 0) };
    if result < 0 {
        return Err(ffmpeg::Error::from(result).into());
    }
    // SAFETY: success returned a live ref held through the copy. No native
    // pointer or borrow survives replacement of the working frame's side data.
    let bytes = unsafe {
        let buffer = buffer
            .0
            .as_ref()
            .ok_or_else(|| invalid("empty normalized EXIF"))?;
        if buffer.size == 0 || buffer.data.is_null() {
            return Err(invalid("empty normalized EXIF"));
        }
        std::slice::from_raw_parts(buffer.data, buffer.size).to_vec()
    };
    frame.remove_side_data(Type::EXIF);
    let mut side = frame
        .new_side_data(Type::EXIF, bytes.len())
        .ok_or_else(|| invalid("could not allocate normalized EXIF"))?;
    // SAFETY: exclusive access to the newly allocated frame-owned region. Source
    // side data belongs to a different reference and was never modified in place.
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), (*side.as_mut_ptr()).data, bytes.len());
    }
    Ok(())
}
