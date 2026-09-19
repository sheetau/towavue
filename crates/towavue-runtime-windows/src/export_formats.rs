use super::*;
use crate::MediaInput;

#[path = "export_format_alpha.rs"]
mod alpha;

/// Value-only description; preparation and all source inspection run on a worker.
#[derive(Clone, Debug)]
pub struct ExportDialogRequest {
    pub input: MediaInput,
    pub kind: MediaKind,
    pub operations: Vec<EditOperation>,
    pub options: ExportOptions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Format {
    Png,
    FramePng,
    Jpeg,
    Webp,
    Avif,
    Gif,
    Tiff,
    Bmp,
    Mp4,
    Mkv,
    Mov,
    Webm,
    Avi,
    Wmv,
    ThreeGp,
    TransportStream,
    Wav,
    Flac,
    Mp3,
    M4a,
    Aac,
    Ogg,
    Opus,
}

pub(crate) const AUDIO_FORMATS: [Format; 7] = [
    Format::Wav,
    Format::Flac,
    Format::Mp3,
    Format::M4a,
    Format::Aac,
    Format::Ogg,
    Format::Opus,
];

impl Format {
    pub(crate) fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Png => &["png", "apng"],
            Self::FramePng => &["png"],
            Self::Jpeg => &["jpg", "jpeg"],
            Self::Webp => &["webp"],
            Self::Avif => &["avif"],
            Self::Gif => &["gif"],
            Self::Tiff => &["tif", "tiff"],
            Self::Bmp => &["bmp"],
            Self::Mp4 => &["mp4"],
            Self::Mkv => &["mkv"],
            Self::Mov => &["mov"],
            Self::Webm => &["webm"],
            Self::Avi => &["avi"],
            Self::Wmv => &["wmv"],
            Self::ThreeGp => &["3gp"],
            Self::TransportStream => &["ts", "mts", "m2ts"],
            Self::Wav => &["wav"],
            Self::Flac => &["flac"],
            Self::Mp3 => &["mp3"],
            Self::M4a => &["m4a"],
            Self::Aac => &["aac"],
            Self::Ogg => &["ogg"],
            Self::Opus => &["opus"],
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG / APNG image",
            Self::FramePng => "PNG image",
            Self::Jpeg => "JPEG image",
            Self::Webp => "WebP image",
            Self::Avif => "AVIF image",
            Self::Gif => "GIF image (256 colors, binary transparency)",
            Self::Tiff => "TIFF image",
            Self::Bmp => "Bitmap image",
            Self::Mp4 => "MP4 video",
            Self::Mkv => "Matroska video",
            Self::Mov => "QuickTime video",
            Self::Webm => "WebM video",
            Self::Avi => "AVI video",
            Self::Wmv => "Windows Media video",
            Self::ThreeGp => "3GP video",
            Self::TransportStream => "MPEG transport stream",
            Self::Wav => "WAV audio",
            Self::Flac => "FLAC audio",
            Self::Mp3 => "MP3 audio",
            Self::M4a => "M4A / AAC audio",
            Self::Aac => "AAC audio",
            Self::Ogg => "Ogg / Opus audio",
            Self::Opus => "Opus audio",
        }
    }

    fn supports_metadata(self, options: &MetadataExportOptions) -> bool {
        MetadataField::ALL.into_iter().all(|field| {
            let Some(value) = options.get(field).filter(|value| !value.is_empty()) else {
                return true;
            };
            match self {
                Self::Aac | Self::ThreeGp | Self::TransportStream => false,
                Self::Wav => !matches!(field, MetadataField::AlbumArtist | MetadataField::Composer),
                Self::Avi => !matches!(
                    field,
                    MetadataField::Album | MetadataField::AlbumArtist | MetadataField::Composer
                ),
                Self::Mov => !matches!(
                    field,
                    MetadataField::AlbumArtist | MetadataField::Composer | MetadataField::Track
                ),
                Self::Mp4 | Self::M4a if field == MetadataField::Track => {
                    let parts: Vec<_> = value.split('/').collect();
                    (1..=2).contains(&parts.len())
                        && parts.iter().all(|part| {
                            part.parse::<u16>()
                                .is_ok_and(|number| number > 0 && number.to_string() == *part)
                        })
                }
                _ => true,
            }
        })
    }

    pub(crate) fn accepts(self, path: &Path) -> bool {
        path.extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| {
                self.extensions()
                    .iter()
                    .any(|allowed| ext.eq_ignore_ascii_case(allowed))
            })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Choices {
    pub formats: Vec<Format>,
    pub initial: usize,
    pub default_extension: String,
}

impl Choices {
    pub(crate) fn new(formats: Vec<Format>, source: &Path) -> Result<Self, ExportError> {
        if formats.is_empty() {
            return Err(ExportError::Failed(
                "No export format supports this document and its current output options".into(),
            ));
        }
        let initial = formats
            .iter()
            .position(|format| format.accepts(source))
            .unwrap_or(0);
        let default_extension = if formats[initial].accepts(source) {
            source
                .extension()
                .expect("matched extension")
                .to_string_lossy()
                .into_owned()
        } else {
            formats[initial].extensions()[0].into()
        };
        Ok(Self {
            formats,
            initial,
            default_extension,
        })
    }

    pub(crate) fn filename(&self, suggested: &str) -> String {
        let mut name = PathBuf::from(suggested);
        if !self.formats[self.initial].accepts(&name) {
            name.set_extension(&self.default_extension);
        }
        name.to_string_lossy().into_owned()
    }

    pub(crate) fn validate(&self, one_based: u32, path: &Path) -> Result<(), String> {
        let format = one_based
            .checked_sub(1)
            .and_then(|i| self.formats.get(i as usize))
            .ok_or_else(|| "Choose a supported file type.".to_owned())?;
        if !format.accepts(path) {
            return Err(format!(
                "The filename extension does not match {}. Change the filename extension or select the matching file type.",
                format.label()
            ));
        }
        Ok(())
    }
}

impl ExportDialogRequest {
    pub(crate) fn choices(&self, cancelled: &AtomicBool) -> Result<Choices, ExportError> {
        use Format::*;
        check_cancelled(cancelled)?;
        let path = self.input.path();
        let stamp = audio_options::SourceStamp::read(path)?;
        let mut request = ExportRequest {
            source: path.to_owned(),
            target: PathBuf::from("export.mp4"),
            kind: self.kind,
            operations: self.operations.clone(),
            hardware_encode: false,
        };
        let mut formats = if self.options.output == ExportOutput::VideoFrame {
            vec![FramePng]
        } else if self.kind == MediaKind::Audio || self.options.output == ExportOutput::AudioOnly {
            if self.kind == MediaKind::Image || ExportStreams::probe(&request)?.audio.is_none() {
                return Err(ExportError::Failed(
                    "This media has no audio stream to export".into(),
                ));
            }
            AUDIO_FORMATS.to_vec()
        } else if self.kind == MediaKind::Video {
            if video_encoding::HighDepth::probe(&request)?.is_some() {
                vec![Mp4, Mkv, Webm]
            } else {
                vec![Mp4, Mkv, Mov, Webm, Avi, Wmv, ThreeGp, TransportStream]
            }
        } else {
            image_formats(&mut request, &self.options.metadata, cancelled)?
        };
        if self.kind != MediaKind::Image {
            formats.retain(|format| format.supports_metadata(&self.options.metadata));
        }
        stamp.verify(path)?;
        check_cancelled(cancelled)?;
        // Audio-only starts in WAV; ordinary audio retains its supported source format.
        let source = if self.options.output == ExportOutput::AudioOnly {
            Path::new("audio.wav")
        } else {
            self.input.logical_path()
        };
        Choices::new(formats, source)
    }
}

fn image_formats(
    request: &mut ExportRequest,
    metadata: &MetadataExportOptions,
    cancelled: &AtomicBool,
) -> Result<Vec<Format>, ExportError> {
    use Format::*;
    let path = &request.source;
    if let Some(format) = ImageMetadataFormat::from_path(path) {
        format.validate_options(metadata)?;
    }
    let mut formats = vec![Png, Jpeg, Webp, Avif, Gif, Tiff, Bmp];
    if !metadata.is_empty() {
        formats.retain(|format| match ImageMetadataFormat::from_path(path) {
            Some(ImageMetadataFormat::Png) => *format == Png,
            Some(ImageMetadataFormat::Jpeg | ImageMetadataFormat::Webp) => {
                matches!(format, Jpeg | Webp)
            }
            None => false,
        });
    }
    let png = if png_metadata::png_path(path) {
        request.target = path.clone();
        Some(png_metadata::PngMetadata::prepare(
            request, metadata, cancelled,
        )?)
    } else {
        None
    };
    let gif = gif_animation::gif_path(path)
        .then(|| gif_animation::Animation::read(path, cancelled))
        .transpose()?;
    let webp = if webp_metadata::webp_path(path) {
        Some(webp_metadata::export_traits(path, cancelled)?)
    } else {
        None
    };
    let avif = avif::avif_path(path)
        .then(|| avif::Animation::read(path, cancelled))
        .transpose()?
        .flatten();
    let animated = png
        .as_ref()
        .is_some_and(png_metadata::PngMetadata::is_animated)
        || gif
            .as_ref()
            .is_some_and(gif_animation::Animation::has_timing)
        || webp.is_some_and(|(_, animated)| animated)
        || avif.is_some();
    if animated {
        formats.retain(|format| matches!(format, Png | Webp | Avif | Gif));
    }
    for format in formats.clone() {
        request.target = PathBuf::from("export").with_extension(format.extensions()[0]);
        let compatible = if let Some(animation) = &avif {
            animation.validate_output(&request.target)
        } else if png
            .as_ref()
            .is_some_and(png_metadata::PngMetadata::is_animated)
            && format != Png
        {
            png_metadata::PngMetadata::prepare_conversion(path, &request.target, cancelled)
                .map(|_| ())
        } else if webp.is_some_and(|(_, animated)| animated) && format != Webp {
            webp_metadata::SnapshotConversion::prepare(path, &request.target, cancelled).map(|_| ())
        } else if let Some(gif) = &gif {
            match format {
                Png | Webp if gif.has_timing() && !gif.is_animated() => Err(ExportError::Failed(
                    "The single timed GIF frame requires GIF or AVIF output".into(),
                )),
                Webp if gif.is_animated() => gif.webp_plays().map(|_| ()),
                Avif => gif.avif_delays().map(|_| ()),
                _ => Ok(()),
            }
        } else {
            Ok(())
        };
        if compatible.is_err() {
            formats.retain(|candidate| *candidate != format);
        }
    }
    // A free rotation can introduce transparent corners even with an opaque input.
    let transparent_edit = request.operations.iter().any(|operation|
        matches!(operation, EditOperation::RotateImage(rotation) if rotation.tenths() % 900 != 0));
    let alpha = if let Some((alpha, _)) = webp {
        alpha
    } else if avif::avif_path(path) {
        avif::has_alpha(path, avif.as_ref(), cancelled)?
    } else {
        image_alpha(path)?
    };
    if formats.iter().any(|format| matches!(format, Jpeg | Gif)) {
        use alpha::Transparency;
        let mut transparency = alpha::inspect(path, animated, alpha, cancelled);
        let interpolated = request.operations.iter().any(|operation| {
            matches!(operation,
            EditOperation::Resize(resize) if resize.filter != towavue_core::ResampleFilter::Nearest)
        });
        if transparent_edit || (transparency == Transparency::Binary && interpolated) {
            transparency = Transparency::Full;
        }
        formats.retain(|format| match format {
            Jpeg | Bmp => transparency == Transparency::Opaque,
            // Static cross-format GIF currently uses FFmpeg's fixed palette,
            // which drops transparent pixels. Animated and GIF-native routes
            // use the existing RGBA palette writer instead.
            Gif => {
                transparency != Transparency::Full
                    && (transparency == Transparency::Opaque || animated || gif.is_some())
            }
            _ => true,
        });
    }
    check_cancelled(cancelled)?;
    Ok(formats)
}

fn image_alpha(path: &Path) -> Result<bool, ExportError> {
    use image::ImageDecoder;
    let reader = image::ImageReader::open(path)
        .map_err(ExportError::Output)?
        .with_guessed_format()
        .map_err(ExportError::Output)?;
    let decoder = reader.into_decoder().map_err(|error| {
        ExportError::Failed(format!("Could not inspect image transparency: {error}"))
    })?;
    Ok(decoder.color_type().has_alpha())
}

#[cfg(test)]
#[path = "export_format_tests.rs"]
mod tests;
