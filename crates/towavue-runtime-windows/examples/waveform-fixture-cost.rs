//! Read-only source waveform timings with a new, explicitly supplied cache root.
use std::error::Error;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::time::Instant;
use towavue_core::{EditTimeline, MediaTime};
use towavue_runtime_windows::{Cancellation, PreviewCache, timeline_waveform};

fn main() -> Result<(), Box<dyn Error>> {
    if cfg!(debug_assertions) {
        return Err("run this cost comparison with --release".into());
    }
    let mut arguments = std::env::args_os().skip(1);
    let source = PathBuf::from(arguments.next().ok_or("expected source path")?);
    let root = PathBuf::from(arguments.next().ok_or("expected new cache path")?);
    let seconds: i64 = arguments
        .next()
        .ok_or("expected duration in whole seconds")?
        .to_str()
        .ok_or("invalid duration text")?
        .parse()?;
    if arguments.next().is_some()
        || !source.is_absolute()
        || !source.is_file()
        || !root.is_absolute()
        || root.exists()
        || !(1..=604_800).contains(&seconds)
    {
        return Err(
            "expected an existing absolute source, new absolute cache and valid duration".into(),
        );
    }
    let before = std::fs::metadata(&source)?;
    let plan =
        EditTimeline::from_operations(MediaTime::from_nanoseconds(seconds * 1_000_000_000), &[])
            .ok_or("fixture timeline")?;
    let cache = PreviewCache::new(root.clone())?;
    let start = Instant::now();
    let overview = cache.waveform(&source, 640, 96)?;
    let native_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let memory = cache.waveform(&source, 640, 96)?;
    let memory_hit_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(memory, overview);
    let entries = std::fs::read_dir(&root)?.collect::<Result<Vec<_>, _>>()?;
    assert!(
        entries.iter().any(|entry| entry
            .path()
            .extension()
            .is_some_and(|extension| extension == "png")),
        "expected a persisted preview before timing its disk cache hit"
    );
    let reopened = PreviewCache::new(root)?;
    let start = Instant::now();
    let disk = reopened.waveform(&source, 640, 96)?;
    let disk_hit_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(disk, overview);
    assert_eq!((overview.width, overview.height), (640, 96));
    let start = Instant::now();
    let detail = timeline_waveform(&source, &plan, 1.0, 1.0, 960, &Cancellation::default())?;
    let detail_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(detail.len(), 960);
    assert!(
        detail
            .iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
    );
    assert!(
        detail.iter().any(|value| *value > 0.02),
        "expected non-silent fixture"
    );
    let mut preview_hash = std::collections::hash_map::DefaultHasher::new();
    overview.rgba.hash(&mut preview_hash);
    let mut detail_hash = std::collections::hash_map::DefaultHasher::new();
    for value in detail {
        value.to_bits().hash(&mut detail_hash);
    }
    let after = std::fs::metadata(&source)?;
    assert_eq!(before.len(), after.len());
    assert_eq!(before.modified()?, after.modified()?);
    println!(
        "{{\"native_ms\":{native_ms:.6},\"memory_hit_ms\":{memory_hit_ms:.6},\"disk_hit_ms\":{disk_hit_ms:.6},\"detail_ms\":{detail_ms:.6},\"preview_hash\":\"{:016x}\",\"detail_hash\":\"{:016x}\"}}",
        preview_hash.finish(),
        detail_hash.finish()
    );
    Ok(())
}
