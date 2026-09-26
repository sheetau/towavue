use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum MetadataField {
    Title,
    Artist,
    Album,
    AlbumArtist,
    Composer,
    Genre,
    Date,
    Track,
    Comment,
    Copyright,
}

impl MetadataField {
    pub const ALL: [Self; 10] = [
        Self::Title,
        Self::Artist,
        Self::Album,
        Self::AlbumArtist,
        Self::Composer,
        Self::Genre,
        Self::Date,
        Self::Track,
        Self::Comment,
        Self::Copyright,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Artist => "artist",
            Self::Album => "album",
            Self::AlbumArtist => "album_artist",
            Self::Composer => "composer",
            Self::Genre => "genre",
            Self::Date => "date",
            Self::Track => "track",
            Self::Comment => "comment",
            Self::Copyright => "copyright",
        }
    }

    pub fn label(self) -> &'static str {
        self.label_in(towavue_core::localization::Language::English)
    }

    /// Display translation is separate from canonical field/tag names.
    pub fn label_in(self, language: towavue_core::localization::Language) -> &'static str {
        use towavue_core::localization::Text;
        match self {
            Self::Title => Text::MetadataTitle,
            Self::Artist => Text::MetadataArtist,
            Self::Album => Text::MetadataAlbum,
            Self::AlbumArtist => Text::MetadataAlbumArtist,
            Self::Composer => Text::MetadataComposer,
            Self::Genre => Text::MetadataGenre,
            Self::Date => Text::MetadataDate,
            Self::Track => Text::MetadataTrack,
            Self::Comment => Text::MetadataComment,
            Self::Copyright => Text::MetadataCopyright,
        }
        .in_language(language)
    }
}

/// Source labels remain typed until the receiving window chooses its language.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MetadataSourceScope {
    File,
    Video,
    Audio,
    PngText,
    Xmp(ImageMetadataFormat),
    XmpLanguage(ImageMetadataFormat, String),
    XmpCreator(ImageMetadataFormat, usize),
}

impl MetadataSourceScope {
    pub fn message(&self, language: towavue_core::localization::Language) -> String {
        use towavue_core::localization::{Text, formatted};
        match self {
            Self::File => Text::MetadataSourceFile.in_language(language).into(),
            Self::Video => Text::MetadataSourceVideo.in_language(language).into(),
            Self::Audio => Text::MetadataSourceAudio.in_language(language).into(),
            Self::PngText => Text::MetadataSourcePngText.in_language(language).into(),
            Self::Xmp(format) => formatted::metadata_source_xmp(language, format.label()),
            Self::XmpLanguage(format, tag) => {
                formatted::metadata_source_xmp_language(language, format.label(), tag)
            }
            Self::XmpCreator(format, creator) => {
                formatted::metadata_source_xmp_creator(language, format.label(), *creator)
            }
        }
    }
}

impl std::fmt::Display for MetadataSourceScope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message(towavue_core::localization::Language::English))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetadataSourceValue {
    pub field: MetadataField,
    pub scope: MetadataSourceScope,
    pub value: String,
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageMetadataFormat {
    Png,
    Jpeg,
    Webp,
}

impl ImageMetadataFormat {
    pub fn from_path(path: &Path) -> Option<Self> {
        if png_metadata::png_path(path) {
            Some(Self::Png)
        } else if jpeg_metadata::jpeg_path(path) {
            Some(Self::Jpeg)
        } else if webp_metadata::webp_path(path) {
            Some(Self::Webp)
        } else {
            None
        }
    }

    pub fn fields(self) -> &'static [MetadataField] {
        match self {
            Self::Png => &MetadataField::ALL,
            Self::Jpeg | Self::Webp => &xmp::FIELDS,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Webp => "WebP",
        }
    }

    pub fn validate_options(self, options: &MetadataExportOptions) -> Result<(), ExportError> {
        if matches!(self, Self::Jpeg | Self::Webp) {
            xmp::apply(&mut Vec::new(), options)?;
        }
        Ok(())
    }
}

/// Reads bounded display values only; export Keep still copies the original tags.
pub fn read_export_metadata(
    path: &Path,
    kind: MediaKind,
) -> Result<Vec<MetadataSourceValue>, ExportError> {
    if kind == MediaKind::Image {
        return match ImageMetadataFormat::from_path(path) {
            Some(ImageMetadataFormat::Png) => png_metadata::inspect(path),
            Some(ImageMetadataFormat::Jpeg) => jpeg_metadata::inspect(path),
            Some(ImageMetadataFormat::Webp) => webp_metadata::inspect(path),
            None => Err(ExportError::Message(Text::ExportImageMetadataFormats)),
        };
    }
    ffmpeg::init().map_err(|error| ExportError::Failed(error.to_string()))?;
    let input =
        ffmpeg::format::input(path).map_err(|error| ExportError::Failed(error.to_string()))?;
    let mut values = Vec::new();
    let mut collect = |scope: MetadataSourceScope, tags: ffmpeg::DictionaryRef<'_>| {
        for field in MetadataField::ALL {
            if let Some((_, value)) = tags
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(field.key()))
            {
                values.push(MetadataSourceValue {
                    field,
                    scope: scope.clone(),
                    value: value[..value.floor_char_boundary(1024)].to_owned(),
                    truncated: value.len() > 1024,
                });
            }
        }
    };
    collect(MetadataSourceScope::File, input.metadata());
    if kind == MediaKind::Video
        && let Some(stream) = input.streams().best(ffmpeg::media::Type::Video)
    {
        collect(MetadataSourceScope::Video, stream.metadata());
    }
    if let Some(stream) = input.streams().best(ffmpeg::media::Type::Audio) {
        collect(MetadataSourceScope::Audio, stream.metadata());
    }
    Ok(values)
}

/// Absent fields keep source tags; empty values remove a field from the output.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MetadataExportOptions {
    fields: BTreeMap<MetadataField, String>,
}

impl MetadataExportOptions {
    pub fn get(&self, field: MetadataField) -> Option<&str> {
        self.fields.get(&field).map(String::as_str)
    }

    /// None restores Keep. Invalid updates leave the previous settings unchanged.
    pub fn set(&mut self, field: MetadataField, value: Option<String>) -> Result<(), ExportError> {
        if let Some(value) = &value {
            let other_bytes: usize = self
                .fields
                .iter()
                .filter(|(key, _)| **key != field)
                .map(|(_, value)| value.len())
                .sum();
            if value.contains('\0') || value.len() > 1024 || other_bytes + value.len() > 4096 {
                return Err(ExportError::Message(Text::ExportMetadataTextLimit));
            }
        }
        if let Some(value) = value {
            self.fields.insert(field, value);
        } else {
            self.fields.remove(&field);
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    pub(super) fn arguments(&self) -> Vec<String> {
        self.fields
            .iter()
            .flat_map(|(field, value)| {
                let tag = format!("{}={value}", field.key());
                ["-metadata:g".into(), tag.clone(), "-metadata:s".into(), tag]
            })
            .collect()
    }

    pub(super) fn verify(&self, path: &Path) -> Result<(), ExportError> {
        if self.is_empty() {
            return Ok(());
        }
        ffmpeg::init().map_err(|error| ExportError::Failed(error.to_string()))?;
        let input = ffmpeg::format::input(path).map_err(|error| {
            crate::ExportFailure::diagnostic(Text::ExportMetadataVerificationContext, error)
        })?;
        for (field, expected) in &self.fields {
            let mut values = Vec::new();
            for (key, value) in input.metadata().iter() {
                if key.eq_ignore_ascii_case(field.key()) {
                    values.push(value.to_owned());
                }
            }
            for stream in input.streams() {
                for (key, value) in stream.metadata().iter() {
                    if key.eq_ignore_ascii_case(field.key()) {
                        values.push(value.to_owned());
                    }
                }
            }
            let valid = if expected.is_empty() {
                values.iter().all(String::is_empty)
            } else {
                !values.is_empty() && values.iter().all(|value| value == expected)
            };
            if !valid {
                return Err(crate::ExportFailure::metadata_not_retained(*field).into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "export_metadata_tests.rs"]
mod tests;
