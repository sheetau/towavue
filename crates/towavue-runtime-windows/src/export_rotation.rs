use super::*;

pub(super) fn validate(request: &ExportRequest) -> Result<(), ExportError> {
    if !request.operations.iter().any(|operation| matches!(operation, EditOperation::RotateVideo(rotation) if rotation.tenths() != 0)) {
        return Ok(());
    }
    let validate = || -> Result<(), String> {
        let input = ffmpeg::format::input(&request.source).map_err(|error| error.to_string())?;
        let stream = input
            .streams()
            .best(ffmpeg::media::Type::Video)
            .ok_or("Video rotation requires a video stream")?;
        let matrix = stream
            .side_data()
            .find(|data| data.kind() == ffmpeg::codec::packet::side_data::Type::DisplayMatrix);
        let orientation =
            crate::VideoOrientation::from_bytes(matrix.as_ref().map(|data| data.data()))
                .map_err(|error| error.to_string())?;
        let decoder = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
            .and_then(|context| context.decoder().video())
            .map_err(|error| error.to_string())?;
        let mut size = (decoder.width(), decoder.height());
        // The owned input and its borrowed stream stay alive on this export worker.
        // FFmpeg only reads their SAR fields; null frame selects the codec-parameter fallback.
        let ratio = unsafe {
            ffmpeg::ffi::av_guess_sample_aspect_ratio(
                input.as_ptr().cast_mut(),
                stream.as_ptr().cast_mut(),
                std::ptr::null_mut(),
            )
        };
        let mut aspect = if ratio.num > 0 && ratio.den > 0 {
            ratio.num as f32 / ratio.den as f32
        } else {
            1.0
        };
        if orientation.swaps_axes() {
            size = (size.1, size.0);
            aspect = 1.0 / aspect;
        }
        for operation in &request.operations {
            match *operation {
                EditOperation::RotateVideo(rotation) if rotation.tenths() != 0 => {
                    if size != rotation.source_size()
                        || (aspect - rotation.source_pixel_aspect()).abs() > aspect.abs() * 0.00001
                    {
                        return Err(format!(
                            "Video rotation input changed: expected {:?} SAR {}, found {size:?} SAR {aspect}",
                            rotation.source_size(),
                            rotation.source_pixel_aspect()
                        ));
                    }
                    size = rotation.size();
                    aspect = 1.0;
                }
                EditOperation::Crop(crop) => {
                    if crop.width == 0
                        || crop.height == 0
                        || crop.x.checked_add(crop.width).is_none_or(|x| x > size.0)
                        || crop.y.checked_add(crop.height).is_none_or(|y| y > size.1)
                    {
                        return Err("Invalid crop before video rotation".into());
                    }
                    size = (crop.width, crop.height);
                }
                EditOperation::RotateClockwise | EditOperation::RotateCounterclockwise => {
                    size = (size.1, size.0);
                    aspect = 1.0 / aspect;
                }
                operation if !operation.applies_to(MediaKind::Video) => {
                    return Err("Non-video operation in video rotation history".into());
                }
                _ => {}
            }
        }
        Ok(())
    };
    validate().map_err(ExportError::Failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::time::{SystemTime, UNIX_EPOCH};
    use towavue_core::{PixelCrop, VideoRotation};

    fn run(executable: &Path, args: &[&str], source: Option<&Path>, tail: &[&str], target: &Path) {
        let mut command = Command::new(executable);
        command.creation_flags(CREATE_NO_WINDOW).args(args);
        if let Some(source) = source {
            command.arg(source);
        }
        let output = command.args(tail).arg(target).output().expect("FFmpeg");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn video(path: &Path) -> Vec<crate::VideoFrame> {
        let mut frames = Vec::new();
        crate::decode::decode_file(path, |output| {
            if let crate::DecodeOutput::Video(frame) = output {
                frames.push(frame);
            }
            true
        })
        .expect("decode");
        frames
    }

    #[test]
    fn oriented_video_rotation_keeps_trim_rate_and_audio_equal_to_the_unrotated_export() {
        use towavue_core::MediaTime;
        let directory = std::env::temp_dir().join(format!(
            "towavue-video-rotation-oriented-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir(&directory).expect("fixture directory");
        let executable = crate::media_tools::tool_path("ffmpeg.exe").expect("fixed FFmpeg");
        let encoded = directory.join("encoded.mp4");
        run(
            &executable,
            &[
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=64x48:rate=4:duration=1,setsar=2",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=48000:cl=mono:d=1",
                "-c:v",
                "libopenh264",
                "-c:a",
                "aac",
            ],
            None,
            &[],
            &encoded,
        );
        let source = directory.join("oriented.mp4");
        run(
            &executable,
            &["-v", "error", "-display_rotation:v:0", "90", "-i"],
            Some(&encoded),
            &["-c", "copy"],
            &source,
        );
        let bytes = fs::read(&source).expect("source bytes");
        let frames = video(&source);
        assert!(frames[0].orientation.swaps_axes());
        assert_eq!(frames[0].pixel_aspect, 2.0);
        let mut request = ExportRequest {
            source: source.clone(),
            target: directory.join("baseline.mp4"),
            kind: MediaKind::Video,
            hardware_encode: false,
            operations: vec![
                EditOperation::SetTrimStart(MediaTime::from_nanoseconds(250_000_000)),
                EditOperation::SetTrimEnd(MediaTime::from_nanoseconds(750_000_000)),
                EditOperation::SetRate(1.25),
                EditOperation::SetVolume(0.5),
            ],
        };
        export_media(&request).expect("baseline export");
        let baseline = request.target.clone();
        let rotation = VideoRotation::new(317, (48, 64), 0.5).expect("oriented geometry");
        request
            .operations
            .push(EditOperation::RotateVideo(rotation));
        request.target = directory.join("rotated.mp4");
        export_media(&request).expect("oriented rotation export");
        let actual = video(&request.target);
        let baseline_video = video(&baseline);
        assert!(!actual.is_empty());
        assert_eq!(actual.len(), baseline_video.len());
        for (actual, baseline) in actual.iter().zip(&baseline_video) {
            assert_eq!((actual.width, actual.height), rotation.size());
            assert_eq!(actual.pixel_aspect, 1.0);
            assert_eq!(actual.orientation, crate::VideoOrientation::default());
            assert_eq!(actual.presentation_time, baseline.presentation_time);
        }
        let audio = |path: &Path| {
            let mut output = Vec::new();
            crate::decode::decode_file(path, |item| {
                if let crate::DecodeOutput::Audio(chunk) = item {
                    output.push((chunk.presentation_time, chunk.frames, chunk.bytes));
                }
                true
            })
            .expect("audio decode");
            output
        };
        let actual_audio = audio(&request.target);
        assert!(!actual_audio.is_empty());
        assert_eq!(actual_audio, audio(&baseline));
        assert_eq!(fs::read(&source).expect("immutable source"), bytes);
        fs::remove_dir_all(&directory).expect("owned fixture cleanup");
    }

    #[test]
    fn square_pixel_rotation_export_matches_independent_filters_and_preserves_source_and_rejected_targets()
     {
        let directory = std::env::temp_dir().join(format!(
            "towavue-video-rotation-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir(&directory).expect("fixture directory");
        let executable = crate::media_tools::tool_path("ffmpeg.exe").expect("fixed FFmpeg");
        for (index, (aspect, angle)) in [
            (2.0, 317),
            (0.5, -317),
            (1.0, 900),
            (2.0, -900),
            (1.0, 1800),
            (2.0, 0),
        ]
        .into_iter()
        .enumerate()
        {
            let source = directory.join(format!("source-{index}.mkv"));
            run(
                &executable,
                &[
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    &format!("testsrc2=size=64x48:rate=4:duration=1,setsar={aspect}"),
                    "-c:v",
                    "ffv1",
                ],
                None,
                &[],
                &source,
            );
            let original = fs::read(&source).expect("source bytes");
            assert_eq!(
                video(&source)[0].pixel_aspect,
                aspect,
                "playback source aspect"
            );
            let rotation = VideoRotation::new(angle, (64, 48), aspect).expect("geometry");
            let target = directory.join(format!("rotated-{index}.mp4"));
            let request = ExportRequest {
                source: source.clone(),
                target: target.clone(),
                kind: MediaKind::Video,
                operations: vec![EditOperation::RotateVideo(rotation)],
                hardware_encode: false,
            };
            export_media(&request).expect("rotation export");
            let reference = directory.join(format!("reference-{index}.mp4"));
            let scale = if aspect >= 1.0 {
                format!("scale=iw*{aspect}:ih:flags=bilinear")
            } else {
                format!("scale=iw:ih/{aspect}:flags=bilinear")
            };
            let turn = match angle {
                900 => "transpose=clock".into(),
                -900 => "transpose=cclock".into(),
                1800 => "hflip,vflip".into(),
                _ => format!(
                    "rotate={angle}*PI/1800:ow='ceil(rotw({angle}*PI/1800))':oh='ceil(roth({angle}*PI/1800))':c=black:bilinear=1"
                ),
            };
            let filter = if angle == 0 {
                "null".into()
            } else {
                format!(
                    "format=gbrp,{scale},format=gbrp,setsar=1,{turn},pad=ceil(iw/2)*2:ceil(ih/2)*2:0:0:color=black,setsar=1,copy"
                )
            };
            run(
                &executable,
                &["-v", "error", "-i"],
                Some(&source),
                &["-vf", &filter, "-c:v", "libopenh264"],
                &reference,
            );
            let actual = video(&target);
            let expected = video(&reference);
            assert_eq!(actual.len(), 4);
            assert_eq!(actual.len(), expected.len());
            for (actual, expected) in actual.iter().zip(&expected) {
                assert_eq!(
                    (actual.width, actual.height),
                    if angle == 0 {
                        (64, 48)
                    } else {
                        rotation.size()
                    }
                );
                assert_eq!(actual.pixel_aspect, if angle == 0 { aspect } else { 1.0 });
                assert_eq!(actual.presentation_time, expected.presentation_time);
                assert_eq!(
                    actual.rgba, expected.rgba,
                    "reference pixels at angle {angle}, SAR {aspect}"
                );
            }
            let saved = fs::read(&target).expect("saved bytes");
            for bad in [
                VideoRotation::new(317, (32, 48), aspect).expect("wrong size"),
                VideoRotation::new(317, (64, 48), if aspect == 1.0 { 2.0 } else { 1.0 })
                    .expect("wrong aspect"),
            ] {
                let bad = ExportRequest {
                    operations: vec![EditOperation::RotateVideo(bad)],
                    ..request.clone()
                };
                assert!(matches!(export_media(&bad), Err(ExportError::Failed(_))));
                assert_eq!(fs::read(&target).expect("existing target"), saved);
            }
            for kind in [MediaKind::Image, MediaKind::Audio] {
                assert!(matches!(
                    export_media(&ExportRequest {
                        kind,
                        ..request.clone()
                    }),
                    Err(ExportError::Failed(_))
                ));
                assert_eq!(fs::read(&target).expect("existing target"), saved);
            }
            assert_eq!(fs::read(&source).expect("immutable source"), original);
        }
        fs::remove_dir_all(&directory).expect("owned fixture cleanup");
    }

    #[test]
    fn video_rotation_preserves_crop_turn_flip_order_and_second_rotation_geometry() {
        let directory = std::env::temp_dir().join(format!(
            "towavue-video-rotation-order-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir(&directory).expect("fixture directory");
        let executable = crate::media_tools::tool_path("ffmpeg.exe").expect("fixed FFmpeg");
        let source = directory.join("source.mkv");
        run(
            &executable,
            &[
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=64x48:rate=4:duration=1,setsar=2",
                "-c:v",
                "ffv1",
            ],
            None,
            &[],
            &source,
        );
        let first = VideoRotation::new(317, (32, 48), 0.5).expect("first");
        let second = VideoRotation::new(-127, (48, 48), 1.0).expect("second");
        let request = ExportRequest {
            source: source.clone(),
            target: directory.join("actual.mp4"),
            kind: MediaKind::Video,
            hardware_encode: false,
            operations: vec![
                EditOperation::Crop(PixelCrop {
                    x: 4,
                    y: 6,
                    width: 48,
                    height: 32,
                }),
                EditOperation::RotateClockwise,
                EditOperation::RotateVideo(first),
                EditOperation::FlipHorizontal,
                EditOperation::Crop(PixelCrop {
                    x: 8,
                    y: 8,
                    width: 48,
                    height: 48,
                }),
                EditOperation::RotateVideo(second),
                EditOperation::RotateCounterclockwise,
            ],
        };
        export_media(&request).expect("composed export");
        let reference = directory.join("reference.mp4");
        let filter = "crop=48:32:4:6:exact=1,transpose=clock,format=gbrp,scale=iw:ih*2:flags=bilinear,format=gbrp,setsar=1,rotate=317*PI/1800:ow='ceil(rotw(317*PI/1800))':oh='ceil(roth(317*PI/1800))':c=black:bilinear=1,pad=ceil(iw/2)*2:ceil(ih/2)*2:0:0:color=black,setsar=1,hflip,crop=48:48:8:8:exact=1,rotate=-127*PI/1800:ow='ceil(rotw(-127*PI/1800))':oh='ceil(roth(-127*PI/1800))':c=black:bilinear=1,pad=ceil(iw/2)*2:ceil(ih/2)*2:0:0:color=black,setsar=1,transpose=cclock,copy";
        run(
            &executable,
            &["-v", "error", "-i"],
            Some(&source),
            &["-vf", filter, "-c:v", "libopenh264"],
            &reference,
        );
        let actual = video(&request.target);
        let expected = video(&reference);
        assert_eq!(actual.len(), 4);
        for (actual, expected) in actual.iter().zip(expected) {
            assert_eq!(
                (actual.width, actual.height),
                (second.size().1, second.size().0)
            );
            assert_eq!(actual.pixel_aspect, 1.0);
            assert!(actual.rgba == expected.rgba, "composed pixels differ");
        }
        let image = crate::DecodedImage {
            format: "test",
            frames: vec![],
        };
        assert!(
            crate::render_image_edits(&image, &request.operations, &crate::Cancellation::default())
                .is_err()
        );
        fs::remove_dir_all(&directory).expect("owned fixture cleanup");
    }
}
