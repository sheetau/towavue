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
            for _ in 0..16 {
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
