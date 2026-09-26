//! Translate owned I/O causes without changing their kind, source or diagnostic text.
use std::{io, path::PathBuf};
use towavue_core::localization::{Language, formatted};

#[derive(Debug, thiserror::Error)]
pub(crate) enum PathReason {
    #[error("required media helper is missing: {}", .0.display())]
    MissingMediaHelper(PathBuf),
    #[error("Packaged licenses and sources are unavailable. Expected: {}", .0.display())]
    MissingLicenseGuide(PathBuf),
}

/// Unknown/native causes stay literal; recognized causes retain structured data
/// until the receiving UI selects a language. Never translate by matching text.
pub fn io_error_message(error: &io::Error, language: Language) -> String {
    let Some(cause) = error.get_ref() else {
        return error.to_string();
    };
    if let Some(reason) = cause.downcast_ref::<crate::RecoveryDetail>() {
        return reason.message(language);
    }
    if let Some(reason) = cause.downcast_ref::<crate::FileOperationError>() {
        return reason.message(language);
    }
    if let Some(reason) = cause.downcast_ref::<crate::shell::FolderOrderError>() {
        return reason.message(language);
    }
    if let Some(reason) = cause.downcast_ref::<PathReason>() {
        return match reason {
            PathReason::MissingMediaHelper(path) => {
                formatted::missing_media_helper(language, &path.display().to_string())
            }
            PathReason::MissingLicenseGuide(path) => {
                formatted::missing_license_guide(language, &path.display().to_string())
            }
        };
    }
    if let Some(inner) = cause.downcast_ref::<io::Error>() {
        return io_error_message(inner, language);
    }
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;
    use towavue_core::localization::Text;

    #[test]
    fn owned_io_preserves_nested_kinds_paths_and_literal_native_details() {
        let original = io::Error::new(
            io::ErrorKind::AlreadyExists,
            crate::RecoveryDetail::from(Text::SaveDeletedPathOccupied),
        );
        let failure = crate::SourceSaveError::Io(original);
        let cause = failure
            .source()
            .expect("original I/O")
            .downcast_ref::<io::Error>()
            .expect("kind retained");
        assert_eq!(cause.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            failure.to_string(),
            "source save failed: the deleted document's path is occupied; use Export as or choose another location"
        );
        assert_eq!(
            failure.message(Language::Japanese),
            "上書き保存に失敗しました: 削除したファイルのパスに別のファイルがあります。書き出すか、別の保存先を選んでください"
        );
        let path = PathBuf::from("C:/owned/日本語 {helper}/ffmpeg.exe");
        let helper = io::Error::new(
            io::ErrorKind::NotFound,
            PathReason::MissingMediaHelper(path.clone()),
        );
        let wrapped = crate::ExportError::Start(helper);
        assert_eq!(
            wrapped.message(Language::Japanese),
            format!(
                "FFmpegの書き出しを開始できませんでした: 必要なメディア補助プログラムが見つかりません: {}",
                path.display()
            )
        );
        let nested = io::Error::other(io::Error::other(
            crate::shell::FolderOrderError::ResponseLost,
        ));
        assert_eq!(
            io_error_message(&nested, Language::Japanese),
            "エクスプローラー連携の処理からフォルダー情報を取得できませんでした"
        );
        let file = io::Error::other(crate::FileOperationError::DestinationExists);
        assert_eq!(
            io_error_message(&file, Language::Japanese),
            Text::FileDestinationExists.in_language(Language::Japanese)
        );
        // Identical text from an unrecognized native source is still diagnostic data.
        for detail in [
            "destination folder is unavailable",
            "the Shell worker stopped",
            "native {detail}\n日本語",
        ] {
            let native = io::Error::new(io::ErrorKind::PermissionDenied, detail);
            assert_eq!(io_error_message(&native, Language::Japanese), detail);
            assert_eq!(native.kind(), io::ErrorKind::PermissionDenied);
        }
    }
}
