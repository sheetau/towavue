//! Structured format validation, separate from external codec diagnostics.
use crate::{ImageDecodeError, MetadataField};
use std::{error::Error, fmt};
use towavue_core::localization::{Language, Text, formatted};

#[derive(Debug)]
pub struct ExportFailure {
    context: Text,
    reason: Reason,
}

#[derive(Debug)]
enum Reason {
    Text(Text),
    Diagnostic(String),
    Avif(crate::AvifFailure),
    Image(Box<ImageDecodeError>),
    PreparedImage(Box<ImageDecodeError>),
    FrameDelay {
        milliseconds: Option<u32>,
        format: String,
        alternative: &'static str,
    },
    UnsupportedMetadata(MetadataField),
    SequenceChild {
        child: [u8; 4],
        parent: [u8; 4],
    },
}

impl ExportFailure {
    pub(crate) fn reason(context: Text, reason: Text) -> Self {
        Self {
            context,
            reason: Reason::Text(reason),
        }
    }

    pub(crate) fn diagnostic(context: Text, error: impl fmt::Display) -> Self {
        Self {
            context,
            reason: Reason::Diagnostic(error.to_string()),
        }
    }

    pub(crate) fn avif(context: Text, error: crate::AvifFailure) -> Self {
        Self {
            context,
            reason: Reason::Avif(error),
        }
    }

    pub(crate) fn image(context: Text, error: ImageDecodeError) -> Self {
        Self {
            context,
            reason: Reason::Image(Box::new(error)),
        }
    }

    pub(crate) fn prepared_image(context: Text, error: ImageDecodeError) -> Self {
        Self {
            context,
            reason: Reason::PreparedImage(Box::new(error)),
        }
    }

    pub(crate) fn frame_delay(
        context: Text,
        milliseconds: Option<u32>,
        format: &str,
        alternative: &'static str,
    ) -> Self {
        Self {
            context,
            reason: Reason::FrameDelay {
                milliseconds,
                format: format.into(),
                alternative,
            },
        }
    }

    pub(crate) fn unsupported_metadata(context: Text, field: MetadataField) -> Self {
        Self {
            context,
            reason: Reason::UnsupportedMetadata(field),
        }
    }

    pub(crate) fn sequence_child(context: Text, child: [u8; 4], parent: [u8; 4]) -> Self {
        Self {
            context,
            reason: Reason::SequenceChild { child, parent },
        }
    }

    pub fn message(&self, language: Language) -> String {
        let reason = match &self.reason {
            Reason::Text(text) => text.in_language(language).into(),
            Reason::Diagnostic(detail) => detail.clone(),
            Reason::Avif(error) => error.message(language),
            Reason::Image(error) => error.message(language),
            Reason::PreparedImage(error) => {
                formatted::export_animation_preparation(language, &error.message(language))
            }
            Reason::FrameDelay {
                milliseconds,
                format,
                alternative,
            } => match milliseconds {
                Some(delay) => {
                    formatted::export_millisecond_delay(language, *delay, format, alternative)
                }
                None => formatted::export_exact_delay(language, format, alternative),
            },
            Reason::UnsupportedMetadata(field) => {
                formatted::export_xmp_unsupported_field(language, field.label_in(language))
            }
            Reason::SequenceChild { child, parent } => formatted::export_sequence_child(
                language,
                &String::from_utf8_lossy(child),
                &String::from_utf8_lossy(parent),
            ),
        };
        format!("{}: {reason}", self.context.in_language(language))
    }
}

impl fmt::Display for ExportFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message(Language::English))
    }
}

impl Error for ExportFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.reason {
            Reason::Image(error) | Reason::PreparedImage(error) => Some(error.as_ref()),
            Reason::Avif(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ExportError;

    #[test]
    fn structured_export_reasons_keep_english_diagnostics_and_format_advice() {
        for (failure, english, japanese_detail) in [
            (
                ExportFailure::reason(
                    Text::ExportPngMetadataContext,
                    Text::ExportValidationInputIsNotPng,
                ),
                "PNG metadata: input is not PNG",
                "入力がPNGではありません",
            ),
            (
                ExportFailure::frame_delay(Text::ExportGifContext, Some(1), "GIF", "WebP"),
                "GIF export: 1 ms frame delay cannot be represented exactly in GIF; use WebP output",
                "1 msのフレーム遅延をGIFで正確に表現できません。WebP出力",
            ),
            (
                ExportFailure::frame_delay(Text::ImageEditAvifContext, None, "GIF", "AVIF"),
                "AVIF export: frame delay cannot be represented exactly in GIF; use AVIF output",
                "GIFで正確に表現できません。AVIF出力",
            ),
            (
                ExportFailure::sequence_child(Text::ImageEditAvifContext, *b"xyz!", *b"moov"),
                "AVIF export: unexpected sequence child xyz! in moov",
                "moovに予期しない子要素xyz!",
            ),
            (
                ExportFailure::unsupported_metadata(
                    Text::ExportXmpMetadataContext,
                    MetadataField::AlbumArtist,
                ),
                "XMP metadata: 'Album artist' is not supported; XMP currently supports Title, Artist, Album, Composer, Genre, Date, Track, Comment and Copyright",
                "アルバムアーティスト",
            ),
        ] {
            assert_eq!(failure.to_string(), english);
            assert_eq!(failure.message(Language::English), english);
            assert!(
                failure
                    .message(Language::Japanese)
                    .contains(japanese_detail)
            );
            let error = ExportError::from(failure);
            assert_eq!(
                error.to_string(),
                format!("FFmpeg export failed: {english}")
            );
            assert_eq!(error.message(Language::English), error.to_string());
            assert!(error.message(Language::Japanese).contains(japanese_detail));
            assert!(
                crate::SourceSaveError::Export(error)
                    .message(Language::Japanese)
                    .contains(japanese_detail)
            );
        }
    }

    #[test]
    fn image_preparation_retains_native_cause_and_literal_diagnostic_content() {
        let native = std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "native {detail} 日本語.png",
        );
        let failure = ExportFailure::prepared_image(
            Text::ExportPngMetadataContext,
            ImageDecodeError::Open(native),
        );
        assert_eq!(
            failure.to_string(),
            "PNG metadata: animation frame preparation failed: could not open image: native {detail} 日本語.png"
        );
        let translated = failure.message(Language::Japanese);
        assert!(translated.contains("アニメーションフレームの準備に失敗しました"));
        assert!(translated.contains("native {detail} 日本語.png"));
        assert!(!translated.contains("could not open image"));
        let native = failure
            .source()
            .expect("image cause")
            .source()
            .expect("native cause")
            .downcast_ref::<std::io::Error>()
            .expect("original I/O error");
        assert_eq!(native.kind(), std::io::ErrorKind::PermissionDenied);
        let literal = "native {detail}\n日本語.xml = 32";
        let failure = ExportFailure::diagnostic(Text::ExportXmpMetadataContext, literal);
        assert_eq!(
            failure.message(Language::Japanese),
            format!("XMPメタデータ: {literal}")
        );
    }
}
