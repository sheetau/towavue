use std::sync::Arc;

use egui::{Context, Id, LayerId, TextureHandle};

pub(crate) const DURATION: f64 = 0.12;

#[derive(Clone, Copy)]
struct Motion {
    from: f32,
    target: bool,
    started: f64,
}

impl Motion {
    fn new(now: f64) -> Self {
        Self {
            from: 0.0,
            target: false,
            started: now,
        }
    }

    fn value(self, now: f64) -> f32 {
        egui::lerp(
            self.from..=f32::from(self.target),
            ((now - self.started) / DURATION).clamp(0.0, 1.0) as f32,
        )
    }

    fn advance(&mut self, context: &Context, target: bool) -> f32 {
        let now = context.input(|input| input.time);
        let value = self.value(now);
        if self.target != target {
            self.from = value;
            self.started = now;
            self.target = target;
        }
        if value != f32::from(target) {
            context.request_repaint();
        }
        value
    }
}

pub(crate) fn transition(context: &Context, id: Id, target: bool) -> f32 {
    let mut motion = context
        .data(|data| data.get_temp::<Motion>(id))
        .unwrap_or_else(|| Motion::new(context.input(|input| input.time)));
    let value = motion.advance(context, target);
    context.data_mut(|data| data.insert_temp(id, motion));
    value
}

struct Snapshot {
    shapes: Vec<egui::epaint::ClippedShape>,
    // Shape texture IDs alone do not own their pixels. Keep managed textures
    // alive after the source card/thumbnail cache releases its handles.
    _textures: Vec<TextureHandle>,
}

#[derive(Clone)]
struct Card {
    layer: LayerId,
    pass: u64,
    motion: Motion,
    snapshot: Arc<Snapshot>,
}

#[derive(Clone, Default)]
struct Cards([Option<Card>; 2]);

fn state_id(context: &Context) -> Id {
    Id::new(("media-card-fades", context.viewport_id()))
}

pub(super) fn record(context: &Context, layer: LayerId, seek: bool) {
    let installed = Id::new("media-card-fade-hook");
    if !context.data(|data| data.get_temp::<bool>(installed).unwrap_or(false)) {
        context.on_end_pass("media card fades", Arc::new(|ui| finish(ui.ctx())));
        context.data_mut(|data| data.insert_temp(installed, true));
    }
    let shapes = context.graphics(|graphics| {
        graphics
            .get(layer)
            .map(|list| list.all_entries().cloned().collect::<Vec<_>>())
            .unwrap_or_default()
    });
    let mut ids = Vec::new();
    fn textures(shape: &egui::Shape, ids: &mut Vec<egui::TextureId>) {
        if let egui::Shape::Vec(shapes) = shape {
            for shape in shapes {
                textures(shape, ids);
            }
        } else {
            let id = shape.texture_id();
            if matches!(id, egui::TextureId::Managed(value) if value != 0) && !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    for shape in &shapes {
        textures(&shape.shape, &mut ids);
    }
    let manager = context.tex_manager();
    let textures = ids
        .into_iter()
        .filter_map(|id| {
            let mut textures = manager.write();
            textures.meta(id)?;
            textures.retain(id);
            Some(TextureHandle::new(Arc::clone(&manager), id))
        })
        .collect();
    let now = context.input(|input| input.time);
    let id = state_id(context);
    let pass = context.cumulative_pass_nr();
    context.data_mut(|data| {
        let cards = data.get_temp_mut_or_default::<Cards>(id);
        let slot = &mut cards.0[usize::from(seek)];
        let motion = slot
            .as_ref()
            .filter(|card| card.layer == layer)
            .map_or_else(|| Motion::new(now), |card| card.motion);
        *slot = Some(Card {
            layer,
            pass,
            motion,
            snapshot: Arc::new(Snapshot {
                shapes,
                _textures: textures,
            }),
        });
    });
}

pub(crate) fn cancel(context: &Context, source: Id) {
    let id = state_id(context);
    context.data_mut(|data| {
        if let Some(mut cards) = data.get_temp::<Cards>(id) {
            for slot in &mut cards.0 {
                if slot.as_ref().is_some_and(|card| card.layer.id == source) {
                    *slot = None;
                }
            }
            data.insert_temp(id, cards);
        }
    });
}

fn finish(context: &Context) {
    let id = state_id(context);
    let Some(mut cards) = context.data(|data| data.get_temp::<Cards>(id)) else {
        return;
    };
    let blocked = egui::Popup::is_any_open(context)
        || context.input(|input| !input.raw.hovered_files.is_empty());
    for slot in &mut cards.0 {
        let Some(card) = slot else {
            continue;
        };
        let visible = card.pass == context.cumulative_pass_nr();
        let opacity = card.motion.advance(context, visible);
        if blocked || (!visible && opacity == 0.0) {
            *slot = None;
            continue;
        }
        // A departing card is paint only: no Area, widgets, accessibility nodes,
        // transport actions, decoding requests or hover ownership survive it.
        let layer = if visible {
            card.layer
        } else {
            LayerId::new(egui::Order::Tooltip, card.layer.id.with("departing"))
        };
        let mut painter = context.layer_painter(layer);
        painter.set_opacity(opacity);
        for (index, shape) in card.snapshot.shapes.iter().enumerate() {
            let painter = painter.with_clip_rect(shape.clip_rect);
            if visible {
                painter.set(egui::layers::ShapeIdx(index), shape.shape.clone());
            } else {
                painter.add(shape.shape.clone());
            }
        }
    }
    context.data_mut(|data| data.insert_temp(id, cards));
}

#[cfg(test)]
mod tests {

    #[test]
    fn cards_fade_in_and_out_without_rebuilding_content_and_keep_texture_ownership() {
        for seek in [false, true] {
            for density in [1.0, 1.25, 2.0] {
                let context = crate::fonts::test_context();
                context.set_pixels_per_point(density);
                let texture = context.load_texture(
                    "fade fixture",
                    egui::ColorImage::filled([8, 4], egui::Color32::WHITE),
                    egui::TextureOptions::LINEAR,
                );
                let texture_id = texture.id();
                let frame = |time, present| {
                    context.run_ui(
                        egui::RawInput {
                            time: Some(time),
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(500.0, 400.0),
                            )),
                            events: vec![egui::Event::PointerMoved(egui::pos2(90.0, 200.0))],
                            ..Default::default()
                        },
                        |ui| {
                            let response = ui.interact(
                                egui::Rect::from_min_max(
                                    egui::pos2(50.0, 190.0),
                                    egui::pos2(150.0, 210.0),
                                ),
                                egui::Id::new("fade source"),
                                egui::Sense::hover(),
                            );
                            if present {
                                let preview = if seek {
                                    super::super::Preview::seek(&response, 0.5)
                                } else {
                                    super::super::Preview::tab(&response)
                                };
                                preview.show(|ui| {
                                    ui.image((texture_id, egui::vec2(80.0, 40.0)));
                                    ui.label("Retained caption");
                                });
                            }
                            if context.current_pass_index() == 0 {
                                context.request_discard("fade multipass");
                            }
                        },
                    )
                };
                let mesh = |output: &egui::FullOutput| {
                    context
                        .tessellate(output.shapes.clone(), context.pixels_per_point())
                        .into_iter()
                        .find_map(|shape| match shape.primitive {
                            egui::epaint::Primitive::Mesh(mesh)
                                if mesh.texture_id == texture_id =>
                            {
                                Some(mesh)
                            }
                            _ => None,
                        })
                };
                frame(0.0, false);
                frame(0.1, false);
                frame(1.0, true);
                let rising = mesh(&frame(1.06, true)).expect("fading image");
                assert!(
                    (i32::from(
                        rising
                            .vertices
                            .iter()
                            .map(|v| v.color.a())
                            .max()
                            .expect("vertices")
                    ) - 128)
                        .abs()
                        <= 2
                );
                let full = mesh(&frame(1.2, true)).expect("full image");
                assert_eq!(
                    full.vertices
                        .iter()
                        .map(|v| v.color.a())
                        .max()
                        .expect("vertices"),
                    255
                );
                frame(2.0, false);
                drop(texture);
                assert!(
                    context.tex_manager().read().meta(texture_id).is_some(),
                    "departure owns the texture"
                );
                let falling =
                    mesh(&frame(2.06, false)).expect("departing image even without a Preview call");
                assert!(
                    (i32::from(
                        falling
                            .vertices
                            .iter()
                            .map(|v| v.color.a())
                            .max()
                            .expect("vertices")
                    ) - 128)
                        .abs()
                        <= 2
                );
                assert_eq!(falling.calc_bounds(), full.calc_bounds());
                assert_eq!(
                    falling.vertices.iter().map(|v| v.uv).collect::<Vec<_>>(),
                    full.vertices.iter().map(|v| v.uv).collect::<Vec<_>>()
                );
                assert!(mesh(&frame(2.2, false)).is_none());
                assert!(
                    context.tex_manager().read().meta(texture_id).is_none(),
                    "finished departure releases pixels"
                );
            }
        }
    }
}
