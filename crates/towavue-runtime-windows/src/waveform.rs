use std::io::{self, Read};

const BINS_PER_COLUMN: usize = 1024;

struct Envelope {
    // Completed bins have equal sample counts; only the pending tail may be shorter.
    sums: Vec<u64>,
    bin_samples: u64,
    pending_sum: u64,
    pending_samples: u64,
    total_samples: u64,
    limit: usize,
}

impl Envelope {
    fn new(width: u32) -> Self {
        let limit = width as usize * BINS_PER_COLUMN;
        Self {
            sums: Vec::with_capacity(limit),
            bin_samples: 1,
            pending_sum: 0,
            pending_samples: 0,
            total_samples: 0,
            limit,
        }
    }

    fn push(&mut self, mut pcm: &[u8]) {
        while !pcm.is_empty() {
            let count = (self.bin_samples - self.pending_samples).min((pcm.len() / 2) as u64);
            let bytes = count as usize * 2;
            self.pending_sum += pcm[..bytes]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|sample| u64::from(i16::from_le_bytes([sample[0], sample[1]]).unsigned_abs()))
                .sum::<u64>();
            self.pending_samples += count;
            self.total_samples += count;
            pcm = &pcm[bytes..];
            if self.pending_samples == self.bin_samples {
                self.sums.push(self.pending_sum);
                self.pending_sum = 0;
                self.pending_samples = 0;
                if self.sums.len() == self.limit {
                    for index in 0..self.limit / 2 {
                        self.sums[index] = self.sums[index * 2] + self.sums[index * 2 + 1];
                    }
                    self.sums.truncate(self.limit / 2);
                    self.bin_samples *= 2;
                }
            }
        }
    }

    fn means(&self, width: u32) -> io::Result<Vec<u16>> {
        let per_column = self.total_samples / u64::from(width);
        if per_column == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "too few audio samples for waveform",
            ));
        }
        let mut means = Vec::with_capacity(width as usize);
        for column in 0..u64::from(width) {
            let start = column * per_column;
            let end = if column + 1 == u64::from(width) {
                self.total_samples
            } else {
                start + per_column
            };
            let mut position = start;
            let mut sum = 0.0;
            while position < end {
                let index = (position / self.bin_samples) as usize;
                let (bin_sum, count) = self
                    .sums
                    .get(index)
                    .map(|sum| (*sum, self.bin_samples))
                    .unwrap_or((self.pending_sum, self.pending_samples));
                let overlap = end.min(index as u64 * self.bin_samples + count) - position;
                sum += bin_sum as f64 * overlap as f64 / count as f64;
                position += overlap;
            }
            means.push((sum / (end - start) as f64).floor() as u16);
        }
        Ok(means)
    }
}

pub(crate) fn read(reader: &mut dyn Read, width: u32, height: u32) -> io::Result<image::RgbaImage> {
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
        envelope.push(&buffer[..total / 2 * 2]);
        carried = total % 2;
        if carried != 0 {
            buffer[0] = buffer[total - 1];
        }
    }
    let mut image = image::RgbaImage::new(width, height);
    for (x, mean) in envelope.means(width)?.into_iter().enumerate() {
        let bar =
            ((u64::from(mean) * u64::from(height) + 16_383) / 32_767).min(u64::from(height)) as u32;
        for y in (height - bar) / 2..(height - bar) / 2 + bar {
            image.put_pixel(x as u32, y, image::Rgba([255; 4]));
        }
    }
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_stays_bounded_and_preserves_average_amplitude() {
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
                    assert!(envelope.sums.len() <= width as usize * BINS_PER_COLUMN);
                    assert_eq!(envelope.sums.capacity(), width as usize * BINS_PER_COLUMN);
                }
                let means = envelope.means(width).expect("means");
                let per_column = length / width as usize;
                for (column, mean) in means.into_iter().enumerate() {
                    let start = column * per_column;
                    let end = if column + 1 == width as usize {
                        length
                    } else {
                        start + per_column
                    };
                    let exact = (samples[start..end]
                        .iter()
                        .map(|sample| u64::from(sample.unsigned_abs()))
                        .sum::<u64>()
                        / (end - start) as u64) as u16;
                    if envelope.bin_samples == 1 {
                        assert_eq!(mean, exact);
                    }
                    assert!(
                        mean.abs_diff(exact) <= 129,
                        "length {length}, pattern {pattern}, column {column}: {mean} != {exact}"
                    );
                    let bar = |value: u16| (u64::from(value) * 160 + 16_383) / 32_767;
                    assert!(bar(mean).abs_diff(bar(exact)) <= 1);
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
