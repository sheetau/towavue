use crate::*;
use egui::{Pos2, Rect, TextureHandle, Vec2, pos2, vec2};

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Dragging,
    Committing,
    AwaitingFrame,
}

#[cfg(test)]
pub(super) mod tests;

pub(super) struct Scrub {
    instance: u64,
    was_playing: bool,
    phase: Phase,
    geometry: Geometry,
    sample: Option<(TextureHandle, Rect, bool)>,
}

struct Geometry {
    vertices: Vec<(Pos2, Pos2)>,
    size: Vec2,
    aspect: f32,
    source_aspect: f32,
}

impl Geometry {
    fn new(
        size: (u32, u32),
        aspect: f32,
        orientation: towavue_runtime_windows::VideoOrientation,
        operations: &[EditOperation],
    ) -> Self {
        // Cached previews already contain source orientation and square-pixel scaling.
        let oriented = ImageTransform::with_orientation(size, orientation, &[]);
        let mut size = vec2(oriented.size.0, oriented.size.1);
        let mut aspect = oriented.pixel_aspect(aspect);
        let source_aspect = size.x * aspect / size.y;
        let mut vertices = vec![
            (Pos2::ZERO, Pos2::ZERO),
            (pos2(size.x, 0.0), pos2(1.0, 0.0)),
            (pos2(size.x, size.y), pos2(1.0, 1.0)),
            (pos2(0.0, size.y), pos2(0.0, 1.0)),
        ];
        for operation in operations {
            match *operation {
                EditOperation::Crop(crop) => {
                    let min = pos2(crop.x as f32, crop.y as f32);
                    let max = min + vec2(crop.width as f32, crop.height as f32);
                    for (axis, edge, lower) in [
                        (0, min.x, true),
                        (0, max.x, false),
                        (1, min.y, true),
                        (1, max.y, false),
                    ] {
                        vertices = clip(&vertices, axis, edge, lower);
                    }
                    for (position, _) in &mut vertices {
                        *position -= min.to_vec2();
                    }
                    size = max - min;
                }
                EditOperation::RotateClockwise | EditOperation::RotateCounterclockwise => {
                    for (position, _) in &mut vertices {
                        *position = if *operation == EditOperation::RotateClockwise {
                            pos2(size.y - position.y, position.x)
                        } else {
                            pos2(position.y, size.x - position.x)
                        };
                    }
                    size = vec2(size.y, size.x);
                    aspect = 1.0 / aspect;
                }
                EditOperation::FlipHorizontal => {
                    for (position, _) in &mut vertices {
                        position.x = size.x - position.x;
                    }
                }
                EditOperation::FlipVertical => {
                    for (position, _) in &mut vertices {
                        position.y = size.y - position.y;
                    }
                }
                EditOperation::ResizeVideo(resize) if !resize.is_identity() => {
                    let next = vec2(resize.size().0 as f32, resize.size().1 as f32);
                    for (position, _) in &mut vertices {
                        *position = (position.to_vec2() * (next / size)).to_pos2();
                    }
                    size = next;
                    aspect = 1.0;
                }
                EditOperation::RotateVideo(rotation) if rotation.tenths() != 0 => {
                    let square = vec2(
                        rotation.square_size().0 as f32,
                        rotation.square_size().1 as f32,
                    );
                    let raster = vec2(
                        rotation.raster_size().0 as f32,
                        rotation.raster_size().1 as f32,
                    );
                    let (sin, cos) =
                        (f32::from(rotation.tenths()) * std::f32::consts::PI / 1800.0).sin_cos();
                    for (position, _) in &mut vertices {
                        let offset = position.to_vec2() * (square / size) - square * 0.5;
                        *position = (vec2(
                            cos * offset.x - sin * offset.y,
                            sin * offset.x + cos * offset.y,
                        ) + raster * 0.5)
                            .to_pos2();
                    }
                    size = vec2(rotation.size().0 as f32, rotation.size().1 as f32);
                    aspect = 1.0;
                }
                _ => {}
            }
        }
        Self {
            vertices,
            size,
            aspect,
            source_aspect,
        }
    }

    fn mesh(&self, texture: &TextureHandle, mut uv: Rect, padded: bool, rect: Rect) -> egui::Mesh {
        if padded {
            let fit = (240.0 / self.source_aspect).min(160.0);
            let fraction = vec2(fit * self.source_aspect / 240.0, fit / 160.0);
            uv = Rect::from_center_size(uv.center(), uv.size() * fraction);
        }
        let mut mesh = egui::Mesh::with_texture(texture.id());
        for (position, source) in &self.vertices {
            mesh.vertices.push(egui::epaint::Vertex {
                pos: rect.min + position.to_vec2() / self.size * rect.size(),
                uv: uv.min + source.to_vec2() * uv.size(),
                color: Color32::WHITE,
            });
        }
        for index in 2..self.vertices.len() as u32 {
            mesh.add_triangle(0, index - 1, index);
        }
        mesh
    }
}

// Affine edits preserve convexity; clipping interpolates source UVs at crop edges.
fn clip(vertices: &[(Pos2, Pos2)], axis: usize, edge: f32, lower: bool) -> Vec<(Pos2, Pos2)> {
    let mut output = Vec::new();
    let Some(mut previous) = vertices.last().copied() else {
        return output;
    };
    for &current in vertices {
        let inside = |position: Pos2| {
            if lower {
                position[axis] >= edge
            } else {
                position[axis] <= edge
            }
        };
        if inside(previous.0) != inside(current.0) {
            let t = (edge - previous.0[axis]) / (current.0[axis] - previous.0[axis]);
            output.push((previous.0.lerp(current.0, t), previous.1.lerp(current.1, t)));
        }
        if inside(current.0) {
            output.push(current);
        }
        previous = current;
    }
    output
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    fn scrub_pause(&mut self, paused: bool) -> bool {
        let Some(session) = self.session.as_mut() else {
            return false;
        };
        if let Err(error) = session.set_paused(paused) {
            self.fail(error.to_string());
            return false;
        }
        if let Some(clock) = &mut self.clock {
            clock.set_paused(paused);
        }
        self.state = if paused {
            PlaybackState::Paused
        } else {
            PlaybackState::Playing
        };
        if !paused && let Some(tab) = self.tabs.active() {
            self.arm_audio_queue(tab.id);
        }
        true
    }

    pub(super) fn begin_video_scrub(&mut self) {
        self.video_scrub_seen = true;
        if self
            .video_scrub
            .as_ref()
            .is_some_and(|scrub| scrub.phase != Phase::AwaitingFrame)
        {
            return;
        }
        let Some((width, height, aspect)) = self
            .session
            .as_ref()
            .and_then(PlaybackSession::video_geometry)
        else {
            return;
        };
        let orientation = self
            .session
            .as_ref()
            .and_then(PlaybackSession::video_orientation)
            .unwrap_or_default();
        let geometry = Geometry::new(
            (width, height),
            aspect,
            orientation,
            self.video_operations(),
        );
        let was_playing = self.state == PlaybackState::Playing;
        if was_playing && !self.scrub_pause(true) {
            return;
        }
        self.video_scrub = Some(Scrub {
            instance: self.media_generation,
            was_playing,
            phase: Phase::Dragging,
            geometry,
            sample: None,
        });
    }

    pub(super) fn queue_video_scrub(&mut self, target: MediaTime, actions: &mut Vec<UiAction>) {
        if let Some(scrub) = &mut self.video_scrub {
            scrub.phase = Phase::Committing;
            actions.push(UiAction::CommitVideoScrub(target));
        } else {
            actions.push(UiAction::Seek(target));
        }
    }

    pub(super) fn cancel_video_scrub(&mut self) -> bool {
        let Some(scrub) = self.video_scrub.take() else {
            return false;
        };
        if scrub.was_playing
            && scrub.instance == self.media_generation
            && self.state == PlaybackState::Paused
        {
            self.scrub_pause(false);
        }
        true
    }

    pub(super) fn commit_video_scrub(&mut self, target: MediaTime) {
        let Some(mut scrub) = self.video_scrub.take() else {
            return;
        };
        if scrub.instance != self.media_generation || scrub.phase != Phase::Committing {
            return;
        }
        // Let the ordinary seek decide EOF/trim pausing, then resume only its accepted range.
        if scrub.was_playing {
            self.state = PlaybackState::Playing;
        }
        self.seek_to(target);
        if self.state == PlaybackState::Playing && !self.scrub_pause(false) {
            return;
        }
        scrub.was_playing = false;
        scrub.phase = Phase::AwaitingFrame;
        self.video_scrub = Some(scrub);
    }

    pub(super) fn update_scrub_sample(&mut self, sample: Option<(TextureHandle, Rect, bool)>) {
        if let Some(scrub) = &mut self.video_scrub
            && scrub.phase == Phase::Dragging
            && sample.is_some()
        {
            scrub.sample = sample;
        }
    }

    pub(super) fn draw_video_scrub(&mut self) {
        if self.video_scrub.as_ref().is_some_and(|scrub| {
            scrub.instance != self.media_generation
                || self.state == PlaybackState::Faulted
                || (scrub.phase == Phase::Dragging && !self.video_scrub_seen)
                || (scrub.phase == Phase::AwaitingFrame
                    && self
                        .session
                        .as_ref()
                        .is_none_or(|session| !session.video_refresh_pending()))
        }) {
            self.cancel_video_scrub();
        }
        let Some(scrub) = &self.video_scrub else {
            return;
        };
        let Some((texture, uv, padded)) = &scrub.sample else {
            return;
        };
        let Some((painter, viewport)) = &self.video_scrub_surface else {
            return;
        };
        let geometry = &scrub.geometry;
        let rect = video_view::rect(
            *viewport,
            (geometry.size.x as u32, geometry.size.y as u32),
            geometry.aspect,
            painter.ctx().pixels_per_point(),
            self.image_view,
        );
        let painter = painter.with_clip_rect(*viewport);
        painter.rect_filled(*viewport, 0.0, Color32::BLACK);
        painter.add(geometry.mesh(texture, *uv, *padded, rect));
        self.video_rect = None;
    }
}
