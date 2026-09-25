use super::*;
use towavue_runtime_windows::WindowPlacementPreferences;

pub(super) fn open() -> Option<WindowPlacementPreferences> {
    // Native/app fixtures inject a store explicitly; never read or write the
    // owner's placement, including when a fixture briefly displays an HWND.
    if cfg!(test) {
        return None;
    }
    let path = PathBuf::from(std::env::var_os("APPDATA")?)
        .join("towavue")
        .join("window-placement.conf");
    match WindowPlacementPreferences::open(path) {
        Ok(store) => Some(store),
        Err(error) => {
            towavue_runtime_windows::diagnostic!("Could not load window placement: {error}");
            None
        }
    }
}

impl WindowHost {
    pub(super) fn restore_initial_placement(&mut self, key: WindowKey) {
        self.windows
            .get_mut(&key)
            .expect("initial window")
            .initial_window_placement = self
            .window_placement
            .as_ref()
            .and_then(|store| store.initial());
    }

    pub(super) fn remember_closed_placement(&mut self) {
        let Some(store) = &mut self.window_placement else {
            return;
        };
        // For one all-window close, prefer the last focused accepted window.
        // For ordinary separate closes, each accepted visible owner supersedes
        // the preceding one. Hidden transfer/startup failures never become saved.
        let placement = self
            .windows
            .iter()
            .filter(|(_, app)| {
                app.exit_requested
                    && app
                        .window
                        .as_ref()
                        .is_some_and(|window| window.is_visible() == Some(true))
            })
            .filter_map(|(key, app)| Some((*key, app.native_caption.as_ref()?.saved_placement()?)))
            .max_by_key(|(key, _)| (Some(*key) == self.last_active_window, *key))
            .map(|(_, placement)| placement);
        if let Some(placement) = placement {
            store.remember(placement);
        }
    }
}

#[cfg(test)]
mod tests;
