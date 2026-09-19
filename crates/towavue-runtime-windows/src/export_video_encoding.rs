use super::*;
use ffmpeg::format::Pixel;

/// Compression presets preserve the requested geometry, timing and sample depth.
/// Encoder-specific controls are deliberately not a common numeric quality scale.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum VideoExportQuality {
    #[default]
    High,
    Balanced,
    Smaller,
}

impl VideoExportQuality {
    pub fn label(self) -> &'static str {
        match self {
            Self::High => "High quality",
            Self::Balanced => "Balanced",
            Self::Smaller => "Smaller file",
        }
    }

    pub(super) fn av1_crf(self, svt: bool) -> &'static str {
        match (self, svt) {
            (Self::High, _) => "12",
            (Self::Balanced, true) => "24",
            (Self::Smaller, true) => "36",
            (Self::Balanced, false) => "28",
            (Self::Smaller, false) => "40",
        }
    }

    pub(super) fn codec_arguments(self, codec: &str) -> &'static [&'static str] {
        match (codec, self) {
            ("libopenh264", Self::High) => &[
                "-profile:v",
                "high",
                "-rc_mode",
                "quality",
                "-qmin:v",
                "1",
                "-qmax:v",
                "20",
            ],
            ("libopenh264", Self::Balanced) => &[
                "-profile:v",
                "high",
                "-rc_mode",
                "quality",
                "-qmin:v",
                "18",
                "-qmax:v",
                "32",
            ],
            ("libopenh264", Self::Smaller) => &[
                "-profile:v",
                "high",
                "-rc_mode",
                "quality",
                "-qmin:v",
                "28",
                "-qmax:v",
                "42",
            ],
            ("libvpx-vp9", Self::High) => &["-crf", "18", "-b:v", "0"],
            ("libvpx-vp9", Self::Balanced) => &["-crf", "30", "-b:v", "0"],
            ("libvpx-vp9", Self::Smaller) => &["-crf", "42", "-b:v", "0"],
            ("mpeg4" | "wmv2", Self::High) => &["-q:v", "2"],
            ("mpeg4" | "wmv2", Self::Balanced) => &["-q:v", "5"],
            ("mpeg4" | "wmv2", Self::Smaller) => &["-q:v", "10"],
            ("h264_mf", Self::High) => &["-rate_control", "quality", "-quality", "95"],
            ("h264_mf", Self::Balanced) => &["-rate_control", "quality", "-quality", "75"],
            ("h264_mf", Self::Smaller) => &["-rate_control", "quality", "-quality", "50"],
            _ => &[],
        }
    }
}

/// A worker-owned encoding plan containing values only, never borrowed codec state.
#[derive(Clone)]
pub(super) struct HighDepth {
    pixel: Pixel,
    svt: bool,
    colors: [i32; 4],
    // AV1 sequence headers explicitly represent left/top-left 4:2:0 siting.
    // Other declarations are not interchangeable with these positions.
    chroma: Option<ffmpeg::util::chroma::Location>,
    orientation: crate::VideoOrientation,
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
                decoder.chroma_location(),
                stream
                    .side_data()
                    .find(|data| {
                        data.kind() == ffmpeg::codec::packet::side_data::Type::DisplayMatrix
                    })
                    .map(|data| data.data().to_vec()),
            ))
        };
        let (source, mut size, colors, source_chroma, matrix) = inspect().map_err(|error| {
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
        let chroma = matches!(pixel, Pixel::YUV420P10LE | Pixel::YUV420P12LE)
            .then_some(source_chroma)
            .filter(|location| {
                matches!(
                    location,
                    ffmpeg::util::chroma::Location::Left | ffmpeg::util::chroma::Location::TopLeft
                )
            });
        let orientation = if chroma.is_some() {
            crate::VideoOrientation::from_bytes(matrix.as_deref()).map_err(|error| {
                ExportError::Failed(format!("Unsupported high-depth video orientation: {error}"))
            })?
        } else {
            crate::VideoOrientation::default()
        };
        if orientation.swaps_axes() {
            size = (size.1, size.0);
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
        Ok(Some(Self {
            pixel,
            svt,
            colors,
            chroma,
            orientation,
        }))
    }

    pub(super) fn input_arguments(&self) -> &'static [&'static str] {
        if self.orientation == crate::VideoOrientation::default() {
            &[]
        } else {
            // Apply the captured stream matrix after chroma reconstruction.
            // Clear its input declaration as well as disabling autorotation:
            // otherwise the baked output can inherit it and rotate twice.
            &[
                "-noautorotate",
                "-display_rotation",
                "0",
                "-nodisplay_hflip",
                "-nodisplay_vflip",
            ]
        }
    }

    pub(super) fn visual_filters(&self, operations: &[EditOperation]) -> Vec<String> {
        let mut filters = visual_filters_with_depth(operations, true);
        let orientation = self.orientation.ffmpeg_filter().trim_end_matches(',');
        if !orientation.is_empty() {
            filters.insert(0, orientation.into());
        }
        if let Some(chroma) = self.chroma
            && !filters.is_empty()
        {
            let location: ffmpeg::ffi::AVChromaLocation = chroma.into();
            // Transform luma and chroma on the same pixel grid. Flipping/cropping
            // subsampled planes directly changes their phase relative to luma,
            // especially at odd crop origins. Resolve the declared positions
            // before edits, then subsample once onto the declared output grid.
            filters.insert(
                0,
                format!(
                    "scale=flags=bilinear+accurate_rnd:in_chroma_loc={},format=yuv444p16le",
                    location as i32
                ),
            );
            filters.push(format!(
                "scale=flags=bilinear+accurate_rnd:out_chroma_loc={},format={}",
                location as i32,
                self.pixel.descriptor().expect("known AV1 format").name()
            ));
        }
        filters
    }

    pub(super) fn arguments(&self, target: &Path, quality: VideoExportQuality) -> Vec<String> {
        let encoder: &[&str] = if self.svt {
            &[
                "-c:v",
                "libsvtav1",
                "-preset",
                "6",
                "-crf",
                quality.av1_crf(self.svt),
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
                quality.av1_crf(self.svt),
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
        if let Some(chroma) = self.chroma {
            let position = match chroma {
                ffmpeg::util::chroma::Location::Left => "vertical",
                ffmpeg::util::chroma::Location::TopLeft => "colocated",
                _ => unreachable!("only AV1-representable declarations enter the plan"),
            };
            let location: ffmpeg::ffi::AVChromaLocation = chroma.into();
            // The native AOM/SVT wrappers do not reliably propagate the frame
            // declaration into the AV1 sequence header. Set both muxer/encoder
            // metadata and the actual bitstream; packet pixels are not rewritten.
            arguments.extend([
                "-chroma_sample_location".into(),
                (location as i32).to_string(),
                "-bsf:v".into(),
                format!("av1_metadata=chroma_sample_position={position}"),
            ]);
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
            let matrix = stream
                .side_data()
                .find(|data| data.kind() == ffmpeg::codec::packet::side_data::Type::DisplayMatrix);
            let orientation_cleared = self.orientation == crate::VideoOrientation::default()
                || crate::VideoOrientation::from_bytes(matrix.as_ref().map(|data| data.data()))
                    .is_ok_and(|orientation| orientation == crate::VideoOrientation::default());
            Ok(decoder.id() == ffmpeg::codec::Id::AV1
                && decoder.format() == self.pixel
                && orientation_cleared
                && preserved_colors
                && self
                    .chroma
                    .is_none_or(|expected| decoder.chroma_location() == expected))
        };
        match verify() {
            Ok(true) => Ok(()),
            _ => Err(ExportError::Failed(
                "Encoded video did not retain the requested sample precision, color tags or baked orientation; existing output was not replaced".into(),
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
