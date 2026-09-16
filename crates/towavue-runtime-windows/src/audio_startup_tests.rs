use super::*;

const FORMAT: AudioFormat = AudioFormat {
    sample_rate: 48_000,
    channels: 2,
};

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
    assert_eq!(
        output.try_event(),
        Some(AudioOutputEvent::Failed(
            "WASAPI output failed: injected startup failure".into()
        ))
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
            AudioOutputError::EndpointChanged.to_string()
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
