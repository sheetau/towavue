use std::io;
#[cfg(any(test, feature = "waveform-verification"))]
use std::io::Read;

const BINS_PER_COLUMN: usize = 1024;

#[cfg(test)]
mod contention_tests;
pub(crate) mod native;
#[cfg(test)]
mod reference_tests;

/// Peak absolute stereo envelope of the edited playback samples, one value per
/// display column. Preserve float amplitudes so height/DPI changes need no raster
/// enlargement. The louder channel supplies each frame; antiphase cannot cancel.
pub fn timeline_waveform(
    source: &std::path::Path,
    plan: &towavue_core::EditTimeline,
    rate: f32,
    volume: f32,
    columns: u32,
    cancellation: &crate::Cancellation,
) -> Result<Vec<f32>, crate::PreviewError> {
    timeline_audio_track_waveform(source, None, plan, rate, volume, columns, cancellation)
}

/// Exact edited envelope for a source-scoped audio track. Preview track changes
/// must also invalidate the caller's waveform request/cache identity.
pub fn timeline_audio_track_waveform(
    source: &std::path::Path,
    track: Option<towavue_core::AudioTrackId>,
    plan: &towavue_core::EditTimeline,
    rate: f32,
    volume: f32,
    columns: u32,
    cancellation: &crate::Cancellation,
) -> Result<Vec<f32>, crate::PreviewError> {
    use crate::{PreviewError, decode, playback::timeline as playback_timeline};
    if cancellation.is_cancelled() {
        return Err(PreviewError::Cancelled);
    }
    if !(1..=8192).contains(&columns)
        || !rate.is_finite()
        || !(0.25..=4.0).contains(&rate)
        || !volume.is_finite()
        || !(0.0..=towavue_core::MAX_VOLUME).contains(&volume)
    {
        return Err(PreviewError::Message(
            towavue_core::localization::Text::WaveformInvalidInput,
        ));
    }
    if plan.spans().is_empty() {
        return Ok(vec![0.0; columns as usize]);
    }
    let format = decode::probe_audio_track_format(source, track)?.ok_or(PreviewError::Message(
        towavue_core::localization::Text::WaveformNoAudio,
    ))?;
    let frames = crate::tempo::output_sample_boundary(
        plan.duration().as_nanoseconds(),
        format.sample_rate,
        rate,
    );
    let mut envelope = DisplayEnvelope::new(columns, frames);
    let mut invalid = false;
    let receive = |chunk: decode::AudioChunk| {
        if !envelope.push_stereo(&chunk.bytes, f64::from(volume)) {
            invalid = true;
            return false;
        }
        !cancellation.is_cancelled()
    };
    let result = if track.is_none() {
        playback_timeline::decode_audio(
            source,
            plan,
            towavue_core::MediaTime::ZERO,
            None,
            rate,
            format,
            cancellation.flag(),
            receive,
        )
    } else {
        playback_timeline::decode_audio_with_policy(
            source,
            track,
            plan,
            towavue_core::MediaTime::ZERO,
            None,
            rate,
            format,
            decode::AudioSeekPolicy::Exact,
            cancellation.flag(),
            receive,
        )
    };
    if cancellation.is_cancelled() {
        return Err(PreviewError::Cancelled);
    }
    if invalid {
        return Err(PreviewError::Message(
            towavue_core::localization::Text::WaveformNonFinite,
        ));
    }
    result?;
    envelope.finish()
}

struct DisplayEnvelope {
    peaks: Vec<f64>,
    frames: u64,
    position: u64,
    column: usize,
    column_end: f64,
}

impl DisplayEnvelope {
    fn push_stereo(&mut self, pcm: &[u8], volume: f64) -> bool {
        let amplitude = |frame: &[u8; 8]| {
            let left = f32::from_le_bytes(frame[..4].try_into().expect("left sample"));
            let right = f32::from_le_bytes(frame[4..].try_into().expect("right sample"));
            (left.is_finite() && right.is_finite())
                .then(|| f64::from(left.abs().max(right.abs())) * volume)
        };
        let mut frames = pcm.as_chunks::<8>().0;
        while !frames.is_empty() {
            // Whole samples inside one column need only a peak reduction.
            // Fractional boundaries and excess samples retain the scalar path.
            let count = if self.column < self.peaks.len() {
                (self.column_end as u64)
                    .saturating_sub(self.position)
                    .min(frames.len() as u64) as usize
            } else {
                0
            };
            if count == 0 {
                let Some(value) = amplitude(&frames[0]) else {
                    return false;
                };
                self.push(value);
                frames = &frames[1..];
            } else {
                let mut peak = self.peaks[self.column];
                for frame in &frames[..count] {
                    let Some(value) = amplitude(frame) else {
                        return false;
                    };
                    peak = peak.max(value);
                }
                self.peaks[self.column] = peak;
                self.position += count as u64;
                frames = &frames[count..];
            }
        }
        true
    }

    fn new(columns: u32, frames: u64) -> Self {
        Self {
            peaks: vec![0.0; columns as usize],
            frames,
            position: 0,
            column: 0,
            column_end: frames as f64 / f64::from(columns),
        }
    }

    fn push(&mut self, amplitude: f64) {
        // Reduce peaks in sample coordinates; calculate boundaries only when crossing
        // a display column, including multiple columns within one short-media sample.
        let mut start = self.position as f64;
        let end = (self.position + 1) as f64;
        while self.column < self.peaks.len() {
            if end <= self.column_end {
                self.peaks[self.column] = self.peaks[self.column].max(amplitude);
                break;
            }
            if self.column_end > start {
                self.peaks[self.column] = self.peaks[self.column].max(amplitude);
            }
            start = self.column_end;
            self.column += 1;
            self.column_end =
                (self.column + 1) as f64 * self.frames as f64 / self.peaks.len() as f64;
        }
        self.position += 1;
    }

    fn finish(self) -> Result<Vec<f32>, crate::PreviewError> {
        if self.frames == 0 || self.position != self.frames {
            return Err(crate::PreviewError::Message(
                towavue_core::localization::Text::WaveformSampleCountMismatch,
            ));
        }
        Ok(self.peaks.into_iter().map(|peak| peak as f32).collect())
    }
}

struct Envelope {
    // Completed bins have equal sample counts; only the pending tail may be shorter.
    peaks: Vec<u64>,
    bin_samples: u64,
    pending_peak: u64,
    pending_samples: u64,
    total_samples: u64,
    limit: usize,
}

impl Envelope {
    fn new(width: u32) -> Self {
        let limit = width as usize * BINS_PER_COLUMN;
        Self {
            peaks: Vec::with_capacity(limit),
            bin_samples: 1,
            pending_peak: 0,
            pending_samples: 0,
            total_samples: 0,
            limit,
        }
    }

    #[cfg(any(test, feature = "waveform-verification"))]
    fn push(&mut self, mut pcm: &[u8]) {
        self.push_channels(&mut pcm, 1);
    }

    fn push_stereo(&mut self, mut pcm: &[u8]) {
        self.push_channels(&mut pcm, 2);
    }

    fn push_channels(&mut self, pcm: &mut &[u8], channels: usize) {
        while !pcm.is_empty() {
            let count =
                (self.bin_samples - self.pending_samples).min((pcm.len() / (2 * channels)) as u64);
            let bytes = count as usize * 2 * channels;
            self.pending_peak = self.pending_peak.max(
                pcm[..bytes]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|sample| {
                        u64::from(i16::from_le_bytes([sample[0], sample[1]]).unsigned_abs())
                    })
                    .max()
                    .unwrap_or(0),
            );
            self.pending_samples += count;
            self.total_samples += count;
            *pcm = &pcm[bytes..];
            if self.pending_samples == self.bin_samples {
                self.peaks.push(self.pending_peak);
                self.pending_peak = 0;
                self.pending_samples = 0;
                if self.peaks.len() == self.limit {
                    for index in 0..self.limit / 2 {
                        self.peaks[index] = self.peaks[index * 2].max(self.peaks[index * 2 + 1]);
                    }
                    self.peaks.truncate(self.limit / 2);
                    self.bin_samples *= 2;
                }
            }
        }
    }

    fn column_peaks(&self, width: u32) -> io::Result<Vec<u16>> {
        let per_column = self.total_samples / u64::from(width);
        if per_column == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                crate::RecoveryDetail::from(
                    towavue_core::localization::Text::WaveformTooFewSamples,
                ),
            ));
        }
        let mut peaks = Vec::with_capacity(width as usize);
        for column in 0..u64::from(width) {
            let start = column * per_column;
            let end = if column + 1 == u64::from(width) {
                self.total_samples
            } else {
                start + per_column
            };
            let mut position = start;
            let mut peak = 0;
            while position < end {
                let index = (position / self.bin_samples) as usize;
                let (bin_peak, count) = self
                    .peaks
                    .get(index)
                    .map(|peak| (*peak, self.bin_samples))
                    .unwrap_or((self.pending_peak, self.pending_samples));
                let overlap = end.min(index as u64 * self.bin_samples + count) - position;
                peak = peak.max(bin_peak);
                position += overlap;
            }
            peaks.push(peak as u16);
        }
        Ok(peaks)
    }
}

#[cfg(test)]
pub(crate) fn read(reader: &mut dyn Read, width: u32, height: u32) -> io::Result<image::RgbaImage> {
    read_channels(reader, width, height, 1)
}

#[cfg(any(test, feature = "waveform-verification"))]
pub(crate) fn read_stereo(
    reader: &mut dyn Read,
    width: u32,
    height: u32,
) -> io::Result<image::RgbaImage> {
    read_channels(reader, width, height, 2)
}

#[cfg(any(test, feature = "waveform-verification"))]
fn read_channels(
    reader: &mut dyn Read,
    width: u32,
    height: u32,
    channels: usize,
) -> io::Result<image::RgbaImage> {
    let mut envelope = Envelope::new(width);
    let mut buffer = [0; 65_536];
    let mut carried = 0;
    loop {
        let count = reader.read(&mut buffer[carried..])?;
        if count == 0 {
            if carried != 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            break;
        }
        let total = carried + count;
        let stride = channels * 2;
        let consumed = total / stride * stride;
        if channels == 2 {
            envelope.push_stereo(&buffer[..consumed]);
        } else {
            envelope.push(&buffer[..consumed]);
        }
        carried = total % stride;
        if carried != 0 {
            buffer.copy_within(consumed..total, 0);
        }
    }
    rasterize(&envelope, width, height)
}

fn rasterize(envelope: &Envelope, width: u32, height: u32) -> io::Result<image::RgbaImage> {
    let mut image = image::RgbaImage::new(width, height);
    for (x, peak) in envelope.column_peaks(width)?.into_iter().enumerate() {
        let bar =
            ((u64::from(peak) * u64::from(height) + 16_383) / 32_767).min(u64::from(height)) as u32;
        for y in (height - bar) / 2..(height - bar) / 2 + bar {
            image.put_pixel(x as u32, y, image::Rgba([255; 4]));
        }
    }
    Ok(image)
}

#[cfg(test)]
mod chunk_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn reference_columns(samples: &[f64], width: u32) -> Vec<f32> {
        (0..width)
            .map(|column| {
                let start = f64::from(column) * samples.len() as f64 / f64::from(width);
                let end = f64::from(column + 1) * samples.len() as f64 / f64::from(width);
                samples[start.floor() as usize..(end.ceil() as usize).min(samples.len())]
                    .iter()
                    .copied()
                    .fold(0.0_f64, f64::max) as f32
            })
            .collect()
    }

    #[test]
    fn stereo_peaks_keep_isolated_transients_and_values_above_full_scale() {
        let pcm = [(0.0_f32, 0.0_f32), (1.5, -1.5), (0.0, 0.0), (0.0, -0.25)]
            .into_iter()
            .flat_map(|(left, right)| [left, right])
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        let mut detail = DisplayEnvelope::new(2, 4);
        assert!(detail.push_stereo(&pcm, 1.0));
        assert_eq!(detail.finish().expect("peak detail"), [1.5, 0.25]);
        let pcm = [(0_i16, 0_i16), (i16::MAX, -i16::MAX), (0, 0), (0, -8192)]
            .into_iter()
            .flat_map(|(left, right)| [left, right])
            .flat_map(i16::to_le_bytes)
            .collect::<Vec<_>>();
        let mut source = Envelope::new(2);
        source.push_stereo(&pcm);
        assert_eq!(source.column_peaks(2).expect("peak source"), [32767, 8192]);
        let pixels = rasterize(&source, 2, 80).expect("peak raster");
        assert!(
            (0..80).all(|y| pixels.get_pixel(0, y)[3] == 255),
            "a full-scale transient reaches both edges"
        );
    }
    #[test]
    fn display_envelope_preserves_peaks_in_short_and_fractional_columns() {
        for frames in [1, 2, 7, 500, 48_001, 1_000_003] {
            let samples = (0..frames)
                .map(|frame| (frame % 31 + 1) as f64 / 137.0)
                .collect::<Vec<_>>();
            for width in [1, 3, 640, 8192] {
                let mut envelope = DisplayEnvelope::new(width, frames as u64);
                for sample in &samples {
                    envelope.push(*sample);
                }
                let actual = envelope.finish().expect("complete envelope");
                for (actual, expected) in actual.iter().zip(reference_columns(&samples, width)) {
                    assert!((actual - expected).abs() < 0.000001);
                }
            }
        }
        assert!(DisplayEnvelope::new(640, 10).finish().is_err());
        let mut extra = DisplayEnvelope::new(3, 2);
        for _ in 0..3 {
            extra.push(1.0);
        }
        assert!(extra.finish().is_err());
    }

    #[test]
    #[ignore = "Release-only comparison of owned amplitudes; excludes decoding and UI"]
    fn display_envelope_reports_column_boundary_cost() -> Result<(), &'static str> {
        if cfg!(debug_assertions) {
            return Err("run with --release");
        }
        let samples: Vec<_> = (0..48_000 * 180)
            .map(|i| f64::from((i % 997) as u32) / 997.0 * 3.0)
            .collect();
        for width in [307, 8192] {
            let reference = reference_columns(&samples, width);
            for legacy in [true, false, false, true] {
                let mut timings = Vec::new();
                for _ in 0..5 {
                    let samples = std::hint::black_box(&samples);
                    let start = std::time::Instant::now();
                    let actual: Vec<f32> = if legacy {
                        // Scalar display-coordinate peak reference.
                        let mut peaks = vec![0.0_f64; width as usize];
                        for (position, amplitude) in samples.iter().enumerate() {
                            let start = position as f64 * f64::from(width) / samples.len() as f64;
                            let end =
                                (position + 1) as f64 * f64::from(width) / samples.len() as f64;
                            let mut column = start.floor() as usize;
                            while column < peaks.len() && (column as f64) < end {
                                if end.min((column + 1) as f64) > start.max(column as f64) {
                                    peaks[column] = peaks[column].max(*amplitude);
                                }
                                column += 1;
                            }
                        }
                        peaks.into_iter().map(|peak| peak as f32).collect()
                    } else {
                        let mut envelope = DisplayEnvelope::new(width, samples.len() as u64);
                        for sample in samples {
                            envelope.push(*sample);
                        }
                        envelope.finish().expect("complete")
                    };
                    std::hint::black_box(&actual);
                    timings.push(start.elapsed().as_secs_f64() * 1000.0);
                    assert!(
                        actual
                            .iter()
                            .zip(&reference)
                            .all(|(a, b)| (a - b).abs() < 0.000001)
                    );
                }
                timings.sort_by(f64::total_cmp);
                eprintln!(
                    "WAVEFORM_COLUMNS width={width} frames={} legacy={legacy} median_ms={:.3}",
                    samples.len(),
                    timings[2]
                );
            }
        }
        Ok(())
    }

    #[test]
    fn timeline_waveform_matches_playback_and_bounds_export_column_error() {
        use towavue_core::{
            EditOperation, EditTimeline, MediaKind, MediaTime, TimeRange, TimelineEdit,
        };
        let time = |ms: i64| MediaTime::from_nanoseconds(ms * 1_000_000);
        let range = |a, b| TimeRange::new(time(a), time(b)).expect("range");
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("towavue-detailed-waveform-{unique}"));
        std::fs::create_dir(&root).expect("owned fixtures");
        let source = root.join("source.wav");
        let mut pcm = Vec::new();
        for index in 0..96_000 {
            let sample = ((index as f64 * 440.0 * std::f64::consts::TAU / 48_000.0).sin()
                * if index % 7300 < 2900 {
                    14_000.0
                } else {
                    2800.0
                }) as i16;
            pcm.extend(sample.to_le_bytes());
            pcm.extend((-sample).to_le_bytes());
        }
        let mut wav = b"RIFF".to_vec();
        wav.extend((36 + pcm.len() as u32).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16_u32.to_le_bytes());
        wav.extend(1_u16.to_le_bytes());
        wav.extend(2_u16.to_le_bytes());
        wav.extend(48_000_u32.to_le_bytes());
        wav.extend(192_000_u32.to_le_bytes());
        wav.extend(4_u16.to_le_bytes());
        wav.extend(16_u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend((pcm.len() as u32).to_le_bytes());
        wav.extend(pcm);
        std::fs::write(&source, &wav).expect("owned PCM fixture");
        let mut repeated = EditTimeline::new(time(2000), Default::default()).expect("plan");
        let base = timeline_waveform(
            &source,
            &repeated,
            1.0,
            1.0,
            64,
            &crate::Cancellation::default(),
        )
        .expect("base envelope");
        for factor in [2.0, 4.0, 8.0] {
            assert!(repeated.apply(TimelineEdit::ScaleVolume(range(0, 2000), 2.0)));
            let amplified = timeline_waveform(
                &source,
                &repeated,
                1.0,
                1.0,
                64,
                &crate::Cancellation::default(),
            )
            .expect("repeated gain envelope");
            for (before, after) in base.iter().zip(amplified) {
                assert!(
                    (after - before * factor).abs() < 1e-5,
                    "playback decode and envelope retain cumulative gain {factor}"
                );
            }
        }

        for (stretch, rate) in [(false, 1.0), (true, 1.0), (true, 1.1)] {
            let mut operations = vec![EditOperation::Timeline(TimelineEdit::Delete(range(
                500, 750,
            )))];
            if stretch {
                operations.push(EditOperation::Timeline(TimelineEdit::Stretch(
                    range(250, 1000),
                    time(1500),
                )));
            }
            operations.push(EditOperation::Timeline(TimelineEdit::SetVolume(
                range(600, 1100),
                0.3,
            )));
            operations.push(EditOperation::Timeline(TimelineEdit::Keep(range(
                100, 1500,
            ))));
            operations.push(EditOperation::SetVolume(1.5));
            operations.push(EditOperation::SetRate(rate));
            let plan = EditTimeline::from_operations(time(2000), &operations).expect("plan");
            let target = root.join(format!("export-{stretch}-{rate}.wav"));
            crate::export_media(&crate::ExportRequest {
                source: source.clone(),
                target: target.clone(),
                kind: MediaKind::Audio,
                operations,
                hardware_encode: false,
            })
            .expect("independent exported PCM");
            let mut amplitudes = Vec::new();
            crate::decode::decode_file(&target, |output| {
                if let crate::DecodeOutput::Audio(chunk) = output {
                    amplitudes.extend(chunk.bytes.as_chunks::<8>().0.iter().map(|frame| {
                        f64::from(
                            f32::from_le_bytes(frame[..4].try_into().expect("left"))
                                .abs()
                                .max(
                                    f32::from_le_bytes(frame[4..].try_into().expect("right")).abs(),
                                ),
                        )
                    }));
                }
                true
            })
            .expect("decode exported samples");
            assert_eq!(
                amplitudes.len() as u64,
                crate::tempo::output_sample_boundary(
                    plan.duration().as_nanoseconds(),
                    48_000,
                    rate
                ),
                "export and playback sample axes"
            );
            let actual = timeline_waveform(
                &source,
                &plan,
                rate,
                1.5,
                307,
                &crate::Cancellation::default(),
            )
            .expect("edited waveform");
            let mut playback = Vec::new();
            crate::playback::timeline::decode_audio(
                &source,
                &plan,
                MediaTime::ZERO,
                None,
                rate,
                crate::AudioFormat {
                    sample_rate: 48_000,
                    channels: 2,
                },
                &std::sync::atomic::AtomicBool::new(false),
                |chunk| {
                    playback.extend(chunk.bytes.as_chunks::<8>().0.iter().map(|frame| {
                        f64::from(
                            f32::from_le_bytes(frame[..4].try_into().expect("left"))
                                .abs()
                                .max(
                                    f32::from_le_bytes(frame[4..].try_into().expect("right")).abs(),
                                ),
                        ) * 1.5
                    }));
                    true
                },
            )
            .expect("independent playback sample capture");
            assert_eq!(playback.len(), amplitudes.len());
            for (value, reference) in actual.iter().zip(reference_columns(&playback, 307)) {
                assert!(
                    (value - reference).abs() < 0.000001,
                    "waveform integrates actual playback samples"
                );
            }
            let expected = reference_columns(&amplitudes, 307);
            let error = actual
                .iter()
                .zip(expected)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0_f32, f32::max);
            // The 1.1x native/CLI tempo paths have equal sample counts but are
            // not PCM-identical. Bound this fixture's display error to less than
            // one pixel at 4096px height; do not certify export phase/PCM parity.
            let tolerance = if rate == 1.0 { 0.0001 } else { 1.0 / 4096.0 };
            eprintln!("waveform stretch={stretch}, rate={rate}, export column error={error}");
            assert!(
                error < tolerance,
                "stretch={stretch}, rate={rate}, maximum column error={error}"
            );
            assert!(
                actual.iter().any(|value| *value > 0.1),
                "antiphase channels must remain visible"
            );
        }
        assert_eq!(std::fs::read(&source).expect("source retained"), wav);
        let plan = EditTimeline::new(time(2000), Default::default()).expect("plan");
        let cancellation = crate::Cancellation::default();
        cancellation.cancel();
        assert!(matches!(
            timeline_waveform(&source, &plan, 1.0, 1.0, 307, &cancellation),
            Err(crate::PreviewError::Cancelled)
        ));
        assert!(
            timeline_waveform(
                &source,
                &plan,
                1.0,
                1.0,
                8193,
                &crate::Cancellation::default()
            )
            .is_err()
        );
        std::fs::remove_dir_all(root).expect("remove owned fixtures");
    }

    #[test]
    fn envelope_stays_bounded_and_preserves_peak_amplitude() {
        let width = 8;
        for length in [257, 8192, 8193, 1_000_003] {
            for pattern in 0..4 {
                let samples = (0..length)
                    .map(|i| match pattern {
                        0 => 0_i16,
                        1 => i16::MIN,
                        2 => {
                            if i % 733 < 79 {
                                i16::MAX
                            } else {
                                0
                            }
                        }
                        _ => (i * 257) as i16,
                    })
                    .collect::<Vec<_>>();
                let mut envelope = Envelope::new(width);
                for chunk in samples.chunks(257) {
                    let pcm = chunk
                        .iter()
                        .flat_map(|sample| sample.to_le_bytes())
                        .collect::<Vec<_>>();
                    envelope.push(&pcm);
                    assert!(envelope.peaks.len() <= width as usize * BINS_PER_COLUMN);
                    assert_eq!(envelope.peaks.capacity(), width as usize * BINS_PER_COLUMN);
                }
                let peaks = envelope.column_peaks(width).expect("peaks");
                let per_column = length / width as usize;
                for (column, peak) in peaks.into_iter().enumerate() {
                    let start = column * per_column;
                    let end = if column + 1 == width as usize {
                        length
                    } else {
                        start + per_column
                    };
                    let exact = samples[start..end]
                        .iter()
                        .map(|sample| u64::from(sample.unsigned_abs()))
                        .max()
                        .expect("nonempty column") as u16;
                    if envelope.bin_samples == 1 {
                        assert_eq!(peak, exact);
                    }
                    assert!(
                        peak.abs_diff(exact) <= 129,
                        "length {length}, pattern {pattern}, column {column}: {peak} != {exact}"
                    );
                    let bar = |value: u16| (u64::from(value) * 160 + 16_383) / 32_767;
                    assert!(bar(peak).abs_diff(bar(exact)) <= 1);
                }
            }
        }
    }

    #[test]
    fn pcm_reads_handle_odd_fragments_and_reject_truncated_or_empty_audio() {
        struct Fragments<'a>(&'a [u8]);
        impl Read for Fragments<'_> {
            fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
                let count = 3.min(output.len()).min(self.0.len());
                output[..count].copy_from_slice(&self.0[..count]);
                self.0 = &self.0[count..];
                Ok(count)
            }
        }
        let pcm = (0..1000_i16)
            .flat_map(|sample| (sample * 30).to_le_bytes())
            .collect::<Vec<_>>();
        let expected = read(&mut pcm.as_slice(), 10, 96).expect("contiguous PCM");
        assert_eq!(
            read(&mut Fragments(&pcm), 10, 96).expect("fragmented PCM"),
            expected
        );
        assert_eq!(
            read(&mut &pcm[..pcm.len() - 1], 10, 96)
                .expect_err("partial sample")
                .kind(),
            io::ErrorKind::UnexpectedEof
        );
        assert_eq!(
            read(&mut &[][..], 10, 96).expect_err("empty audio").kind(),
            io::ErrorKind::InvalidData
        );
    }
}
