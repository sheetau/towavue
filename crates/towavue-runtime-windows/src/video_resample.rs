use towavue_core::ResampleFilter;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FilterSpec {
    pub source: u32,
    pub target: u32,
    pub filter: ResampleFilter,
}

impl FilterSpec {
    pub fn taps(self) -> u32 {
        if self.source == self.target || self.filter == ResampleFilter::Nearest {
            return 1;
        }
        let diameter = match self.filter {
            ResampleFilter::Nearest => 1,
            ResampleFilter::Bilinear => 2,
            ResampleFilter::Bicubic => 4,
            ResampleFilter::Lanczos => 6,
        };
        (1 + (diameter * self.source).div_ceil(self.target).max(diameter))
            .min(self.source.saturating_sub(2))
            .max(1)
    }

    pub fn bytes(self) -> u64 {
        u64::from(self.taps()) * u64::from(self.target) * 8
    }

    pub fn coefficients(self) -> Vec<[f32; 2]> {
        let taps = self.taps();
        let mut result = Vec::with_capacity((self.target * taps) as usize);
        // Match the pinned scaler's 16-bit sample-center increment, not UI geometry.
        let step =
            ((u64::from(self.source) << 16) + u64::from(self.target / 2)) / u64::from(self.target);
        let shrink = (f64::from(self.source) / f64::from(self.target)).max(1.0);
        for pixel in 0..self.target {
            let center = (f64::from(pixel) + 0.5) * step as f64 / 65536.0 - 0.5;
            if taps == 1 {
                result.push([
                    (center + 0.5)
                        .floor()
                        .clamp(0.0, f64::from(self.source - 1)) as f32,
                    1.0,
                ]);
                continue;
            }
            let first = (center - (f64::from(taps) - 2.0) * 0.5).trunc() as i32;
            let offset = result.len();
            let mut sum = 0.0;
            for tap in 0..taps {
                let sample = first + tap as i32;
                let weight = kernel(self.filter, (f64::from(sample) - center).abs() / shrink);
                sum += weight;
                result.push([
                    sample.clamp(0, self.source as i32 - 1) as f32,
                    weight as f32,
                ]);
            }
            for entry in &mut result[offset..] {
                entry[1] /= sum as f32;
            }
        }
        result
    }
}

fn kernel(filter: ResampleFilter, distance: f64) -> f64 {
    match filter {
        ResampleFilter::Nearest => 1.0,
        ResampleFilter::Bilinear => (1.0 - distance).max(0.0),
        ResampleFilter::Bicubic => {
            // Keys cubic with B=0, C=0.6, matching the export scaler defaults.
            if distance < 1.0 {
                (1.4 * distance - 2.4) * distance * distance + 1.0
            } else if distance < 2.0 {
                ((-0.6 * distance + 3.0) * distance - 4.8) * distance + 2.4
            } else {
                0.0
            }
        }
        ResampleFilter::Lanczos => {
            if distance < 1e-12 {
                1.0
            } else if distance >= 3.0 {
                0.0
            } else {
                let x = distance * std::f64::consts::PI;
                x.sin() * (x / 3.0).sin() / (x * x / 3.0)
            }
        }
    }
}
