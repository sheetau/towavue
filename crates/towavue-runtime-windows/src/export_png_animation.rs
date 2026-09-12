use super::*;

const FRAME_LIMIT: usize = 65_536;

#[derive(Debug, PartialEq)]
pub(super) struct Animation {
    pub(super) plays: u32,
    pub(super) delays: Vec<[u8; 4]>,
    pub(super) includes_default: bool,
}

impl Animation {
    /// Join independently encoded RGBA PNG frames without decoding or changing IDAT bytes.
    pub(super) fn assemble(
        &self,
        mut reader: impl Read,
        writer: &mut dyn Write,
        cancelled: &AtomicBool,
    ) -> Result<(), ExportError> {
        let mut canvas = Vec::new();
        let mut sequence = 0u32;
        let mut buffer = [0; 65536];
        writer.write_all(SIGNATURE).map_err(ExportError::Output)?;
        for (index, delay) in self.delays.iter().enumerate() {
            check_cancelled(cancelled)?;
            let mut signature = [0; 8];
            reader
                .read_exact(&mut signature)
                .map_err(ExportError::Output)?;
            if &signature != SIGNATURE {
                return Err(invalid("missing encoded PNG frame"));
            }
            let mut first = true;
            let mut image_seen = false;
            loop {
                check_cancelled(cancelled)?;
                let mut header = [0; 8];
                reader
                    .read_exact(&mut header)
                    .map_err(ExportError::Output)?;
                let length = number(&header, 0) as usize;
                let kind: [u8; 4] = header[4..].try_into().expect("kind");
                if length > i32::MAX as usize - 4
                    || (first && (kind != *b"IHDR" || length != 13))
                    || (!first && kind == *b"IHDR")
                    || matches!(&kind, b"acTL" | b"fcTL" | b"fdAT")
                    || (&kind == b"IEND" && (length != 0 || !image_seen))
                {
                    return Err(invalid("invalid encoded PNG frame structure"));
                }
                first = false;
                if &kind == b"IDAT" && !image_seen {
                    if index == 0 {
                        write_chunk(
                            writer,
                            b"acTL",
                            &[
                                &(self.delays.len() as u32).to_be_bytes()[..],
                                &self.plays.to_be_bytes(),
                            ]
                            .concat(),
                        )
                        .map_err(ExportError::Output)?;
                    }
                    let mut control = sequence.to_be_bytes().to_vec();
                    sequence = sequence
                        .checked_add(1)
                        .ok_or_else(|| invalid("APNG sequence limit"))?;
                    control.extend_from_slice(&canvas[..8]);
                    control.extend_from_slice(&[0; 8]);
                    control.extend_from_slice(delay);
                    control.extend_from_slice(&[0, 0]);
                    write_chunk(writer, b"fcTL", &control).map_err(ExportError::Output)?;
                }
                image_seen |= &kind == b"IDAT";
                let copy = (index == 0 && &kind != b"IEND") || &kind == b"IDAT";
                let frame_data = index != 0 && &kind == b"IDAT";
                let mut output_crc = crc32fast::Hasher::new();
                if copy {
                    if frame_data {
                        writer
                            .write_all(&((length + 4) as u32).to_be_bytes())
                            .map_err(ExportError::Output)?;
                        writer.write_all(b"fdAT").map_err(ExportError::Output)?;
                        writer
                            .write_all(&sequence.to_be_bytes())
                            .map_err(ExportError::Output)?;
                        output_crc.update(b"fdAT");
                        output_crc.update(&sequence.to_be_bytes());
                        sequence = sequence
                            .checked_add(1)
                            .ok_or_else(|| invalid("APNG sequence limit"))?;
                    } else {
                        writer.write_all(&header).map_err(ExportError::Output)?;
                        output_crc.update(&kind);
                    }
                }
                let mut input_crc = crc32fast::Hasher::new();
                input_crc.update(&kind);
                let mut remaining = length;
                while remaining > 0 {
                    check_cancelled(cancelled)?;
                    let count = remaining.min(buffer.len());
                    reader
                        .read_exact(&mut buffer[..count])
                        .map_err(ExportError::Output)?;
                    input_crc.update(&buffer[..count]);
                    if &kind == b"IHDR" {
                        if index == 0 {
                            canvas.extend_from_slice(&buffer[..count]);
                        } else if canvas != buffer[..count] {
                            return Err(invalid("encoded PNG frame formats differ"));
                        }
                        if buffer[8..13] != [8, 6, 0, 0, 0] {
                            return Err(invalid("encoded animation must use 8-bit RGBA PNG"));
                        }
                    }
                    if copy {
                        writer
                            .write_all(&buffer[..count])
                            .map_err(ExportError::Output)?;
                        output_crc.update(&buffer[..count]);
                    }
                    remaining -= count;
                }
                let mut checksum = [0; 4];
                reader
                    .read_exact(&mut checksum)
                    .map_err(ExportError::Output)?;
                if input_crc.finalize() != u32::from_be_bytes(checksum) {
                    return Err(invalid("encoded PNG CRC mismatch"));
                }
                if copy {
                    writer
                        .write_all(&output_crc.finalize().to_be_bytes())
                        .map_err(ExportError::Output)?;
                }
                if &kind == b"IEND" {
                    break;
                }
            }
        }
        if reader.read(&mut buffer[..1]).map_err(ExportError::Output)? != 0 {
            return Err(invalid("extra encoded animation frames"));
        }
        write_chunk(writer, b"IEND", &[]).map_err(ExportError::Output)
    }
}

#[derive(Default)]
pub(super) struct Scan {
    animation: Option<Animation>,
    expected: usize,
    sequence: u64,
    canvas: (u32, u32),
    image_seen: bool,
    image_closed: bool,
    frame_data: bool,
}

fn number(data: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(
        data[offset..offset + 4]
            .try_into()
            .expect("validated PNG header"),
    )
}

impl Scan {
    pub(super) fn prefix_length(kind: &[u8; 4], length: usize) -> Result<usize, ExportError> {
        let count = match kind {
            b"IHDR" => 13,
            b"acTL" => 8,
            b"fcTL" => 26,
            b"fdAT" if length >= 4 => return Ok(4),
            b"fdAT" => return Err(invalid("short APNG frame data")),
            _ => return Ok(0),
        };
        if length != count {
            return Err(invalid("invalid APNG control length"));
        }
        Ok(count)
    }

    pub(super) fn observe(
        &mut self,
        kind: &[u8; 4],
        data: &[u8],
        length: usize,
    ) -> Result<(), ExportError> {
        if kind != b"IDAT" && self.image_seen {
            self.image_closed = true;
        }
        match kind {
            b"IHDR" => self.canvas = (number(data, 0), number(data, 4)),
            b"acTL" => {
                self.expected = number(data, 0) as usize;
                if self.animation.is_some()
                    || self.image_seen
                    || number(data, 4) > i32::MAX as u32
                    || !(1..=FRAME_LIMIT).contains(&self.expected)
                {
                    return Err(invalid(
                        "APNG requires one pre-image control and 1..=65536 frames",
                    ));
                }
                self.animation = Some(Animation {
                    plays: number(data, 4),
                    delays: Vec::new(),
                    includes_default: false,
                });
            }
            b"fcTL" | b"fdAT" => {
                let animation = self
                    .animation
                    .as_mut()
                    .ok_or_else(|| invalid("APNG frame without animation control"))?;
                if u64::from(number(data, 0)) != self.sequence || self.sequence > i32::MAX as u64 {
                    return Err(invalid("invalid APNG sequence number"));
                }
                self.sequence += 1;
                if kind == b"fcTL" {
                    let first = animation.delays.is_empty();
                    let (width, height) = (number(data, 4), number(data, 8));
                    let (x, y) = (number(data, 12), number(data, 16));
                    if (!first && !self.frame_data)
                        || animation.delays.len() >= self.expected
                        || width == 0
                        || height == 0
                        || x.checked_add(width).is_none_or(|end| end > self.canvas.0)
                        || y.checked_add(height).is_none_or(|end| end > self.canvas.1)
                        || data[24] > 2
                        || data[25] > 1
                        || (!self.image_seen
                            && (!first || x != 0 || y != 0 || (width, height) != self.canvas))
                    {
                        return Err(invalid("invalid APNG frame bounds, data or controls"));
                    }
                    if first {
                        animation.includes_default = !self.image_seen;
                    }
                    animation
                        .delays
                        .push(data[20..24].try_into().expect("delay"));
                    self.frame_data = false;
                } else {
                    if !self.image_seen
                        || animation.delays.is_empty()
                        || (animation.includes_default && animation.delays.len() == 1)
                    {
                        return Err(invalid("misplaced APNG frame data"));
                    }
                    self.frame_data |= length > 4;
                }
            }
            b"IDAT" => {
                if self.image_closed {
                    return Err(invalid("nonconsecutive PNG image data"));
                }
                self.image_seen = true;
                if self
                    .animation
                    .as_ref()
                    .is_some_and(|animation| animation.includes_default)
                {
                    self.frame_data |= length > 0;
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) fn finish(self) -> Result<Option<Animation>, ExportError> {
        if self
            .animation
            .as_ref()
            .is_some_and(|animation| animation.delays.len() != self.expected || !self.frame_data)
        {
            return Err(invalid("APNG frame count or final frame data mismatch"));
        }
        Ok(self.animation)
    }
}
