use super::*;

pub(crate) struct Still {
    pub(crate) color: u32,
    pub(crate) alpha: Option<u32>,
    pub(crate) premultiplied: bool,
}

fn number(data: &mut &[u8], width: usize) -> Result<u32, Error> {
    let head = data
        .get(..width)
        .ok_or_else(|| invalid("truncated item field"))?;
    let value = head
        .iter()
        .fold(0, |value, byte| (value << 8) | u32::from(*byte));
    *data = &data[width..];
    Ok(value)
}

fn full_children(
    file: &mut fs::File,
    item: BoxRange,
    current: &dyn Fn() -> bool,
) -> Result<(u8, Vec<BoxRange>), Error> {
    if item.end - item.start < 4 {
        return Err(invalid("truncated full box"));
    }
    let header = bytes(
        file,
        BoxRange {
            end: item.start + 4,
            ..item
        },
        4,
    )?;
    if header[1..] != [0, 0, 0] {
        return Err(invalid("unsupported full-box flags"));
    }
    Ok((header[0], boxes(file, item.start + 4, item.end, current)?))
}

fn visit_properties(
    file: &mut fs::File,
    meta: &[BoxRange],
    current: &dyn Fn() -> bool,
    mut visit: impl FnMut(&mut fs::File, u32, BoxRange) -> Result<(), Error>,
) -> Result<(), Error> {
    let properties = one(meta, b"iprp")?.ok_or_else(|| invalid("missing item properties"))?;
    let properties = boxes(file, properties.start, properties.end, current)?;
    let values = one(&properties, b"ipco")?.ok_or_else(|| invalid("missing property container"))?;
    let values = boxes(file, values.start, values.end, current)?;
    for association in properties.iter().filter(|item| &item.kind == b"ipma") {
        let payload = bytes(file, *association, 1024 * 1024)?;
        let mut payload = payload.as_slice();
        let control = number(&mut payload, 4)?;
        if control & 0x00fffffe != 0 || control >> 24 > 1 {
            return Err(invalid("unsupported property association flags/version"));
        }
        let id_width = if control >> 24 == 0 { 2 } else { 4 };
        let wide = control & 1 != 0;
        let count = number(&mut payload, 4)?;
        if count > 65536 {
            return Err(invalid("too many property associations"));
        }
        for _ in 0..count {
            check_current(current)?;
            let id = number(&mut payload, id_width)?;
            let count = number(&mut payload, 1)?;
            for _ in 0..count {
                let index = number(&mut payload, if wide { 2 } else { 1 })?
                    & if wide { 0x7fff } else { 0x7f };
                if index == 0 {
                    continue;
                }
                let value = values
                    .get(index as usize - 1)
                    .ok_or_else(|| invalid("invalid property index"))?;
                visit(file, id, *value)?;
            }
        }
        if !payload.is_empty() {
            return Err(invalid("invalid property association size"));
        }
    }
    Ok(())
}

pub(crate) fn aperture(
    path: &std::path::Path,
    id: u32,
    size: (u32, u32),
    current: &dyn Fn() -> bool,
) -> Result<Option<CleanAperture>, Error> {
    let mut file = fs::File::open(path)?;
    let length = file.metadata()?.len();
    let root = boxes(&mut file, 0, length, current)?;
    let meta = one(&root, b"meta")?.ok_or_else(|| invalid("missing image metadata"))?;
    let (version, meta) = full_children(&mut file, meta, current)?;
    if version != 0 {
        return Err(invalid("unsupported meta version"));
    }
    let mut aperture = None;
    visit_properties(&mut file, &meta, current, |file, item, value| {
        if item == id && value.kind == *b"clap" {
            if aperture.is_some() {
                return Err(invalid("multiple clean aperture properties"));
            }
            aperture = Some(CleanAperture::from_clap(&bytes(file, value, 32)?, size)?);
        }
        Ok(())
    })?;
    Ok(aperture)
}

pub(crate) fn read(
    file: &mut fs::File,
    root: &[BoxRange],
    current: &dyn Fn() -> bool,
) -> Result<Still, Error> {
    let meta = one(root, b"meta")?.ok_or_else(|| invalid("missing image metadata"))?;
    let (version, meta) = full_children(file, meta, current)?;
    if version != 0 {
        return Err(invalid("unsupported meta version"));
    }
    let primary = one(&meta, b"pitm")?.ok_or_else(|| invalid("missing primary item"))?;
    let primary = bytes(file, primary, 8)?;
    let mut primary = primary.as_slice();
    let version = number(&mut primary, 4)?;
    let color = number(
        &mut primary,
        match version {
            0 => 2,
            0x01000000 => 4,
            _ => return Err(invalid("invalid pitm version")),
        },
    )?;
    if !primary.is_empty() || color == 0 {
        return Err(invalid("invalid primary item"));
    }
    let mut auxiliaries = Vec::new();
    let mut premultiplied_with = None;
    if let Some(references) = one(&meta, b"iref")? {
        let (version, references) = full_children(file, references, current)?;
        let width = match version {
            0 => 2,
            1 => 4,
            _ => return Err(invalid("unsupported iref version")),
        };
        for reference in references
            .into_iter()
            .filter(|item| matches!(&item.kind, b"auxl" | b"prem"))
        {
            check_current(current)?;
            let payload = bytes(file, reference, 256 * 1024)?;
            let mut payload = payload.as_slice();
            let from = number(&mut payload, width)?;
            let count = number(&mut payload, 2)?;
            for _ in 0..count {
                let to = number(&mut payload, width)?;
                if &reference.kind == b"auxl" && to == color {
                    auxiliaries.push(from);
                }
                if &reference.kind == b"prem"
                    && from == color
                    && premultiplied_with.replace(to).is_some()
                {
                    return Err(invalid("multiple premultiplied references"));
                }
            }
            if !payload.is_empty() {
                return Err(invalid("invalid item reference size"));
            }
        }
    }
    let mut alpha = None;
    if !auxiliaries.is_empty() {
        visit_properties(file, &meta, current, |file, id, value| {
            if auxiliaries.contains(&id) && &value.kind == b"auxC" {
                let value = bytes(file, value, 4096)?;
                if value == b"\0\0\0\0urn:mpeg:mpegB:cicp:systems:auxiliary:alpha\0" {
                    if alpha.is_some_and(|previous| previous != id) {
                        return Err(invalid("multiple alpha items"));
                    }
                    alpha = Some(id);
                }
            }
            Ok(())
        })?;
    }
    if alpha == Some(color) || (premultiplied_with.is_some() && premultiplied_with != alpha) {
        return Err(invalid("invalid premultiplied alpha item"));
    }
    Ok(Still {
        color,
        alpha,
        premultiplied: premultiplied_with.is_some(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atom(kind: &[u8; 4], payload: Vec<u8>) -> Vec<u8> {
        [
            (payload.len() as u32 + 8).to_be_bytes().as_slice(),
            kind,
            &payload,
        ]
        .concat()
    }

    fn fixture(wide: bool, alpha_type: &[u8]) -> Vec<u8> {
        let id = |value: u32| {
            if wide {
                value.to_be_bytes().to_vec()
            } else {
                (value as u16).to_be_bytes().to_vec()
            }
        };
        let color = if wide { 70000 } else { 1 };
        let alpha = color + 1;
        let header = vec![u8::from(wide), 0, 0, 0];
        let primary = atom(b"pitm", [header.as_slice(), &id(color)].concat());
        let auxiliary = atom(
            b"auxl",
            [id(alpha), 1u16.to_be_bytes().to_vec(), id(color)].concat(),
        );
        let premultiplied = atom(
            b"prem",
            [id(color), 1u16.to_be_bytes().to_vec(), id(alpha)].concat(),
        );
        let references = atom(b"iref", [header, auxiliary, premultiplied].concat());
        let mut properties = Vec::new();
        if wide {
            for _ in 0..128 {
                properties.extend(atom(b"free", vec![]));
            }
        }
        properties.extend(atom(b"auxC", [vec![0; 4], alpha_type.to_vec()].concat()));
        let associations = atom(
            b"ipma",
            [
                vec![u8::from(wide), 0, 0, u8::from(wide)],
                1u32.to_be_bytes().to_vec(),
                id(alpha),
                vec![1],
                if wide {
                    0x8081u16.to_be_bytes().to_vec()
                } else {
                    vec![0x81]
                },
            ]
            .concat(),
        );
        let properties = atom(b"iprp", [atom(b"ipco", properties), associations].concat());
        atom(
            b"meta",
            [vec![0; 4], primary, references, properties].concat(),
        )
    }

    #[test]
    fn clean_aperture_follows_only_selected_item_associations() {
        let root = std::env::temp_dir().join(format!(
            "towavue-avif-apertures-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("timestamp")
                .as_nanos()
        ));
        fs::create_dir(&root).expect("owned test directory");
        let path = root.join("metadata.avif");
        for wide_id in [false, true] {
            for wide_index in [false, true] {
                let selected: u32 = if wide_id { 70000 } else { 1 };
                let index: u16 = if wide_index { 129 } else { 1 };
                let mut values = Vec::new();
                for _ in 1..index {
                    values.extend(atom(b"free", vec![]));
                }
                values.extend(atom(
                    b"clap",
                    [4u32, 1, 2, 1, 1, 2, u32::MAX, 2]
                        .map(u32::to_be_bytes)
                        .concat(),
                ));
                values.extend(atom(b"clap", vec![0; 32]));
                let fixture = |duplicate: bool| {
                    let mut associations = vec![u8::from(wide_id), 0, 0, u8::from(wide_index)];
                    associations.extend(2u32.to_be_bytes());
                    for (id, property, count) in [
                        (selected, index, if duplicate { 2 } else { 1 }),
                        (selected + 1, index + 1, 1),
                    ] {
                        if wide_id {
                            associations.extend(id.to_be_bytes());
                        } else {
                            associations.extend((id as u16).to_be_bytes());
                        }
                        associations.push(count);
                        for _ in 0..count {
                            if wide_index {
                                associations.extend((property | 0x8000).to_be_bytes());
                            } else {
                                associations.push(property as u8 | 0x80);
                            }
                        }
                    }
                    atom(
                        b"meta",
                        [
                            vec![0; 4],
                            atom(
                                b"iprp",
                                [atom(b"ipco", values.clone()), atom(b"ipma", associations)]
                                    .concat(),
                            ),
                        ]
                        .concat(),
                    )
                };
                fs::write(&path, fixture(false)).expect("fixture");
                let rect = aperture(&path, selected, (7, 5), &|| true)
                    .expect("unrelated invalid aperture is ignored")
                    .expect("selected aperture")
                    .rectangle((7, 5))
                    .expect("integer crop");
                assert_eq!((rect.x, rect.y, rect.width, rect.height), (2, 1, 4, 2));
                assert!(aperture(&path, selected + 1, (7, 5), &|| true).is_err());
                assert_eq!(
                    aperture(&path, selected + 2, (7, 5), &|| true).expect("no association"),
                    None
                );
                assert!(matches!(
                    aperture(&path, selected, (7, 5), &|| false),
                    Err(Error::Cancelled)
                ));
                fs::write(&path, fixture(true)).expect("duplicate fixture");
                assert!(aperture(&path, selected, (7, 5), &|| true).is_err());
            }
        }
        fs::remove_dir_all(root).expect("owned fixture cleanup");
    }

    #[test]
    fn item_references_support_wide_ids_properties_and_reject_malformed_controls() {
        let root = std::env::temp_dir().join(format!(
            "towavue-avif-items-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("timestamp")
                .as_nanos()
        ));
        fs::create_dir(&root).expect("owned test directory");
        let path = root.join("metadata.avif");
        let parse = |payload: &[u8]| {
            fs::write(&path, payload).expect("fixture");
            let mut file = fs::File::open(&path).expect("open");
            let root = boxes(&mut file, 0, payload.len() as u64, &|| true)?;
            read(&mut file, &root, &|| true)
        };
        for wide in [false, true] {
            let payload = fixture(wide, b"urn:mpeg:mpegB:cicp:systems:auxiliary:alpha\0");
            let parsed = parse(&payload).expect("valid metadata");
            assert_eq!(parsed.color, if wide { 70000 } else { 1 });
            assert_eq!(parsed.alpha, Some(parsed.color + 1));
            assert!(parsed.premultiplied);
            let association = payload
                .windows(4)
                .position(|kind| kind == b"ipma")
                .expect("association");
            for (offset, value) in [
                (association + 4, 2),
                (association + 7, 2),
                (association + 8, 0xff),
            ] {
                let mut damaged = payload.clone();
                damaged[offset] = value;
                assert!(parse(&damaged).is_err());
            }
            assert!(parse(&payload[..payload.len() - 1]).is_err());
        }
        // A premultiplication reference must resolve to an actual alpha property.
        assert!(parse(&fixture(false, b"not-an-alpha-type\0")).is_err());
        fs::remove_dir_all(root).expect("owned fixture cleanup");
    }
}
