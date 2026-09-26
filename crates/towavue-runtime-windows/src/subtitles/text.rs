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
