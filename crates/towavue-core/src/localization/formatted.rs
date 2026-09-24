//! Typed templates keep interpolation checked for every supported language.
use super::Language;

macro_rules! templates {
    ($($name:ident($($arg:ident: $ty:ty),*) => ($english:literal, $japanese:literal),)*) => {
        $(pub fn $name(language: Language, $($arg: $ty),*) -> String {
            match language {
                Language::English => format!($english, $($arg = $arg),*),
                Language::Japanese => format!($japanese, $($arg = $arg),*),
            }
        })*
    };
}

templates! {
    language_save_failed(error: &str) => ("Could not save the language: {error}", "表示言語を保存できませんでした: {error}"),
    rotation_drag(angle: f32, help: &str) => ("Rotation: {angle:.1} degrees · {help}", "回転: {angle:.1}度 · {help}"),
    rotation_drag_invalid(error: &str) => ("{error} · Preview unchanged; release cancels", "{error} · プレビューは変更しません。離すとキャンセル"),
    rotation_cancelled(error: &str) => ("Rotation cancelled: {error}", "回転をキャンセルしました: {error}"),
    png_keyword(keyword: &str) => ("PNG keyword: {keyword}", "PNGのキーワード: {keyword}"),
    xmp_property(format: &str, property: &str) => ("{format} XMP property: {property}", "{format}のXMPプロパティ: {property}"),
    metadata_read_failed(error: &str) => ("Could not read metadata: {error}", "メタデータを読み取れませんでした: {error}"),
    pixel_size(width: u32, height: u32) => ("{width} x {height} pixels", "{width} × {height} ピクセル"),
    animation_resize_budget(frames: usize) => ("All {frames} animation frames must fit within 512 MiB.", "アニメーションの全{frames}フレームを512 MiB以内に収めてください。"),
    video_resize_size(width: u32, height: u32, ratio: f64, suffix: &str) => ("{width} x {height} square pixels — ratio {ratio:.4}:1{suffix}", "{width} × {height} 正方形ピクセル — 縦横比 {ratio:.4}:1{suffix}"),
    rotation_size(angle: f64, width: u32, height: u32) => ("{angle:.1} degrees — {width} x {height} pixels", "{angle:.1}度 — {width} × {height} ピクセル"),
    loudness_summary(integrated: f64, peak: f64) => ("{integrated:.1} LUFS / max {peak:.1} dBTP", "{integrated:.1} LUFS / 最大 {peak:.1} dBTP"),
    next_audio_export(options: &str) => ("Next export: {options}. Playback and edits unchanged.", "次回の書き出し: {options}。再生と編集内容は変更しません。"),
    release_menu(menu: &str) => ("Release to open the {menu} menu", "離すと「{menu}」メニューを開きます"),
    also_used_by(commands: &str) => ("Also used by: {commands}", "同じキーを使用: {commands}"),
    configure_keybinding(command: &str) => ("Configure keybinding: {command}", "キー割り当てを設定: {command}"),
    remove_recent_command(command: &str) => ("Remove {command} from Recently Used", "「{command}」を最近使用した履歴から削除"),
}
