use super::{ProgramInput, dd_records, project_sort_record, split_control_fields};
use mainframe_env_encoding::{CodePage, compare_ebcdic, decode_zoned};
use mainframe_env_host_api::HostProblem;
use std::{cmp::Ordering, collections::BTreeMap};

#[derive(Clone, Copy, Eq, PartialEq)]
enum Format {
    Ch,
    Zd,
}
#[derive(Clone, Copy)]
struct Field {
    start: usize,
    len: usize,
    format: Format,
}
enum Symbol {
    Field(Field),
    Constant(String),
}
struct Key {
    field: Field,
    descending: bool,
}
enum Operand {
    Field(Field),
    Constant(Vec<u8>),
}
#[derive(Clone, Copy)]
enum Op {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
}
struct Comparison {
    left: Field,
    op: Op,
    right: Operand,
}
struct Condition {
    groups: Vec<Vec<Comparison>>,
    omit: bool,
}
#[derive(Eq, PartialEq)]
enum Value {
    Ch(Vec<u8>),
    Zd(i128),
}
fn unsupported<T>() -> Result<T, HostProblem> {
    Err(HostProblem::Unsupported)
}

fn text(input: &ProgramInput, name: &str) -> Result<String, HostProblem> {
    String::from_utf8(
        dd_records(input, name)?
            .into_iter()
            .flat_map(|mut r| {
                r.push(b'\n');
                r
            })
            .collect(),
    )
    .map_err(|_| HostProblem::Unsupported)
}
fn literal(token: &str) -> Option<&str> {
    token.strip_prefix("C'").and_then(|s| s.strip_suffix('\''))
}
fn field(start: &str, len: &str, format: &str) -> Result<Field, HostProblem> {
    let start = start
        .parse::<usize>()
        .map_err(|_| HostProblem::Unsupported)?;
    let len = len.parse::<usize>().map_err(|_| HostProblem::Unsupported)?;
    let format = match format.to_ascii_uppercase().as_str() {
        "CH" => Format::Ch,
        "ZD" => Format::Zd,
        _ => return unsupported(),
    };
    if start == 0 || len == 0 || start.checked_add(len).is_none() {
        return unsupported();
    }
    Ok(Field { start, len, format })
}
fn symbols(input: &ProgramInput) -> Result<BTreeMap<String, Symbol>, HostProblem> {
    let source = match text(input, "SYMNAMES") {
        Ok(s) => s,
        Err(HostProblem::NotFound) => return Ok(BTreeMap::new()),
        Err(e) => return Err(e),
    };
    let mut map = BTreeMap::new();
    for line in source.lines().filter(|l| !l.trim().is_empty()) {
        let mut quote = false;
        let end = line
            .char_indices()
            .find_map(|(i, c)| {
                if c == '\'' {
                    quote = !quote;
                }
                (!quote && c.is_whitespace()).then_some(i)
            })
            .unwrap_or(line.len());
        if quote {
            return unsupported();
        }
        let parts = split_control_fields(&line[..end]).map_err(|_| HostProblem::Unsupported)?;
        let name = parts
            .first()
            .ok_or(HostProblem::Unsupported)?
            .to_ascii_uppercase();
        if name.is_empty() || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_') {
            return unsupported();
        }
        let symbol = match parts.as_slice() {
            [_, value] => {
                Symbol::Constant(literal(value).ok_or(HostProblem::Unsupported)?.to_string())
            }
            [_, start, len, fmt] => Symbol::Field(field(start, len, fmt)?),
            _ => return unsupported(),
        };
        if map.insert(name, symbol).is_some() {
            return unsupported();
        }
    }
    Ok(map)
}
fn read_field(
    tokens: &[String],
    i: &mut usize,
    symbols: &BTreeMap<String, Symbol>,
) -> Result<Field, HostProblem> {
    let token = tokens.get(*i).ok_or(HostProblem::Unsupported)?;
    if token.bytes().all(|c| c.is_ascii_digit()) {
        let result = field(
            token,
            tokens.get(*i + 1).ok_or(HostProblem::Unsupported)?,
            tokens.get(*i + 2).ok_or(HostProblem::Unsupported)?,
        )?;
        *i += 3;
        Ok(result)
    } else {
        *i += 1;
        match symbols.get(&token.to_ascii_uppercase()) {
            Some(Symbol::Field(f)) => Ok(*f),
            _ => unsupported(),
        }
    }
}
fn keys(s: &str, symbols: &BTreeMap<String, Symbol>) -> Result<Vec<Key>, HostProblem> {
    let tokens = split_control_fields(s).map_err(|_| HostProblem::Unsupported)?;
    let (mut i, mut result) = (0, Vec::new());
    while i < tokens.len() {
        let field = read_field(&tokens, &mut i, symbols)?;
        let descending = match tokens.get(i).map(|s| s.to_ascii_uppercase()).as_deref() {
            Some("A") => false,
            Some("D") => true,
            _ => return unsupported(),
        };
        i += 1;
        result.push(Key { field, descending });
    }
    if result.is_empty() {
        return unsupported();
    }
    Ok(result)
}
fn encode(s: &str, ccsid: Option<u16>) -> Result<Vec<u8>, HostProblem> {
    match ccsid {
        Some(37) => CodePage::Cp037
            .encode(s, s.chars().count())
            .map_err(|_| HostProblem::Unsupported),
        None | Some(1208) => Ok(s.as_bytes().to_vec()),
        Some(_) => unsupported(),
    }
}
fn right(
    tokens: &[String],
    i: &mut usize,
    symbols: &BTreeMap<String, Symbol>,
    ccsid: Option<u16>,
) -> Result<Operand, HostProblem> {
    let token = tokens.get(*i).ok_or(HostProblem::Unsupported)?;
    if let Some(s) = literal(token) {
        *i += 1;
        return Ok(Operand::Constant(encode(s, ccsid)?));
    }
    if let Some(Symbol::Constant(s)) = symbols.get(&token.to_ascii_uppercase()) {
        *i += 1;
        return Ok(Operand::Constant(encode(s, ccsid)?));
    }
    read_field(tokens, i, symbols).map(Operand::Field)
}
fn condition(
    s: &str,
    symbols: &BTreeMap<String, Symbol>,
    ccsid: Option<u16>,
    omit: bool,
) -> Result<Condition, HostProblem> {
    let mut quote = false;
    for c in s.chars() {
        if c == '\'' {
            quote = !quote;
        } else if !quote && (c == '(' || c == ')') {
            return unsupported();
        }
    }
    let tokens = split_control_fields(s).map_err(|_| HostProblem::Unsupported)?;
    let (mut i, mut groups) = (0, vec![Vec::new()]);
    while i < tokens.len() {
        let left = read_field(&tokens, &mut i, symbols)?;
        let op = match tokens.get(i).map(|s| s.to_ascii_uppercase()).as_deref() {
            Some("EQ") => Op::Eq,
            Some("NE") => Op::Ne,
            Some("GT") => Op::Gt,
            Some("GE") => Op::Ge,
            Some("LT") => Op::Lt,
            Some("LE") => Op::Le,
            _ => return unsupported(),
        };
        i += 1;
        let right = right(&tokens, &mut i, symbols, ccsid)?;
        match &right {
            Operand::Field(field) if field.format != left.format => return unsupported(),
            Operand::Constant(_) if left.format != Format::Ch => return unsupported(),
            _ => {}
        }
        groups
            .last_mut()
            .unwrap()
            .push(Comparison { left, op, right });
        if i < tokens.len() {
            match tokens[i].to_ascii_uppercase().as_str() {
                "AND" => {}
                "OR" => groups.push(Vec::new()),
                _ => return unsupported(),
            }
            i += 1;
            if i == tokens.len() {
                return unsupported();
            }
        }
    }
    if groups.iter().any(Vec::is_empty) {
        return unsupported();
    }
    Ok(Condition { groups, omit })
}
fn statements(s: &str) -> Result<Vec<String>, HostProblem> {
    let (mut out, mut pending) = (Vec::new(), String::new());
    for line in s.lines() {
        let line = line
            .get(..line.len().min(71))
            .ok_or(HostProblem::Unsupported)?;
        if line.trim().is_empty() || line.starts_with('*') {
            continue;
        }
        if !line.starts_with(' ') {
            return unsupported();
        }
        pending.push_str(line.trim());
        if !pending.ends_with(',') {
            out.push(std::mem::take(&mut pending));
        }
    }
    if !pending.is_empty() {
        return unsupported();
    }
    Ok(out)
}
fn body<'a>(s: &'a str, prefix: &str) -> Result<Option<&'a str>, HostProblem> {
    if !s.to_ascii_uppercase().starts_with(prefix) {
        return Ok(None);
    }
    let rest = &s[prefix.len()..];
    if !rest.ends_with(')') {
        return unsupported();
    }
    Ok(Some(&rest[..rest.len() - 1]))
}
fn ascii_zoned(bytes: &[u8]) -> Result<Vec<u8>, HostProblem> {
    let mut zoned = Vec::with_capacity(bytes.len());
    for (index, byte) in bytes.iter().enumerate() {
        let last = index + 1 == bytes.len();
        let encoded = match *byte {
            b'0'..=b'9' => 0xf0 | (byte - b'0'),
            b'{' if last => 0xc0,
            b'A'..=b'I' if last => 0xc1 + (byte - b'A'),
            b'}' if last => 0xd0,
            b'J'..=b'R' if last => 0xd1 + (byte - b'J'),
            _ => return unsupported(),
        };
        zoned.push(encoded);
    }
    Ok(zoned)
}
fn value(record: &[u8], field: Field, ccsid: Option<u16>) -> Result<Value, HostProblem> {
    let bytes = record
        .get(field.start - 1..field.start - 1 + field.len)
        .ok_or_else(|| HostProblem::Condition {
            name: "LENGERR".into(),
            response: 22,
            response2: 0,
        })?;
    match field.format {
        Format::Ch => Ok(Value::Ch(bytes.to_vec())),
        Format::Zd => {
            let converted;
            let zoned = match ccsid {
                Some(37) => bytes,
                None | Some(1208) => {
                    converted = ascii_zoned(bytes)?;
                    &converted
                }
                Some(_) => return unsupported(),
            };
            Ok(Value::Zd(
                decode_zoned(zoned, 0)
                    .map_err(|_| HostProblem::Unsupported)?
                    .coefficient(),
            ))
        }
    }
}
fn compare(a: &Value, b: &Value, ccsid: Option<u16>) -> Result<Ordering, HostProblem> {
    match (a, b) {
        (Value::Ch(a), Value::Ch(b)) => Ok(if ccsid == Some(37) {
            compare_ebcdic(a, b)
        } else {
            a.cmp(b)
        }),
        (Value::Zd(a), Value::Zd(b)) => Ok(a.cmp(b)),
        _ => unsupported(),
    }
}
fn matches(record: &[u8], cond: &Condition, ccsid: Option<u16>) -> Result<bool, HostProblem> {
    let mut any = false;
    for group in &cond.groups {
        let mut all = true;
        for test in group {
            let left = value(record, test.left, ccsid)?;
            let right = match &test.right {
                Operand::Field(f) => value(record, *f, ccsid)?,
                Operand::Constant(c) => {
                    if !matches!(test.left.format, Format::Ch) {
                        return unsupported();
                    }
                    Value::Ch(c.clone())
                }
            };
            let ord = compare(&left, &right, ccsid)?;
            all &= match test.op {
                Op::Eq => ord == Ordering::Equal,
                Op::Ne => ord != Ordering::Equal,
                Op::Gt => ord == Ordering::Greater,
                Op::Ge => ord != Ordering::Less,
                Op::Lt => ord == Ordering::Less,
                Op::Le => ord != Ordering::Greater,
            };
        }
        any |= all;
    }
    Ok(if cond.omit { !any } else { any })
}
pub(super) fn execute(
    input: &ProgramInput,
    records: Vec<Vec<u8>>,
) -> Result<Vec<Vec<u8>>, HostProblem> {
    let control = match text(input, "SYSIN") {
        Ok(s) => s,
        Err(HostProblem::NotFound) => {
            let mut r = records;
            r.sort();
            return Ok(r);
        }
        Err(e) => return Err(e),
    };
    let symbols = symbols(input)?;
    let ccsid = input
        .dds
        .iter()
        .find(|dd| dd.name.eq_ignore_ascii_case("SORTIN"))
        .and_then(|dd| dd.ccsid);
    super::dataset_space(ccsid)?;
    let (mut sort_keys, mut filter, mut outrec) = (None, None, None);
    for statement in statements(&control)? {
        if let Some(s) = body(&statement, "SORT FIELDS=(")? {
            if sort_keys.replace(keys(s, &symbols)?).is_some() {
                return unsupported();
            }
        } else if let Some(s) = body(&statement, "INCLUDE COND=(")? {
            if filter
                .replace(condition(s, &symbols, ccsid, false)?)
                .is_some()
            {
                return unsupported();
            }
        } else if let Some(s) = body(&statement, "OMIT COND=(")? {
            if filter
                .replace(condition(s, &symbols, ccsid, true)?)
                .is_some()
            {
                return unsupported();
            }
        } else if let Some(s) = body(&statement, "OUTREC FIELDS=(")? {
            if outrec
                .replace(split_control_fields(s).map_err(|_| HostProblem::Unsupported)?)
                .is_some()
            {
                return unsupported();
            }
        } else {
            return unsupported();
        }
    }
    let mut records = records
        .into_iter()
        .filter_map(|r| match &filter {
            Some(f) => match matches(&r, f, ccsid) {
                Ok(true) => Some(Ok(r)),
                Ok(false) => None,
                Err(e) => Some(Err(e)),
            },
            None => Some(Ok(r)),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(keys) = sort_keys {
        let mut decorated = records
            .into_iter()
            .map(|r| {
                let values = keys
                    .iter()
                    .map(|k| value(&r, k.field, ccsid))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok((r, values))
            })
            .collect::<Result<Vec<_>, HostProblem>>()?;
        // DFSORT leaves default equal-key order unspecified; this model preserves input order.
        decorated.sort_by(|a, b| {
            keys.iter()
                .enumerate()
                .find_map(|(i, k)| {
                    let order = compare(&a.1[i], &b.1[i], ccsid).expect("matching key formats");
                    (order != Ordering::Equal).then_some(if k.descending {
                        order.reverse()
                    } else {
                        order
                    })
                })
                .unwrap_or(Ordering::Equal)
        });
        records = decorated.into_iter().map(|(r, _)| r).collect();
    } else {
        records.sort();
    }
    if let Some(tokens) = outrec {
        let fields = symbols
            .iter()
            .filter_map(|(name, s)| match s {
                Symbol::Field(f) => Some((name.clone(), (f.start, f.len))),
                _ => None,
            })
            .collect();
        records = records
            .into_iter()
            .map(|r| project_sort_record(&r, &tokens, &fields, ccsid))
            .collect::<Result<Vec<_>, _>>()?;
    }
    Ok(records)
}
