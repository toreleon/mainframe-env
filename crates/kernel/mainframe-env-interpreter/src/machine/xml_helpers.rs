use super::*;

pub(super) fn xml_unescape(value: &str) -> Result<String, MachineProblem> {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(at) = rest.find('&') {
        output.push_str(&rest[..at]);
        rest = &rest[at..];
        let (decoded, length) = if rest.starts_with("&amp;") {
            ('&', 5usize)
        } else if rest.starts_with("&lt;") {
            ('<', 4)
        } else if rest.starts_with("&gt;") {
            ('>', 4)
        } else if rest.starts_with("&quot;") {
            ('"', 6)
        } else if rest.starts_with("&apos;") {
            ('\'', 6)
        } else if let Some(reference) = rest.strip_prefix("&#") {
            let end = reference.find(';').ok_or(MachineProblem::DataException)?;
            let (digits, radix) = reference
                .get(..end)
                .and_then(|digits| {
                    digits
                        .strip_prefix(['x', 'X'])
                        .map(|digits| (digits, 16))
                        .or(Some((digits, 10)))
                })
                .ok_or(MachineProblem::DataException)?;
            let scalar = u32::from_str_radix(digits, radix)
                .ok()
                .and_then(char::from_u32)
                .filter(|character| xml_character_allowed(*character))
                .ok_or(MachineProblem::DataException)?;
            (scalar, end + 3)
        } else {
            return Err(MachineProblem::DataException);
        };
        output.push(decoded);
        rest = &rest[length..];
    }
    output.push_str(rest);
    Ok(output)
}

pub(super) fn xml_character_allowed(character: char) -> bool {
    matches!(character, '\u{9}' | '\u{a}' | '\u{d}')
        || ('\u{20}'..='\u{d7ff}').contains(&character)
        || ('\u{e000}'..='\u{fffd}').contains(&character)
        || ('\u{10000}'..='\u{10ffff}').contains(&character)
}

pub(super) fn xml_document(source: &str) -> Result<XmlNode, MachineProblem> {
    let (mut at, _) = xml_declaration(source)?;
    let node = xml_node(source, &mut at, 0)?;
    (at == source.len())
        .then_some(node)
        .ok_or(MachineProblem::DataException)
}

pub(super) fn xml_declaration(source: &str) -> Result<(usize, Vec<XmlEvent>), MachineProblem> {
    if !source.starts_with("<?xml") {
        return Ok((0, Vec::new()));
    }
    let end = source.find("?>").ok_or(MachineProblem::DataException)?;
    let declaration = source.get(2..end).ok_or(MachineProblem::DataException)?;
    let (name, attributes) = xml_opening_tag(declaration)?;
    if name != "xml" {
        return Err(MachineProblem::DataException);
    }
    let mut events = Vec::new();
    let mut version = false;
    for (name, value) in attributes {
        let kind = match name.as_str() {
            "version" if matches!(value.as_str(), "1.0" | "1.1") => {
                version = true;
                "VERSION-INFORMATION"
            }
            "encoding" if !value.is_empty() => "ENCODING-DECLARATION",
            "standalone" if matches!(value.as_str(), "yes" | "no") => "STANDALONE-DECLARATION",
            _ => return Err(MachineProblem::DataException),
        };
        events.push(XmlEvent::new(kind, value.into_bytes()));
    }
    if !version {
        return Err(MachineProblem::DataException);
    }
    Ok((end + 2, events))
}

pub(super) fn xml_node(
    source: &str,
    at: &mut usize,
    depth: usize,
) -> Result<XmlNode, MachineProblem> {
    if depth >= 64 || !source[*at..].starts_with('<') || source[*at..].starts_with("</") {
        return Err(MachineProblem::DataException);
    }
    let open_end = source[*at..]
        .find('>')
        .map(|offset| *at + offset)
        .ok_or(MachineProblem::DataException)?;
    let opening = source
        .get(*at + 1..open_end)
        .ok_or(MachineProblem::DataException)?;
    let empty = opening.trim_end().ends_with('/');
    let opening = if empty {
        opening
            .trim_end()
            .strip_suffix('/')
            .ok_or(MachineProblem::DataException)?
    } else {
        opening
    };
    let (name, attributes) = xml_opening_tag(opening)?;
    *at = open_end + 1;
    if empty {
        return Ok(XmlNode {
            name,
            attributes,
            text: String::new(),
            children: Vec::new(),
        });
    }
    let mut text = String::new();
    let mut children = Vec::new();
    loop {
        let rest = source.get(*at..).ok_or(MachineProblem::DataException)?;
        if rest.starts_with("</") {
            let close_end = rest.find('>').ok_or(MachineProblem::DataException)?;
            if rest.get(2..close_end) != Some(name.as_str()) {
                return Err(MachineProblem::DataException);
            }
            *at += close_end + 1;
            if !children.is_empty() && !text.trim().is_empty() {
                return Err(MachineProblem::DataException);
            }
            return Ok(XmlNode {
                name,
                attributes,
                text: xml_unescape(&text)?,
                children,
            });
        }
        if rest.starts_with('<') {
            children.push(xml_node(source, at, depth + 1)?);
            continue;
        }
        let next = rest.find('<').ok_or(MachineProblem::DataException)?;
        text.push_str(&rest[..next]);
        *at += next;
    }
}

pub(super) fn xml_opening_tag(
    opening: &str,
) -> Result<(String, Vec<(String, String)>), MachineProblem> {
    let bytes = opening.as_bytes();
    let mut at = 0usize;
    let skip_space = |at: &mut usize| {
        while bytes.get(*at).is_some_and(u8::is_ascii_whitespace) {
            *at += 1;
        }
    };
    skip_space(&mut at);
    let name_start = at;
    while bytes.get(at).is_some_and(|byte| {
        !byte.is_ascii_whitespace() && !matches!(*byte, b'/' | b'=' | b'<' | b'>')
    }) {
        at += 1;
    }
    let name = opening
        .get(name_start..at)
        .filter(|name| !name.is_empty())
        .ok_or(MachineProblem::DataException)?
        .to_string();
    let mut attributes = Vec::new();
    let mut names = BTreeSet::new();
    loop {
        skip_space(&mut at);
        if at == bytes.len() {
            return Ok((name, attributes));
        }
        let attribute_start = at;
        while bytes.get(at).is_some_and(|byte| {
            !byte.is_ascii_whitespace() && !matches!(*byte, b'/' | b'=' | b'<' | b'>')
        }) {
            at += 1;
        }
        let attribute = opening
            .get(attribute_start..at)
            .filter(|attribute| !attribute.is_empty())
            .ok_or(MachineProblem::DataException)?
            .to_string();
        if !names.insert(attribute.clone()) || attributes.len() >= 4_096 {
            return Err(MachineProblem::DataException);
        }
        skip_space(&mut at);
        if bytes.get(at) != Some(&b'=') {
            return Err(MachineProblem::DataException);
        }
        at += 1;
        skip_space(&mut at);
        let quote = *bytes
            .get(at)
            .filter(|quote| matches!(quote, b'\'' | b'"'))
            .ok_or(MachineProblem::DataException)?;
        at += 1;
        let value_start = at;
        while bytes.get(at).is_some_and(|byte| *byte != quote) {
            if matches!(bytes[at], b'<' | b'>') {
                return Err(MachineProblem::DataException);
            }
            at += 1;
        }
        let value = opening
            .get(value_start..at)
            .ok_or(MachineProblem::DataException)?;
        if bytes.get(at) != Some(&quote) {
            return Err(MachineProblem::DataException);
        }
        at += 1;
        attributes.push((attribute, xml_unescape(value)?));
    }
}

pub(super) fn xml_processing_target(args: &[String]) -> Option<&str> {
    position(args, "PROCESSING")
        .and_then(|at| {
            args.get(at + 1)
                .filter(|token| token.as_str() == "PROCEDURE")
        })
        .and_then(|_| position(args, "PROCESSING"))
        .and_then(|at| args.get(at + 2))
        .map(String::as_str)
}

pub(super) const fn xml_state_key(pc: usize) -> usize {
    pc | (1usize << (usize::BITS - 1))
}

pub(super) const fn out_of_line_perform_key(pc: usize) -> usize {
    pc | (1usize << (usize::BITS - 2))
}

pub(super) fn declarative_state_key(pc: usize) -> String {
    format!("__COBOL_DECLARATIVE_RETURN_{pc}")
}

pub(super) fn xml_document_events(source: &str) -> Result<Vec<XmlEvent>, MachineProblem> {
    let (_, declaration_events) = xml_declaration(source)?;
    let document = xml_document(source)?;
    let mut events = vec![XmlEvent::new("START-OF-DOCUMENT", Vec::new())];
    events.extend(declaration_events);
    let namespaces =
        BTreeMap::from([("xml".into(), "http://www.w3.org/XML/1998/namespace".into())]);
    append_xml_node_events(&document, &namespaces, &mut events)?;
    events.push(XmlEvent::new("END-OF-DOCUMENT", Vec::new()));
    Ok(events)
}

pub(super) fn append_xml_node_events(
    node: &XmlNode,
    inherited_namespaces: &BTreeMap<String, String>,
    events: &mut Vec<XmlEvent>,
) -> Result<(), MachineProblem> {
    events
        .len()
        .checked_add(2usize.saturating_add(node.attributes.len().saturating_mul(2)))
        .filter(|count| *count <= 65_536)
        .ok_or(MachineProblem::ResourceExhausted)?;
    let mut namespaces = inherited_namespaces.clone();
    for (name, value) in &node.attributes {
        let prefix = if name == "xmlns" {
            Some("")
        } else {
            name.strip_prefix("xmlns:")
        };
        let Some(prefix) = prefix else {
            continue;
        };
        if prefix == "xmlns"
            || (prefix == "xml"
                && value != namespaces.get("xml").ok_or(MachineProblem::DataException)?)
        {
            return Err(MachineProblem::DataException);
        }
        if value.is_empty() {
            namespaces.remove(prefix);
        } else {
            namespaces.insert(prefix.into(), value.clone());
        }
        let mut event = XmlEvent::new("NAMESPACE-DECLARATION", Vec::new());
        event.namespace = value.as_bytes().to_vec();
        event.prefix = prefix.as_bytes().to_vec();
        events.push(event);
    }
    let (prefix, local) = split_xml_name(&node.name)?;
    let namespace = xml_namespace(&namespaces, prefix, true)?;
    let mut start = XmlEvent::new("START-OF-ELEMENT", local.as_bytes().to_vec());
    start.namespace = namespace.as_bytes().to_vec();
    start.prefix = prefix.as_bytes().to_vec();
    events.push(start);
    for (name, value) in &node.attributes {
        if name == "xmlns" || name.starts_with("xmlns:") {
            continue;
        }
        let (prefix, local) = split_xml_name(name)?;
        let namespace = xml_namespace(&namespaces, prefix, false)?;
        let mut attribute = XmlEvent::new("ATTRIBUTE-NAME", local.as_bytes().to_vec());
        attribute.namespace = namespace.as_bytes().to_vec();
        attribute.prefix = prefix.as_bytes().to_vec();
        events.push(attribute);
        events.push(XmlEvent::new(
            "ATTRIBUTE-CHARACTERS",
            value.as_bytes().to_vec(),
        ));
    }
    if !node.text.is_empty() {
        events.push(XmlEvent::new(
            "CONTENT-CHARACTERS",
            node.text.as_bytes().to_vec(),
        ));
    }
    for child in &node.children {
        append_xml_node_events(child, &namespaces, events)?;
    }
    let mut end = XmlEvent::new("END-OF-ELEMENT", local.as_bytes().to_vec());
    end.namespace = namespace.as_bytes().to_vec();
    end.prefix = prefix.as_bytes().to_vec();
    events.push(end);
    Ok(())
}

pub(super) fn split_xml_name(name: &str) -> Result<(&str, &str), MachineProblem> {
    let mut parts = name.split(':');
    let first = parts.next().ok_or(MachineProblem::DataException)?;
    let second = parts.next();
    if first.is_empty() || parts.next().is_some() || second.is_some_and(str::is_empty) {
        return Err(MachineProblem::DataException);
    }
    Ok(second.map_or(("", first), |local| (first, local)))
}

pub(super) fn xml_namespace<'a>(
    namespaces: &'a BTreeMap<String, String>,
    prefix: &str,
    default_for_unprefixed: bool,
) -> Result<&'a str, MachineProblem> {
    if prefix.is_empty() && !default_for_unprefixed {
        return Ok("");
    }
    namespaces
        .get(prefix)
        .map(String::as_str)
        .or_else(|| prefix.is_empty().then_some(""))
        .ok_or(MachineProblem::DataException)
}
