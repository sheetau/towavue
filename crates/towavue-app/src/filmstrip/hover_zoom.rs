use egui::{Context, Id};

#[derive(Clone, Copy, Default)]
struct Motion {
    value: f32,
    velocity: f32,
    time: f64,
}

impl Motion {
    fn advance(&mut self, target: f32, now: f64) {
        let dt = (now - self.time).clamp(0.0, 0.05) as f32;
        self.time = now;
        // An analytic critically damped spring keeps position and velocity when
        // hover reverses. It approaches both ends gently over roughly 0.8 s.
        let omega = 9.0;
        let delta = self.value - target;
        let slope = self.velocity + omega * delta;
        let decay = (-omega * dt).exp();
        self.value = target + (delta + slope * dt) * decay;
        self.velocity = (self.velocity - omega * slope * dt) * decay;
        if self.value < 0.0 || self.value > 1.0 {
            self.value = self.value.clamp(0.0, 1.0);
            self.velocity = 0.0;
        }
        if (self.value - target).abs() < 0.0001 && self.velocity.abs() < 0.001 {
            self.value = target;
            self.velocity = 0.0;
        }
    }
}

pub(super) fn value(context: &Context, id: Id, hovered: bool) -> f32 {
    let now = context.input(|input| input.time);
    let mut motion = context
        .data(|data| data.get_temp::<Motion>(id))
        .unwrap_or(Motion {
            time: now,
            ..Default::default()
        });
    let target = if hovered { 1.0 } else { 0.0 };
    motion.advance(target, now);
    context.data_mut(|data| data.insert_temp(id, motion));
    if motion.value != target || motion.velocity != 0.0 {
        context.request_repaint();
    }
    motion.value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reversals_keep_position_and_velocity_without_restarting() {
        let mut motion = Motion::default();
        motion.advance(1.0, 0.01);
        assert!(motion.value > 0.0 && motion.value < 0.01);
        for step in 2..20 {
            motion.advance(1.0, f64::from(step) * 0.01);
        }
        let before = motion;
        motion.advance(0.0, before.time);
        assert!((motion.value - before.value).abs() < 0.00001);
        assert!((motion.velocity - before.velocity).abs() < 0.00001);
        motion.advance(0.0, before.time + 0.01);
        assert!((motion.value - before.value).abs() < 0.05);
        for step in 1..200 {
            motion.advance(0.0, before.time + f64::from(step) * 0.01);
        }
        assert_eq!(motion.value, 0.0);
        assert_eq!(motion.velocity, 0.0);
        for step in 0..200 {
            motion.advance(1.0, 3.0 + f64::from(step) * 0.01);
        }
        assert_eq!(motion.value, 1.0);
    }
}
