use super::*;
use crate::localization::{Language, Settings, Text};
use towavue_runtime_windows::LanguagePreferences;

#[derive(Default)]
pub(super) struct State {
    pub settings: Settings,
    store: Option<LanguagePreferences>,
    origin: Option<WindowKey>,
    notice: Option<(WindowKey, String)>,
}

impl State {
    pub(super) fn open(proxy: Option<EventLoopProxy<Event>>) -> Self {
        let Some(root) = std::env::var_os("APPDATA").filter(|root| !root.is_empty()) else {
            return Self::default();
        };
        let path = PathBuf::from(root).join("towavue").join("language.conf");
        match LanguagePreferences::open(path, move |result| {
            if let Err(error) = &result {
                towavue_runtime_windows::diagnostic!("towavue: could not save language: {error}");
            }
            if let Some(proxy) = &proxy {
                let _ = proxy.send_event(Event::LanguageSaved(result));
            }
        }) {
            Ok(store) => Self {
                settings: Settings {
                    display: store.initial(),
                    next: store.initial(),
                    saving: false,
                },
                store: Some(store),
                ..Default::default()
            },
            Err(error) => {
                towavue_runtime_windows::diagnostic!("towavue: could not load language: {error}");
                Self::default()
            }
        }
    }
}

impl WindowHost {
    pub(super) fn select_language(&mut self, origin: WindowKey, language: Language) {
        if self.language.settings.saving
            || language == self.language.settings.next
            || self
                .windows
                .get(&origin)
                .is_none_or(|app| app.exit_requested || app.modal_input_blocked())
        {
            return;
        }
        let Some(store) = &self.language.store else {
            let app = self.windows.get_mut(&origin).expect("language owner");
            app.set_status(Text::LanguageUnavailable.in_language(app.language()).into());
            return;
        };
        if let Err(error) = store.remember(language) {
            let app = self.windows.get_mut(&origin).expect("language owner");
            app.set_status(towavue_core::localization::formatted::language_save_failed(
                app.language(),
                &error.to_string(),
            ));
            return;
        }
        self.language.origin = Some(origin);
        self.language.settings.saving = true;
        self.language.notice = None;
        self.broadcast_language_settings();
        let app = self.windows.get_mut(&origin).expect("language owner");
        app.set_status(Text::LanguageSaving.in_language(app.language()).into());
    }

    fn broadcast_language_settings(&mut self) {
        for app in self.windows.values_mut().filter(|app| !app.exit_requested) {
            app.language_settings = self.language.settings;
            app.request_redraw();
        }
    }

    pub(super) fn finish_language_save(&mut self, result: Result<Language, String>) {
        let Some(origin) = self.language.origin.take() else {
            return;
        };
        self.language.settings.saving = false;
        let display = self.language.settings.display;
        let message = match result {
            Ok(language) => {
                self.language.settings.next = language;
                let key = if language == display {
                    Text::LanguageUnchanged
                } else {
                    Text::LanguageRestart
                };
                let message = key.in_language(display).to_owned();
                self.language.notice = Some((origin, message.clone()));
                message
            }
            Err(error) => {
                towavue_core::localization::formatted::language_save_failed(display, &error)
            }
        };
        self.broadcast_language_settings();
        let target = self
            .windows
            .get(&origin)
            .filter(|app| !app.exit_requested)
            .map(|_| origin)
            .or_else(|| {
                self.windows
                    .iter()
                    .find_map(|(key, app)| (!app.exit_requested).then_some(*key))
            });
        if let Some(app) = target.and_then(|key| self.windows.get_mut(&key)) {
            app.set_status(message);
        }
        self.show_language_notice();
    }

    pub(super) fn show_language_notice(&mut self) {
        let Some((origin, _)) = &self.language.notice else {
            return;
        };
        let target = if self
            .windows
            .get(origin)
            .is_some_and(|app| !app.exit_requested)
        {
            Some(*origin)
        } else {
            self.windows
                .iter()
                .find_map(|(key, app)| (!app.exit_requested).then_some(*key))
        };
        let Some(app) = target.and_then(|key| self.windows.get_mut(&key)) else {
            return;
        };
        if app.exit_requested || app.modal_input_blocked() {
            return;
        }
        #[cfg(not(test))]
        if app.window.is_none() {
            return;
        }
        let (_, message) = self.language.notice.take().expect("queued language notice");
        app.set_status(message.clone());
        app.open_native_prompt(FallbackPrompt::LanguageNotice(message));
    }
}

#[cfg(test)]
mod tests;
