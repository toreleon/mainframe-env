use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataCategory {
    Alphanumeric,
    NumericDisplay,
    PackedDecimal,
    Binary,
    Pointer,
    Group,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CobolLayout {
    pub name: String,
    pub offset: usize,
    pub length: usize,
    pub category: DataCategory,
    pub initial: Vec<u8>,
    pub alias_of: Option<String>,
    pub occurs: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticModel {
    pub program_id: String,
    pub layouts: Vec<CobolLayout>,
    by_name: BTreeMap<String, usize>,
    pub storage_bytes: usize,
}

impl SemanticModel {
    pub(crate) fn analyze(
        source: &str,
        max_storage: usize,
        max_items: usize,
    ) -> Result<Self, SemanticProblem> {
        let upper = source.to_ascii_uppercase();
        let program_id = extract_program_id(&upper).ok_or(SemanticProblem::MissingProgramId)?;
        let data_end = upper.find("PROCEDURE DIVISION").unwrap_or(source.len());
        let data = &source[..data_end];
        let mut layouts: Vec<CobolLayout> = Vec::new();
        let mut by_name: BTreeMap<String, usize> = BTreeMap::new();
        let mut offset = 0usize;
        for raw in data.split('.') {
            let line = raw.trim().trim_end_matches('.');
            let words = words(line);
            if words.len() < 2 || words[0].parse::<u8>().is_err() {
                continue;
            }
            if layouts.len() >= max_items {
                return Err(SemanticProblem::ItemLimitExceeded);
            }
            let name = words[1].to_ascii_uppercase();
            if by_name.contains_key(&name) {
                return Err(SemanticProblem::DuplicateName);
            }
            let alias_of = find_after(&words, "REDEFINES").map(str::to_ascii_uppercase);
            let (category, base_length) = picture(&words);
            let occurs = find_after(&words, "OCCURS")
                .and_then(|value| value.parse().ok())
                .unwrap_or(1usize);
            let length = base_length
                .checked_mul(occurs)
                .ok_or(SemanticProblem::StorageLimitExceeded)?;
            let item_offset = if let Some(target) = &alias_of {
                let index = by_name
                    .get(target)
                    .ok_or(SemanticProblem::UnknownRedefines)?;
                layouts[*index].offset
            } else {
                offset
            };
            let initial = initial_value(line, &words, length, category);
            if alias_of.is_none() {
                offset = offset
                    .checked_add(length)
                    .ok_or(SemanticProblem::StorageLimitExceeded)?;
            }
            if offset > max_storage {
                return Err(SemanticProblem::StorageLimitExceeded);
            }
            by_name.insert(name.clone(), layouts.len());
            layouts.push(CobolLayout {
                name,
                offset: item_offset,
                length,
                category,
                initial,
                alias_of,
                occurs,
            });
        }
        Ok(Self {
            program_id,
            layouts,
            by_name,
            storage_bytes: offset,
        })
    }

    #[must_use]
    pub fn layout(&self, name: &str) -> Option<&CobolLayout> {
        self.by_name
            .get(&name.to_ascii_uppercase())
            .and_then(|index| self.layouts.get(*index))
    }
}

fn extract_program_id(source: &str) -> Option<String> {
    let marker = source.find("PROGRAM-ID")?;
    let rest = &source[marker + "PROGRAM-ID".len()..];
    Some(
        rest.trim_start_matches([' ', '\t', '.'])
            .split(|ch: char| ch == '.' || ch.is_whitespace())
            .next()?
            .trim()
            .to_string(),
    )
}
fn words(line: &str) -> Vec<&str> {
    line.split_whitespace()
        .map(|word| word.trim_matches([',', '.']))
        .collect()
}
fn find_after<'a>(words: &'a [&str], name: &str) -> Option<&'a str> {
    words
        .iter()
        .position(|word| word.eq_ignore_ascii_case(name))
        .and_then(|index| words.get(index + 1).copied())
}
fn picture(words: &[&str]) -> (DataCategory, usize) {
    if let Some(pic) = find_after(words, "PIC").or_else(|| find_after(words, "PICTURE")) {
        let upper = pic.to_ascii_uppercase();
        let digits = expanded_picture_length(&upper).max(1);
        let usage = words
            .iter()
            .map(|word| word.to_ascii_uppercase())
            .collect::<Vec<_>>();
        if usage
            .iter()
            .any(|word| word == "COMP-3" || word == "PACKED-DECIMAL")
        {
            return (DataCategory::PackedDecimal, (digits + 2) / 2);
        }
        if usage
            .iter()
            .any(|word| word == "COMP" || word == "BINARY" || word == "COMP-5")
        {
            return (
                DataCategory::Binary,
                if digits <= 4 {
                    2
                } else if digits <= 9 {
                    4
                } else {
                    8
                },
            );
        }
        if upper.contains('9') {
            return (DataCategory::NumericDisplay, digits);
        }
        return (DataCategory::Alphanumeric, digits);
    }
    if words
        .iter()
        .any(|word| word.eq_ignore_ascii_case("POINTER"))
    {
        (DataCategory::Pointer, 8)
    } else {
        (DataCategory::Group, 0)
    }
}
fn expanded_picture_length(pic: &str) -> usize {
    let mut count = 0usize;
    let bytes = pic.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if matches!(bytes[index], b'X' | b'A' | b'9' | b'Z') {
            let mut repeats = 1;
            if bytes.get(index + 1) == Some(&b'(')
                && let Some(close) = pic[index + 2..].find(')')
            {
                repeats = pic[index + 2..index + 2 + close].parse().unwrap_or(1);
                index += close + 2;
            }
            count += repeats;
        }
        index += 1;
    }
    count
}
fn initial_value(line: &str, words: &[&str], length: usize, category: DataCategory) -> Vec<u8> {
    let fill = if category == DataCategory::NumericDisplay {
        b'0'
    } else {
        b' '
    };
    let mut result = vec![fill; length];
    if let Some(value_index) = line.to_ascii_uppercase().find("VALUE") {
        let tail = line[value_index + "VALUE".len()..].trim();
        let clean = if let Some(quote) = tail.chars().next().filter(|ch| matches!(ch, '\'' | '"')) {
            tail[quote.len_utf8()..]
                .split(quote)
                .next()
                .unwrap_or_default()
        } else {
            find_after(words, "VALUE").unwrap_or_default()
        };
        let bytes = clean.as_bytes();
        let copy = bytes.len().min(result.len());
        if category == DataCategory::NumericDisplay {
            let start = result.len() - copy;
            result[start..].copy_from_slice(&bytes[bytes.len() - copy..]);
        } else {
            result[..copy].copy_from_slice(&bytes[..copy]);
        }
    }
    result
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SemanticProblem {
    MissingProgramId,
    DuplicateName,
    UnknownRedefines,
    ItemLimitExceeded,
    StorageLimitExceeded,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layouts_redefines_and_occurs_are_explicit() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. T.\nDATA DIVISION. WORKING-STORAGE SECTION.\n01 A PIC X(4) VALUE 'AB'.\n01 B REDEFINES A PIC 9(4).\n01 C PIC X(2) OCCURS 3 TIMES.\nPROCEDURE DIVISION. STOP RUN.";
        let model = SemanticModel::analyze(source, 1024, 16).unwrap();
        assert_eq!(
            model.layout("A").unwrap().offset,
            model.layout("B").unwrap().offset
        );
        assert_eq!(model.layout("C").unwrap().length, 6);
        assert_eq!(model.storage_bytes, 10);
    }
}
