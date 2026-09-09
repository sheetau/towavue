use towavue_core::ReadingSettings;

const STEP: f64 = 24.0;

pub struct ReadingDrag {
    pub before: ReadingSettings,
    pub was_enabled: bool,
    pub settings: ReadingSettings,
    axis: Option<bool>,
    pending: (f64, f64),
    pixels_per_point: f64,
}

impl ReadingDrag {
    pub fn new(settings: ReadingSettings, was_enabled: bool, pixels_per_point: f64) -> Self {
        Self {
            before: settings,
            was_enabled,
            settings,
            axis: None,
            pending: (0.0, 0.0),
            pixels_per_point,
        }
    }

    pub fn motion(&mut self, delta: (f64, f64)) {
        self.pending.0 += delta.0 / self.pixels_per_point;
        self.pending.1 += delta.1 / self.pixels_per_point;
        let vertical = *self
            .axis
            .get_or_insert_with(|| self.pending.1.abs() >= self.pending.0.abs());
        let distance = if vertical {
            &mut self.pending.1
        } else {
            &mut self.pending.0
        };
        let steps = (*distance / STEP).trunc() as i32;
        *distance %= STEP;
        for _ in 0..steps.unsigned_abs().min(10) {
            match (vertical, steps > 0) {
                (true, true) => self.settings.decrease_pages(),
                (true, false) => self.settings.increase_pages(),
                (false, true) => self.settings.increase_first_page(),
                (false, false) => self.settings.decrease_first_page(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drag_scales_accumulates_locks_axis_and_reverses_at_limits() {
        for density in [1.0, 1.25, 2.0] {
            let mut drag = ReadingDrag::new(ReadingSettings::default(), false, density);
            drag.motion((2.0 * density, -12.0 * density));
            assert_eq!(drag.settings.page_count, 2);
            drag.motion((100.0 * density, -12.0 * density));
            assert_eq!(
                (drag.settings.page_count, drag.settings.first_page_count),
                (3, 3)
            );
            drag.motion((0.0, -24_000.0 * density));
            assert_eq!(drag.settings.page_count, 10);
            drag.motion((0.0, 24.0 * density));
            assert_eq!(
                (drag.settings.page_count, drag.settings.first_page_count),
                (9, 9)
            );
            assert_eq!(drag.before, ReadingSettings::default());
            assert!(!drag.was_enabled);
            let mut drag = ReadingDrag::new(ReadingSettings::default(), true, density);
            drag.motion((-12.0 * density, 0.0));
            drag.motion((-12.0 * density, -100.0 * density));
            assert_eq!(
                (drag.settings.page_count, drag.settings.first_page_count),
                (2, 1)
            );
            drag.motion((-24_000.0 * density, 0.0));
            drag.motion((24.0 * density, 0.0));
            assert_eq!(drag.settings.first_page_count, 2);
            assert!(drag.was_enabled);
        }
    }
}
