//! Reuse native button geometry only while its native inputs remain unchanged.
use super::*;
use windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;

#[derive(Clone, Copy, PartialEq)]
struct Key {
    client: RECT,
    origin: POINT,
    dpi: u32,
    style: isize,
    extended_style: isize,
    class_style: usize,
    enabled: bool,
    visible: bool,
    minimized: bool,
    maximized: bool,
    commands: [u32; 4],
}

impl Key {
    fn read(handle: HWND) -> Option<Self> {
        let mut client = RECT::default();
        let mut origin = POINT::default();
        // SAFETY: the caller retains this same-thread HWND; only synchronous
        // scalar queries and stack outputs are used. No native borrow is saved.
        unsafe {
            GetClientRect(handle, &mut client).ok()?;
            ClientToScreen(handle, &mut origin)
                .as_bool()
                .then_some(())?;
            let menu = GetSystemMenu(handle, false);
            Some(Self {
                client,
                origin,
                dpi: GetDpiForWindow(handle),
                style: GetWindowLongPtrW(handle, GWL_STYLE),
                extended_style: GetWindowLongPtrW(handle, GWL_EXSTYLE),
                class_style: GetClassLongPtrW(handle, GCL_STYLE),
                enabled: IsWindowEnabled(handle).as_bool(),
                visible: IsWindowVisible(handle).as_bool(),
                minimized: IsIconic(handle).as_bool(),
                maximized: IsZoomed(handle).as_bool(),
                // EnableMenuItem need not deliver a geometry notification.
                // Observe all commands that can affect the exposed buttons.
                commands: [SC_MINIMIZE, SC_MAXIMIZE, SC_RESTORE, SC_CLOSE]
                    .map(|command| GetMenuState(menu, command, MF_BYCOMMAND)),
            })
        }
    }
}

#[derive(Clone, Copy)]
struct Saved {
    key: Key,
    info: TITLEBARINFOEX,
    origin: POINT,
}

#[derive(Default)]
pub(super) struct InfoCache {
    epoch: Cell<u64>,
    saved: Cell<Option<Saved>>,
    #[cfg(test)]
    queries: Cell<usize>,
}

impl InfoCache {
    pub(super) fn invalidate(&self) {
        self.epoch.set(self.epoch.get().wrapping_add(1));
        self.saved.set(None);
    }

    pub(super) fn read(&self, handle: HWND) -> Option<(TITLEBARINFOEX, POINT)> {
        #[cfg(feature = "presentation-verification")]
        {
            static DISABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
            if *DISABLED
                .get_or_init(|| std::env::var("TOWAVUE_VERIFY_CAPTION_CACHE").as_deref() == Ok("0"))
            {
                return titlebar_info(handle);
            }
        }
        let epoch = self.epoch.get();
        let key = Key::read(handle)?;
        if key.visible
            && !key.minimized
            && let Some(saved) = self.saved.get().filter(|saved| saved.key == key)
        {
            #[cfg(feature = "presentation-verification")]
            crate::record_burst(
                crate::BurstEvent::CaptionInfoCacheHit,
                0,
                None,
                [epoch, 0, 0],
            );
            return Some((saved.info, saved.origin));
        }
        #[cfg(test)]
        self.queries.set(self.queries.get() + 1);
        let (info, origin) = titlebar_info(handle)?;
        // Calls into the native procedure may dispatch other messages. Never
        // publish a result across a reentrant invalidation or native key change.
        // Hidden/minimized/empty snapshots cannot become a later visible cutout.
        if key.visible
            && !key.minimized
            && epoch == self.epoch.get()
            && Key::read(handle) == Some(key)
            && [2, 3, 5].into_iter().all(|index| {
                let rect = info.rgrect[index];
                info.rgstate[index] & 0x18000 == 0
                    && rect.right > rect.left
                    && rect.bottom > rect.top
            })
        {
            // Only the three complete, visible standard button geometries are
            // retained. Other layouts keep querying the original native path.
            // Callers consume geometry and unavailable/invisible flags, not
            // transient pressed state from the native snapshot.
            self.saved.set(Some(Saved { key, info, origin }));
        }
        Some((info, origin))
    }
}

pub(super) fn invalidates(message: u32) -> bool {
    matches!(
        message,
        WM_WINDOWPOSCHANGING
            | WM_WINDOWPOSCHANGED
            | WM_SIZE
            | WM_MOVE
            | WM_DPICHANGED
            | WM_STYLECHANGING
            | WM_STYLECHANGED
            | WM_SHOWWINDOW
            | WM_ENABLE
            | WM_THEMECHANGED
            | WM_SETTINGCHANGE
            | WM_DISPLAYCHANGE
            | WM_DWMCOMPOSITIONCHANGED
            | WM_DWMNCRENDERINGCHANGED
            | WM_ACTIVATE
    )
}

#[cfg(test)]
mod tests;
