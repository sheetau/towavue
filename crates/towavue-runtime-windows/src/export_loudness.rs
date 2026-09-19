use super::*;

const STATISTICS: &str = "astats@towavue_loudness_samples=metadata=0:reset=0:measure_perchannel=none:measure_overall=Peak_level+Number_of_samples+Number_of_NaNs+Number_of_Infs";
const LOUDNESS_TOLERANCE: f64 = 0.1;
pub(super) const MAX_ATTEMPTS: usize = 4;

#[derive(Clone, Copy, Debug)]
struct Measurement {
    integrated: f64,
    true_peak: f64,
    range: f64,
    threshold: f64,
    offset: f64,
}

/// Owns only measured values, never decoded samples or native objects. Each retry
/// reads the same guarded original and edits, not the previously encoded candidate.
pub(super) struct Plan {
    target: LoudnessTarget,
    input: Measurement,
    correction: f64,
    peak_margin: f64,
    sample_rate: i32,
}

impl Plan {
    pub(super) fn analyze(
        target: LoudnessTarget,
        request: &ExportRequest,
        streams: &ExportStreams,
        staging: &StagedExport,
        executable: &Path,
        cancelled: &AtomicBool,
        progress: &(impl Fn(Duration) + Sync),
    ) -> Result<Self, ExportError> {
        target.validate()?;
        let sample_rate = sample_rate(streams)?;
        let input = measure(
            target, request, streams, staging, executable, cancelled, progress,
        )?;
        Ok(Self {
            target,
            input,
            correction: 0.0,
            peak_margin: 0.1,
            sample_rate,
        })
    }

    pub(super) fn filters(&self) -> Result<Vec<String>, ExportError> {
        let gain = self.target.integrated() - self.input.integrated + self.correction;
        let ceiling = self.target.true_peak() - self.peak_margin;
        if self.input.true_peak + gain <= ceiling {
            return Ok(vec![format!("volume={gain:.9}dB:precision=double")]);
        }
        // Never silently replace a wide measured range by loudnorm's default 7 LU.
        // Its supported maximum is 50 LU; reject rather than promise preservation
        // for a range the selected dynamic algorithm cannot represent.
        if self.input.range > 50.0 || !(-99.0..=0.0).contains(&self.input.integrated) {
            return Err(ExportError::Failed("The measured loudness range or level cannot be represented by the peak-limited normalization pass".into()));
        }
        let limiter_peak = ceiling.max(-9.0);
        let attenuation = ceiling - limiter_peak;
        let offset = self.input.offset + self.correction - attenuation;
        if !(-99.0..=99.0).contains(&offset) {
            return Err(ExportError::Failed(
                "Loudness correction exceeds the supported gain range".into(),
            ));
        }
        Ok(vec![
            format!(
                "loudnorm=I={:.1}:TP={limiter_peak:.6}:LRA={:.2}:measured_I={:.2}:measured_TP={:.2}:measured_LRA={:.2}:measured_thresh={:.2}:offset={offset:.6}:linear=false:dual_mono=false",
                self.target.integrated(),
                self.input.range.max(1.0),
                self.input.integrated,
                self.input.true_peak,
                self.input.range,
                self.input.threshold,
            ),
            // loudnorm's dynamic mode produces 192 kHz. Restore the selected
            // source rate before encoding; verification includes this resampling.
            format!("aresample={}", self.sample_rate),
            format!("volume={attenuation:.9}dB:precision=double"),
        ])
    }

    /// Returns false only after computing a correction for another original-input
    /// encode. Even the final failed attempt never reaches target publication.
    pub(super) fn verify_candidate(
        &mut self,
        staging: &StagedExport,
        executable: &Path,
        cancelled: &AtomicBool,
        progress: &(impl Fn(Duration) + Sync),
        attempt: usize,
    ) -> Result<bool, ExportError> {
        let request = ExportRequest {
            source: staging.output.clone(),
            target: staging.output.with_extension("wav"),
            kind: MediaKind::Audio,
            operations: Vec::new(),
            hardware_encode: false,
        };
        let streams = ExportStreams::probe(&request)?;
        let output = measure(
            self.target,
            &request,
            &streams,
            staging,
            executable,
            cancelled,
            progress,
        )?;
        let error = self.target.integrated() - output.integrated;
        // FFmpeg prints hundredths. Reserve the half-step rounding uncertainty
        // rather than accepting a reported overshoot as a measurement tolerance.
        let peak_excess = output.true_peak + 0.005 - self.target.true_peak();
        if error.abs() <= LOUDNESS_TOLERANCE + 1e-9 && peak_excess <= 0.0 {
            return Ok(true);
        }
        if attempt + 1 >= MAX_ATTEMPTS {
            return Err(ExportError::Failed(format!(
                "Encoded audio did not meet {:.1} LUFS (+/-{LOUDNESS_TOLERANCE:.1} LU) / maximum {:.1} dBTP after {MAX_ATTEMPTS} attempts: measured {:.2} LUFS / {:.2} dBTP; nothing was published",
                self.target.integrated(),
                self.target.true_peak(),
                output.integrated,
                output.true_peak,
            )));
        }
        self.correction += error;
        if peak_excess > 0.0 {
            self.peak_margin += peak_excess + 0.1;
        }
        Ok(false)
    }
}

fn sample_rate(streams: &ExportStreams) -> Result<i32, ExportError> {
    streams
        .audio
        .map(|(_, base)| base.denominator())
        .filter(|rate| *rate > 0)
        .ok_or_else(|| {
            ExportError::Failed(
                "Loudness normalization requires an audio stream with a valid sample rate".into(),
            )
        })
}

fn measure(
    target: LoudnessTarget,
    request: &ExportRequest,
    streams: &ExportStreams,
    staging: &StagedExport,
    executable: &Path,
    cancelled: &AtomicBool,
    progress: &(impl Fn(Duration) + Sync),
) -> Result<Measurement, ExportError> {
    let rate = sample_rate(streams)?;
    let mut analysis = streams.clone();
    analysis.audio_post_filters.extend([
        "aformat=sample_fmts=dbl".into(),
        STATISTICS.into(),
        format!(
            "loudnorm@towavue_loudness=I={:.1}:TP={:.1}:LRA=50:dual_mono=false:print_format=json",
            target.integrated(),
            target.true_peak()
        ),
        format!("aresample={rate}"),
    ]);
    progress(Duration::ZERO);
    let log =
        audio_options::analysis_log(request, &analysis, staging, executable, cancelled, progress)?;
    parse(&log, rate)
}

fn parse(log: &str, rate: i32) -> Result<Measurement, ExportError> {
    let invalid =
        || ExportError::Failed("Loudness analysis is incomplete, non-finite or ambiguous".into());
    let statistic = |name: &str| -> Result<f64, ExportError> {
        let mut values = log.lines().filter_map(|line| {
            line.strip_prefix("[astats@towavue_loudness_samples @ ")?
                .split_once("] ")?
                .1
                .strip_prefix(name)
        });
        let value = values
            .next()
            .and_then(|text| text.trim().parse::<f64>().ok())
            .ok_or_else(invalid)?;
        if values.next().is_some() {
            return Err(invalid());
        }
        Ok(value)
    };
    let samples = statistic("Number of samples: ")?;
    if !samples.is_finite()
        || samples <= 0.0
        || statistic("Number of NaNs: ")? != 0.0
        || statistic("Number of Infs: ")? != 0.0
    {
        return Err(invalid());
    }
    let peak = statistic("Peak level dB: ")?;
    if peak == f64::NEG_INFINITY {
        return Err(ExportError::Failed(
            "Silent audio has no measurable integrated loudness; choose Off or Peak".into(),
        ));
    }
    if !peak.is_finite() {
        return Err(invalid());
    }
    if samples / f64::from(rate) < 0.4 {
        return Err(ExportError::Failed(
            "Audio is too short for integrated loudness measurement (at least 400 ms is required)"
                .into(),
        ));
    }
    let mut blocks = log
        .split("[loudnorm@towavue_loudness @ ")
        .skip(1)
        .filter_map(|tail| {
            let (_, tail) = tail.split_once("]")?;
            tail.trim_start()
                .strip_prefix('{')?
                .split_once('}')
                .map(|(body, _)| body)
        });
    let block = blocks.next().ok_or_else(invalid)?;
    if blocks.next().is_some() {
        return Err(invalid());
    }
    let value = |key: &str| -> Result<f64, ExportError> {
        let mut matches = block.lines().filter_map(|line| {
            let (name, value) = line.trim().split_once(':')?;
            (name.trim() == format!("\"{key}\""))
                .then_some(value.trim().trim_end_matches(',').trim_matches('"'))
        });
        let result = matches
            .next()
            .and_then(|text| text.parse::<f64>().ok())
            .ok_or_else(invalid)?;
        if matches.next().is_some() {
            return Err(invalid());
        }
        Ok(result)
    };
    let integrated = value("input_i")?;
    if integrated == f64::NEG_INFINITY {
        return Err(ExportError::Failed("Audio has no measurable gated integrated loudness; it may be below the absolute loudness gate".into()));
    }
    let measurement = Measurement {
        integrated,
        true_peak: value("input_tp")?,
        range: value("input_lra")?,
        threshold: value("input_thresh")?,
        offset: value("target_offset")?,
    };
    if ![
        measurement.integrated,
        measurement.true_peak,
        measurement.range,
        measurement.threshold,
        measurement.offset,
    ]
    .iter()
    .all(|value| value.is_finite())
        || !(0.0..=99.0).contains(&measurement.range)
        || !(-99.0..=0.0).contains(&measurement.threshold)
    {
        return Err(invalid());
    }
    Ok(measurement)
}

#[cfg(test)]
#[path = "export_loudness_tests.rs"]
mod tests;
