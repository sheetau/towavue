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
    status_zoom(value: &str) => ("{value} zoom", "表示倍率 {value}"),
    status_reduction(zoom: &str, reduction: &str) => ("{zoom}. {reduction}", "{zoom}。{reduction}"),
    status_speed(value: &str) => ("Speed {value}", "再生速度 {value}"),
    source_frame_rate(value: &str) => ("Source frame rate: {value} (stream-reported; independent of playback speed)", "元のフレームレート: {value}（ストリームの値。再生速度には依存しません）"),
    status_pixels(value: &str) => ("{value} pixels", "{value} ピクセル"),
    status_frames(count: usize) => ("{count} frames", "{count}フレーム"),
    status_animation_frames(count: usize) => ("{count} animation frames", "アニメーションのフレーム数: {count}"),
    status_modified(value: &str) => ("Modified (local): {value}", "更新日時（ローカル）: {value}"),
    time_focus(label: &str, time: &str) => ("{label}: {time} · Left/Right adjust", "{label}: {time} · ←／→で調整"),
    value_focus(label: &str, value: f64) => ("{label}: {value:.3} · Left/Right adjust", "{label}: {value:.3} · ←／→で調整"),
    timeline_length(value: &str) => ("Length {value}", "長さ {value}"),
    timeline_gain(value: f64) => ("Gain {value:.0}%", "倍率 {value:.0}%"),
    preview_unavailable(error: &str) => ("\nPreview unavailable: {error}", "\nプレビューを表示できません: {error}"),
    mute_named_tab(name: &str) => ("Mute tab: {name}", "タブをミュート: {name}"),
    unmute_named_tab(name: &str) => ("Unmute tab: {name}", "タブのミュートを解除: {name}"),
    reading_status(pages_hint: &str, pages: usize, first_hint: &str, first: usize) => ("{pages_hint}Reading {pages} · {first_hint}first {first}", "{pages_hint}読書 {pages}ページ · {first_hint}先頭 {first}ページ"),
    calendar_month(name: &str, year: u16) => ("{name} {year}", "{year}年{name}"),
    preparation_elapsed(phase: &str, time: &str) => ("{phase} · elapsed {time}", "{phase} · 経過時間 {time}"),
    tab_label(title: &str) => ("{title} tab", "{title} タブ"),
    close_title(title: &str) => ("Close {title}", "{title}を閉じる"),
    close_tab(title: &str) => ("Close tab: {title}", "タブを閉じる: {title}"),
    file_search_summary(shown: usize, matches: u64, skipped: u64) => ("Showing {shown} of {matches} matches; {skipped} entries skipped", "{matches}件中{shown}件を表示、{skipped}件をスキップ"),
    file_search_help(summary: &str) => ("{summary}\nUnreadable entries, links/junctions and folders deeper than 128 levels are skipped. Refine the query to narrow results.", "{summary}\n読み取れない項目、リンク／ジャンクション、128階層より深いフォルダーはスキップします。検索語を追加して結果を絞り込んでください。"),
    dialog_thread_failed(error: &str) => ("the file-dialog thread could not start: {error}", "ファイルダイアログの処理を開始できませんでした: {error}"),
    dialog_windows_failed(error: &str) => ("Windows file dialog failed: {error}", "Windowsのファイルダイアログでエラーが発生しました: {error}"),
    dialog_invalid_path(error: &str) => ("Windows returned an invalid UTF-16 path: {error}", "Windowsが返したパスの文字情報が不正です（UTF-16）: {error}"),
    dialog_export_failed(error: &str) => ("Could not prepare export formats: {error}", "書き出し形式を準備できませんでした: {error}"),
    native_delete(path: &str, retained: &str) => ("{path}\n\nThe file will be moved to the Recycle Bin.{retained}", "{path}\n\nこのファイルをごみ箱へ移動します。{retained}"),
    native_about_title(version: &str) => ("towavue / Version {version}", "towavue / バージョン {version}"),
    native_about_content(license: &str) => ("Media viewer for Windows\n\nCreator: <a href=\"author\">sheeta</a>\n<a href=\"repository\">GitHub</a>\n\n{license}. Provided without warranty.", "Windows用メディアビューアー\n\n制作者：<a href=\"author\">sheeta</a>\n<a href=\"repository\">GitHub</a>\n\n{license}。無保証で提供されます。"),
    native_extension_mismatch(format: &str) => ("The filename extension does not match {format}. Change the filename extension or select the matching file type.", "ファイル名の拡張子が{format}と一致しません。拡張子を変更するか、一致するファイル形式を選択してください。"),
    native_update_notice(version: &str, failed: &str) => ("Version {version} is downloaded and ready to install.{failed}\n\nInstallation can take several minutes. towavue will close and reopen automatically.", "バージョン{version}をダウンロードしました。インストールできます。{failed}\n\nインストールには数分かかる場合があります。towavueは自動で終了し、起動し直します。"),
    native_recovery(error: &str) => ("Graphics could not be restored.\n\n{error}\n\nRetry: restore graphics at the saved playback position.\nCancel: keep all edits. Press Alt+F4 afterward to export or close.", "描画を復元できませんでした。\n\n{error}\n\n再試行：保存した再生位置で描画を復元します。\nキャンセル：すべての編集を保持します。その後Alt+F4を押すと、書き出しまたは終了を選択できます。"),
    native_save_guard(name: &str, save: &str) => ("{name}\n\n{save} Cancel keeps your edits and stops this action.", "{name}\n\n{save} キャンセルすると、編集内容を保持してこの操作を取り消します。"),
    native_export_error(error: &str) => ("Export failed. Your edits are retained.\n\n{error}", "書き出しに失敗しました。編集内容は保持されています。\n\n{error}"),
    about_failed(error: &str) => ("Could not show About: {error}", "アプリ情報を表示できませんでした: {error}"),
    link_failed(error: &str) => ("Could not open link: {error}", "リンクを開けませんでした: {error}"),
    update_notice_failed(error: &str) => ("Could not show the update notification: {error}", "更新通知を表示できませんでした: {error}"),
    confirmation_failed(error: &str) => ("Could not show the confirmation; edits kept: {error}", "確認を表示できませんでした。編集内容は保持されています: {error}"),
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
