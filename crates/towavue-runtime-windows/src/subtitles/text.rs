/// FFmpeg emits ASS events as ReadOrder,Layer,Style,Name,MarginL,MarginR,
/// MarginV,Effect,Text. Render plain UI-font text, not authored effects/drawings.
pub(super) fn plain(value: &str, ass: bool) -> String {
    let value = if ass {
        value.splitn(9, ',').nth(8).unwrap_or("")
    } else {
        value
    };
    let mut result = String::new();
    let mut chars = value.chars().peekable();
    let mut drawing = false;
    while let Some(character) = chars.next() {
        if ass && character == '{' {
            let mut tag = String::new();
            for next in chars.by_ref() {
                if next == '}' {
                    break;
                }
                tag.push(next);
            }
            for directive in tag.split('\\') {
                if let Some(number) = directive.strip_prefix('p')
                    && let Ok(mode) = number.trim().parse::<u32>()
                {
                    drawing = mode != 0;
                }
            }
        } else if ass && character == '\\' {
            match chars.next() {
                Some('N') => {
                    if !drawing {
                        result.push('\n');
                    }
                }
                Some('n' | 'h') => {
                    if !drawing {
                        result.push(' ');
                    }
                }
                Some(next) => {
                    if !drawing {
                        result.push('\\');
                        result.push(next);
                    }
                }
                None => {
                    if !drawing {
                        result.push('\\');
                    }
                }
            }
        } else if !drawing && (character == '\n' || character == '\t' || !character.is_control()) {
            result.push(character);
        }
    }
    result.trim().to_owned()
}

pub(super) fn entities(mut value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    while let Some(index) = value.find('&') {
        result.push_str(&value[..index]);
        value = &value[index..];
        let parsed = value
            .as_bytes()
            .iter()
            .take(16)
            .position(|byte| *byte == b';')
            .and_then(|end| {
                let name = &value[1..end];
                let character = match name {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    "nbsp" => Some('\u{a0}'),
                    "lrm" => Some('\u{200e}'),
                    "rlm" => Some('\u{200f}'),
                    _ => name
                        .strip_prefix("#x")
                        .or_else(|| name.strip_prefix("#X"))
                        .and_then(|number| u32::from_str_radix(number, 16).ok())
                        .or_else(|| {
                            name.strip_prefix('#')
                                .and_then(|number| number.parse().ok())
                        })
                        .and_then(char::from_u32),
                };
                character
                    .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
                    .map(|character| (end + 1, character))
            });
        if let Some((length, character)) = parsed {
            result.push(character);
            value = &value[length..];
        } else {
            result.push('&');
            value = &value[1..];
        }
    }
    result.push_str(value);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn character_references_decode_once_without_reinterpreting_literal_tags() {
        assert_eq!(
            entities("&amp; &lt;b&gt; &#65; &#x65e5; &amp;lt; &unknown; &#0;"),
            "& <b> A 日 &lt; &unknown; &#0;"
        );
        assert_eq!(entities("&&&&日本語&"), "&&&&日本語&");
    }
}
