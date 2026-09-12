use super::*;
use image::{DynamicImage, RgbaImage};
use std::io::Write;

fn atom(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    [&((data.len() + 8) as u32).to_be_bytes()[..], kind, data].concat()
}

// Append fixture-only item transforms, relocating meta without moving media.
fn orient_still(path: &Path, angle: u8, mirror: Option<u8>, alpha: bool) {
    let mut properties = vec![(b"irot", vec![angle])];
    if let Some(axis) = mirror {
        properties.push((b"imir", vec![axis]));
    }
    append_still_properties(path, &properties, if alpha { &[1, 2] } else { &[1] });
}

fn append_still_properties(path: &Path, properties: &[(&[u8; 4], Vec<u8>)], ids: &[u16]) {
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .expect("fixture");
    let length = file.metadata().expect("fixture").len();
    let cancel = AtomicBool::new(false);
    let root = boxes(&mut file, 0, length, &cancel).expect("fixture");
    let meta = one(&root, b"meta").expect("fixture").expect("meta");
    let children = boxes(&mut file, meta.start + 4, meta.end, &cancel).expect("fixture");
    let iprp = one(&children, b"iprp").expect("fixture").expect("iprp");
    let children = boxes(&mut file, iprp.start, iprp.end, &cancel).expect("fixture");
    let ipco = one(&children, b"ipco").expect("fixture").expect("ipco");
    let count = boxes(&mut file, ipco.start, ipco.end, &cancel)
        .expect("fixture")
        .len();
    fn rewrite(
        file: &mut fs::File,
        item: BoxRange,
        props: &[(&[u8; 4], Vec<u8>)],
        count: usize,
        ids: &[u16],
    ) -> Vec<u8> {
        let mut data = bytes(file, item, 1024 * 1024).expect("fixture");
        match &item.kind {
            b"meta" | b"iprp" => {
                let prefix = if item.kind == *b"meta" { 4 } else { 0 };
                data.truncate(prefix);
                for child in boxes(
                    file,
                    item.start + prefix as u64,
                    item.end,
                    &AtomicBool::new(false),
                )
                .expect("fixture")
                {
                    data.extend(rewrite(file, child, props, count, ids));
                }
            }
            b"ipco" => {
                for (kind, value) in props {
                    data.extend(atom(kind, value));
                }
            }
            b"ipma" => {
                assert_eq!(&data[..4], &[0; 4]);
                let mut output = data[..8].to_vec();
                let mut pos = 8;
                while pos < data.len() {
                    let id = u16::from_be_bytes([data[pos], data[pos + 1]]);
                    let old_count = data[pos + 2] as usize;
                    output.extend(&data[pos..pos + 2]);
                    let add = ids.contains(&id);
                    output.push((old_count + if add { props.len() } else { 0 }) as u8);
                    output.extend(&data[pos + 3..pos + 3 + old_count]);
                    if add {
                        for index in 0..props.len() {
                            output.push(0x80 | (count + index + 1) as u8);
                        }
                    }
                    pos += 3 + old_count;
                }
                data = output;
            }
            _ => {}
        }
        atom(&item.kind, &data)
    }
    let replacement = rewrite(&mut file, meta, properties, count, ids);
    file.seek(SeekFrom::End(0)).expect("fixture");
    file.write_all(&replacement).expect("fixture");
    file.seek(SeekFrom::Start(meta.header + 4))
        .expect("fixture");
    file.write_all(b"free").expect("fixture");
}

#[test]
fn avif_clean_aperture_precedes_orientation_preview_and_static_saving() {
    let root = audio_tests::root("avif-clean-aperture");
    let png = root.join("original.png");
    let source = root.join("source.avif");
    let target = root.join("saved.avif");
    let pixels = RgbaImage::from_fn(7, 5, |x, y| {
        image::Rgba([
            (x * 31) as u8,
            (y * 47) as u8,
            (x * 13 + y * 11) as u8,
            (x * 23 + y * 17) as u8,
        ])
    });
    pixels.save(&png).expect("fixture");
    export_media(&request(&png, &source)).expect("fixture AVIF");
    let intact = fs::read(&source).expect("fixture");
    // A 4x2 aperture at (2,1) has signed center offsets (+1/2,-1/2).
    let clap = [4i32, 1, 2, 1, 1, 2, -1, 2]
        .into_iter()
        .flat_map(i32::to_be_bytes)
        .collect::<Vec<_>>();
    for alpha in [false, true] {
        for angle in 0..4 {
            fs::write(&source, &intact).expect("restore fixture");
            append_still_properties(
                &source,
                &[(b"clap", clap.clone())],
                if alpha { &[1, 2] } else { &[1] },
            );
            orient_still(&source, angle, Some(1), alpha);
            let cropped = crate::DecodedImageFrame {
                width: 4,
                height: 2,
                rgba: image::imageops::crop_imm(&pixels, 2, 1, 4, 2)
                    .to_image()
                    .into_raw(),
                delay: Duration::ZERO,
            };
            let expected = transformed(&cropped, angle, Some(1)).into_rgba8();
            let decoded = crate::decode_image(&source).expect("aperture display");
            assert_eq!(decoded.dimensions(), expected.dimensions());
            assert_eq!(decoded.frames[0].rgba, expected.as_raw().as_slice());
            let preview = crate::image::first_animation_frame(&source, 7 * 5 * 4, &|| true)
                .expect("preview")
                .expect("frame");
            assert_eq!(preview.rgba, decoded.frames[0].rgba);
            let cache_path = root.join(format!("cache-{alpha}-{angle}"));
            let cache = crate::PreviewCache::new(cache_path.clone()).expect("cache");
            cache
                .filmstrip(&source, MediaKind::Image)
                .expect("persist preview");
            let persisted = crate::PreviewCache::new(cache_path)
                .expect("fresh cache")
                .cached_image(&source)
                .expect("cache lookup")
                .expect("cached preview");
            assert_eq!(persisted.source_size, expected.dimensions());
            assert_eq!(
                persisted.image.rgba,
                DynamicImage::ImageRgba8(expected.clone())
                    .resize(240, 160, image::imageops::FilterType::Nearest)
                    .into_rgba8()
                    .into_raw()
            );
            export_media(&request(&source, &target)).expect("save aperture");
            let saved = crate::decode_image(&target).expect("saved image");
            assert_eq!(saved.dimensions(), expected.dimensions());
            assert_eq!(saved.frames[0].rgba, decoded.frames[0].rgba);
        }
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn avif_invalid_and_conflicting_apertures_preserve_existing_targets() {
    let root = audio_tests::root("avif-aperture-guards");
    let source = root.join("source.avif");
    let target = root.join("existing.avif");
    let payload = |values: [i32; 8]| {
        values
            .into_iter()
            .flat_map(i32::to_be_bytes)
            .collect::<Vec<_>>()
    };
    for animated in [false, true] {
        if animated {
            fixture(&source, "3", true);
        } else {
            let png = root.join("original.png");
            RgbaImage::from_pixel(32, 24, image::Rgba([50, 70, 90, 100]))
                .save(&png)
                .expect("fixture");
            export_media(&request(&png, &source)).expect("fixture");
        }
        let intact = fs::read(&source).expect("fixture");
        for conflicting in [false, true] {
            fs::write(&source, &intact).expect("restore fixture");
            let color = payload(if conflicting {
                [19, 1, 13, 1, -7, 2, -7, 2]
            } else {
                [40, 1, 13, 1, 0, 1, 0, 1]
            });
            let alpha = payload([18, 1, 12, 1, -3, 1, -3, 1]);
            if animated {
                append_track_aperture(&source, &color, &[0]);
                if conflicting {
                    append_track_aperture(&source, &alpha, &[1]);
                }
            } else {
                append_still_properties(&source, &[(b"clap", color)], &[1]);
                if conflicting {
                    append_still_properties(&source, &[(b"clap", alpha)], &[2]);
                }
            }
            fs::write(&target, b"existing target").expect("target");
            assert!(
                crate::decode_image(&source).is_err(),
                "animated={animated} conflicting={conflicting}"
            );
            assert!(crate::image::first_animation_frame(&source, 32 * 24 * 4, &|| true).is_err());
            assert!(export_media(&request(&source, &target)).is_err());
            assert_eq!(
                fs::read(&target).expect("target preserved"),
                b"existing target"
            );
        }
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

fn transformed(frame: &crate::DecodedImageFrame, angle: u8, mirror: Option<u8>) -> DynamicImage {
    let image = DynamicImage::ImageRgba8(
        RgbaImage::from_raw(frame.width, frame.height, frame.rgba.clone()).expect("fixture"),
    );
    let image = match angle {
        0 => image,
        1 => image.rotate270(),
        2 => image.rotate180(),
        3 => image.rotate90(),
        _ => unreachable!(),
    };
    match mirror {
        None => image,
        Some(0) => image.flipv(),
        Some(1) => image.fliph(),
        _ => unreachable!(),
    }
}

fn append_track_aperture(path: &Path, clap: &[u8], ids: &[usize]) {
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .expect("fixture");
    let length = file.metadata().expect("fixture").len();
    let root = boxes(&mut file, 0, length, &AtomicBool::new(false)).expect("fixture");
    let movie = one(&root, b"moov").expect("fixture").expect("movie");
    fn rewrite(file: &mut fs::File, item: BoxRange, clap: &[u8]) -> Vec<u8> {
        let mut data = bytes(file, item, 1024 * 1024).expect("fixture");
        match &item.kind {
            b"trak" | b"mdia" | b"minf" | b"stbl" | b"stsd" => {
                let prefix = if item.kind == *b"stsd" { 8 } else { 0 };
                data.truncate(prefix);
                for child in boxes(
                    file,
                    item.start + prefix as u64,
                    item.end,
                    &AtomicBool::new(false),
                )
                .expect("fixture")
                {
                    data.extend(rewrite(file, child, clap));
                }
            }
            b"av01" => data.extend(atom(b"clap", clap)),
            _ => {}
        }
        atom(&item.kind, &data)
    }
    let mut data = Vec::new();
    let mut index = 0;
    for child in boxes(&mut file, movie.start, movie.end, &AtomicBool::new(false)).expect("fixture")
    {
        let selected = child.kind == *b"trak" && ids.contains(&index);
        index += usize::from(child.kind == *b"trak");
        if selected {
            data.extend(rewrite(&mut file, child, clap));
        } else {
            data.extend(atom(
                &child.kind,
                &bytes(&mut file, child, 1024 * 1024).expect("fixture"),
            ));
        }
    }
    file.seek(SeekFrom::End(0)).expect("fixture");
    file.write_all(&atom(b"moov", &data)).expect("fixture");
    file.seek(SeekFrom::Start(movie.header + 4))
        .expect("fixture");
    file.write_all(b"free").expect("fixture");
}

#[test]
fn avif_sequence_aperture_keeps_alpha_timing_orientation_and_resaved_pixels() {
    let root = audio_tests::root("avif-aperture-sequence");
    let source = root.join("source.avif");
    let target = root.join("saved.avif");
    let resaved = root.join("resaved.avif");
    let clap = [19i32, 1, 13, 1, -7, 2, -7, 2]
        .into_iter()
        .flat_map(i32::to_be_bytes)
        .collect::<Vec<_>>();
    for count in [1, 3] {
        for alpha in [false, true] {
            if count == 1 {
                single_fixture(&source, 3, alpha, 375);
            } else {
                fixture(&source, "3", alpha);
            }
            let intact = fs::read(&source).expect("fixture");
            let baseline = crate::decode_image(&source).expect("baseline");
            for alpha_transform in [false, true] {
                for angle in 0..4 {
                    fs::write(&source, &intact).expect("restore fixture");
                    append_track_aperture(
                        &source,
                        &clap,
                        if alpha_transform { &[0, 1] } else { &[0] },
                    );
                    orient_tracks(&source, angle, Some(1), alpha_transform);
                    let expected = baseline
                        .frames
                        .iter()
                        .map(|frame| {
                            let image =
                                RgbaImage::from_raw(frame.width, frame.height, frame.rgba.clone())
                                    .expect("frame");
                            transformed(
                                &crate::DecodedImageFrame {
                                    width: 19,
                                    height: 13,
                                    rgba: image::imageops::crop_imm(&image, 3, 2, 19, 13)
                                        .to_image()
                                        .into_raw(),
                                    delay: frame.delay,
                                },
                                angle,
                                Some(1),
                            )
                            .into_rgba8()
                        })
                        .collect::<Vec<_>>();
                    let displayed = crate::decode_image(&source).expect("aperture display");
                    export_media(&request(&source, &target)).expect("aperture save");
                    export_media(&request(&target, &resaved)).expect("aperture resave");
                    for (stage, image) in [
                        displayed,
                        crate::decode_image(&target).expect("saved"),
                        crate::decode_image(&resaved).expect("resaved"),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        assert_eq!(image.frames.len(), count);
                        for ((frame, expected), original) in
                            image.frames.iter().zip(&expected).zip(&baseline.frames)
                        {
                            assert_eq!((frame.width, frame.height), expected.dimensions());
                            assert!(
                                frame.rgba == expected.as_raw().as_slice(),
                                "stage={stage} count={count} alpha={alpha} alpha_transform={alpha_transform} angle={angle}; first difference {:?}",
                                frame
                                    .rgba
                                    .iter()
                                    .zip(expected.as_raw())
                                    .position(|(a, b)| a != b)
                            );
                            assert_eq!(frame.delay, original.delay);
                        }
                    }
                }
            }
        }
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

fn orient_tracks(path: &Path, angle: u8, mirror: Option<u8>, alpha: bool) {
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .expect("fixture");
    let length = file.metadata().expect("fixture").len();
    let cancel = AtomicBool::new(false);
    let root = boxes(&mut file, 0, length, &cancel).expect("fixture");
    let movie = one(&root, b"moov").expect("fixture").expect("movie");
    let tracks = boxes(&mut file, movie.start, movie.end, &cancel).expect("fixture");
    let [mut a, mut b, mut c, mut d]: [i32; 4] = match angle {
        0 => [1, 0, 0, 1],
        1 => [0, -1, 1, 0],
        2 => [-1, 0, 0, -1],
        3 => [0, 1, -1, 0],
        _ => unreachable!(),
    };
    if mirror == Some(0) {
        b = -b;
        d = -d;
    }
    if mirror == Some(1) {
        a = -a;
        c = -c;
    }
    let matrix: Vec<_> = [
        a * 65536,
        b * 65536,
        0,
        c * 65536,
        d * 65536,
        0,
        0,
        0,
        1 << 30,
    ]
    .into_iter()
    .flat_map(i32::to_be_bytes)
    .collect();
    for (index, track) in tracks
        .into_iter()
        .filter(|item| item.kind == *b"trak")
        .enumerate()
    {
        if index > 0 && !alpha {
            continue;
        }
        let children = boxes(&mut file, track.start, track.end, &cancel).expect("fixture");
        let header = one(&children, b"tkhd").expect("fixture").expect("header");
        let data = bytes(&mut file, header, 128).expect("fixture");
        let offset = if data[0] == 1 { 52 } else { 40 };
        file.seek(SeekFrom::Start(header.start + offset))
            .expect("fixture");
        file.write_all(&matrix).expect("fixture");
    }
}

#[test]
fn avif_sequence_orientation_precedes_edits_and_survives_resave() {
    let root = audio_tests::root("avif-oriented-sequence");
    let source = root.join("source.avif");
    let target = root.join("saved.avif");
    for alpha in [false, true] {
        fixture(&source, "3", alpha);
        let intact = fs::read(&source).expect("fixture");
        let original = crate::decode_image(&source).expect("baseline");
        for angle in 0..4 {
            for mirror in [None, Some(0), Some(1)] {
                fs::write(&source, &intact).expect("fixture");
                orient_tracks(&source, angle, mirror, alpha);
                let oriented = crate::decode_image(&source).expect("oriented sequence");
                for (raw, actual) in original.frames.iter().zip(&oriented.frames) {
                    let expected = transformed(raw, angle, mirror).into_rgba8();
                    assert_eq!((actual.width, actual.height), expected.dimensions());
                    assert_eq!(actual.rgba, expected.as_raw().as_slice());
                }
                let mut save = request(&source, &target);
                export_media(&save).expect("oriented save without additional edits");
                let saved = crate::decode_image(&target).expect("saved orientation");
                assert_eq!(saved.dimensions(), oriented.dimensions());
                assert_eq!(saved.frames[0].rgba, oriented.frames[0].rgba);
                save.operations = vec![
                    EditOperation::Crop(towavue_core::PixelCrop {
                        x: 2,
                        y: 3,
                        width: 13,
                        height: 17,
                    }),
                    EditOperation::FlipHorizontal,
                ];
                let expected = crate::render_image_edits(
                    &oriented,
                    &save.operations,
                    &crate::Cancellation::default(),
                )
                .expect("edited display");
                export_media(&save).expect("oriented sequence save");
                let actual = crate::decode_image(&target).expect("saved sequence");
                assert_eq!(actual.frames.len(), 3);
                for (expected, actual) in expected.frames.iter().zip(&actual.frames) {
                    assert_eq!(expected.rgba, actual.rgba);
                    assert_eq!(expected.delay, actual.delay);
                }
                let resaved = root.join("resaved.avif");
                export_media(&request(&target, &resaved)).expect("resave");
                let after = crate::decode_image(&resaved).expect("resaved");
                assert_eq!(after.frames[0].rgba, actual.frames[0].rgba);
            }
        }
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn avif_alpha_orientation_omission_is_supported_but_conflicts_preserve_targets() {
    let root = audio_tests::root("avif-alpha-orientation");
    let source = root.join("source.avif");
    let target = root.join("saved.avif");
    for single in [false, true] {
        if single {
            single_fixture(&source, 3, true, 375);
        } else {
            fixture(&source, "3", true);
        }
        let original = crate::decode_image(&source).expect("original");
        orient_tracks(&source, 1, Some(1), false);
        let expected = transformed(&original.frames[0], 1, Some(1)).into_rgba8();
        let actual = crate::decode_image(&source).expect("omitted alpha transform");
        assert_eq!(actual.frames[0].rgba, expected.as_raw().as_slice());
        export_media(&request(&source, &target)).expect("save omitted alpha transform");
        let saved = crate::decode_image(&target).expect("saved");
        assert_eq!(saved.frames.len(), original.frames.len());
        assert_eq!(saved.frames[0].rgba, expected.as_raw().as_slice());
        orient_tracks(&source, 2, None, true);
        orient_tracks(&source, 1, None, false);
        fs::write(&target, b"existing target").expect("fixture");
        assert!(crate::decode_image(&source).is_err());
        assert!(export_media(&request(&source, &target)).is_err());
        assert_eq!(fs::read(&target).expect("target"), b"existing target");
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn avif_still_rotation_and_mirror_reach_display_preview_and_saved_pixels() {
    let root = audio_tests::root("avif-orientation-still");
    let png = root.join("original.png");
    let source = root.join("source.avif");
    let target = root.join("saved.avif");
    let pixels = RgbaImage::from_fn(7, 5, |x, y| {
        image::Rgba([
            (x * 31) as u8,
            (y * 47) as u8,
            (x * 13 + y * 11) as u8,
            (x * 23 + y * 17) as u8,
        ])
    });
    pixels.save(&png).expect("fixture");
    export_media(&request(&png, &source)).expect("fixture");
    let intact = fs::read(&source).expect("fixture");
    let original = crate::decode_image(&source).expect("baseline");
    for angle in 0..4 {
        for mirror in [None, Some(0), Some(1)] {
            for alpha_properties in [false, true] {
                fs::write(&source, &intact).expect("fixture");
                orient_still(&source, angle, mirror, alpha_properties);
                let expected = transformed(&original.frames[0], angle, mirror).into_rgba8();
                let decoded = crate::decode_image(&source).expect("oriented display");
                assert_eq!(
                    decoded.dimensions(),
                    expected.dimensions(),
                    "angle={angle} mirror={mirror:?}"
                );
                assert_eq!(
                    decoded.frames[0].rgba,
                    expected.as_raw().as_slice(),
                    "angle={angle} mirror={mirror:?} alpha={alpha_properties}"
                );
                let preview = crate::image::first_animation_frame(&source, 7 * 5 * 4, &|| true)
                    .expect("preview")
                    .expect("frame");
                assert_eq!((preview.width, preview.height), expected.dimensions());
                assert_eq!(preview.rgba, decoded.frames[0].rgba);
                let cache_path = root.join("cache");
                let cache = crate::PreviewCache::new(cache_path.clone()).expect("cache");
                let thumbnail = cache
                    .prepare_animation_preview(&source, 7 * 5 * 4, &|| true)
                    .expect("thumbnail");
                assert_eq!(thumbnail.source_size, expected.dimensions());
                assert_eq!(thumbnail.image.rgba, decoded.frames[0].rgba);
                // Speculative previews are memory-only. A fresh filmstrip cache
                // exercises the separate persisted-thumbnail generation path.
                let disk_writer =
                    crate::PreviewCache::new(cache_path.clone()).expect("disk writer");
                let disk_thumbnail = disk_writer
                    .filmstrip(&source, MediaKind::Image)
                    .expect("persist thumbnail");
                let reopened = crate::PreviewCache::new(cache_path).expect("fresh cache");
                let persisted = reopened
                    .cached_image(&source)
                    .expect("disk lookup")
                    .expect("cached thumbnail");
                assert_eq!(persisted.source_size, thumbnail.source_size);
                assert_eq!(persisted.image.rgba, disk_thumbnail.image.rgba);
                assert_eq!(
                    persisted.image.rgba,
                    DynamicImage::ImageRgba8(expected.clone())
                        .resize(240, 160, image::imageops::FilterType::Nearest)
                        .into_rgba8()
                        .into_raw()
                );
                export_media(&request(&source, &target)).expect("oriented save");
                assert_eq!(
                    crate::decode_image(&target).expect("saved").frames[0].rgba,
                    decoded.frames[0].rgba
                );
                let converted = root.join("converted.png");
                export_media(&request(&source, &converted)).expect("oriented conversion");
                assert_eq!(
                    crate::decode_image(&converted).expect("converted").frames[0].rgba,
                    decoded.frames[0].rgba
                );
            }
        }
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}
