use egui::{Atom, AtomExt, Color32};
use std::{ops::Range, sync::Arc};

/// Map case-folded matches back to UTF-8 boundaries, including expanding letters.
fn ranges(text: &str, query: &str) -> Vec<Range<usize>> {
    let mut folded = String::new();
    let mut original = Vec::new();
    for (start, ch) in text.char_indices() {
        let lower = ch.to_lowercase().collect::<String>();
        original.extend(std::iter::repeat_n(
            start..start + ch.len_utf8(),
            lower.len(),
        ));
        folded.push_str(&lower);
    }
    let mut hits = Vec::new();
    for term in query.split_whitespace().map(str::to_lowercase) {
        for (start, found) in folded.match_indices(&term) {
            hits.push(original[start].start..original[start + found.len() - 1].end);
        }
    }
    hits.sort_by_key(|range| range.start);
    let mut merged: Vec<Range<usize>> = Vec::new();
    for hit in hits {
        if let Some(last) = merged.last_mut()
            && hit.start <= last.end
        {
            last.end = last.end.max(hit.end);
        } else {
            merged.push(hit);
        }
    }
    merged
}

pub(super) fn label(text: &str, query: &str, size: f32, color: Color32) -> Atom<'static> {
    let hits = ranges(text, query);
    if hits.is_empty() {
        return egui::RichText::new(text).size(size).color(color).into();
    }
    let mut job = egui::text::LayoutJob::default();
    let format = egui::TextFormat {
        font_id: egui::FontId::proportional(size),
        color,
        ..Default::default()
    };
    let mut start = 0;
    for hit in hits {
        job.append(&text[start..hit.start], 0.0, format.clone());
        job.append(
            &text[hit.clone()],
            0.0,
            egui::TextFormat {
                color: crate::chrome::FOREGROUND,
                ..format.clone()
            },
        );
        start = hit.end;
    }
    job.append(&text[start..], 0.0, format);
    Atom {
        kind: egui::AtomKind::closure(move |ui, args| {
            let mut galley = egui::WidgetText::from(job).into_galley(
                ui,
                Some(args.wrap_mode),
                args.available_size.x,
                args.fallback_font,
            );
            embolden_matches(Arc::make_mut(&mut galley), size * 0.04);
            egui::IntoSizedResult {
                intrinsic_size: galley.intrinsic_size(),
                sized: egui::SizedAtomKind::Text(galley),
            }
        }),
        ..Default::default()
    }
    .atom_shrink(true)
}

/// The bundled face and installed language fallbacks have no shared bold face.
/// Apply a small synthetic weight only to the explicit match-colored glyphs.
/// Keep egui's shaping, truncation, advances and accessibility text unchanged.
fn embolden_matches(galley: &mut egui::Galley, spread: f32) {
    for placed in &mut galley.rows {
        let row = Arc::make_mut(&mut placed.row);
        let mesh = &mut row.visuals.mesh;
        let old_vertices = mesh.vertices.len();
        let old_indices = mesh.indices.len();
        for triangle in (0..old_indices).step_by(3) {
            let vertices = [0, 1, 2].map(|i| mesh.vertices[mesh.indices[triangle + i] as usize]);
            if vertices
                .iter()
                .all(|v| v.color == crate::chrome::FOREGROUND)
            {
                for mut vertex in vertices {
                    vertex.pos.x += spread;
                    mesh.indices.push(mesh.vertices.len() as u32);
                    mesh.vertices.push(vertex);
                }
            }
        }
        row.visuals.glyph_vertex_range.end = mesh.vertices.len();
        row.visuals.mesh_bounds = mesh.calc_bounds();
        galley.num_vertices += mesh.vertices.len() - old_vertices;
        galley.num_indices += mesh.indices.len() - old_indices;
        galley.mesh_bounds |= row.visuals.mesh_bounds.translate(placed.pos.to_vec2());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_preserve_unicode_boundaries_and_merge_overlapping_terms() {
        assert_eq!(ranges("Open FILE", "file open"), [0..4, 5..9]);
        assert_eq!(ranges("画像を保存", "保存"), std::slice::from_ref(&(9..15)));
        assert_eq!(ranges("İmage", "i"), std::slice::from_ref(&(0..2)));
        assert_eq!(
            ranges("Playback", "play playback"),
            std::slice::from_ref(&(0..8))
        );
        assert!(ranges("Playback rate", "fast").is_empty());
    }

    #[test]
    fn match_weight_changes_only_glyph_paint_and_survives_selection() {
        for density in [1.0, 1.25, 2.0] {
            for selected in [false, true] {
                let context = crate::fonts::test_context();
                context.set_pixels_per_point(density);
                context.global_style_mut(crate::chrome::style);
                let output = context.run_ui(Default::default(), |ui| {
                    ui.add(egui::Button::selectable(
                        selected,
                        label("Open file", "file", 10.0, Color32::PLACEHOLDER),
                    ));
                });
                let text = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) if text.galley.text() == "Open file" => Some(text),
                        _ => None,
                    })
                    .expect("label");
                let original =
                    context.fonts_mut(|fonts| fonts.layout_job((*text.galley.job).clone()));
                assert_eq!(text.galley.rect, original.rect);
                assert!(text.galley.num_vertices > original.num_vertices);
                assert_eq!(
                    text.galley
                        .job
                        .sections
                        .iter()
                        .filter(|section| section.format.color == crate::chrome::FOREGROUND)
                        .map(|section| &text.galley.job.text
                            [section.byte_range.start.0..section.byte_range.end.0])
                        .collect::<String>(),
                    "file"
                );
            }
        }
    }
}
