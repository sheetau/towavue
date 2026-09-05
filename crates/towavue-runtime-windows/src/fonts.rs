use std::path::PathBuf;

/// Reads one installed Japanese font; no font files are bundled or modified.
pub fn japanese_ui_font() -> Option<Vec<u8>> {
    let directory = PathBuf::from(std::env::var_os("WINDIR")?).join("Fonts");
    ["YuGothM.ttc", "meiryo.ttc", "msgothic.ttc"]
        .into_iter()
        .find_map(|name| std::fs::read(directory.join(name)).ok())
}
