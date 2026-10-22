//! Owned ASS placement metadata. Effects and vector drawings remain unsupported.
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct SubtitleText {
    pub text: String,
    pub layout: SubtitleLayout,
}

#[derive(Clone, Debug)]
pub struct SubtitleLayout {
    pub canvas: (f32, f32),
    /// ASS keypad alignment, from bottom-left (1) to top-right (9).
    pub alignment: u8,
    pub margins: [f32; 3],
    pub position: Option<(f32, f32)>,
    pub font_size: f32,
    pub vertical: bool,
}

impl Default for SubtitleLayout {
    fn default() -> Self {
        Self {
            canvas: (384.0, 288.0),
            alignment: 2,
            margins: [10.0; 3],
            position: None,
            font_size: 20.0,
            vertical: false,
        }
    }
}

#[derive(Default)]
pub(super) struct Script {
    base: SubtitleLayout,
    styles: BTreeMap<String, SubtitleLayout>,
}

fn number(text: &str) -> Option<f32> {
    text.trim()
        .parse::<f32>()
        .ok()
        .filter(|value| value.is_finite() && value.abs() <= 1_000_000.0)
}

fn legacy_alignment(value: u8) -> Option<u8> {
    match value {
        1..=3 => Some(value),
        5..=7 => Some(value + 2),
        9..=11 => Some(value - 5),
        _ => None,
    }
}

impl Script {
    pub(super) fn parse(header: &str) -> Self {
        let mut script = Self::default();
        let mut fields = Vec::new();
        let mut styles = false;
        let mut legacy = false;
        for line in header.lines().map(str::trim) {
            if line.starts_with('[') {
                styles = matches!(line, "[V4+ Styles]" | "[V4 Styles]");
                legacy = line == "[V4 Styles]";
                continue;
            }
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            match name.trim() {
                "PlayResX" => {
                    if let Some(value) = number(value).filter(|value| *value > 0.0) {
                        script.base.canvas.0 = value;
                    }
                }
                "PlayResY" => {
                    if let Some(value) = number(value).filter(|value| *value > 0.0) {
                        script.base.canvas.1 = value;
                    }
                }
                "Format" if styles => fields = value.split(',').map(str::trim).collect(),
                "Style" if styles && script.styles.len() < 4096 => {
                    let values: Vec<_> = value.split(',').map(str::trim).collect();
                    let get = |name| {
                        fields
                            .iter()
                            .position(|field| *field == name)
                            .and_then(|index| values.get(index))
                            .copied()
                            .unwrap_or("")
                    };
                    let mut layout = script.base.clone();
                    layout.font_size = number(get("Fontsize"))
                        .filter(|value| *value > 0.0)
                        .unwrap_or(layout.font_size);
                    let alignment = get("Alignment").parse::<u8>().ok();
                    layout.alignment = alignment
                        .and_then(|value| {
                            if legacy {
                                legacy_alignment(value)
                            } else {
                                (1..=9).contains(&value).then_some(value)
                            }
                        })
                        .unwrap_or(2);
                    for (index, field) in ["MarginL", "MarginR", "MarginV"].iter().enumerate() {
                        if let Some(value) = number(get(field)).filter(|value| *value >= 0.0) {
                            layout.margins[index] = value;
                        }
                    }
                    layout.vertical = get("Fontname").starts_with('@');
                    script.styles.insert(get("Name").to_owned(), layout);
                }
                _ => {}
            }
        }
        for style in script.styles.values_mut() {
            style.canvas = script.base.canvas;
        }
        script
    }

    pub(super) fn event(&self, value: &str, text: String) -> SubtitleText {
        let fields: Vec<_> = value.splitn(9, ',').collect();
        let mut layout = fields
            .get(2)
            .and_then(|style| self.styles.get(*style))
            .unwrap_or(&self.base)
            .clone();
        for index in 0..3 {
            if let Some(value) = fields
                .get(index + 4)
                .and_then(|value| number(value))
                .filter(|value| *value > 0.0)
            {
                layout.margins[index] = value;
            }
        }
        // Placement applies to a whole event. Do not combine independently placed
        // concurrent events; inline font styling is outside this renderer's scope.
        for block in fields.get(8).copied().unwrap_or("").split('{').skip(1) {
            let Some((tags, _)) = block.split_once('}') else {
                continue;
            };
            for tag in tags.split('\\') {
                if let Some(value) = tag
                    .strip_prefix("an")
                    .and_then(|value| value.parse::<u8>().ok())
                    .filter(|value| (1..=9).contains(value))
                {
                    layout.alignment = value;
                } else if let Some(value) = tag
                    .strip_prefix('a')
                    .and_then(|value| value.parse().ok())
                    .and_then(legacy_alignment)
                {
                    layout.alignment = value;
                } else if let Some(value) = tag
                    .strip_prefix("pos(")
                    .and_then(|value| value.strip_suffix(')'))
                    .and_then(|value| value.split_once(','))
                {
                    if let (Some(x), Some(y)) = (number(value.0), number(value.1)) {
                        layout.position = Some((x, y));
                    }
                } else if let Some(value) = tag
                    .strip_prefix("fs")
                    .and_then(number)
                    .filter(|value| *value > 0.0)
                {
                    layout.font_size = value;
                } else if let Some(font) = tag.strip_prefix("fn") {
                    layout.vertical = font.starts_with('@');
                }
            }
        }
        SubtitleText { text, layout }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_styles_event_margins_and_positions_remain_independent() {
        let script = Script::parse(
            "[Script Info]\nPlayResX: 1920\nPlayResY: 1080\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, Alignment, MarginL, MarginR, MarginV\nStyle: Name,Arial,40,8,20,30,50\nStyle: Dialogue,Arial,48,2,20,30,60\nStyle: Vertical,@Yu Gothic,42,9,20,40,50",
        );
        let name = script.event("0,0,Name,,0,0,70,,Name", "Name".into());
        let dialogue = script.event("1,0,Dialogue,,0,0,0,,Dialogue", "Dialogue".into());
        let vertical = script.event(
            "2,0,Vertical,,0,0,0,,{\\pos(1800,80)}Vertical",
            "Vertical".into(),
        );
        assert_eq!(name.layout.alignment, 8);
        assert_eq!(name.layout.margins[2], 70.0);
        assert_eq!(dialogue.layout.alignment, 2);
        assert_eq!(vertical.layout.position, Some((1800.0, 80.0)));
        assert!(vertical.layout.vertical);
        assert_eq!(vertical.layout.canvas, (1920.0, 1080.0));
        let invalid = script.event(
            "0,0,Name,,0,0,0,,{\\an0\\pos(NaN,0)\\fsinf}Text",
            "Text".into(),
        );
        assert_eq!(invalid.layout.alignment, 8);
        assert!(invalid.layout.position.is_none());
        assert_eq!(invalid.layout.font_size, 40.0);
    }
}
