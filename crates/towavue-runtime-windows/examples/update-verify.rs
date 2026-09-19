//! Offline release verification using the same embedded public key as the app.
//! Does not download, install, execute the payload or modify any input.
use std::{
    error::Error,
    fs::{File, OpenOptions},
    io::Read,
    os::windows::fs::OpenOptionsExt,
    path::Path,
};
use towavue_runtime_windows::update::SignedUpdate;

fn bounded(path: &Path, maximum: u64) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(maximum + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        return Err("metadata exceeds protocol limit".into());
    }
    Ok(bytes)
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err("usage: update-verify MANIFEST SIGNATURE SETUP".into());
    }
    let manifest = bounded(Path::new(&args[0]), 256)?;
    let signature = bounded(Path::new(&args[1]), 512)?;
    let update = SignedUpdate::authenticate(&manifest, &signature)?;
    // Keep the payload sharing-locked throughout hashing. Only read access is
    // shared; Windows refuses concurrent modification or name replacement.
    let mut file = OpenOptions::new().read(true).share_mode(1).open(&args[2])?;
    update.verify_file(&mut file)?;
    println!(
        "Verified signed update {} ({} bytes)",
        update.manifest().version,
        update.manifest().bytes
    );
    Ok(())
}
