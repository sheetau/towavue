//! Historical serial-per-pixel Paeth control; verification-only, not a second decoder.

use std::arch::x86_64::*;

// Paeth's nearest-neighbor choice, including a-before-b-before-c tie order.
// Each of the four low i16 lanes is one channel in 0..255. Intermediate values
// stay in -510..765. SSE2 is guaranteed by the Windows x64 target.
#[inline]
unsafe fn predict(a: __m128i, b: __m128i, c: __m128i) -> __m128i {
    unsafe {
        let threshold = _mm_sub_epi16(_mm_sub_epi16(_mm_add_epi16(c, _mm_add_epi16(c, c)), a), b);
        let lo = _mm_min_epi16(a, b);
        let hi = _mm_max_epi16(a, b);
        let hi_above = _mm_cmpgt_epi16(hi, threshold);
        let first = _mm_or_si128(_mm_and_si128(hi_above, c), _mm_andnot_si128(hi_above, lo));
        let threshold_above = _mm_cmpgt_epi16(threshold, lo);
        _mm_or_si128(
            _mm_and_si128(threshold_above, first),
            _mm_andnot_si128(threshold_above, hi),
        )
    }
}

pub(super) fn unfilter_rgba(previous: &[u8], current: &mut [u8]) {
    assert_eq!(previous.len(), current.len());
    assert!(current.len().is_multiple_of(4));
    // All memory accesses use bounded four-byte arrays; no pointer alignment,
    // padding, overreads or writes beyond the row are assumed by SIMD operations.
    unsafe {
        let zero = _mm_setzero_si128();
        let mask = _mm_set1_epi16(255);
        let (mut a, mut c) = (zero, zero);
        for (filtered, above) in current
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(previous.as_chunks::<4>().0.iter())
        {
            let b = _mm_unpacklo_epi8(_mm_cvtsi32_si128(i32::from_ne_bytes(*above)), zero);
            let value = _mm_unpacklo_epi8(_mm_cvtsi32_si128(i32::from_ne_bytes(*filtered)), zero);
            a = _mm_and_si128(_mm_add_epi16(value, predict(a, b, c)), mask);
            *filtered = _mm_cvtsi128_si32(_mm_packus_epi16(a, zero)).to_ne_bytes();
            c = b;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stages::paeth;

    fn rows(width: usize, count: usize, pattern: usize) -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut seed = 0x8394_1451u32;
        let mut byte = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as u8
        };
        (0..count)
            .map(|row| {
                let mut previous = vec![0; width * 4];
                let mut pixel = [0; 4];
                for (index, target) in previous.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                    if pattern == 2
                        || (pattern == 1 && index % 31 == 0)
                        || (pattern == 0 && index == 0 && row % 2 == 0)
                    {
                        pixel = [byte(), byte(), byte(), byte()];
                    }
                    *target = pixel;
                }
                (previous, (0..width * 4).map(|_| byte()).collect())
            })
            .collect()
    }

    #[test]
    fn repeated_pixels_match_scalar_recurrence_across_blocks_and_tails() {
        for width in [0, 1, 2, 3, 4, 5, 7, 8, 9, 17, 31, 127, 8706] {
            for pattern in 0..3 {
                for (previous, filtered) in rows(width, 16, pattern) {
                    let mut expected = filtered.clone();
                    unfilter_rgba(&previous, &mut expected);
                    let mut actual = filtered;
                    paeth::unfilter_rgba(&previous, &mut actual);
                    assert!(
                        actual == expected,
                        "width={width} pattern={pattern}; pixels not printed"
                    );
                }
            }
        }
    }

    #[test]
    #[ignore = "isolated generated row timing; use Release"]
    fn repeated_pixels_report_row_cost() {
        assert!(!cfg!(debug_assertions), "use Release");
        for pattern in 0..3 {
            let input = rows(8706, 512, pattern);
            let expected: Vec<_> = input
                .iter()
                .map(|(previous, filtered)| {
                    let mut result = filtered.clone();
                    unfilter_rgba(previous, &mut result);
                    result
                })
                .collect();
            for candidate in [false, true, true, false] {
                let mut samples = Vec::new();
                for _ in 0..7 {
                    let mut actual: Vec<_> =
                        input.iter().map(|(_, filtered)| filtered.clone()).collect();
                    let started = std::time::Instant::now();
                    for ((previous, _), result) in input.iter().zip(&mut actual) {
                        if candidate {
                            paeth::unfilter_rgba(previous, result);
                        } else {
                            unfilter_rgba(previous, result);
                        }
                    }
                    samples.push(started.elapsed());
                    assert!(
                        actual == expected,
                        "complete generated rows match; no pixels printed"
                    );
                }
                samples.sort_unstable();
                eprintln!(
                    "PAETH_FLAT pattern={pattern} candidate={candidate} width=8706 rows=512 median_ms={:.3}; generated flat/mixed/noisy preceding rows; reset/equality outside timing, not PNG decode or navigation",
                    samples[3].as_secs_f64() * 1000.0
                );
            }
        }
    }
}
