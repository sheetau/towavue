use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

const SUPPRESSED: &[u8] = b"# towavue delete confirmation v1\nconfirm_delete = false\n";

pub(crate) fn path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|root| {
        PathBuf::from(root)
            .join("towavue")
            .join("delete-confirmation.conf")
    })
}

pub(crate) fn suppressed(path: &Path) -> bool {
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    let mut bytes = Vec::new();
    file.take(256).read_to_end(&mut bytes).is_ok() && bytes == SUPPRESSED
}

/// Only explicit, confirmed choices create this small opt-out marker. Partial,
/// malformed, unreadable or unknown versions always retain confirmation.
pub(crate) fn save_suppressed(path: &Path) -> io::Result<()> {
    if suppressed(path) {
        return Ok(());
    }
    fs::create_dir_all(
        path.parent()
            .ok_or_else(|| io::Error::other("preference folder unavailable"))?,
    )?;
    // Do not truncate an unknown file or overwrite an external preference edit.
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(SUPPRESSED)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opt_out_requires_a_complete_known_record_and_preserves_unknown_files() {
        let Some(root) = crate::tests::isolated_test_root(
            "file_operations::preferences::tests::opt_out_requires_a_complete_known_record_and_preserves_unknown_files",
        ) else {
            return;
        };
        let path = root.join("preference.conf");
        assert!(!suppressed(&path));
        save_suppressed(&path).expect("persist explicit opt-out");
        assert!(suppressed(&path));
        save_suppressed(&path).expect("idempotent");
        for bytes in [
            b"confirm_delete = false".as_slice(),
            b"unknown",
            &SUPPRESSED[..SUPPRESSED.len() - 1],
        ] {
            fs::write(&path, bytes).expect("owned malformed preference");
            assert!(!suppressed(&path));
            assert!(save_suppressed(&path).is_err());
            assert_eq!(fs::read(&path).expect("preserved"), bytes);
        }
    }
}
