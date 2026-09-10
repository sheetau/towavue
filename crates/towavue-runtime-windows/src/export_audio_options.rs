use super::*;
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AudioChannels {
    #[default]
    Keep,
    Mono,
    Stereo,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AudioExportOptions {
    /// Apply one common gain to reach -1 dBFS sample peak after edits and channel conversion.
    pub normalize_peak: bool,
    pub channels: AudioChannels,
}

impl AudioExportOptions {
    pub(super) fn filters(self, channels: Option<u16>) -> Result<Vec<String>, ExportError> {
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
    let request = ExportRequest {
        kind: MediaKind::Audio,
        target: request.target.with_extension("wav"),
        hardware_encode: false,
        ..request.clone()
    };
    let mut streams = streams.clone();
    streams.video = None;
    streams.audio_post_filters.push("astats@towavue_peak=metadata=0:reset=0:measure_perchannel=none:measure_overall=Peak_level+Number_of_samples+Number_of_NaNs+Number_of_Infs".into());
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
            "Audio normalization analysis failed: {}",
            String::from_utf8_lossy(&result.stderr).trim()
        )));
    }
    gain_from_statistics(&String::from_utf8_lossy(&result.stderr))
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
