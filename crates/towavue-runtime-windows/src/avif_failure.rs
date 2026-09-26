//! AVIF validation shared by image decoding and export, before display localization.
use std::fmt;
use towavue_core::localization::{Language, Text, formatted};

#[derive(Debug)]
pub enum AvifFailure {
    Message(Text),
    DuplicateBox([u8; 4]),
    Diagnostic(String),
}

impl AvifFailure {
    pub fn message(&self, language: Language) -> String {
        match self {
            Self::Message(text) => text.in_language(language).into(),
            Self::DuplicateBox(kind) => {
                formatted::avif_duplicate_box(language, &String::from_utf8_lossy(kind))
            }
            Self::Diagnostic(detail) => detail.clone(),
        }
    }
}

impl From<String> for AvifFailure {
    fn from(detail: String) -> Self {
        Self::Diagnostic(detail)
    }
}

impl From<&str> for AvifFailure {
    fn from(detail: &str) -> Self {
        Self::Diagnostic(detail.into())
    }
}

impl fmt::Display for AvifFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message(Language::English))
    }
}

impl std::error::Error for AvifFailure {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ExportError, ImageDecodeError, avif_container as container};
    use std::error::Error;

    #[test]
    fn actual_avif_validation_keeps_typed_causes_for_image_and_export() {
        let invalid_aperture = || {
            container::CleanAperture::from_clap(&[0; 31], (7, 5))
                .expect_err("truncated aperture must be rejected")
        };
        let image = ImageDecodeError::from(invalid_aperture());
        assert_eq!(
            image.to_string(),
            "could not decode AVIF: invalid clap size"
        );
        assert!(
            image
                .message(Language::Japanese)
                .contains("clapのサイズが不正です")
        );
        assert!(
            image
                .source()
                .expect("image retains its validation cause")
                .downcast_ref::<AvifFailure>()
                .is_some()
        );

        let export = ExportError::from(invalid_aperture());
        assert_eq!(
            export.to_string(),
            "FFmpeg export failed: AVIF export: invalid clap size"
        );
        assert!(
            export
                .message(Language::Japanese)
                .contains("clapのサイズが不正です")
        );
        assert!(
            export
                .source()
                .expect("export retains its format context")
                .source()
                .expect("format context retains its AVIF cause")
                .downcast_ref::<AvifFailure>()
                .is_some()
        );

        let kind = *b"m{ta";
        let item = container::BoxRange {
            kind,
            header: 0,
            start: 8,
            end: 8,
        };
        let Err(error) = container::one(&[item, item], &kind) else {
            panic!("duplicate boxes must be rejected");
        };
        assert_eq!(error.to_string(), "duplicate m{ta box");
        let image = ImageDecodeError::from(error);
        assert!(
            image
                .message(Language::Japanese)
                .contains("m{taボックスが重複しています")
        );
        let raw = ImageDecodeError::Avif("native {detail}: Ω".into());
        assert!(
            raw.message(Language::Japanese)
                .contains("native {detail}: Ω")
        );
    }

    #[test]
    fn avif_routing_preserves_cancellation_and_original_io_error() {
        assert!(matches!(
            ImageDecodeError::from(container::Error::Cancelled),
            ImageDecodeError::Cancelled
        ));
        assert!(matches!(
            ExportError::from(container::Error::Cancelled),
            ExportError::Cancelled
        ));
        let io = || std::io::Error::new(std::io::ErrorKind::PermissionDenied, "native {path}: Ω");
        let image = ImageDecodeError::from(container::Error::Io(io()));
        let native = image
            .source()
            .expect("image retains its I/O cause")
            .downcast_ref::<std::io::Error>()
            .expect("image cause remains the original I/O error type");
        assert_eq!(native.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(native.to_string(), "native {path}: Ω");
        let export = ExportError::from(container::Error::Io(io()));
        assert_eq!(
            export.to_string(),
            "FFmpeg export failed: AVIF export: native {path}: Ω"
        );
        assert!(
            export
                .message(Language::Japanese)
                .contains("native {path}: Ω")
        );
    }
}
