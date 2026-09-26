use super::*;
use towavue_core::{AudioTrackId, AudioTrackRetention};

#[cfg(test)]
#[path = "export_audio_tracks_tests.rs"]
mod tests;

// ffmpeg first reconstructs sample timestamps with av_rescale_delta. At coarse
// source ticks it may still correct a few samples after a real gap: do not drop
// encoded PCM to chase quantization. Exact sample-tick inputs use zero tolerance.
fn alignment(track: &Track) -> String {
    let tolerance = track.tolerance;
    // Preserve the decoder's sample format: otherwise aresample negotiates a
    // downstream format across atempo, changing its established arithmetic.
    format!(
        "aresample=osf={}:async=1:min_comp={tolerance:.12}:min_hard_comp={tolerance:.12}:first_pts=0",
        track.sample_format
    )
}

#[derive(Clone)]
pub(super) struct Track {
    pub stream: (usize, ffmpeg::Rational),
    pub channels: u16,
    tolerance: f64,
    sample_format: String,
    disposition: i32,
    filters: Vec<String>,
    output_samples: Option<u64>,
}

pub(super) fn probe(source: &Path) -> Result<Vec<Track>, ExportError> {
    probe_selected(source, &AudioTrackRetention::All)
}

fn probe_selected(
    source: &Path,
    selection: &AudioTrackRetention,
) -> Result<Vec<Track>, ExportError> {
    let read = || -> Result<Vec<Track>, ExportError> {
        let failed = |error: ffmpeg::Error| ExportError::Failed(error.to_string());
        ffmpeg::init().map_err(failed)?;
        let input = ffmpeg::format::input(source).map_err(failed)?;
        if let AudioTrackRetention::Selected(ids) = selection
            && ids.iter().any(|id| {
                !input.streams().any(|stream| {
                    stream.index() == id.index()
                        && stream.parameters().medium() == ffmpeg::media::Type::Audio
                })
            })
        {
            return Err(ExportError::Message(Text::ExportAudioTrackUnavailable));
        }
        input
            .streams()
            .filter(|stream| stream.parameters().medium() == ffmpeg::media::Type::Audio)
            .filter(|stream| match selection {
                AudioTrackRetention::All => true,
                AudioTrackRetention::Selected(ids) => {
                    ids.contains(&AudioTrackId::from_index(stream.index()))
                }
            })
            .map(|stream| {
                let decoder = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
                    .map_err(failed)?
                    .decoder()
                    .audio()
                    .map_err(failed)?;
                Ok(Track {
                    stream: (stream.index(), ffmpeg::Rational(1, decoder.rate() as i32)),
                    channels: decoder.channels(),
                    sample_format: decoder.format().name().to_owned(),
                    tolerance: (if crate::audio_timestamps::quantized_aac(&stream) {
                        0.001
                    } else {
                        f64::from(stream.time_base())
                    } - 1.0 / f64::from(decoder.rate()))
                    .max(0.0),
                    disposition: stream.disposition().bits(),
                    filters: Vec::new(),
                    output_samples: None,
                })
            })
            .collect()
    };
    read()
}

pub(super) fn retained(
    request: &ExportRequest,
    selection: &AudioTrackRetention,
) -> Result<Option<Vec<Track>>, ExportError> {
    if request.kind != MediaKind::Video {
        return Ok(None);
    }
    let tracks = probe_selected(&request.source, selection)?;
    if *selection == AudioTrackRetention::All && tracks.len() <= 1 {
        // Keep ordinary single-track exports on their established path.
        return Ok(None);
    }
    Ok(Some(tracks))
}

fn project(streams: &ExportStreams, track: &Track) -> ExportStreams {
    let mut projected = streams.clone();
    projected.video = None;
    projected.audio_set = None;
    projected.audio = Some(track.stream);
    projected.audio_channels = Some(track.channels);
    projected.audio_output_samples = track.output_samples;
    projected.audio_post_filters = track.filters.clone();
    projected.audio_alignment = Some(alignment(track));
    projected
}

#[derive(Default)]
pub(super) struct Processing {
    base_filters: Vec<Vec<String>>,
    loudness: Vec<Option<loudness::Plan>>,
}

impl Processing {
    pub(super) fn prepare(
        request: &ExportRequest,
        streams: &mut ExportStreams,
        options: AudioExportOptions,
        staging: &StagedExport,
        executable: &Path,
        cancelled: &AtomicBool,
        progress: &(impl Fn(Duration) + Sync),
    ) -> Result<Self, ExportError> {
        let Some(mut tracks) = streams.audio_set.take() else {
            return Ok(Self::default());
        };
        let mut processing = Self::default();
        let rate = EditState::from_operations(&request.operations).rate;
        for (ordinal, track) in tracks.iter_mut().enumerate() {
            check_cancelled(cancelled)?;
            track.filters = options.filters(Some(track.channels))?;
            if streams.timeline.is_none() && rate != 1.0 {
                if !(0.25..=4.0).contains(&rate) {
                    return Err(ExportError::Message(Text::ExportInvalidAudioRate));
                }
                let samples = audio_options::count_samples(
                    request,
                    &project(streams, track),
                    staging,
                    executable,
                    cancelled,
                    progress,
                )?;
                const SCALE: u128 = 1 << 25;
                let units = (f64::from(rate) * SCALE as f64) as u128;
                track.output_samples = Some(
                    u64::try_from((u128::from(samples) * SCALE).div_ceil(units))
                        .map_err(|_| ExportError::Message(Text::ExportAudioSampleCountTooLarge))?,
                );
            }
            if options.normalization == AudioNormalization::Peak {
                let gain = audio_options::analyze(
                    request,
                    &project(streams, track),
                    staging,
                    executable,
                    cancelled,
                    progress,
                )?;
                track
                    .filters
                    .push(format!("volume={gain:.17e}:precision=double"));
            }
            let loudness = if let AudioNormalization::Loudness(target) = options.normalization {
                match loudness::Plan::analyze(
                    target,
                    request,
                    &project(streams, track),
                    staging,
                    executable,
                    cancelled,
                    progress,
                ) {
                    Ok(mut plan) => {
                        plan.output_track = Some(ordinal);
                        Some(plan)
                    }
                    // Silent companions stay silent while other tracks normalize.
                    Err(ExportError::Message(Text::ExportSilentLoudness)) => None,
                    Err(error) => return Err(error),
                }
            } else {
                None
            };
            processing.base_filters.push(track.filters.clone());
            processing.loudness.push(loudness);
        }
        if matches!(options.normalization, AudioNormalization::Loudness(_))
            && !tracks.is_empty()
            && processing.loudness.iter().all(Option::is_none)
        {
            return Err(ExportError::Message(Text::ExportSilentLoudness));
        }
        streams.audio_set = Some(tracks);
        Ok(processing)
    }

    pub(super) fn apply(&self, streams: &mut ExportStreams) -> Result<(), ExportError> {
        if let Some(tracks) = &mut streams.audio_set {
            for ((track, base), plan) in tracks
                .iter_mut()
                .zip(&self.base_filters)
                .zip(&self.loudness)
            {
                track.filters = base.clone();
                if let Some(plan) = plan {
                    track.filters.extend(plan.filters()?);
                }
            }
        }
        Ok(())
    }

    pub(super) fn verify(
        &mut self,
        staging: &StagedExport,
        executable: &Path,
        cancelled: &AtomicBool,
        progress: &(impl Fn(Duration) + Sync),
        attempt: usize,
    ) -> Result<bool, ExportError> {
        let mut accepted = true;
        for plan in self.loudness.iter_mut().flatten() {
            accepted &= plan.verify_candidate(staging, executable, cancelled, progress, attempt)?;
        }
        Ok(accepted)
    }
}

pub(super) fn arguments(
    arguments: &mut Vec<String>,
    request: &ExportRequest,
    hardware: bool,
    streams: &ExportStreams,
    video_input: usize,
) {
    let tracks = streams.audio_set.as_ref().expect("retained tracks");
    let state = EditState::from_operations(&request.operations);
    let mut filters = Vec::new();
    arguments.extend(["-copyts".into(), "-start_at_zero".into()]);
    if let Some((index, base)) = streams.video {
        let video = if let Some(timeline) = &streams.timeline {
            let mut only_video = streams.clone();
            only_video.audio = None;
            timeline_filters(request, &only_video, timeline, hardware, video_input)
        } else {
            let mut chain = video_filters(
                streams.visual_filters(&request.operations),
                &state,
                Some(base),
            );
            if streams
                .codec_arguments(request, hardware)
                .iter()
                .any(|codec| matches!(codec.as_str(), "libopenh264" | "libsvtav1" | "libaom-av1"))
            {
                chain.push("copy".into());
            }
            if chain.is_empty() {
                chain.push("null".into());
            }
            format!("[{video_input}:{index}]{}[outv]", chain.join(","))
        };
        filters.push(video);
        arguments.extend(["-map".into(), "[outv]".into()]);
    }
    for (ordinal, track) in tracks.iter().enumerate() {
        let audio = if let Some(timeline) = &streams.timeline {
            timeline_filters(request, &project(streams, track), timeline, hardware, 0)
                .replace("[as", &format!("[t{ordinal}s"))
                .replace("[a", &format!("[t{ordinal}a"))
                .replace("[joineda]", &format!("[t{ordinal}joined]"))
                .replace("[outa]", &format!("[outa{ordinal}]"))
        } else {
            let mut chain = vec![alignment(track)];
            chain.extend(audio_filters(
                &state,
                Some(track.stream.1),
                track.output_samples,
            ));
            chain.extend(track.filters.iter().cloned());
            format!("[0:{}]{}[outa{ordinal}]", track.stream.0, chain.join(","))
        };
        filters.push(audio);
        arguments.extend([
            "-map".into(),
            format!("[outa{ordinal}]"),
            format!("-map_metadata:s:a:{ordinal}"),
            format!("0:s:{}", track.stream.0),
            format!("-disposition:a:{ordinal}"),
            track.disposition.to_string(),
        ]);
    }
    arguments.extend(["-filter_complex".into(), filters.join(";")]);
    arguments.extend(streams.codec_arguments(request, hardware));
    arguments.push(request.target.display().to_string());
}
