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
    release_menu(menu: &str) => ("Release to open the {menu} menu", "離すと「{menu}」メニューを開きます"),
    also_used_by(commands: &str) => ("Also used by: {commands}", "同じキーを使用: {commands}"),
    configure_keybinding(command: &str) => ("Configure keybinding: {command}", "キー割り当てを設定: {command}"),
    remove_recent_command(command: &str) => ("Remove {command} from Recently Used", "「{command}」を最近使用した履歴から削除"),
}
