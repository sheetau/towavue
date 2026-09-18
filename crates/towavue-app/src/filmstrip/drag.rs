use super::*;

#[derive(Clone, PartialEq)]
struct Scope {
    folder: PathBuf,
    generation: u64,
    current: Option<PathBuf>,
    screen: Rect,
    media: Rect,
    density: f32,
}

struct Drag {
    path: PathBuf,
    widget: egui::Id,
    origin: egui::Pos2,
    offset: Vec2,
    crossed: bool,
}

#[derive(Default)]
pub(super) struct State {
    scope: Option<Scope>,
    drag: Option<Drag>,
    last_frame: u64,
    claimed: Option<u64>,
    eligible: bool,
    band: Option<Rect>,
}

impl State {
    pub(super) fn clear(&mut self) {
        self.drag = None;
        self.scope = None;
        self.eligible = false;
        self.band = None;
    }

    pub(super) fn cancel(&mut self, context: &Context) -> bool {
        let active = self.drag.is_some();
        self.clear();
        self.claimed = Some(context.cumulative_frame_nr());
        if active {
            context.stop_dragging();
        }
        active
    }

    pub(super) fn begin(
        &mut self,
        context: &Context,
        snapshot: Option<&FolderSnapshot>,
        current: Option<&Path>,
        media: Rect,
        enabled: bool,
    ) {
        let scope = snapshot.map(|snapshot| Scope {
            folder: snapshot.folder_path.clone(),
            generation: snapshot.generation,
            current: current.map(Path::to_owned),
            screen: context.content_rect(),
            media,
            density: context.pixels_per_point(),
        });
        let frame = context.cumulative_frame_nr();
        self.eligible = enabled
            && scope.is_some()
            && !egui::Popup::is_any_open(context)
            && context.input(|input| input.focused && !input.key_pressed(egui::Key::Escape));
        if (!self.eligible
            || (self.scope.is_some() && self.scope != scope)
            || frame > self.last_frame + 1)
            && let Some(drag) = self.drag.take()
        {
            if context.dragged_id() == Some(drag.widget) {
                context.stop_dragging();
            }
            self.claimed = Some(frame);
        }
        self.scope = scope;
        self.last_frame = frame;
    }

    pub(super) fn observe(&mut self, response: &egui::Response, path: &Path) {
        if self.eligible
            && self.drag.is_none()
            && self.claimed != Some(response.ctx.cumulative_frame_nr())
            && let Some(origin) =
                crate::view_drag_button_positions(response, egui::PointerButton::Primary).0
        {
            self.drag = Some(Drag {
                path: path.to_owned(),
                widget: response.id,
                origin,
                offset: origin - response.rect.min,
                crossed: false,
            });
            self.claimed = Some(response.ctx.cumulative_frame_nr());
        }
    }

    pub(super) fn active_pointer(
        &self,
        context: &Context,
        current: Option<&Path>,
    ) -> Option<(&Path, u64, egui::Pos2)> {
        let scope = self.scope.as_ref()?;
        let drag = self.drag.as_ref()?;
        if !self.eligible
            || !drag.crossed
            || scope.current.as_deref() != current
            || scope.screen != context.content_rect()
            || scope.density != context.pixels_per_point()
            || context.cumulative_frame_nr() > self.last_frame + 1
        {
            return None;
        }
        context.input(|input| {
            (input.focused && input.pointer.primary_down() && !input.key_pressed(egui::Key::Escape))
                .then(|| {
                    input
                        .pointer
                        .interact_pos()
                        .filter(|point| self.band.is_some_and(|band| !band.contains(*point)))
                        .map(|point| (drag.path.as_path(), scope.generation, point))
                })
                .flatten()
        })
    }

    pub(super) fn finish(
        &mut self,
        context: &Context,
        band: Option<Rect>,
        actions: &mut Vec<UiAction>,
    ) {
        self.band = band;
        let (pointer, down, released) = context.input(|input| {
            (
                input.pointer.interact_pos(),
                input.pointer.primary_down(),
                input.pointer.primary_released(),
            )
        });
        if !down && !released {
            self.drag = None;
        }
        if let (Some(drag), Some(scope)) = (&mut self.drag, &self.scope) {
            if let Some(pointer) = pointer {
                drag.crossed |= band.is_some_and(|band| !band.contains(pointer))
                    && pointer.distance_sq(drag.origin) > 36.0;
            }
            if drag.crossed && down {
                context.set_dragged_id(drag.widget);
            }
            if released
                && drag.crossed
                // Leaving the band arms the drag, but returning cancels transfer
                // eligibility at the live/release position, including batched input.
                && pointer.is_some_and(|point| band.is_some_and(|band| !band.contains(point)))
                && pointer.is_some_and(|pointer| {
                    !scope.screen.contains(pointer)
                        || crate::tab_drag::over_incoming_client(context, pointer)
                })
            {
                actions.push(UiAction::OpenWindow(
                    drag.path.clone(),
                    scope.generation,
                    pointer.expect("drop point"),
                    drag.offset,
                ));
            }
        }
        if released {
            self.drag = None;
            self.claimed = Some(context.cumulative_frame_nr());
        }
    }
}
