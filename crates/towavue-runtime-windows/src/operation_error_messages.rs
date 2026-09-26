//! Localized operation errors; Display remains the stable English diagnostic.
use crate::{DecodeError, ExportError, FileOperationError, ImageDecodeError, SourceSaveError};
use towavue_core::localization::{Language, Text, formatted};

impl DecodeError {
    pub fn message(&self, language: Language) -> String {
        match self {
            Self::FrameImage(error) => formatted::frame_extract_failed(language, error),
            Self::MissingVideoTimestamp => {
                Text::DecodeMissingTimestamp.in_language(language).into()
            }
            Self::UnsupportedOrientation => Text::DecodeUnsupportedOrientation
                .in_language(language)
                .into(),
            Self::Ffmpeg(error) => formatted::ffmpeg_decode_failed(language, &error.to_string()),
            Self::NoMediaStream => Text::DecodeNoStream.in_language(language).into(),
            Self::FrameTooLarge => Text::DecodeFrameTooLarge.in_language(language).into(),
            Self::ConsumerClosed => Text::DecodeConsumerStopped.in_language(language).into(),
            Self::HardwareUnavailable(error) => {
                formatted::hardware_decode_unavailable(language, error)
            }
            Self::WorkerStart(error) => {
                formatted::decode_worker_start_failed(language, &error.to_string())
            }
            Self::WorkerPanicked => Text::DecodeWorkerPanicked.in_language(language).into(),
        }
    }
}

impl ImageDecodeError {
    pub fn message(&self, language: Language) -> String {
        match self {
            Self::Open(error) => formatted::image_open_failed(language, &error.to_string()),
            Self::Decode(error) => formatted::image_decode_failed(language, &error.to_string()),
            Self::Gif(error) => formatted::gif_decode_failed(language, &error.to_string()),
            Self::Ffmpeg(error) => {
                formatted::ffmpeg_image_decode_failed(language, &error.message(language))
            }
            Self::Avif(error) => formatted::avif_decode_failed(language, error),
            Self::Png(error) => formatted::png_decode_failed(language, &error.to_string()),
            Self::PngEncode(error) => formatted::apng_encode_failed(language, &error.to_string()),
            Self::UnknownFormat => Text::ImageFormatUnknown.in_language(language).into(),
            Self::Empty => Text::ImageDecodedEmpty.in_language(language).into(),
            Self::TooLarge => Text::ImageMemoryBudget.in_language(language).into(),
            Self::Cancelled => Text::ImageSuperseded.in_language(language).into(),
        }
    }
}

impl ExportError {
    pub fn message(&self, language: Language) -> String {
        match self {
            Self::SameAsSource => Text::ExportSameAsSource.in_language(language).into(),
            Self::InvalidTrim => Text::ExportInvalidTrim.in_language(language).into(),
            Self::InvalidTimeline => Text::ExportInvalidTimeline.in_language(language).into(),
            Self::Start(error) => formatted::export_start_failed(language, &error.to_string()),
            Self::Failed(error) => formatted::ffmpeg_export_failed(language, error),
            Self::Message(text) => {
                formatted::ffmpeg_export_failed(language, text.in_language(language))
            }
            Self::Cancelled => Text::ExportCancelledUnchanged.in_language(language).into(),
            Self::Output(error) => formatted::export_output_failed(language, &error.to_string()),
        }
    }
}

impl FileOperationError {
    pub fn message(&self, language: Language) -> String {
        match self {
            Self::Io(error) => formatted::file_operation_failed(language, &error.to_string()),
            Self::Windows(error) => {
                formatted::windows_file_operation_failed(language, &error.to_string())
            }
            Self::SourceChanged => Text::FileSourceChanged.in_language(language).into(),
            Self::NotRegularFile => Text::FileNotRegular.in_language(language).into(),
            Self::InvalidName => Text::FileInvalidName.in_language(language).into(),
            Self::DestinationExists => Text::FileDestinationExists.in_language(language).into(),
            Self::NotCompleted => Text::FileOperationIncomplete.in_language(language).into(),
            Self::WorkerStopped => Text::FileWorkerStopped.in_language(language).into(),
            Self::RecoveryRequired { message, directory } => formatted::file_deletion_recovery(
                language,
                message,
                &directory.display().to_string(),
            ),
        }
    }
}

impl SourceSaveError {
    pub fn message(&self, language: Language) -> String {
        match self {
            Self::Io(error) => formatted::source_save_failed(language, &error.to_string()),
            Self::Source(error) => error.message(language),
            Self::Export(error) => error.message(language),
            Self::InvalidRequest => Text::SourceSaveInvalidRequest.in_language(language).into(),
            Self::RecoveryRequired { message, directory } => {
                formatted::source_replacement_recovery(
                    language,
                    message,
                    &directory.display().to_string(),
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_errors_keep_english_diagnostics_and_localize_nested_image_failures() {
        let detail = "native {detail}\n日本語.png = 32";
        for error in [
            DecodeError::FrameImage(detail.into()),
            DecodeError::MissingVideoTimestamp,
            DecodeError::UnsupportedOrientation,
            DecodeError::Ffmpeg(ffmpeg_next::Error::InvalidData),
            DecodeError::NoMediaStream,
            DecodeError::FrameTooLarge,
            DecodeError::ConsumerClosed,
            DecodeError::HardwareUnavailable(detail.into()),
            DecodeError::WorkerStart(std::io::Error::other(detail)),
            DecodeError::WorkerPanicked,
        ] {
            let diagnostic = error.to_string();
            let japanese = error.message(Language::Japanese);
            assert_eq!(error.message(Language::English), diagnostic);
            assert_ne!(japanese, diagnostic);
            if diagnostic.contains(detail) {
                assert!(japanese.contains(detail), "external details are literal");
            }
            let nested = ImageDecodeError::Ffmpeg(error);
            assert_eq!(nested.message(Language::English), nested.to_string());
            assert_eq!(
                nested.message(Language::Japanese),
                format!("FFmpegで画像を読み込めません: {japanese}")
            );
        }
        for error in [
            ImageDecodeError::Open(std::io::Error::other(detail)),
            ImageDecodeError::Decode(image::ImageError::IoError(std::io::Error::other(detail))),
            ImageDecodeError::Avif(detail.into()),
            ImageDecodeError::UnknownFormat,
            ImageDecodeError::Empty,
            ImageDecodeError::TooLarge,
            ImageDecodeError::Cancelled,
        ] {
            assert_eq!(error.message(Language::English), error.to_string());
            assert_ne!(error.message(Language::Japanese), error.to_string());
            if error.to_string().contains(detail) {
                assert!(error.message(Language::Japanese).contains(detail));
            }
        }
    }

    #[test]
    fn operation_errors_keep_english_diagnostics_and_nested_japanese_details() {
        let detail = "native {detail}\n日本語 = 32";
        let directory = std::path::PathBuf::from(r"D:\日本語 {original}\recovery");
        let io = || std::io::Error::other(detail);
        let file_errors = [
            FileOperationError::Io(io()),
            FileOperationError::Windows(windows::core::Error::from_hresult(
                windows::core::HRESULT(0x80004005_u32 as i32),
            )),
            FileOperationError::SourceChanged,
            FileOperationError::NotRegularFile,
            FileOperationError::InvalidName,
            FileOperationError::DestinationExists,
            FileOperationError::NotCompleted,
            FileOperationError::WorkerStopped,
            FileOperationError::RecoveryRequired {
                message: detail.into(),
                directory: directory.clone(),
            },
        ];
        for error in file_errors {
            let english = error.to_string();
            assert_eq!(error.message(Language::English), english);
            assert_ne!(error.message(Language::Japanese), english);
            let nested = SourceSaveError::Source(error);
            assert_eq!(nested.message(Language::English), english);
            assert_eq!(nested.to_string(), english);
        }
        for error in [
            ExportError::SameAsSource,
            ExportError::InvalidTrim,
            ExportError::InvalidTimeline,
            ExportError::Start(io()),
            ExportError::Failed(detail.into()),
            ExportError::Message(Text::ExportFrameSourceChanged),
            ExportError::Cancelled,
            ExportError::Output(io()),
        ] {
            let english = error.to_string();
            let japanese = error.message(Language::Japanese);
            assert_eq!(error.message(Language::English), english);
            assert_ne!(japanese, english);
            let nested = SourceSaveError::Export(error);
            assert_eq!(nested.message(Language::Japanese), japanese);
            assert_eq!(nested.message(Language::English), nested.to_string());
        }
        for error in [
            SourceSaveError::Io(io()),
            SourceSaveError::InvalidRequest,
            SourceSaveError::RecoveryRequired {
                message: detail.into(),
                directory: directory.clone(),
            },
        ] {
            assert_eq!(error.message(Language::English), error.to_string());
            assert_ne!(error.message(Language::Japanese), error.to_string());
        }
        let deletion = FileOperationError::RecoveryRequired {
            message: detail.into(),
            directory: directory.clone(),
        };
        assert_eq!(
            SourceSaveError::Source(deletion).message(Language::Japanese),
            format!(
                "ファイルの削除に失敗し、元ファイルの状態を確認できません: {detail}。保持したファイルのフォルダー: {}",
                directory.display()
            )
        );
        let replacement = SourceSaveError::RecoveryRequired {
            message: detail.into(),
            directory: directory.clone(),
        };
        assert_eq!(
            replacement.message(Language::Japanese),
            format!(
                "元ファイルの置き換えに復旧が必要です: {detail}。保持したファイル: {}",
                directory.display()
            )
        );
        assert_eq!(
            SourceSaveError::Export(ExportError::Failed(detail.into())).message(Language::Japanese),
            format!("FFmpegの書き出しに失敗しました: {detail}")
        );
        assert_eq!(
            SourceSaveError::Io(io()).message(Language::Japanese),
            format!("上書き保存に失敗しました: {detail}")
        );
    }
}
