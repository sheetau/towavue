use super::*;

fn pcm(frames: usize) -> Vec<u8> {
    (0..frames)
        .flat_map(|i| {
            let left = (i as i32 % 997 - 498) as f32 / 499.0;
            let right = (i as i32 % 613 - 306) as f32 / 307.0;
            left.to_le_bytes().into_iter().chain(right.to_le_bytes())
        })
        .collect()
}

fn scalar(envelope: &mut DisplayEnvelope, pcm: &[u8], volume: f64) -> bool {
    for frame in pcm.as_chunks::<8>().0 {
        let left = f32::from_le_bytes(frame[..4].try_into().expect("left"));
        let right = f32::from_le_bytes(frame[4..].try_into().expect("right"));
        if !left.is_finite() || !right.is_finite() {
            return false;
        }
        envelope.push(f64::from(left.abs().max(right.abs())) * volume);
    }
    true
}

#[test]
fn stereo_chunks_match_scalar_bits_at_fractional_boundaries_and_reject_invalid_samples() {
    for frames in [1, 2, 7, 500, 48_001] {
        let bytes = pcm(frames);
        for columns in [1, 3, 640, 8192] {
            for volume in [0.0, f64::from(0.7_f32), 3.0] {
                let mut reference = DisplayEnvelope::new(columns, frames as u64);
                assert!(scalar(&mut reference, &bytes, volume));
                for chunk in [1, 7, 512, 4096] {
                    let mut actual = DisplayEnvelope::new(columns, frames as u64);
                    for bytes in bytes.chunks(chunk * 8) {
                        assert!(actual.push_stereo(bytes, volume));
                    }
                    assert_eq!(actual.position, reference.position);
                    assert!(
                        actual
                            .sums
                            .iter()
                            .zip(&reference.sums)
                            .all(|(a, b)| a.to_bits() == b.to_bits()),
                        "frames={frames}, columns={columns}, volume={volume}, chunk={chunk}"
                    );
                    let actual = actual.finish().expect("complete");
                    let scale = frames as f64 / f64::from(columns);
                    assert!(
                        actual
                            .iter()
                            .zip(&reference.sums)
                            .all(|(a, b)| a.to_bits() == ((b / scale) as f32).to_bits())
                    );
                }
            }
        }
    }
    for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for channel in [0, 1] {
            let mut bytes = pcm(7);
            bytes[24 + channel * 4..28 + channel * 4].copy_from_slice(&invalid.to_le_bytes());
            assert!(!DisplayEnvelope::new(3, 7).push_stereo(&bytes, 1.0));
        }
    }
    for expected_frames in [0, 6, 8] {
        let mut envelope = DisplayEnvelope::new(3, expected_frames);
        assert!(envelope.push_stereo(&pcm(7), 1.0));
        assert!(
            envelope.finish().is_err(),
            "zero/incomplete/excess sample counts still fail"
        );
    }
}

#[test]
#[ignore = "Release-only owned stereo aggregation comparison; no decoding, file I/O or UI"]
fn stereo_chunks_report_aggregation_cost() -> Result<(), &'static str> {
    if cfg!(debug_assertions) {
        return Err("run with --release");
    }
    let frames = 48_000 * 180;
    let bytes = pcm(frames);
    for columns in [307, 8192] {
        let mut reference = DisplayEnvelope::new(columns, frames as u64);
        assert!(scalar(&mut reference, &bytes, 3.0));
        let expected = reference.finish().expect("reference");
        for chunked in [false, true, true, false] {
            let mut times = Vec::new();
            for _ in 0..5 {
                let start = std::time::Instant::now();
                let mut envelope = DisplayEnvelope::new(columns, frames as u64);
                for chunk in std::hint::black_box(&bytes).chunks(1024 * 8) {
                    if chunked {
                        assert!(envelope.push_stereo(chunk, 3.0));
                    } else {
                        assert!(scalar(&mut envelope, chunk, 3.0));
                    }
                }
                let actual = envelope.finish().expect("complete");
                std::hint::black_box(&actual);
                times.push(start.elapsed().as_secs_f64() * 1000.0);
                assert!(
                    actual
                        .iter()
                        .zip(&expected)
                        .all(|(a, b)| a.to_bits() == b.to_bits())
                );
            }
            times.sort_by(f64::total_cmp);
            eprintln!(
                "WAVEFORM_STEREO frames={frames}, columns={columns}, chunked={chunked}, median_ms={:.3}; five samples, warm generated PCM, allocation/validation/aggregation/normalization, equality outside timing",
                times[2]
            );
        }
    }
    Ok(())
}
