use super::*;
use quick_xml::events::Event;
use quick_xml::name::{LocalName, ResolveResult};
use quick_xml::reader::NsReader;

pub(super) const LIMIT: usize = 65502;
pub(super) const FIELDS: [MetadataField; 9] = [
    MetadataField::Title,
    MetadataField::Artist,
    MetadataField::Album,
    MetadataField::Composer,
    MetadataField::Genre,
    MetadataField::Date,
    MetadataField::Track,
    MetadataField::Comment,
    MetadataField::Copyright,
];
const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const DC: &str = "http://purl.org/dc/elements/1.1/";
const DM: &str = "http://ns.adobe.com/xmp/1.0/DynamicMedia/";
const XML: &str = "http://www.w3.org/XML/1998/namespace";
const META: &str = "adobe:ns:meta/";
const RIGHTS: &str = "http://ns.adobe.com/xap/1.0/rights/";

#[path = "export_xmp_typed.rs"]
mod typed;

#[cfg(test)]
#[path = "export_xmp_tests.rs"]
mod tests;

fn invalid(message: impl std::fmt::Display) -> ExportError {
    ExportError::Failed(format!("XMP metadata: {message}"))
}

#[derive(Debug, PartialEq, Eq)]
struct Name(String, String);

impl Name {
    fn is(&self, namespace: &str, local: &str) -> bool {
        self.0 == namespace && self.1 == local
    }

    fn field(&self) -> Option<MetadataField> {
        match (self.0.as_str(), self.1.as_str()) {
            (DC, "title") => Some(MetadataField::Title),
            (DC, "creator") => Some(MetadataField::Artist),
            (DC, "description") => Some(MetadataField::Comment),
            (DC, "rights") => Some(MetadataField::Copyright),
            (DM, "album") => Some(MetadataField::Album),
            (DM, "composer") => Some(MetadataField::Composer),
            (DM, "genre") => Some(MetadataField::Genre),
            (DM, "releaseDate") => Some(MetadataField::Date),
            (DM, "trackNumber") => Some(MetadataField::Track),
            _ => None,
        }
    }

    fn retained_descriptive_property(&self) -> bool {
        (self.0 == DC && matches!(self.1.as_str(), "subject" | "contributor" | "publisher"))
            || (self.0 == RIGHTS
                && matches!(
                    self.1.as_str(),
                    "Owner" | "UsageTerms" | "WebStatement" | "Marked"
                ))
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
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.starts_with('\u{feff}') {
        return Err(invalid("multiple UTF-8 byte order marks"));
    }
    let mut reader = NsReader::from_str(text);
    reader.config_mut().expand_empty_elements = true;
    reader.config_mut().check_comments = true;
    reader.resolver_mut().set_max_declarations_per_element(64);
    let mut stack: Vec<Node> = Vec::new();
    let mut root = None;
    let mut count = 0;
    loop {
        check_cancelled(cancelled)?;
        // XML declarations precede even whitespace, comments and XMP packet PIs.
        let declaration_allowed = reader.buffer_position() == 0;
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
                    || !declaration_allowed
                {
                    return Err(invalid(
                        "only an initial XML 1.0 UTF-8 declaration is supported",
                    ));
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

pub(super) fn display_values(
    values: Vec<Value>,
    format: ImageMetadataFormat,
) -> Vec<MetadataSourceValue> {
    let mut creator = 0;
    values
        .into_iter()
        .map(|value| {
            let label = format.label();
            let scope = if let Some(language) = value.language {
                // The XMP reader accepts only ASCII language tags.
                format!(
                    "{label} XMP ({}{})",
                    &language[..language.len().min(63)],
                    if language.len() > 63 { "…" } else { "" }
                )
            } else if value.field == MetadataField::Artist {
                creator += 1;
                format!("{label} XMP (creator {creator})")
            } else {
                format!("{label} XMP")
            };
            MetadataSourceValue {
                field: value.field,
                scope,
                truncated: value.text.len() > 1024,
                value: value.text[..value.text.floor_char_boundary(1024)].to_owned(),
            }
        })
        .collect()
}

fn alt(field: MetadataField) -> bool {
    matches!(
        field,
        MetadataField::Title | MetadataField::Comment | MetadataField::Copyright
    )
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
    if !alt(field) && field != MetadataField::Artist {
        return Err(invalid("expected a simple Dynamic Media text property"));
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
    // Even rdf:about="" resolves against an inherited base URI. This parser has
    // no document URI with which to prove a nonempty base still names this image.
    let changes_base = |node: &Node| {
        node.attributes
            .iter()
            .any(|(key, text)| key.is(XML, "base") && !text.is_empty())
    };
    if changes_base(&root) || changes_base(rdf) || rdf.children.iter().any(changes_base) {
        return Err(invalid(
            "nonempty xml:base is unsupported for current-document descriptions",
        ));
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
            if !FIELDS.contains(&field) {
                return Err(invalid(format!(
                    "'{}' is not supported; XMP currently supports Title, Artist, Album, Composer, Genre, Date, Track, Comment and Copyright",
                    field.label()
                )));
            }
            if !valid_text(text) {
                return Err(invalid("requested value contains invalid XML characters"));
            }
            if !text.is_empty() {
                typed::validate(field, text)?;
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
    let result = format!(
        "<x:xmpmeta xmlns:x=\"{META}\"><rdf:RDF xmlns:rdf=\"{RDF}\">{}</rdf:RDF></x:xmpmeta>",
        description(values)
    );
    if result.len() > LIMIT {
        return Err(invalid("serialized packet exceeds 65502 bytes"));
    }
    Ok(result.into_bytes())
}

fn description(values: &[Value]) -> String {
    format!(
        "<rdf:Description xmlns:rdf=\"{RDF}\" rdf:about=\"\" xmlns:dc=\"{DC}\" xmlns:xmpDM=\"{DM}\">{}</rdf:Description>",
        properties(values, false)
    )
}

fn properties(values: &[Value], self_contained: bool) -> String {
    let mut result = String::new();
    let dc_namespace = if self_contained {
        format!(" xmlns:dc=\"{DC}\" xmlns:rdf=\"{RDF}\"")
    } else {
        String::new()
    };
    let dm_namespace = if self_contained {
        format!(" xmlns:xmpDM=\"{DM}\"")
    } else {
        String::new()
    };
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
        result.push_str(&format!("<dc:{local}{dc_namespace}><rdf:{array}>"));
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
    for (field, local) in [
        (MetadataField::Album, "album"),
        (MetadataField::Composer, "composer"),
        (MetadataField::Genre, "genre"),
        (MetadataField::Date, "releaseDate"),
        (MetadataField::Track, "trackNumber"),
    ] {
        if let Some(value) = values.iter().find(|value| value.field == field) {
            result.push_str(&format!(
                "<xmpDM:{local}{dm_namespace}>{}</xmpDM:{local}>",
                escaped(&value.text)
            ));
        }
    }
    result
}

/// Change only requested top-level properties; keep opaque XML and namespace scopes.
/// Call only when source pixels and format are unchanged, so technical tags stay valid.
pub(super) fn rewrite_unedited(
    packet: &[u8],
    options: &MetadataExportOptions,
    cancelled: &AtomicBool,
) -> Result<Vec<u8>, ExportError> {
    rewrite(packet, options, cancelled, true)
}

/// Carry descriptive text, attribution, keywords and rights expressions across raster/format
/// changes, not technical geometry, asset identifiers or original certificates.
pub(super) fn rewrite_edited(
    packet: &[u8],
    options: &MetadataExportOptions,
    cancelled: &AtomicBool,
) -> Result<Vec<u8>, ExportError> {
    rewrite(packet, options, cancelled, false)
}

fn rewrite(
    packet: &[u8],
    options: &MetadataExportOptions,
    cancelled: &AtomicBool,
    unchanged_pixels: bool,
) -> Result<Vec<u8>, ExportError> {
    let original = parse(packet, cancelled)?;
    let mut expected = original.clone();
    apply(&mut expected, options)?;
    // Empty RDF lists have no parsed values, but Remove must still delete the property.
    let removes = FIELDS.iter().any(|field| options.get(*field) == Some(""));
    if unchanged_pixels && expected == original && !removes {
        return Ok(packet.to_vec());
    }
    let changed: Vec<_> = expected
        .iter()
        .filter(|value| options.get(value.field).is_some())
        .cloned()
        .collect();
    let additions = properties(&changed, true);
    let new_description = if additions.is_empty() {
        String::new()
    } else {
        format!(
            "<rdf:Description xmlns:rdf=\"{RDF}\" rdf:about=\"\" xml:lang=\"\">{additions}</rdf:Description>"
        )
    };
    let mut reader = NsReader::from_reader(packet);
    let mut writer = quick_xml::Writer::new(Vec::new());
    if packet.starts_with(b"\xef\xbb\xbf") {
        writer.get_mut().extend_from_slice(b"\xef\xbb\xbf");
    }
    let mut depth = 0;
    let mut rdf_depth = 0;
    let mut skip = None;
    let mut inserted = false;
    let mut reusable_description = false;
    let mut inherited_language = false;
    let mut retained_property = false;
    let remove = |key: &Name| {
        key.field().map_or_else(
            || !unchanged_pixels && !key.retained_descriptive_property(),
            |field| options.get(field).is_some(),
        )
    };
    loop {
        check_cancelled(cancelled)?;
        let event = reader.read_event().map_err(invalid)?;
        let empty = matches!(&event, Event::Empty(_));
        match event {
            Event::Start(mut start) | Event::Empty(mut start) => {
                let key = name(reader.resolver().resolve_element(start.name()))?;
                if depth == 0 {
                    rdf_depth = usize::from(!key.is(RDF, "RDF"));
                }
                // XMP wrappers and RDF roots can both supply a language. An empty
                // declaration cancels inheritance; newly set plain values have none.
                if depth <= rdf_depth {
                    for attribute in start.attributes() {
                        let attribute = attribute.map_err(invalid)?;
                        if attribute.key.as_ref() == b"xml:lang" {
                            inherited_language = !attribute.value.is_empty();
                        }
                    }
                }
                if skip.is_none() && depth == rdf_depth + 2 && remove(&key) {
                    if !empty {
                        skip = Some(depth);
                    }
                } else if skip.is_none() {
                    retained_property |= depth == rdf_depth + 2;
                    if depth == rdf_depth + 1 {
                        reusable_description = !inserted && !additions.is_empty();
                        let mut attributes = Vec::new();
                        let mut removed = false;
                        let mut language_reset = false;
                        for attribute in start.attributes() {
                            let attribute = attribute.map_err(invalid)?;
                            let namespace = attribute.key.as_ref() == b"xmlns"
                                || attribute.key.as_ref().starts_with(b"xmlns:");
                            let key = (!namespace)
                                .then(|| name(reader.resolver().resolve_attribute(attribute.key)))
                                .transpose()?;
                            // Do not give newly set properties another description's
                            // inherited xml:lang/base/space context.
                            if key.as_ref().is_some_and(|key| key.is(XML, "lang"))
                                && attribute.value.is_empty()
                            {
                                language_reset = true;
                            } else if key.as_ref().is_some_and(|key| key.0 == XML) {
                                reusable_description = false;
                            }
                            if key
                                .as_ref()
                                .is_some_and(|key| key.0 != RDF && key.0 != XML && remove(key))
                            {
                                removed = true;
                            } else {
                                retained_property |= key.as_ref().is_some_and(|key| {
                                    key.field().is_some() || key.retained_descriptive_property()
                                });
                                attributes.push((
                                    attribute.key.as_ref().to_vec(),
                                    attribute.value.into_owned(),
                                ));
                            }
                        }
                        reusable_description &= !inherited_language || language_reset;
                        if removed {
                            start = start.into_owned();
                            start.clear_attributes();
                            for (key, value) in &attributes {
                                start.push_attribute((key.as_slice(), value.as_slice()));
                            }
                        }
                    }
                    if empty
                        && ((depth == rdf_depth && !new_description.is_empty())
                            || (depth == rdf_depth + 1 && reusable_description))
                    {
                        writer
                            .write_event(Event::Start(start.borrow()))
                            .map_err(invalid)?;
                        writer.get_mut().extend_from_slice(if depth == rdf_depth {
                            new_description.as_bytes()
                        } else {
                            additions.as_bytes()
                        });
                        inserted = true;
                        writer
                            .write_event(Event::End(start.to_end()))
                            .map_err(invalid)?;
                    } else {
                        writer
                            .write_event(if empty {
                                Event::Empty(start)
                            } else {
                                Event::Start(start)
                            })
                            .map_err(invalid)?;
                    }
                }
                if !empty {
                    depth += 1;
                }
            }
            Event::End(end) => {
                depth -= 1;
                if skip == Some(depth) {
                    skip = None;
                } else if skip.is_none() {
                    if depth == rdf_depth + 1 && reusable_description {
                        writer.get_mut().extend_from_slice(additions.as_bytes());
                        inserted = true;
                    } else if depth == rdf_depth && !inserted {
                        writer
                            .get_mut()
                            .extend_from_slice(new_description.as_bytes());
                    }
                    writer.write_event(Event::End(end)).map_err(invalid)?;
                }
            }
            Event::Eof => break,
            event if skip.is_none() => writer.write_event(event).map_err(invalid)?,
            _ => {}
        }
        if writer.get_ref().len() > LIMIT {
            return Err(invalid("rewritten packet exceeds 65502 bytes"));
        }
    }
    let output = writer.into_inner();
    if parse(&output, cancelled)? != expected {
        return Err(invalid("rewritten packet differs from requested values"));
    }
    if !unchanged_pixels && !retained_property && expected.is_empty() {
        return Ok(Vec::new());
    }
    Ok(output)
}
