use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MediaKind {
    Image,
    Video,
    Audio,
}

impl MediaKind {
    pub fn from_path(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "avif" | "bmp" | "gif" | "jpeg" | "jpg" | "png" | "tif" | "tiff" | "webp" => {
                Some(Self::Image)
            }
            "3gp" | "avi" | "m2ts" | "m4v" | "mkv" | "mov" | "mp4" | "mpeg" | "mpg" | "mts"
            | "ogv" | "ts" | "webm" | "wmv" => Some(Self::Video),
            "aac" | "aiff" | "alac" | "flac" | "m4a" | "mp3" | "oga" | "ogg" | "opus" | "wav"
            | "wma" => Some(Self::Audio),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_supported_media_without_case_sensitivity() {
        assert_eq!(
            MediaKind::from_path(Path::new("photo.JPEG")),
            Some(MediaKind::Image)
        );
        assert_eq!(
            MediaKind::from_path(Path::new("movie.MKV")),
            Some(MediaKind::Video)
        );
        assert_eq!(
            MediaKind::from_path(Path::new("track.FLAC")),
            Some(MediaKind::Audio)
        );
        assert_eq!(MediaKind::from_path(Path::new("notes.txt")), None);
    }
}
