//! Fixed project links only; user input can never become a Shell command.
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{PCWSTR, w};

#[derive(Clone, Copy, PartialEq)]
pub enum ProjectLink {
    Author,
    Repository,
}

impl ProjectLink {
    pub fn open(self) -> std::io::Result<()> {
        // A dedicated STA avoids changing the caller's apartment. Shell launch
        // finishes before COM teardown; no native handle crosses the boundary.
        std::thread::Builder::new()
            .name("project-link".into())
            .spawn(move || {
                // SAFETY: this fresh thread has no apartment; every successful
                // initialization is balanced on this same thread below.
                let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
                if initialized.is_err() {
                    return;
                }
                let url = match self {
                    Self::Author => w!("https://linktr.ee/sheetau"),
                    Self::Repository => w!("https://github.com/sheetau/towavue"),
                };
                // SAFETY: static, NUL-terminated HTTPS literals; no parameters,
                // working directory, borrowed window or returned owned handle.
                unsafe {
                    ShellExecuteW(
                        None,
                        w!("open"),
                        url,
                        PCWSTR::null(),
                        PCWSTR::null(),
                        SW_SHOWNORMAL,
                    );
                    CoUninitialize();
                }
            })?;
        Ok(())
    }
}
