use super::*;
use std::io::Write;

#[path = "export_png_animation.rs"]
mod animation;

const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const TEXT_LIMIT: usize = 1024 * 1024;
const TEXT_COUNT_LIMIT: usize = 128;

#[derive(Debug, PartialEq)]
struct TextChunk {
    kind: [u8; 4],
    data: Vec<u8>,
    field: MetadataField,
    text: String,
}

fn invalid(message: &str) -> ExportError {
    ExportError::Failed(format!("PNG metadata: {message}"))
}

fn keyword(field: MetadataField) -> &'static str {
    match field {
        MetadataField::Artist => "Author",
        MetadataField::AlbumArtist => "Album Artist",
        MetadataField::Date => "Creation Time",
        _ => field.label(),
    }
}

pub(super) fn png_path(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("png") || extension.eq_ignore_ascii_case("apng")
        })
}

fn terminated<'a>(data: &mut &'a [u8]) -> Result<&'a [u8], ExportError> {
    let end = data
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| invalid("missing text separator"))?;
    let value = &data[..end];
    *data = &data[end + 1..];
    Ok(value)
}

fn utf8(data: &[u8]) -> Result<String, ExportError> {
    String::from_utf8(data.to_vec()).map_err(|_| invalid("invalid UTF-8 text"))
}

fn latin1(data: &[u8]) -> String {
    data.iter().map(|byte| char::from(*byte)).collect()
}

fn inflate(data: &[u8], cancelled: &AtomicBool) -> Result<Vec<u8>, ExportError> {
    let mut decoder = flate2::Decompress::new(true);
    let mut result = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        check_cancelled(cancelled)?;
        let before_in = decoder.total_in();
        let before_out = decoder.total_out();
        // Finish on the first call would require space for the entire expanded text.
        let status = decoder
            .decompress(
                &data[before_in as usize..],
                &mut buffer,
                flate2::FlushDecompress::None,
            )
            .map_err(|_| invalid("invalid compressed text"))?;
        let count = (decoder.total_out() - before_out) as usize;
        if result.len() + count > TEXT_LIMIT {
            return Err(invalid("expanded text exceeds 1 MiB"));
        }
        result.extend_from_slice(&buffer[..count]);
        if status == flate2::Status::StreamEnd {
            if decoder.total_in() as usize != data.len() {
                return Err(invalid("trailing compressed text bytes"));
            }
            return Ok(result);
        }
        if before_in == decoder.total_in() && count == 0 {
            return Err(invalid("incomplete compressed text"));
        }
    }
}

fn parse_text(
    kind: [u8; 4],
    data: &[u8],
    cancelled: &AtomicBool,
) -> Result<(Option<MetadataField>, String), ExportError> {
    let mut rest = data;
    let key = terminated(&mut rest)?;
    if key.is_empty()
        || key.len() > 79
        || key.first() == Some(&b' ')
        || key.last() == Some(&b' ')
        || key.windows(2).any(|pair| pair == b"  ")
        || !key.iter().all(|byte| matches!(*byte, 32..=126 | 161..=255))
    {
        return Err(invalid("invalid text keyword"));
    }
    let key = latin1(key);
    let field = MetadataField::ALL.into_iter().find(|field| {
        key.eq_ignore_ascii_case(keyword(*field)) || key.eq_ignore_ascii_case(field.key())
    });
    let text = match &kind {
        b"tEXt" => latin1(rest),
        b"zTXt" => {
            if rest.first() != Some(&0) {
                return Err(invalid("unsupported text compression method"));
            }
            latin1(&inflate(&rest[1..], cancelled)?)
        }
        b"iTXt" => {
            if rest.len() < 2 || rest[0] > 1 || rest[1] != 0 {
                return Err(invalid("invalid international text compression"));
            }
            let compressed = rest[0] == 1;
            rest = &rest[2..];
            let language = terminated(&mut rest)?;
            if !language
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
            {
                return Err(invalid("invalid text language tag"));
            }
            utf8(terminated(&mut rest)?)?;
            if compressed {
                utf8(&inflate(rest, cancelled)?)?
            } else {
                utf8(rest)?
            }
        }
        _ => unreachable!("only PNG text chunks are parsed"),
    };
    if text.contains('\0') {
        return Err(invalid("text contains NUL"));
    }
    Ok((field, text))
}

fn write_chunk(writer: &mut dyn Write, kind: &[u8; 4], data: &[u8]) -> std::io::Result<()> {
    writer.write_all(&(data.len() as u32).to_be_bytes())?;
    writer.write_all(kind)?;
    writer.write_all(data)?;
    let mut crc = crc32fast::Hasher::new();
    crc.update(kind);
    crc.update(data);
    writer.write_all(&crc.finalize().to_be_bytes())
}

/// Streams image bytes with bounded text and animation controls. Rewriting never decodes IDAT.
fn scan(
    reader: impl Read,
    replacement: Option<(&mut dyn Write, &[TextChunk])>,
    cancelled: &AtomicBool,
) -> Result<Vec<TextChunk>, ExportError> {
    Ok(scan_contents(reader, replacement, None, cancelled)?.0)
}

fn scan_contents(
    mut reader: impl Read,
    mut replacement: Option<(&mut dyn Write, &[TextChunk])>,
    expected_animation: Option<&animation::Animation>,
    cancelled: &AtomicBool,
) -> Result<(Vec<TextChunk>, Option<animation::Animation>), ExportError> {
    check_cancelled(cancelled)?;
    let mut signature = [0; 8];
    reader
        .read_exact(&mut signature)
        .map_err(ExportError::Output)?;
    if &signature != SIGNATURE {
        return Err(invalid("input is not PNG"));
    }
    if let Some((writer, _)) = &mut replacement {
        writer.write_all(SIGNATURE).map_err(ExportError::Output)?;
    }
    let mut texts = Vec::new();
    let mut text_count = 0;
    let mut stored = 0;
    let mut expanded = 0;
    let mut first = true;
    let mut image_seen = false;
    let mut animation = animation::Scan::default();
    let mut buffer = [0; 65536];
    loop {
        check_cancelled(cancelled)?;
        let mut header = [0; 8];
        reader
            .read_exact(&mut header)
            .map_err(ExportError::Output)?;
        let length = u32::from_be_bytes(header[..4].try_into().expect("four bytes")) as usize;
        let kind: [u8; 4] = header[4..].try_into().expect("four bytes");
        if length > i32::MAX as usize
            || !kind.iter().all(u8::is_ascii_alphabetic)
            || !kind[2].is_ascii_uppercase()
            || (first && (&kind != b"IHDR" || length != 13))
            || (!first && &kind == b"IHDR")
            || (&kind == b"IEND" && (length != 0 || !image_seen))
        {
            return Err(invalid("invalid chunk header or image boundaries"));
        }
        first = false;
        image_seen |= &kind == b"IDAT";
        let prefix_length = animation::Scan::prefix_length(&kind, length)?;
        let is_text = matches!(&kind, b"tEXt" | b"zTXt" | b"iTXt");
        if is_text {
            text_count += 1;
            stored += length;
            if text_count > TEXT_COUNT_LIMIT || stored > TEXT_LIMIT {
                return Err(invalid("stored text exceeds 128 chunks / 1 MiB"));
            }
        }
        if let Some((writer, chunks)) = &mut replacement {
            if &kind == b"IEND" {
                for chunk in *chunks {
                    check_cancelled(cancelled)?;
                    write_chunk(*writer, &chunk.kind, &chunk.data).map_err(ExportError::Output)?;
                }
            }
            if !is_text {
                writer.write_all(&header).map_err(ExportError::Output)?;
            }
        }
        let mut crc = crc32fast::Hasher::new();
        crc.update(&kind);
        let mut data = Vec::new();
        let mut prefix = Vec::new();
        let mut remaining = length;
        while remaining > 0 {
            check_cancelled(cancelled)?;
            let count = remaining.min(buffer.len());
            reader
                .read_exact(&mut buffer[..count])
                .map_err(ExportError::Output)?;
            crc.update(&buffer[..count]);
            let prefix_count = count.min(prefix_length - prefix.len());
            prefix.extend_from_slice(&buffer[..prefix_count]);
            if is_text {
                data.extend_from_slice(&buffer[..count]);
            } else if let Some((writer, _)) = &mut replacement {
                writer
                    .write_all(&buffer[..count])
                    .map_err(ExportError::Output)?;
            }
            remaining -= count;
        }
        let mut checksum = [0; 4];
        reader
            .read_exact(&mut checksum)
            .map_err(ExportError::Output)?;
        if crc.finalize() != u32::from_be_bytes(checksum) {
            return Err(invalid("chunk CRC mismatch"));
        }
        animation.observe(&kind, &prefix, length)?;
        if is_text {
            let (field, text) = parse_text(kind, &data, cancelled)?;
            expanded += text.len();
            if expanded > TEXT_LIMIT {
                return Err(invalid("expanded text exceeds 1 MiB total"));
            }
            if let Some(field) = field {
                texts.push(TextChunk {
                    kind,
                    data,
                    field,
                    text,
                });
            } else if let Some((writer, _)) = &mut replacement {
                write_chunk(*writer, &kind, &data).map_err(ExportError::Output)?;
            }
        } else if let Some((writer, _)) = &mut replacement {
            writer.write_all(&checksum).map_err(ExportError::Output)?;
        }
        if &kind == b"IEND" {
            if reader.read(&mut buffer[..1]).map_err(ExportError::Output)? != 0 {
                return Err(invalid("trailing bytes after IEND"));
            }
            let animation = animation.finish()?;
            if expected_animation.is_some_and(|expected| animation.as_ref() != Some(expected)) {
                return Err(invalid("staged animation differs from source controls"));
            }
            return Ok((texts, animation));
        }
    }
}

fn read(path: &Path, cancelled: &AtomicBool) -> Result<Vec<TextChunk>, ExportError> {
    scan(
        BufReader::new(fs::File::open(path).map_err(ExportError::Output)?),
        None,
        cancelled,
    )
}

pub(super) fn inspect(path: &Path) -> Result<Vec<MetadataSourceValue>, ExportError> {
    if !png_path(path) {
        return Err(invalid(
            "image text inspection currently requires PNG input",
        ));
    }
    Ok(read(path, &AtomicBool::new(false))?
        .into_iter()
        .map(|chunk| MetadataSourceValue {
            field: chunk.field,
            scope: "PNG text".into(),
            value: chunk.text[..chunk.text.floor_char_boundary(1024)].to_owned(),
            truncated: chunk.text.len() > 1024,
        })
        .collect())
}

pub(super) struct PngMetadata {
    chunks: Vec<TextChunk>,
    animation: Option<animation::Animation>,
}

impl PngMetadata {
    pub(super) fn from_animation(plays: u32, delays: Vec<[u8; 4]>) -> Self {
        Self {
            chunks: Vec::new(),
            animation: Some(animation::Animation {
                plays,
                delays,
                includes_default: true,
            }),
        }
    }

    pub(super) fn prepare(
        request: &ExportRequest,
        options: &MetadataExportOptions,
        cancelled: &AtomicBool,
    ) -> Result<Self, ExportError> {
        if !png_path(&request.source) || !png_path(&request.target) {
            return Err(invalid(
                "image text export currently requires PNG input and PNG output",
            ));
        }
        let (mut chunks, animation) = scan_contents(
            BufReader::new(fs::File::open(&request.source).map_err(ExportError::Output)?),
            None,
            None,
            cancelled,
        )?;
        chunks.retain(|chunk| options.get(chunk.field).is_none());
        for field in MetadataField::ALL {
            if let Some(text) = options.get(field).filter(|text| !text.is_empty()) {
                let mut data = keyword(field).as_bytes().to_vec();
                // Keyword terminator, no compression, method zero, empty language/translation.
                data.extend_from_slice(&[0; 5]);
                data.extend_from_slice(text.as_bytes());
                chunks.push(TextChunk {
                    kind: *b"iTXt",
                    data,
                    field,
                    text: text.into(),
                });
            }
        }
        Ok(Self { chunks, animation })
    }

    pub(super) fn is_animated(&self) -> bool {
        self.animation.is_some()
    }

    pub(super) fn prepare_animation_source(
        &self,
        request: &ExportRequest,
        staging: &StagedExport,
        cancelled: &AtomicBool,
    ) -> Result<Option<PathBuf>, ExportError> {
        if !self.is_animated() {
            return Ok(None);
        }
        // Share display compositing for every APNG: FFmpeg rounds OVER differently,
        // and cannot demux some valid partial first frames with a separate poster.
        let source = staging.directory.join("animation-source.png");
        {
            let mut output = std::io::BufWriter::new(
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&source)
                    .map_err(ExportError::Output)?,
            );
            let result = crate::image::apng::write_frames(&request.source, &mut output, &|| {
                !cancelled.load(Ordering::Relaxed)
            });
            check_cancelled(cancelled)?;
            result.map_err(|error| {
                invalid(&format!("animation frame preparation failed: {error}"))
            })?;
            output.flush().map_err(ExportError::Output)?;
        }
        Ok(Some(source))
    }

    pub(super) fn apply(
        &self,
        staging: &StagedExport,
        cancelled: &AtomicBool,
    ) -> Result<(), ExportError> {
        if let Some(animation) = &self.animation {
            let temporary = staging.directory.join("animation.png");
            {
                let input =
                    BufReader::new(fs::File::open(&staging.output).map_err(ExportError::Output)?);
                let mut output = std::io::BufWriter::new(
                    fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&temporary)
                        .map_err(ExportError::Output)?,
                );
                animation.assemble(input, &mut output, cancelled)?;
                output.flush().map_err(ExportError::Output)?;
            }
            check_cancelled(cancelled)?;
            fs::rename(&temporary, &staging.output).map_err(ExportError::Output)?;
        }
        let temporary = staging.directory.join("metadata.png");
        {
            let input =
                BufReader::new(fs::File::open(&staging.output).map_err(ExportError::Output)?);
            let mut output = std::io::BufWriter::new(
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&temporary)
                    .map_err(ExportError::Output)?,
            );
            scan_contents(
                input,
                Some((&mut output, &self.chunks)),
                self.animation.as_ref(),
                cancelled,
            )?;
            output.flush().map_err(ExportError::Output)?;
        }
        let (texts, animation) = scan_contents(
            BufReader::new(fs::File::open(&temporary).map_err(ExportError::Output)?),
            None,
            None,
            cancelled,
        )?;
        if texts != self.chunks || animation != self.animation {
            return Err(invalid(
                "staged text or animation did not retain the requested values",
            ));
        }
        check_cancelled(cancelled)?;
        fs::rename(&temporary, &staging.output).map_err(ExportError::Output)
    }
}

pub(super) fn require_static(path: &Path, cancelled: &AtomicBool) -> Result<(), ExportError> {
    let (_, animation) = scan_contents(
        BufReader::new(fs::File::open(path).map_err(ExportError::Output)?),
        None,
        None,
        cancelled,
    )?;
    if animation.is_some() {
        return Err(invalid("animation export requires PNG or APNG output"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "export_png_metadata_tests.rs"]
mod tests;
