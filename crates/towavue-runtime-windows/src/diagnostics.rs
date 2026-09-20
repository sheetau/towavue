//! Best-effort, bounded session diagnostics. Producers never wait for disk I/O.
use std::{
    fmt::{self, Write as _},
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::windows::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{OnceLock, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows::Win32::{
    System::Com::CoTaskMemFree,
    UI::Shell::{FOLDERID_LocalAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath},
};

const MAX_MESSAGE: usize = 4096;
const MAX_FILE: usize = 1024 * 1024;
const RETAIN_SESSIONS: usize = 5;
static SINK: OnceLock<mpsc::SyncSender<Message>> = OnceLock::new();

enum Message {
    Line(String),
    Stop,
}

/// Keep alive until normal shutdown so queued diagnostics are drained.
pub struct Diagnostics {
    sender: mpsc::SyncSender<Message>,
    worker: Option<JoinHandle<()>>,
}

impl Drop for Diagnostics {
    fn drop(&mut self) {
        let _ = self.sender.send(Message::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Initialize once at process entry, before starting any codec workers.
/// Failure is nonfatal: diagnostic storage must not prevent viewing media.
pub fn start_diagnostics() -> io::Result<Diagnostics> {
    // SAFETY: Shell returns a task-allocated NUL-terminated string. Copy it and
    // release the allocation on both conversion outcomes; no pointer escapes.
    let folder = unsafe { SHGetKnownFolderPath(&FOLDERID_LocalAppData, KF_FLAG_DEFAULT, None) }
        .map_err(io::Error::other)?;
    let local = unsafe { folder.to_string() }.map_err(io::Error::other);
    unsafe { CoTaskMemFree(Some(folder.0.cast())) };
    let guard = start_at(&PathBuf::from(local?).join("towavue").join("logs"))?;
    SINK.set(guard.sender.clone())
        .map_err(|_| io::Error::other("Diagnostics already initialized"))?;
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        record_diagnostic(format_args!("panic: {info}"));
        previous(info);
    }));
    // SAFETY: installed before codec work, process lifetime callback, no state
    // from FFmpeg is retained. The callback is thread safe and cannot unwind.
    unsafe {
        ffmpeg_next::ffi::av_log_set_level(ffmpeg_next::ffi::AV_LOG_ERROR);
        ffmpeg_next::ffi::av_log_set_callback(Some(ffmpeg_error));
    }
    Ok(guard)
}

fn start_at(directory: &Path) -> io::Result<Diagnostics> {
    fs::create_dir_all(directory)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = directory.join(format!("session-{stamp:020}-{}.log", std::process::id()));
    // Only readers may share an active session: retention cannot delete it or
    // another process append to it. File ownership stays on the writer thread.
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .share_mode(1)
        .open(&path)?;
    prune(directory);
    let (sender, receiver) = mpsc::sync_channel(128);
    let worker = thread::Builder::new()
        .name("towavue-diagnostics".into())
        .spawn(move || write_session(file, receiver))?;
    Ok(Diagnostics {
        sender,
        worker: Some(worker),
    })
}

fn prune(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut sessions: Vec<_> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            let identity = name.strip_prefix("session-")?.strip_suffix(".log")?;
            let (stamp, pid) = identity.split_once('-')?;
            if stamp.len() < 20
                || stamp.parse::<u128>().is_err()
                || pid.parse::<u32>().is_err()
                || !entry.file_type().ok()?.is_file()
            {
                return None;
            }
            Some((name.to_owned(), entry.path()))
        })
        .collect();
    sessions.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    for (_, path) in sessions.into_iter().skip(RETAIN_SESSIONS) {
        let _ = fs::remove_file(path);
    }
}

struct BoundedText(String);
impl fmt::Write for BoundedText {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let remaining = MAX_MESSAGE.saturating_sub(self.0.len());
        let mut end = remaining.min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        // One event per line; discard control sequences from native errors.
        self.0.extend(
            text[..end]
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c }),
        );
        Ok(())
    }
}

pub fn record_diagnostic(arguments: fmt::Arguments<'_>) {
    if let Some(sender) = SINK.get() {
        let mut text = BoundedText(String::new());
        let _ = text.write_fmt(arguments);
        let _ = sender.try_send(Message::Line(text.0));
    }
    #[cfg(debug_assertions)]
    eprintln!("{arguments}");
}

/// Record a recoverable warning or error, never per-frame progress or timing.
#[macro_export]
macro_rules! diagnostic {
    ($($arg:tt)*) => { $crate::record_diagnostic(format_args!($($arg)*)) };
}

fn write_session(mut file: File, receiver: mpsc::Receiver<Message>) {
    write_bounded_session(&mut file, receiver, MAX_FILE);
}

fn write_bounded_session(file: &mut File, receiver: mpsc::Receiver<Message>, limit: usize) {
    let started = Instant::now();
    let header = format!(
        "towavue {} pid={}\n",
        env!("CARGO_PKG_VERSION"),
        std::process::id()
    );
    let _ = file.write_all(header.as_bytes());
    let mut bytes = header.len();
    let mut recent: Vec<(String, Instant)> = Vec::new();
    let mut period = Instant::now();
    let mut count = 0;
    let mut suppressed = 0u64;
    while let Ok(Message::Line(text)) = receiver.recv() {
        let now = Instant::now();
        if now.duration_since(period) >= Duration::from_secs(1) {
            period = now;
            count = 0;
        }
        recent.retain(|(_, time)| now.duration_since(*time) < Duration::from_secs(10));
        if count >= 32 || recent.iter().any(|(previous, _)| previous == &text) {
            suppressed = suppressed.saturating_add(1);
            continue;
        }
        if recent.len() == 64 {
            recent.remove(0);
        }
        recent.push((text.clone(), now));
        count += 1;
        let line = format!(
            "{:.3}s suppressed={suppressed} {text}\n",
            started.elapsed().as_secs_f64()
        );
        // Reserve room for one final marker, then stop writing for this session.
        if bytes + line.len() + 64 > limit {
            let _ = file.write_all(b"Session log limit reached; further diagnostics discarded.\n");
            break;
        }
        if file.write_all(line.as_bytes()).is_err() {
            break;
        }
        bytes += line.len();
        suppressed = 0;
    }
    let _ = file.flush();
}

unsafe extern "C" fn ffmpeg_error(
    context: *mut std::ffi::c_void,
    level: i32,
    format: *const std::ffi::c_char,
    arguments: ffmpeg_next::ffi::va_list,
) {
    if level > ffmpeg_next::ffi::AV_LOG_ERROR || format.is_null() {
        return;
    }
    let _ = std::panic::catch_unwind(|| {
        let mut buffer = [0i8; 2048];
        let mut prefix = 1;
        // SAFETY: FFmpeg supplies context/format/va_list for this callback only.
        // Formatting is synchronous, bounded and NUL-terminated; each call has
        // its own buffer/prefix, including concurrently running codec threads.
        let result = unsafe {
            ffmpeg_next::ffi::av_log_format_line2(
                context,
                level,
                format,
                arguments,
                buffer.as_mut_ptr(),
                buffer.len() as i32,
                &mut prefix,
            )
        };
        if result >= 0 {
            let text = unsafe { std::ffi::CStr::from_ptr(buffer.as_ptr()) }.to_string_lossy();
            record_diagnostic(format_args!("FFmpeg: {}", text.trim()));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_codec_errors_reach_the_session_without_info_chatter() {
        const CHILD: &str = "TOWAVUE_DIAGNOSTIC_TEST_ROOT";
        if let Some(root) = std::env::var_os(CHILD) {
            let guard = start_at(Path::new(&root)).expect("diagnostic fixture operation");
            assert!(SINK.set(guard.sender.clone()).is_ok());
            // SAFETY: this isolated child owns the global callback and starts
            // no other codec users. The callback retains none of its arguments.
            unsafe {
                ffmpeg_next::ffi::av_log_set_callback(Some(ffmpeg_error));
                ffmpeg_next::ffi::av_log_set_level(ffmpeg_next::ffi::AV_LOG_ERROR);
            }
            ffmpeg_next::init().expect("diagnostic fixture operation");
            thread::scope(|scope| {
                for _ in 0..4 {
                    scope.spawn(|| unsafe {
                        ffmpeg_next::ffi::av_log(
                            std::ptr::null_mut(),
                            ffmpeg_next::ffi::AV_LOG_INFO,
                            c"frame progress %d".as_ptr(),
                            42i32,
                        );
                        ffmpeg_next::ffi::av_log(
                            std::ptr::null_mut(),
                            ffmpeg_next::ffi::AV_LOG_ERROR,
                            c"codec failure %d\n".as_ptr(),
                            7i32,
                        );
                    });
                }
            });
            drop(guard);
            return;
        }
        let root = std::env::temp_dir().join(format!(
            "towavue-native-log-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("diagnostic fixture operation")
                .as_nanos()
        ));
        let output = std::process::Command::new(
            std::env::current_exe().expect("diagnostic fixture operation"),
        )
        .args([
            "--exact",
            "diagnostics::tests::native_codec_errors_reach_the_session_without_info_chatter",
            "--test-threads=1",
        ])
        .env(CHILD, &root)
        .output()
        .expect("diagnostic fixture operation");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let paths: Vec<_> = fs::read_dir(&root)
            .expect("diagnostic fixture operation")
            .flatten()
            .collect();
        assert_eq!(paths.len(), 1);
        let text = fs::read_to_string(paths[0].path()).expect("diagnostic fixture operation");
        assert_eq!(text.matches("codec failure 7").count(), 1);
        assert!(!text.contains("frame progress"));
        fs::remove_dir_all(root).expect("diagnostic fixture operation");
    }

    #[test]
    fn messages_are_bounded_utf8_and_single_line() {
        let mut text = BoundedText(String::new());
        write!(&mut text, "error\n{}", "画".repeat(5000)).expect("diagnostic fixture operation");
        assert!(text.0.len() <= MAX_MESSAGE);
        assert!(!text.0.contains('\n'));
        assert!(text.0.starts_with("error 画"));
    }

    #[test]
    fn saturated_queue_never_waits_and_file_stops_at_its_limit() {
        let (sender, receiver) = mpsc::sync_channel(128);
        for index in 0..128 {
            sender
                .try_send(Message::Line(format!("{index}: {}", "x".repeat(100))))
                .expect("diagnostic fixture operation");
        }
        assert!(matches!(
            sender.try_send(Message::Line("overflow".into())),
            Err(mpsc::TrySendError::Full(_))
        ));
        let path =
            std::env::temp_dir().join(format!("towavue-log-limit-{}.log", std::process::id()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .expect("diagnostic fixture operation");
        write_bounded_session(&mut file, receiver, 1024);
        drop(file);
        let log = fs::read(&path).expect("diagnostic fixture operation");
        assert!(log.len() <= 1024);
        assert!(
            String::from_utf8(log)
                .expect("diagnostic fixture operation")
                .ends_with("Session log limit reached; further diagnostics discarded.\n")
        );
        fs::remove_file(path).expect("diagnostic fixture operation");
    }

    #[test]
    fn sessions_drain_coalesce_and_prune_only_owned_closed_files() {
        let root = std::env::temp_dir().join(format!(
            "towavue-diagnostics-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("diagnostic fixture operation")
                .as_nanos()
        ));
        fs::create_dir(&root).expect("diagnostic fixture operation");
        fs::write(root.join("owner.log"), "preserve").expect("diagnostic fixture operation");
        let active = start_at(&root).expect("diagnostic fixture operation");
        for _ in 0..8 {
            drop(start_at(&root).expect("diagnostic fixture operation"));
        }
        for _ in 0..100 {
            active
                .sender
                .send(Message::Line("repeated failure".into()))
                .expect("diagnostic fixture operation");
        }
        active
            .sender
            .send(Message::Line("final distinct failure".into()))
            .expect("diagnostic fixture operation");
        drop(active);
        let content: String = fs::read_dir(&root)
            .expect("diagnostic fixture operation")
            .flatten()
            .filter(|entry| entry.file_name() != "owner.log")
            .map(|entry| fs::read_to_string(entry.path()).expect("diagnostic fixture operation"))
            .collect();
        assert_eq!(content.matches("repeated failure").count(), 1);
        assert!(content.contains("suppressed=99 final distinct failure"));
        drop(start_at(&root).expect("diagnostic fixture operation"));
        assert_eq!(
            fs::read_dir(&root)
                .expect("diagnostic fixture operation")
                .count(),
            RETAIN_SESSIONS + 1
        );
        assert_eq!(
            fs::read_to_string(root.join("owner.log")).expect("diagnostic fixture operation"),
            "preserve"
        );
        fs::remove_dir_all(root).expect("diagnostic fixture operation");
    }
}
