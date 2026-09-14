//! Generated-media timings for source overviews and native timeline refinement.
//! Files are already OS-cache-warm; only the preview cache starts empty each round.

use std::error::Error;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use towavue_core::{EditTimeline, MediaTime};
use towavue_runtime_windows::{Cancellation, PreviewCache, timeline_waveform};

fn generate(ffmpeg: &Path, path: &Path, codec: &str, seconds: u32) -> Result<(), Box<dyn Error>> {
    let output = Command::new(ffmpeg)
        .creation_flags(0x0800_0000)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "aevalsrc=0.2*sin(2*PI*440*t)|0.3*sin(2*PI*880*t):s=48000",
            "-t",
            &seconds.to_string(),
            "-c:a",
            codec,
        ])
        .arg(path)
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    if cfg!(debug_assertions) {
        return Err("run this cost comparison with --release".into());
    }
    let ffmpeg = PathBuf::from(std::env::var_os("FFMPEG_DIR").ok_or("set FFMPEG_DIR")?)
        .join("bin/ffmpeg.exe");
    let root = std::env::temp_dir().join(format!(
        "towavue-waveform-cost-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    std::fs::create_dir(&root)?;
    // Retain owned fixtures if a check fails, to permit investigation.
    eprintln!("WAVEFORM_COST fixture={}", root.display());
    for seconds in [1, 180] {
        for (codec, extension) in [("pcm_s16le", "wav"), ("aac", "m4a")] {
            let path = root.join(format!("{seconds}.{extension}"));
            generate(&ffmpeg, &path, codec, seconds)?;
            let stamp = std::fs::metadata(&path)?;
            let plan = EditTimeline::from_operations(
                MediaTime::from_nanoseconds(i64::from(seconds) * 1_000_000_000),
                &[],
            )
            .ok_or("fixture timeline")?;
            let mut expected_overview = None;
            let mut expected_detail = None;
            for round in 0..3 {
                let cache =
                    PreviewCache::new(root.join(format!("cache-{seconds}-{codec}-{round}")))?;
                let cli_cache =
                    PreviewCache::new(root.join(format!("cli-{seconds}-{codec}-{round}")))?;
                let overview = || {
                    let start = Instant::now();
                    let image = cache.waveform(&path, 640, 96)?;
                    Ok::<_, Box<dyn Error>>((image, start.elapsed()))
                };
                let cli = || {
                    let start = Instant::now();
                    let image = cli_cache.verification_cli_waveform(&path, 640, 96)?;
                    Ok::<_, Box<dyn Error>>((image, start.elapsed()))
                };
                let detail = || {
                    let start = Instant::now();
                    let values =
                        timeline_waveform(&path, &plan, 1.0, 1.0, 960, &Cancellation::default())?;
                    Ok::<_, Box<dyn Error>>((values, start.elapsed()))
                };
                let (overview, cli, detail) = if round % 2 == 0 {
                    (overview()?, cli()?, detail()?)
                } else {
                    let detail = detail()?;
                    let cli = cli()?;
                    (overview()?, cli, detail)
                };
                assert!(overview.0 == cli.0, "native and CLI overviews differ");
                let cached_start = Instant::now();
                let cached = cache.waveform(&path, 640, 96)?;
                let cached_time = cached_start.elapsed();
                assert_eq!(cached, overview.0);
                assert_eq!((overview.0.width, overview.0.height), (640, 96));
                assert_eq!(detail.0.len(), 960);
                assert!(
                    detail
                        .0
                        .iter()
                        .all(|value| value.is_finite() && *value > 0.05 && *value < 0.4)
                );
                if let Some(expected) = &expected_overview {
                    assert_eq!(&overview.0, expected);
                }
                if let Some(expected) = &expected_detail {
                    assert_eq!(&detail.0, expected);
                }
                expected_overview = Some(overview.0);
                expected_detail = Some(detail.0);
                println!(
                    "WAVEFORM_COST seconds={seconds} codec={codec} round={round} native_ms={:.3} cli_ms={:.3} cached_ms={:.3} detail_ms={:.3}",
                    overview.1.as_secs_f64() * 1000.0,
                    cli.1.as_secs_f64() * 1000.0,
                    cached_time.as_secs_f64() * 1000.0,
                    detail.1.as_secs_f64() * 1000.0,
                );
            }
            let after = std::fs::metadata(&path)?;
            assert_eq!(stamp.len(), after.len());
            assert_eq!(stamp.modified()?, after.modified()?);
        }
    }
    std::fs::remove_dir_all(&root)?;
    Ok(())
}
