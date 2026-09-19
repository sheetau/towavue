use super::*;
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AudioChannels {
    #[default]
    Keep,
    Mono,
    Stereo,
}

/// Output normalization is independent of listening volume and edit history.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AudioNormalization {
    #[default]
    Off,
    /// One common gain to -1 dBFS sample peak; encoded peaks are not constrained.
    Peak,
    Loudness(LoudnessTarget),
}

impl AudioNormalization {
    pub fn is_enabled(self) -> bool {
        self != Self::Off
    }
}

/// Tenths avoid floating-point equality in tab/export snapshot comparisons.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LoudnessTarget {
    pub integrated_tenths: i16,
    pub true_peak_tenths: i16,
}

impl Default for LoudnessTarget {
    fn default() -> Self {
        Self {
            integrated_tenths: -140,
            true_peak_tenths: -10,
        }
    }
}

impl LoudnessTarget {
    pub fn validate(self) -> Result<(), ExportError> {
        if !(-700..=-50).contains(&self.integrated_tenths)
            || !(-90..=0).contains(&self.true_peak_tenths)
        {
            return Err(ExportError::Failed(
                "Loudness target must be -70 to -5 LUFS and maximum true peak -9 to 0 dBTP".into(),
            ));
        }
        Ok(())
    }

    pub(super) fn integrated(self) -> f64 {
        f64::from(self.integrated_tenths) / 10.0
    }
    pub(super) fn true_peak(self) -> f64 {
        f64::from(self.true_peak_tenths) / 10.0
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AudioExportOptions {
    pub normalization: AudioNormalization,
    pub channels: AudioChannels,
}

impl AudioExportOptions {
    pub(super) fn filters(self, channels: Option<u16>) -> Result<Vec<String>, ExportError> {
        if let AudioNormalization::Loudness(target) = self.normalization {
            target.validate()?;
        }
        if self == Self::default() {
            return Ok(Vec::new());
        }
        let channels = channels.filter(|count| *count > 0).ok_or_else(|| {
            ExportError::Failed("Audio export options require an audio stream".into())
        })?;
        let mut filters = vec!["aformat=sample_fmts=dbl".into()];
        match (self.channels, channels) {
            (AudioChannels::Keep, _) | (AudioChannels::Mono, 1) | (AudioChannels::Stereo, 2) => {}
            (AudioChannels::Mono, 2) => filters.push("pan=mono|c0=0.5*c0+0.5*c1".into()),
            (AudioChannels::Stereo, 1) => filters.push("pan=stereo|c0=c0|c1=c0".into()),
            _ => return Err(ExportError::Failed("Mono/stereo conversion requires a mono or stereo source; use Keep for multichannel audio".into())),
        }
        Ok(filters)
    }
}

#[derive(Eq, PartialEq)]
pub(super) struct SourceStamp {
    length: u64,
    modified: SystemTime,
}

impl SourceStamp {
    pub(super) fn read(source: &Path) -> Result<Self, ExportError> {
        let metadata = fs::metadata(source).map_err(ExportError::Output)?;
        Ok(Self {
            length: metadata.len(),
            modified: metadata.modified().map_err(ExportError::Output)?,
        })
    }

    pub(super) fn verify(&self, source: &Path) -> Result<(), ExportError> {
        if *self != Self::read(source)? {
            return Err(ExportError::Failed(
                "The source changed during export; nothing was published".into(),
            ));
        }
        Ok(())
    }
}

pub(super) fn analyze(
    request: &ExportRequest,
    streams: &ExportStreams,
    staging: &StagedExport,
    executable: &Path,
    cancelled: &AtomicBool,
    progress: &(impl Fn(Duration) + Sync),
) -> Result<f64, ExportError> {
    let mut streams = streams.clone();
    streams.audio_post_filters.push("astats@towavue_peak=metadata=0:reset=0:measure_perchannel=none:measure_overall=Peak_level+Number_of_samples+Number_of_NaNs+Number_of_Infs".into());
    let log = analysis_log(request, &streams, staging, executable, cancelled, progress)?;
    gain_from_statistics(&log)
}

pub(super) fn count_samples(
    request: &ExportRequest,
    streams: &ExportStreams,
    staging: &StagedExport,
    executable: &Path,
    cancelled: &AtomicBool,
    progress: &(impl Fn(Duration) + Sync),
) -> Result<u64, ExportError> {
    let mut request = request.clone();
    request.operations.push(EditOperation::SetRate(1.0));
    let mut streams = streams.clone();
    streams.audio_post_filters = vec!["astats@towavue_count=metadata=0:reset=0:measure_perchannel=none:measure_overall=Number_of_samples".into()];
    let log = analysis_log(&request, &streams, staging, executable, cancelled, progress)?;
    sample_count_from_statistics(&log)
}

fn sample_count_from_statistics(log: &str) -> Result<u64, ExportError> {
    let mut values = log.lines().filter_map(|line| {
        line.strip_prefix("[astats@towavue_count @ ")?
            .split_once("] ")?
            .1
            .strip_prefix("Number of samples: ")
    });
    let count = values
        .next()
        .and_then(|value| value.trim().parse::<u64>().ok());
    if values.next().is_some() || count.is_none_or(|count| count == 0) {
        return Err(ExportError::Failed(
            "Audio sample-count analysis is empty, invalid or ambiguous".into(),
        ));
    }
    Ok(count.expect("validated count"))
}

pub(super) fn analysis_log(
    request: &ExportRequest,
    streams: &ExportStreams,
    staging: &StagedExport,
    executable: &Path,
    cancelled: &AtomicBool,
    progress: &(impl Fn(Duration) + Sync),
) -> Result<String, ExportError> {
    let request = ExportRequest {
        kind: MediaKind::Audio,
        target: request.target.with_extension("wav"),
        hardware_encode: false,
        ..request.clone()
    };
    let mut streams = streams.clone();
    streams.video = None;
    let mut arguments = staging.arguments(&request, false, &streams)?;
    let level = arguments
        .iter()
        .position(|argument| argument == "-loglevel")
        .expect("export log level");
    arguments[level + 1] = "info".into();
    let codec = arguments
        .iter()
        .position(|argument| argument == "-c:a")
        .expect("audio analysis codec");
    arguments[codec + 1] = "pcm_f64le".into();
    arguments.pop();
    arguments.extend(["-f".into(), "null".into(), "-".into()]);
    // Stream to the null muxer; never retain an audio-length PCM buffer or intermediate file.
    let result = run_ffmpeg(executable, arguments, cancelled, progress)?;
    check_cancelled(cancelled)?;
    if !result.status.success() {
        return Err(ExportError::Failed(format!(
            "Audio analysis failed: {}",
            String::from_utf8_lossy(&result.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&result.stderr).into_owned())
}

fn gain_from_statistics(log: &str) -> Result<f64, ExportError> {
    let value = |name: &str| -> Result<f64, ExportError> {
        let mut matches = log.lines().filter_map(|line| {
            line.strip_prefix("[astats@towavue_peak @ ")?
                .split_once("] ")?
                .1
                .strip_prefix(name)
        });
        let number = matches
            .next()
            .and_then(|value| value.trim().parse::<f64>().ok());
        if matches.next().is_some() || number.is_none() {
            return Err(ExportError::Failed(format!(
                "Audio normalization analysis is incomplete or ambiguous: {name}"
            )));
        }
        Ok(number.expect("validated statistic"))
    };
    let samples = value("Number of samples: ")?;
    if !samples.is_finite()
        || samples <= 0.0
        || value("Number of NaNs: ")? != 0.0
        || value("Number of Infs: ")? != 0.0
    {
        return Err(ExportError::Failed(
            "Audio normalization requires nonempty finite audio samples".into(),
        ));
    }
    let peak = value("Peak level dB: ")?;
    if peak == f64::NEG_INFINITY {
        return Ok(1.0);
    }
    let gain = 10_f64.powf((-1.0 - peak) / 20.0);
    if !peak.is_finite() || !gain.is_finite() || gain <= 0.0 {
        return Err(ExportError::Failed(
            "Audio normalization produced an invalid peak or gain".into(),
        ));
    }
    Ok(gain)
}

#[cfg(test)]
#[path = "export_audio_options_tests.rs"]
mod tests;
