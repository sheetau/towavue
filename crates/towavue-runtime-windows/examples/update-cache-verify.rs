//! Inspect an existing local trial cache through the production verifier.
//! This never downloads, schedules, installs, launches, or removes an update.
use std::{error::Error, path::PathBuf};
use towavue_core::release::ReleaseVersion;
use towavue_runtime_windows::update::{UpdatePhase, UpdateStore};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: update-cache-verify CACHE EXPECTED_VERSION".into());
    }
    let root = PathBuf::from(&args[0]);
    if !root.is_absolute() || !root.is_dir() || !root.join("state.txt").is_file() {
        return Err("an existing absolute cache and state file are required".into());
    }
    let version: ReleaseVersion = args[1]
        .to_str()
        .ok_or("version is not valid Unicode")?
        .parse()?;
    // load() serializes inspection with the same cache.lock used by the app.
    // Its retained handles recheck the compiled key, payload and path safety.
    let cached = UpdateStore::new(root)
        .load()?
        .ok_or("the trial cache has no authenticated update")?;
    if cached.version() != version || cached.phase() != UpdatePhase::Ready {
        return Err("trial version or ready phase differs".into());
    }
    println!("Verified ready trial update {version}");
    Ok(())
}
