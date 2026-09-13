use super::*;
use std::path::PathBuf;
use std::time::SystemTime;

const MAX_CHECKPOINTS: usize = 512;
const MAX_PROBE_PACKETS: usize = 4096;

#[derive(Clone, Copy)]
struct Checkpoint {
    position: isize,
    pts: i64,
    size: usize,
    crc: u32,
    next_sample: i64,
}

/// Scalar bookmarks belong to one open demux input, never to a global path cache.
/// The audio worker exclusively mutates them during decoding; its scoped borrow
/// ends after worker join. No packet payload, decoder, resampler or native pointer
/// survives a run here.
pub(super) struct AudioCheckpoints {
    path: PathBuf,
    stamp: Option<(u64, SystemTime)>,
    points: VecDeque<Checkpoint>,
    pending: Option<Checkpoint>,
    previous_position: Option<isize>,
    next_checkpoint: i64,
    #[cfg(test)]
    pub(super) resumes: usize,
    #[cfg(test)]
    pub(super) probe_packets: usize,
    #[cfg(test)]
    pub(super) worker_packets: usize,
}

impl AudioCheckpoints {
    #[cfg(test)]
    pub(super) fn discard_for_comparison(&mut self) {
        self.points.clear();
    }

    pub(super) fn new(path: &Path) -> Self {
        Self {
            path: path.to_owned(),
            stamp: Self::stamp(path),
            points: VecDeque::new(),
            pending: None,
            previous_position: None,
            next_checkpoint: 0,
            #[cfg(test)]
            resumes: 0,
            #[cfg(test)]
            probe_packets: 0,
            #[cfg(test)]
            worker_packets: 0,
        }
    }

    fn stamp(path: &Path) -> Option<(u64, SystemTime)> {
        let metadata = std::fs::metadata(path).ok()?;
        Some((metadata.len(), metadata.modified().ok()?))
    }

    pub(super) fn begin_run(&mut self) {
        if self.stamp != Self::stamp(&self.path) {
            // A changed source needs a fresh demux owner before learning again.
            self.stamp = None;
            self.points.clear();
        }
        self.pending = None;
        self.previous_position = None;
        self.next_checkpoint = 0;
        #[cfg(test)]
        {
            self.probe_packets = 0;
            self.worker_packets = 0;
        }
    }

    pub(super) fn resume(
        &mut self,
        input: &mut format::context::Input,
        config: &StreamConfig,
        target: MediaTime,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Option<(i64, ffmpeg::Packet)>, DecodeError> {
        if self.stamp.is_none()
            || !matches!(
                config.sample_preroll(target),
                Some((_, PrerollSamples::Pcm(_) | PrerollSamples::Flac))
            )
        {
            return Ok(None);
        }
        // Parameters remains owned by config throughout this immutable scalar read.
        let rate = unsafe { (*config.parameters.as_ptr()).sample_rate };
        let target_sample =
            target
                .as_nanoseconds()
                .rescale_with((1, 1_000_000_000), (1, rate), Rounding::Down);
        let Some(point) = self
            .points
            .iter()
            .filter(|point| point.next_sample <= target_sample)
            .max_by_key(|point| point.next_sample)
            .copied()
        else {
            return Ok(None);
        };
        let origin = input_origin(input);
        let offset = origin.rescale(ffmpeg::rescale::TIME_BASE, config.time_base);
        let seek_tick = point.pts.saturating_add(offset).saturating_sub(1);
        check_cancelled(cancelled)?;
        // Select audio explicitly: an interleaved input's default video stream
        // can otherwise force the probe to start at a much earlier video GOP.
        if seek_stream_packet(input, config.index, seek_tick).is_ok() {
            // A container seek may land before, after or nowhere near a bookmark.
            // Do not seed a sample axis until its exact packet is found. Bound
            // failed probes; the caller rewinds and uses the original prefix path.
            for (stream, mut packet) in input.packets().take(MAX_PROBE_PACKETS) {
                check_cancelled(cancelled)?;
                #[cfg(test)]
                {
                    self.probe_packets += 1;
                }
                if stream.index() != config.index {
                    continue;
                }
                if packet.position() > point.position {
                    break;
                }
                normalize_packet_time(&mut packet, config.time_base, origin);
                if packet.position() == point.position
                    && packet.pts() == Some(point.pts)
                    && packet.size() == point.size
                    && packet.side_data().next().is_none()
                    && packet
                        .data()
                        .is_some_and(|data| crc32fast::hash(data) == point.crc)
                {
                    #[cfg(test)]
                    {
                        self.resumes += 1;
                    }
                    return Ok(Some((point.next_sample, packet)));
                }
            }
        }
        self.points.retain(|entry| entry.position != point.position);
        Ok(None)
    }

    pub(super) fn observe(
        &mut self,
        packet: &ffmpeg::Packet,
        next_sample: i64,
        sample_rate: u32,
        counted: bool,
    ) {
        #[cfg(test)]
        {
            self.worker_packets += 1;
        }
        let position = packet.position();
        // Defer publication until the following packet proves this file position
        // is unique. In particular, never resume inside an ambiguous laced block.
        if let Some(point) = self.pending.take()
            && position > point.position
            && !self
                .points
                .iter()
                .any(|entry| entry.position == point.position)
        {
            if self.points.len() == MAX_CHECKPOINTS {
                self.points.pop_front();
            }
            self.points.push_back(point);
        }
        if self.stamp.is_some()
            && counted
            && next_sample >= self.next_checkpoint
            && position >= 0
            && self
                .previous_position
                .is_some_and(|previous| previous < position)
            && packet.side_data().next().is_none()
            && let Some(pts) = packet.pts()
            && let Some(data) = packet.data()
        {
            self.pending = Some(Checkpoint {
                position,
                pts,
                size: packet.size(),
                crc: crc32fast::hash(data),
                next_sample,
            });
            self.next_checkpoint = next_sample.saturating_add(i64::from(sample_rate));
        }
        self.previous_position = Some(position);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::audio_seek_tests::{collect, fixture, samples};

    fn packet(position: isize) -> ffmpeg::Packet {
        let mut packet = ffmpeg::Packet::copy(&position.to_le_bytes());
        packet.set_position(position);
        packet.set_pts(Some(position as i64));
        packet
    }

    #[test]
    fn checkpoints_are_bounded_and_exclude_unconfirmed_or_shared_positions() {
        let mut cache =
            AudioCheckpoints::new(&Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"));
        assert!(cache.stamp.is_some());
        for (sample, position) in [0, 10, 10, 20, 30].into_iter().enumerate() {
            cache.observe(&packet(position), sample as i64, 1, true);
        }
        assert_eq!(cache.points.len(), 1);
        assert_eq!(
            cache.points[0].position, 20,
            "laced and pending positions excluded"
        );
        cache.begin_run();
        assert!(cache.stamp.is_some());
        assert_eq!(
            cache.points.len(),
            1,
            "verified points survive cancellation"
        );
        assert!(
            cache.pending.is_none(),
            "cancelled run cannot publish pending point"
        );

        cache.path.push("not-a-directory");
        cache.begin_run();
        assert!(cache.stamp.is_none(), "missing source disables reuse");
        assert!(cache.points.is_empty());

        cache.stamp = Some((0, SystemTime::UNIX_EPOCH));
        for index in 0..MAX_CHECKPOINTS + 10 {
            cache.observe(&packet(index as isize * 100), index as i64, 1, true);
        }
        assert_eq!(cache.points.len(), MAX_CHECKPOINTS);
        assert!(cache.points.front().expect("oldest retained").position > 100);
        assert!(size_of::<Checkpoint>() * (MAX_CHECKPOINTS + 1) <= 32 * 1024);
    }

    #[test]
    fn unverified_checkpoints_rewind_without_reusing_an_inexact_sample_axis() {
        let (directory, source, _) = fixture(48_000, 1, "pcm_s16le", 4, 1001);
        let target = MediaTime::from_nanoseconds(3_317_000_000);
        let end = target.saturating_add(Duration::from_millis(50));
        let expected = samples(&source, target, end, MediaTime::ZERO).0;
        for failure in ["payload", "position", "timestamp", "stamp"] {
            let mut input = ParallelInput::open(&source, &|| false).expect("input");
            assert!(collect(&mut input, target, end, target).0 == expected);
            assert!(!input.audio_checkpoints.points.is_empty());
            for point in &mut input.audio_checkpoints.points {
                match failure {
                    "payload" => point.crc ^= 1,
                    "position" => point.position -= 1,
                    "timestamp" => point.pts += 123,
                    "stamp" => {}
                    _ => unreachable!(),
                }
            }
            if failure == "stamp" {
                input.audio_checkpoints.stamp.as_mut().expect("stamp").0 += 1;
            }
            assert!(
                collect(&mut input, target, end, target).0 == expected,
                "{failure}: fallback PCM"
            );
            assert_eq!(
                input.audio_checkpoints.resumes, 0,
                "{failure}: no stale axis"
            );
            assert!(
                input.audio_checkpoints.worker_packets > 100,
                "fallback traverses prefix"
            );
            if failure == "stamp" {
                assert!(input.audio_checkpoints.stamp.is_none());
                assert!(
                    input.audio_checkpoints.points.is_empty(),
                    "no relearning on changed owner"
                );
            }
        }
        std::fs::remove_dir_all(directory).expect("remove owned fixtures");
    }
}
