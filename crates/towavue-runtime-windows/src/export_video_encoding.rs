use super::*;
use ffmpeg::format::Pixel;

/// A worker-owned encoding plan containing values only, never borrowed codec state.
#[derive(Clone)]
pub(super) struct HighDepth {
    pixel: Pixel,
    svt: bool,
    colors: [i32; 4],
}

impl HighDepth {
    pub(super) fn probe(request: &ExportRequest) -> Result<Option<Self>, ExportError> {
        if request.kind != MediaKind::Video {
            return Ok(None);
        }
        let inspect = || -> Result<_, ffmpeg::Error> {
            let input = ffmpeg::format::input(&request.source)?;
            let stream = input
                .streams()
                .best(ffmpeg::media::Type::Video)
                .ok_or(ffmpeg::Error::StreamNotFound)?;
            let decoder = ffmpeg::codec::context::Context::from_parameters(stream.parameters())?
                .decoder()
                .video()?;
            Ok((
                decoder.format(),
                (decoder.width(), decoder.height()),
                colors(&decoder),
            ))
        };
        let (source, mut size, colors) = inspect().map_err(|error| {
            ExportError::Failed(format!("could not inspect video precision: {error}"))
        })?;
        // SAFETY: descriptors are immutable FFmpeg storage. No native pointer
        // escapes this call; the plan copies only format/depth/color values.
        let descriptor = unsafe { ffmpeg::ffi::av_pix_fmt_desc_get(source.into()).as_ref() }
            .ok_or_else(|| ExportError::Failed("Unknown video sample precision".into()))?;
        let depth = descriptor.comp[..usize::from(descriptor.nb_components)]
            .iter()
            .map(|component| component.depth)
            .max()
            .unwrap_or(0);
        if (1..=8).contains(&depth) {
            return Ok(None);
        }
        let pixel =
            match source {
                Pixel::YUV420P10LE | Pixel::YUV420P10BE | Pixel::P010LE | Pixel::P010BE => {
                    Pixel::YUV420P10LE
                }
                Pixel::YUV422P10LE | Pixel::YUV422P10BE => Pixel::YUV422P10LE,
                Pixel::YUV444P10LE | Pixel::YUV444P10BE => Pixel::YUV444P10LE,
                Pixel::GBRP10LE | Pixel::GBRP10BE => Pixel::GBRP10LE,
                Pixel::GRAY10LE | Pixel::GRAY10BE => Pixel::GRAY10LE,
                Pixel::YUV420P12LE | Pixel::YUV420P12BE => Pixel::YUV420P12LE,
                Pixel::YUV422P12LE | Pixel::YUV422P12BE => Pixel::YUV422P12LE,
                Pixel::YUV444P12LE | Pixel::YUV444P12BE => Pixel::YUV444P12LE,
                Pixel::GBRP12LE | Pixel::GBRP12BE => Pixel::GBRP12LE,
                Pixel::GRAY12LE | Pixel::GRAY12BE => Pixel::GRAY12LE,
                _ => return Err(ExportError::Failed(
                    "This video sample format cannot yet be exported without reducing precision"
                        .into(),
                )),
            };
        let extension = extension(&request.target);
        if !matches!(extension.as_str(), "mp4" | "mkv" | "webm") {
            return Err(ExportError::Failed(
                "High-depth video export requires MP4, MKV or WebM to preserve sample precision"
                    .into(),
            ));
        }
        for operation in &request.operations {
            match *operation {
                EditOperation::Crop(crop) => size = (crop.width, crop.height),
                EditOperation::ResizeVideo(resize) => size = resize.size(),
                EditOperation::RotateVideo(rotation) => size = rotation.size(),
                EditOperation::RotateClockwise | EditOperation::RotateCounterclockwise => {
                    size = (size.1, size.0);
                }
                _ => {}
            }
        }
        // SVT requires even dimensions of at least 64. AOM covers small/odd
        // canvases and other supported chroma/depth layouts without padding or
        // silently reducing sample depth. Both encoders are already bundled.
        let svt = pixel == Pixel::YUV420P10LE
            && size.0 >= 64
            && size.1 >= 64
            && size.0.is_multiple_of(2)
            && size.1.is_multiple_of(2);
        Ok(Some(Self { pixel, svt, colors }))
    }

    pub(super) fn arguments(&self, target: &Path) -> Vec<String> {
        let encoder: &[&str] = if self.svt {
            &[
                "-c:v",
                "libsvtav1",
                "-preset",
                "6",
                "-crf",
                "12",
                "-svtav1-params",
                "lp=4:tune=0",
            ]
        } else {
            &[
                "-c:v",
                "libaom-av1",
                "-cpu-used",
                "6",
                "-crf",
                "12",
                "-b:v",
                "0",
                "-row-mt",
                "1",
                "-threads:v",
                "8",
            ]
        };
        let mut arguments: Vec<String> =
            encoder.iter().map(|argument| (*argument).into()).collect();
        arguments.extend([
            "-pix_fmt".into(),
            self.pixel
                .descriptor()
                .expect("known AV1 format")
                .name()
                .into(),
            "-c:a".into(),
            if extension(target) == "webm" {
                "libopus"
            } else {
                "aac"
            }
            .into(),
        ]);
        for (option, value) in [
            "-color_range",
            "-colorspace",
            "-color_primaries",
            "-color_trc",
        ]
        .into_iter()
        .zip(self.colors)
        {
            arguments.extend([option.into(), value.to_string()]);
        }
        arguments
    }

    pub(super) fn verify(&self, output: &Path) -> Result<(), ExportError> {
        let verify = || -> Result<_, ffmpeg::Error> {
            let input = ffmpeg::format::input(output)?;
            let stream = input
                .streams()
                .best(ffmpeg::media::Type::Video)
                .ok_or(ffmpeg::Error::StreamNotFound)?;
            let decoder = ffmpeg::codec::context::Context::from_parameters(stream.parameters())?
                .decoder()
                .video()?;
            let preserved_colors = self
                .colors
                .into_iter()
                .zip(colors(&decoder))
                .enumerate()
                .all(|(index, (expected, actual))| {
                    expected == if index == 0 { 0 } else { 2 } || expected == actual
                });
            Ok(decoder.id() == ffmpeg::codec::Id::AV1
                && decoder.format() == self.pixel
                && preserved_colors)
        };
        match verify() {
            Ok(true) => Ok(()),
            _ => Err(ExportError::Failed(
                "Encoded video did not retain the requested sample precision or color tags; existing output was not replaced".into(),
            )),
        }
    }
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn colors(decoder: &ffmpeg::decoder::Video) -> [i32; 4] {
    [
        ffmpeg::ffi::AVColorRange::from(decoder.color_range()) as i32,
        ffmpeg::ffi::AVColorSpace::from(decoder.color_space()) as i32,
        ffmpeg::ffi::AVColorPrimaries::from(decoder.color_primaries()) as i32,
        ffmpeg::ffi::AVColorTransferCharacteristic::from(decoder.color_transfer_characteristic())
            as i32,
    ]
}
