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
        match self {
            Self::Title => "Title",
            Self::Artist => "Artist",
            Self::Album => "Album",
            Self::AlbumArtist => "Album artist",
            Self::Composer => "Composer",
            Self::Genre => "Genre",
            Self::Date => "Date",
            Self::Track => "Track",
            Self::Comment => "Comment",
            Self::Copyright => "Copyright",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetadataSourceValue {
    pub field: MetadataField,
    pub scope: &'static str,
    pub value: String,
    pub truncated: bool,
}

/// Reads bounded display values only; export Keep still copies the original tags.
pub fn read_export_metadata(
    path: &Path,
    kind: MediaKind,
) -> Result<Vec<MetadataSourceValue>, ExportError> {
    if kind == MediaKind::Image {
        return png_metadata::inspect(path);
    }
    ffmpeg::init().map_err(|error| ExportError::Failed(error.to_string()))?;
    let input =
        ffmpeg::format::input(path).map_err(|error| ExportError::Failed(error.to_string()))?;
    let mut values = Vec::new();
    let mut collect = |scope, tags: ffmpeg::DictionaryRef<'_>| {
        for field in MetadataField::ALL {
            if let Some((_, value)) = tags
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(field.key()))
            {
                values.push(MetadataSourceValue {
                    field,
                    scope,
                    value: value[..value.floor_char_boundary(1024)].to_owned(),
                    truncated: value.len() > 1024,
                });
            }
        }
    };
    collect("File", input.metadata());
    if kind == MediaKind::Video
        && let Some(stream) = input.streams().best(ffmpeg::media::Type::Video)
    {
        collect("Video", stream.metadata());
    }
    if let Some(stream) = input.streams().best(ffmpeg::media::Type::Audio) {
        collect("Audio", stream.metadata());
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
                return Err(ExportError::Failed(
                    "Metadata text must contain no NUL and fit 1024 UTF-8 bytes per field / 4096 total".into(),
                ));
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
            ExportError::Failed(format!("Could not verify exported metadata: {error}"))
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
                return Err(ExportError::Failed(format!(
                    "Output format did not retain the requested '{}' metadata; existing target unchanged",
                    field.key(),
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "export_metadata_tests.rs"]
mod tests;
