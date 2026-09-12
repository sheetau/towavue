use super::*;
use image::{DynamicImage, RgbaImage};

fn atom(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    [&((data.len() + 8) as u32).to_be_bytes()[..], kind, data].concat()
}

#[derive(Clone)]
struct Item {
    data: Vec<u8>,
    properties: Vec<([u8; 4], Vec<u8>)>,
    inputs: Vec<u16>,
    auxiliary_for: Option<u16>,
    premultiplied_with: Option<u16>,
}

// Encode only the AV1 payloads with FFmpeg. Build item identities, associations,
// grid descriptors and references here, independently of the display adapter.
fn encoded(root: &Path, image: DynamicImage, alpha: bool) -> Item {
    let png = root.join("plane.png");
    let avif = root.join("plane.avif");
    image.save(&png).expect("plane PNG");
    audio_tests::ffmpeg(
        &[
            "-i",
            png.to_str().expect("path"),
            "-frames:v",
            "1",
            "-c:v",
            "libaom-av1",
            "-crf",
            "0",
            "-cpu-used",
            "8",
            "-threads",
            "1",
            "-vf",
            if alpha {
                "format=gray,setparams=colorspace=bt709"
            } else {
                "format=gbrp"
            },
            "-color_range",
            "pc",
        ],
        &avif,
    );
    let mut file = fs::File::open(avif).expect("encoded plane");
    let length = file.metadata().expect("length").len();
    let cancel = AtomicBool::new(false);
    let root = boxes(&mut file, 0, length, &cancel).expect("boxes");
    let data = bytes(
        &mut file,
        one(&root, b"mdat").expect("unique").expect("data"),
        1024 * 1024,
    )
    .expect("AV1 sample");
    let meta = one(&root, b"meta").expect("unique").expect("metadata");
    let children = boxes(&mut file, meta.start + 4, meta.end, &cancel).expect("metadata");
    let iprp = one(&children, b"iprp")
        .expect("unique")
        .expect("properties");
    let children = boxes(&mut file, iprp.start, iprp.end, &cancel).expect("properties");
    let ipco = one(&children, b"ipco").expect("unique").expect("values");
    let properties = boxes(&mut file, ipco.start, ipco.end, &cancel)
        .expect("values")
        .into_iter()
        .map(|item| (item.kind, bytes(&mut file, item, 1024).expect("property")))
        .collect();
    Item {
        data,
        properties,
        inputs: vec![],
        auxiliary_for: None,
        premultiplied_with: None,
    }
}

fn grid(tiles: Vec<u16>, alpha: bool) -> Item {
    Item {
        // 2x2, 64x64 tiles, cropped right/bottom padding.
        data: [
            vec![0, 0, 1, 1],
            125u16.to_be_bytes().to_vec(),
            123u16.to_be_bytes().to_vec(),
        ]
        .concat(),
        properties: vec![
            (
                *b"ispe",
                [
                    vec![0; 4],
                    125u32.to_be_bytes().to_vec(),
                    123u32.to_be_bytes().to_vec(),
                ]
                .concat(),
            ),
            (
                *b"pixi",
                if alpha {
                    vec![0, 0, 0, 0, 1, 8]
                } else {
                    vec![0, 0, 0, 0, 3, 8, 8, 8]
                },
            ),
        ],
        inputs: tiles,
        auxiliary_for: None,
        premultiplied_with: None,
    }
}

fn fixture(items: &[Item], path: &Path) {
    let count = items.len() as u16;
    let mut info = [vec![0; 4], count.to_be_bytes().to_vec()].concat();
    let mut locations = [vec![1, 0, 0, 0, 0x44, 0], count.to_be_bytes().to_vec()].concat();
    let mut associations = [vec![0; 4], u32::from(count).to_be_bytes().to_vec()].concat();
    let mut references = vec![0; 4];
    let mut properties = Vec::new();
    let mut data = Vec::new();
    let mut property_count = 0u8;
    for (index, item) in items.iter().enumerate() {
        let id = (index as u16 + 1).to_be_bytes();
        let kind = if item.inputs.is_empty() {
            b"av01"
        } else {
            b"grid"
        };
        info.extend(atom(
            b"infe",
            &[
                &[2, 0, 0, u8::from(index > 0)][..],
                &id,
                &[0, 0],
                kind,
                &[0],
            ]
            .concat(),
        ));
        locations.extend(
            [
                &id[..],
                &[0, 1, 0, 0, 0, 1],
                &(data.len() as u32).to_be_bytes(),
                &(item.data.len() as u32).to_be_bytes(),
            ]
            .concat(),
        );
        data.extend(&item.data);
        associations.extend(id);
        associations.push(item.properties.len() as u8 + u8::from(item.auxiliary_for.is_some()));
        for (kind, value) in &item.properties {
            property_count += 1;
            assert!(property_count < 128);
            properties.extend(atom(kind, value));
            associations.push(property_count | if matches!(kind, b"av1C") { 0x80 } else { 0 });
        }
        if let Some(color) = item.auxiliary_for {
            property_count += 1;
            properties.extend(atom(
                b"auxC",
                b"\0\0\0\0urn:mpeg:mpegB:cicp:systems:auxiliary:alpha\0",
            ));
            associations.push(property_count);
            references.extend(atom(
                b"auxl",
                &[id.to_vec(), vec![0, 1], color.to_be_bytes().to_vec()].concat(),
            ));
        }
        if !item.inputs.is_empty() {
            references.extend(atom(
                b"dimg",
                &[
                    id.to_vec(),
                    (item.inputs.len() as u16).to_be_bytes().to_vec(),
                    item.inputs.iter().flat_map(|id| id.to_be_bytes()).collect(),
                ]
                .concat(),
            ));
        }
        if let Some(alpha) = item.premultiplied_with {
            references.extend(atom(
                b"prem",
                &[id.to_vec(), vec![0, 1], alpha.to_be_bytes().to_vec()].concat(),
            ));
        }
    }
    let metadata = [
        vec![0; 4],
        atom(b"hdlr", &[&[0; 8][..], b"pict", &[0; 13]].concat()),
        atom(b"pitm", &[0, 0, 0, 0, 0, 1]),
        atom(b"iloc", &locations),
        atom(b"iinf", &info),
        atom(b"iref", &references),
        atom(
            b"iprp",
            &[atom(b"ipco", &properties), atom(b"ipma", &associations)].concat(),
        ),
        atom(b"idat", &data),
    ]
    .concat();
    fs::write(
        path,
        [
            atom(b"ftyp", b"avif\0\0\0\0avifmif1miaf"),
            atom(b"meta", &metadata),
        ]
        .concat(),
    )
    .expect("grid file");
}

#[test]
fn generated_avif_color_alpha_grids_and_mixed_planes_preserve_pixels() {
    let root = audio_tests::root("avif-generated-grids");
    let pixels = RgbaImage::from_fn(128, 128, |x, y| {
        image::Rgba([
            (x * 31 + y * 7) as u8,
            (y * 47) as u8,
            (x * 13 + y * 11) as u8,
            (x * 23 + y * 17) as u8,
        ])
    });
    let expected = image::imageops::crop_imm(&pixels, 0, 0, 125, 123).to_image();
    let plane = |image: RgbaImage, alpha: bool| {
        if alpha {
            DynamicImage::ImageLuma8(image::GrayImage::from_fn(
                image.width(),
                image.height(),
                |x, y| image::Luma([image.get_pixel(x, y)[3]]),
            ))
        } else {
            DynamicImage::ImageRgb8(DynamicImage::ImageRgba8(image).into_rgb8())
        }
    };
    let single: Vec<_> = [false, true]
        .map(|alpha| encoded(&root, plane(expected.clone(), alpha), alpha))
        .into();
    let tiles: Vec<Vec<_>> = [false, true]
        .map(|alpha| {
            (0..4)
                .map(|i| {
                    let tile = image::imageops::crop_imm(&pixels, i % 2 * 64, i / 2 * 64, 64, 64)
                        .to_image();
                    encoded(&root, plane(tile, alpha), alpha)
                })
                .collect()
        })
        .into();
    let source = root.join("grid.avif");
    let target = root.join("saved.avif");
    let resaved = root.join("resaved.avif");
    for (color_grid, alpha_grid, tile_alpha) in [
        (true, true, true),
        (true, true, false),
        (true, false, false),
        (false, true, false),
        (false, false, false),
    ] {
        let mut items = single.clone();
        for (index, is_grid) in [color_grid, alpha_grid].into_iter().enumerate() {
            if is_grid {
                let first = items.len() as u16 + 1;
                items[index] = grid((first..first + 4).collect(), index == 1);
                items[index].properties.extend(
                    single[index]
                        .properties
                        .iter()
                        .filter(|(kind, _)| kind == b"colr")
                        .cloned(),
                );
                items.extend(tiles[index].clone());
            }
        }
        if tile_alpha {
            let alpha_ids = items.remove(1).inputs;
            for color in &mut items[0].inputs {
                *color -= 1;
            }
            for (color, alpha) in items[0].inputs.clone().into_iter().zip(alpha_ids) {
                items[alpha as usize - 2].auxiliary_for = Some(color);
            }
        } else {
            items[1].auxiliary_for = Some(1);
        }
        fixture(&items, &source);
        let intact = fs::read(&source).expect("fixture bytes");
        let clap = [121u32, 1, 119, 1, 0, 1, 0, 1]
            .map(u32::to_be_bytes)
            .concat();
        for transforms in if tile_alpha {
            &[0, 1][..]
        } else {
            &[0, 1, 2][..]
        } {
            fs::write(&source, &intact).expect("restore fixture");
            let expected = if *transforms == 0 {
                expected.clone()
            } else {
                orientation_tests::append_still_properties(
                    &source,
                    &[(b"clap", clap.clone()), (b"irot", vec![1])],
                    if *transforms == 1 { &[1] } else { &[1, 2] },
                );
                image::imageops::rotate270(
                    &image::imageops::crop_imm(&expected, 2, 2, 121, 119).to_image(),
                )
            };
            let decoded = crate::decode_image(&source).unwrap_or_else(|error| {
                panic!("color_grid={color_grid}, alpha_grid={alpha_grid}, tile_alpha={tile_alpha}, transforms={transforms}: {error}")
            });
            assert_eq!(decoded.dimensions(), expected.dimensions());
            assert_eq!(decoded.frames.len(), 1);
            assert!(
                decoded.frames[0].rgba == *expected.as_raw(),
                "color_grid={color_grid}, alpha_grid={alpha_grid}, tile_alpha={tile_alpha}, transforms={transforms}: first mismatch {:?}",
                decoded.frames[0]
                    .rgba
                    .iter()
                    .zip(expected.as_raw())
                    .position(|(a, b)| a != b)
            );
            let preview = crate::image::first_animation_frame(&source, 128 * 128 * 4, &|| true)
                .expect("preview")
                .expect("frame");
            assert_eq!(preview.rgba, *expected.as_raw());
            if tile_alpha {
                let cache_path = root.join(format!("cache-{transforms}"));
                crate::PreviewCache::new(cache_path.clone())
                    .expect("cache")
                    .filmstrip(&source, MediaKind::Image)
                    .expect("persist tile alpha");
                let cached = crate::PreviewCache::new(cache_path)
                    .expect("fresh cache")
                    .cached_image(&source)
                    .expect("lookup")
                    .expect("persisted preview");
                assert_eq!(cached.source_size, expected.dimensions());
                assert_eq!(
                    cached.image.rgba,
                    DynamicImage::ImageRgba8(expected.clone())
                        .resize(240, 160, image::imageops::FilterType::Nearest)
                        .into_rgba8()
                        .into_raw()
                );
            }
            assert!(matches!(
                crate::image::first_animation_frame(&source, 128 * 128 * 4, &|| false),
                Err(crate::ImageDecodeError::Cancelled)
            ));
            assert!(crate::image::first_animation_frame(&source, 1024, &|| true).is_err());
            export_media(&request(&source, &target)).expect("grid save");
            assert_eq!(
                crate::decode_image(&target).expect("saved pixels").frames[0].rgba,
                *expected.as_raw()
            );
            let mut edited = request(&source, &target);
            edited.operations = vec![EditOperation::FlipHorizontal];
            export_media(&edited).expect("edited grid save");
            export_media(&request(&target, &resaved)).expect("resave");
            assert_eq!(
                crate::decode_image(&resaved)
                    .expect("resaved pixels")
                    .frames[0]
                    .rgba,
                image::imageops::flip_horizontal(&expected).into_raw()
            );
        }
        fs::write(&source, &intact).expect("restore fixture");
        // Explicit alpha-only geometry must not silently shift the alpha pixels.
        orientation_tests::append_still_properties(
            &source,
            &[(b"clap", clap)],
            if tile_alpha { &[7] } else { &[2] },
        );
        assert!(crate::decode_image(&source).is_err());
        let before = fs::read(&target).expect("saved target");
        assert!(export_media(&request(&source, &target)).is_err());
        assert_eq!(fs::read(&target).expect("protected target"), before);
        if tile_alpha {
            for defect in 0..3 {
                let mut damaged = items.clone();
                match defect {
                    0 => damaged[5].auxiliary_for = None,
                    1 => damaged.push(damaged[5].clone()),
                    _ => {
                        damaged[5] = single[1].clone();
                        damaged[5].auxiliary_for = Some(2);
                    }
                }
                fixture(&damaged, &source);
                assert!(
                    crate::decode_image(&source).is_err(),
                    "tile-alpha defect {defect}"
                );
                assert!(export_media(&request(&source, &target)).is_err());
                assert_eq!(fs::read(&target).expect("protected target"), before);
            }
            let premultiplied = RgbaImage::from_fn(128, 128, |x, y| {
                let mut pixel = *pixels.get_pixel(x, y);
                for channel in 0..3 {
                    pixel[channel] =
                        ((u32::from(pixel[channel]) * u32::from(pixel[3]) + 127) / 255) as u8;
                }
                pixel
            });
            let prem_tiles: Vec<_> = (0..4)
                .map(|i| {
                    encoded(
                        &root,
                        plane(
                            image::imageops::crop_imm(
                                &premultiplied,
                                i % 2 * 64,
                                i / 2 * 64,
                                64,
                                64,
                            )
                            .to_image(),
                            false,
                        ),
                        false,
                    )
                })
                .collect();
            for count in [1, 4] {
                let mut prem_items = items.clone();
                for i in 0..count {
                    prem_items[i + 1] = prem_tiles[i].clone();
                    prem_items[i + 1].premultiplied_with = Some(i as u16 + 6);
                }
                fixture(&prem_items, &source);
                let restored = RgbaImage::from_fn(125, 123, |x, y| {
                    if y / 64 * 2 + x / 64 >= count as u32 {
                        return *pixels.get_pixel(x, y);
                    }
                    let mut pixel = *premultiplied.get_pixel(x, y);
                    for channel in 0..3 {
                        pixel[channel] = if pixel[3] == 0 {
                            0
                        } else {
                            (f64::from(pixel[channel]) * 255.0 / f64::from(pixel[3]))
                                .round()
                                .min(255.0) as u8
                        };
                    }
                    pixel
                });
                assert_eq!(
                    crate::decode_image(&source)
                        .expect("per-tile premultiplication")
                        .frames[0]
                        .rgba,
                    *restored.as_raw()
                );
                let preview = crate::image::first_animation_frame(&source, 128 * 128 * 4, &|| true)
                    .expect("prem preview")
                    .expect("frame");
                assert_eq!(preview.rgba, *restored.as_raw());
                let mut edited = request(&source, &target);
                edited.operations = vec![EditOperation::FlipHorizontal];
                export_media(&edited).expect("prem tile save");
                export_media(&request(&target, &resaved)).expect("prem tile resave");
                assert_eq!(
                    crate::decode_image(&resaved)
                        .expect("prem saved pixels")
                        .frames[0]
                        .rgba,
                    image::imageops::flip_horizontal(&restored).into_raw()
                );
            }
        }
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}
