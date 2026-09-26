//! Offscreen Release measurements of the actual timeline UI and tessellation.
//! No window, renderer, media decoder, seek dispatch or system clipboard is used.
use super::*;
use std::fmt::Write as _;

type App = Application<fn(AppEvent)>;
const SAMPLES: usize = 360;

struct Measurement {
    ui_us: f64,
    tessellation_us: f64,
    vertices: usize,
    seeks: usize,
    selections: usize,
}

fn frame(app: &mut App, size: egui::Vec2, events: Vec<egui::Event>) -> Measurement {
    let context = app.ui_context.clone().expect("verification context");
    let mut actions = Vec::new();
    let density = context.data(|data| {
        data.get_temp::<f32>(egui::Id::new("timeline-verification-density"))
            .expect("native density")
    });
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
        events,
        ..Default::default()
    };
    input
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .native_pixels_per_point = Some(density);
    let start = Instant::now();
    let output = context.run_ui(input, |ui| app.draw_timeline(ui, &mut actions));
    let ui_us = start.elapsed().as_secs_f64() * 1_000_000.0;
    let start = Instant::now();
    let primitives = context.tessellate(output.shapes, output.pixels_per_point);
    let tessellation_us = start.elapsed().as_secs_f64() * 1_000_000.0;
    let vertices = primitives
        .iter()
        .map(|primitive| match &primitive.primitive {
            egui::epaint::Primitive::Mesh(mesh) => mesh.vertices.len(),
            egui::epaint::Primitive::Callback(_) => 0,
        })
        .sum();
    let seeks = actions
        .iter()
        .filter(|action| matches!(action, UiAction::Seek(_)))
        .count();
    let selections = actions
        .iter()
        .filter(|action| matches!(action, UiAction::TimeSelection(..)))
        .count();
    std::hint::black_box(primitives);
    Measurement {
        ui_us,
        tessellation_us,
        vertices,
        seeks,
        selections,
    }
}

fn button(position: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn percentile(values: &mut [f64], fraction: f64) -> f64 {
    values.sort_by(f64::total_cmp);
    values[((values.len() - 1) as f64 * fraction).round() as usize]
}

fn setup(
    root: &Path,
    width: f32,
    density: f32,
    mode: &str,
    playing: bool,
) -> Result<(App, egui::Vec2, u32), Box<dyn Error>> {
    let mut app = Application::new_with_preview_cache(
        None,
        (|_| {}) as fn(AppEvent),
        PreviewCache::local()?,
    )?;
    let path = root.join("unopened-verification.wav");
    let tab = app.tabs.open_new(path.clone(), MediaKind::Audio);
    app.path = Some(path);
    app.displayed_tab = Some(tab);
    app.media_kind = Some(MediaKind::Audio);
    app.media_duration = Some(Duration::from_secs(600));
    app.timeline_open = true;
    app.state = if playing {
        PlaybackState::Playing
    } else {
        PlaybackState::Paused
    };
    let position = media_time(Duration::from_secs(300));
    app.clock = Some(if playing {
        PlaybackClock::new(position, 1.0)
    } else {
        PlaybackClock::paused(position, 1.0)
    });
    // Suppress refinement submission until the first real layout has established its key.
    app.waveform_loading = true;
    let context = egui::Context::default();
    fonts::install(&context);
    context.global_style_mut(chrome::style);
    context
        .data_mut(|data| data.insert_temp(egui::Id::new("timeline-verification-density"), density));
    if mode != "none" {
        app.waveform = Some(context.load_texture(
            "verification-overview",
            egui::ColorImage::filled([1024, 64], Color32::WHITE),
            TextureOptions::LINEAR,
        ));
    }
    app.ui_context = Some(context);
    let size = egui::vec2(width, 600.0);
    for _ in 0..30 {
        frame(&mut app, size, vec![]);
    }
    let columns = app.verification_complete_waveform(mode == "refined");
    app.waveform_loading = false;
    for _ in 0..30 {
        frame(&mut app, size, vec![]);
    }
    if app.verification_waveform_shape()
        != (
            columns,
            if mode == "refined" {
                columns as usize
            } else {
                0
            },
        )
    {
        return Err("settled waveform did not retain its requested detail".into());
    }
    if (app.ui_context.as_ref().expect("context").pixels_per_point() - density).abs() > 0.001 {
        return Err("native density was not applied".into());
    }
    Ok((app, size, columns))
}

pub(super) fn run() -> Result<(), Box<dyn Error>> {
    if cfg!(debug_assertions) {
        return Err("Timeline CPU verification requires Release".into());
    }
    let root = PathBuf::from(
        std::env::var_os("TOWAVUE_TIMELINE_VERIFY_ROOT")
            .ok_or("explicit isolated root required")?,
    );
    if !root.is_absolute() {
        return Err("verification root must be absolute".into());
    }
    for (variable, directory) in [("APPDATA", "config"), ("LOCALAPPDATA", "local")] {
        if std::env::var_os(variable).map(PathBuf::from) != Some(root.join(directory)) {
            return Err("verification configuration must be isolated".into());
        }
    }
    std::fs::create_dir_all(&root)?;
    let mut csv = String::from(
        "mode,width,density,clock,direction,phase,columns,samples,ui_median_us,ui_p95_us,tess_median_us,tess_p95_us,max_vertices,seeks,selections\n",
    );
    for width in [640.0, 1920.0, 3840.0] {
        for density in [1.0, 2.0] {
            for mode in ["none", "overview", "refined"] {
                for playing in [false, true] {
                    for reverse in [false, true] {
                        let (mut app, size, columns) = setup(&root, width, density, mode, playing)?;
                        let context = app.ui_context.clone().expect("context");
                        let outer = egui::containers::panel::PanelState::load(
                            &context,
                            app.timeline_panel_id(),
                        )
                        .ok_or("missing panel layout")?
                        .outer_rect;
                        let left = outer.left() + 8.0;
                        let right = outer.right() - 8.0;
                        let y = outer.top() + 8.0 + (outer.height() - 8.0) * 0.72;
                        let at = |fraction: f32| egui::pos2(left + (right - left) * fraction, y);
                        let (from, to) = if reverse { (0.78, 0.18) } else { (0.18, 0.78) };
                        for phase in ["idle", "drag"] {
                            if phase == "drag" {
                                frame(
                                    &mut app,
                                    size,
                                    vec![
                                        egui::Event::PointerMoved(at(from)),
                                        button(at(from), true),
                                    ],
                                );
                                frame(
                                    &mut app,
                                    size,
                                    vec![egui::Event::PointerMoved(at(from + (to - from) * 0.01))],
                                );
                                if !crate::timeline_input::is_active(&context) {
                                    return Err(
                                        "selection drag did not acquire the timeline".into()
                                    );
                                }
                            }
                            let mut measurements = Vec::with_capacity(SAMPLES);
                            for index in 0..SAMPLES {
                                let events = if phase == "drag" {
                                    let fraction = (index + 1) as f32 / (SAMPLES + 1) as f32;
                                    vec![egui::Event::PointerMoved(at(
                                        from + (to - from) * fraction
                                    ))]
                                } else {
                                    vec![]
                                };
                                measurements.push(frame(&mut app, size, events));
                                if app.verification_waveform_shape()
                                    != (
                                        columns,
                                        if mode == "refined" {
                                            columns as usize
                                        } else {
                                            0
                                        },
                                    )
                                {
                                    return Err("measurement changed its waveform detail".into());
                                }
                            }
                            let mut ui: Vec<_> = measurements.iter().map(|m| m.ui_us).collect();
                            let mut tess: Vec<_> =
                                measurements.iter().map(|m| m.tessellation_us).collect();
                            let seeks: usize = measurements.iter().map(|m| m.seeks).sum();
                            let selections: usize = measurements.iter().map(|m| m.selections).sum();
                            if phase == "drag" && (seeks != 0 || selections != 0) {
                                return Err(
                                    "held selection unexpectedly dispatched a seek or commit"
                                        .into(),
                                );
                            }
                            writeln!(
                                csv,
                                "{mode},{width},{density},{playing},{reverse},{phase},{columns},{SAMPLES},{:.3},{:.3},{:.3},{:.3},{},{seeks},{selections}",
                                percentile(&mut ui, 0.5),
                                percentile(&mut ui, 0.95),
                                percentile(&mut tess, 0.5),
                                percentile(&mut tess, 0.95),
                                measurements.iter().map(|m| m.vertices).max().unwrap_or(0)
                            )?;
                        }
                        let release = frame(&mut app, size, vec![button(at(to), false)]);
                        if release.selections != 1 || crate::timeline_input::is_active(&context) {
                            return Err("selection release did not commit exactly once".into());
                        }
                    }
                }
            }
        }
    }
    std::fs::write(root.join("timeline-cpu.csv"), csv)?;
    Ok(())
}
