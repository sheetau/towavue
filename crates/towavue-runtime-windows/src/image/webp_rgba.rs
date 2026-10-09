// Adapted from the image 0.25.10 WebP adapter (MIT OR Apache-2.0).
// Static decoding requests RGBA directly; animation and metadata retain the
// pinned adapter's behavior. No RGB intermediate is needed for opaque stills.
use std::io::{BufRead, Read, Seek};

use image_webp::LoopCount;

use image::buffer::ConvertBuffer;
use image::error::{DecodingError, ImageError, ImageResult};
use image::metadata::Orientation;
use image::{
    AnimationDecoder, ColorType, Delay, Frame, Frames, ImageDecoder, ImageFormat, RgbImage,
    RgbaImage,
};

/// WebP Image format decoder.
///
/// Supports both lossless and lossy WebP images.
pub struct WebPDecoder<R> {
    inner: image_webp::WebPDecoder<R>,
    orientation: Option<Orientation>,
}

impl<R: BufRead + Seek> WebPDecoder<R> {
    /// Create a new `WebPDecoder` from the Reader `r`.
    pub fn new(r: R) -> ImageResult<Self> {
        Ok(Self {
            inner: image_webp::WebPDecoder::new(r).map_err(decode_error)?,
            orientation: None,
        })
    }

    /// Returns true if the image as described by the bitstream is animated.
    pub fn has_animation(&self) -> bool {
        self.inner.is_animated()
    }
}

impl<R: BufRead + Seek> ImageDecoder for WebPDecoder<R> {
    fn dimensions(&self) -> (u32, u32) {
        self.inner.dimensions()
    }

    fn color_type(&self) -> ColorType {
        if !self.inner.is_animated() || self.inner.has_alpha() {
            ColorType::Rgba8
        } else {
            ColorType::Rgb8
        }
    }

    fn read_image(mut self, buf: &mut [u8]) -> ImageResult<()> {
        assert_eq!(u64::try_from(buf.len()), Ok(self.total_bytes()));

        if self.inner.is_animated() {
            self.inner.read_image(buf).map_err(decode_error)
        } else {
            self.inner.read_image_rgba(buf).map_err(decode_error)
        }
    }

    fn read_image_boxed(self: Box<Self>, buf: &mut [u8]) -> ImageResult<()> {
        (*self).read_image(buf)
    }

    fn icc_profile(&mut self) -> ImageResult<Option<Vec<u8>>> {
        self.inner.icc_profile().map_err(decode_error)
    }

    fn exif_metadata(&mut self) -> ImageResult<Option<Vec<u8>>> {
        let exif = self.inner.exif_metadata().map_err(decode_error)?;

        self.orientation = Some(
            exif.as_ref()
                .and_then(|exif| Orientation::from_exif_chunk(exif))
                .unwrap_or(Orientation::NoTransforms),
        );

        Ok(exif)
    }

    fn xmp_metadata(&mut self) -> ImageResult<Option<Vec<u8>>> {
        self.inner.xmp_metadata().map_err(decode_error)
    }

    fn orientation(&mut self) -> ImageResult<Orientation> {
        // `exif_metadata` caches the orientation, so call it if `orientation` hasn't been set yet.
        if self.orientation.is_none() {
            let _ = self.exif_metadata()?;
        }
        Ok(self.orientation.unwrap_or(Orientation::NoTransforms))
    }
}

impl<'a, R: 'a + BufRead + Seek> AnimationDecoder<'a> for WebPDecoder<R> {
    fn loop_count(&self) -> image::metadata::LoopCount {
        match self.inner.loop_count() {
            LoopCount::Forever => image::metadata::LoopCount::Infinite,
            LoopCount::Times(n) => image::metadata::LoopCount::Finite(n.into()),
        }
    }

    fn into_frames(self) -> Frames<'a> {
        struct FramesInner<R: Read + Seek> {
            decoder: WebPDecoder<R>,
            current: u32,
        }
        impl<R: BufRead + Seek> Iterator for FramesInner<R> {
            type Item = ImageResult<Frame>;

            fn next(&mut self) -> Option<Self::Item> {
                if self.current == self.decoder.inner.num_frames() {
                    return None;
                }
                self.current += 1;
                let (width, height) = self.decoder.inner.dimensions();

                let (img, delay) = if self.decoder.inner.has_alpha() {
                    let mut img = RgbaImage::new(width, height);
                    match self.decoder.inner.read_frame(&mut img) {
                        Ok(delay) => (img, delay),
                        Err(image_webp::DecodingError::NoMoreFrames) => return None,
                        Err(e) => return Some(Err(decode_error(e))),
                    }
                } else {
                    let mut img = RgbImage::new(width, height);
                    match self.decoder.inner.read_frame(&mut img) {
                        Ok(delay) => (img.convert(), delay),
                        Err(image_webp::DecodingError::NoMoreFrames) => return None,
                        Err(e) => return Some(Err(decode_error(e))),
                    }
                };

                Some(Ok(Frame::from_parts(
                    img,
                    0,
                    0,
                    Delay::from_numer_denom_ms(delay, 1),
                )))
            }
        }

        Frames::new(Box::new(FramesInner {
            decoder: self,
            current: 0,
        }))
    }
}

fn decode_error(e: image_webp::DecodingError) -> ImageError {
    match e {
        image_webp::DecodingError::IoError(e) => ImageError::IoError(e),
        _ => ImageError::Decoding(DecodingError::new(ImageFormat::WebP.into(), e)),
    }
}

#[cfg(test)]
mod tests;
