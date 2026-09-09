use std::path::PathBuf;

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
