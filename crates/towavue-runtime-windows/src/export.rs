use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;
use towavue_core::{EditOperation, EditState, MediaKind};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Debug)]
pub struct ExportRequest {
    pub source: PathBuf,
    pub target: PathBuf,
    pub kind: MediaKind,
    pub operations: Vec<EditOperation>,
}

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("export target must differ from the source path")]
    SameAsSource,
    #[error("trim start must be earlier than trim end")]
    InvalidTrim,
    #[error("could not start FFmpeg export: {0}")]
    Start(#[source] std::io::Error),
    #[error("FFmpeg export failed: {0}")]
    Failed(String),
}

pub fn export_media(request: &ExportRequest) -> Result<(), ExportError> {
    if same_path(&request.source, &request.target) {
        return Err(ExportError::SameAsSource);
    }
    let state = EditState::from_operations(&request.operations);
    if state.trim_start.is_some() && state.trim_end.is_some() && state.valid_trim().is_none() {
        return Err(ExportError::InvalidTrim);
    }
    let executable = std::env::var_os("FFMPEG_DIR")
        .map(PathBuf::from)
        .map(|directory| directory.join("bin").join("ffmpeg.exe"))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("ffmpeg.exe"));
    let arguments = ffmpeg_arguments(request);
    let output = <Command as std::os::windows::process::CommandExt>::creation_flags(
        Command::new(executable).args(arguments),
        CREATE_NO_WINDOW,
    )
    .output()
    .map_err(ExportError::Start)?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(ExportError::Failed(if message.is_empty() {
            format!("process exited with {}", output.status)
        } else {
            message
        }));
    }
    Ok(())
}

fn ffmpeg_arguments(request: &ExportRequest) -> Vec<String> {
    let state = EditState::from_operations(&request.operations);
    let mut arguments = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-y".into(),
        "-i".into(),
        request.source.display().to_string(),
        "-map_metadata".into(),
        "0".into(),
    ];
    let visual = visual_filters(&request.operations);
    match request.kind {
        MediaKind::Image => {
            if !visual.is_empty() {
                arguments.extend(["-vf".into(), visual.join(",")]);
            }
            arguments.extend(["-frames:v".into(), "1".into()]);
        }
        MediaKind::Video => {
            let video = video_filters(visual, &state);
            if !video.is_empty() {
                arguments.extend(["-vf".into(), video.join(",")]);
            }
            let audio = audio_filters(&state);
            if !audio.is_empty() {
                arguments.extend(["-af".into(), audio.join(",")]);
            }
        }
        MediaKind::Audio => {
            let audio = audio_filters(&state);
            if !audio.is_empty() {
                arguments.extend(["-af".into(), audio.join(",")]);
            }
        }
    }
    arguments.extend(codec_arguments(request));
    arguments.push(request.target.display().to_string());
    arguments
}

fn codec_arguments(request: &ExportRequest) -> Vec<String> {
    let extension = request
        .target
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let codecs: &[&str] = match (request.kind, extension.as_str()) {
        (MediaKind::Video, "webm") => &["-c:v", "libvpx-vp9", "-c:a", "libopus"],
        (MediaKind::Video, "avi") => &["-c:v", "mpeg4", "-c:a", "pcm_s16le"],
        (MediaKind::Video, "wmv") => &["-c:v", "wmv2", "-c:a", "wmav2"],
        (MediaKind::Video, _) => &["-c:v", "libopenh264", "-c:a", "aac"],
        (MediaKind::Audio, "mp3") => &["-c:a", "libmp3lame"],
        (MediaKind::Audio, "ogg" | "opus") => &["-c:a", "libopus"],
        (MediaKind::Audio, "wav") => &["-c:a", "pcm_s16le"],
        (MediaKind::Audio, "flac") => &["-c:a", "flac"],
        (MediaKind::Audio, _) => &["-c:a", "aac"],
        (MediaKind::Image, "avif") => &["-c:v", "libaom-av1", "-still-picture", "1"],
        (MediaKind::Image, "webp") => &["-c:v", "libwebp"],
        (MediaKind::Image, "jpg" | "jpeg") => &["-c:v", "mjpeg"],
        (MediaKind::Image, "png") => &["-c:v", "png"],
        (MediaKind::Image, "gif") => &["-c:v", "gif"],
        (MediaKind::Image, "tif" | "tiff") => &["-c:v", "tiff"],
        (MediaKind::Image, "bmp") => &["-c:v", "bmp"],
        (MediaKind::Image, _) => &[],
    };
    codecs.iter().map(|argument| (*argument).into()).collect()
}

fn visual_filters(operations: &[EditOperation]) -> Vec<String> {
    operations
        .iter()
        .filter_map(|operation| match *operation {
            EditOperation::Crop(region) => Some(format!(
                "crop=iw*{:.8}:ih*{:.8}:iw*{:.8}:ih*{:.8}",
                region.width(),
                region.height(),
                region.min.x,
                region.min.y
            )),
            EditOperation::RotateClockwise => Some("transpose=clock".into()),
            EditOperation::RotateCounterclockwise => Some("transpose=cclock".into()),
            EditOperation::FlipHorizontal => Some("hflip".into()),
            EditOperation::FlipVertical => Some("vflip".into()),
            EditOperation::SetTrimStart(_)
            | EditOperation::SetTrimEnd(_)
            | EditOperation::SetVolume(_)
            | EditOperation::SetRate(_) => None,
        })
        .collect()
}

fn video_filters(mut filters: Vec<String>, state: &EditState) -> Vec<String> {
    if let Some(trim) = trim_filter("trim", state) {
        filters.push(trim);
    }
    if state.trim_start.is_some() || state.trim_end.is_some() || state.rate != 1.0 {
        filters.push(if state.rate == 1.0 {
            "setpts=PTS-STARTPTS".into()
        } else {
            format!("setpts=(PTS-STARTPTS)/{:.4}", state.rate)
        });
    }
    filters
}

fn audio_filters(state: &EditState) -> Vec<String> {
    let mut filters = Vec::new();
    if let Some(trim) = trim_filter("atrim", state) {
        filters.push(trim);
        filters.push("asetpts=PTS-STARTPTS".into());
    }
    if state.rate != 1.0 {
        filters.extend(atempo_filters(state.rate));
    }
    if state.volume != 1.0 {
        filters.push(format!("volume={:.4}", state.volume));
    }
    filters
}

fn trim_filter(name: &str, state: &EditState) -> Option<String> {
    match (state.trim_start, state.trim_end) {
        (Some(start), Some(end)) if start < end => Some(format!(
            "{name}=start={:.6}:end={:.6}",
            start.as_seconds_f64(),
            end.as_seconds_f64()
        )),
        (Some(start), None) => Some(format!("{name}=start={:.6}", start.as_seconds_f64())),
        (None, Some(end)) => Some(format!("{name}=end={:.6}", end.as_seconds_f64())),
        _ => None,
    }
}

fn atempo_filters(mut rate: f32) -> Vec<String> {
    let mut filters = Vec::new();
    while rate < 0.5 {
        filters.push("atempo=0.5000".into());
        rate /= 0.5;
    }
    while rate > 2.0 {
        filters.push("atempo=2.0000".into());
        rate /= 2.0;
    }
    filters.push(format!("atempo={rate:.4}"));
    filters
}

fn same_path(left: &Path, right: &Path) -> bool {
    let normalize = |path: &Path| {
        path.canonicalize()
            .unwrap_or_else(|_| path.to_owned())
            .to_string_lossy()
            .to_lowercase()
    };
    normalize(left) == normalize(right)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use image::{ImageFormat, Rgb, RgbImage};
    use towavue_core::{MediaTime, UnitPoint, UnitRect};

    use super::*;

    #[test]
    fn image_export_preserves_operation_order() {
        let request = ExportRequest {
            source: "in.png".into(),
            target: "out.png".into(),
            kind: MediaKind::Image,
            operations: vec![
                EditOperation::Crop(UnitRect {
                    min: UnitPoint { x: 0.1, y: 0.2 },
                    max: UnitPoint { x: 0.6, y: 0.8 },
                }),
                EditOperation::RotateClockwise,
                EditOperation::FlipHorizontal,
            ],
        };

        let arguments = ffmpeg_arguments(&request);
        let filter = arguments
            .windows(2)
            .find_map(|pair| (pair[0] == "-vf").then_some(pair[1].as_str()))
            .expect("video filter argument");

        assert_eq!(
            filter,
            "crop=iw*0.50000000:ih*0.60000002:iw*0.10000000:ih*0.20000000,transpose=clock,hflip"
        );
    }

    #[test]
    fn video_export_builds_trim_rate_and_volume_filters() {
        let request = ExportRequest {
            source: "in.mp4".into(),
            target: "out.mp4".into(),
            kind: MediaKind::Video,
            operations: vec![
                EditOperation::SetTrimStart(MediaTime::from_nanoseconds(2_000_000_000)),
                EditOperation::SetTrimEnd(MediaTime::from_nanoseconds(5_000_000_000)),
                EditOperation::SetRate(4.0),
                EditOperation::SetVolume(0.5),
            ],
        };

        let arguments = ffmpeg_arguments(&request).join(" ");

        assert!(
            arguments.contains("trim=start=2.000000:end=5.000000,setpts=(PTS-STARTPTS)/4.0000")
        );
        assert!(arguments.contains("atrim=start=2.000000:end=5.000000,asetpts=PTS-STARTPTS,atempo=2.0000,atempo=2.0000,volume=0.5000"));
    }

    #[test]
    fn source_cannot_be_its_own_export_target() {
        let request = ExportRequest {
            source: "same.wav".into(),
            target: "same.wav".into(),
            kind: MediaKind::Audio,
            operations: Vec::new(),
        };

        assert!(matches!(
            export_media(&request),
            Err(ExportError::SameAsSource)
        ));
    }

    #[test]
    fn ffmpeg_exports_image_edits_without_changing_source() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let source = std::env::temp_dir().join(format!("towavue-export-{unique}-source.png"));
        let target = std::env::temp_dir().join(format!("towavue-export-{unique}-target.png"));
        RgbImage::from_pixel(40, 30, Rgb([20, 40, 60]))
            .save_with_format(&source, ImageFormat::Png)
            .expect("write export fixture");
        let request = ExportRequest {
            source: source.clone(),
            target: target.clone(),
            kind: MediaKind::Image,
            operations: vec![
                EditOperation::Crop(UnitRect {
                    min: UnitPoint { x: 0.0, y: 0.0 },
                    max: UnitPoint { x: 0.5, y: 1.0 },
                }),
                EditOperation::RotateClockwise,
            ],
        };

        export_media(&request).expect("export edited image");
        let exported = crate::decode_image(&target).expect("decode exported image");
        let original = crate::decode_image(&source).expect("decode original image");
        fs::remove_file(source).expect("remove source fixture");
        fs::remove_file(target).expect("remove target fixture");

        assert_eq!(original.dimensions(), (40, 30));
        assert_eq!(exported.dimensions(), (30, 20));
    }
}
