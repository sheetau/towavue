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
}

impl AudioTempo {
    pub(crate) fn new(sample_rate: u32, rate: f32) -> Result<Self, Error> {
        let factor = f64::from(rate).sqrt();
        Self::from_chain(
            sample_rate,
            (rate != 1.0).then(|| format!("atempo={factor},atempo={factor}")),
        )
    }

    pub(crate) fn timeline(sample_rate: u32, rate: f64) -> Result<Self, Error> {
        Self::from_chain(
            sample_rate,
            (rate != 1.0).then(|| filters(rate, 9).join(",")),
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
        })
    }

    pub(crate) fn push(&mut self, bytes: &[u8], output: &mut VecDeque<u8>) -> Result<(), Error> {
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
        Self::drain(graph, output)
    }

    pub(crate) fn finish(&mut self, output: &mut VecDeque<u8>) -> Result<(), Error> {
        if let Some(graph) = &mut self.graph {
            graph.get("in").expect("audio source").source().flush()?;
            Self::drain(graph, output)?;
        }
        Ok(())
    }

    fn drain(graph: &mut filter::Graph, output: &mut VecDeque<u8>) -> Result<(), Error> {
        loop {
            let mut frame = frame::Audio::empty();
            match graph
                .get("out")
                .expect("audio sink")
                .sink()
                .frame(&mut frame)
            {
                Ok(()) => output.extend(&frame.data(0)[..frame.samples() * 8]),
                Err(Error::Eof) => return Ok(()),
                Err(Error::Other { errno }) if errno == ffmpeg_next::error::EAGAIN => return Ok(()),
                Err(error) => return Err(error),
            }
        }
    }
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
        for rate in [0.25, 0.5, 1.0, 1.5, 2.0, 4.0] {
            let mut tempo = AudioTempo::new(SAMPLE_RATE, rate).expect("tempo filter");
            let mut output = VecDeque::new();
            for chunk in source.chunks(1024 * 8) {
                tempo.push(chunk, &mut output).expect("filter input");
            }
            tempo.finish(&mut output).expect("drain tail");
            let bytes = output.make_contiguous();
            if rate == 1.0 {
                assert_eq!(bytes, source);
            }
            let expected_frames = f64::from(SAMPLE_RATE) * 4.0 / f64::from(rate);
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
