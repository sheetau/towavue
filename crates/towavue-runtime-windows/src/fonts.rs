use std::path::{Path, PathBuf};

/// Detached installed font bytes. No native font handle survives the read.
pub struct UiFontFallback {
    pub name: &'static str,
    pub data: Vec<u8>,
    pub index: u32,
}

/// Installed regular faces for Windows' recommended non-Latin UI families.
/// Missing optional language packs are skipped; nothing is installed or bundled.
pub fn ui_font_fallbacks() -> Vec<UiFontFallback> {
    let Some(directory) = std::env::var_os("WINDIR") else {
        return Vec::new();
    };
    read_fallbacks(&PathBuf::from(directory).join("Fonts"))
}

fn read_fallbacks(directory: &Path) -> Vec<UiFontFallback> {
    // https://learn.microsoft.com/windows/apps/design/signature-experiences/typography#fonts-for-non-latin-languages
    // Keep only regular faces. Nirmala ships as separate TTFs or a collection;
    // index zero of the collection is the regular UI face.
    [
        ("windows-segoe-ui", &[("segoeui.ttf", 0)][..]),
        ("windows-malgun", &[("malgun.ttf", 0)][..]),
        ("windows-yahei", &[("msyh.ttc", 1), ("msyh.ttf", 0)][..]),
        ("windows-jhenghei", &[("msjh.ttc", 1), ("msjh.ttf", 0)][..]),
        (
            "windows-nirmala",
            &[("Nirmala.ttf", 0), ("Nirmala.ttc", 0)][..],
        ),
        ("windows-leelawadee", &[("LeelawUI.ttf", 0)][..]),
        ("windows-myanmar", &[("mmrtext.ttf", 0)][..]),
        ("windows-ebrima", &[("ebrima.ttf", 0)][..]),
        ("windows-gadugi", &[("gadugi.ttf", 0)][..]),
        ("windows-himalaya", &[("himalaya.ttf", 0)][..]),
        ("windows-phagspa", &[("phagspa.ttf", 0)][..]),
    ]
    .into_iter()
    .filter_map(|(name, files)| {
        files.iter().find_map(|(file, index)| {
            std::fs::read(directory.join(file))
                .ok()
                .filter(|data| !data.is_empty())
                .map(|data| UiFontFallback {
                    name,
                    data,
                    index: *index,
                })
        })
    })
    .collect()
}

/// Reads one installed Japanese font; no font files are bundled or modified.
pub fn japanese_ui_font() -> Option<(Vec<u8>, u32)> {
    let directory = PathBuf::from(std::env::var_os("WINDIR")?).join("Fonts");
    [("YuGothM.ttc", 1), ("meiryo.ttc", 0), ("msgothic.ttc", 0)]
        .into_iter()
        .find_map(|(name, index)| {
            std::fs::read(directory.join(name))
                .ok()
                .map(|data| (data, index))
        })
}

/// Reads the installed Windows symbol face without copying or modifying it.
pub fn ui_symbol_font() -> Option<Vec<u8>> {
    let directory = PathBuf::from(std::env::var_os("WINDIR")?).join("Fonts");
    std::fs::read(directory.join("seguisym.ttf")).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_fallback_candidates_skip_missing_empty_and_unreadable_files() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "towavue-font-fixture-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&root).expect("owned font fixture");
        assert!(read_fallbacks(&root).is_empty());
        std::fs::write(root.join("malgun.ttf"), b"korean fixture").expect("fixture");
        std::fs::write(root.join("Nirmala.ttf"), []).expect("empty legacy candidate");
        std::fs::write(root.join("Nirmala.ttc"), b"collection fixture").expect("fixture");
        std::fs::create_dir(root.join("segoeui.ttf")).expect("unreadable file candidate");
        let fonts = read_fallbacks(&root);
        assert_eq!(
            fonts.iter().map(|font| font.name).collect::<Vec<_>>(),
            ["windows-malgun", "windows-nirmala"]
        );
        assert_eq!(fonts[1].data, b"collection fixture");
        assert_eq!(fonts[1].index, 0);
        std::fs::write(root.join("Nirmala.ttf"), b"regular fixture")
            .expect("legacy regular candidate");
        let fonts = read_fallbacks(&root);
        assert_eq!(fonts.len(), 2, "only one source per family");
        assert_eq!(fonts[1].data, b"regular fixture");
        std::fs::remove_file(root.join("malgun.ttf")).expect("remove fixture");
        std::fs::remove_file(root.join("Nirmala.ttf")).expect("remove fixture");
        std::fs::remove_file(root.join("Nirmala.ttc")).expect("remove fixture");
        std::fs::remove_dir(root.join("segoeui.ttf")).expect("remove fixture directory");
        std::fs::remove_dir(root).expect("remove owned root");
    }
}
