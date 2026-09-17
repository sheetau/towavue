use towavue_core::ReadingSettings;

const STEP: f64 = 24.0;
const START_DISTANCE: f64 = 6.0;
const AXIS_EMA: f64 = 0.35;
const SWITCH_RATIO: f64 = 1.5;
const SWITCH_DISTANCE: f64 = 8.0;

pub struct ReadingDrag {
    pub before: ReadingSettings,
    pub was_enabled: bool,
    pub settings: ReadingSettings,
    pub direction: Option<bool>,
    axis: Option<bool>,
    start: (f64, f64),
    motion: (f64, f64),
    residual: f64,
    switch_distance: f64,
    switch_sign: f64,
    full_first_page: bool,
    pixels_per_point: f64,
}

impl ReadingDrag {
    pub fn new(settings: ReadingSettings, was_enabled: bool, pixels_per_point: f64) -> Self {
        Self {
            before: settings,
            was_enabled,
            settings,
            direction: None,
            axis: None,
            start: (0.0, 0.0),
            motion: (0.0, 0.0),
            residual: 0.0,
            switch_distance: 0.0,
            switch_sign: 0.0,
            full_first_page: settings.first_page_count == settings.page_count,
            pixels_per_point,
        }
    }

    pub fn motion(&mut self, delta: (f64, f64)) {
        let delta = (
            delta.0 / self.pixels_per_point,
            delta.1 / self.pixels_per_point,
        );
        if !self.was_enabled {
            self.start.0 += delta.0;
            self.start.1 += delta.1;
            self.direction = (self.start.0.abs() >= START_DISTANCE
                && self.start.0.abs() >= self.start.1.abs())
            .then_some(self.start.0 < 0.0);
            self.settings.reversed = self.direction.unwrap_or(self.before.reversed);
            return;
        }
        self.motion.0 = self.motion.0 * (1.0 - AXIS_EMA) + delta.0 * AXIS_EMA;
        self.motion.1 = self.motion.1 * (1.0 - AXIS_EMA) + delta.1 * AXIS_EMA;
        let (vertical, distance) = if let Some(vertical) = self.axis {
            if self.switch_axis(vertical, delta) {
                return;
            }
            (vertical, if vertical { delta.1 } else { delta.0 })
        } else {
            self.start.0 += delta.0;
            self.start.1 += delta.1;
            if self.start.0.abs().max(self.start.1.abs()) < START_DISTANCE {
                return;
            }
            let vertical = self.start.1.abs() > self.start.0.abs();
            self.axis = Some(vertical);
            (vertical, if vertical { self.start.1 } else { self.start.0 })
        };
        self.residual += distance;
        let steps = (self.residual / STEP).trunc() as i32;
        if steps == 0 {
            return;
        }
        self.residual %= STEP;
        let before = self.settings;
        for _ in 0..steps.unsigned_abs().min(10) {
            let first = self.settings.first_page_count;
            match (vertical, steps > 0) {
                (true, true) => self.settings.decrease_pages(),
                (true, false) => self.settings.increase_pages(),
                (false, true) => self.settings.increase_first_page(),
                (false, false) => self.settings.decrease_first_page(),
            }
            if vertical {
                // Reaching a partial first spread's size does not turn it into
                // a linked full spread when the same gesture grows it again.
                self.settings.first_page_count = if self.full_first_page {
                    self.settings.page_count
                } else {
                    first.min(self.settings.page_count)
                };
            }
        }
        if self.settings == before {
            self.residual = 0.0;
        }
    }

    fn switch_axis(&mut self, vertical: bool, delta: (f64, f64)) -> bool {
        let (current, cross, current_motion, cross_motion) = if vertical {
            (delta.1, delta.0, self.motion.1, self.motion.0)
        } else {
            (delta.0, delta.1, self.motion.0, self.motion.1)
        };
        if cross.abs() <= current.abs() * SWITCH_RATIO {
            self.switch_distance = 0.0;
            self.switch_sign = 0.0;
            return false;
        }
        if cross.signum() != self.switch_sign {
            self.switch_sign = cross.signum();
            self.switch_distance = cross.abs();
        } else {
            self.switch_distance += cross.abs();
        }
        if self.switch_distance < SWITCH_DISTANCE
            || cross_motion.abs() <= current_motion.abs() * SWITCH_RATIO
        {
            return false;
        }
        self.axis = Some(!vertical);
        self.motion = delta;
        self.switch_distance = 0.0;
        self.switch_sign = 0.0;
        self.residual = 0.0;
        if !vertical {
            self.full_first_page = self.settings.first_page_count == self.settings.page_count;
        }
        // Switching chooses the next control, without applying this event or
        // residual movement from the previous control to the new value.
        true
    }
}

#[cfg(test)]
mod tests;
