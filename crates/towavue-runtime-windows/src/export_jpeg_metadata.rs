use super::*;
use std::io::Write;
use towavue_core::localization::Text;

const XMP: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const EXTENDED: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";

fn invalid_reason(reason: Text) -> ExportError {
    crate::ExportFailure::reason(Text::ExportJpegMetadataContext, reason).into()
}

pub(super) fn jpeg_path(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg")
        })
}

fn emit(output: &mut Option<&mut dyn Write>, bytes: &[u8]) -> Result<(), ExportError> {
    if let Some(writer) = output {
        writer.write_all(bytes).map_err(ExportError::Output)?;
    }
    Ok(())
}

/// Copies marker and entropy bytes without interpreting compressed samples, including progressive scans.
fn scan(
    mut input: impl BufRead,
    mut output: Option<&mut dyn Write>,
    replacement: &[u8],
    cancelled: &AtomicBool,
) -> Result<Option<Vec<u8>>, ExportError> {
    let mut signature = [0; 2];
    input
        .read_exact(&mut signature)
        .map_err(ExportError::Output)?;
    if signature != [0xff, 0xd8] {
        return Err(invalid_reason(Text::ExportValidationInputIsNotJpeg));
    }
    emit(&mut output, &signature)?;
    let mut packet = None;
    let mut entropy = false;
    let mut image_seen = false;
    let mut inserted = false;
    let mut count = 0;
    loop {
        check_cancelled(cancelled)?;
        if entropy {
            let available = input.fill_buf().map_err(ExportError::Output)?;
            if available.is_empty() {
                return Err(invalid_reason(Text::ExportValidationMissingEndOfImage));
            }
            let count = available
                .iter()
                .position(|byte| *byte == 0xff)
                .unwrap_or(available.len())
                .min(65536);
            if count > 0 {
                emit(&mut output, &available[..count])?;
                input.consume(count);
                continue;
            }
        }
        let mut byte = [0];
        input.read_exact(&mut byte).map_err(ExportError::Output)?;
        if byte[0] != 0xff {
            return Err(invalid_reason(Text::ExportValidationExpectedMarker));
        }
        let mut marker = vec![0xff];
        loop {
            input.read_exact(&mut byte).map_err(ExportError::Output)?;
            marker.push(byte[0]);
            if marker.len() > 4096 {
                return Err(invalid_reason(Text::ExportValidationExcessiveMarkerPadding));
            }
            if byte[0] != 0xff {
                break;
            }
        }
        let code = byte[0];
        if entropy && (code == 0 || matches!(code, 0xd0..=0xd7)) {
            emit(&mut output, &marker)?;
            continue;
        }
        count += 1;
        if count > 65536 {
            return Err(invalid_reason(Text::ExportValidationTooManyMarkerSegments));
        }
        if !inserted && !matches!(code, 0xe0 | 0xe1) {
            if output.is_some() && !replacement.is_empty() {
                emit(&mut output, &[0xff, 0xe1])?;
                emit(
                    &mut output,
                    &((replacement.len() + XMP.len() + 2) as u16).to_be_bytes(),
                )?;
                emit(&mut output, XMP)?;
                emit(&mut output, replacement)?;
            }
            inserted = true;
        }
        if code == 0xd9 {
            if !image_seen {
                return Err(invalid_reason(Text::ExportValidationJpegHasNoImageScan));
            }
            emit(&mut output, &marker)?;
            if input.read(&mut byte).map_err(ExportError::Output)? != 0 {
                return Err(invalid_reason(
                    Text::ExportValidationTrailingDataAfterJpegImage,
                ));
            }
            return Ok(packet);
        }
        if code == 1 {
            emit(&mut output, &marker)?;
            continue;
        }
        if code == 0 || matches!(code, 0xd0..=0xd8) {
            return Err(invalid_reason(
                Text::ExportValidationUnexpectedStandaloneMarker,
            ));
        }
        let mut length = [0; 2];
        input.read_exact(&mut length).map_err(ExportError::Output)?;
        let length_value = u16::from_be_bytes(length) as usize;
        if length_value < 2 {
            return Err(invalid_reason(Text::ExportValidationInvalidSegmentLength));
        }
        let mut data = vec![0; length_value - 2];
        input.read_exact(&mut data).map_err(ExportError::Output)?;
        if code == 0xe1 && data.starts_with(EXTENDED) {
            return Err(invalid_reason(
                Text::ExportValidationExtendedXmpIsNotSupportedYet,
            ));
        }
        let is_xmp = code == 0xe1 && data.starts_with(XMP);
        if is_xmp {
            if packet.is_some() {
                return Err(invalid_reason(
                    Text::ExportValidationMultipleStandardXmpPackets,
                ));
            }
            if data.len() - XMP.len() > xmp::LIMIT {
                return Err(invalid_reason(
                    Text::ExportValidationXmpPacketExceeds65502Bytes,
                ));
            }
            packet = Some(data[XMP.len()..].to_vec());
        }
        if !is_xmp || output.is_none() {
            emit(&mut output, &marker)?;
            emit(&mut output, &length)?;
            emit(&mut output, &data)?;
        }
        entropy = code == 0xda || (entropy && code == 0xdc);
        image_seen |= code == 0xda;
    }
}

pub(super) fn read(path: &Path, cancelled: &AtomicBool) -> Result<Vec<xmp::Value>, ExportError> {
    read_packet(path, cancelled)?
        .map(|packet| xmp::parse(&packet, cancelled))
        .transpose()
        .map(Option::unwrap_or_default)
}

pub(super) fn read_packet(
    path: &Path,
    cancelled: &AtomicBool,
) -> Result<Option<Vec<u8>>, ExportError> {
    let input = BufReader::new(fs::File::open(path).map_err(ExportError::Output)?);
    scan(input, None, &[], cancelled)
}

pub(super) fn inspect(path: &Path) -> Result<Vec<MetadataSourceValue>, ExportError> {
    Ok(xmp::display_values(
        read(path, &AtomicBool::new(false))?,
        ImageMetadataFormat::Jpeg,
    ))
}

pub(super) struct JpegMetadata {
    values: Vec<xmp::Value>,
    packet: Vec<u8>,
}

impl JpegMetadata {
    pub(super) fn prepare(
        request: &ExportRequest,
        options: &MetadataExportOptions,
        cancelled: &AtomicBool,
    ) -> Result<Self, ExportError> {
        if !(jpeg_path(&request.source) || webp_metadata::webp_path(&request.source))
            || !jpeg_path(&request.target)
        {
            return Err(invalid_reason(
                Text::ExportValidationXmpExportRequiresJpegOrStaticWebpInputAndJpegOutput,
            ));
        }
        let source_packet = if jpeg_path(&request.source) {
            read_packet(&request.source, cancelled)?
        } else {
            webp_metadata::read_for_jpeg(&request.source, cancelled)?
        };
        let mut values = source_packet
            .as_deref()
            .map(|packet| xmp::parse(packet, cancelled))
            .transpose()?
            .unwrap_or_default();
        xmp::apply(&mut values, options)?;
        let packet = if let Some(packet) = source_packet {
            if request.operations.is_empty() && jpeg_path(&request.source) {
                xmp::rewrite_unedited(&packet, options, cancelled)?
            } else {
                xmp::rewrite_edited(&packet, options, cancelled)?
            }
        } else if values.is_empty() {
            Vec::new()
        } else {
            xmp::encode(&values)?
        };
        Ok(Self { values, packet })
    }

    pub(super) fn apply(
        &self,
        staging: &StagedExport,
        cancelled: &AtomicBool,
    ) -> Result<(), ExportError> {
        self.apply_from(&staging.output, staging, cancelled)
    }

    pub(super) fn apply_from(
        &self,
        source: &Path,
        staging: &StagedExport,
        cancelled: &AtomicBool,
    ) -> Result<(), ExportError> {
        let temporary = staging.directory.join("metadata.jpg");
        {
            let input = BufReader::new(fs::File::open(source).map_err(ExportError::Output)?);
            let mut output = std::io::BufWriter::new(
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&temporary)
                    .map_err(ExportError::Output)?,
            );
            scan(input, Some(&mut output), &self.packet, cancelled)?;
            output.flush().map_err(ExportError::Output)?;
        }
        let packet = read_packet(&temporary, cancelled)?;
        if packet.as_deref().unwrap_or_default() != self.packet {
            return Err(invalid_reason(
                Text::ExportValidationStagedXmpDiffersFromThePreparedPacket,
            ));
        }
        if packet
            .map(|packet| xmp::parse(&packet, cancelled))
            .transpose()?
            .unwrap_or_default()
            != self.values
        {
            return Err(invalid_reason(
                Text::ExportValidationStagedXmpDidNotRetainRequestedTextValues,
            ));
        }
        check_cancelled(cancelled)?;
        fs::rename(&temporary, &staging.output).map_err(ExportError::Output)
    }
}

#[cfg(test)]
#[path = "export_jpeg_metadata_tests.rs"]
mod tests;
