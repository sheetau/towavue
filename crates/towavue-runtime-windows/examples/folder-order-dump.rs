//! Read-only fixture ordering for external navigation benchmark preparation.
use std::path::PathBuf;
use towavue_runtime_windows::FolderOrderProvider;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let folder = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("expected fixture folder")?,
    );
    if !folder.is_absolute() || !folder.is_dir() {
        return Err("expected an existing absolute folder".into());
    }
    let snapshot = FolderOrderProvider::new()?.snapshot(&folder)?;
    eprintln!(
        "source={:?} columns={:?}",
        snapshot.source, snapshot.sort_columns
    );
    for item in snapshot.items {
        println!("{}", item.path.display());
    }
    Ok(())
}
