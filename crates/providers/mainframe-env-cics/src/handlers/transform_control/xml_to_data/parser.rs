use super::super::*;

const XSI_NAMESPACE: &str = "http://www.w3.org/2001/XMLSchema-instance";
const WSA_NAMESPACE: &str = "http://www.w3.org/2005/08/addressing";

struct XmlNode {
    name: String,
    attributes: BTreeMap<String, String>,
    children: Vec<XmlNode>,
    text: String,
}

pub(super) struct ParsedXml {
    root: XmlNode,
    pub(super) metadata: CicsXmlTransformMetadata,
    namespaces: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum XmlReadProblem {
    Syntax,
    Conversion,
    ResourceExhausted,
}

struct Cursor<'a> {
    source: &'a str,
    at: usize,
    nodes: usize,
    max_nodes: usize,
}

impl Cursor<'_> {
    fn skip_whitespace(&mut self) {
        while self
            .source
            .as_bytes()
            .get(self.at)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.at += 1;
        }
    }

    fn skip_misc(&mut self) -> Result<(), XmlReadProblem> {
        loop {
            self.skip_whitespace();
            let rest = self.source.get(self.at..).ok_or(XmlReadProblem::Syntax)?;
            if rest.starts_with("<!--") {
                self.skip_comment()?;
            } else if rest.starts_with("<?") {
                self.skip_processing_instruction()?;
            } else {
                return Ok(());
            }
        }
    }

    fn skip_comment(&mut self) -> Result<(), XmlReadProblem> {
        let rest = self.source.get(self.at..).ok_or(XmlReadProblem::Syntax)?;
        let end = rest.find("-->").ok_or(XmlReadProblem::Syntax)?;
        if rest
            .get(4..end)
            .ok_or(XmlReadProblem::Syntax)?
            .contains("--")
        {
            return Err(XmlReadProblem::Syntax);
        }
        self.at += end + 3;
        Ok(())
    }

    fn skip_processing_instruction(&mut self) -> Result<(), XmlReadProblem> {
        let rest = self.source.get(self.at..).ok_or(XmlReadProblem::Syntax)?;
        let end = rest.find("?>").ok_or(XmlReadProblem::Syntax)?;
        let instruction = rest.get(2..end).ok_or(XmlReadProblem::Syntax)?;
        let target = instruction
            .split_ascii_whitespace()
            .next()
            .ok_or(XmlReadProblem::Syntax)?;
        if !valid_xml_name(target) || target.eq_ignore_ascii_case("xml") {
            return Err(XmlReadProblem::Syntax);
        }
        self.at += end + 2;
        Ok(())
    }

    fn node(&mut self, depth: usize) -> Result<XmlNode, XmlReadProblem> {
        if depth >= 16 || self.nodes >= self.max_nodes {
            return Err(XmlReadProblem::ResourceExhausted);
        }
        let rest = self.source.get(self.at..).ok_or(XmlReadProblem::Syntax)?;
        if !rest.starts_with('<')
            || rest.starts_with("</")
            || rest.starts_with("<!")
            || rest.starts_with("<?")
        {
            return Err(XmlReadProblem::Syntax);
        }
        let end = tag_end(self.source, self.at)?;
        let opening = self
            .source
            .get(self.at + 1..end)
            .ok_or(XmlReadProblem::Syntax)?;
        let empty = opening.trim_end().ends_with('/');
        let opening = if empty {
            opening
                .trim_end()
                .strip_suffix('/')
                .ok_or(XmlReadProblem::Syntax)?
        } else {
            opening
        };
        let (name, attributes) = opening_tag(opening)?;
        self.nodes += 1;
        self.at = end + 1;
        if empty {
            return Ok(XmlNode {
                name,
                attributes,
                children: Vec::new(),
                text: String::new(),
            });
        }
        let mut children = Vec::new();
        let mut text = String::new();
        loop {
            let rest = self.source.get(self.at..).ok_or(XmlReadProblem::Syntax)?;
            if rest.starts_with("</") {
                let close_end = rest.find('>').ok_or(XmlReadProblem::Syntax)?;
                if rest.get(2..close_end) != Some(name.as_str()) {
                    return Err(XmlReadProblem::Syntax);
                }
                self.at += close_end + 1;
                if !children.is_empty() && !text.trim().is_empty() {
                    return Err(XmlReadProblem::Syntax);
                }
                return Ok(XmlNode {
                    name,
                    attributes,
                    children,
                    text,
                });
            }
            if rest.starts_with("<!--") {
                self.skip_comment()?;
                continue;
            }
            if rest.starts_with("<?") {
                self.skip_processing_instruction()?;
                continue;
            }
            if let Some(content) = rest.strip_prefix("<![CDATA[") {
                let end = content.find("]]>").ok_or(XmlReadProblem::Syntax)?;
                let value = content.get(..end).ok_or(XmlReadProblem::Syntax)?;
                if !value.chars().all(xml_character_allowed) {
                    return Err(XmlReadProblem::Syntax);
                }
                text.push_str(value);
                self.at += 9 + end + 3;
                continue;
            }
            if rest.starts_with('<') {
                children.push(self.node(depth + 1)?);
                continue;
            }
            let next = rest.find('<').ok_or(XmlReadProblem::Syntax)?;
            text.push_str(&unescape(&rest[..next])?);
            if text.len() > self.source.len() {
                return Err(XmlReadProblem::ResourceExhausted);
            }
            self.at += next;
        }
    }
}

pub(super) fn parse_xml(
    source: &str,
    namespace_source: Option<&str>,
    limits: CicsLimits,
) -> Result<ParsedXml, XmlReadProblem> {
    if source.len() > limits.max_transform_bytes {
        return Err(XmlReadProblem::ResourceExhausted);
    }
    let mut namespaces = namespace_source
        .map(|source| namespace_list(source, limits))
        .transpose()?
        .unwrap_or_default();
    let mut cursor = Cursor {
        source,
        at: 0,
        nodes: 0,
        max_nodes: limits.max_fields.saturating_mul(4).max(1),
    };
    cursor.skip_whitespace();
    if source
        .get(cursor.at..)
        .is_some_and(|rest| rest.starts_with("<?xml"))
    {
        let rest = source.get(cursor.at..).ok_or(XmlReadProblem::Syntax)?;
        let end = rest.find("?>").ok_or(XmlReadProblem::Syntax)?;
        let declaration = rest.get(2..end).ok_or(XmlReadProblem::Syntax)?;
        let (name, attributes) = opening_tag(declaration)?;
        if name != "xml"
            || attributes.get("version").map(String::as_str) != Some("1.0")
            || attributes
                .keys()
                .any(|key| !matches!(key.as_str(), "version" | "encoding" | "standalone"))
            || attributes
                .get("encoding")
                .is_some_and(|encoding| !encoding.eq_ignore_ascii_case("UTF-8"))
            || attributes
                .get("standalone")
                .is_some_and(|value| !matches!(value.as_str(), "yes" | "no"))
        {
            return Err(XmlReadProblem::Syntax);
        }
        cursor.at += end + 2;
        cursor.skip_whitespace();
    }
    cursor.skip_misc()?;
    let root = cursor.node(0)?;
    cursor.skip_misc()?;
    if cursor.at != source.len() {
        return Err(XmlReadProblem::Syntax);
    }
    merge_namespaces(&mut namespaces, &root.attributes)?;
    let (element_name, element_namespace) = resolve_qname(&root.name, &namespaces, true)?;
    let mut type_value = None;
    for (name, value) in &root.attributes {
        if name == "xmlns" || name.starts_with("xmlns:") {
            continue;
        }
        let (local, namespace) = resolve_qname(name, &namespaces, false)?;
        if local == "type"
            && namespace == XSI_NAMESPACE
            && type_value.replace(value.as_str()).is_some()
        {
            return Err(XmlReadProblem::Syntax);
        }
    }
    let (type_name, type_namespace) = match type_value {
        Some(value) => {
            let (name, namespace) = resolve_qname(value, &namespaces, false)?;
            (Some(name), Some(namespace))
        }
        None => (None, None),
    };
    Ok(ParsedXml {
        root,
        metadata: CicsXmlTransformMetadata {
            element_name,
            element_namespace,
            type_name,
            type_namespace,
        },
        namespaces,
    })
}

pub(super) fn endpoint_fields(
    source: &str,
    limits: CicsLimits,
) -> Option<BTreeMap<String, Vec<u8>>> {
    let parsed = parse_xml(source, None, limits).ok()?;
    if parsed.metadata.element_name != "EndpointReference"
        || parsed.metadata.element_namespace != WSA_NAMESPACE
    {
        return None;
    }
    let mut fields = BTreeMap::new();
    for child in &parsed.root.children {
        let mut namespaces = parsed.namespaces.clone();
        merge_namespaces(&mut namespaces, &child.attributes).ok()?;
        let (name, namespace) = resolve_qname(&child.name, &namespaces, true).ok()?;
        if namespace != WSA_NAMESPACE {
            continue;
        }
        let (key, bytes) = match name.as_str() {
            "Address" if child.children.is_empty() => ("ADDRESS", child.text.as_bytes().to_vec()),
            "Metadata" => (
                "METADATA",
                serialize_endpoint_child(child, &parsed.root.attributes),
            ),
            "ReferenceParameters" => (
                "REFPARMS",
                serialize_endpoint_child(child, &parsed.root.attributes),
            ),
            _ => continue,
        };
        if fields.insert(key.into(), bytes).is_some() {
            return None;
        }
    }
    fields.contains_key("ADDRESS").then_some(fields)
}

fn serialize_endpoint_child(node: &XmlNode, inherited: &BTreeMap<String, String>) -> Vec<u8> {
    let mut output = String::new();
    let mut attributes = node.attributes.clone();
    for (name, value) in inherited {
        if name == "xmlns" || name.starts_with("xmlns:") {
            attributes
                .entry(name.clone())
                .or_insert_with(|| value.clone());
        }
    }
    serialize_node(node, &attributes, &mut output);
    output.into_bytes()
}

fn serialize_node(node: &XmlNode, attributes: &BTreeMap<String, String>, output: &mut String) {
    output.push('<');
    output.push_str(&node.name);
    for (name, value) in attributes {
        output.push(' ');
        output.push_str(name);
        output.push_str("=\"");
        escape_xml(value, output);
        output.push('"');
    }
    output.push('>');
    escape_xml(&node.text, output);
    for child in &node.children {
        serialize_node(child, &child.attributes, output);
    }
    output.push_str("</");
    output.push_str(&node.name);
    output.push('>');
}

fn escape_xml(value: &str, output: &mut String) {
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&apos;"),
            _ => output.push(character),
        }
    }
}

fn namespace_list(
    source: &str,
    limits: CicsLimits,
) -> Result<BTreeMap<String, String>, XmlReadProblem> {
    if source.len() > limits.max_transform_bytes {
        return Err(XmlReadProblem::ResourceExhausted);
    }
    let wrapped = format!("<ns {source}/>");
    let mut cursor = Cursor {
        source: &wrapped,
        at: 0,
        nodes: 0,
        max_nodes: 1,
    };
    let node = cursor.node(0)?;
    if cursor.at != wrapped.len()
        || node.name != "ns"
        || node
            .attributes
            .keys()
            .any(|name| name != "xmlns" && !name.starts_with("xmlns:"))
    {
        return Err(XmlReadProblem::Syntax);
    }
    let mut namespaces = BTreeMap::new();
    merge_namespaces(&mut namespaces, &node.attributes)?;
    Ok(namespaces)
}

fn merge_namespaces(
    namespaces: &mut BTreeMap<String, String>,
    attributes: &BTreeMap<String, String>,
) -> Result<(), XmlReadProblem> {
    for (name, value) in attributes {
        let prefix = if name == "xmlns" {
            ""
        } else if let Some(prefix) = name.strip_prefix("xmlns:") {
            if !valid_xml_local_name(prefix) {
                return Err(XmlReadProblem::Syntax);
            }
            prefix
        } else {
            continue;
        };
        namespaces.insert(prefix.to_string(), value.clone());
    }
    Ok(())
}

fn resolve_qname(
    name: &str,
    namespaces: &BTreeMap<String, String>,
    default_namespace: bool,
) -> Result<(String, String), XmlReadProblem> {
    if let Some((prefix, local)) = name.split_once(':') {
        if !valid_xml_local_name(prefix) || !valid_xml_local_name(local) || local.contains(':') {
            return Err(XmlReadProblem::Syntax);
        }
        let namespace = namespaces.get(prefix).ok_or(XmlReadProblem::Syntax)?;
        Ok((local.to_string(), namespace.clone()))
    } else if valid_xml_local_name(name) {
        Ok((
            name.to_string(),
            if default_namespace {
                namespaces.get("").cloned().unwrap_or_default()
            } else {
                String::new()
            },
        ))
    } else {
        Err(XmlReadProblem::Syntax)
    }
}

fn tag_end(source: &str, start: usize) -> Result<usize, XmlReadProblem> {
    let mut quote = None;
    for (offset, byte) in source.as_bytes()[start + 1..].iter().enumerate() {
        match (*byte, quote) {
            (b'\'' | b'"', None) => quote = Some(*byte),
            (byte, Some(active)) if byte == active => quote = None,
            (b'>', None) => return Ok(start + 1 + offset),
            _ => {}
        }
    }
    Err(XmlReadProblem::Syntax)
}

fn opening_tag(opening: &str) -> Result<(String, BTreeMap<String, String>), XmlReadProblem> {
    let bytes = opening.as_bytes();
    let mut at = 0usize;
    skip_space(bytes, &mut at);
    let start = at;
    while bytes.get(at).is_some_and(|byte| {
        !byte.is_ascii_whitespace() && !matches!(*byte, b'/' | b'=' | b'<' | b'>')
    }) {
        at += 1;
    }
    let name = opening.get(start..at).ok_or(XmlReadProblem::Syntax)?;
    if !valid_xml_name(name) {
        return Err(XmlReadProblem::Syntax);
    }
    let mut attributes = BTreeMap::new();
    loop {
        skip_space(bytes, &mut at);
        if at == bytes.len() {
            return Ok((name.to_string(), attributes));
        }
        let start = at;
        while bytes.get(at).is_some_and(|byte| {
            !byte.is_ascii_whitespace() && !matches!(*byte, b'/' | b'=' | b'<' | b'>')
        }) {
            at += 1;
        }
        let attribute = opening.get(start..at).ok_or(XmlReadProblem::Syntax)?;
        if !valid_xml_name(attribute) || attributes.len() >= 256 {
            return Err(XmlReadProblem::Syntax);
        }
        skip_space(bytes, &mut at);
        if bytes.get(at) != Some(&b'=') {
            return Err(XmlReadProblem::Syntax);
        }
        at += 1;
        skip_space(bytes, &mut at);
        let quote = *bytes
            .get(at)
            .filter(|byte| matches!(byte, b'\'' | b'"'))
            .ok_or(XmlReadProblem::Syntax)?;
        at += 1;
        let start = at;
        while bytes.get(at).is_some_and(|byte| *byte != quote) {
            if bytes[at] == b'<' {
                return Err(XmlReadProblem::Syntax);
            }
            at += 1;
        }
        let value = opening.get(start..at).ok_or(XmlReadProblem::Syntax)?;
        if bytes.get(at) != Some(&quote) {
            return Err(XmlReadProblem::Syntax);
        }
        at += 1;
        if attributes
            .insert(attribute.to_string(), unescape(value)?)
            .is_some()
        {
            return Err(XmlReadProblem::Syntax);
        }
    }
}

fn valid_xml_name(name: &str) -> bool {
    if let Some((prefix, local)) = name.split_once(':') {
        valid_xml_local_name(prefix) && valid_xml_local_name(local) && !local.contains(':')
    } else {
        valid_xml_local_name(name)
    }
}

fn valid_xml_local_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 255 {
        return false;
    }
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|character| character.is_alphabetic() || character == '_')
        && characters
            .all(|character| character.is_alphanumeric() || matches!(character, '_' | '-' | '.'))
}

fn skip_space(bytes: &[u8], at: &mut usize) {
    while bytes.get(*at).is_some_and(u8::is_ascii_whitespace) {
        *at += 1;
    }
}

fn unescape(value: &str) -> Result<String, XmlReadProblem> {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(at) = rest.find('&') {
        output.push_str(&rest[..at]);
        rest = &rest[at..];
        let (character, length) = if rest.starts_with("&amp;") {
            ('&', 5)
        } else if rest.starts_with("&lt;") {
            ('<', 4)
        } else if rest.starts_with("&gt;") {
            ('>', 4)
        } else if rest.starts_with("&quot;") {
            ('"', 6)
        } else if rest.starts_with("&apos;") {
            ('\'', 6)
        } else if let Some(reference) = rest.strip_prefix("&#") {
            let end = reference.find(';').ok_or(XmlReadProblem::Syntax)?;
            let digits = reference.get(..end).ok_or(XmlReadProblem::Syntax)?;
            let (digits, radix) = digits
                .strip_prefix(['x', 'X'])
                .map(|digits| (digits, 16))
                .unwrap_or((digits, 10));
            let character = u32::from_str_radix(digits, radix)
                .ok()
                .and_then(char::from_u32)
                .filter(|character| xml_character_allowed(*character))
                .ok_or(XmlReadProblem::Syntax)?;
            (character, end + 3)
        } else {
            return Err(XmlReadProblem::Syntax);
        };
        output.push(character);
        rest = &rest[length..];
    }
    output.push_str(rest);
    if !output.chars().all(xml_character_allowed) {
        return Err(XmlReadProblem::Syntax);
    }
    Ok(output)
}

pub(super) fn data_from_xml(
    definition: &CicsTransformDefinition,
    parsed: &ParsedXml,
    limits: CicsLimits,
) -> Result<Vec<u8>, XmlReadProblem> {
    if parsed.root.children.len() != definition.fields.len() {
        return Err(XmlReadProblem::Conversion);
    }
    let length = definition
        .fields
        .iter()
        .map(|field| field.offset.checked_add(field.length))
        .collect::<Option<Vec<_>>>()
        .ok_or(XmlReadProblem::ResourceExhausted)?
        .into_iter()
        .max()
        .ok_or(XmlReadProblem::Conversion)?;
    if length > limits.max_transform_bytes {
        return Err(XmlReadProblem::ResourceExhausted);
    }
    let mut fields = BTreeMap::new();
    for child in &parsed.root.children {
        if !child.children.is_empty() {
            return Err(XmlReadProblem::Conversion);
        }
        let mut namespaces = parsed.namespaces.clone();
        merge_namespaces(&mut namespaces, &child.attributes)?;
        let (name, namespace) = resolve_qname(&child.name, &namespaces, true)?;
        if namespace != parsed.metadata.element_namespace
            || fields.insert(name, child.text.as_str()).is_some()
        {
            return Err(XmlReadProblem::Conversion);
        }
    }
    let mut data = vec![b' '; length];
    for field in &definition.fields {
        let value = fields.get(&field.name).ok_or(XmlReadProblem::Conversion)?;
        let encoded = match field.kind {
            CicsTransformFieldKind::Text => value.as_bytes().to_vec(),
            CicsTransformFieldKind::SignedInteger => {
                let number = value
                    .trim()
                    .parse::<i64>()
                    .map_err(|_| XmlReadProblem::Conversion)?;
                format!("{number:0width$}", width = field.length).into_bytes()
            }
        };
        if encoded.len() > field.length {
            return Err(XmlReadProblem::Conversion);
        }
        let target = &mut data[field.offset..field.offset + field.length];
        target[..encoded.len()].copy_from_slice(&encoded);
    }
    Ok(data)
}
