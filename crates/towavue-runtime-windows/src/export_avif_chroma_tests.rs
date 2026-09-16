use super::*;

#[test]
fn avif_declared_chroma_positions_survive_display_preview_and_edited_png() {
    let root = audio_tests::root("avif-chroma");
    let source = root.join("source.avif");
    let reference = root.join("reference.rgba");
    for pixel in ["yuv420p", "yuv420p10le"] {
        for (location, vertical) in [("left", 128), ("topleft", 0)] {
            for frames in [1, 3] {
                audio_tests::ffmpeg(
                    &[
                        "-f",
                        "lavfi",
                        "-i",
                        "testsrc2=size=32x24:rate=2:duration=1.5",
                        "-frames:v",
                        &frames.to_string(),
                        "-pix_fmt",
                        pixel,
                        "-c:v",
                        "libaom-av1",
                        "-crf",
                        "0",
                        "-cpu-used",
                        "8",
                        "-threads",
                        "1",
                        "-color_range",
                        "tv",
                        "-colorspace",
                        "bt709",
                        "-chroma_sample_location",
                        location,
                        "-bsf:v",
                        &format!(
                            "av1_metadata=chroma_sample_position={}",
                            if location == "left" {
                                "vertical"
                            } else {
                                "colocated"
                            }
                        ),
                    ],
                    &source,
                );
                let probe =
                    Command::new(crate::media_tools::tool_path("ffprobe.exe").expect("ffprobe"))
                        .args([
                            "-v",
                            "error",
                            "-select_streams",
                            if frames == 1 { "0" } else { "1" },
                            "-show_entries",
                            "frame=chroma_location",
                            "-of",
                            "csv=p=0",
                        ])
                        .arg(&source)
                        .output()
                        .expect("source chroma metadata");
                assert!(probe.status.success());
                let metadata = String::from_utf8(probe.stdout).expect("chroma locations");
                assert_eq!(metadata.lines().collect::<Vec<_>>(), vec![location; frames]);
                audio_tests::ffmpeg(
                    &[
                        "-i",
                        source.to_str().expect("source path"),
                        "-map",
                        if frames == 1 { "0:0" } else { "0:1" },
                        "-vf",
                        &format!(
                            "scale=flags=bilinear:in_color_matrix=bt709:in_range=limited:out_range=full:in_h_chr_pos=0:in_v_chr_pos={vertical},format=rgba"
                        ),
                        "-f",
                        "rawvideo",
                        "-pix_fmt",
                        "rgba",
                    ],
                    &reference,
                );
                let expected = fs::read(&reference).expect("explicit-position pixels");
                assert_eq!(expected.len(), frames * 32 * 24 * 4);
                let original = fs::read(&source).expect("source bytes");
                let modified = fs::metadata(&source)
                    .expect("source stamp")
                    .modified()
                    .expect("source time");
                let decoded = crate::decode_image(&source).expect("AVIF display");
                assert_eq!(decoded.frames.len(), frames);
                for (frame, pixels) in decoded
                    .frames
                    .iter()
                    .zip(expected.as_chunks::<{ 32 * 24 * 4 }>().0)
                {
                    assert!(
                        frame.rgba == pixels,
                        "display: {pixel}, {location}, frames={frames}"
                    );
                }
                let preview = crate::image::first_animation_frame(&source, 32 * 24 * 4, &|| true)
                    .expect("AVIF preview")
                    .expect("preview frame");
                assert_eq!(preview.rgba, expected[..32 * 24 * 4]);
                let target = root.join("edited.png");
                fs::write(&target, b"existing target").expect("replace target");
                let mut export = request(&source, &target);
                export.operations.push(EditOperation::FlipHorizontal);
                export_media(&export).expect("edited PNG export");
                let saved = crate::decode_image(&target).expect("PNG/APNG readback");
                assert_eq!(saved.frames.len(), frames);
                for (frame, pixels) in saved
                    .frames
                    .iter()
                    .zip(expected.as_chunks::<{ 32 * 24 * 4 }>().0)
                {
                    let reference = image::RgbaImage::from_raw(32, 24, pixels.to_vec())
                        .expect("reference image");
                    assert!(
                        frame.rgba == *image::imageops::flip_horizontal(&reference).as_raw(),
                        "saved frame: {pixel}, {location}, frames={frames}"
                    );
                }
                assert_eq!(fs::read(&source).expect("unchanged source"), original);
                assert_eq!(
                    fs::metadata(&source)
                        .expect("source stamp")
                        .modified()
                        .expect("source time"),
                    modified
                );
            }
        }
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}
