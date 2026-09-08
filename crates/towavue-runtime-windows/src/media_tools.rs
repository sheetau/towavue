use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};

pub(crate) fn tool_path(name: &str) -> io::Result<PathBuf> {
    let executable = std::env::current_exe()?;
    let directory = executable
        .parent()
        .ok_or_else(|| io::Error::other("application executable has no parent directory"))?;
    resolve_tool(name, directory, std::env::var_os("FFMPEG_DIR").as_deref())
}

fn resolve_tool(
    name: &str,
    application: &Path,
    development: Option<&OsStr>,
) -> io::Result<PathBuf> {
    // A partial bundle must not mix helpers with a different development build.
    let bundled = ["ffmpeg.exe", "ffprobe.exe"]
        .iter()
        .any(|name| application.join(name).exists());
    let directory = match development.filter(|value| !value.is_empty()) {
        Some(directory) if !bundled => std::path::absolute(Path::new(directory).join("bin"))?,
        _ => application.to_owned(),
    };
    let executable = directory.join(name);
    if !executable.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("required media helper is missing: {}", executable.display()),
        ));
    }
    Ok(executable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn development_paths_are_explicit_and_missing_helpers_report_the_expected_path() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("towavue-tool-path-{unique}"));
        let application = root.join("application");
        let development = root.join("development");
        std::fs::create_dir_all(&application).expect("application directory");
        std::fs::create_dir_all(development.join("bin")).expect("development directory");
        for name in ["ffmpeg.exe", "ffprobe.exe"] {
            std::fs::write(development.join("bin").join(name), []).expect("helper marker");
            assert_eq!(
                resolve_tool(name, &application, Some(development.as_os_str()))
                    .expect("development helper"),
                development.join("bin").join(name)
            );
            for empty in [None, Some(OsStr::new(""))] {
                let error =
                    resolve_tool(name, &application, empty).expect_err("no implicit PATH lookup");
                assert_eq!(error.kind(), io::ErrorKind::NotFound);
                assert!(
                    error
                        .to_string()
                        .contains(&application.join(name).display().to_string())
                );
            }
        }
        std::fs::create_dir(application.join("ffmpeg.exe")).expect("invalid bundled helper");
        for name in ["ffmpeg.exe", "ffprobe.exe"] {
            assert_eq!(
                resolve_tool(name, &application, Some(development.as_os_str()))
                    .expect_err("no mixed bundle")
                    .kind(),
                io::ErrorKind::NotFound
            );
        }
        std::fs::remove_dir_all(root).expect("remove helper markers");
    }
}
