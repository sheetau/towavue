//! Owned frame-extraction reasons remain typed across decoder/export workers.
use std::fmt;
use towavue_core::localization::{Language, Text, formatted};

#[derive(Debug)]
pub enum FrameImageFailure {
    Message(Text),
    UnexpectedFrame {
        actual_size: (u32, u32),
        actual_format: String,
        expected_size: (u32, u32),
        expected_format: String,
    },
    Diagnostic(String),
}

impl FrameImageFailure {
    pub fn message(&self, language: Language) -> String {
        match self {
            Self::Message(text) => text.in_language(language).into(),
            Self::UnexpectedFrame {
                actual_size,
                actual_format,
                expected_size,
                expected_format,
            } => formatted::frame_image_unexpected(
                language,
                actual_size.0,
                actual_size.1,
                actual_format,
                expected_size.0,
                expected_size.1,
                expected_format,
            ),
            Self::Diagnostic(detail) => detail.clone(),
        }
    }
}

impl From<String> for FrameImageFailure {
    fn from(detail: String) -> Self {
        Self::Diagnostic(detail)
    }
}

impl From<&str> for FrameImageFailure {
    fn from(detail: &str) -> Self {
        Self::Diagnostic(detail.into())
    }
}

impl fmt::Display for FrameImageFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message(Language::English))
    }
}

impl std::error::Error for FrameImageFailure {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn frame_geometry_and_native_diagnostics_survive_decoder_and_export_boundaries() {
        let reason = FrameImageFailure::UnexpectedFrame {
            actual_size: (63, 47),
            actual_format: "RGB48LE".into(),
            expected_size: (64, 48),
            expected_format: "RGBA64LE".into(),
        };
        let english = "unexpected edited frame: 63x47 RGB48LE, expected 64x48 RGBA64LE";
        assert_eq!(reason.to_string(), english);
        assert_eq!(
            reason.message(Language::Japanese),
            "編集後のフレームが一致しません。実際の値: 63x47 RGB48LE、必要な値: 64x48 RGBA64LE"
        );
        let decode = crate::DecodeError::FrameImage(reason);
        assert_eq!(
            decode.to_string(),
            format!("could not extract video frame: {english}")
        );
        assert!(
            decode
                .source()
                .expect("frame reason")
                .downcast_ref::<FrameImageFailure>()
                .is_some()
        );
        let export = crate::ExportError::from(crate::ExportFailure::decode(None, decode));
        assert!(export.message(Language::Japanese).contains("63x47 RGB48LE"));
        assert!(
            export
                .message(Language::Japanese)
                .contains("64x48 RGBA64LE")
        );
        let detail = crate::DecodeError::FrameImage("native {detail}: Ω".into());
        assert!(
            detail
                .message(Language::Japanese)
                .contains("native {detail}: Ω")
        );
    }
}
