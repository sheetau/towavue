use crate::*;

#[cfg(test)]
pub(crate) mod tests;

#[derive(Clone)]
struct Drag {
    tab: TabId,
    widget: egui::Id,
    offset: egui::Vec2,
    origin: egui::Pos2,
    crossed: bool,
    tabs: Vec<(TabId, PathBuf)>,
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
    #[cfg(test)]
    strip: Option<egui::Rect>,
}

fn state_id() -> egui::Id {
    egui::Id::new("tab-drag-projection")
}

pub(super) struct Layout {
    state: State,
    rectangles: Vec<egui::Rect>,
    gap: Option<(usize, f32)>,
    pub(super) floating: Option<TabId>,
}

impl Layout {
    pub(super) fn new(
        ui: &egui::Ui,
        tabs: Vec<(TabId, PathBuf)>,
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
                .find(|(_, _, rect)| rect.intersect(strip).contains(origin))
            && tabs.iter().any(|(id, _)| id == tab)
        {
            state.drag = Some(Drag {
                tab: *tab,
                widget: *widget,
                offset: origin - rect.min,
                origin,
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
            drag.crossed |= pointer.distance_sq(drag.origin) > 36.0;
            if drag.crossed && down {
                context.set_dragged_id(drag.widget);
            }
        }
        let mut projected = rectangles.clone();
        let mut gap = None;
        let floating = state
            .drag
            .as_ref()
            .filter(|drag| drag.crossed && ui.input(|input| input.pointer.hover_pos().is_some()))
            .map(|drag| drag.tab);
        if let Some(drag) = &state.drag
            && drag.crossed
            && floating.is_some()
            && let Some(pointer) = pointer
            && let Some(from) = tabs.iter().position(|(tab, _)| *tab == drag.tab)
        {
            gap = chrome::tab_drop_gap(&rectangles, strip, pointer);
            if let Some((gap, _)) = gap {
                let to = gap - usize::from(gap > from);
                for (index, rect) in projected.iter_mut().enumerate() {
                    let slot = if index > from && index <= to {
                        index - 1
                    } else if index < from && index >= to {
                        index + 1
                    } else {
                        index
                    };
                    *rect = rectangles[slot];
                }
            }
            projected[from] =
                egui::Rect::from_min_size(pointer - drag.offset, rectangles[from].size());
        }
        state.widgets.clear();
        Self {
            state,
            rectangles: projected,
            gap,
            floating,
        }
    }

    pub(super) fn rect(&self, index: usize) -> egui::Rect {
        self.rectangles[index]
    }

    pub(super) fn register(&mut self, tab: TabId, response: &egui::Response) {
        self.state.widgets.push((tab, response.id, response.rect));
    }

    pub(super) fn finish(mut self, ui: &egui::Ui, strip: egui::Rect) -> Option<UiAction> {
        let context = ui.ctx();
        let screen = context.content_rect();
        let mut action = None;
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
                } else if ui.input(|input| {
                    input
                        .pointer
                        .interact_pos()
                        .is_some_and(|p| !screen.contains(p))
                }) {
                    Some(UiAction::DetachTab(drag.tab))
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
        #[cfg(test)]
        {
            self.state.strip = Some(strip);
        }
        context.data_mut(|data| data.insert_temp(state_id(), self.state));
        action
    }
}
