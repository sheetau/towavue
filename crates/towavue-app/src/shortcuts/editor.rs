use super::*;

/// Merge one command into the latest valid configuration. Cooperating windows/processes
/// serialize commits; edits to other commands are rebased and same-command conflicts fail.
pub fn save_command(
    path: &Path,
    command: CommandId,
    expected: &[KeySequence],
    replacement: &[KeySequence],
) -> Result<ShortcutBindings, String> {
    let lock_path = path.with_extension("conf.lock");
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|error| error.to_string())?;
    lock.try_lock().map_err(|_| {
        "Keyboard shortcuts are being saved by another window. Try again.".to_owned()
    })?;
    let original = fs::read(path).map_err(|error| error.to_string())?;
    let text = std::str::from_utf8(&original).map_err(|error| error.to_string())?;
    let mut bindings = parse(text, defaults())?;
    if bindings.all(command) != expected {
        return Err(
            "This command changed on disk. Reload keyboard shortcuts before editing it again."
                .into(),
        );
    }
    bindings.remove(command);
    for sequence in replacement {
        bindings.add(command, sequence.clone());
    }
    let output = rewrite(text, &bindings)?;
    let temporary = path.with_extension(format!(
        "conf.{}-{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos()
    ));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(output.as_bytes())?;
        file.sync_all()?;
        drop(file);
        // External editors do not use our lock. Reject observed concurrent replacement.
        if fs::read(path)? != original {
            return Err(std::io::Error::other(
                "Keyboard shortcuts changed while saving. Reload and try again.",
            ));
        }
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|error| error.to_string())?;
    Ok(bindings)
}

fn rewrite(text: &str, bindings: &ShortcutBindings) -> Result<String, String> {
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut output = String::new();
    if text.starts_with('\u{feff}') {
        output.push('\u{feff}');
    }
    output.push_str(CURRENT_BINDING_HEADER);
    output.push_str(newline);
    let mut written = std::collections::BTreeSet::new();
    for line in text.trim_start_matches('\u{feff}').lines() {
        if line.trim().starts_with("# towavue shortcuts v") {
            continue;
        }
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            output.push_str(line);
        } else {
            let (name, _) = line
                .split_once('=')
                .ok_or("Missing '=' in keyboard shortcuts")?;
            let command: CommandId = name
                .trim()
                .parse()
                .map_err(|_| "Unknown shortcut command")?;
            if !written.insert(command) {
                continue;
            }
            append_binding(&mut output, command, bindings);
        }
        output.push_str(newline);
    }
    // Materialize effective migrated values before upgrading the format version.
    // Empty declarations retain explicit removals instead of resurrecting defaults.
    for definition in towavue_core::command_definitions() {
        if written.insert(definition.id) {
            append_binding(&mut output, definition.id, bindings);
            output.push_str(newline);
        }
    }
    if parse(&output, defaults())? != *bindings {
        return Err("Keyboard shortcuts could not be preserved; the file was not changed.".into());
    }
    Ok(output)
}

fn append_binding(output: &mut String, command: CommandId, bindings: &ShortcutBindings) {
    output.push_str(command.as_str());
    output.push_str(" = ");
    output.push_str(
        &bindings
            .all(command)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" | "),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_migrates_reload_without_stealing_custom_keys_and_preserves_explicit_removal() {
        let old = parse(
            "# towavue shortcuts v7\nreload_shortcuts = Ctrl+K Ctrl+S\n",
            defaults(),
        )
        .expect("shortcut editor fixture");
        assert!(old.all(CommandId::ReloadShortcuts).is_empty());
        assert_eq!(
            old.get(CommandId::OpenKeyboardSettings)
                .expect("shortcut editor fixture")
                .to_string(),
            "Ctrl+K Ctrl+S"
        );
        for text in [
            "# towavue shortcuts v7\nreload_shortcuts = Ctrl+K Ctrl+S\nopen_file = Ctrl+K Ctrl+S\n",
            "# towavue shortcuts v8\nreload_shortcuts = Ctrl+K Ctrl+S\n",
            "open_file = Ctrl+K\n",
        ] {
            let bindings = parse(text, defaults()).expect("shortcut editor fixture");
            assert!(bindings.all(CommandId::OpenKeyboardSettings).is_empty());
        }
        let custom = parse("reload_shortcuts = Ctrl+K Ctrl+R\n", defaults())
            .expect("shortcut editor fixture");
        assert_eq!(
            custom
                .get(CommandId::ReloadShortcuts)
                .expect("shortcut editor fixture")
                .to_string(),
            "Ctrl+K Ctrl+R"
        );
        let cleared = parse("toggle_fullscreen =\nopen_file = Enter\n", defaults())
            .expect("shortcut editor fixture");
        assert!(cleared.all(CommandId::ToggleFullscreen).is_empty());
        assert_eq!(
            parse(&serialize(&cleared), defaults()).expect("shortcut editor fixture"),
            cleared
        );
        for key in ["F1", "Ctrl+F12", "Alt+F24", "Insert", "F11"] {
            let sequence: KeySequence = key.parse().expect("valid test key");
            assert_eq!(
                sequence
                    .to_string()
                    .parse::<KeySequence>()
                    .expect("shortcut editor fixture"),
                sequence
            );
        }
        for key in ["F0", "F25", "Ctrl+"] {
            assert!(key.parse::<KeySequence>().is_err());
        }
    }

    #[test]
    fn editor_saves_removal_rebases_other_commands_and_refuses_stale_invalid_or_locked_files() {
        let Some(root) = crate::tests::isolated_test_root(
            "shortcuts::editor::tests::editor_saves_removal_rebases_other_commands_and_refuses_stale_invalid_or_locked_files",
        ) else {
            return;
        };
        let path = root.join("shortcuts.conf");
        let original = "\u{feff}# towavue shortcuts v7\r\n# Keep my notes\r\nopen_file = Ctrl+O\r\nreload_shortcuts = Ctrl+K Ctrl+S\r\n";
        fs::write(&path, original).expect("shortcut editor fixture");
        let initial = load_from(&path).expect("shortcut editor fixture");
        let removed = save_command(
            &path,
            CommandId::OpenFile,
            initial.all(CommandId::OpenFile),
            &[],
        )
        .expect("shortcut editor fixture");
        assert!(removed.all(CommandId::OpenFile).is_empty());
        assert_eq!(load_from(&path).expect("shortcut editor fixture"), removed);
        let text = fs::read_to_string(&path).expect("shortcut editor fixture");
        assert!(text.starts_with('\u{feff}') && text.contains("# Keep my notes\r\n"));
        assert!(!text.replace("\r\n", "").contains('\n'));
        let changed = text.replace("open_folder = Ctrl+Shift+O", "open_folder = Alt+F12");
        fs::write(&path, &changed).expect("shortcut editor fixture");
        let saved = save_command(
            &path,
            CommandId::OpenFile,
            &[],
            &["Ctrl+K F2".parse().expect("valid test key")],
        )
        .expect("shortcut editor fixture");
        assert_eq!(
            saved
                .get(CommandId::OpenFolder)
                .expect("shortcut editor fixture")
                .to_string(),
            "Alt+F12"
        );
        let good = fs::read(&path).expect("shortcut editor fixture");
        assert!(save_command(&path, CommandId::OpenFile, &[], &[]).is_err());
        assert_eq!(fs::read(&path).expect("shortcut editor fixture"), good);
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path.with_extension("conf.lock"))
            .expect("shortcut editor fixture");
        lock.try_lock().expect("shortcut editor fixture");
        assert!(
            save_command(
                &path,
                CommandId::OpenFile,
                saved.all(CommandId::OpenFile),
                &[]
            )
            .is_err()
        );
        assert_eq!(fs::read(&path).expect("shortcut editor fixture"), good);
        drop(lock);
        fs::write(&path, "unknown_command = Ctrl+O\n").expect("shortcut editor fixture");
        assert!(
            save_command(
                &path,
                CommandId::OpenFile,
                saved.all(CommandId::OpenFile),
                &[]
            )
            .is_err()
        );
        assert_eq!(
            fs::read_to_string(&path).expect("shortcut editor fixture"),
            "unknown_command = Ctrl+O\n"
        );
    }
}
