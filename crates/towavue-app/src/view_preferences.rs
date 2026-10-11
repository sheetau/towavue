use super::*;
use towavue_runtime_windows::{ViewPreference as Key, ViewPreferences};

pub(super) fn open(
    proxy: Option<EventLoopProxy<window_host::Event>>,
) -> Option<Arc<ViewPreferences>> {
    // Tests inject an owned store; never access the owner's settings.
    if cfg!(test) {
        return None;
    }
    let path = PathBuf::from(std::env::var_os("APPDATA")?)
        .join("towavue")
        .join("viewing.conf");
    let report = move |error| {
        towavue_runtime_windows::diagnostic!("Could not restore or save settings: {error}");
        if let Some(proxy) = &proxy {
            let _ = proxy.send_event(window_host::Event::ViewPreferenceFailed(error));
        }
    };
    match ViewPreferences::open(path, report.clone()) {
        Ok(store) => Some(Arc::new(store)),
        Err(error) => {
            report(error);
            None
        }
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn restore_view_preferences(&mut self, store: Option<Arc<ViewPreferences>>) {
        let Some(store) = store else { return };
        self.view_resize_mode = match store.value(Key::Resize) {
            1 => view_resize::Mode::KeepArea,
            2 => view_resize::Mode::KeepScale,
            _ => view_resize::Mode::Fit,
        };
        self.volume_step_percent = store.value(Key::VolumeStep) as u8;
        self.nearest_images = store.value(Key::Nearest) != 0;
        self.high_quality_minification = store.value(Key::Minification) != 0;
        self.folder_navigation_loop = store.value(Key::FolderLoop) != 0;
        self.tab_icons = store.value(Key::TabIcons) != 0;
        self.reuse_folder_tab = store.value(Key::ReuseFolderTab) != 0;
        let pages = store.value(Key::ReadingPages);
        self.reading_settings = ReadingSettings {
            page_count: usize::from(pages / 100),
            first_page_count: usize::from(pages % 100),
            axis: if store.value(Key::ReadingAxis) == 0 {
                ReadingAxis::Horizontal
            } else {
                ReadingAxis::Vertical
            },
            reversed: store.value(Key::ReadingReversed) != 0,
        };
        *self
            .video_export_quality
            .lock()
            .expect("video export quality") = match store.value(Key::VideoQuality) {
            1 => towavue_runtime_windows::VideoExportQuality::Balanced,
            2 => towavue_runtime_windows::VideoExportQuality::Smaller,
            _ => towavue_runtime_windows::VideoExportQuality::High,
        };
        self.prefer_hardware_encode = store.value(Key::HardwareEncode) != 0;
        self.view_preferences = Some(store);
    }

    // Only accepted preference commands reach this hook. Activation, restored
    // tab snapshots, close, and transient per-document modes never write defaults.
    pub(super) fn remember_view_choice(&mut self, command: CommandId) {
        use CommandId::*;
        let (key, value) = match command {
            ResizeViewFit | ResizeViewKeepArea | ResizeViewKeepScale => (
                Key::Resize,
                match self.view_resize_mode {
                    view_resize::Mode::Fit => 0,
                    view_resize::Mode::KeepArea => 1,
                    view_resize::Mode::KeepScale => 2,
                },
            ),
            VolumeStepTwo | VolumeStepFive | VolumeStepTen | CycleVolumeStep => {
                (Key::VolumeStep, u16::from(self.volume_step_percent))
            }
            ToggleTabIcons => (Key::TabIcons, u16::from(self.tab_icons)),
            ToggleReuseFolderTab => (Key::ReuseFolderTab, u16::from(self.reuse_folder_tab)),
            ToggleImageInterpolation => (Key::Nearest, u16::from(self.nearest_images)),
            ToggleImageMinification => {
                (Key::Minification, u16::from(self.high_quality_minification))
            }
            FolderNavigationStop | FolderNavigationLoop => {
                (Key::FolderLoop, u16::from(self.folder_navigation_loop))
            }
            IncreaseReadingPages
            | DecreaseReadingPages
            | IncreaseReadingFirstPage
            | DecreaseReadingFirstPage => (
                Key::ReadingPages,
                (self.reading_settings.page_count * 100 + self.reading_settings.first_page_count)
                    as u16,
            ),
            ToggleReadingAxis => (
                Key::ReadingAxis,
                u16::from(self.reading_settings.axis == ReadingAxis::Vertical),
            ),
            ReverseReadingOrder => (
                Key::ReadingReversed,
                u16::from(self.reading_settings.reversed),
            ),
            ExportQualityHigh | ExportQualityBalanced | ExportQualitySmaller => (
                Key::VideoQuality,
                match self.video_export_quality() {
                    towavue_runtime_windows::VideoExportQuality::High => 0,
                    towavue_runtime_windows::VideoExportQuality::Balanced => 1,
                    towavue_runtime_windows::VideoExportQuality::Smaller => 2,
                },
            ),
            ToggleHardwareEncode => (Key::HardwareEncode, u16::from(self.prefer_hardware_encode)),
            _ => return,
        };
        if let Some(store) = &self.view_preferences
            && let Err(error) = store.remember(key, value)
        {
            self.set_status(
                towavue_core::localization::formatted::view_preference_failed(
                    self.language(),
                    &towavue_runtime_windows::io_error_message(&error, self.language()),
                ),
            );
        }
    }
}
