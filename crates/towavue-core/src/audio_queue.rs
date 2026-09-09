use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

#[derive(Clone, Debug, Default)]
pub struct AudioQueue {
    repeat: RepeatMode,
    shuffled: bool,
    items: Vec<PathBuf>,
    order: Vec<PathBuf>,
    pending_shuffle: Option<(PathBuf, u64)>,
}

impl AudioQueue {
    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    pub fn cycle_repeat(&mut self) {
        self.repeat = match self.repeat {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        };
    }

    pub fn shuffled(&self) -> bool {
        self.shuffled
    }

    /// Shell order is authoritative; refresh preserves the existing shuffled cycle.
    pub fn set_items(&mut self, items: Vec<PathBuf>) {
        if self.items == items {
            return;
        }
        if self.shuffled {
            self.order.retain(|path| items.contains(path));
            for path in &items {
                if !self.order.contains(path) {
                    self.order.push(path.clone());
                }
            }
        } else {
            self.order.clone_from(&items);
        }
        self.items = items;
        if let Some((current, seed)) = self.pending_shuffle.take() {
            self.shuffle_order(&current, seed);
        }
    }

    /// The caller supplies non-cryptographic shuffle entropy; the current track stays first.
    pub fn toggle_shuffle(&mut self, current: &Path, seed: u64) {
        self.shuffled = !self.shuffled;
        self.order.clone_from(&self.items);
        self.pending_shuffle = None;
        if self.shuffled {
            if self.items.is_empty() {
                self.pending_shuffle = Some((current.to_owned(), seed));
            } else {
                self.shuffle_order(current, seed);
            }
        }
    }

    fn shuffle_order(&mut self, current: &Path, seed: u64) {
        let mut random = seed.max(1);
        for end in (1..self.order.len()).rev() {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            self.order.swap(end, (random % (end as u64 + 1)) as usize);
        }
        if let Some(index) = self.order.iter().position(|path| path == current) {
            self.order.rotate_left(index);
        }
    }

    /// Choosing a candidate does not consume it, so cancelling a dirty guard changes no order.
    pub fn next(&self, current: &Path, automatic: bool) -> Option<PathBuf> {
        if automatic && self.repeat == RepeatMode::One {
            return Some(current.to_owned());
        }
        let index = self.order.iter().position(|path| path == current)?;
        self.order
            .get(index + 1)
            .or_else(|| {
                (self.repeat == RepeatMode::All)
                    .then(|| self.order.first())
                    .flatten()
            })
            .cloned()
    }

    pub fn previous(&self, current: &Path) -> Option<PathBuf> {
        let index = self.order.iter().position(|path| path == current)?;
        index
            .checked_sub(1)
            .and_then(|index| self.order.get(index))
            .or_else(|| {
                (self.repeat == RepeatMode::All)
                    .then(|| self.order.last())
                    .flatten()
            })
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeat_modes_and_explicit_navigation_use_shell_order() {
        let paths: Vec<_> = ["z.wav", "a.wav", "m.wav"].map(PathBuf::from).into();
        let mut queue = AudioQueue::default();
        queue.set_items(paths.clone());
        assert_eq!(queue.next(&paths[0], true), Some(paths[1].clone()));
        assert_eq!(queue.next(&paths[2], true), None);
        assert_eq!(queue.previous(&paths[0]), None);
        queue.cycle_repeat();
        assert_eq!(queue.next(&paths[2], true), Some(paths[0].clone()));
        assert_eq!(queue.previous(&paths[0]), Some(paths[2].clone()));
        queue.cycle_repeat();
        assert_eq!(queue.next(&paths[1], true), Some(paths[1].clone()));
        assert_eq!(queue.next(&paths[1], false), Some(paths[2].clone()));
        queue.cycle_repeat();
        assert_eq!(queue.repeat(), RepeatMode::Off);
    }

    #[test]
    fn shuffle_has_no_duplicate_tracks_and_refresh_preserves_the_cycle() {
        let paths: Vec<_> = (0..40)
            .map(|index| PathBuf::from(format!("{index}.wav")))
            .collect();
        let mut queue = AudioQueue::default();
        queue.set_items(paths.clone());
        queue.toggle_shuffle(&paths[13], 12345);
        let mut visited = vec![paths[13].clone()];
        while let Some(next) = queue.next(visited.last().expect("current"), true) {
            assert!(!visited.contains(&next));
            assert_eq!(queue.previous(&next).as_ref(), visited.last());
            visited.push(next);
        }
        assert_eq!(visited.len(), paths.len());
        assert_ne!(visited, paths);
        queue.set_items(paths.clone());
        assert_eq!(queue.order, visited);
        let mut changed = paths.clone();
        changed.remove(4);
        changed.push("added.wav".into());
        queue.set_items(changed);
        let mut expected = visited;
        expected.retain(|path| path != &paths[4]);
        expected.push("added.wav".into());
        assert_eq!(queue.order, expected);
        queue.toggle_shuffle(&paths[13], 0);
        assert_eq!(queue.order, queue.items);
    }

    #[test]
    fn empty_missing_and_single_track_queues_are_bounded() {
        let path = Path::new("single.wav");
        let mut queue = AudioQueue::default();
        assert_eq!(queue.next(path, true), None);
        queue.set_items(vec![path.to_owned()]);
        queue.toggle_shuffle(path, 0);
        assert_eq!(queue.next(path, true), None);
        queue.cycle_repeat();
        assert_eq!(queue.next(path, true), Some(path.to_owned()));
        assert_eq!(queue.next(Path::new("missing.wav"), true), None);
    }

    #[test]
    fn shuffle_before_folder_completion_matches_shuffle_after_completion() {
        let paths: Vec<_> = (0..20)
            .map(|index| PathBuf::from(format!("{index}.wav")))
            .collect();
        let mut early = AudioQueue::default();
        early.toggle_shuffle(&paths[5], 42);
        early.set_items(paths.clone());
        let mut ready = AudioQueue::default();
        ready.set_items(paths.clone());
        ready.toggle_shuffle(&paths[5], 42);
        assert_eq!(early.order, ready.order);
        assert_eq!(early.order.first(), Some(&paths[5]));
    }
}
