use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use towavue_core::CommandId;

pub const LIMIT: usize = 50;
const HEADER: &str = "towavue command history v1";
const MAX_BYTES: u64 = 64 * 1024;

pub(super) fn apply(history: &mut Vec<CommandId>, pending: &[(CommandId, bool)]) {
    for &(command, remember) in pending {
        history.retain(|old| *old != command);
        if remember {
            history.insert(0, command);
            history.truncate(LIMIT);
        }
    }
}

pub(super) fn update(
    path: &Path,
    pending: &[(CommandId, bool)],
) -> std::io::Result<Vec<CommandId>> {
    fs::create_dir_all(
        path.parent()
            .ok_or_else(|| std::io::Error::other("Command history path has no parent."))?,
    )?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_extension("lock"))?;
    lock.lock()?;
    let mut history = Vec::new();
    match File::open(path) {
        Ok(file) => {
            let mut text = String::new();
            file.take(MAX_BYTES + 1).read_to_string(&mut text)?;
            if text.len() as u64 > MAX_BYTES {
                return Err(std::io::Error::other(
                    "Command history exceeds its size limit.",
                ));
            }
            let mut lines = text.lines();
            if lines.next() != Some(HEADER) {
                return Err(std::io::Error::other(
                    "Unrecognized command history format; existing file was retained.",
                ));
            }
            // Unknown command identifiers may belong to a newer version; ignore them
            // without rewriting the file merely because this window opened.
            for line in lines {
                if let Ok(command) = line.parse::<CommandId>()
                    && history.len() < LIMIT
                    && !history.contains(&command)
                {
                    history.push(command);
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    if pending.is_empty() {
        return Ok(history);
    }
    apply(&mut history, pending);
    let mut text = format!("{HEADER}\n");
    for command in &history {
        text.push_str(command.as_str());
        text.push('\n');
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = path.with_extension(format!("{}.{}.tmp", std::process::id(), nonce));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    Ok(history)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RecentFiles;

    fn fixture(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "towavue-command-history-{name}-{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("owned directory");
        root
    }

    #[test]
    fn bounded_mru_removal_and_unknown_ids_round_trip_without_rewriting_reads() {
        let root = fixture("roundtrip");
        let path = root.join("command-history.txt");
        let commands: Vec<_> = towavue_core::command_definitions()
            .iter()
            .map(|definition| definition.id)
            .collect();
        assert!(commands.len() > LIMIT);
        let pending: Vec<_> = commands.iter().map(|command| (*command, true)).collect();
        let stored = update(&path, &pending).expect("save");
        assert_eq!(
            stored,
            commands
                .iter()
                .rev()
                .copied()
                .take(LIMIT)
                .collect::<Vec<_>>()
        );
        let newest = stored[0];
        let older = stored[10];
        let changed = update(&path, &[(newest, false), (older, true), (older, true)])
            .expect("remove and use");
        assert_eq!(changed[0], older);
        assert_eq!(changed.len(), LIMIT - 1);
        assert!(!changed.contains(&newest));
        assert_eq!(update(&path, &[]).expect("reopen"), changed);
        let raw = format!(
            "{HEADER}\nfuture_command\n{}\n{}\n",
            older.as_str(),
            older.as_str()
        );
        fs::write(&path, &raw).expect("future identifier fixture");
        assert_eq!(update(&path, &[]).expect("compatible read"), [older]);
        assert_eq!(fs::read_to_string(&path).expect("unchanged bytes"), raw);
        fs::write(&path, "invalid header\n").expect("corrupt fixture");
        assert!(update(&path, &[(older, true)]).is_err());
        assert_eq!(
            fs::read_to_string(&path).expect("retained corrupt data"),
            "invalid header\n"
        );
        fs::write(&path, vec![b'x'; MAX_BYTES as usize + 1]).expect("oversize fixture");
        assert!(update(&path, &[]).is_err());
        fs::remove_dir_all(root).expect("owned cleanup");
    }

    #[test]
    fn queued_edits_suppress_stale_commands_and_flush_with_media_history() {
        let root = fixture("queued");
        let path = root.join("command-history.txt");
        let recent_path = root.join("recent-files.txt");
        let source = root.join("source.png");
        fs::write(&source, b"untouched source").expect("owned source");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.with_extension("lock"))
            .expect("lock");
        lock.lock().expect("hold worker");
        let (sender, receiver) = std::sync::mpsc::channel();
        let recent = RecentFiles::new(recent_path.clone(), move || {
            let _ = sender.send(());
        })
        .expect("worker");
        recent.record_command(CommandId::OpenFile);
        recent.record_command(CommandId::OpenFolder);
        recent.remove_command(CommandId::OpenFile);
        recent.record(source.clone());
        drop(lock);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            receiver
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .expect("delivery");
            if let Some(result) = recent.take_completed() {
                assert!(result.error.is_none());
                if let Some(commands) = result.commands {
                    assert_eq!(
                        commands,
                        [CommandId::OpenFolder],
                        "older results cannot replace queued edits"
                    );
                    break;
                }
            }
        }
        recent.remove_command(CommandId::OpenFolder);
        recent.record_command(CommandId::ShowLicenses);
        drop(recent);
        assert_eq!(
            update(&path, &[]).expect("flushed commands"),
            [CommandId::ShowLicenses]
        );
        assert_eq!(
            super::super::read(&recent_path).expect("media history")[0].path,
            source
        );
        assert_eq!(fs::read(&source).expect("source"), b"untouched source");
        fs::remove_dir_all(root).expect("owned cleanup");
    }

    #[test]
    fn independent_workers_merge_commands_and_file_clear_does_not_erase_mru() {
        let root = fixture("workers");
        let path = root.join("command-history.txt");
        let recent_path = root.join("recent-files.txt");
        let a = RecentFiles::new(recent_path.clone(), || {}).expect("first worker");
        let b = RecentFiles::new(recent_path, || {}).expect("second worker");
        a.record_command(CommandId::OpenFile);
        b.record_command(CommandId::OpenFolder);
        a.clear();
        drop(a);
        drop(b);
        let stored = update(&path, &[]).expect("merged commands");
        assert_eq!(stored.len(), 2);
        assert!(stored.contains(&CommandId::OpenFile) && stored.contains(&CommandId::OpenFolder));
        fs::remove_dir_all(root).expect("owned cleanup");
    }
    #[test]
    fn corrupt_command_storage_does_not_block_file_history_or_overwrite_data() {
        let root = fixture("corrupt-worker");
        let command_path = root.join("command-history.txt");
        fs::write(&command_path, b"unsupported command history").expect("corrupt fixture");
        let source = root.join("source.png");
        fs::write(&source, b"source data").expect("owned source");
        let recent_path = root.join("recent-files.txt");
        let (sender, receiver) = std::sync::mpsc::channel();
        let recent = RecentFiles::new(recent_path.clone(), move || {
            let _ = sender.send(());
        })
        .expect("worker");
        recent.record(source.clone());
        recent.record_command(CommandId::OpenFile);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            receiver
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .expect("delivery");
            if let Some(result) = recent.take_completed() {
                assert!(result.commands.is_none());
                assert!(
                    result
                        .error
                        .is_some_and(|error| error.contains("Command history unavailable"))
                );
                if !result.entries.is_empty() {
                    break;
                }
            }
        }
        drop(recent);
        assert_eq!(
            fs::read(&command_path).expect("retained bytes"),
            b"unsupported command history"
        );
        assert_eq!(
            super::super::read(&recent_path).expect("file history")[0].path,
            source
        );
        assert_eq!(fs::read(&source).expect("source"), b"source data");
        fs::remove_dir_all(root).expect("owned cleanup");
    }
}
