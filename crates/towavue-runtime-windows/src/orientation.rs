use towavue_core::UnitPoint;

use crate::DecodeError;

/// Display-order corners referring to the unchanged decoded video surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VideoOrientation {
    corners: [usize; 4],
}

impl Default for VideoOrientation {
    fn default() -> Self {
        Self {
            corners: [0, 1, 2, 3],
        }
    }
}

impl VideoOrientation {
    pub fn source_uv(self) -> [UnitPoint; 4] {
        let points = [
            UnitPoint { x: 0.0, y: 0.0 },
            UnitPoint { x: 1.0, y: 0.0 },
            UnitPoint { x: 1.0, y: 1.0 },
            UnitPoint { x: 0.0, y: 1.0 },
        ];
        self.corners.map(|index| points[index])
    }

    pub fn swaps_axes(self) -> bool {
        let uv = self.source_uv();
        uv[0].x == uv[1].x
    }

    pub(crate) fn from_bytes(bytes: Option<&[u8]>) -> Result<Self, DecodeError> {
        let Some(bytes) = bytes else {
            return Ok(Self::default());
        };
        if bytes.len() != 36 {
            return Err(DecodeError::UnsupportedOrientation);
        }
        let mut matrix = [0_i32; 9];
        for (value, bytes) in matrix.iter_mut().zip(bytes.as_chunks::<4>().0) {
            *value = i32::from_ne_bytes(*bytes);
        }
        if matrix[2] != 0 || matrix[5] != 0 || matrix[8] != 1 << 30 {
            return Err(DecodeError::UnsupportedOrientation);
        }
        let mut linear = [0; 4];
        for (value, entry) in linear
            .iter_mut()
            .zip([matrix[0], matrix[1], matrix[3], matrix[4]])
        {
            *value = match entry {
                -65537..=-65535 => -1,
                -1..=1 => 0,
                65535..=65537 => 1,
                _ => return Err(DecodeError::UnsupportedOrientation),
            };
        }
        let [a, b, c, d] = linear;
        if a * a + b * b != 1 || c * c + d * d != 1 || a * c + b * d != 0 {
            return Err(DecodeError::UnsupportedOrientation);
        }
        // Normalize the transformed bounding box; container translation does not
        // affect an aspect-fitted image. No native pointer or matrix is retained.
        let mapped = [(0, 0), (1, 0), (1, 1), (0, 1)].map(|(x, y)| (a * x + c * y, b * x + d * y));
        let min_x = mapped
            .iter()
            .map(|point| point.0)
            .min()
            .expect("four corners");
        let min_y = mapped
            .iter()
            .map(|point| point.1)
            .min()
            .expect("four corners");
        let mut corners = [0; 4];
        for (source, (x, y)) in mapped.into_iter().enumerate() {
            let destination = match (x - min_x, y - min_y) {
                (0, 0) => 0,
                (1, 0) => 1,
                (1, 1) => 2,
                (0, 1) => 3,
                _ => unreachable!("orthogonal unit matrix"),
            };
            corners[destination] = source;
        }
        Ok(Self { corners })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(matrix: [i32; 9]) -> Result<VideoOrientation, DecodeError> {
        VideoOrientation::from_bytes(Some(
            &matrix
                .into_iter()
                .flat_map(i32::to_ne_bytes)
                .collect::<Vec<_>>(),
        ))
    }

    #[test]
    fn display_matrix_preserves_quarter_turns_reflections_and_translation() {
        for (linear, corners) in [
            ([1, 0, 0, 1], [0, 1, 2, 3]),
            ([0, -1, 1, 0], [1, 2, 3, 0]),
            ([-1, 0, 0, -1], [2, 3, 0, 1]),
            ([0, 1, -1, 0], [3, 0, 1, 2]),
            ([-1, 0, 0, 1], [1, 0, 3, 2]),
            ([1, 0, 0, -1], [3, 2, 1, 0]),
            ([0, 1, 1, 0], [0, 3, 2, 1]),
            ([0, -1, -1, 0], [2, 1, 0, 3]),
        ] {
            let [a, b, c, d] = linear.map(|value| value * 65536);
            let orientation = parse([a, b, 0, c, d, 0, 640 * 65536, -360 * 65536, 1 << 30])
                .expect("orthogonal matrix");
            assert_eq!(orientation.corners, corners);
            assert_eq!(orientation.swaps_axes(), a == 0);
        }
        assert_eq!(
            VideoOrientation::from_bytes(None).expect("absent matrix"),
            VideoOrientation::default()
        );
    }

    #[test]
    fn malformed_non_orthogonal_and_scaled_matrices_are_explicit_errors() {
        assert!(VideoOrientation::from_bytes(Some(&[0; 35])).is_err());
        for matrix in [
            [0; 9],
            [46341, -46341, 0, 46341, 46341, 0, 0, 0, 1 << 30],
            [131072, 0, 0, 0, 65536, 0, 0, 0, 1 << 30],
            [65536, 1 << 15, 0, 0, 65536, 0, 0, 0, 1 << 30],
            [65536, 0, 1, 0, 65536, 0, 0, 0, 1 << 30],
        ] {
            assert!(matches!(
                parse(matrix),
                Err(DecodeError::UnsupportedOrientation)
            ));
        }
    }
}
