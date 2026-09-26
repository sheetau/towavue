use super::*;
use crate::renderer::preroll_pacing::{BATCH_OVERRIDE, SUBMITTED};
use std::os::windows::process::CommandExt;
use std::sync::atomic::Ordering;

#[test]
#[ignore = "windowless native D3D11 preroll completion/cancellation; run explicitly and serially"]
fn hardware_preroll_pacing_preserves_targets_eof_and_cancelled_input_reuse() {
    struct Restore(u64);
    impl Drop for Restore {
        fn drop(&mut self) {
            BATCH_OVERRIDE.store(self.0, Ordering::Relaxed);
        }
    }
    let _restore = Restore(BATCH_OVERRIDE.load(Ordering::Relaxed));
    let root = std::env::temp_dir().join(format!(
        "towavue-preroll-pacing-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time after epoch")
            .as_nanos()
    ));
    std::fs::create_dir(&root).expect("unique owned root");
    let path = root.join("long-gop.mp4");
    let executable =
        std::path::PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg"))
            .join("bin/ffmpeg.exe");
    let generated = std::process::Command::new(executable)
        .creation_flags(0x0800_0000)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x192:rate=30:duration=6",
            "-c:v",
            "libopenh264",
            "-g",
            "180",
            "-bf",
            "0",
            "-an",
        ])
        .arg(&path)
        .output()
        .expect("hidden fixture generation");
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let bytes = std::fs::read(&path).expect("fixture bytes");
    let device = GraphicsDevice::hardware_for_test().expect("windowless shared device");
    let mut input = ParallelInput::open(&path, &|| false).expect("owned input");
    for (start_ms, end_ms) in [
        (0, None),
        (4550, None),
        (4550, Some(4650)),
        (5980, None),
        (6000, None),
        (7000, None),
        (1000, Some(1000)),
        (0, Some(0)),
    ] {
        let mut expected = None;
        for batch in [0, 8] {
            BATCH_OVERRIDE.store(batch, Ordering::Relaxed);
            let mut frames = Vec::new();
            let mut eof = 0;
            let summary = input
                .decode_hardware(
                    &device,
                    MediaTime::from_nanoseconds(start_ms * 1_000_000),
                    end_ms.map(|ms| MediaTime::from_nanoseconds(ms * 1_000_000)),
                    &|| false,
                    |output| {
                        match output {
                            ParallelRuntimeDecodeOutput::Item(RuntimeDecodeOutput::Video(
                                frame,
                            )) => {
                                assert!(frame.texture_and_slice().is_some(), "real native surface");
                                frames.push((frame.presentation_time, frame.width, frame.height));
                            }
                            ParallelRuntimeDecodeOutput::VideoFinished => eof += 1,
                        }
                        true
                    },
                )
                .expect("complete hardware decode without software fallback");
            assert_eq!(eof, 1);
            let observed = (frames, summary.video_frames);
            if let Some(reference) = &expected {
                assert_eq!(&observed, reference);
            } else {
                expected = Some(observed);
            }
        }
    }
    assert!(
        SUBMITTED.load(Ordering::Relaxed) > 0,
        "actual completion queries"
    );
    for _ in 0..3 {
        let before = SUBMITTED.load(Ordering::Relaxed);
        let result = input.decode_hardware(
            &device,
            MediaTime::from_nanoseconds(4_550_000_000),
            None,
            &|| SUBMITTED.load(Ordering::Relaxed) > before,
            |_| panic!("cancel during preroll before selected output"),
        );
        assert!(
            matches!(result, Err(DecodeError::ConsumerClosed)),
            "{result:?}"
        );
        assert!(
            SUBMITTED.load(Ordering::Relaxed) > before,
            "cancel after native query submission"
        );
    }
    let mut recovered = Vec::new();
    input
        .decode_hardware(&device, MediaTime::ZERO, None, &|| false, |output| {
            if let ParallelRuntimeDecodeOutput::Item(RuntimeDecodeOutput::Video(frame)) = output {
                recovered.push(frame.presentation_time);
            }
            true
        })
        .expect("same input recovers after cancelled pending query");
    assert_eq!(recovered.len(), 180);
    assert_eq!(recovered[0], MediaTime::ZERO);
    assert!(recovered.windows(2).all(|pair| pair[0] < pair[1]));
    drop(input);
    drop(device);
    assert_eq!(std::fs::read(&path).expect("unchanged fixture"), bytes);
    std::fs::remove_file(path).expect("owned fixture cleanup");
    std::fs::remove_dir(root).expect("empty owned root cleanup");
}
