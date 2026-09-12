use super::*;
use std::io::Write;

fn atom(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    [&((data.len() + 8) as u32).to_be_bytes()[..], kind, data].concat()
}

// Add a color-to-alpha prem reference without moving encoded media offsets.
fn mark_premultiplied(path: &Path, animated: bool, alpha_id: u16) {
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .expect("fixture");
    let length = file.metadata().expect("fixture").len();
    let cancel = AtomicBool::new(false);
    let root = boxes(&mut file, 0, length, &cancel).expect("fixture");
    let kind = if animated { b"moov" } else { b"meta" };
    let parent = one(&root, kind).expect("fixture").expect("container");
    let prefix = if animated { 0 } else { 4 };
    let mut data = bytes(&mut file, parent, 1024 * 1024).expect("fixture");
    data.truncate(prefix);
    let mut changed = false;
    for child in
        boxes(&mut file, parent.start + prefix as u64, parent.end, &cancel).expect("fixture")
    {
        let mut content = bytes(&mut file, child, 1024 * 1024).expect("fixture");
        if !changed
            && ((animated && child.kind == *b"trak") || (!animated && child.kind == *b"iref"))
        {
            if animated {
                assert!(
                    one(
                        &boxes(&mut file, child.start, child.end, &cancel).expect("track"),
                        b"tref"
                    )
                    .expect("track")
                    .is_none()
                );
                content.extend(atom(
                    b"tref",
                    &atom(b"prem", &u32::from(alpha_id).to_be_bytes()),
                ));
            } else {
                assert_eq!(&content[..4], &[0; 4]);
                let reference: Vec<_> = [1u16, 1, alpha_id]
                    .into_iter()
                    .flat_map(u16::to_be_bytes)
                    .collect();
                content.extend(atom(b"prem", &reference));
            }
            changed = true;
        }
        data.extend(atom(&child.kind, &content));
    }
    assert!(changed);
    file.seek(SeekFrom::End(0)).expect("fixture");
    file.write_all(&atom(kind, &data)).expect("fixture");
    file.seek(SeekFrom::Start(parent.header + 4))
        .expect("fixture");
    file.write_all(b"free").expect("fixture");
}

#[test]
fn avif_premultiplied_alpha_matches_display_preview_and_edited_saves() {
    let root = audio_tests::root("avif-premultiplied");
    let source = root.join("source.avif");
    let target = root.join("saved.avif");
    let resaved = root.join("resaved.avif");
    let pixels: Vec<_> = (0..3)
        .map(|frame| {
            // Exercise every valid 8-bit (premultiplied component, alpha) pair.
            image::RgbaImage::from_fn(256, 256, |x, y| {
                let alpha = (x as u8).wrapping_add(frame * 31);
                image::Rgba([
                    (y as u8).min(alpha),
                    (255 - y as u8).min(alpha),
                    (frame * 73 + 101).min(alpha),
                    alpha,
                ])
            })
        })
        .collect();
    for (index, image) in pixels.iter().enumerate() {
        image
            .save(root.join(format!("{index}.png")))
            .expect("fixture PNG");
    }
    for animated in [false, true] {
        if animated {
            audio_tests::ffmpeg(
                &[
                    "-framerate",
                    "2",
                    "-i",
                    root.join("%d.png").to_str().expect("fixture path"),
                    "-filter_complex",
                    "format=rgba,split[c][a];[c]format=gbrp[color];[a]alphaextract,format=gray,setparams=colorspace=bt709[alpha]",
                    "-map",
                    "[color]",
                    "-map",
                    "[alpha]",
                    "-colorspace:v:1",
                    "bt709",
                    "-c:v",
                    "libaom-av1",
                    "-crf",
                    "0",
                    "-cpu-used",
                    "8",
                    "-threads",
                    "1",
                    "-loop",
                    "3",
                ],
                &source,
            );
        } else {
            export_media(&request(&root.join("0.png"), &source)).expect("still fixture");
        }
        let raw = crate::decode_image(&source).expect("unmarked fixture");
        assert_eq!(raw.frames.len(), if animated { 3 } else { 1 });
        for (frame, expected) in raw.frames.iter().zip(&pixels) {
            assert_eq!(
                &frame.rgba,
                expected.as_raw(),
                "fixture must retain encoded components"
            );
        }
        let intact = fs::read(&source).expect("fixture");
        mark_premultiplied(&source, animated, 2);
        let displayed = crate::decode_image(&source).expect("premultiplied display");
        for (frame, original) in displayed.frames.iter().zip(&pixels) {
            for (actual, encoded) in frame.rgba.as_chunks::<4>().0.iter().zip(original.pixels()) {
                let alpha = encoded[3];
                let channel = |value| {
                    if alpha == 0 {
                        0
                    } else {
                        (f64::from(value) * 255.0 / f64::from(alpha)).round() as u8
                    }
                };
                assert_eq!(
                    *actual,
                    [
                        channel(encoded[0]),
                        channel(encoded[1]),
                        channel(encoded[2]),
                        alpha
                    ]
                );
            }
        }
        let preview = crate::image::first_animation_frame(&source, 256 * 256 * 4, &|| true)
            .expect("preview")
            .expect("frame");
        assert_eq!(preview.rgba, displayed.frames[0].rgba);
        let mut save = request(&source, &target);
        save.operations = vec![EditOperation::RotateClockwise];
        let expected = crate::render_image_edits(
            &displayed,
            &save.operations,
            &crate::Cancellation::default(),
        )
        .expect("edited display");
        export_media(&save).expect("premultiplied save");
        export_media(&request(&target, &resaved)).expect("resave");
        for path in [&target, &resaved] {
            let actual = crate::decode_image(path).expect("saved decode");
            assert_eq!(actual.frames.len(), expected.frames.len());
            for (index, (actual, expected)) in
                actual.frames.iter().zip(&expected.frames).enumerate()
            {
                assert_eq!(
                    (actual.width, actual.height),
                    (expected.width, expected.height)
                );
                assert_eq!(actual.delay, expected.delay);
                assert!(
                    actual.rgba == expected.rgba,
                    "animated={animated}, frame={index}, path={path:?}, first difference {:?}",
                    actual
                        .rgba
                        .iter()
                        .zip(&expected.rgba)
                        .enumerate()
                        .find(|(_, (a, b))| a != b)
                );
            }
        }
        if animated {
            let saved = Animation::read(&target, &AtomicBool::new(false))
                .expect("saved controls")
                .expect("sequence");
            assert_eq!(saved.color().loops, Some(3));
            assert!(saved.color().premultiplied_with.is_none());
        }
        fs::write(&source, intact).expect("restore fixture");
        mark_premultiplied(&source, animated, 3);
        assert!(crate::decode_image(&source).is_err());
        let before = fs::read(&target).expect("existing target");
        assert!(export_media(&request(&source, &target)).is_err());
        assert_eq!(fs::read(&target).expect("protected target"), before);
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}
