use crate::*;

pub(crate) mod badge;

#[cfg(test)]
pub(crate) mod tests;

#[derive(Clone)]
struct Drag {
    tab: TabId,
    widget: egui::Id,
    detached_anchor: egui::Vec2,
    origin: egui::Pos2,
    pointer: egui::Pos2,
    crossed: bool,
    tabs: Vec<(TabId, Option<PathBuf>)>,
    source: (Option<TabId>, u64, u64),
    screen: egui::Rect,
    density: f32,
}

#[derive(Clone, Default)]
struct State {
    widgets: Vec<(TabId, egui::Id, egui::Rect)>,
    drag: Option<Drag>,
    suppressed: bool,
    finished: Option<u64>,
    last_frame: u64,
    local_drop: bool,
    #[cfg(test)]
    strip: Option<egui::Rect>,
}

fn state_id() -> egui::Id {
    egui::Id::new("tab-drag-state")
}

pub(super) struct Layout {
    state: State,
    gap: Option<(usize, f32)>,
    source_rect: Option<egui::Rect>,
}

impl Layout {
    pub(super) fn new(
        ui: &egui::Ui,
        tabs: Vec<(TabId, Option<PathBuf>)>,
        rectangles: Vec<egui::Rect>,
        strip: egui::Rect,
        source: (Option<TabId>, u64, u64),
        allowed: bool,
    ) -> Self {
        let context = ui.ctx();
        let mut state = context
            .data(|data| data.get_temp::<State>(state_id()))
            .unwrap_or_default();
        let screen = context.content_rect();
        let density = context.pixels_per_point();
        let overflowing = rectangles
            .first()
            .is_some_and(|rect| rect.left() < strip.left())
            || rectangles
                .last()
                .is_some_and(|rect| rect.right() > strip.right());
        let press_strip = if overflowing {
            // Reserve the floating scrollbar even when press/move/release arrive in one frame.
            strip.with_max_y(
                strip.bottom()
                    - ui.spacing().scroll.bar_width
                    - ui.spacing().scroll.bar_outer_margin,
            )
        } else {
            strip
        };
        let (pointer, down, released, focused, escape, events) = ui.input(|input| {
            (
                input.pointer.interact_pos(),
                input.pointer.primary_down(),
                input.pointer.primary_released(),
                input.focused,
                input.key_pressed(egui::Key::Escape),
                input.events.clone(),
            )
        });
        if !down && !released {
            state.suppressed = false;
        }
        let invalid = !allowed
            || !focused
            || escape
            || (pointer.is_none() && !down)
            || state.drag.as_ref().is_some_and(|drag| {
                drag.tabs != tabs
                    || context.cumulative_frame_nr() > state.last_frame + 1
                    || drag.source != source
                    || drag.screen != screen
                    || drag.density != density
            });
        if invalid {
            if let Some(drag) = state.drag.take() {
                if context.dragged_id() == Some(drag.widget) {
                    context.stop_dragging();
                }
                state.suppressed = true;
            }
        } else if state.drag.is_none()
            && !state.suppressed
            && state.finished != Some(context.cumulative_frame_nr())
            && let Some(origin) = events.iter().find_map(|event| match event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    ..
                } => Some(*pos),
                _ => None,
            })
            && let Some((tab, widget, rect)) = state
                .widgets
                .iter()
                .find(|(_, _, rect)| rect.intersect(press_strip).contains(origin))
            && tabs.iter().any(|(id, _)| id == tab)
            && ((!down && released)
                || context.read_response(*widget).is_some_and(|response| {
                    response.is_pointer_button_down_on()
                        || response.drag_stopped()
                        || response.clicked()
                }))
        {
            state.drag = Some(Drag {
                tab: *tab,
                widget: *widget,
                detached_anchor: strip.min.to_vec2() + (origin - rect.min),
                origin,
                pointer: origin,
                crossed: false,
                tabs: tabs.clone(),
                source,
                screen,
                density,
            });
        }
        if !down && !released {
            state.drag = None;
        }
        if let Some(drag) = &mut state.drag
            && let Some(pointer) = pointer
        {
            drag.pointer = pointer;
            drag.crossed |= pointer.distance_sq(drag.origin) > 36.0;
            if drag.crossed && down {
                context.set_dragged_id(drag.widget);
            }
        }
        let gap = state
            .drag
            .as_ref()
            .filter(|drag| drag.crossed)
            .and_then(|_| {
                ui.input(|input| input.pointer.hover_pos())
                    .and_then(|pointer| tab_gap(&rectangles, strip, screen, pointer))
            });
        let source_rect = state
            .drag
            .as_ref()
            .filter(|drag| drag.crossed)
            .and_then(|drag| tabs.iter().position(|(id, _)| *id == drag.tab))
            .and_then(|index| rectangles.get(index).copied());
        state.widgets.clear();
        Self {
            state,
            gap,
            source_rect,
        }
    }

    pub(super) fn register(&mut self, tab: TabId, response: &egui::Response) {
        self.state.widgets.push((tab, response.id, response.rect));
    }

    pub(super) fn finish(mut self, ui: &egui::Ui, strip: egui::Rect) -> Option<UiAction> {
        let context = ui.ctx();
        let mut action = None;
        // The toolbar behind every tab is opaque black. One final overlay gives
        // group opacity (including text/audio/close), without multiplying each
        // overlapping primitive's alpha separately or copying a GPU surface.
        if let Some(rect) = self.source_rect {
            ui.painter_at(strip)
                .rect_filled(rect, 3.0, egui::Color32::from_black_alpha(128));
        }
        if let Some(drag) = &self.state.drag {
            if drag.crossed
                && self.state.last_frame != context.cumulative_frame_nr()
                && let Some(pointer) = ui.input(|input| {
                    input
                        .pointer
                        .hover_pos()
                        .filter(|_| input.pointer.primary_down())
                })
                && strip.contains(pointer)
            {
                let direction = if pointer.x < strip.left() + 12.0 {
                    1.0
                } else if pointer.x > strip.right() - 12.0 {
                    -1.0
                } else {
                    0.0
                };
                if direction != 0.0 {
                    let delta = ui.input(|input| input.stable_dt.min(0.05)) * 360.0 * direction;
                    ui.scroll_with_delta_animation(
                        egui::vec2(delta, 0.0),
                        egui::style::ScrollAnimation::none(),
                    );
                    context.request_repaint();
                }
            }
            if let Some((_, x)) = self.gap {
                ui.painter_at(strip).line_segment(
                    [egui::pos2(x, strip.top()), egui::pos2(x, strip.bottom())],
                    egui::Stroke::new(2.0, chrome::FOREGROUND),
                );
            }
            if ui.input(|input| input.pointer.primary_released()) {
                action = if !drag.crossed {
                    None
                } else if let Some((gap, _)) = self.gap {
                    Some(UiAction::ReorderTab(drag.tab, gap))
                } else if ui
                    .input(|input| input.pointer.interact_pos().is_some_and(|p| p.is_finite()))
                {
                    Some(UiAction::DropTab(
                        drag.tab,
                        ui.input(|input| input.pointer.interact_pos())
                            .expect("release point"),
                        drag.detached_anchor,
                    ))
                } else {
                    None
                };
            }
        }
        if ui.input(|input| input.pointer.primary_released()) {
            self.state.drag = None;
            self.state.suppressed = false;
            self.state.finished = Some(context.cumulative_frame_nr());
        }
        self.state.last_frame = context.cumulative_frame_nr();
        self.state.local_drop = self.gap.is_some();
        #[cfg(test)]
        {
            self.state.strip = Some(strip);
        }
        context.data_mut(|data| data.insert_temp(state_id(), self.state));
        action
    }
}

pub(super) fn cancel(context: &egui::Context) -> bool {
    let drag = context.data_mut(|data| {
        let state = data.get_temp_mut_or_default::<State>(state_id());
        let drag = state.drag.take();
        if drag.is_some() {
            state.suppressed = true;
            state.local_drop = false;
        }
        drag
    });
    if let Some(drag) = drag {
        if context.dragged_id() == Some(drag.widget) {
            context.stop_dragging();
        }
        true
    } else {
        false
    }
}

pub(super) fn active_pointer(
    context: &egui::Context,
    source: (Option<TabId>, u64, u64),
) -> Option<(TabId, egui::Pos2, bool)> {
    let state = context.data(|data| data.get_temp::<State>(state_id()))?;
    let drag = state.drag.filter(|drag| drag.crossed)?;
    if drag.source != source
        || drag.screen != context.content_rect()
        || drag.density != context.pixels_per_point()
        || context.cumulative_frame_nr() > state.last_frame + 1
    {
        return None;
    }
    context.input(|input| {
        (input.focused && input.pointer.primary_down()).then_some((
            drag.tab,
            input.pointer.interact_pos().unwrap_or(drag.pointer),
            state.local_drop,
        ))
    })
}

#[derive(Clone)]
struct DropStrip {
    tabs: Vec<TabId>,
    rectangles: Vec<egui::Rect>,
    strip: egui::Rect,
    screen: egui::Rect,
    density: f32,
    frame: u64,
}

impl DropStrip {
    fn gap(&self, point: egui::Pos2) -> Option<(usize, f32)> {
        client_gap(&self.rectangles, self.strip, self.screen, point)
    }
}

fn tab_region(strip: egui::Rect, screen: egui::Rect, point: egui::Pos2) -> bool {
    // Match the reference tab strip's 15-point vertical detach magnetism.
    point.is_finite()
        && screen.contains(point)
        && point.y >= strip.top() - 15.0
        && point.y <= strip.bottom() + 15.0
}

fn tab_gap(
    rectangles: &[egui::Rect],
    strip: egui::Rect,
    screen: egui::Rect,
    point: egui::Pos2,
) -> Option<(usize, f32)> {
    tab_region(strip, screen, point)
        .then(|| client_gap(rectangles, strip, screen, point))
        .flatten()
}

#[cfg(test)]
pub(super) fn over_tab_region(context: &egui::Context, point: egui::Pos2) -> bool {
    context
        .data(|data| data.get_temp::<DropStrip>("incoming-tab-strip".into()))
        .is_some_and(|layout| tab_region(layout.strip, layout.screen, point))
}

fn client_gap(
    rectangles: &[egui::Rect],
    strip: egui::Rect,
    screen: egui::Rect,
    point: egui::Pos2,
) -> Option<(usize, f32)> {
    if !point.is_finite() || !screen.contains(point) || strip.width() < 2.0 {
        return None;
    }
    if rectangles.is_empty() || point.x < strip.left() {
        Some((0, strip.left() + 1.0))
    } else if point.x > strip.right() {
        Some((rectangles.len(), strip.right() - 1.0))
    } else {
        chrome::tab_drop_gap(rectangles, strip, egui::pos2(point.x, strip.center().y))
    }
}

pub(super) fn over_incoming_client(context: &egui::Context, point: egui::Pos2) -> bool {
    context
        .data(|data| data.get_temp::<DropStrip>("incoming-tab-strip".into()))
        .is_some_and(|layout| incoming_gap(context, &layout.tabs, point).is_some())
}

pub(super) fn incoming_gap(
    context: &egui::Context,
    tabs: &[TabId],
    point: egui::Pos2,
) -> Option<usize> {
    let strip = context.data(|data| data.get_temp::<DropStrip>("incoming-tab-strip".into()))?;
    if strip.tabs != tabs
        || strip.screen != context.content_rect()
        || strip.density != context.pixels_per_point()
        || context.cumulative_frame_nr() > strip.frame + 1
    {
        return None;
    }
    strip.gap(point).map(|(gap, _)| gap)
}

pub(super) fn incoming(
    ui: &egui::Ui,
    tabs: Vec<TabId>,
    rectangles: Vec<egui::Rect>,
    strip: egui::Rect,
    pointer: Option<egui::Pos2>,
) {
    let layout = DropStrip {
        tabs,
        rectangles,
        strip,
        screen: ui.ctx().content_rect(),
        density: ui.ctx().pixels_per_point(),
        frame: ui.ctx().cumulative_frame_nr(),
    };
    if let Some(point) = pointer
        && let Some((_, x)) = layout.gap(point)
    {
        ui.painter_at(strip).line_segment(
            [egui::pos2(x, strip.top()), egui::pos2(x, strip.bottom())],
            egui::Stroke::new(2.0, chrome::FOREGROUND),
        );
        let direction = if point.x < strip.left() + 12.0 {
            1.0
        } else if point.x > strip.right() - 12.0 {
            -1.0
        } else {
            0.0
        };
        if direction != 0.0 && !layout.tabs.is_empty() && strip.contains(point) {
            let delta = ui.input(|input| input.stable_dt.min(0.05)) * 360.0 * direction;
            ui.scroll_with_delta_animation(
                egui::vec2(delta, 0.0),
                egui::style::ScrollAnimation::none(),
            );
            ui.ctx().request_repaint();
        }
    }
    ui.ctx()
        .data_mut(|data| data.insert_temp("incoming-tab-strip".into(), layout));
}
