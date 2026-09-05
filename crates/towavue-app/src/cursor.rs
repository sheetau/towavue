use std::time::{Duration, Instant};

const IDLE_DELAY: Duration = Duration::from_secs(2);

#[derive(Default)]
pub struct ViewingCursor {
    pub hidden: bool,
    pub deadline: Option<Instant>,
}

impl ViewingCursor {
    pub fn activity(&mut self) {
        self.deadline = None;
        self.hidden = false;
    }

    pub fn update(&mut self, now: Instant, eligible: bool) {
        if !eligible {
            self.activity();
        } else if !self.hidden {
            let deadline = *self.deadline.get_or_insert(now + IDLE_DELAY);
            if now >= deadline {
                self.hidden = true;
                self.deadline = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_hides_once_and_activity_or_ineligibility_restarts_the_delay() {
        let now = Instant::now();
        let mut cursor = ViewingCursor::default();
        cursor.update(now, false);
        assert_eq!(cursor.deadline, None);
        cursor.update(now, true);
        assert_eq!(cursor.deadline, Some(now + IDLE_DELAY));
        cursor.update(now + IDLE_DELAY / 2, true);
        assert!(!cursor.hidden);
        cursor.update(now + IDLE_DELAY, true);
        assert!(cursor.hidden);
        assert_eq!(cursor.deadline, None);
        cursor.update(now + IDLE_DELAY * 2, true);
        assert!(cursor.hidden);
        assert_eq!(cursor.deadline, None);
        cursor.activity();
        assert!(!cursor.hidden);
        cursor.update(now + IDLE_DELAY * 3, true);
        assert_eq!(cursor.deadline, Some(now + IDLE_DELAY * 4));
        cursor.update(now + IDLE_DELAY * 4, false);
        assert!(!cursor.hidden);
        assert_eq!(cursor.deadline, None);
    }
}
