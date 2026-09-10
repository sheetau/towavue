use super::*;
use quick_xml::events::Event;
use quick_xml::name::{LocalName, ResolveResult};
use quick_xml::reader::NsReader;

pub(super) const LIMIT: usize = 65502;
const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const DC: &str = "http://purl.org/dc/elements/1.1/";
const XML: &str = "http://www.w3.org/XML/1998/namespace";
const META: &str = "adobe:ns:meta/";

fn invalid(message: impl std::fmt::Display) -> ExportError {
    ExportError::Failed(format!("JPEG XMP text: {message}"))
}

#[derive(Debug, PartialEq, Eq)]
struct Name(String, String);

impl Name {
    fn is(&self, namespace: &str, local: &str) -> bool {
        self.0 == namespace && self.1 == local
    }

    fn field(&self) -> Option<MetadataField> {
        if self.0 != DC {
            return None;
        }
        match self.1.as_str() {
            "title" => Some(MetadataField::Title),
            "creator" => Some(MetadataField::Artist),
            "description" => Some(MetadataField::Comment),
            "rights" => Some(MetadataField::Copyright),
            _ => None,
        }
    }
}

fn name((namespace, local): (ResolveResult<'_>, LocalName<'_>)) -> Result<Name, ExportError> {
    let namespace = match namespace {
        ResolveResult::Bound(value) => std::str::from_utf8(value.as_ref())
            .map_err(invalid)?
            .to_owned(),
        ResolveResult::Unbound => String::new(),
        ResolveResult::Unknown(_) => return Err(invalid("undeclared namespace prefix")),
    };
    Ok(Name(
        namespace,
        std::str::from_utf8(local.as_ref()).map_err(invalid)?.into(),
    ))
}

#[derive(Debug)]
struct Node {
    name: Name,
    attributes: Vec<(Name, String)>,
    text: String,
    children: Vec<Node>,
}

fn valid_text(text: &str) -> bool {
    text.chars().all(|character| matches!(character as u32, 9 | 10 | 13 | 0x20..=0xd7ff | 0xe000..=0xfffd | 0x10000..=0x10ffff))
}

fn tree(packet: &[u8], cancelled: &AtomicBool) -> Result<Node, ExportError> {
    if packet.len() > LIMIT {
        return Err(invalid("packet exceeds 65502 bytes"));
    }
    let text = std::str::from_utf8(packet).map_err(invalid)?;
    if !valid_text(text) {
        return Err(invalid("invalid XML character"));
    }
    let mut reader = NsReader::from_str(text.trim_start_matches('\u{feff}'));
    reader.config_mut().expand_empty_elements = true;
    reader.config_mut().check_comments = true;
    reader.resolver_mut().set_max_declarations_per_element(64);
    let mut stack: Vec<Node> = Vec::new();
    let mut root = None;
    let mut count = 0;
    loop {
        check_cancelled(cancelled)?;
        let event = reader.read_event().map_err(invalid)?;
        let content = match event {
            Event::Start(start) => {
                count += 1;
                if count > 4096 || stack.len() >= 32 {
                    return Err(invalid("XML structure exceeds 4096 elements / 32 levels"));
                }
                if stack.is_empty() && root.is_some() {
                    return Err(invalid("multiple XML roots"));
                }
                let mut attributes = Vec::new();
                for attribute in start.attributes() {
                    let attribute = attribute.map_err(invalid)?;
                    let value = attribute
                        .decoded_and_normalized_value(
                            quick_xml::XmlVersion::Implicit1_0,
                            reader.decoder(),
                        )
                        .map_err(invalid)?
                        .into_owned();
                    if !valid_text(&value) {
                        return Err(invalid("invalid attribute character"));
                    }
                    if attribute.key.as_ref() == b"xmlns"
                        || attribute.key.as_ref().starts_with(b"xmlns:")
                    {
                        if attribute.value.as_ref() != value.as_bytes() {
                            return Err(invalid(
                                "escaped namespace declarations are not supported",
                            ));
                        }
                        continue;
                    }
                    let key = name(reader.resolver().resolve_attribute(attribute.key))?;
                    if attributes.iter().any(|(other, _)| *other == key) {
                        return Err(invalid("duplicate expanded attribute name"));
                    }
                    attributes.push((key, value));
                }
                stack.push(Node {
                    name: name(reader.resolver().resolve_element(start.name()))?,
                    attributes,
                    text: String::new(),
                    children: Vec::new(),
                });
                None
            }
            Event::End(_) => {
                let node = stack
                    .pop()
                    .ok_or_else(|| invalid("unexpected closing element"))?;
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                } else {
                    root = Some(node);
                }
                None
            }
            Event::Text(text) => Some(text.xml10_content().map_err(invalid)?.into_owned()),
            Event::CData(text) => Some(text.xml10_content().map_err(invalid)?.into_owned()),
            Event::GeneralRef(reference) => {
                let raw = reference.decode().map_err(invalid)?;
                Some(
                    quick_xml::escape::unescape(&format!("&{raw};"))
                        .map_err(invalid)?
                        .into_owned(),
                )
            }
            Event::DocType(_) => {
                return Err(invalid("DTD and external entities are not supported"));
            }
            Event::Decl(declaration) => {
                if declaration.version().map_err(invalid)?.as_ref() != b"1.0"
                    || declaration
                        .encoding()
                        .transpose()
                        .map_err(invalid)?
                        .is_some_and(|encoding| !encoding.eq_ignore_ascii_case(b"utf-8"))
                    || count != 0
                {
                    return Err(invalid("only XML 1.0 UTF-8 declarations are supported"));
                }
                None
            }
            Event::PI(_) | Event::Comment(_) => None,
            Event::Eof => break,
            Event::Empty(_) => unreachable!("empty elements are expanded"),
        };
        if let Some(content) = content {
            if !valid_text(&content) {
                return Err(invalid("invalid referenced XML character"));
            }
            if let Some(node) = stack.last_mut() {
                node.text.push_str(&content);
            } else if !content.trim().is_empty() {
                return Err(invalid("text outside XML root"));
            }
        }
    }
    if !stack.is_empty() {
        return Err(invalid("unclosed XML element"));
    }
    root.ok_or_else(|| invalid("missing XMP document"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Value {
    pub field: MetadataField,
    pub language: Option<String>,
    pub text: String,
}

fn alt(field: MetadataField) -> bool {
    field != MetadataField::Artist
}

fn values(node: &Node, field: MetadataField) -> Result<Vec<Value>, ExportError> {
    if !node.attributes.is_empty() {
        return Err(invalid("qualified text properties are not supported"));
    }
    if node.children.is_empty() {
        return Ok(vec![Value {
            field,
            language: alt(field).then(|| "x-default".into()),
            text: node.text.clone(),
        }]);
    }
    if !node.text.trim().is_empty() || node.children.len() != 1 {
        return Err(invalid("mixed or multiple property structures"));
    }
    let array = &node.children[0];
    if !array.name.is(RDF, if alt(field) { "Alt" } else { "Seq" })
        || !array.attributes.is_empty()
        || !array.text.trim().is_empty()
    {
        return Err(invalid(
            "expected an Alt language list or ordered creator Seq",
        ));
    }
    let mut result: Vec<Value> = Vec::new();
    for item in &array.children {
        if !item.name.is(RDF, "li") || !item.children.is_empty() {
            return Err(invalid("only plain text list items are supported"));
        }
        let language = match item.attributes.as_slice() {
            [] if !alt(field) => None,
            [(key, language)]
                if alt(field)
                    && key.is(XML, "lang")
                    && !language.is_empty()
                    && language
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-') =>
            {
                Some(language.clone())
            }
            _ => return Err(invalid("missing or unsupported text language/qualifier")),
        };
        if let Some(language) = &language
            && result.iter().any(|other| {
                other
                    .language
                    .as_ref()
                    .is_some_and(|other| other.eq_ignore_ascii_case(language))
            })
        {
            return Err(invalid("duplicate language alternative"));
        }
        result.push(Value {
            field,
            language,
            text: item.text.clone(),
        });
    }
    Ok(result)
}

pub(super) fn parse(packet: &[u8], cancelled: &AtomicBool) -> Result<Vec<Value>, ExportError> {
    let root = tree(packet, cancelled)?;
    let rdf = if root.name.is(RDF, "RDF") {
        &root
    } else if root.name.is(META, "xmpmeta")
        && root.text.trim().is_empty()
        && root.children.len() == 1
        && root.children[0].name.is(RDF, "RDF")
    {
        &root.children[0]
    } else {
        return Err(invalid("expected XMP RDF root"));
    };
    if !rdf.text.trim().is_empty() {
        return Err(invalid("text in RDF root"));
    }
    let mut result = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for description in &rdf.children {
        if !description.name.is(RDF, "Description") || !description.text.trim().is_empty() {
            return Err(invalid("expected RDF Description"));
        }
        for (key, text) in &description.attributes {
            if key.0 == RDF && (!key.is(RDF, "about") || !text.is_empty()) {
                return Err(invalid(
                    "only current-document RDF descriptions are supported",
                ));
            }
            if let Some(field) = key.field() {
                if !seen.insert(field) {
                    return Err(invalid("duplicate text property"));
                }
                result.push(Value {
                    field,
                    language: alt(field).then(|| "x-default".into()),
                    text: text.clone(),
                });
            }
        }
        for child in &description.children {
            if let Some(field) = child.name.field() {
                if !seen.insert(field) {
                    return Err(invalid("duplicate text property"));
                }
                result.extend(values(child, field)?);
            }
        }
    }
    if result.len() > 128 {
        return Err(invalid("more than 128 text values"));
    }
    result.sort_by_key(|value| value.field);
    Ok(result)
}

pub(super) fn apply(
    values: &mut Vec<Value>,
    options: &MetadataExportOptions,
) -> Result<(), ExportError> {
    for field in MetadataField::ALL {
        if let Some(text) = options.get(field) {
            if !matches!(
                field,
                MetadataField::Title
                    | MetadataField::Artist
                    | MetadataField::Comment
                    | MetadataField::Copyright
            ) {
                return Err(invalid(format!(
                    "'{}' is not supported; JPEG currently supports Title, Artist, Comment and Copyright",
                    field.label()
                )));
            }
            if !valid_text(text) {
                return Err(invalid("requested value contains invalid XML characters"));
            }
            values.retain(|value| value.field != field);
            if !text.is_empty() {
                values.push(Value {
                    field,
                    language: alt(field).then(|| "x-default".into()),
                    text: text.into(),
                });
            }
        }
    }
    values.sort_by_key(|value| value.field);
    Ok(())
}

fn escaped(text: &str) -> String {
    // Literal CR is normalized by XML; a reference preserves the requested character.
    quick_xml::escape::escape(text).replace('\r', "&#13;")
}

pub(super) fn encode(values: &[Value]) -> Result<Vec<u8>, ExportError> {
    let mut result = format!(
        "<x:xmpmeta xmlns:x=\"{META}\"><rdf:RDF xmlns:rdf=\"{RDF}\"><rdf:Description rdf:about=\"\" xmlns:dc=\"{DC}\">"
    );
    for (field, local) in [
        (MetadataField::Title, "title"),
        (MetadataField::Artist, "creator"),
        (MetadataField::Comment, "description"),
        (MetadataField::Copyright, "rights"),
    ] {
        let items: Vec<_> = values.iter().filter(|value| value.field == field).collect();
        if items.is_empty() {
            continue;
        }
        let array = if alt(field) { "Alt" } else { "Seq" };
        result.push_str(&format!("<dc:{local}><rdf:{array}>"));
        for item in items {
            let language = item
                .language
                .as_ref()
                .map(|language| format!(" xml:lang=\"{}\"", escaped(language)))
                .unwrap_or_default();
            result.push_str(&format!(
                "<rdf:li{language}>{}</rdf:li>",
                escaped(&item.text)
            ));
        }
        result.push_str(&format!("</rdf:{array}></dc:{local}>"));
    }
    result.push_str("</rdf:Description></rdf:RDF></x:xmpmeta>");
    if result.len() > LIMIT {
        return Err(invalid("serialized packet exceeds JPEG APP1 capacity"));
    }
    Ok(result.into_bytes())
}
