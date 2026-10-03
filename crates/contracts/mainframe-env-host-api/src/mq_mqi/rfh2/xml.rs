//! Bounded flat XML subset. No DTD, external entity, tree, namespace dispatch,
//! normalization, or ignored metadata; unrepresented syntax is explicit pending.
use super::*;

fn scalar(value: &MqPropertyData) -> Result<String, MqPropertyProblem> {
    use crate::MqPropertyType as T;
    Ok(match value.kind {
        T::Null => String::new(),
        T::ByteString => value.bytes.iter().map(|v| format!("{v:02X}")).collect(),
        T::String => std::str::from_utf8(&value.bytes)
            .map_err(|_| MqPropertyProblem::Value)?
            .to_owned(),
        T::Int8 => i8::from_be_bytes(
            value
                .bytes
                .as_slice()
                .try_into()
                .map_err(|_| MqPropertyProblem::Value)?,
        )
        .to_string(),
        T::Int16 => i16::from_be_bytes(
            value
                .bytes
                .as_slice()
                .try_into()
                .map_err(|_| MqPropertyProblem::Value)?,
        )
        .to_string(),
        T::Int32 => i32::from_be_bytes(
            value
                .bytes
                .as_slice()
                .try_into()
                .map_err(|_| MqPropertyProblem::Value)?,
        )
        .to_string(),
        T::Int64 => i64::from_be_bytes(
            value
                .bytes
                .as_slice()
                .try_into()
                .map_err(|_| MqPropertyProblem::Value)?,
        )
        .to_string(),
        _ => return Err(MqPropertyProblem::Unsupported),
    })
}
fn attribute(value: &MqPropertyData) -> Result<String, MqPropertyProblem> {
    if value.kind == crate::MqPropertyType::Null {
        return Ok(" xsi:nil='true'".to_owned());
    }
    Ok(format!(
        " dt='{}'",
        generated::lexical(value.kind).ok_or(MqPropertyProblem::Unsupported)?
    ))
}
pub(super) fn required(
    name: &MqPropertyName,
    pd: &MqPropertyDescriptor,
    value: &MqPropertyData,
    limits: MqMessageLimits,
) -> Result<usize, MqPropertyProblem> {
    custom_name(name, limits)?;
    pd.validate_input()?;
    value.validate(limits)?;
    if value.kind == crate::MqPropertyType::String
        && !value.bytes.iter().all(|b| (b' '..=b'~').contains(b))
    {
        return Err(MqPropertyProblem::Unsupported);
    }
    let (folder, leaf) = name
        .as_str()
        .split_once('.')
        .ok_or(MqPropertyProblem::Name)?;
    // For byte strings, count hex length without creating its expanded text.
    let content = if value.kind == crate::MqPropertyType::ByteString {
        value
            .bytes
            .len()
            .checked_mul(2)
            .ok_or(MqPropertyProblem::Capacity)?
    } else {
        let numeric;
        let bytes = if value.kind == crate::MqPropertyType::String {
            value.bytes.as_slice()
        } else {
            numeric = scalar(value)?;
            numeric.as_bytes()
        };
        bytes.iter().enumerate().try_fold(0usize, |n, (i, b)| {
            n.checked_add(match b {
                b'&' => 5,
                b'<' => 4,
                b'>' if i >= 2 && &bytes[i - 2..i] == b"]]" => 4,
                _ => 1,
            })
            .ok_or(MqPropertyProblem::Capacity)
        })?
    };
    // Exact tag spelling shared with render; no output bytes are allocated here.
    let tags = format!(
        "<{folder} content='properties'><{leaf}{}></{leaf}></{folder}>",
        attribute(value)?
    );
    tags.len()
        .checked_add(content)
        .ok_or(MqPropertyProblem::Capacity)
}
pub(super) fn render(
    name: &MqPropertyName,
    value: &MqPropertyData,
) -> Result<String, MqPropertyProblem> {
    let (folder, leaf) = name
        .as_str()
        .split_once('.')
        .ok_or(MqPropertyProblem::Name)?;
    let text = scalar(value)?
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace("]]>", "]]&gt;");
    Ok(format!(
        "<{folder} content='properties'><{leaf}{}>{text}</{leaf}></{folder}>",
        attribute(value)?
    ))
}
fn tag(input: &str) -> Result<(&str, Vec<(&str, &str)>), MqPropertyProblem> {
    let split = input.find(char::is_whitespace).unwrap_or(input.len());
    let name = &input[..split];
    let mut tail = input[split..].trim();
    let mut attrs = Vec::new();
    if name.is_empty() {
        return Err(MqPropertyProblem::Value);
    }
    while !tail.is_empty() {
        if attrs.len() == 2 {
            return Err(MqPropertyProblem::Unsupported);
        }
        let (key, rest) = tail.split_once('=').ok_or(MqPropertyProblem::Value)?;
        let rest = rest.trim_start();
        let quote = rest
            .as_bytes()
            .first()
            .copied()
            .ok_or(MqPropertyProblem::Value)?;
        if !matches!(quote, b'\'' | b'"') {
            return Err(MqPropertyProblem::Value);
        }
        let end = rest[1..]
            .find(quote as char)
            .ok_or(MqPropertyProblem::Value)?
            + 1;
        let key = key.trim();
        if attrs.iter().any(|(k, _)| *k == key) {
            return Err(MqPropertyProblem::Value);
        }
        attrs.push((key, &rest[1..end]));
        tail = rest[end + 1..].trim_start();
    }
    Ok((name, attrs))
}
fn unescape(input: &str, ceiling: usize) -> Result<Vec<u8>, MqPropertyProblem> {
    if input.contains("]]>") {
        return Err(MqPropertyProblem::Unsupported);
    }
    let mut out = Vec::with_capacity(input.len().min(ceiling));
    let mut input = input;
    while !input.is_empty() {
        if out.len() == ceiling {
            return Err(MqPropertyProblem::Capacity);
        }
        if let Some(rest) = input.strip_prefix('&') {
            let (entity, tail) = rest.split_once(';').ok_or(MqPropertyProblem::Value)?;
            out.push(match entity {
                "amp" => b'&',
                "lt" => b'<',
                "gt" => b'>',
                "apos" => b'\'',
                "quot" => b'"',
                _ => return Err(MqPropertyProblem::Unsupported),
            });
            input = tail;
        } else {
            let byte = input.as_bytes()[0];
            if !(b' '..=b'~').contains(&byte) || byte == b'<' {
                return Err(MqPropertyProblem::Unsupported);
            }
            out.push(byte);
            input = &input[1..];
        }
    }
    Ok(out)
}
pub(super) fn parse(
    bytes: &[u8],
    limits: MqMessageLimits,
) -> Result<(MqPropertyName, MqPropertyDescriptor, MqPropertyData), MqPropertyProblem> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| MqPropertyProblem::Value)?
        .trim_end_matches(' ');
    if text.starts_with("<?") || text.starts_with("<!") {
        return Err(MqPropertyProblem::Unsupported);
    }
    let (folder_tag, rest) = text
        .strip_prefix('<')
        .ok_or(MqPropertyProblem::Value)?
        .split_once('>')
        .ok_or(MqPropertyProblem::Value)?;
    let (folder, attrs) = tag(folder_tag)?;
    if attrs != [("content", "properties")] {
        return Err(MqPropertyProblem::Unsupported);
    }
    let end = format!("</{folder}>");
    let inside = rest.strip_suffix(&end).ok_or(MqPropertyProblem::Value)?;
    let (leaf_tag, rest) = inside
        .strip_prefix('<')
        .ok_or(MqPropertyProblem::Value)?
        .split_once('>')
        .ok_or(MqPropertyProblem::Value)?;
    let (leaf, attrs) = tag(leaf_tag)?;
    let end = format!("</{leaf}>");
    let content = rest.strip_suffix(&end).ok_or(MqPropertyProblem::Value)?;
    if content.contains('<') {
        return Err(MqPropertyProblem::Unsupported);
    }
    let name_length = folder
        .len()
        .checked_add(leaf.len())
        .and_then(|n| n.checked_add(1))
        .ok_or(MqPropertyProblem::Capacity)?;
    if name_length > limits.property_name_bytes {
        return Err(MqPropertyProblem::Capacity);
    }
    let name = MqPropertyName::checked(format!("{folder}.{leaf}"), limits)?;
    custom_name(&name, limits)?;
    let (kind, bytes) = if attrs == [("xsi:nil", "true")] {
        if !content.is_empty() {
            return Err(MqPropertyProblem::Value);
        }
        (crate::MqPropertyType::Null, Vec::new())
    } else {
        let lexical = if attrs.is_empty() {
            "string"
        } else if attrs.len() == 1 && attrs[0].0 == "dt" {
            attrs[0].1
        } else {
            return Err(MqPropertyProblem::Unsupported);
        };
        use crate::MqPropertyType as T;
        let kind = [
            T::ByteString,
            T::Int8,
            T::Int16,
            T::Int32,
            T::Int64,
            T::String,
        ]
        .into_iter()
        .find(|k| generated::lexical(*k) == Some(lexical))
        .ok_or(MqPropertyProblem::Unsupported)?;
        let ceiling = if kind == T::ByteString {
            limits
                .property_value_bytes
                .checked_mul(2)
                .ok_or(MqPropertyProblem::Capacity)?
        } else {
            limits.property_value_bytes
        };
        let raw = unescape(content, ceiling)?;
        let number = || -> Result<i64, MqPropertyProblem> {
            let s = std::str::from_utf8(&raw).map_err(|_| MqPropertyProblem::Value)?;
            let digits = s
                .strip_prefix('+')
                .or_else(|| s.strip_prefix('-'))
                .unwrap_or(s);
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return Err(MqPropertyProblem::Value);
            }
            s.parse().map_err(|_| MqPropertyProblem::Value)
        };
        let bytes = match kind {
            T::String => raw,
            T::ByteString => {
                if raw.len() % 2 != 0 || !raw.iter().all(u8::is_ascii_hexdigit) {
                    return Err(MqPropertyProblem::Value);
                }
                raw.chunks_exact(2)
                    .map(|pair| {
                        let s = std::str::from_utf8(pair).map_err(|_| MqPropertyProblem::Value)?;
                        u8::from_str_radix(s, 16).map_err(|_| MqPropertyProblem::Value)
                    })
                    .collect::<Result<Vec<_>, _>>()?
            }
            T::Int8 => i8::try_from(number()?)
                .map_err(|_| MqPropertyProblem::Value)?
                .to_be_bytes()
                .to_vec(),
            T::Int16 => i16::try_from(number()?)
                .map_err(|_| MqPropertyProblem::Value)?
                .to_be_bytes()
                .to_vec(),
            T::Int32 => i32::try_from(number()?)
                .map_err(|_| MqPropertyProblem::Value)?
                .to_be_bytes()
                .to_vec(),
            T::Int64 => number()?.to_be_bytes().to_vec(),
            _ => return Err(MqPropertyProblem::Unsupported),
        };
        (kind, bytes)
    };
    let value = MqPropertyData {
        kind,
        encoding: native(),
        ccsid: mq_property_profile_ccsid(),
        bytes,
    };
    value.validate(limits)?;
    Ok((name, MqPropertyDescriptor::source_default(), value))
}
