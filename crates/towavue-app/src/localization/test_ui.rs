//! Offscreen controls using the same font installation as production windows.
use super::*;

pub(crate) fn japanese_context(density: f32) -> egui::Context {
    let context = crate::fonts::test_context();
    configure_japanese(&context, density);
    context
}

pub(crate) fn configure_japanese(context: &egui::Context, density: f32) {
    if !crate::fonts::install(context) {
        eprintln!(
            "SKIP Japanese glyph qualification: no installed Japanese UI font; dialog actions remain checked"
        );
    }
    context.enable_accesskit();
    context.set_pixels_per_point(density);
    context.global_style_mut(crate::chrome::style);
    set_language(context, Language::Japanese);
}

pub(crate) fn settle<T>(
    context: &egui::Context,
    size: egui::Vec2,
    mut show: impl FnMut(&egui::Context) -> Option<T>,
) -> egui::FullOutput {
    let mut output = egui::FullOutput::default();
    for _ in 0..4 {
        let result = frame(context, size, vec![], &mut show);
        assert!(result.1.is_empty(), "layout must not apply edits");
        output = result.0;
    }
    output
}

pub(crate) fn frame<T>(
    context: &egui::Context,
    size: egui::Vec2,
    events: Vec<egui::Event>,
    mut show: impl FnMut(&egui::Context) -> Option<T>,
) -> (egui::FullOutput, Vec<T>) {
    let mut actions = Vec::new();
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            focused: true,
            events,
            ..Default::default()
        },
        |_| actions.extend(show(context)),
    );
    (output, actions)
}

pub(crate) fn action(output: &egui::FullOutput, label: &str, value: Option<&str>) -> egui::Event {
    let tree = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("tree");
    crate::video_rotation::tests::access(crate::video_rotation::tests::node(tree, label), value)
}

pub(crate) fn visible_button(
    output: &egui::FullOutput,
    label: &str,
    size: egui::Vec2,
    enabled: bool,
) {
    let tree = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("tree");
    let node = &tree
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some(label))
        .unwrap_or_else(|| panic!("missing {label}"))
        .1;
    assert_eq!(node.is_disabled(), !enabled, "{label}");
    let bounds = node.bounds().expect("button bounds");
    assert!(
        bounds.x0 >= 0.0
            && bounds.y0 >= 0.0
            && bounds.x1 <= f64::from(size.x)
            && bounds.y1 <= f64::from(size.y),
        "{label}: {bounds:?} at {size:?}"
    );
    assert!(
        output.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Text(text) if text.galley.text() == label && !text.galley.elided)),
        "untruncated {label}"
    );
}
