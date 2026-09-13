use std::collections::VecDeque;

use ffmpeg_next::{ChannelLayout, Error, filter, format::Sample, frame};

pub(crate) fn output_sample_boundary(nanoseconds: i64, sample_rate: u32, rate: f32) -> u64 {
    // Export's malformed NaN rate still reaches filter validation, not integer division.
    if rate.is_nan() {
        return 0;
    }
    debug_assert!((0.25..=4.0).contains(&rate));
    // Every f32 master rate in the validated range is an exact multiple of 2^-25.
    // Integer ceil avoids padding an aligned join because floating-point seconds rounded up.
    const SCALE: i128 = 1 << 25;
    let rate_units = (f64::from(rate) * SCALE as f64) as i128;
    let numerator = i128::from(nanoseconds.max(0)) * i128::from(sample_rate) * SCALE;
    let denominator = 1_000_000_000 * rate_units;
    ((numerator + denominator - 1) / denominator).min(i128::from(u64::MAX)) as u64
}

pub(crate) struct AudioTempo {
    graph: Option<filter::Graph>,
    sample_rate: u32,
    input_frames: i64,
    finished: bool,
    ordinary_rate: Option<f32>,
    output_frames: u64,
    pending: VecDeque<u8>,
}

impl AudioTempo {
    pub(crate) fn new(sample_rate: u32, rate: f32) -> Result<Self, Error> {
        if !(0.25..=4.0).contains(&rate) {
            return Err(Error::InvalidData);
        }
        let factor = f64::from(rate).sqrt();
        let mut tempo = Self::from_chain(
            sample_rate,
            (rate != 1.0).then(|| {
                with_eof_context(vec![format!("atempo={factor}"); 2], sample_rate).join(",")
            }),
        )?;
        tempo.ordinary_rate = (rate != 1.0).then_some(rate);
        Ok(tempo)
    }

    pub(crate) fn timeline(sample_rate: u32, rate: f64, output_frames: u64) -> Result<Self, Error> {
        Self::from_chain(
            sample_rate,
            (rate != 1.0).then(|| {
                // atempo can otherwise flush no audio for sub-window spans.
                // Supply EOF context, bounded by the caller's exact output interval.
                format!(
                    "{},atrim=end_sample={output_frames}",
                    timeline_filters(rate, 9, sample_rate).join(",")
                )
            }),
        )
    }

    fn from_chain(sample_rate: u32, chain: Option<String>) -> Result<Self, Error> {
        let graph = if let Some(chain) = chain {
            ffmpeg_next::init()?;
            let mut graph = filter::Graph::new();
            graph.add(&filter::find("abuffer").ok_or(Error::FilterNotFound)?, "in",
                &format!("time_base=1/{sample_rate}:sample_rate={sample_rate}:sample_fmt=flt:channel_layout=stereo"))?;
            graph.add(
                &filter::find("abuffersink").ok_or(Error::FilterNotFound)?,
                "out",
                "",
            )?;
            graph.output("in", 0)?.input("out", 0)?.parse(&format!(
                "{},aformat=sample_fmts=flt:sample_rates={sample_rate}:channel_layouts=stereo",
                chain
            ))?;
            graph.validate()?;
            Some(graph)
        } else {
            None
        };
        Ok(Self {
            graph,
            sample_rate,
            input_frames: 0,
            finished: false,
            ordinary_rate: None,
            output_frames: 0,
            pending: VecDeque::new(),
        })
    }

    pub(crate) fn push(&mut self, bytes: &[u8], output: &mut VecDeque<u8>) -> Result<(), Error> {
        // A bounded timeline sink can finish before the source span is exhausted.
        if self.finished {
            return Ok(());
        }
        let Some(graph) = &mut self.graph else {
            output.extend(bytes);
            return Ok(());
        };
        let samples = bytes.len() / 8;
        let mut input = frame::Audio::new(
            Sample::F32(ffmpeg_next::format::sample::Type::Packed),
            samples,
            ChannelLayout::STEREO,
        );
        input.set_rate(self.sample_rate);
        input.set_pts(Some(self.input_frames));
        self.input_frames += samples as i64;
        input.data_mut(0)[..bytes.len()].copy_from_slice(bytes);
        graph
            .get("in")
            .expect("audio source")
            .source()
            .add(&input)?;
        if self.ordinary_rate.is_some() {
            self.finished = Self::drain(graph, &mut self.pending)?;
            self.publish_ordinary(output);
        } else {
            self.finished = Self::drain(graph, output)?;
        }
        Ok(())
    }

    pub(crate) fn finish(&mut self, output: &mut VecDeque<u8>) -> Result<(), Error> {
        if self.finished {
            return Ok(());
        }
        if let Some(graph) = &mut self.graph {
            graph.get("in").expect("audio source").source().flush()?;
            if self.ordinary_rate.is_some() {
                Self::drain(graph, &mut self.pending)?;
                self.publish_ordinary(output);
                self.pending.clear();
            } else {
                Self::drain(graph, output)?;
            }
        }
        self.finished = true;
        Ok(())
    }

    fn publish_ordinary(&mut self, output: &mut VecDeque<u8>) {
        let rate = self.ordinary_rate.expect("ordinary tempo");
        // Cap against actual received samples, not estimated media duration. Keep
        // any not-yet-publishable output for later input; played samples cannot be
        // retracted at EOF. Discard output beyond the final duration only at EOF.
        const SCALE: u128 = 1 << 25;
        let rate_units = (f64::from(rate) * SCALE as f64) as u128;
        let limit = (self.input_frames as u128 * SCALE).div_ceil(rate_units) as u64;
        let frames = (limit - self.output_frames).min(self.pending.len() as u64 / 8) as usize;
        output.extend(self.pending.drain(..frames * 8));
        self.output_frames += frames as u64;
    }

    fn drain(graph: &mut filter::Graph, output: &mut VecDeque<u8>) -> Result<bool, Error> {
        loop {
            let mut frame = frame::Audio::empty();
            match graph
                .get("out")
                .expect("audio sink")
                .sink()
                .frame(&mut frame)
            {
                Ok(()) => output.extend(&frame.data(0)[..frame.samples() * 8]),
                Err(Error::Eof) => return Ok(true),
                Err(Error::Other { errno }) if errno == ffmpeg_next::error::EAGAIN => {
                    return Ok(false);
                }
                Err(error) => return Err(error),
            }
        }
    }
}

pub(crate) fn timeline_filters(rate: f64, precision: usize, sample_rate: u32) -> Vec<String> {
    with_eof_context(filters(rate, precision), sample_rate)
}

fn with_eof_context(filters: Vec<String>, sample_rate: u32) -> Vec<String> {
    // FFmpeg atempo uses a power-of-two window >= sample_rate / 24.
    // Two windows of EOF context per stage release short spans without an
    // unbounded silence source if a truncated file ends before its planned span.
    let padding = u64::from(sample_rate / 24).next_power_of_two() * 2;
    filters
        .into_iter()
        .flat_map(|filter| [format!("apad=pad_len={padding}"), filter])
        .collect()
}

pub(crate) fn filters(mut rate: f64, precision: usize) -> Vec<String> {
    let mut filters = Vec::new();
    while rate < 0.5 {
        filters.push("atempo=0.5000".into());
        rate /= 0.5;
    }
    while rate > 2.0 {
        filters.push("atempo=2.0000".into());
        rate /= 2.0;
    }
    filters.push(format!("atempo={rate:.precision$}"));
    filters
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_sample_boundaries_use_exact_master_rate_and_integer_ceil() {
        for sample_rate in [44100_u32, 48000, 96000, 192000] {
            for (rate, rate_numerator, rate_denominator) in [
                (0.25, 1_i128, 4_i128),
                (0.5, 1, 2),
                (1.0, 1, 1),
                (1.25, 5, 4),
                (1.5, 3, 2),
                (4.0, 4, 1),
                (1.1, 9_227_469, 8_388_608),
            ] {
                for nanos in (0..2000).map(|ms| ms * 1_000_000).chain([
                    1,
                    20_833,
                    20_834,
                    999_999_999,
                    1_000_000_001,
                    i64::MAX,
                ]) {
                    let numerator = i128::from(nanos) * i128::from(sample_rate) * rate_denominator;
                    let denominator = 1_000_000_000 * rate_numerator;
                    let expected =
                        numerator / denominator + i128::from(numerator % denominator != 0);
                    assert_eq!(
                        output_sample_boundary(nanos, sample_rate, rate),
                        expected as u64,
                        "{nanos} ns / {sample_rate} Hz / {rate}x"
                    );
                }
            }
        }
        assert_eq!(output_sample_boundary(-1, 48000, 1.0), 0);
        assert_eq!(output_sample_boundary(17_000_000, 48000, f32::NAN), 0);
    }

    #[test]
    fn short_tempo_spans_retain_signal_with_bounded_eof_context() {
        for frames in [48, 480, 2400, 48000] {
            let source: Vec<_> = (0..frames)
                .flat_map(|index| {
                    let value = (index as f64 * 440.0 * std::f64::consts::TAU / 48000.0).sin()
                        as f32
                        * 0.25;
                    [value, -value].into_iter().flat_map(f32::to_le_bytes)
                })
                .collect();
            for rate in [0.0625, 0.25, 0.5, 1.5, 2.0, 4.0, 16.0] {
                let expected = (frames as f64 / rate).ceil() as usize;
                for limit in [0, expected / 2, expected] {
                    let mut tempo = AudioTempo::timeline(48000, rate, limit as u64).expect("tempo");
                    let mut output = VecDeque::new();
                    for chunk in source.chunks(127 * 8) {
                        tempo.push(chunk, &mut output).expect("push");
                    }
                    tempo.finish(&mut output).expect("flush");
                    let produced = output.len() / 8;
                    assert_eq!(produced, limit, "frames={frames} rate={rate}");
                    let nonzero = output
                        .make_contiguous()
                        .as_chunks::<8>()
                        .0
                        .iter()
                        .filter(|frame| {
                            f32::from_le_bytes(frame[..4].try_into().expect("sample")).abs()
                                > 0.0001
                        })
                        .count();
                    assert!(
                        nonzero >= frames.min(limit) / 2,
                        "short span must not disappear: frames={frames} rate={rate} limit={limit}"
                    );
                }
            }
        }
    }

    #[test]
    fn ordinary_tempo_keeps_short_input_and_exact_duration_after_queue_consumption() {
        for rate in [0.0, -1.0, 0.24, 4.01, f32::NAN, f32::INFINITY] {
            assert!(matches!(
                AudioTempo::new(48000, rate),
                Err(Error::InvalidData)
            ));
        }
        for sample_rate in [44100, 48000, 96000] {
            for frames in [0, 48, 480, 2400, sample_rate as usize] {
                let source: Vec<_> = (0..frames)
                    .flat_map(|index| {
                        let value = ((index + 1) as f64 * 440.0 * std::f64::consts::TAU
                            / f64::from(sample_rate))
                        .sin() as f32
                            * 0.25;
                        [value, -value].into_iter().flat_map(f32::to_le_bytes)
                    })
                    .collect();
                for rate in [0.25_f32, 0.5, 1.0, 1.1, 1.5, 2.0, 4.0] {
                    let expected_at = |frames: usize| {
                        let numerator = frames as u128 * (1 << 25);
                        let denominator = (f64::from(rate) * f64::from(1 << 25)) as u128;
                        numerator.div_ceil(denominator) as usize
                    };
                    let mut tempo = AudioTempo::new(sample_rate, rate).expect("ordinary tempo");
                    let mut queue = VecDeque::new();
                    let mut actual = Vec::new();
                    let mut input_frames = 0;
                    for chunk in source.chunks(127 * 8) {
                        tempo.push(chunk, &mut queue).expect("push");
                        input_frames += chunk.len() / 8;
                        actual.extend(queue.drain(..));
                        assert!(
                            actual.len() / 8 <= expected_at(input_frames),
                            "cannot retract samples already played"
                        );
                    }
                    tempo.finish(&mut queue).expect("EOF");
                    actual.extend(queue.drain(..));
                    assert_eq!(
                        actual.len() / 8,
                        expected_at(frames),
                        "{sample_rate} Hz, {frames} frames at {rate}x"
                    );
                    if frames > 0 {
                        assert!(
                            actual
                                .as_chunks::<4>()
                                .0
                                .iter()
                                .any(|sample| f32::from_le_bytes(*sample).abs() > 0.0001),
                            "short input must not become silent"
                        );
                    }
                    if rate == 1.0 {
                        assert_eq!(actual, source);
                    }
                    tempo.finish(&mut queue).expect("finished once");
                    assert!(queue.is_empty());
                }
            }
        }
    }

    #[test]
    fn truncated_tempo_input_has_finite_tail_and_finishes_once() {
        let mut tempo = AudioTempo::timeline(48000, 0.25, 48000 * 60).expect("tempo");
        let mut output = VecDeque::new();
        tempo.push(&[0; 48 * 8], &mut output).expect("short input");
        tempo.finish(&mut output).expect("finite EOF context");
        assert!(
            output.len() < 48000 * 8,
            "do not synthesize the entire missing minute"
        );
        let length = output.len();
        tempo.finish(&mut output).expect("already finished");
        tempo
            .push(&[0; 48 * 8], &mut output)
            .expect("already finished");
        assert_eq!(output.len(), length);
    }

    #[test]
    fn tempo_changes_duration_without_transposing_a_stereo_tone() {
        const SAMPLE_RATE: u32 = 48_000;
        let source = (0..SAMPLE_RATE * 4)
            .flat_map(|index| {
                let value = (f64::from(index) * 440.0 * std::f64::consts::TAU
                    / f64::from(SAMPLE_RATE))
                .sin() as f32
                    * 0.25;
                [value, -value].into_iter().flat_map(f32::to_le_bytes)
            })
            .collect::<Vec<_>>();
        for (rate, bounded) in [0.25, 0.5, 1.0, 1.5, 2.0, 4.0]
            .into_iter()
            .flat_map(|rate| [(rate, false), (rate, true)])
        {
            let expected_frames = f64::from(SAMPLE_RATE) * 4.0 / f64::from(rate);
            let mut tempo = if bounded {
                AudioTempo::timeline(SAMPLE_RATE, f64::from(rate), expected_frames.ceil() as u64)
            } else {
                AudioTempo::new(SAMPLE_RATE, rate)
            }
            .expect("tempo filter");
            let mut output = VecDeque::new();
            for chunk in source.chunks(1024 * 8) {
                tempo.push(chunk, &mut output).expect("filter input");
            }
            tempo.finish(&mut output).expect("drain tail");
            let bytes = output.make_contiguous();
            if rate == 1.0 {
                assert_eq!(bytes, source);
            }
            if bounded {
                assert_eq!(bytes.len() / 8, expected_frames.ceil() as usize);
            }
            assert!(
                (bytes.len() as f64 / 8.0 - expected_frames).abs() < 0.08 * f64::from(SAMPLE_RATE),
                "duration at {rate}x"
            );
            let samples = bytes
                .as_chunks::<8>()
                .0
                .iter()
                .map(|frame| {
                    let left = f32::from_le_bytes(frame[..4].try_into().expect("left"));
                    let right = f32::from_le_bytes(frame[4..].try_into().expect("right"));
                    assert!((left + right).abs() < 0.00001);
                    left
                })
                .collect::<Vec<_>>();
            let crossings = samples
                .windows(2)
                .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
                .count();
            let pitch = crossings as f64 * f64::from(SAMPLE_RATE) / samples.len() as f64;
            assert!((pitch - 440.0).abs() < 3.0, "pitch at {rate}x: {pitch}");
        }
    }
}
