//! Structured format validation, separate from external codec diagnostics.
use crate::{ImageDecodeError, MetadataField};
use std::{error::Error, fmt};
use towavue_core::localization::{Language, Text, formatted};

#[derive(Debug)]
pub struct ExportFailure {
    context: Option<Text>,
    reason: Reason,
}

#[derive(Debug)]
enum Reason {
    Decode(Box<crate::DecodeError>),
    JpegValidation {
        status: String,
        detail: String,
    },
    ProcessExit(String),
    EmptyTrim {
        video: bool,
    },
    MetadataNotRetained(MetadataField),
    Geometry {
        operation: Text,
        expected_size: (u32, u32),
        expected_aspect: f32,
        size: (u32, u32),
        aspect: f32,
    },
    Loudness {
        target: (f64, f64),
        measured: (f64, f64),
        tolerance: f64,
        attempts: usize,
    },
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
    fn standalone(reason: Reason) -> Self {
        Self {
            context: None,
            reason,
        }
    }

    pub(crate) fn decode(context: Option<Text>, error: crate::DecodeError) -> Self {
        Self {
            context,
            reason: Reason::Decode(Box::new(error)),
        }
    }

    pub(crate) fn jpeg_validation(status: impl fmt::Display, detail: &str) -> Self {
        Self::standalone(Reason::JpegValidation {
            status: status.to_string(),
            detail: detail.into(),
        })
    }

    pub(crate) fn process_exit(status: impl fmt::Display) -> Self {
        Self::standalone(Reason::ProcessExit(status.to_string()))
    }

    pub(crate) fn empty_trim(video: bool) -> Self {
        Self::standalone(Reason::EmptyTrim { video })
    }

    pub(crate) fn metadata_not_retained(field: MetadataField) -> Self {
        Self::standalone(Reason::MetadataNotRetained(field))
    }

    pub(crate) fn geometry(
        operation: Text,
        expected_size: (u32, u32),
        expected_aspect: f32,
        size: (u32, u32),
        aspect: f32,
    ) -> Self {
        Self::standalone(Reason::Geometry {
            operation,
            expected_size,
            expected_aspect,
            size,
            aspect,
        })
    }

    pub(crate) fn loudness(
        target: (f64, f64),
        measured: (f64, f64),
        tolerance: f64,
        attempts: usize,
    ) -> Self {
        Self::standalone(Reason::Loudness {
            target,
            measured,
            tolerance,
            attempts,
        })
    }

    pub(crate) fn reason(context: Text, reason: Text) -> Self {
        Self {
            context: Some(context),
            reason: Reason::Text(reason),
        }
    }

    pub(crate) fn diagnostic(context: Text, error: impl fmt::Display) -> Self {
        Self {
            context: Some(context),
            reason: Reason::Diagnostic(error.to_string()),
        }
    }

    pub(crate) fn avif(context: Text, error: crate::AvifFailure) -> Self {
        Self {
            context: Some(context),
            reason: Reason::Avif(error),
        }
    }

    pub(crate) fn image(context: Text, error: ImageDecodeError) -> Self {
        Self {
            context: Some(context),
            reason: Reason::Image(Box::new(error)),
        }
    }

    pub(crate) fn prepared_image(context: Text, error: ImageDecodeError) -> Self {
        Self {
            context: Some(context),
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
            context: Some(context),
            reason: Reason::FrameDelay {
                milliseconds,
                format: format.into(),
                alternative,
            },
        }
    }

    pub(crate) fn unsupported_metadata(context: Text, field: MetadataField) -> Self {
        Self {
            context: Some(context),
            reason: Reason::UnsupportedMetadata(field),
        }
    }

    pub(crate) fn sequence_child(context: Text, child: [u8; 4], parent: [u8; 4]) -> Self {
        Self {
            context: Some(context),
            reason: Reason::SequenceChild { child, parent },
        }
    }

    pub fn message(&self, language: Language) -> String {
        let reason = match &self.reason {
            Reason::Decode(error) => error.message(language),
            Reason::JpegValidation { status, detail } => {
                formatted::export_jpeg_validation(language, status, detail)
            }
            Reason::ProcessExit(status) => formatted::export_process_exit(language, status),
            Reason::EmptyTrim { video: true } => {
                Text::ExportTrimVideoEmpty.in_language(language).into()
            }
            Reason::EmptyTrim { video: false } => {
                Text::ExportTrimAudioEmpty.in_language(language).into()
            }
            Reason::MetadataNotRetained(field) => formatted::export_metadata_not_retained(
                language,
                if language == Language::English {
                    field.key()
                } else {
                    field.label_in(language)
                },
            ),
            Reason::Geometry {
                operation,
                expected_size,
                expected_aspect,
                size,
                aspect,
            } => formatted::export_geometry_changed(
                language,
                operation.in_language(language),
                *expected_size,
                *expected_aspect,
                *size,
                *aspect,
            ),
            Reason::Loudness {
                target,
                measured,
                tolerance,
                attempts,
            } => formatted::export_loudness_refused(
                language, target.0, *tolerance, target.1, *attempts, measured.0, measured.1,
            ),
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
        match self.context {
            Some(context) => format!("{}: {reason}", context.in_language(language)),
            None => reason,
        }
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
            Reason::Decode(error) => Some(error.as_ref()),
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

#[cfg(test)]
mod validation_tests {
    use super::*;

    #[test]
    fn root_export_validation_keeps_numeric_refusals_and_literal_native_details() {
        for (error, english, japanese) in [
            (
                ExportFailure::jpeg_validation("exit code: 7", "codec {detail}: Ω"),
                "JPEG decode validation failed (exit code: 7): codec {detail}: Ω",
                "JPEGのデコード検証に失敗しました（exit code: 7）: codec {detail}: Ω",
            ),
            (
                ExportFailure::process_exit("exit code: 7"),
                "process exited with exit code: 7",
                "処理が終了しました: exit code: 7",
            ),
            (
                ExportFailure::empty_trim(false),
                "trim contains no Audio frames; choose a wider range",
                "トリミング範囲に音声フレームがありません。範囲を広げてください",
            ),
            (
                ExportFailure::empty_trim(true),
                "trim contains no Video frames; choose a wider range",
                "トリミング範囲に映像フレームがありません。範囲を広げてください",
            ),
            (
                ExportFailure::metadata_not_retained(MetadataField::AlbumArtist),
                "Output format did not retain the requested 'album_artist' metadata; existing target unchanged",
                "出力形式で指定した「アルバムアーティスト」メタデータを保持できませんでした。既存の保存先は変更していません",
            ),
            (
                ExportFailure::geometry(
                    Text::ExportRotationOperation,
                    (64, 48),
                    1.0,
                    (32, 48),
                    2.0,
                ),
                "Video rotation input changed: expected (64, 48) SAR 1, found (32, 48) SAR 2",
                "動画の回転の入力が変わりました。必要な値: (64, 48) SAR 1、実際の値: (32, 48) SAR 2",
            ),
            (
                ExportFailure::loudness((-14.0, -1.0), (-13.7, -0.8), 0.1, 4),
                "Encoded audio did not meet -14.0 LUFS (+/-0.1 LU) / maximum -1.0 dBTP after 4 attempts: measured -13.70 LUFS / -0.80 dBTP; nothing was published",
                "エンコード後の音声が4回の試行で-14.0 LUFS（±0.1 LU）／最大-1.0 dBTPを満たしませんでした。測定値: -13.70 LUFS／-0.80 dBTP。ファイルは保存していません",
            ),
        ] {
            assert_eq!(error.to_string(), english);
            assert_eq!(error.message(Language::Japanese), japanese);
            let export = crate::ExportError::from(error);
            assert!(export.message(Language::Japanese).contains(japanese));
        }
    }

    #[test]
    fn export_frame_and_orientation_failures_keep_decoder_causes_until_display() {
        let error = crate::VideoOrientation::from_bytes(Some(&[0; 3]))
            .expect_err("truncated display matrix is rejected");
        let diagnostic = error.to_string();
        let japanese = error.message(Language::Japanese);
        let failure = ExportFailure::decode(Some(Text::ExportHighDepthOrientationContext), error);
        assert_eq!(
            failure.to_string(),
            format!("Unsupported high-depth video orientation: {diagnostic}")
        );
        assert!(failure.message(Language::Japanese).contains(&japanese));
        assert!(matches!(
            failure
                .source()
                .and_then(|cause| cause.downcast_ref::<crate::DecodeError>()),
            Some(crate::DecodeError::UnsupportedOrientation)
        ));

        let error = crate::DecodeError::Ffmpeg(ffmpeg_next::Error::InvalidData);
        let diagnostic = error.to_string();
        let failure = ExportFailure::decode(None, error);
        assert_eq!(failure.to_string(), diagnostic);
        assert!(matches!(
            failure
                .source()
                .and_then(|cause| cause.downcast_ref::<crate::DecodeError>()),
            Some(crate::DecodeError::Ffmpeg(ffmpeg_next::Error::InvalidData))
        ));
    }
}
