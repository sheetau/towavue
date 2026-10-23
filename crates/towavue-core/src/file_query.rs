//! Bounded, filesystem-free file expressions shared by the browsing surfaces.
mod dates;
mod parser;
#[cfg(test)]
mod tests;

use std::path::Path;

use crate::{MediaKind, file_search_score, search_text};
use dates::DateRange;

pub type SearchDate = (u16, u16, u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileQueryError {
    Syntax,
    Date,
    UnsupportedProperty,
    TooComplex,
}

/// Compile once when the query changes. Evaluation never reads file metadata.
#[derive(Clone, Debug)]
pub struct FileQuery {
    expression: Result<Expression, FileQueryError>,
    plain: Option<String>,
    gallery: bool,
}

#[derive(Clone, Debug)]
enum Expression {
    All(Vec<Self>),
    Any(Vec<Self>),
    Not(Box<Self>),
    Text(Field, Pattern),
    Kind(MediaKind),
    Date(DateRange),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Default,
    Name,
    Path,
    Folder,
    Parent,
    Stem,
    Extension,
    Whole,
    GalleryDate,
}

#[derive(Clone, Debug)]
struct Pattern {
    text: String,
    wildcard: bool,
}

impl FileQuery {
    pub fn new(query: &str, today: Option<SearchDate>) -> Self {
        let expression = parser::parse(query, today);
        let plain = match &expression {
            Ok(Expression::Text(Field::Default, pattern))
                if !pattern.wildcard && !query.contains('"') =>
            {
                Some(pattern.text.clone())
            }
            _ => None,
        };
        let gallery = expression.as_ref().is_ok_and(Expression::uses_gallery);
        Self {
            expression,
            plain,
            gallery,
        }
    }

    pub fn error(&self) -> Option<FileQueryError> {
        self.expression.as_ref().err().copied()
    }

    pub fn matches_path(&self, path: &Path, full_path: bool) -> bool {
        !self.gallery && self.matches(path, full_path, None)
    }

    pub fn uses_gallery_date(&self) -> bool {
        self.gallery
    }

    pub fn matches_gallery(&self, path: &Path, date: Option<SearchDate>) -> bool {
        self.matches(path, true, Some(date))
    }

    /// Preserve the picker's existing fuzzy ranking for one unquoted plain term.
    /// Explicit expressions, phrases and wildcards always use strict matching.
    pub fn score(&self, path: &Path) -> Option<usize> {
        if let Some(plain) = &self.plain {
            if plain.as_bytes().get(1) == Some(&b':') {
                return crate::search_text_score(&search_text(&path.to_string_lossy()), plain);
            }
            return file_search_score(path, plain);
        }
        self.matches_path(path, false).then_some(0)
    }

    fn matches(&self, path: &Path, full_path: bool, date: Option<Option<SearchDate>>) -> bool {
        let Ok(expression) = &self.expression else {
            return false;
        };
        let path = search_text(&path.to_string_lossy());
        let (folder, name) = path.rsplit_once('/').unwrap_or(("", &path));
        expression.matches(&Candidate {
            path: &path,
            folder,
            name,
            full_path,
            date,
        })
    }
}

struct Candidate<'a> {
    path: &'a str,
    folder: &'a str,
    name: &'a str,
    full_path: bool,
    // Outer None means this surface has no Gallery date, even for unknown searches.
    date: Option<Option<SearchDate>>,
}

impl Expression {
    fn uses_gallery(&self) -> bool {
        match self {
            Self::Date(_) => true,
            Self::All(terms) | Self::Any(terms) => terms.iter().any(Self::uses_gallery),
            Self::Not(term) => term.uses_gallery(),
            _ => false,
        }
    }
    fn matches(&self, item: &Candidate<'_>) -> bool {
        match self {
            Self::All(terms) => terms.iter().all(|term| term.matches(item)),
            Self::Any(terms) => terms.iter().any(|term| term.matches(item)),
            Self::Not(term) => !term.matches(item),
            Self::Kind(kind) => MediaKind::from_path(Path::new(item.name)) == Some(*kind),
            Self::Date(range) => item.date.is_some_and(|date| range.matches(date)),
            Self::Text(field, pattern) => {
                let text = match field {
                    Field::Default
                        if item.full_path && !pattern.wildcard
                            || pattern.text.contains('/')
                            || pattern.text.as_bytes().get(1) == Some(&b':') =>
                    {
                        item.path
                    }
                    Field::Default | Field::Name | Field::Whole => item.name,
                    Field::Path => item.path,
                    Field::Folder | Field::Parent => item.folder,
                    Field::Stem => item
                        .name
                        .rsplit_once('.')
                        .map_or(item.name, |(stem, _)| stem),
                    Field::Extension => item.name.rsplit_once('.').map_or("", |(_, ext)| ext),
                    Field::GalleryDate => unreachable!("date expressions are compiled separately"),
                };
                // A folder path matches an ancestor boundary, never a sibling prefix.
                if *field == Field::Folder && pattern.text.contains('/') && !pattern.wildcard {
                    let folder = pattern.text.trim_end_matches('/');
                    return text == folder
                        || text
                            .strip_prefix(folder)
                            .is_some_and(|tail| tail.starts_with('/'));
                }
                pattern.matches(
                    text,
                    matches!(field, Field::Extension | Field::Whole | Field::Parent),
                )
            }
        }
    }
}

impl Pattern {
    fn matches(&self, text: &str, exact: bool) -> bool {
        if self.wildcard {
            wildcard_match(&self.text, text)
        } else if exact {
            text == self.text.trim_end_matches('/')
        } else {
            text.contains(&self.text)
        }
    }
}

/// Dynamic programming avoids exponential backtracking, including adversarial
/// repeated stars. `?` consumes one Unicode scalar; only `**` crosses separators.
fn wildcard_match(pattern: &str, text: &str) -> bool {
    let text: Vec<_> = text.chars().collect();
    let mut row = vec![false; text.len() + 1];
    row[0] = true;
    let mut chars = pattern.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '*' {
            let recursive = chars.peek() == Some(&'*');
            if recursive {
                chars.next();
            }
            for index in 1..row.len() {
                row[index] |= row[index - 1] && (recursive || text[index - 1] != '/');
            }
        } else {
            for index in (1..row.len()).rev() {
                row[index] = row[index - 1]
                    && (ch == text[index - 1] || ch == '?' && text[index - 1] != '/');
            }
            row[0] = false;
        }
    }
    row[text.len()]
}
