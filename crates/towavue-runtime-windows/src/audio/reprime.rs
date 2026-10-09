use super::*;

impl AudioOutput {
    /// Replace a live same-format run on its existing native owner. On any
    /// failure the caller must drop this output and use ordinary queued startup.
    /// The acknowledgement is bounded; Drop retains the existing shutdown/join.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn reprime(
        &mut self,
        format: AudioFormat,
        media_anchor: MediaTime,
        volume: f32,
        rate: f32,
        tempo_rate: f32,
        paused: bool,
        notify: impl Fn() + Send + 'static,
    ) -> Result<(), AudioOutputError> {
        if format != self.format {
            return Err(AudioOutputError::UnsupportedFormat(
                format.sample_rate,
                format.channels,
            ));
        }
        if self.drained.load(Ordering::Acquire)
            || self.thread.as_ref().is_none_or(JoinHandle::is_finished)
        {
            return Err(AudioOutputError::Closed);
        }
        let (audio_tx, audio_rx) = mpsc::sync_channel(AUDIO_CHANNEL_CAPACITY);
        let (event_tx, event_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let position = Arc::new(PositionClock::new(media_anchor));
        let volume = Arc::new(AtomicU32::new(normalized_volume(volume).to_bits()));
        let drained = Arc::new(AtomicBool::new(false));
        let run = AudioRun {
            audio_rx,
            media_anchor,
            rate,
            tempo_rate,
            paused,
            position_clock: Arc::clone(&position),
            volume: Arc::clone(&volume),
            drained: Arc::clone(&drained),
            event_tx,
            ready_tx,
            notify: Box::new(notify),
        };
        self.control_tx
            .send(AudioControl::Reprime(Box::new(run)))
            .map_err(|_| AudioOutputError::Closed)?;
        self.control_wake.signal();
        ready_rx
            .recv_timeout(Duration::from_millis(100))
            .map_err(|_| AudioOutputError::Closed)??;
        // Publish the new generation only after the old receiver, device PCM,
        // clock and tempo have been retired/reset on the native owner. Old
        // senders cannot deliver into this new bounded channel. Replacing the
        // event receiver also excludes queued old-generation notifications.
        self.audio_tx = audio_tx;
        self.event_rx = event_rx;
        self.position_clock = position;
        self.paused_position.set(paused.then_some(media_anchor));
        self.volume = volume;
        self.drained = drained;
        self.initial_audio_queued = Arc::new(AtomicBool::new(false));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const FORMAT: AudioFormat = AudioFormat {
        sample_rate: 48_000,
        channels: 2,
    };

    fn chunk(anchor: MediaTime, frames: usize) -> AudioChunk {
        AudioChunk {
            format: FORMAT,
            frames,
            bytes: vec![0; frames * 8],
            presentation_time: anchor,
        }
    }

    #[test]
    fn replacement_retires_blocked_senders_and_isolates_clocks_and_notifications() {
        let (audio_tx, audio_rx) = mpsc::sync_channel(AUDIO_CHANNEL_CAPACITY);
        let (control_tx, control_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let position = Arc::new(PositionClock::new(MediaTime::from_nanoseconds(100)));
        let drained = Arc::new(AtomicBool::new(false));
        let old_position = Arc::clone(&position);
        let old_drained = Arc::clone(&drained);
        let (notified, notification) = mpsc::channel();
        let worker = thread::spawn(move || {
            let run = loop {
                match control_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("replacement control")
                {
                    AudioControl::InitialAudioQueued => {}
                    AudioControl::Reprime(run) => break run,
                    _ => panic!("unexpected old control"),
                }
            };
            assert!(run.paused);
            assert_eq!(run.rate, 2.0);
            assert_eq!(run.tempo_rate, 1.0);
            drop(audio_rx);
            old_position.run(1.0).publish_device(899);
            old_drained.store(true, Ordering::Release);
            event_tx
                .send(AudioOutputEvent::Drained)
                .expect("stale notification");
            run.ready_tx.send(Ok(())).expect("reset acknowledged");
            let AudioMessage::Chunk(chunk) = run
                .audio_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("new PCM")
            else {
                panic!("new PCM required")
            };
            assert_eq!(chunk.presentation_time, run.media_anchor);
            run.drained.store(true, Ordering::Release);
            run.event_tx
                .send(AudioOutputEvent::Drained)
                .expect("new notification");
            (run.notify)();
            loop {
                match control_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("shutdown")
                {
                    AudioControl::InitialAudioQueued => {}
                    AudioControl::Shutdown => break,
                    _ => panic!("unexpected new control"),
                }
            }
        });
        let mut output = AudioOutput {
            format: FORMAT,
            audio_tx,
            control_tx,
            control_wake: Arc::new(ControlWake::default()),
            initial_audio_queued: Arc::new(AtomicBool::new(false)),
            event_rx,
            position_clock: position,
            paused_position: Cell::new(None),
            volume: Arc::new(AtomicU32::new(0)),
            drained,
            thread: Some(worker),
        };
        let sender = output.sender();
        for _ in 0..AUDIO_CHANNEL_CAPACITY {
            sender
                .push(chunk(MediaTime::ZERO, 1))
                .expect("fill old channel");
        }
        let blocked = sender.clone();
        let producer = thread::spawn(move || blocked.push(chunk(MediaTime::ZERO, 1)));
        let anchor = MediaTime::from_nanoseconds(3_000_000_000);
        output
            .reprime(FORMAT, anchor, 0.0, 2.0, 1.0, true, move || {
                let _ = notified.send(());
            })
            .expect("replace live run");
        assert!(matches!(
            producer.join().expect("old producer"),
            Err(AudioOutputError::Closed)
        ));
        assert!(matches!(sender.finish(), Err(AudioOutputError::Closed)));
        assert_eq!(output.position(), anchor);
        assert!(!output.drained.load(Ordering::Acquire));
        assert!(output.try_event().is_none(), "old event queue was retired");
        output.push(chunk(anchor, 1)).expect("new producer");
        notification
            .recv_timeout(Duration::from_secs(5))
            .expect("new notification callback");
        assert_eq!(output.try_event(), Some(AudioOutputEvent::Drained));
        assert!(output.drained.load(Ordering::Acquire));
        let worker = output.thread.take().expect("native owner surrogate");
        drop(output);
        worker.join().expect("surrogate assertions");
    }

    #[test]
    #[ignore = "requires live WASAPI Shared; only generated muted silence is played"]
    fn native_reprime_preserves_owner_pause_anchor_rate_and_natural_release() {
        let (old_notify, old_events) = mpsc::channel();
        let mut output =
            AudioOutput::start_with_settings(FORMAT, MediaTime::ZERO, 0.0, 1.0, move || {
                let _ = old_notify.send(());
            })
            .expect("native output");
        let worker = output.worker_id();
        let changed_format = AudioFormat {
            sample_rate: 44_100,
            channels: 2,
        };
        assert!(matches!(
            output.reprime(changed_format, MediaTime::ZERO, 0.0, 1.0, 1.0, false, || {}),
            Err(AudioOutputError::UnsupportedFormat(44_100, 2))
        ));
        output
            .reprime(FORMAT, MediaTime::ZERO, 0.0, 1.0, 1.0, false, || {})
            .expect("reset initialized owner before its first PCM");
        assert_eq!(output.worker_id(), worker);
        output
            .push(chunk(MediaTime::ZERO, 144_000))
            .expect("old silence");
        thread::sleep(Duration::from_millis(35));
        for (index, rate, tempo_rate) in [(1, 2.0, 2.0), (2, 0.5, 1.0), (3, 1.0, 1.0)] {
            if index == 2 {
                output.set_paused(true).expect("pause old run before reset");
                thread::sleep(Duration::from_millis(30));
            }
            let old_sender = output.sender();
            let anchor = MediaTime::from_nanoseconds(index * 3_000_000_000);
            output
                .reprime(FORMAT, anchor, 0.0, rate, tempo_rate, true, || {})
                .expect("same native owner reset");
            assert_eq!(output.worker_id(), worker);
            assert!(matches!(old_sender.finish(), Err(AudioOutputError::Closed)));
            output.push(chunk(anchor, 144_000)).expect("new silence");
            // This finite single-frame fixture needs its End marker to flush
            // the existing atempo graph, just as a completed decoder feed does.
            output.finish().expect("finite silent fixture EOF");
            thread::sleep(Duration::from_millis(30));
            assert_eq!(output.position(), anchor, "paused clock remains anchored");
            assert!(output.try_event().is_none());
            output.set_paused(false).expect("resume");
            thread::sleep(Duration::from_millis(60));
            assert!(
                output.position() > anchor,
                "run {index}: position {:?}, event {:?}, worker finished {}",
                output.position(),
                output.try_event(),
                output.thread.as_ref().is_some_and(JoinHandle::is_finished)
            );
            assert!(output.position() < anchor.saturating_add(Duration::from_secs(1)));
            assert!(
                old_events.try_recv().is_err(),
                "retired callback must not be notified"
            );
        }
        let (notify, events) = mpsc::channel();
        let anchor = MediaTime::from_nanoseconds(12_000_000_000);
        output
            .reprime(FORMAT, anchor, 0.0, 1.0, 1.0, false, move || {
                let _ = notify.send(());
            })
            .expect("final reset");
        output.push(chunk(anchor, 4_800)).expect("final silence");
        output.finish().expect("final EOF");
        events
            .recv_timeout(Duration::from_secs(5))
            .expect("natural completion");
        assert_eq!(output.try_event(), Some(AudioOutputEvent::Drained));
        assert!(
            matches!(
                output.reprime(FORMAT, MediaTime::ZERO, 0.0, 1.0, 1.0, false, || {}),
                Err(AudioOutputError::Closed)
            ),
            "completed owners use ordinary startup"
        );
        output
            .set_paused(true)
            .expect("pause after natural completion");
        let worker = output.thread.take().expect("native owner");
        drop(output);
        worker.join().expect("native shutdown");
    }

    #[test]
    fn failed_disconnected_and_unacknowledged_reprime_remain_owned_until_drop() {
        for failure in ["error", "disconnected", "timeout"] {
            let (audio_tx, audio_rx) = mpsc::sync_channel(AUDIO_CHANNEL_CAPACITY);
            let (control_tx, control_rx) = mpsc::channel();
            let (_event_tx, event_rx) = mpsc::channel();
            let worker = thread::spawn(move || {
                let AudioControl::Reprime(run) = control_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("replacement request")
                else {
                    panic!("replacement required")
                };
                let pending = match failure {
                    "error" => {
                        run.ready_tx
                            .send(Err(AudioOutputError::EndpointChanged))
                            .expect("negative acknowledgement");
                        drop(run);
                        None
                    }
                    "disconnected" => {
                        drop(run);
                        None
                    }
                    "timeout" => Some(run),
                    _ => unreachable!(),
                };
                assert!(
                    matches!(
                        control_rx.recv_timeout(Duration::from_secs(5)),
                        Ok(AudioControl::Shutdown)
                    ),
                    "failed replacement must still be joined"
                );
                drop((pending, audio_rx));
            });
            let mut output = AudioOutput {
                format: FORMAT,
                audio_tx,
                control_tx,
                control_wake: Arc::new(ControlWake::default()),
                initial_audio_queued: Arc::new(AtomicBool::new(false)),
                event_rx,
                position_clock: Arc::new(PositionClock::new(MediaTime::from_nanoseconds(100))),
                paused_position: Cell::new(None),
                volume: Arc::new(AtomicU32::new(0)),
                drained: Arc::new(AtomicBool::new(false)),
                thread: Some(worker),
            };
            let sender = output.sender();
            let result = output.reprime(FORMAT, MediaTime::ZERO, 0.0, 1.0, 1.0, false, || {});
            assert!(match failure {
                "error" => matches!(result, Err(AudioOutputError::EndpointChanged)),
                _ => matches!(result, Err(AudioOutputError::Closed)),
            });
            assert_eq!(
                output.position(),
                MediaTime::from_nanoseconds(100),
                "unacknowledged generation is not published"
            );
            let worker = output.thread.take().expect("still-owned worker");
            drop(output);
            worker.join().expect("failure lifecycle assertions");
            assert!(matches!(sender.finish(), Err(AudioOutputError::Closed)));
        }
    }
}
