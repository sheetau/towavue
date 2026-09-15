use std::path::Path;

/// Shared ranking for recent items and background hierarchy search.
pub fn search_text(value: &str) -> String {
    value.replace('\\', "/").to_lowercase()
}

pub fn file_search_score(path: &Path, query: &str) -> Option<usize> {
    let value = if query.contains('/') {
        path.as_os_str()
    } else {
        path.file_name().unwrap_or(path.as_os_str())
    };
    search_text_score(&search_text(&value.to_string_lossy()), query)
}

/// Query and text must already be normalized with `search_text`.
pub fn search_text_score(text: &str, query: &str) -> Option<usize> {
    if query.is_empty() || text == query {
        return Some(0);
    }
    if text.starts_with(query) {
        return Some(1);
    }
    if text.contains(query) {
        return Some(2);
    }
    let mut chars = text.chars();
    query
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .all(|ch| chars.any(|value| value == ch))
        .then_some(3)
}
