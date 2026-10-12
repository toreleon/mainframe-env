//! COBOL symbolic maps, structurally checked against shipped CardDemo copybooks.
//!
//! CICS TS 6.x DFHMSD/DFHMDI pins establish mapset options and inheritance.
//! DFHMDF and ADS topic bodies were unavailable offline; field layouts are
//! corroborated by the external assembled copybooks, never by a licensed run.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write};

const EXTENSIONS: [(&str, char); 4] = [
    ("COLOR", 'C'),
    ("PS", 'P'),
    ("HILIGHT", 'H'),
    ("VALIDN", 'V'),
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BmsSymbolicProblem {
    InvalidSource(String),
    UnverifiedFeature(String),
    LimitExceeded,
}

impl fmt::Display for BmsSymbolicProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSource(feature) => write!(f, "invalid BMS symbolic map: {feature}"),
            Self::UnverifiedFeature(feature) => {
                write!(f, "unverified BMS symbolic-map feature: {feature}")
            }
            Self::LimitExceeded => f.write_str("BMS symbolic-map limit exceeded"),
        }
    }
}
impl std::error::Error for BmsSymbolicProblem {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BmsMapset {
    name: String,
    mode: String,
    maps: Vec<Map>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Map {
    name: String,
    prefix: bool,
    extensions: BTreeSet<String>,
    fields: Vec<Field>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Field {
    name: String,
    input_picture: String,
    output_picture: String,
}

struct Statement {
    name: Option<String>,
    operation: String,
    options: BTreeMap<String, String>,
}

type Result<T> = std::result::Result<T, BmsSymbolicProblem>;

impl BmsMapset {
    /// Parses the verified COBOL, field-data, STORAGE=AUTO subset of BMS.
    /// Unsupported layout options fail closed, including unverified pictures.
    pub fn parse(source: &str) -> Result<Self> {
        let statements = statements(source)?;
        let mut mapset: Option<Self> = None;
        let mut defaults = BTreeMap::new();
        let mut names = BTreeSet::new();
        let mut final_seen = false;
        for statement in statements {
            match statement.operation.as_str() {
                "TITLE" | "END" if statement.name.is_none() => {}
                "DFHMSD" if statement.options.get("TYPE").is_some_and(|v| v == "FINAL") => {
                    if mapset.is_none() || final_seen || statement.options.len() != 1 {
                        return invalid("DFHMSD TYPE=FINAL");
                    }
                    final_seen = true;
                }
                "DFHMSD" if mapset.is_none() && !final_seen => {
                    validate_options(&statement, &["TYPE", "MODE", "LANG", "STORAGE"])?;
                    let name = required_name(&statement)?;
                    if statement
                        .options
                        .get("TYPE")
                        .is_some_and(|v| !matches!(v.as_str(), "MAP" | "DSECT" | "&&SYSPARM"))
                    {
                        return unverified("DFHMSD TYPE");
                    }
                    let mode = statement.options.get("MODE").map_or("OUT", String::as_str);
                    if !matches!(mode, "IN" | "OUT" | "INOUT") {
                        return unverified(format!("MODE={mode}"));
                    }
                    if let Some(lang) = statement.options.get("LANG")
                        && !matches!(lang.as_str(), "COBOL" | "COBOL2")
                    {
                        return unverified(format!("LANG={lang}"));
                    }
                    if statement
                        .options
                        .get("STORAGE")
                        .is_some_and(|v| v != "AUTO")
                    {
                        return unverified("STORAGE (only AUTO is verified)");
                    }
                    mapset = Some(Self {
                        name,
                        mode: mode.into(),
                        maps: Vec::new(),
                    });
                    defaults = statement.options;
                    // Validate even defaults overridden by every map.
                    map_options(&defaults)?;
                }
                "DFHMDI" if !final_seen => {
                    validate_options(&statement, &["SIZE", "LINE", "COLUMN"])?;
                    let name = required_name(&statement)?;
                    if !names.insert(name.clone()) {
                        return invalid(format!("duplicate name {name}"));
                    }
                    let mapset = mapset
                        .as_mut()
                        .ok_or_else(|| problem("DFHMDI before DFHMSD"))?;
                    if !mapset.maps.is_empty()
                        && !defaults.get("STORAGE").is_some_and(|v| v == "AUTO")
                    {
                        return unverified("STORAGE overlay (specify STORAGE=AUTO)");
                    }
                    let mut options = defaults.clone();
                    // EXTATT on a map supplies fresh DSATTS/MAPATTS defaults.
                    if statement.options.contains_key("EXTATT") {
                        options.remove("DSATTS");
                        options.remove("MAPATTS");
                    }
                    options.extend(statement.options);
                    let (prefix, extensions) = map_options(&options)?;
                    mapset.maps.push(Map {
                        name,
                        prefix,
                        extensions,
                        fields: Vec::new(),
                    });
                }
                "DFHMDF" if !final_seen => {
                    validate_options(
                        &statement,
                        &[
                            "POS", "LENGTH", "ATTRB", "JUSTIFY", "INITIAL", "XINIT", "PICIN",
                            "PICOUT",
                        ],
                    )?;
                    let map = mapset
                        .as_mut()
                        .and_then(|set| set.maps.last_mut())
                        .ok_or_else(|| problem("DFHMDF before DFHMDI"))?;
                    let length = statement
                        .options
                        .get("LENGTH")
                        .and_then(|v| v.parse::<usize>().ok())
                        .filter(|v| *v <= 32767)
                        .ok_or_else(|| problem("DFHMDF LENGTH"))?;
                    let input_picture = picture(&statement.options, "PICIN", length)?;
                    let output_picture = picture(&statement.options, "PICOUT", length)?;
                    // Literal/attribute-only fields do not occupy the symbolic ADS.
                    if statement.name.is_some() {
                        let name = required_name(&statement)?;
                        if length == 0 || !names.insert(name.clone()) {
                            return invalid(format!("zero-length or duplicate named field {name}"));
                        }
                        map.fields.push(Field {
                            name,
                            input_picture,
                            output_picture,
                        });
                    }
                }
                operation => return unverified(format!("macro {operation}")),
            }
        }
        let mapset = mapset.ok_or_else(|| problem("missing DFHMSD"))?;
        if !final_seen || mapset.maps.is_empty() {
            return invalid("missing DFHMDI or DFHMSD TYPE=FINAL");
        }
        Ok(mapset)
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Produces fixed-format COBOL text; input/output maps overlay for INOUT.
    pub fn symbolic_copybook(&self) -> Result<String> {
        let mut text = String::new();
        for map in &self.maps {
            if self.mode != "OUT" {
                writeln!(text, "       01 {}I.", map.name).expect("string write");
                prefix(&mut text, map.prefix);
                for field in &map.fields {
                    let name = &field.name;
                    writeln!(text, "           02 {name}L COMP PIC S9(4).").expect("string write");
                    writeln!(text, "           02 {name}F PIC X.").expect("string write");
                    writeln!(text, "           02 FILLER REDEFINES {name}F.")
                        .expect("string write");
                    writeln!(text, "             03 {name}A PIC X.").expect("string write");
                    if !map.extensions.is_empty() {
                        writeln!(
                            text,
                            "           02 FILLER PIC X({}).",
                            map.extensions.len()
                        )
                        .expect("string write");
                    }
                    writeln!(text, "           02 {name}I PIC {}.", field.input_picture)
                        .expect("string write");
                }
            }
            if self.mode != "IN" {
                let overlay = if self.mode == "INOUT" {
                    format!(" REDEFINES {}I", map.name)
                } else {
                    String::new()
                };
                writeln!(text, "       01 {}O{overlay}.", map.name).expect("string write");
                prefix(&mut text, map.prefix);
                for field in &map.fields {
                    if self.mode == "OUT" {
                        writeln!(text, "           02 FILLER PIC X(2).").expect("string write");
                        writeln!(text, "           02 {}A PIC X.", field.name)
                            .expect("string write");
                    } else {
                        writeln!(text, "           02 FILLER PIC X(3).").expect("string write");
                    }
                    for (attribute, suffix) in EXTENSIONS {
                        if map.extensions.contains(attribute) {
                            writeln!(text, "           02 {}{suffix} PIC X.", field.name)
                                .expect("string write");
                        }
                    }
                    writeln!(
                        text,
                        "           02 {}O PIC {}.",
                        field.name, field.output_picture
                    )
                    .expect("string write");
                }
            }
            if text.len() > 16 * 1024 * 1024 {
                return Err(BmsSymbolicProblem::LimitExceeded);
            }
        }
        Ok(text)
    }
}

fn prefix(text: &mut String, enabled: bool) {
    if enabled {
        text.push_str("           02 FILLER PIC X(12).\n");
    }
}

fn required_name(statement: &Statement) -> Result<String> {
    statement
        .name
        .as_ref()
        .filter(|name| {
            name.len() <= 7
                && name.as_bytes()[0].is_ascii_alphabetic()
                && name.bytes().all(|b| b.is_ascii_alphanumeric())
        })
        .cloned()
        .ok_or_else(|| problem(format!("{} name", statement.operation)))
}

fn validate_options(statement: &Statement, extra: &[&str]) -> Result<()> {
    for (key, value) in &statement.options {
        if extra.contains(&key.as_str()) {
            continue;
        }
        match key.as_str() {
            "TIOAPFX" | "EXTATT" | "DSATTS" | "MAPATTS" if statement.operation != "DFHMDF" => {}
            "DATA" if value == "FIELD" && statement.operation != "DFHMDF" => {}
            "DSECT" if value == "ADS" && statement.operation == "DFHMSD" => {}
            // These physical-map attributes do not change the symbolic layout.
            "COLOR" | "HILIGHT" | "PS" | "VALIDN" | "CTRL" | "CURSLOC" => {}
            _ => return unverified(format!("{} {key}={value}", statement.operation)),
        }
    }
    Ok(())
}

fn map_options(options: &BTreeMap<String, String>) -> Result<(bool, BTreeSet<String>)> {
    let auto = options.get("STORAGE").is_some_and(|v| v == "AUTO");
    let prefix = match options.get("TIOAPFX").map(String::as_str) {
        Some("YES") => true,
        Some("NO") => false,
        None => auto,
        Some(value) => return unverified(format!("TIOAPFX={value}")),
    };
    let extatt = options.get("EXTATT").map_or("NO", String::as_str);
    let extended = matches!(extatt, "YES" | "MAPONLY");
    if !matches!(extatt, "NO" | "YES" | "MAPONLY") {
        return unverified(format!("EXTATT={extatt}"));
    }
    let defaults = "(COLOR,HILIGHT,PS,VALIDN)";
    let dsatts = attributes(options.get("DSATTS").map_or(
        if extatt == "YES" { defaults } else { "()" },
        String::as_str,
    ))?;
    let mut mapatts = attributes(
        options
            .get("MAPATTS")
            .map_or(if extended { defaults } else { "()" }, String::as_str),
    )?;
    for (name, _) in EXTENSIONS {
        if options.contains_key(name) {
            mapatts.insert(name.into());
        }
    }
    if !dsatts.is_subset(&mapatts) {
        return invalid("DSATTS must be included in MAPATTS");
    }
    Ok((prefix, dsatts))
}

fn attributes(value: &str) -> Result<BTreeSet<String>> {
    let mut attributes = BTreeSet::new();
    for attribute in value
        .trim_start_matches('(')
        .trim_end_matches(')')
        .split(',')
        .filter(|v| !v.is_empty())
    {
        if !EXTENSIONS.iter().any(|(name, _)| *name == attribute) {
            return unverified(format!("extended attribute {attribute}"));
        }
        if !attributes.insert(attribute.into()) {
            return invalid(format!("duplicate attribute {attribute}"));
        }
    }
    Ok(attributes)
}

fn picture(options: &BTreeMap<String, String>, key: &str, length: usize) -> Result<String> {
    let Some(value) = options.get(key) else {
        return Ok(format!("X({length})"));
    };
    let value = value
        .strip_prefix('\'')
        .and_then(|v| v.strip_suffix('\''))
        .ok_or_else(|| problem(format!("{key} quoted picture")))?;
    // Only these editing patterns are corroborated by COACTVW's shipped ADS.
    if !matches!(
        (key, value, length),
        ("PICIN", "99999999999", 11) | ("PICOUT", "+ZZZ,ZZZ,ZZZ.99", 15)
    ) {
        return unverified(format!("{key}={value} LENGTH={length}"));
    }
    Ok(value.into())
}

pub(crate) fn source_mapset_name(source: &str) -> Result<Option<String>> {
    Ok(statements(source)?
        .into_iter()
        .find(|s| s.operation == "DFHMSD")
        .and_then(|s| s.name))
}

fn statements(source: &str) -> Result<Vec<Statement>> {
    if source.len() > 4 * 1024 * 1024 {
        return Err(BmsSymbolicProblem::LimitExceeded);
    }
    if !source.is_ascii() {
        return invalid("non-ASCII BMS source");
    }
    let mut result = Vec::new();
    let mut current = String::new();
    for line in source.lines() {
        if line.trim().is_empty() || line.starts_with('*') {
            continue;
        }
        let continuation = line
            .as_bytes()
            .get(71)
            .is_some_and(|b| matches!(b, b'-' | b'X' | b'*'));
        let content = if continuation { &line[..71] } else { line };
        current.push_str(content.trim());
        if continuation {
            continue;
        }
        let text = std::mem::take(&mut current);
        let text = text.trim();
        let (first, rest) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
        let first = first.to_ascii_uppercase();
        let (name, operation, operands) = if matches!(
            first.as_str(),
            "DFHMSD" | "DFHMDI" | "DFHMDF" | "TITLE" | "END"
        ) {
            (None, first, rest)
        } else {
            let (operation, operands) = rest
                .trim_start()
                .split_once(char::is_whitespace)
                .unwrap_or((rest, ""));
            (Some(first), operation.to_ascii_uppercase(), operands)
        };
        let options = if operation == "TITLE" {
            BTreeMap::new()
        } else {
            operands_map(operands)?
        };
        result.push(Statement {
            name,
            operation,
            options,
        });
    }
    if !current.is_empty() {
        return invalid("unterminated BMS continuation");
    }
    Ok(result)
}

fn operands_map(text: &str) -> Result<BTreeMap<String, String>> {
    // Assembler remarks after a completed quoted operand are outside the
    // operand area (including the retained GenApp Fund Name line).
    let mut in_quote = false;
    let mut end = text.len();
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\'' {
            in_quote = !in_quote;
        }
        if !in_quote
            && byte.is_ascii_whitespace()
            && text[..index].ends_with('\'')
            && !text[index..].trim_start().starts_with(',')
        {
            end = index;
            break;
        }
    }
    let text = &text[..end];
    let mut options = BTreeMap::new();
    let mut start = 0;
    let mut depth = 0usize;
    let mut quoted = false;
    for (index, byte) in text.bytes().chain(std::iter::once(b',')).enumerate() {
        match byte {
            b'\'' => quoted = !quoted,
            b'(' if !quoted => depth += 1,
            b')' if !quoted => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| problem("operand parentheses"))?
            }
            b',' if !quoted && depth == 0 => {
                let operand = text[start..index].trim();
                if !operand.is_empty() {
                    let (key, value) = operand
                        .split_once('=')
                        .ok_or_else(|| problem(format!("operand {operand}")))?;
                    let key = key.trim().to_ascii_uppercase();
                    let value = if value.trim().starts_with('\'') {
                        value.trim().to_ascii_uppercase()
                    } else {
                        value
                            .chars()
                            .filter(|c| !c.is_ascii_whitespace())
                            .collect::<String>()
                            .to_ascii_uppercase()
                    };
                    if value.is_empty() || options.insert(key.clone(), value).is_some() {
                        return invalid(format!("operand {key}"));
                    }
                }
                start = index + 1;
            }
            _ => {}
        }
    }
    if quoted || depth != 0 {
        return invalid("unterminated BMS operand");
    }
    Ok(options)
}

fn problem(value: impl Into<String>) -> BmsSymbolicProblem {
    BmsSymbolicProblem::InvalidSource(value.into())
}
fn invalid<T>(value: impl Into<String>) -> Result<T> {
    Err(problem(value))
}
fn unverified<T>(value: impl Into<String>) -> Result<T> {
    Err(BmsSymbolicProblem::UnverifiedFeature(value.into()))
}

#[cfg(test)]
mod tests;
