use super::*;

#[derive(Clone, PartialEq)]
struct Scope {
    folder: PathBuf,
    generation: u64,
    current: Option<PathBuf>,
    screen: Rect,
    density: f32,
}

struct Drag {
    path: PathBuf,
    widget: egui::Id,
    origin: egui::Pos2,
    offset: Vec2,
    crossed: bool,
    texture: Option<TextureHandle>,
}

#[derive(Default)]
pub(super) struct State {
    scope: Option<Scope>,
    drag: Option<Drag>,
    last_frame: u64,
    claimed: Option<u64>,
    eligible: bool,
}

impl State {
    pub(super) fn clear(&mut self) {
        self.drag = None;
        self.scope = None;
        self.eligible = false;
    }

    pub(super) fn begin(
        &mut self,
        context: &Context,
        snapshot: Option<&FolderSnapshot>,
        current: Option<&Path>,
        enabled: bool,
    ) {
        let scope = snapshot.map(|snapshot| Scope {
            folder: snapshot.folder_path.clone(),
            generation: snapshot.generation,
            current: current.map(Path::to_owned),
            screen: context.content_rect(),
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

    pub(super) fn observe(
        &mut self,
        response: &egui::Response,
        path: &Path,
        preview: Option<&Preview>,
    ) {
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
                texture: preview
                    .and_then(|preview| preview.as_ref().ok())
                    .map(|(texture, _)| texture.clone()),
            });
            self.claimed = Some(response.ctx.cumulative_frame_nr());
        }
    }

    pub(super) fn finish(&mut self, context: &Context, actions: &mut Vec<UiAction>) {
        let (pointer, hover, down, released) = context.input(|input| {
            (
                input.pointer.interact_pos(),
                input.pointer.hover_pos(),
                input.pointer.primary_down(),
                input.pointer.primary_released(),
            )
        });
        if !down && !released {
            self.drag = None;
        }
        if let (Some(drag), Some(scope)) = (&mut self.drag, &self.scope) {
            if let Some(pointer) = pointer {
                drag.crossed |= pointer.distance_sq(drag.origin) > 36.0;
            }
            if drag.crossed && down {
                context.set_dragged_id(drag.widget);
            }
            if drag.crossed
                && down
                && let Some(pointer) = hover
            {
                let painter = egui::Painter::new(
                    context.clone(),
                    egui::LayerId::new(egui::Order::Tooltip, "filmstrip-drag".into()),
                    scope.screen,
                );
                let rect = Rect::from_min_size(pointer - drag.offset, egui::vec2(120.0, 80.0));
                painter.rect_filled(rect, 0.0, Color32::from_gray(28));
                if let Some(texture) = &drag.texture {
                    let scale = (rect.width() / texture.size_vec2().x)
                        .min(rect.height() / texture.size_vec2().y);
                    painter.image(
                        texture.id(),
                        Rect::from_center_size(rect.center(), texture.size_vec2() * scale),
                        Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                painter.rect_stroke(
                    rect,
                    0.0,
                    egui::Stroke::new(1.0, Color32::WHITE),
                    egui::StrokeKind::Inside,
                );
                let name = display_name(&drag.path);
                let label = if name.chars().count() > 32 {
                    format!("{}…", name.chars().take(32).collect::<String>())
                } else {
                    name
                };
                painter.text(
                    rect.left_bottom() + egui::vec2(0.0, 3.0),
                    Align2::LEFT_TOP,
                    label,
                    FontId::proportional(12.0),
                    Color32::WHITE,
                );
            }
            if released
                && drag.crossed
                && pointer.is_some_and(|pointer| !scope.screen.contains(pointer))
            {
                actions.push(UiAction::OpenWindow(
                    drag.path.clone(),
                    scope.generation,
                    pointer.expect("outside release point") - drag.offset,
                ));
            }
        }
        if released {
            self.drag = None;
            self.claimed = Some(context.cumulative_frame_nr());
        }
    }
}
