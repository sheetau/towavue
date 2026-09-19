use super::{
    CachedUpdate, PUBLIC_KEY_HEX, UpdatePhase, UpdateStore,
    storage::{read_file, stage_name},
};
use crate::Cancellation;
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    os::windows::{fs::OpenOptionsExt, process::CommandExt},
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::FILETIME,
    System::{
        SystemInformation::GetSystemDirectoryW,
        Threading::{CREATE_NO_WINDOW, GetCurrentProcess, GetProcessTimes},
    },
};

const HELPER: &str = include_str!("handoff.cs");

/// A ready external helper, still waiting for this process to exit. Dropping it
/// cancels installation. Commit only after every window's edit/export guards
/// approve shutdown, then exit the primary process promptly.
pub struct PendingHandoff {
    child: Child,
    store: UpdateStore,
    selected: CachedUpdate,
    _script: File,
    committed: bool,
}

impl PendingHandoff {
    pub fn commit(mut self) -> io::Result<()> {
        if self.child.try_wait()?.is_some() {
            return Err(io::Error::other(
                "The update helper stopped before shutdown",
            ));
        }
        self.committed = true;
        Ok(())
    }
}

impl Drop for PendingHandoff {
    fn drop(&mut self) {
        if !self.committed {
            // Setup cannot have started while this parent process is alive.
            // Reap the helper before changing its claim to a retryable failure.
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = self
                .store
                .transition(&mut self.selected, UpdatePhase::Failed);
        }
    }
}

fn powershell() -> io::Result<std::path::PathBuf> {
    let mut buffer = [0u16; 32768];
    // SAFETY: Windows writes only the given live UTF-16 output buffer. Use the
    // system directory, never PATH, a user environment variable or Shell lookup.
    let count = unsafe { GetSystemDirectoryW(Some(&mut buffer)) } as usize;
    if count == 0 || count >= buffer.len() {
        return Err(io::Error::last_os_error());
    }
    use std::os::windows::ffi::OsStringExt;
    Ok(
        std::path::PathBuf::from(std::ffi::OsString::from_wide(&buffer[..count]))
            .join(r"WindowsPowerShell\v1.0\powershell.exe"),
    )
}

fn process_start() -> io::Result<u64> {
    let mut created = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: the borrowed current-process pseudo-handle is not closed. All
    // output FILETIMEs are valid for this synchronous query.
    unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    }
    .map_err(io::Error::other)?;
    Ok((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
}

fn script() -> String {
    let code = HELPER.replace("@PUBLIC_KEY_HEX@", PUBLIC_KEY_HEX.trim());
    format!(
        "param([string]$CacheRoot,[string]$Stage,[string]$Attempt,[string]$Installation,[int]$ParentId,[long]$ParentStart,[string]$InitialPath)\n\
         $ErrorActionPreference = 'Stop'\n\
         Add-Type -TypeDefinition @'\n{code}\n'@\n\
         exit [TowavueReleaseHandoff]::Run($CacheRoot,$Stage,$Attempt,$Installation,$ParentId,$ParentStart,$InitialPath)\n"
    )
}

impl UpdateStore {
    /// Blocking worker operation. Does not close windows. The helper verifies
    /// the current executable/production registration, signature and locked
    /// payload again before acknowledging readiness. An error never installs.
    pub fn start_handoff(
        &self,
        mut selected: CachedUpdate,
        installation: &Path,
        initial_path: Option<&Path>,
        cancel: &Cancellation,
    ) -> io::Result<PendingHandoff> {
        if cancel.is_cancelled() {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let attempt = stage_name()?;
        let directory = selected.directory().to_owned();
        let script_path = directory.join(format!("{attempt}.ps1"));
        let mut script_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .share_mode(1)
            .open(&script_path)?;
        let expected_script = script();
        script_file.write_all(expected_script.as_bytes())?;
        script_file.sync_all()?;
        // Opening a second read-only handle while retaining a write handle would
        // allow the script to change. Reopen with read sharing only before spawn.
        drop(script_file);
        let mut script_file = read_file(&script_path)?;
        let mut observed = String::new();
        (&mut script_file)
            .take(expected_script.len() as u64 + 1)
            .read_to_string(&mut observed)?;
        if observed != expected_script {
            return Err(io::Error::other("The update helper script changed"));
        }
        self.transition(&mut selected, UpdatePhase::Installing)?;
        let result = (|| {
            let executable = powershell()?;
            let mut command = Command::new(&executable);
            command
                .creation_flags(CREATE_NO_WINDOW.0)
                .args([
                    "-NoLogo",
                    "-NoProfile",
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                ])
                .arg(&script_path)
                .arg("-CacheRoot")
                .arg(self.root())
                .arg("-Stage")
                .arg(
                    directory
                        .file_name()
                        .ok_or_else(|| io::Error::other("Invalid update stage"))?,
                )
                .arg("-Attempt")
                .arg(&attempt)
                .arg("-Installation")
                .arg(installation)
                .arg("-ParentId")
                .arg(std::process::id().to_string())
                .arg("-ParentStart")
                .arg(process_start()?.to_string())
                .current_dir(executable.parent().expect("system executable directory"))
                .env_remove("PSModulePath")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            if let Some(path) = initial_path {
                command.arg("-InitialPath").arg(path);
            }
            command.spawn()
        })();
        let child = match result {
            Ok(child) => child,
            Err(error) => {
                let _ = self.transition(&mut selected, UpdatePhase::Failed);
                return Err(error);
            }
        };
        let mut pending = PendingHandoff {
            child,
            store: self.clone(),
            selected,
            _script: script_file,
            committed: false,
        };
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if cancel.is_cancelled() {
                return Err(io::ErrorKind::Interrupted.into());
            }
            if let Some(status) = pending.child.try_wait()? {
                let detail = read_file(&directory.join(format!("{attempt}.error")))
                    .and_then(|file| {
                        let mut text = String::new();
                        file.take(4096).read_to_string(&mut text)?;
                        Ok(text)
                    })
                    .unwrap_or_default();
                return Err(io::Error::other(format!(
                    "Update helper exited before readiness ({status}): {detail}"
                )));
            }
            match read_file(&directory.join(format!("{attempt}.ready"))) {
                Ok(file) => {
                    let mut bytes = Vec::new();
                    file.take(7).read_to_end(&mut bytes)?;
                    if bytes == b"ready\n" {
                        return Ok(pending);
                    }
                    return Err(io::Error::other("Invalid update helper acknowledgment"));
                }
                Err(error)
                    if error.kind() == io::ErrorKind::NotFound
                        || error.raw_os_error() == Some(32) => {}
                Err(error) => return Err(error),
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Update helper did not become ready",
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
