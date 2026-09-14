//! SSE2 Paeth reconstruction for four-byte PNG pixels, behind a safe row API.

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
    // Loads/stores are bounded unaligned 16-byte blocks or four-byte arrays.
    // SSE2 is available on x64; no new buffer, padding or CPU feature is required.
    unsafe {
        let zero = _mm_setzero_si128();
        let mask = _mm_set1_epi16(255);
        let (mut a, mut c) = (zero, zero);
        let mut offset = 0;
        while offset + 16 <= current.len() {
            let above = _mm_loadu_si128(previous.as_ptr().add(offset).cast());
            let repeated_c = _mm_shuffle_epi32(_mm_packus_epi16(c, zero), 0);
            if _mm_movemask_epi8(_mm_cmpeq_epi8(above, repeated_c)) == 0xffff {
                // b == c makes Paeth(a, b, c) == a, including all ties.
                // Four independent channel-prefix sums then reconstruct four pixels.
                let values = _mm_loadu_si128(current.as_ptr().add(offset).cast());
                let values = _mm_add_epi8(values, _mm_slli_si128(values, 4));
                let values = _mm_add_epi8(values, _mm_slli_si128(values, 8));
                let values = _mm_add_epi8(values, _mm_shuffle_epi32(_mm_packus_epi16(a, zero), 0));
                _mm_storeu_si128(current.as_mut_ptr().add(offset).cast(), values);
                a = _mm_unpacklo_epi8(_mm_srli_si128(values, 12), zero);
            } else {
                for index in (offset..offset + 16).step_by(4) {
                    let b = _mm_unpacklo_epi8(
                        _mm_cvtsi32_si128(i32::from_ne_bytes(
                            previous[index..index + 4]
                                .try_into()
                                .expect("four-byte pixel"),
                        )),
                        zero,
                    );
                    let value = _mm_unpacklo_epi8(
                        _mm_cvtsi32_si128(i32::from_ne_bytes(
                            current[index..index + 4]
                                .try_into()
                                .expect("four-byte pixel"),
                        )),
                        zero,
                    );
                    a = _mm_and_si128(_mm_add_epi16(value, predict(a, b, c)), mask);
                    current[index..index + 4].copy_from_slice(
                        &_mm_cvtsi128_si32(_mm_packus_epi16(a, zero)).to_ne_bytes(),
                    );
                    c = b;
                }
            }
            offset += 16;
        }
        for (filtered, above) in current[offset..]
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(previous[offset..].as_chunks::<4>().0.iter())
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

    #[test]
    fn predictor_matches_all_byte_triples_and_ties() {
        for a in 0i16..256 {
            for b in 0i16..256 {
                for c in (0i16..256).step_by(4) {
                    let mut result = [0i16; 8];
                    unsafe {
                        let predicted = predict(
                            _mm_set1_epi16(a),
                            _mm_set1_epi16(b),
                            _mm_setr_epi16(c, c + 1, c + 2, c + 3, 0, 0, 0, 0),
                        );
                        _mm_storeu_si128(result.as_mut_ptr().cast(), predicted);
                    }
                    for (lane, actual) in result[..4].iter().enumerate() {
                        let c = c + lane as i16;
                        let p = a + b - c;
                        let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
                        let expected = if pa <= pb && pa <= pc {
                            a
                        } else if pb <= pc {
                            b
                        } else {
                            c
                        };
                        assert_eq!(*actual, expected, "a={a} b={b} c={c}");
                    }
                }
            }
        }
    }

    #[test]
    fn rows_match_png_for_odd_widths_wrapping_and_prior_rows() {
        let mut seed = 0x894d_18b3u32;
        let mut byte = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as u8
        };
        for width in [1, 2, 3, 7, 8, 9, 17, 4096, 8706] {
            let mut previous: Vec<u8> = (0..width * 4).map(|_| byte()).collect();
            for case in 0..16 {
                // Exercise flat/mixed upper-row runs as well as the preceding
                // reconstructed row, including transitions to the serial tail.
                if case % 4 != 3 {
                    let run_pixels = [1, 31, width][case % 4];
                    for run in previous.chunks_mut(run_pixels * 4) {
                        let pixel: [u8; 4] = run[..4].try_into().expect("first pixel");
                        for target in run.as_chunks_mut::<4>().0 {
                            *target = pixel;
                        }
                    }
                }
                let mut actual: Vec<u8> = (0..width * 4).map(|_| byte()).collect();
                let mut expected = actual.clone();
                for i in 0..expected.len() {
                    let a = if i >= 4 {
                        i16::from(expected[i - 4])
                    } else {
                        0
                    };
                    let b = i16::from(previous[i]);
                    let c = if i >= 4 {
                        i16::from(previous[i - 4])
                    } else {
                        0
                    };
                    let p = a + b - c;
                    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
                    let predictor = if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    };
                    expected[i] = expected[i].wrapping_add(predictor as u8);
                }
                unfilter_rgba(&previous, &mut actual);
                assert!(actual == expected, "generated rows differ at width {width}");
                previous = actual;
            }
        }
    }
}
