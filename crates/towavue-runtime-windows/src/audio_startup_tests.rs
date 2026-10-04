use super::*;

const FORMAT: AudioFormat = AudioFormat {
    sample_rate: 48_000,
    channels: 2,
};

#[test]
fn cloned_producers_wake_initial_output_once_without_changing_pcm() {
    let (audio_tx, audio_rx) = mpsc::sync_channel(4);
    let (control_tx, control_rx) = mpsc::channel();
    let sender = AudioOutputSender {
        format: FORMAT,
        audio_tx,
        control_tx,
        initial_audio_queued: Arc::new(AtomicBool::new(false)),
    };
    let producers: Vec<_> = (0..4)
        .map(|index| {
            let sender = sender.clone();
            thread::spawn(move || {
                sender.push(AudioChunk {
                    format: FORMAT,
                    frames: 1,
                    bytes: vec![index; 8],
                    presentation_time: MediaTime::from_nanoseconds(i64::from(index)),
                })
            })
        })
        .collect();
    for producer in producers {
        producer.join().expect("producer").expect("queued PCM");
    }
    assert!(matches!(
        control_rx.try_recv(),
        Ok(AudioControl::InitialAudioQueued)
    ));
    assert!(matches!(control_rx.try_recv(), Err(TryRecvError::Empty)));
    let mut received = Vec::new();
    for _ in 0..4 {
        let AudioMessage::Chunk(chunk) = audio_rx.try_recv().expect("published PCM") else {
            panic!("PCM payload");
        };
        assert_eq!(chunk.format, FORMAT);
        assert_eq!(chunk.frames, 1);
        assert_eq!(
            chunk.bytes,
            vec![chunk.presentation_time.as_nanoseconds() as u8; 8]
        );
        received.push(chunk.presentation_time.as_nanoseconds());
    }
    received.sort_unstable();
    assert_eq!(received, [0, 1, 2, 3]);
    sender.finish().expect("end marker");
    assert!(matches!(audio_rx.try_recv(), Ok(AudioMessage::End)));
    assert!(matches!(control_rx.try_recv(), Err(TryRecvError::Empty)));
}

#[test]
fn empty_audio_wakes_output_but_failed_publication_does_not() {
    for receiver_open in [true, false] {
        let (audio_tx, audio_rx) = mpsc::sync_channel(1);
        let audio_rx = receiver_open.then_some(audio_rx);
        let (control_tx, control_rx) = mpsc::channel();
        let sender = AudioOutputSender {
            format: FORMAT,
            audio_tx,
            control_tx,
            initial_audio_queued: Arc::new(AtomicBool::new(false)),
        };
        assert_eq!(sender.finish().is_ok(), receiver_open);
        if receiver_open {
            assert!(matches!(
                control_rx.try_recv(),
                Ok(AudioControl::InitialAudioQueued)
            ));
            assert!(matches!(
                audio_rx.expect("live receiver").try_recv(),
                Ok(AudioMessage::End)
            ));
        } else {
            assert!(matches!(control_rx.try_recv(), Err(TryRecvError::Empty)));
        }
    }
}

#[test]
fn queued_startup_returns_before_setup_and_reports_its_failure_once() {
    let (release, gate) = mpsc::channel();
    STARTUP_TEST_HOOK.set(Some(Box::new(move || {
        gate.recv_timeout(Duration::from_secs(5))
            .expect("caller must return before setup");
        Err(AudioOutputError::Wasapi("injected startup failure".into()))
    })));
    let (wake, events) = mpsc::channel();
    let anchor = MediaTime::from_nanoseconds(7_000_000_000);
    let output = AudioOutput::start_queued_with_rates(FORMAT, anchor, 0.0, 2.0, 1.0, move || {
        let _ = wake.send(());
    })
    .expect("queued handle before native setup");
    assert_eq!(output.position(), anchor);
    assert!(output.try_event().is_none());
    output
        .set_paused(true)
        .expect("pause before initialization");
    output.set_volume(0.25);
    assert_eq!(f32::from_bits(output.volume.load(Ordering::Relaxed)), 0.25);
    let sender = output.sender();
    for _ in 0..AUDIO_CHANNEL_CAPACITY {
        output
            .push(AudioChunk {
                format: FORMAT,
                frames: 1,
                bytes: vec![0; 8],
                presentation_time: anchor,
            })
            .expect("bounded pre-start queue");
    }
    let producer = thread::spawn(move || sender.finish());
    release.send(()).expect("release setup");
    events
        .recv_timeout(Duration::from_secs(5))
        .expect("failure wakes owner");
    let event = output.try_event();
    assert_eq!(
        event,
        Some(AudioOutputEvent::Failed(
            "WASAPI output failed: injected startup failure".into()
        ))
    );
    let Some(AudioOutputEvent::Failed(error)) = event else {
        panic!("startup failure");
    };
    assert_eq!(
        error.message(towavue_core::localization::Language::Japanese),
        "WASAPI音声出力に失敗しました: injected startup failure"
    );
    assert!(
        std::error::Error::source(&error)
            .expect("startup cause")
            .is::<AudioOutputError>()
    );
    drop(output);
    assert!(events.try_recv().is_err(), "one terminal notification");
    assert!(matches!(
        producer.join().expect("blocked producer released"),
        Err(AudioOutputError::Closed)
    ));
}

#[test]
fn dropping_pending_output_joins_startup_and_closes_its_producer() {
    let (release, gate) = mpsc::channel();
    let (entered, started) = mpsc::channel();
    STARTUP_TEST_HOOK.set(Some(Box::new(move || {
        entered.send(()).expect("setup entered");
        gate.recv_timeout(Duration::from_secs(5))
            .expect("release pending setup");
        Err(AudioOutputError::Closed)
    })));
    let output =
        AudioOutput::start_queued_with_rates(FORMAT, MediaTime::ZERO, 0.0, 1.0, 1.0, || {})
            .expect("queued output");
    let sender = output.sender();
    started
        .recv_timeout(Duration::from_secs(5))
        .expect("setup in flight");
    let (closed, completed) = mpsc::channel();
    let owner = thread::spawn(move || {
        drop(output);
        closed.send(()).expect("drop completed");
    });
    assert!(
        completed.try_recv().is_err(),
        "pending thread must remain owned"
    );
    release.send(()).expect("allow startup to finish");
    completed
        .recv_timeout(Duration::from_secs(5))
        .expect("joined pending startup");
    owner.join().expect("owner returned");
    assert!(matches!(sender.finish(), Err(AudioOutputError::Closed)));
}

#[test]
fn synchronous_startup_still_returns_the_native_error() {
    STARTUP_TEST_HOOK.set(Some(Box::new(|| {
        Err(AudioOutputError::Wasapi(
            "injected synchronous failure".into(),
        ))
    })));
    let result = AudioOutput::start_with_rates(FORMAT, MediaTime::ZERO, 0.0, 1.0, 1.0, || {});
    assert!(
        matches!(result, Err(AudioOutputError::Wasapi(message)) if message == "injected synchronous failure")
    );
    assert!(STARTUP_TEST_HOOK.take().is_none());
}

#[test]
fn invalid_endpoint_during_setup_fails_instead_of_requesting_another_restart() {
    STARTUP_TEST_HOOK.set(Some(Box::new(|| Err(AudioOutputError::EndpointChanged))));
    let (wake, events) = mpsc::channel();
    let output =
        AudioOutput::start_queued_with_rates(FORMAT, MediaTime::ZERO, 0.0, 1.0, 1.0, move || {
            let _ = wake.send(());
        })
        .expect("queued setup");
    events
        .recv_timeout(Duration::from_secs(5))
        .expect("failure notification");
    assert_eq!(
        output.try_event(),
        Some(AudioOutputEvent::Failed(
            AudioOutputError::EndpointChanged.into()
        ))
    );
}

#[test]
#[ignore = "requires live WASAPI Shared; queues only generated silence before native setup"]
fn queued_native_startup_preserves_pause_anchor_resume_and_drain() {
    for (rate, tempo_rate) in [(1.0, 1.0), (2.0, 2.0), (0.5, 1.0)] {
        let (release, gate) = mpsc::channel();
        STARTUP_TEST_HOOK.set(Some(Box::new(move || {
            gate.recv_timeout(Duration::from_secs(5))
                .expect("queued startup returns");
            Ok(())
        })));
        let anchor = MediaTime::from_nanoseconds(3_000_000_000);
        let (wake, events) = mpsc::channel();
        let output = AudioOutput::start_queued_with_rates(
            FORMAT,
            anchor,
            0.0,
            rate,
            tempo_rate,
            move || {
                let _ = wake.send(());
            },
        )
        .expect("queued native handle");
        output.set_paused(true).expect("pre-start pause");
        output
            .push(AudioChunk {
                format: FORMAT,
                frames: 9600,
                bytes: vec![0; 9600 * 8],
                presentation_time: anchor,
            })
            .expect("pre-start silence");
        output.finish().expect("pre-start EOF");
        release.send(()).expect("allow native setup");
        thread::sleep(Duration::from_millis(150));
        assert!(
            output.try_event().is_none(),
            "setup must succeed without draining paused audio"
        );
        assert_eq!(output.position(), anchor);
        output.set_paused(false).expect("resume");
        events
            .recv_timeout(Duration::from_secs(5))
            .expect("terminal wake");
        assert_eq!(output.try_event(), Some(AudioOutputEvent::Drained));
        assert!(output.position() > anchor);
        output.set_paused(true).expect("pause after drain");
        output.set_paused(false).expect("resume after drain");
    }
}
