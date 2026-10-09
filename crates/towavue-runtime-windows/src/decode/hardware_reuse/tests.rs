use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

struct Policy(Option<bool>);
impl Policy {
    fn set(value: bool) -> Self {
        Self(ENABLED.replace(Some(value)))
    }
}
impl Drop for Policy {
    fn drop(&mut self) {
        ENABLED.set(self.0);
    }
}

struct Pacing(u64);
impl Pacing {
    fn set(value: u64) -> Self {
        Self(crate::renderer::preroll_pacing::BATCH_OVERRIDE.swap(value, Ordering::Relaxed))
    }
}
impl Drop for Pacing {
    fn drop(&mut self) {
        crate::renderer::preroll_pacing::BATCH_OVERRIDE.store(self.0, Ordering::Relaxed);
    }
}

struct Pixels {
    geometry: (MediaTime, u32, u32, Pixel),
    aspect: f32,
    orientation: VideoOrientation,
    transfer: VideoTransfer,
    bytes: Vec<u8>,
}

fn pixels(item: &HardwareVideoFrame) -> Pixels {
    let HardwareFrameBacking::Decoded(native) = &item.backing else {
        panic!("expected the decoder's original surface");
    };
    let mut transferred = frame::Video::empty();
    // SAFETY: both owned frames outlive the synchronous transfer. The source
    // retains its D3D11 pool and slice; only this thread writes the destination.
    let result = unsafe {
        ffmpeg::ffi::av_hwframe_transfer_data(transferred.as_mut_ptr(), native.as_ptr(), 0)
    };
    assert!(result >= 0, "native readback failed: {result}");
    let sample_bytes = match transferred.format() {
        Pixel::NV12 => 1,
        Pixel::P010LE | Pixel::P016LE => 2,
        other => panic!("add a lossless plane reader for {other:?}"),
    };
    let width = item.width as usize;
    let height = item.height as usize;
    let mut bytes = Vec::new();
    for (plane, rows, row_bytes) in [
        (0, height, width * sample_bytes),
        (1, height.div_ceil(2), width.div_ceil(2) * 2 * sample_bytes),
    ] {
        let stride = transferred.stride(plane);
        assert!(stride >= row_bytes);
        for row in 0..rows {
            bytes.extend_from_slice(
                &transferred.data(plane)[row * stride..row * stride + row_bytes],
            );
        }
    }
    Pixels {
        geometry: (
            item.presentation_time,
            item.width,
            item.height,
            transferred.format(),
        ),
        aspect: item.pixel_aspect,
        orientation: item.orientation,
        transfer: item.transfer,
        bytes,
    }
}

fn equal(actual: &Pixels, expected: &Pixels) {
    assert_eq!(actual.geometry, expected.geometry);
    assert_eq!(actual.aspect, expected.aspect);
    assert_eq!(actual.orientation, expected.orientation);
    assert_eq!(actual.transfer, expected.transfer);
    assert!(
        actual.bytes == expected.bytes,
        "full source sample planes differ"
    );
}

fn select(
    input: &mut ParallelInput,
    device: &GraphicsDevice,
    target: MediaTime,
) -> HardwareVideoFrame {
    let cancelled = AtomicBool::new(false);
    let mut selected = None;
    let result = input.decode_hardware(
        device,
        target,
        None,
        &|| cancelled.load(Ordering::Relaxed),
        |output| {
            if let ParallelRuntimeDecodeOutput::Item(RuntimeDecodeOutput::Video(frame)) = output {
                assert!(selected.is_none(), "only one selected target frame");
                selected = Some(frame);
                cancelled.store(true, Ordering::Relaxed);
                return false;
            }
            true
        },
    );
    assert!(
        matches!(result, Err(DecodeError::ConsumerClosed)),
        "{result:?}"
    );
    selected.expect("selected hardware frame")
}

struct PrerollEnabled(bool);
impl PrerollEnabled {
    fn set(value: bool) -> Self {
        Self(super::super::hardware_preroll::ENABLED.swap(value, Ordering::Relaxed))
    }
}
impl Drop for PrerollEnabled {
    fn drop(&mut self) {
        super::super::hardware_preroll::ENABLED.store(self.0, Ordering::Relaxed);
    }
}

fn reference(path: &Path, device: &GraphicsDevice, target: MediaTime) -> Pixels {
    let _preroll = PrerollEnabled::set(false);
    let _policy = Policy::set(false);
    let mut input =
        ParallelInput::open_tracked_video(path, &|| false).expect("fresh reference input");
    let selected = select(&mut input, device, target);
    assert!(input.retained_hardware.is_none());
    pixels(&selected)
}

#[test]
#[ignore = "explicit HEVC fixture and native D3D11 residual-output regression; run alone"]
fn native_backward_seek_after_gop_boundary_rejects_residual_output() {
    let path = std::env::split_paths(
        &std::env::var_os("TOWAVUE_HW_REUSE_FIXTURES").expect("explicit HEVC fixture"),
    )
    .next()
    .expect("one HEVC fixture");
    let _policy = Policy::set(true);
    let _pacing = Pacing::set(8);
    let device = GraphicsDevice::hardware_for_test().expect("native shared device");
    let target = MediaTime::from_nanoseconds(1_215_189_904);
    let expected = reference(&path, &device, target);
    for _ in 0..4 {
        let mut input =
            ParallelInput::open_tracked_video(&path, &|| false).expect("owned playback input");
        let config = best_stream_config(&input.input, Type::Video).expect("video stream");
        assert_eq!(config.parameters.id(), codec::Id::HEVC);
        let index = config.index;
        let key = Signature::new(&config);
        let mut pipeline = create_hardware_video_pipeline_from(config, &device)
            .expect("owned native HEVC decoder");
        seek_input_with_audio_track(
            &mut input.input,
            MediaTime::from_nanoseconds(7_215_189_904),
            true,
            input.audio_track,
            &|| false,
        )
        .expect("seek to preceding GOP");
        let mut held = None;
        let mut summary = DecodeSummary::default();
        // Stop inside receive, without a worker queue racing ahead and draining
        // the second output from the packet crossing this GOP boundary.
        for (stream, packet) in input.input.packets() {
            if stream.index() != index {
                continue;
            }
            pipeline
                .decoder
                .send_packet(&packet)
                .expect("native packet");
            let result = pipeline.receive(
                &mut |output| {
                    let RuntimeDecodeOutput::Video(frame) = output;
                    if frame.presentation_time >= MediaTime::from_nanoseconds(7_933_333_333) {
                        held = Some(frame);
                        return false;
                    }
                    true
                },
                &mut summary,
            );
            if held.is_some() {
                assert!(matches!(result, Err(DecodeError::ConsumerClosed)));
                break;
            }
            result.expect("preroll decode");
        }
        let held = held.expect("boundary output");
        input.started = true;
        input.retained_hardware = Some(Retained {
            pipeline,
            key,
            device: device.clone(),
        });
        let held_pixels = pixels(&held);
        let selected = select(&mut input, &device, target);
        equal(&pixels(&selected), &expected);
        equal(&pixels(&held), &held_pixels);
    }
}

#[test]
fn source_stamp_rejects_modified_replaced_missing_and_unleased_files() {
    let root = std::env::temp_dir().join(format!(
        "towavue-reuse-source-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos()
    ));
    std::fs::create_dir(&root).expect("unique owned fixture directory");
    let path = root.join("owned.bin");
    std::fs::write(&path, b"original").expect("write owned original");
    let lease = crate::export::frame::SourceLease::open(&path).expect("source lease");
    let source = lease.source.clone();
    drop(lease);
    assert!(source.matches_current());
    std::fs::write(&path, b"modified").expect("modify owned fixture");
    assert!(!source.matches_current(), "same-length in-place change");
    std::fs::remove_file(&path).expect("remove owned fixture");
    std::fs::write(&path, b"original").expect("write owned original");
    assert!(!source.matches_current(), "replacement with original bytes");
    std::fs::remove_file(&path).expect("remove owned fixture");
    assert!(!source.matches_current(), "missing source");
    std::fs::remove_dir(root).expect("remove empty owned directory");
}

#[test]
fn stream_signature_owns_metadata_and_discriminates_video_configuration() {
    let mut parameters = codec::Parameters::new();
    parameters.set_medium(Type::Video);
    parameters.set_id(codec::Id::H264);
    let mut config = StreamConfig {
        quantized_audio_timestamps: false,
        index: 0,
        time_base: Rational(1, 1000),
        parameters,
        display_matrix: Some(vec![1, 2, 3]),
    };
    let original = Signature::new(&config);
    config.index = 1;
    assert!(Signature::new(&config) != original);
    config.index = 0;
    config.time_base = Rational(1, 90000);
    assert!(Signature::new(&config) != original);
    config.time_base = Rational(1, 1000);
    // SAFETY: the local parameters exclusively own this initialized allocation.
    unsafe {
        (*config.parameters.as_mut_ptr()).width = 640;
    }
    assert!(Signature::new(&config) != original);
    unsafe {
        (*config.parameters.as_mut_ptr()).width = 0;
    }
    config.display_matrix.as_mut().expect("seeded matrix")[0] = 9;
    assert!(
        Signature::new(&config) != original,
        "owned metadata snapshot"
    );
    assert!(!supports(codec::Id::MPEG2VIDEO));
}

#[test]
#[ignore = "read-only explicit 12-second fixtures and native D3D11; run alone"]
fn native_full_source_reuse_preserves_pixels_cancel_eof_and_invalidation() {
    let paths: Vec<_> = std::env::split_paths(
        &std::env::var_os("TOWAVUE_HW_REUSE_FIXTURES")
            .expect("explicit semicolon-separated 12-second fixture paths"),
    )
    .collect();
    assert!(!paths.is_empty());
    let _policy = Policy::set(true);
    // This is a content/lifetime control, not a GPU timing endpoint. The separate
    // preroll cancellation block below restores the production query interval.
    let _pacing = Pacing::set(0);
    let device = GraphicsDevice::hardware_for_test().expect("native shared device");
    for (case, path) in paths.iter().enumerate() {
        let before = std::fs::read(path).expect("read-only fixture");
        let before_time = std::fs::metadata(path)
            .expect("fixture metadata")
            .modified()
            .expect("fixture modified time");
        let mut input =
            ParallelInput::open_tracked_video(path, &|| false).expect("owned playback input");
        let mut held = select(&mut input, &device, MediaTime::ZERO);
        let mut held_pixels = pixels(&held);
        assert!(input.retained_hardware.is_some());
        for percent in [70, 20, 80, 30, 60, 10, 1, 90] {
            let target = MediaTime::from_nanoseconds(percent * 120_000_000 + 16_666_667);
            let expected = reference(path, &device, target);
            let selected = select(&mut input, &device, target);
            let actual = pixels(&selected);
            equal(&actual, &expected);
            equal(&pixels(&held), &held_pixels);
            held = selected;
            held_pixels = actual;
            assert!(
                input.retained_hardware.is_some(),
                "interrupted owner reclaimed"
            );
        }
        {
            let _queries = Pacing::set(8);
            let before = crate::renderer::preroll_pacing::SUBMITTED.load(Ordering::Relaxed);
            let result = input.decode_hardware(
                &device,
                MediaTime::from_nanoseconds(8_416_666_667),
                None,
                &|| crate::renderer::preroll_pacing::SUBMITTED.load(Ordering::Relaxed) > before,
                |_| panic!("cancel before any selected frame"),
            );
            assert!(
                matches!(result, Err(DecodeError::ConsumerClosed)),
                "{result:?}"
            );
            assert!(input.retained_hardware.is_some());
        }
        let target = MediaTime::from_nanoseconds(2_416_666_667);
        equal(
            &pixels(&select(&mut input, &device, target)),
            &reference(path, &device, target),
        );
        equal(&pixels(&held), &held_pixels);
        let old_id = input
            .retained_hardware
            .as_ref()
            .expect("interrupted hardware owner")
            .pipeline
            .creation_id;
        input
            .retained_hardware
            .as_mut()
            .expect("interrupted hardware owner")
            .key
            .index += 1;
        let _ = select(&mut input, &device, target);
        assert_ne!(
            old_id,
            input
                .retained_hardware
                .as_ref()
                .expect("interrupted hardware owner")
                .pipeline
                .creation_id,
            "different stream signature cannot reuse the decoder"
        );
        let source = input.frame_source.take();
        let _ = select(&mut input, &device, target);
        assert!(
            input.retained_hardware.is_none(),
            "missing source identity disqualifies reuse"
        );
        input.frame_source = source;
        let _ = select(&mut input, &device, target);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            input.decode_hardware(&device, target, None, &|| false, |_| {
                panic!("owned consumer failure")
            })
        }));
        assert!(result.is_err());
        assert!(
            input.retained_hardware.is_none(),
            "unwinding cannot publish a retained decoder"
        );
        let _ = select(&mut input, &device, target);
        input
            .decode_software(target, None, Some(DecodeStream::Video), &|| false, |_| {
                false
            })
            .expect_err("closed software consumer");
        assert!(
            input.retained_hardware.is_none(),
            "software fallback clears hardware state"
        );
        let _ = select(&mut input, &device, target);
        input
            .decode_hardware(&device, target, Some(target), &|| false, |_| true)
            .expect("bounded terminal preview");
        assert!(
            input.retained_hardware.is_none(),
            "bounded previews do not retain a decoder"
        );
        let _ = select(&mut input, &device, target);
        input
            .decode_hardware(
                &device,
                MediaTime::from_nanoseconds(13_000_000_000),
                None,
                &|| false,
                |_| true,
            )
            .expect("natural EOF/drain");
        assert!(
            input.retained_hardware.is_none(),
            "natural EOF releases the decoder"
        );
        equal(&pixels(&held), &held_pixels);
        drop(input);
        assert!(
            std::fs::read(path).expect("unchanged fixture") == before,
            "fixture bytes changed"
        );
        assert_eq!(
            std::fs::metadata(path)
                .expect("fixture metadata")
                .modified()
                .expect("fixture modified time"),
            before_time
        );
        println!(
            "HARDWARE_REUSE_CONTENT case={case} full_samples=8 cancel=1 signature=1 source=1 consumer_panic=1 software=1 bounded=1 eof=1 held=1"
        );
    }
}

#[test]
#[ignore = "native hardware D3D11 identity control; run alone"]
fn native_recovery_device_cannot_receive_an_old_decoder() {
    let path = std::env::split_paths(
        &std::env::var_os("TOWAVUE_HW_REUSE_FIXTURES").expect("explicit fixture"),
    )
    .next()
    .expect("one fixture");
    let _policy = Policy::set(true);
    let _pacing = Pacing::set(0);
    let old_device = GraphicsDevice::hardware_for_test().expect("native hardware device");
    let mut input =
        ParallelInput::open_tracked_video(&path, &|| false).expect("tracked fixture input");
    let held = select(&mut input, &old_device, MediaTime::ZERO);
    let expected = pixels(&held);
    let old_id = input
        .retained_hardware
        .as_ref()
        .expect("interrupted hardware owner")
        .pipeline
        .creation_id;
    // Two distinct devices simulate old/new recovery ownership only in this
    // serial control. Production still creates and uses a single current device.
    let recovered_device = GraphicsDevice::hardware_for_test().expect("native hardware device");
    assert!(!old_device.same_device(&recovered_device));
    let _ = select(
        &mut input,
        &recovered_device,
        MediaTime::from_nanoseconds(2_416_666_667),
    );
    let retained = input
        .retained_hardware
        .as_ref()
        .expect("interrupted hardware owner");
    assert_ne!(retained.pipeline.creation_id, old_id);
    assert!(retained.device.same_device(&recovered_device));
    equal(&pixels(&held), &expected);
}

#[test]
#[ignore = "explicit video corpus and real D3D11 fence/query pixel comparison; run alone"]
fn native_overlapping_fence_and_query_preserve_source_planes_and_held_frames() {
    use crate::renderer::preroll_pacing::{FENCE_SUBMITTED, FORCE_QUERY, SUBMITTED};
    struct QueryPolicy(bool);
    impl Drop for QueryPolicy {
        fn drop(&mut self) {
            FORCE_QUERY.store(self.0, Ordering::Relaxed);
        }
    }
    let _query_policy = QueryPolicy(FORCE_QUERY.load(Ordering::Relaxed));
    let _pacing = Pacing::set(0);
    let _reuse = Policy::set(false);
    let paths: Vec<_> = std::env::split_paths(
        &std::env::var_os("TOWAVUE_HW_REUSE_FIXTURES").expect("explicit video fixtures"),
    )
    .collect();
    assert!(!paths.is_empty());
    let device = GraphicsDevice::hardware_for_test().expect("native hardware device");
    let before_fence = FENCE_SUBMITTED.load(Ordering::Relaxed);
    let before_all = SUBMITTED.load(Ordering::Relaxed);
    for (case, path) in paths.iter().enumerate() {
        let original = std::fs::read(path).expect("read-only source");
        for target_ms in [1215, 3615, 7215, 9615] {
            let target = MediaTime::from_nanoseconds(target_ms * 1_000_000);
            let mut expected = None;
            for mode in 0..3 {
                crate::renderer::preroll_pacing::BATCH_OVERRIDE
                    .store(if mode == 0 { 0 } else { 8 }, Ordering::Relaxed);
                FORCE_QUERY.store(mode == 1, Ordering::Relaxed);
                let mut input = ParallelInput::open_tracked_video(path, &|| false).expect("input");
                let held = select(&mut input, &device, target);
                let observed = pixels(&held);
                if let Some(reference) = &expected {
                    equal(&observed, reference);
                } else {
                    expected = Some(observed);
                }
                let _next = select(&mut input, &device, MediaTime::ZERO);
                equal(&pixels(&held), expected.as_ref().expect("reference"));
            }
        }
        assert_eq!(std::fs::read(path).expect("unchanged source"), original);
        eprintln!("PACING_PLANES case={case} modes=3 targets=4 held=verified");
    }
    let fences = FENCE_SUBMITTED.load(Ordering::Relaxed) - before_fence;
    let all = SUBMITTED.load(Ordering::Relaxed) - before_all;
    assert!(
        fences > 0,
        "real fence markers required on this explicit test host"
    );
    assert!(all > fences, "real query fallback markers required");
    eprintln!("PACING_MARKERS fence={fences} query={}", all - fences);
}

#[test]
#[ignore = "explicit private FFmpeg policy and HEVC fixture; run alone"]
fn native_metadata_preroll_preserves_seek_preview_and_reuse() {
    use super::super::hardware_preroll as omission;
    struct Flags;
    impl Drop for Flags {
        fn drop(&mut self) {
            omission::ENABLED.store(true, Ordering::Relaxed);
            omission::FORCE_SELECTED.store(false, Ordering::Relaxed);
            omission::DAMAGE_PTS.store(false, Ordering::Relaxed);
            omission::DAMAGE_REFERENCE.store(false, Ordering::Relaxed);
        }
    }
    let _flags = Flags;
    let _reuse = Policy::set(true);
    let path =
        std::path::PathBuf::from(std::env::var_os("TOWAVUE_PREROLL_FIXTURE").expect("fixture"));
    let device = GraphicsDevice::hardware_for_test().expect("one native device");
    let mut input = ParallelInput::open_tracked_video(&path, &|| false).expect("owned input");
    let mut held = None;
    let mut pairs = 0;
    for damaged in [false, true] {
        omission::DAMAGE_PTS.store(damaged, Ordering::Relaxed);
        for forced in [false, true] {
            omission::FORCE_SELECTED.store(forced, Ordering::Relaxed);
            for percent in [70, 20, 80, 30, 60, 10] {
                let target = MediaTime::from_nanoseconds(percent * 120_000_000 + 16_666_667);
                omission::ENABLED.store(false, Ordering::Relaxed);
                let expected = reference(&path, &device, target);
                omission::ENABLED.store(true, Ordering::Relaxed);
                let retries = omission::RETRIES.load(Ordering::Relaxed);
                let omitted = omission::OMITTED_OUTPUTS.load(Ordering::Relaxed);
                let selected = select(&mut input, &device, target);
                assert!(!selected.pixels_omitted());
                equal(&pixels(&selected), &expected);
                if let Some((previous, expected)) = &held {
                    equal(&pixels(previous), expected);
                }
                if forced && percent == 70 {
                    assert_eq!(
                        omission::RETRIES.load(Ordering::Relaxed),
                        retries + 1,
                        "one ordinary retry before selection"
                    );
                }
                let decoded = pixels(&selected);
                held = Some((selected, decoded));
                pairs += 1;
                println!(
                    "PREROLL_PAIR damaged={damaged} forced={forced} position={percent} omitted={} retries={} equal=1",
                    omission::OMITTED_OUTPUTS.load(Ordering::Relaxed) - omitted,
                    omission::RETRIES.load(Ordering::Relaxed) - retries
                );
            }
        }
    }
    omission::DAMAGE_PTS.store(false, Ordering::Relaxed);
    omission::FORCE_SELECTED.store(false, Ordering::Relaxed);
    for (name, target, end) in [
        (
            "bounded",
            MediaTime::from_nanoseconds(8_433_398_438),
            Some(MediaTime::from_nanoseconds(8_433_398_438)),
        ),
        ("eof", MediaTime::from_nanoseconds(13_000_000_000), None),
    ] {
        let mut reference_pixels = None;
        for enabled in [false, true] {
            omission::ENABLED.store(enabled, Ordering::Relaxed);
            let retries = omission::RETRIES.load(Ordering::Relaxed);
            let mut current =
                ParallelInput::open_tracked_video(&path, &|| false).expect("preview input");
            let mut outputs = Vec::new();
            let mut finished = 0;
            current
                .decode_hardware(&device, target, end, &|| false, |output| {
                    match output {
                        ParallelRuntimeDecodeOutput::Item(RuntimeDecodeOutput::Video(frame)) => {
                            assert!(!frame.pixels_omitted());
                            outputs.push(pixels(&frame));
                        }
                        ParallelRuntimeDecodeOutput::VideoFinished => finished += 1,
                    }
                    true
                })
                .expect("preview completed");
            assert_eq!(outputs.len(), 1);
            assert_eq!(finished, 1, "once-only terminal publication");
            if let Some(expected) = &reference_pixels {
                equal(&outputs[0], expected);
            } else {
                reference_pixels = outputs.pop();
            }
            if enabled && name == "bounded" {
                assert_eq!(omission::RETRIES.load(Ordering::Relaxed), retries + 1);
            }
            println!(
                "PREROLL_PREVIEW kind={name} enabled={enabled} retries={} finished={finished}",
                omission::RETRIES.load(Ordering::Relaxed) - retries
            );
        }
    }
    omission::ENABLED.store(true, Ordering::Relaxed);
    let before = omission::OMITTED_OUTPUTS.load(Ordering::Relaxed);
    let retries = omission::RETRIES.load(Ordering::Relaxed);
    let target = MediaTime::from_nanoseconds(2_416_666_667);
    let result = input.decode_hardware(
        &device,
        target,
        None,
        &|| omission::OMITTED_OUTPUTS.load(Ordering::Relaxed) > before,
        |_| panic!("cancelled preroll must not publish a selected frame"),
    );
    assert!(
        matches!(result, Err(DecodeError::ConsumerClosed)),
        "{result:?}"
    );
    assert_eq!(
        omission::RETRIES.load(Ordering::Relaxed),
        retries,
        "cancellation must not retry"
    );
    omission::ENABLED.store(false, Ordering::Relaxed);
    let expected = reference(&path, &device, target);
    omission::ENABLED.store(true, Ordering::Relaxed);
    equal(&pixels(&select(&mut input, &device, target)), &expected);
    if let Some((frame, expected)) = &held {
        equal(&pixels(frame), expected);
    }
    assert!(omission::OMITTED_OUTPUTS.load(Ordering::Relaxed) > 0);
    omission::DAMAGE_REFERENCE.store(true, Ordering::Relaxed);
    omission::ENABLED.store(false, Ordering::Relaxed);
    let target = MediaTime::from_nanoseconds(8_416_666_667);
    let expected = reference(&path, &device, target);
    omission::ENABLED.store(true, Ordering::Relaxed);
    let retries = omission::RETRIES.load(Ordering::Relaxed);
    equal(&pixels(&select(&mut input, &device, target)), &expected);
    assert_eq!(
        omission::RETRIES.load(Ordering::Relaxed),
        retries + 1,
        "invalid omitted reference retries once"
    );
    omission::DAMAGE_REFERENCE.store(false, Ordering::Relaxed);
    println!("PREROLL_COMPLETE pairs={pairs} previews=2 cancel=1 held=1 ordinary_retry=1");
}

#[test]
#[ignore = "private native policy, one device, full-plane continuation pairs; run alone"]
fn native_metadata_preroll_preserves_continuation() {
    use super::super::hardware_preroll as omission;
    let _enabled = PrerollEnabled::set(false);
    let path =
        std::path::PathBuf::from(std::env::var_os("TOWAVUE_PREROLL_FIXTURE").expect("fixture"));
    let device = GraphicsDevice::hardware_for_test().expect("shared device");
    let target = MediaTime::from_nanoseconds(2_416_666_667);
    let end = MediaTime::from_nanoseconds(3_416_666_667);
    std::thread::scope(|scope| {
        let (baseline_tx, baseline_rx) = std::sync::mpsc::sync_channel(1);
        let (candidate_tx, candidate_rx) = std::sync::mpsc::sync_channel(1);
        let decode = |tx: std::sync::mpsc::SyncSender<HardwareVideoFrame>| {
            let mut input = ParallelInput::open_tracked_video(&path, &|| false).expect("input");
            let mut finished = 0;
            input
                .decode_hardware(
                    &device,
                    target,
                    Some(end),
                    &|| false,
                    |output| match output {
                        ParallelRuntimeDecodeOutput::Item(RuntimeDecodeOutput::Video(frame)) => {
                            tx.send(frame).is_ok()
                        }
                        ParallelRuntimeDecodeOutput::VideoFinished => {
                            finished += 1;
                            true
                        }
                    },
                )
                .expect("bounded continuation");
            assert_eq!(finished, 1);
        };
        let baseline = scope.spawn(move || decode(baseline_tx));
        // A first output proves the baseline captured its disabled run policy.
        let mut previous = Some(baseline_rx.recv().expect("baseline first frame"));
        omission::ENABLED.store(true, Ordering::Relaxed);
        let omitted_before = omission::OMITTED_OUTPUTS.load(Ordering::Relaxed);
        let candidate = scope.spawn(move || decode(candidate_tx));
        let mut count = 0;
        let mut last_time = None;
        while let Some(expected) = previous {
            let actual = candidate_rx.recv().expect("matching continuation frame");
            assert!(!actual.pixels_omitted());
            equal(&pixels(&actual), &pixels(&expected));
            assert!(last_time.is_none_or(|time| actual.presentation_time > time));
            last_time = Some(actual.presentation_time);
            count += 1;
            previous = baseline_rx.recv().ok();
        }
        assert!(candidate_rx.recv().is_err(), "no duplicate or extra output");
        baseline.join().expect("baseline joined");
        candidate.join().expect("candidate joined");
        assert!(count >= 20);
        assert!(omission::OMITTED_OUTPUTS.load(Ordering::Relaxed) > omitted_before);
        println!("PREROLL_CONTINUATION frames={count} all_full_planes_equal=1 once_only=1");
    });
}

#[test]
#[ignore = "private AV1 native policy and fixed single-layer fixture; run alone"]
fn native_av1_preroll_preserves_seek_preview_cancel_and_reuse() {
    use super::super::hardware_preroll as omission;
    struct Flags;
    impl Drop for Flags {
        fn drop(&mut self) {
            omission::ENABLED.store(true, Ordering::Relaxed);
            omission::FORCE_SELECTED.store(false, Ordering::Relaxed);
            omission::DAMAGE_PTS.store(false, Ordering::Relaxed);
        }
    }
    let _flags = Flags;
    let _reuse = Policy::set(true);
    let paths: Vec<_> = std::env::split_paths(
        &std::env::var_os("TOWAVUE_AV1_PREROLL_FIXTURES").expect("fixed AV1 fixtures"),
    )
    .collect();
    assert_eq!(paths.len(), 1);
    let device = GraphicsDevice::hardware_for_test().expect("one native device");
    for (case, path) in paths.iter().enumerate() {
        let mut input = ParallelInput::open_tracked_video(path, &|| false).expect("input");
        let mut held = None;
        let mut forced_retries = 0;
        let mut pairs = 0;
        let mut ordinary_omitted = 0;
        for damaged in [false, true] {
            omission::DAMAGE_PTS.store(damaged, Ordering::Relaxed);
            for forced in [false, true] {
                omission::FORCE_SELECTED.store(forced, Ordering::Relaxed);
                for percent in [70, 20, 80, 30, 60, 10] {
                    let target = MediaTime::from_nanoseconds(percent * 120_000_000 + 16_666_667);
                    let expected = reference(path, &device, target);
                    let retries = omission::RETRIES.load(Ordering::Relaxed);
                    let omitted = omission::OMITTED_OUTPUTS.load(Ordering::Relaxed);
                    let selected = select(&mut input, &device, target);
                    assert!(!selected.pixels_omitted());
                    equal(&pixels(&selected), &expected);
                    if let Some((previous, expected)) = &held {
                        equal(&pixels(previous), expected);
                    }
                    let retries = omission::RETRIES.load(Ordering::Relaxed) - retries;
                    let omitted = omission::OMITTED_OUTPUTS.load(Ordering::Relaxed) - omitted;
                    assert!(retries <= 1, "bounded ordinary reconstruction");
                    if forced {
                        forced_retries += retries;
                    } else if !damaged {
                        ordinary_omitted += omitted;
                    }
                    let decoded = pixels(&selected);
                    held = Some((selected, decoded));
                    pairs += 1;
                    println!(
                        "AV1_PREROLL_PAIR case={case} damaged={damaged} forced={forced} position={percent} omitted={omitted} retries={retries} equal=1"
                    );
                }
            }
        }
        assert!(
            ordinary_omitted > 0,
            "actual nonreference submissions omitted"
        );
        assert!(
            forced_retries > 0,
            "actual selected/terminal reconstruction"
        );
        omission::DAMAGE_PTS.store(false, Ordering::Relaxed);
        omission::FORCE_SELECTED.store(false, Ordering::Relaxed);
        let mut preview_retries = 0;
        for percent in [70, 20, 80, 30, 60, 10, 110] {
            let target = MediaTime::from_nanoseconds(percent * 120_000_000 + 33_398_438);
            let end = (percent < 100).then_some(target);
            let mut expected = None;
            for enabled in [false, true] {
                omission::ENABLED.store(enabled, Ordering::Relaxed);
                let retries = omission::RETRIES.load(Ordering::Relaxed);
                let mut current =
                    ParallelInput::open_tracked_video(path, &|| false).expect("preview");
                let mut outputs = Vec::new();
                let mut finished = 0;
                current
                    .decode_hardware(&device, target, end, &|| false, |output| {
                        match output {
                            ParallelRuntimeDecodeOutput::Item(RuntimeDecodeOutput::Video(
                                frame,
                            )) => {
                                assert!(!frame.pixels_omitted());
                                outputs.push(pixels(&frame));
                            }
                            ParallelRuntimeDecodeOutput::VideoFinished => finished += 1,
                        }
                        true
                    })
                    .expect("preview completed");
                assert_eq!(outputs.len(), 1);
                assert_eq!(finished, 1);
                if let Some(reference) = &expected {
                    equal(&outputs[0], reference);
                } else {
                    expected = outputs.pop();
                }
                let retries = omission::RETRIES.load(Ordering::Relaxed) - retries;
                assert!(retries <= 1);
                if enabled {
                    preview_retries += retries;
                }
                println!(
                    "AV1_PREROLL_PREVIEW case={case} position={percent} enabled={enabled} retries={retries} finished={finished}"
                );
            }
        }
        assert!(
            preview_retries > 0,
            "omitted bounded/EOF preview reconstruction"
        );
        omission::ENABLED.store(true, Ordering::Relaxed);
        let before = omission::OMITTED_OUTPUTS.load(Ordering::Relaxed);
        let retries = omission::RETRIES.load(Ordering::Relaxed);
        let target = MediaTime::from_nanoseconds(2_416_666_667);
        let result = input.decode_hardware(
            &device,
            target,
            None,
            &|| omission::OMITTED_OUTPUTS.load(Ordering::Relaxed) > before,
            |_| panic!("cancelled preroll must not publish a selected frame"),
        );
        assert!(
            matches!(result, Err(DecodeError::ConsumerClosed)),
            "{result:?}"
        );
        assert_eq!(omission::RETRIES.load(Ordering::Relaxed), retries);
        let expected = reference(path, &device, target);
        equal(&pixels(&select(&mut input, &device, target)), &expected);
        if let Some((frame, expected)) = &held {
            equal(&pixels(frame), expected);
        }
        println!(
            "AV1_PREROLL_COMPLETE case={case} pairs={pairs} previews=7 cancel=1 held=1 ordinary_omitted={ordinary_omitted} forced_retries={forced_retries} preview_retries={preview_retries}"
        );
    }
}
