use super::*;
use std::io::{Seek, SeekFrom};

#[path = "export_avif_single.rs"]
mod single;

#[derive(Clone, Copy)]
struct SequenceTiming {
    time_base: ffmpeg::Rational,
    loops: u32,
    single_duration: Option<u32>,
}

fn invalid(error: impl std::fmt::Display) -> ExportError {
    ExportError::Failed(format!("AVIF export: {error}"))
}

pub(super) fn avif_path(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("avif"))
}

use crate::avif_container::{BoxRange, Track, bytes, one};

impl From<crate::avif_container::Error> for ExportError {
    fn from(error: crate::avif_container::Error) -> Self {
        match error {
            crate::avif_container::Error::Cancelled => Self::Cancelled,
            error => invalid(error),
        }
    }
}

fn boxes(
    file: &mut fs::File,
    start: u64,
    end: u64,
    cancelled: &AtomicBool,
) -> Result<Vec<BoxRange>, ExportError> {
    crate::avif_container::boxes(file, start, end, &|| !cancelled.load(Ordering::Relaxed))
        .map_err(Into::into)
}

fn track(
    file: &mut fs::File,
    item: BoxRange,
    cancelled: &AtomicBool,
) -> Result<Track, ExportError> {
    crate::avif_container::track(file, item, &|| !cancelled.load(Ordering::Relaxed), true)
        .map_err(Into::into)
}

#[derive(Clone, Debug)]
struct Samples {
    index: usize,
    id: u32,
    size: (u32, u32),
    time_base: ffmpeg::Rational,
    times: Vec<(i64, i64)>,
    orientation: Option<crate::VideoOrientation>,
    aperture: Option<crate::avif_container::CleanAperture>,
}

#[derive(Clone, Debug)]
pub(super) struct Animation {
    tracks: Vec<Track>,
    samples: Vec<Samples>,
}

impl Animation {
    pub(super) fn read(path: &Path, cancelled: &AtomicBool) -> Result<Option<Self>, ExportError> {
        check_cancelled(cancelled)?;
        let mut file = fs::File::open(path).map_err(ExportError::Output)?;
        let length = file.metadata().map_err(ExportError::Output)?.len();
        let root = boxes(&mut file, 0, length, cancelled)?;
        let ftyp = one(&root, b"ftyp")?.ok_or_else(|| invalid("missing ftyp"))?;
        let brands = bytes(&mut file, ftyp, 4096)?;
        if brands.len() < 8
            || brands.len() % 4 != 0
            || !brands
                .as_chunks::<4>()
                .0
                .iter()
                .enumerate()
                .any(|(i, brand)| i != 1 && matches!(brand, b"avif" | b"avis"))
        {
            return Err(invalid("not an AVIF container"));
        }
        let Some(moov) = one(&root, b"moov")? else {
            return Ok(None);
        };
        let movie = boxes(&mut file, moov.start, moov.end, cancelled)?;
        let tracks = movie
            .iter()
            .filter(|item| &item.kind == b"trak")
            .map(|item| track(&mut file, *item, cancelled))
            .collect::<Result<Vec<_>, _>>()?;
        if tracks.is_empty() || tracks.len() > 2 {
            return Err(invalid("expected a color track and optional alpha track"));
        }
        if tracks
            .iter()
            .enumerate()
            .any(|(i, track)| tracks[..i].iter().any(|old| old.id == track.id))
        {
            return Err(invalid("duplicate track IDs"));
        }
        let color = tracks
            .iter()
            .find(|track| track.alpha_for.is_none())
            .ok_or_else(|| invalid("missing color track"))?;
        if tracks.len() == 2 && !tracks.iter().any(|track| track.alpha_for == Some(color.id)) {
            return Err(invalid("second track is not linked alpha"));
        }
        ffmpeg::init().map_err(invalid)?;
        // AVIF sample durations are unsigned; do not apply MOV's legacy
        // negative-DTS correction heuristic to long image holds.
        let mut options = ffmpeg::Dictionary::new();
        options.set("max_stts_delta", &u32::MAX.to_string());
        options.set("err_detect", "explode");
        let mut input = ffmpeg::format::input_with_dictionary(path, options).map_err(invalid)?;
        let mut samples = Vec::new();
        for track in &tracks {
            // The demuxer also exposes primary still items, often with the same ID.
            // Select the timed track, not its single-frame cover image.
            let mut matching = input.streams().filter(|stream| {
                stream.id() as u32 == track.id
                    && stream.time_base() == ffmpeg::Rational(1, track.timescale as i32)
                    && stream.duration() > 0
                    && stream.duration() as u64 == track.duration
            });
            let stream = matching
                .next()
                .ok_or_else(|| invalid("missing timed AVIF track"))?;
            if matching.next().is_some() {
                return Err(invalid("ambiguous timed AVIF track"));
            }
            if stream.parameters().medium() != ffmpeg::media::Type::Video
                || stream.parameters().id() != ffmpeg::codec::Id::AV1
            {
                return Err(invalid("AVIF sequence contains a non-AV1 video stream"));
            }
            let id = u32::try_from(stream.id()).map_err(invalid)?;
            let matrix = stream
                .side_data()
                .find(|data| data.kind() == ffmpeg::codec::packet::side_data::Type::DisplayMatrix);
            let orientation = matrix
                .as_ref()
                .map(|data| crate::VideoOrientation::from_bytes(Some(data.data())))
                .transpose()
                .map_err(invalid)?;
            let decoder = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
                .and_then(|context| context.decoder().video())
                .map_err(invalid)?;
            let size = (decoder.width(), decoder.height());
            crate::image_edits::output_size(size, &[]).map_err(invalid)?;
            let aperture = stream
                .side_data()
                .find(|data| data.kind() == ffmpeg::codec::packet::side_data::Type::FRAME_CROPPING)
                .map(|data| crate::avif_container::CleanAperture::from_bytes(data.data()))
                .transpose()?;
            if let Some(aperture) = aperture {
                aperture.rectangle(size)?;
            }
            samples.push(Samples {
                index: stream.index(),
                id,
                size,
                time_base: stream.time_base(),
                times: Vec::new(),
                orientation,
                aperture,
            });
        }
        loop {
            check_cancelled(cancelled)?;
            let mut packet = ffmpeg::Packet::empty();
            match packet.read(&mut input) {
                Ok(()) => {}
                Err(ffmpeg::Error::Eof) => break,
                Err(error) => return Err(invalid(error)),
            }
            let Some(samples) = samples
                .iter_mut()
                .find(|sample| sample.index == packet.stream())
            else {
                continue;
            };
            if packet.is_corrupt() || packet.size() == 0 {
                return Err(invalid("corrupt or empty AVIF sample"));
            }
            if samples.times.len() == 65536 {
                return Err(invalid("animation exceeds 65536 frames"));
            }
            samples.times.push((
                packet.pts().ok_or_else(|| invalid("missing sample PTS"))?,
                packet.duration(),
            ));
        }
        for samples in &mut samples {
            samples.times.sort_unstable_by_key(|(time, _)| *time);
            let declared = input
                .stream(samples.index)
                .expect("selected stream")
                .frames();
            if samples.times.is_empty()
                || (declared > 0 && declared as u64 != samples.times.len() as u64)
                || samples.times.iter().any(|(_, duration)| *duration <= 0)
                || samples.times.windows(2).any(|pair| pair[0].0 >= pair[1].0)
            {
                return Err(invalid("missing or nonpositive sample timing"));
            }
        }
        samples.sort_by_key(|sample| sample.id != color.id);
        if samples.get(1).is_some_and(|alpha| {
            alpha.size != samples[0].size
                || alpha
                    .aperture
                    .is_some_and(|value| value != samples[0].aperture.unwrap_or_default())
                || alpha
                    .orientation
                    .is_some_and(|value| value != samples[0].orientation.unwrap_or_default())
                || !same_timing(&samples[0], alpha)
                || i128::from(alpha.times[0].0) * i128::from(samples[0].time_base.denominator())
                    != i128::from(samples[0].times[0].0) * i128::from(alpha.time_base.denominator())
        }) {
            return Err(invalid(
                "alpha dimensions, aperture, orientation or sample timing differ from color",
            ));
        }
        Ok(Some(Self { tracks, samples }))
    }

    fn color(&self) -> &Track {
        self.tracks
            .iter()
            .find(|track| track.id == self.samples[0].id)
            .expect("color track")
    }

    pub(super) fn export(
        &self,
        request: &ExportRequest,
        staging: &StagedExport,
        cancelled: &AtomicBool,
        progress: &(impl Fn(Duration) + Sync),
    ) -> Result<(), ExportError> {
        use std::io::Write;
        let single_duration = (self.samples[0].times.len() == 1)
            .then(|| u32::try_from(self.samples[0].times[0].1).map_err(invalid))
            .transpose()?;
        let source_alpha = self.samples.get(1);
        let orientation = self.samples[0].orientation.unwrap_or_default();
        let mut source_size = self.samples[0].size;
        let aperture = self.samples[0]
            .aperture
            .map(|value| value.rectangle(source_size))
            .transpose()?;
        if let Some(aperture) = aperture {
            source_size = (aperture.width, aperture.height);
        }
        if orientation.swaps_axes() {
            source_size = (source_size.1, source_size.0);
        }
        let output_size =
            crate::image_edits::output_size(source_size, &request.operations).map_err(invalid)?;
        if let Some(alpha_id) = self.color().premultiplied_with
            && source_alpha.is_none_or(|sample| sample.id != alpha_id)
        {
            return Err(invalid("invalid premultiplied alpha reference"));
        }
        let alpha_output = source_alpha.is_some()
            || request
                .operations
                .iter()
                .any(|operation| matches!(operation, EditOperation::RotateImage(_)));
        let mut filters = if let Some(alpha) = source_alpha {
            format!(
                "[0:{}]scale=flags=bilinear,format=rgba[color];[0:{}]format=gray[alpha];[color][alpha]alphamerge",
                self.samples[0].index, alpha.index
            )
        } else {
            format!(
                "[0:{}]scale=flags=bilinear,format=rgba",
                self.samples[0].index
            )
        };
        if self.color().premultiplied_with.is_some() {
            // Match the display decoder's RGBA8 rounding and zero-alpha rule.
            // alphamerge labels its output straight: unpremultiply would insert
            // another premultiplication, losing low-alpha color precision.
            filters.push_str(",format=gbrap,geq=");
            for channel in ["r", "g", "b"] {
                filters.push_str(&format!(
                    "{channel}='if(gt(alpha(X,Y),0),min(255,floor({channel}(X,Y)*255/alpha(X,Y)+0.5)),0)':"
                ));
            }
            filters.push_str("a='alpha(X,Y)',format=rgba");
        }
        // Match display: merge full-size planes, then apply the color aperture
        // and orientation once. Omitted legacy alpha transforms follow color.
        if let Some(crop) = aperture {
            filters.push_str(&format!(
                ",crop={}:{}:{}:{}:exact=1",
                crop.width, crop.height, crop.x, crop.y
            ));
        }
        let orientation = orientation.ffmpeg_filter().trim_end_matches(',');
        if !orientation.is_empty() {
            filters.push(',');
            filters.push_str(orientation);
        }
        for filter in visual_filters(&request.operations) {
            filters.push(',');
            filters.push_str(&filter);
        }
        if alpha_output {
            filters.push_str(",split[color][alpha];[color]format=gbrp[outv];[alpha]alphaextract,format=gray,setparams=colorspace=bt709[outa]");
        } else {
            filters.push_str(",format=gbrp[outv]");
        }
        encode(
            request,
            staging,
            &filters,
            alpha_output,
            Some(SequenceTiming {
                time_base: self.samples[0].time_base,
                loops: self.color().loops.unwrap_or(1),
                single_duration,
            }),
            cancelled,
            progress,
        )?;
        if self.color().loops.is_none() {
            // Preserve an undeclared repetition policy without changing box offsets.
            let mut file = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&staging.output)
                .map_err(ExportError::Output)?;
            let length = file.metadata().map_err(ExportError::Output)?.len();
            let root = boxes(&mut file, 0, length, cancelled)?;
            let moov = one(&root, b"moov")?.ok_or_else(|| invalid("saved sequence has no moov"))?;
            for item in boxes(&mut file, moov.start, moov.end, cancelled)?
                .into_iter()
                .filter(|item| &item.kind == b"trak")
            {
                if let Some(edit) =
                    one(&boxes(&mut file, item.start, item.end, cancelled)?, b"edts")?
                {
                    file.seek(SeekFrom::Start(edit.header + 4))
                        .map_err(ExportError::Output)?;
                    file.write_all(b"free").map_err(ExportError::Output)?;
                }
            }
        }
        let output = Self::read(&staging.output, cancelled)?
            .ok_or_else(|| invalid("encoder flattened animation"))?;
        if output.color().loops != self.color().loops
            || output.samples.len() != if alpha_output { 2 } else { 1 }
            || output.samples[0].size != output_size
        {
            return Err(invalid("saved loop or alpha controls differ"));
        }
        for (index, actual) in output.samples.iter().enumerate() {
            let expected = self.samples.get(index).unwrap_or(&self.samples[0]);
            if !same_timing(expected, actual) {
                return Err(invalid("saved frame count or timing differs"));
            }
        }
        check_cancelled(cancelled)
    }
}

fn encode(
    request: &ExportRequest,
    staging: &StagedExport,
    filters: &str,
    alpha_output: bool,
    timing: Option<SequenceTiming>,
    cancelled: &AtomicBool,
    progress: &(impl Fn(Duration) + Sync),
) -> Result<(), ExportError> {
    let graph = staging.directory.join("timeline-filter.txt");
    fs::write(&graph, filters).map_err(ExportError::Output)?;
    let mut args: Vec<String> = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-nostats",
        "-progress",
        "pipe:1",
        "-xerror",
        "-err_detect",
        "explode",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    if avif_path(&request.source) {
        args.extend(["-max_stts_delta".into(), u32::MAX.to_string()]);
    }
    if timing.is_some() {
        // Container transforms are explicit in the merged-RGBA graph above;
        // retain normal codec cropping without applying the container twice.
        args.extend([
            "-apply_cropping".into(),
            "codec".into(),
            "-noautorotate".into(),
            "-display_rotation".into(),
            "0".into(),
            "-nodisplay_hflip".into(),
            "-nodisplay_vflip".into(),
        ]);
    }
    args.extend([
        "-i".into(),
        request.source.display().to_string(),
        "-/filter_complex".into(),
        graph.display().to_string(),
        "-map".into(),
        "[outv]".into(),
    ]);
    if alpha_output {
        args.extend(["-map", "[outa]"].map(String::from));
        // Alpha extraction retains RGB frame properties, but monochrome AV1
        // cannot signal the identity RGB matrix used by the color track.
        args.extend(["-colorspace:v:1", "bt709"].map(String::from));
    }
    args.extend(
        [
            "-map_metadata",
            "-1",
            "-c:v",
            "libaom-av1",
            "-crf",
            "0",
            "-cpu-used",
            "6",
            "-threads",
            "1",
            "-fps_mode",
            "passthrough",
        ]
        .map(String::from),
    );
    if let Some(timing) = timing {
        args.extend([
            "-enc_time_base".into(),
            format!(
                "{}/{}",
                timing.time_base.numerator(),
                timing.time_base.denominator()
            ),
        ]);
        if timing.single_duration.is_some() {
            // FFmpeg's AVIF muxer omits moov for one sample. Its MP4 muxer keeps
            // the sample tables; specialize that owned output to an AVIF sequence.
            // A placeholder packet time avoids MP4's rescaled-duration limit;
            // finish restores the exact source ticks, without duplicating frames.
            args.extend(
                [
                    "-f",
                    "mp4",
                    "-use_editlist",
                    "0",
                    "-frames:v",
                    "1",
                    "-bsf:v",
                    "setts=pts=0:dts=0:duration=1",
                ]
                .map(String::from),
            );
        } else {
            args.extend(["-loop".into(), timing.loops.to_string()]);
        }
    } else {
        args.extend(["-frames:v", "1"].map(String::from));
    }
    args.push(staging.output.display().to_string());
    let executable = crate::media_tools::tool_path("ffmpeg.exe").map_err(ExportError::Start)?;
    let result = run_ffmpeg(&executable, args, cancelled, progress)?;
    if !result.status.success() {
        return Err(invalid(String::from_utf8_lossy(&result.stderr)));
    }
    if let Some(timing) = timing
        && timing.single_duration.is_some()
    {
        single::finish(&staging.output, timing, alpha_output, cancelled)?;
    }
    check_cancelled(cancelled)
}

pub(super) fn export_still(
    request: &ExportRequest,
    staging: &StagedExport,
    cancelled: &AtomicBool,
    progress: &(impl Fn(Duration) + Sync),
) -> Result<(), ExportError> {
    use image::ImageEncoder;
    let current = || !cancelled.load(Ordering::Relaxed);
    check_cancelled(cancelled)?;
    let decoded = crate::image::decode_image_cancellable(
        &request.source,
        crate::image::IMAGE_BYTE_LIMIT,
        &current,
    );
    check_cancelled(cancelled)?;
    let decoded = decoded.map_err(invalid)?;
    if decoded.frames.len() != 1 {
        return Err(invalid("static export must not discard animation frames"));
    }
    let frame = crate::image_edits::render_frame_cancellable(
        &decoded.frames[0],
        &request.operations,
        &|| !current(),
    );
    check_cancelled(cancelled)?;
    let frame = frame.map_err(invalid)?;
    drop(decoded);
    // Reuse the same decoded and edited RGBA pixels as display, including associated alpha.
    // This owned PNG is also the input for alpha-capable static format conversion.
    let source = staging.directory.join("animation-source.png");
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&source)
        .map_err(ExportError::Output)?;
    image::codecs::png::PngEncoder::new(file)
        .write_image(
            &frame.rgba,
            frame.width,
            frame.height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(invalid)?;
    check_cancelled(cancelled)?;
    let staged = ExportRequest {
        source,
        operations: vec![],
        target: staging.output.clone(),
        ..request.clone()
    };
    if avif_path(&request.target) {
        let alpha = frame
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] != 255);
        let filters = if alpha {
            "[0:v]format=rgba,split[color][alpha];[color]format=gbrp[outv];[alpha]alphaextract,format=gray,setparams=colorspace=bt709[outa]"
        } else {
            "[0:v]format=gbrp[outv]"
        };
        encode(&staged, staging, filters, alpha, None, cancelled, progress)?;
        let output = crate::image::decode_image_cancellable(
            &staging.output,
            crate::image::IMAGE_BYTE_LIMIT,
            &current,
        );
        check_cancelled(cancelled)?;
        let output = output.map_err(invalid)?;
        if output.frames.len() != 1
            || output.dimensions() != (frame.width, frame.height)
            || output.frames[0].rgba != frame.rgba
        {
            return Err(invalid("saved static RGBA pixels differ"));
        }
    } else {
        let executable = crate::media_tools::tool_path("ffmpeg.exe").map_err(ExportError::Start)?;
        let output = run_ffmpeg(
            &executable,
            staging.arguments(&staged, false, &ExportStreams::default())?,
            cancelled,
            progress,
        )?;
        if !output.status.success() {
            return Err(invalid(String::from_utf8_lossy(&output.stderr)));
        }
    }
    check_cancelled(cancelled)
}

fn same_timing(left: &Samples, right: &Samples) -> bool {
    let same = |a: i128, b: i128| {
        a * i128::from(left.time_base.numerator()) * i128::from(right.time_base.denominator())
            == b * i128::from(right.time_base.numerator())
                * i128::from(left.time_base.denominator())
    };
    left.times.len() == right.times.len()
        && left
            .times
            .iter()
            .zip(&right.times)
            .all(|(left_time, right_time)| {
                same(
                    i128::from(left_time.0) - i128::from(left.times[0].0),
                    i128::from(right_time.0) - i128::from(right.times[0].0),
                ) && same(i128::from(left_time.1), i128::from(right_time.1))
            })
}

#[cfg(test)]
#[path = "export_avif_tests.rs"]
mod tests;
