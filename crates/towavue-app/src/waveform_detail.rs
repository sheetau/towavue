use super::*;
use towavue_core::EditTimeline;

const SETTLE_TIME: Duration = Duration::from_millis(150);
pub(super) fn color() -> Color32 {
    chrome::FAINT
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Key {
    path: PathBuf,
    track: Option<towavue_core::AudioTrackId>,
    plan: EditTimeline,
    columns: u32,
}

#[cfg(test)]
mod hold_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use towavue_core::TimelineEdit;

    #[test]
    fn japanese_refinement_failure_keeps_stale_results_and_external_details_separate() {
        let Some(root) = crate::tests::isolated_test_root(
            "waveform_detail::tests::japanese_refinement_failure_keeps_stale_results_and_external_details_separate",
        ) else {
            return;
        };
        let detail = "native {detail} 日本語.wav 0x80004005";
        for density in [1.0, 1.25, 2.0] {
            for owned in [false, true] {
                let failure = || {
                    if owned {
                        towavue_runtime_windows::PreviewError::Message(
                            localization::Text::WaveformInvalidInput,
                        )
                    } else {
                        towavue_runtime_windows::PreviewError::Generate(detail.into())
                    }
                };
                let context = localization::test_ui::japanese_context(density);
                let mut app = Application::new(None, |_| {}).expect("app");
                app.ui_context = Some(context.clone());
                app.language_settings.next = localization::Language::English;
                let path = root.join("日本語{original}.wav");
                app.path = Some(path.clone());
                app.media_kind = Some(MediaKind::Audio);
                app.state = PlaybackState::Paused;
                app.tabs.open_new(path.clone(), MediaKind::Audio);
                let key = Arc::new(Key {
                    path,
                    track: None,
                    plan: EditTimeline::new(
                        media_time(Duration::from_secs(10)),
                        Default::default(),
                    )
                    .expect("plan"),

                    columns: 400,
                });
                app.waveform_detail = Detail {
                    key: Some(key.clone()),
                    started: true,
                    ..Default::default()
                };
                app.set_status("retained status".into());
                app.install_detailed_waveform(
                    app.media_generation,
                    Arc::new((*key).clone()),
                    Err(failure()),
                );
                assert_eq!(app.status_notice().as_deref(), Some("retained status"));
                assert!(!app.waveform_detail.finished);
                app.install_detailed_waveform(app.media_generation, key, Err(failure()));
                let reason = if owned {
                    "タイムライン波形のサイズ、音量、または速度が不正です"
                } else {
                    detail
                };
                let expected = format!(
                    "詳細な波形を取得できません: FFmpegのプレビュー生成に失敗しました: {reason}"
                );
                assert_eq!(app.status_notice().as_deref(), Some(expected.as_str()));
                assert!(app.waveform_detail.finished);
                assert!(app.edits.is_empty());
                let mut output = egui::FullOutput::default();
                for _ in 0..3 {
                    output = context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(1100.0, 600.0),
                            )),
                            ..Default::default()
                        },
                        |ui| {
                            app.draw_status_bar(ui, &mut Vec::new(), &mut Vec::new());
                        },
                    );
                }
                assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::Text(text) if text.galley.text() == expected && !text.galley.elided)));
            }
        }
    }

    #[test]
    fn gain_preview_scales_the_unclipped_envelope_only_inside_selection() {
        let range = towavue_core::TimeRange::new(
            media_time(Duration::from_millis(2500)),
            media_time(Duration::from_millis(7500)),
        )
        .expect("selection");
        let plan = EditTimeline::new(media_time(Duration::from_secs(10)), Default::default())
            .expect("plan");
        let detail = Detail {
            key: Some(Arc::new(Key {
                path: "fixture.wav".into(),
                track: None,
                plan,

                columns: 4,
            })),
            values: Some(Arc::from([0.1, 0.2, 1.5, 0.4])),
            ..Default::default()
        };
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 100.0));
        for density in [1.0, 1.25, 2.0] {
            for (preview, heights) in [
                (None, [10.0, 20.0, 100.0, 40.0]),
                (Some((range, 0.5)), [10.0, 10.0, 75.0, 40.0]),
                (Some((range, 2.0)), [10.0, 40.0, 100.0, 40.0]),
            ] {
                let mesh = detail.mesh(rect, density, preview).expect("mesh");
                for (index, height) in heights.into_iter().enumerate() {
                    let x = index as f32 * 100.0 + 50.0;
                    let bounds = mesh
                        .vertices
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|vertices| egui::Rect::from_points(&vertices.map(|vertex| vertex.pos)))
                        .find(|bounds| bounds.x_range().contains(x))
                        .expect("visible envelope");
                    assert!(
                        (bounds.height() - height).abs() <= 1.0 / density + 0.001,
                        "index={index}, density={density}, preview={preview:?}, expected={height}, bounds={bounds:?}"
                    );
                }
            }
        }
        assert_eq!(
            detail.values.as_deref(),
            Some([0.1, 0.2, 1.5, 0.4].as_slice())
        );
    }

    #[test]
    fn source_waveform_survives_zoom_edits_recovery_and_rejects_stale_sources() {
        use std::os::windows::process::CommandExt;
        let Some(root) = crate::tests::isolated_test_root(
            "waveform_detail::tests::source_waveform_survives_zoom_edits_recovery_and_rejects_stale_sources",
        ) else {
            return;
        };
        let path = root.join("tone.wav");
        let ffmpeg =
            PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg")).join("bin/ffmpeg.exe");
        assert!(
            std::process::Command::new(ffmpeg)
                .creation_flags(0x0800_0000)
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:sample_rate=48000:duration=1",
                    "-c:a",
                    "pcm_s16le"
                ])
                .arg(&path)
                .status()
                .expect("fixture")
                .success()
        );
        let (send, receive) = std::sync::mpsc::channel();
        let mut app = Application::new(None, move |event| {
            let _ = send.send(event);
        })
        .expect("app");
        let context = fonts::test_context();
        app.ui_context = Some(context.clone());
        let tab = app.tabs.open_new(path.clone(), MediaKind::Audio);
        app.path = Some(path);
        app.media_kind = Some(MediaKind::Audio);
        app.media_duration = Some(Duration::from_secs(1));
        app.state = PlaybackState::Paused;
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 80.0));
        app.detailed_waveform(&context, rect, None);
        app.waveform_detail.changed = Some(Instant::now() - SETTLE_TIME);
        app.detailed_waveform(&context, rect, None);
        let AppEvent::DetailedWaveform(generation, key, result) = receive
            .recv_timeout(Duration::from_secs(10))
            .expect("source peaks")
        else {
            panic!("waveform event")
        };
        let values = result.expect("native peaks");
        assert_eq!(values.len(), 1024);
        assert!(values.iter().all(|v| *v > 0.02 && *v < 0.2));
        app.install_detailed_waveform(generation.wrapping_add(1), key.clone(), Ok(values.clone()));
        assert!(app.waveform_detail.values.is_none());
        app.install_detailed_waveform(generation, Arc::new((*key).clone()), Ok(values.clone()));
        assert!(
            app.waveform_detail.values.is_none(),
            "obsolete request identity"
        );
        app.install_detailed_waveform(generation, key.clone(), Ok(values));
        let retained = app.waveform_detail.values.clone().expect("installed");
        for density in [1.0, 1.25, 2.0] {
            context.set_pixels_per_point(density);
            for width in [200.0, 640.0, 20_000.0] {
                let full = egui::Rect::from_min_size(
                    egui::pos2(-width * 0.2, 0.0),
                    egui::vec2(width, 80.0),
                );
                let mesh = app
                    .detailed_waveform_view(&context, full, rect, None, None, None)
                    .expect("retained mesh");
                assert!(!mesh.vertices.is_empty());
                assert!(mesh.vertices.len() <= ((rect.width() * density).ceil() as usize + 2) * 4);
                assert!(Arc::ptr_eq(
                    &retained,
                    app.waveform_detail
                        .values
                        .as_ref()
                        .expect("source unchanged")
                ));
                assert!(Arc::ptr_eq(
                    &key,
                    app.waveform_detail.key.as_ref().expect("no new request")
                ));
            }
        }
        for operation in [
            TimelineEdit::ScaleVolume(
                towavue_core::TimeRange::new(
                    MediaTime::ZERO,
                    media_time(Duration::from_millis(500)),
                )
                .expect("range"),
                0.0,
            ),
            TimelineEdit::Stretch(
                towavue_core::TimeRange::new(
                    MediaTime::ZERO,
                    media_time(Duration::from_millis(500)),
                )
                .expect("range"),
                media_time(Duration::from_millis(750)),
            ),
            TimelineEdit::Delete(
                towavue_core::TimeRange::new(
                    MediaTime::ZERO,
                    media_time(Duration::from_millis(250)),
                )
                .expect("range"),
            ),
        ] {
            app.edits
                .entry(tab)
                .or_default()
                .push(EditOperation::Timeline(operation), MediaKind::Audio);
            assert!(app.detailed_waveform(&context, rect, None).is_some());
            assert!(Arc::ptr_eq(
                &key,
                app.waveform_detail
                    .key
                    .as_ref()
                    .expect("edit does not decode")
            ));
        }
        let retained_detail = app.waveform_detail.take_retained();
        app.waveform_detail = retained_detail;
        app.graphics_epoch += 1;
        assert!(app.detailed_waveform(&context, rect, None).is_some());
        assert!(Arc::ptr_eq(
            &retained,
            app.waveform_detail.values.as_ref().expect("CPU data")
        ));
        assert!(
            receive.try_recv().is_err(),
            "no work submitted for view/edit changes"
        );
        app.path = Some(root.join("another.wav"));
        assert!(app.detailed_waveform(&context, rect, None).is_none());
        app.install_detailed_waveform(generation, key, Ok(vec![1.0; 1024]));
        assert!(
            app.waveform_detail.values.is_none(),
            "old source cannot install"
        );
        app.waveform_worker.clear();
    }

    #[test]
    fn trim_reuses_source_waveform_without_starting_an_overview_job() {
        let Some(root) = crate::tests::isolated_test_root(
            "waveform_detail::tests::trim_reuses_source_waveform_without_starting_an_overview_job",
        ) else {
            return;
        };
        for kind in [MediaKind::Audio, MediaKind::Video] {
            let mut app = Application::new(None, |_| {}).expect("app");
            let context = fonts::test_context();
            app.ui_context = Some(context.clone());
            let path = root.join("retained-source.wav");
            app.tabs.open_new(path.clone(), kind);
            app.path = Some(path);
            app.media_kind = Some(kind);
            app.media_duration = Some(Duration::from_secs(10));
            app.state = PlaybackState::Paused;
            app.waveform = Some(context.load_texture(
                "retained overview",
                egui::ColorImage::filled([16, 4], Color32::WHITE),
                TextureOptions::LINEAR,
            ));
            let texture = app.waveform.as_ref().expect("overview").id();
            let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 96.0));
            app.detailed_waveform(&context, rect, None);
            for operation in [
                EditOperation::SetTrimStart(media_time(Duration::from_secs(2))),
                EditOperation::SetTrimEnd(media_time(Duration::from_secs(8))),
            ] {
                let previous = app.waveform_detail.key.clone().expect("original plan");
                app.push_edit(operation);
                assert!(app.timeline_open);
                assert!(
                    !app.waveform_loading,
                    "trim must reuse the original overview"
                );
                assert_eq!(app.waveform.as_ref().expect("same overview").id(), texture);
                app.detailed_waveform(&context, rect, None);
                assert_eq!(app.waveform_detail.key.as_ref(), Some(&previous));
                assert!(
                    !app.waveform_detail.started,
                    "source preparation still waits for its initial settle time"
                );
            }
            assert_eq!(
                app.edit_state().trim_start,
                Some(media_time(Duration::from_secs(2)))
            );
            assert_eq!(
                app.edit_state().trim_end,
                Some(media_time(Duration::from_secs(8)))
            );
            // Navigation/recovery removes this texture; an absent overview must still load.
            app.waveform = None;
            app.load_waveform();
            assert!(app.waveform_loading);
            app.waveform_worker.clear();
        }
    }
}

#[derive(Default)]
pub(super) struct Detail {
    key: Option<Arc<Key>>,
    changed: Option<Instant>,
    started: bool,
    finished: bool,
    values: Option<Arc<[f32]>>,
    levels: Vec<Arc<[f32]>>,
    display_plan: Option<EditTimeline>,
    display_volume: f32,
}

impl Detail {
    fn install(&mut self, values: Vec<f32>) {
        self.levels.clear();
        let mut level: Arc<[f32]> = values.into();
        self.values = Some(level.clone());
        while level.len() > 1 {
            level = level
                .chunks(2)
                .map(|pair| pair.iter().copied().fold(0.0, f32::max))
                .collect::<Vec<_>>()
                .into();
            self.levels.push(level.clone());
        }
    }

    fn peak(&self, from: f64, to: f64) -> f32 {
        let Some(values) = self.values.as_deref() else {
            return 0.0;
        };
        if values.is_empty() {
            return 0.0;
        }
        let begin = (from * values.len() as f64 + 1e-9).floor().max(0.0) as usize;
        let end = (to * values.len() as f64 - 1e-9).ceil().max(0.0) as usize;
        let mut begin = begin.min(values.len() - 1);
        let end = end.max(begin + 1).min(values.len());
        let mut peak = 0.0_f32;
        // Exact range maximum from aligned power-of-two blocks. Cost grows
        // logarithmically with source bins, not the duration of the visible media.
        while begin < end {
            let remaining = end - begin;
            let available = (usize::BITS - 1 - remaining.leading_zeros()) as usize;
            let aligned = if begin == 0 {
                available
            } else {
                begin.trailing_zeros() as usize
            };
            let level = available.min(aligned).min(self.levels.len());
            peak = peak.max(if level == 0 {
                values[begin]
            } else {
                self.levels[level - 1][begin >> level]
            });
            begin += 1 << level;
        }
        peak
    }

    pub(super) fn is_pending(&self) -> bool {
        self.started && !self.finished
    }
    pub(super) fn restart_pending(&mut self) {
        if self.values.is_none() {
            self.started = false;
            self.changed = Some(Instant::now());
        }
    }
    pub(super) fn take_retained(&mut self) -> Self {
        let mut saved = std::mem::take(self);
        saved.restart_pending();
        saved
    }

    #[cfg(test)]
    fn mesh(
        &self,
        rect: egui::Rect,
        density: f32,
        preview: Option<(towavue_core::TimeRange, f32)>,
    ) -> Option<egui::Mesh> {
        self.mesh_view(rect, rect, density, preview, None)
    }

    fn mesh_view(
        &self,
        rect: egui::Rect,
        clip: egui::Rect,
        density: f32,
        preview: Option<(towavue_core::TimeRange, f32)>,
        viewport: Option<crate::timeline_view::View>,
    ) -> Option<egui::Mesh> {
        self.values.as_ref()?;
        let key = self.key.as_ref()?;
        let plan = self.display_plan.as_ref().unwrap_or(&key.plan);
        let volume = if self.display_plan.is_some() {
            self.display_volume
        } else {
            1.0
        };
        let duration = plan.duration().as_seconds_f64();
        let source_duration = key.plan.duration().as_seconds_f64();
        let (start, span_seconds) =
            viewport.map_or((0.0, duration), |view| (view.start, view.span));
        let mut mesh = egui::Mesh::default();
        let mut offset = 0.0;
        let mut last: Option<(egui::Rect, f32)> = None;
        for span in plan.spans() {
            let end = offset + span.duration().as_seconds_f64();
            let span_left =
                f64::from(rect.left()) + f64::from(rect.width()) * (offset - start) / span_seconds;
            let span_right =
                f64::from(rect.left()) + f64::from(rect.width()) * (end - start) / span_seconds;
            let left = span_left.max(f64::from(clip.left()));
            let right = span_right.min(f64::from(clip.right()));
            if right > left {
                let first = (left * f64::from(density)).floor() as i64;
                let last_pixel = (right * f64::from(density)).ceil() as i64;
                for pixel in first..last_pixel {
                    let x0 = (pixel as f64 / f64::from(density)).max(left);
                    let x1 = ((pixel + 1) as f64 / f64::from(density)).min(right);
                    let time = |x: f64| {
                        offset + (x - span_left) / (span_right - span_left) * (end - offset)
                    };
                    let mut cuts = [x0, x1, x1, x1];
                    let mut cut_count = 2;
                    if let Some((range, _)) = preview {
                        for boundary in [range.start(), range.end()] {
                            let x = f64::from(rect.left())
                                + f64::from(rect.width()) * (boundary.as_seconds_f64() - start)
                                    / span_seconds;
                            if x > x0 && x < x1 {
                                cuts[cut_count] = x;
                                cut_count += 1;
                            }
                        }
                    }
                    cuts[..cut_count].sort_by(f64::total_cmp);
                    for pair in cuts[..cut_count].windows(2) {
                        let source = |x| {
                            (span.source().start().as_seconds_f64()
                                + (time(x) - offset) * span.rate())
                                / source_duration
                        };
                        let factor = preview
                            .filter(|(range, _)| {
                                time((pair[0] + pair[1]) * 0.5) >= range.start().as_seconds_f64()
                                    && time((pair[0] + pair[1]) * 0.5)
                                        < range.end().as_seconds_f64()
                            })
                            .map_or(1.0, |(_, factor)| factor);
                        let value = (self.peak(source(pair[0]), source(pair[1]))
                            * span.volume()
                            * volume
                            * factor)
                            .min(1.0);
                        let half = (rect.height() * value * 0.5 * density).round() / density;
                        let bar = egui::Rect::from_min_max(
                            egui::pos2(
                                pair[0] as f32,
                                (rect.center().y * density).round() / density - half,
                            ),
                            egui::pos2(
                                pair[1] as f32,
                                (rect.center().y * density).round() / density + half,
                            ),
                        );
                        if let Some((previous, previous_value)) = &mut last
                            && *previous_value == value
                            && (previous.right() - bar.left()).abs() < 0.001
                        {
                            previous.max.x = bar.right();
                        } else {
                            if let Some((bar, _)) = last.take()
                                && bar.is_positive()
                            {
                                mesh.add_colored_rect(bar, color());
                            }
                            last = Some((bar, value));
                        }
                    }
                }
            }
            offset = end;
        }
        if let Some((bar, _)) = last
            && bar.is_positive()
        {
            mesh.add_colored_rect(bar, color());
        }
        Some(mesh)
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    #[cfg(any(test, feature = "presentation-verification"))]
    pub(super) fn verification_complete_waveform(&mut self, refined: bool) -> u32 {
        let detail = &mut self.waveform_detail;
        let columns = detail.key.as_ref().expect("laid-out waveform key").columns;
        detail.started = true;
        detail.finished = true;
        if refined {
            detail.install(
                (0..columns)
                    .map(|column| 0.1 + (column as f32 * 0.037).sin().abs() * 0.9)
                    .collect(),
            );
        }
        columns
    }

    #[cfg(feature = "presentation-verification")]
    pub(super) fn verification_waveform_shape(&self) -> (u32, usize) {
        (
            self.waveform_detail
                .key
                .as_ref()
                .expect("waveform key")
                .columns,
            self.waveform_detail
                .values
                .as_deref()
                .map_or(0, <[f32]>::len),
        )
    }

    pub(super) fn draw_waveform_activity(&self, ui: &mut egui::Ui, rect: egui::Rect) {
        let language = localization::language(ui.ctx());
        let label = if self.waveform_loading {
            localization::Text::LoadingWaveform.in_language(language)
        } else if self.waveform_detail.is_pending() {
            localization::Text::RefiningWaveform.in_language(language)
        } else {
            return;
        };
        ui.put(
            egui::Rect::from_center_size(
                rect.center(),
                egui::vec2((rect.width() - 48.0).max(0.0), 20.0_f32.min(rect.height())),
            ),
            egui::Label::new(
                egui::RichText::new(label)
                    .size(time_selection::LABEL_SIZE)
                    .color(chrome::FOREGROUND)
                    .background_color(chrome::BORDER),
            )
            .truncate()
            .show_tooltip_when_elided(false)
            .selectable(false),
        );
    }

    pub(super) fn clear_detailed_waveform(&mut self) {
        if self.waveform_detail.started && !self.waveform_loading {
            self.waveform_worker.clear();
        }
        self.waveform_detail = Detail::default();
    }

    #[cfg(test)]
    pub(super) fn detailed_waveform(
        &mut self,
        context: &egui::Context,
        rect: egui::Rect,
        gain_preview: Option<(towavue_core::TimeRange, f32)>,
    ) -> Option<egui::Mesh> {
        self.detailed_waveform_view(context, rect, rect, gain_preview, None, None)
    }

    pub(super) fn detailed_waveform_view(
        &mut self,
        context: &egui::Context,
        rect: egui::Rect,
        clip: egui::Rect,
        gain_preview: Option<(towavue_core::TimeRange, f32)>,
        stretch: Option<(towavue_core::TimeRange, MediaTime)>,
        viewport: Option<crate::timeline_view::View>,
    ) -> Option<egui::Mesh> {
        if self.source_save.frozen {
            return self.waveform_detail.mesh_view(
                rect,
                clip,
                context.pixels_per_point(),
                gain_preview,
                viewport,
            );
        }
        let path = self.path.as_ref()?;
        let state = self.edit_state();
        let source_plan = EditTimeline::new(media_time(self.media_duration?), Default::default())?;
        let mut plan = self
            .session
            .as_ref()
            .and_then(PlaybackSession::timeline)
            .cloned()
            .or(self.history_timeline().ok()?)
            .or_else(|| {
                EditTimeline::new(media_time(self.media_duration?), state.playback_range())
            })?;
        if let Some((range, length)) = stretch {
            plan.apply(towavue_core::TimelineEdit::Stretch(range, length));
        }
        // 240 bins/second gives eight source bins per 30 fps frame. Long sources
        // use a bounded coarser envelope; zooming never causes another decode.
        let columns = (source_plan.duration().as_seconds_f64() * 240.0)
            .ceil()
            .clamp(1024.0, 262_144.0) as u32;
        let track = self.waveform_audio_track();
        let matches =
            self.waveform_detail.key.as_ref().is_some_and(|key| {
                key.path == *path && key.track == track && key.plan == source_plan
            });
        if !matches {
            if self.waveform_detail.started && !self.waveform_loading {
                self.waveform_worker.clear();
            }
            self.waveform_detail = Detail {
                key: Some(Arc::new(Key {
                    path: path.clone(),
                    track,
                    plan: source_plan,

                    columns,
                })),
                changed: Some(Instant::now()),
                ..Default::default()
            };
        }
        let input = self.media_input(path);
        let detail = &mut self.waveform_detail;
        detail.display_plan = Some(plan);
        detail.display_volume = state.volume;
        if !detail.started
            && !self.waveform_loading
            && gain_preview.is_none()
            && !crate::timeline_input::is_active(context)
        {
            let remaining =
                SETTLE_TIME.saturating_sub(detail.changed.expect("key timestamp").elapsed());
            if remaining.is_zero() {
                let key = Arc::new(detail.key.as_deref().expect("requested key").clone());
                detail.key = Some(key.clone());
                let notify = self.notify.clone();
                let generation = self.media_generation;
                detail.started = true;
                detail.finished = false;
                self.waveform_worker.submit(move |cancellation| {
                    let result = towavue_runtime_windows::source_audio_track_waveform(
                        input.path(),
                        key.track,
                        key.plan.duration(),
                        key.columns,
                        &cancellation,
                    );
                    notify(AppEvent::DetailedWaveform(generation, key, result));
                });
            } else {
                context.request_repaint_after(remaining);
            }
        }
        detail.mesh_view(
            rect,
            clip,
            context.pixels_per_point(),
            gain_preview,
            viewport,
        )
    }

    pub(super) fn install_detailed_waveform(
        &mut self,
        generation: u64,
        key: Arc<Key>,
        result: Result<Vec<f32>, towavue_runtime_windows::PreviewError>,
    ) {
        if generation != self.media_generation
            || self.path.as_ref() != Some(&key.path)
            || self.waveform_audio_track() != key.track
            || !self
                .waveform_detail
                .key
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &key))
            || !self.waveform_detail.started
        {
            return;
        }
        self.waveform_detail.finished = true;
        match result {
            Ok(values) => self.waveform_detail.install(values),
            Err(error) => {
                let language = self.language();
                self.set_status(
                    towavue_core::localization::formatted::detailed_waveform_failed(
                        language,
                        &error.message(language),
                    ),
                );
            }
        }
        self.request_redraw();
    }
}
