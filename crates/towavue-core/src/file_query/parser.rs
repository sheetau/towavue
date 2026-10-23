use super::{Expression, Field, FileQueryError, MediaKind, Pattern, SearchDate, dates::DateRange};

pub(super) fn parse(query: &str, today: Option<SearchDate>) -> Result<Expression, FileQueryError> {
    if query.len() > 8192 {
        return Err(FileQueryError::TooComplex);
    }
    let mut parser = Parser {
        input: query,
        today,
        terms: 0,
    };
    parser.space();
    if parser.input.is_empty() {
        return Ok(Expression::All(Vec::new()));
    }
    let expression = parser.and(Field::Default, 0)?;
    parser.space();
    if !parser.input.is_empty() {
        return Err(FileQueryError::Syntax);
    }
    Ok(expression)
}

struct Parser<'a> {
    input: &'a str,
    today: Option<SearchDate>,
    terms: usize,
}

impl Parser<'_> {
    fn space(&mut self) {
        self.input = self.input.trim_start();
    }
    fn consume(&mut self, ch: char) -> bool {
        if let Some(rest) = self.input.strip_prefix(ch) {
            self.input = rest;
            true
        } else {
            false
        }
    }
    // Everything's default precedence is NOT, OR, AND.
    fn and(&mut self, field: Field, depth: usize) -> Result<Expression, FileQueryError> {
        let mut terms = vec![self.or(field, depth)?];
        self.space();
        while !self.input.is_empty() && !self.input.starts_with('>') {
            terms.push(self.or(field, depth)?);
            self.space();
        }
        Ok(if terms.len() == 1 {
            terms.pop().expect("one parsed term")
        } else {
            Expression::All(terms)
        })
    }
    fn or(&mut self, field: Field, depth: usize) -> Result<Expression, FileQueryError> {
        let mut terms = vec![self.term(field, depth)?];
        self.space();
        while self.consume('|') {
            self.space();
            terms.push(self.term(field, depth)?);
            self.space();
        }
        Ok(if terms.len() == 1 {
            terms.pop().expect("one parsed term")
        } else {
            Expression::Any(terms)
        })
    }
    fn term(&mut self, mut field: Field, depth: usize) -> Result<Expression, FileQueryError> {
        if depth > 32 || self.terms >= 512 {
            return Err(FileQueryError::TooComplex);
        }
        self.terms += 1;
        self.space();
        if self.consume('!') {
            return Ok(Expression::Not(Box::new(self.term(field, depth + 1)?)));
        }
        if self.consume('<') {
            let expr = self.and(field, depth + 1)?;
            if !self.consume('>') {
                return Err(FileQueryError::Syntax);
            }
            return Ok(expr);
        }
        if let Some((prefix, rest)) = self.input.split_once(':') {
            let prefix = prefix.to_ascii_lowercase().replace('-', "");
            let recognized = match prefix.as_str() {
                "name" => Some(Field::Name),
                "path" => Some(Field::Path),
                "folder" => Some(Field::Folder),
                "parent" => Some(Field::Parent),
                "stem" => Some(Field::Stem),
                "ext" | "extension" => Some(Field::Extension),
                "whole" => Some(Field::Whole),
                "dg" | "dategallery" => Some(Field::GalleryDate),
                "size" | "dm" | "dc" | "datemodified" | "datecreated" => {
                    return Err(FileQueryError::UnsupportedProperty);
                }
                "audio" | "image" | "video" => {
                    self.input = rest;
                    return Ok(Expression::Kind(match prefix.as_str() {
                        "audio" => MediaKind::Audio,
                        "image" => MediaKind::Image,
                        _ => MediaKind::Video,
                    }));
                }
                _ => None,
            };
            if let Some(value) = recognized {
                field = value;
                self.input = rest;
                if self.input.starts_with('<') && field != Field::GalleryDate {
                    return self.term(field, depth + 1);
                }
            }
        }
        let mut text = String::new();
        let mut quoted = false;
        let mut saw_quote = false;
        let mut wildcard = false;
        let mut alternatives = Vec::new();
        while let Some(ch) = self.input.chars().next() {
            if ch == '"' {
                quoted = !quoted;
                saw_quote = true;
                self.input = &self.input[1..];
                continue;
            }
            let comparator =
                field == Field::GalleryDate && text.is_empty() && matches!(ch, '<' | '>' | '!');
            if !quoted && !comparator && (ch.is_whitespace() || matches!(ch, '<' | '>' | '|' | '!'))
            {
                break;
            }
            self.input = &self.input[ch.len_utf8()..];
            if ch == ';' && !quoted && field != Field::Default {
                alternatives.push(self.atom(field, &text, wildcard)?);
                text.clear();
                wildcard = false;
            } else {
                // Quotes protect boolean operators/spaces; wildcard filenames remain useful.
                wildcard |= matches!(ch, '*' | '?');
                text.push(ch);
            }
        }
        if quoted || text.is_empty() && !saw_quote {
            return Err(FileQueryError::Syntax);
        }
        let atom = self.atom(field, &text, wildcard)?;
        if alternatives.is_empty() {
            Ok(atom)
        } else {
            alternatives.push(atom);
            Ok(Expression::Any(alternatives))
        }
    }
    fn atom(&self, field: Field, text: &str, wildcard: bool) -> Result<Expression, FileQueryError> {
        if field == Field::GalleryDate {
            return Ok(Expression::Date(
                DateRange::parse(text, self.today).ok_or(FileQueryError::Date)?,
            ));
        }
        Ok(Expression::Text(
            field,
            Pattern {
                text: crate::search_text(text),
                wildcard,
            },
        ))
    }
}
