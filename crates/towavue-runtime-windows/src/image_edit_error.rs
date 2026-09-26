use std::sync::Arc;
use towavue_core::localization::{Language, Text, formatted};

/// Image-edit reasons remain structured until a validated result reaches the UI.
/// Native filter errors retain their original cause and English diagnostic.
#[derive(Clone, Debug, thiserror::Error)]
pub enum ImageEditError {
    #[error("{}", .0.in_language(Language::English))]
    Message(Text),
    #[error("Could not resample image: {0}")]
    Resample(#[source] Arc<ffmpeg_next::Error>),
    #[error("{0}")]
    Detail(String),
}

impl ImageEditError {
    pub fn message(&self, language: Language) -> String {
        match self {
            Self::Message(text) => text.in_language(language).into(),
            Self::Resample(error) => formatted::image_resample_reason(language, &error.to_string()),
            Self::Detail(detail) => detail.clone(),
        }
    }
}

impl From<Text> for ImageEditError {
    fn from(text: Text) -> Self {
        Self::Message(text)
    }
}

impl From<ffmpeg_next::Error> for ImageEditError {
    fn from(error: ffmpeg_next::Error) -> Self {
        Self::Resample(Arc::new(error))
    }
}

impl From<String> for ImageEditError {
    fn from(detail: String) -> Self {
        Self::Detail(detail)
    }
}

impl From<&str> for ImageEditError {
    fn from(detail: &str) -> Self {
        Self::Detail(detail.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn image_edit_reasons_preserve_diagnostics_causes_and_nested_export_translation() {
        for (text, english) in [
            (
                Text::ImageEditCompareVideo,
                "Video raster edits cannot be compared as image frames",
            ),
            (
                Text::ImageEditApplyVideo,
                "Video raster edits cannot be applied to image frames",
            ),
            (Text::ImageComparisonCancelled, "Image comparison cancelled"),
            (Text::ImageEditCancelled, "Image edit cancelled"),
            (Text::ImageResampleBudget, "Resampled image exceeds 512 MiB"),
            (
                Text::ImageEditInvalidDimensions,
                "Invalid source image dimensions",
            ),
            (
                Text::ImageEditRotationSizeChanged,
                "Image rotation input dimensions changed",
            ),
            (Text::ImageEditInvalidCrop, "Invalid image crop"),
            (Text::ImageEditInvalidPixels, "Invalid source image pixels"),
        ] {
            let error = ImageEditError::from(text);
            assert_eq!(error.to_string(), english);
            assert_eq!(error.message(Language::English), english);
            assert_ne!(error.message(Language::Japanese), english);
            let export = crate::ExportError::ImageEdit(Text::ImageEditAvifContext, error);
            assert_eq!(
                export.to_string(),
                format!("FFmpeg export failed: AVIF export: {english}")
            );
            assert_eq!(export.message(Language::English), export.to_string());
            assert!(export.source().is_some());
            let expected = format!(
                "FFmpegの書き出しに失敗しました: AVIFの書き出し: {}",
                text.in_language(Language::Japanese)
            );
            assert_eq!(export.message(Language::Japanese), expected);
            let saved = crate::SourceSaveError::Export(export);
            assert!(saved.message(Language::Japanese).contains(&expected));
        }
        let native = Arc::new(ffmpeg_next::Error::InvalidData);
        let error = ImageEditError::Resample(Arc::clone(&native));
        assert_eq!(
            error.to_string(),
            format!("Could not resample image: {native}")
        );
        assert_eq!(
            error.source().expect("native cause").to_string(),
            native.to_string()
        );
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || sender.send(error.clone()).expect("send typed failure"))
            .join()
            .expect("worker");
        let received = receiver.recv().expect("typed failure");
        let ImageEditError::Resample(retained) = &received else {
            panic!("retained native cause")
        };
        assert!(Arc::ptr_eq(retained, &native));
        assert_eq!(
            received.message(Language::Japanese),
            format!("画像を再サンプリングできません: {native}")
        );
        let detail = "native {error}\n日本語.png = 32";
        let raw = ImageEditError::from(detail);
        assert_eq!(raw.message(Language::Japanese), detail);
        let export = crate::ExportError::ImageEdit(Text::ImageEditWebpContext, raw);
        assert_eq!(
            export.to_string(),
            format!("FFmpeg export failed: WebP metadata: {detail}")
        );
        assert_eq!(
            export.message(Language::Japanese),
            format!("FFmpegの書き出しに失敗しました: WebP画像の処理: {detail}")
        );
    }
}
