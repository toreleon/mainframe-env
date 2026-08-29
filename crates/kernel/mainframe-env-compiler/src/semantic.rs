use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataCategory {
    Alphanumeric,
    NumericDisplay,
    NumericEdited,
    PackedDecimal,
    Binary,
    Pointer,
    Group,
    Condition,
    Rename,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum StorageSection {
    File,
    Working,
    Local,
    Linkage,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CobolLayout {
    pub name: String,
    pub qualified_name: String,
    pub level: u8,
    pub section: StorageSection,
    pub parent: Option<String>,
    pub offset: usize,
    pub length: usize,
    pub element_length: usize,
    pub category: DataCategory,
    pub initial: Vec<u8>,
    pub alias_of: Option<String>,
    pub occurs: usize,
    pub occurs_min: usize,
    pub depending_on: Option<String>,
    pub indexes: Vec<String>,
    pub condition_values: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataReference {
    pub qualified_name: String,
    pub offset: usize,
    pub length: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolutionProblem {
    Missing(String),
    Ambiguous(String),
    InvalidSubscript,
    InvalidReferenceModification,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticModel {
    pub program_id: String,
    pub layouts: Vec<CobolLayout>,
    by_qualified: BTreeMap<String, usize>,
    by_simple: BTreeMap<String, Vec<usize>>,
    pub storage_bytes: usize,
}

impl SemanticModel {
    pub(crate) fn analyze(
        source: &str,
        max_storage: usize,
        max_items: usize,
    ) -> Result<Self, SemanticProblem> {
        let cleaned = strip_comments(source);
        let upper = cleaned.to_ascii_uppercase();
        let program_id = extract_program_id(&upper).ok_or(SemanticProblem::MissingProgramId)?;
        let data_start = upper.find("DATA DIVISION").unwrap_or(0);
        let data_end = upper[data_start..]
            .find("PROCEDURE DIVISION")
            .map_or(cleaned.len(), |relative| data_start + relative);
        let declarations = declaration_sentences(&cleaned[data_start..data_end]);
        let specs = parse_specs(&declarations, max_items)?;
        let mut layouts = vec![None; specs.len()];
        let mut cursor = 0usize;
        let roots = specs
            .iter()
            .enumerate()
            .filter(|(_, spec)| spec.parent.is_none() && !matches!(spec.level, 66 | 88))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        layout_siblings(&roots, &specs, &mut layouts, &mut cursor, max_storage)?;
        layout_specials(&specs, &mut layouts)?;
        let layouts = layouts
            .into_iter()
            .enumerate()
            .map(|(index, layout)| {
                layout.ok_or_else(|| {
                    SemanticProblem::IncompleteLayout(specs[index].qualified.clone())
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut by_qualified = BTreeMap::new();
        let mut by_simple = BTreeMap::<String, Vec<usize>>::new();
        for (index, layout) in layouts.iter().enumerate() {
            if by_qualified
                .insert(layout.qualified_name.clone(), index)
                .is_some()
            {
                return Err(SemanticProblem::DuplicateQualifiedName(
                    layout.qualified_name.clone(),
                ));
            }
            if layout.name != "FILLER" {
                by_simple
                    .entry(layout.name.clone())
                    .or_default()
                    .push(index);
            }
        }
        Ok(Self {
            program_id,
            layouts,
            by_qualified,
            by_simple,
            storage_bytes: cursor,
        })
    }

    #[must_use]
    pub fn layout(&self, name: &str) -> Option<&CobolLayout> {
        self.resolve(name).ok()
    }

    pub fn resolve(&self, reference: &str) -> Result<&CobolLayout, ResolutionProblem> {
        let normalized = reference.trim().to_ascii_uppercase();
        if let Some(index) = self.by_qualified.get(&normalized) {
            return self
                .layouts
                .get(*index)
                .ok_or(ResolutionProblem::Missing(normalized));
        }
        let parts = normalized
            .split_whitespace()
            .filter(|word| !matches!(*word, "OF" | "IN"))
            .collect::<Vec<_>>();
        let Some(simple) = parts.first() else {
            return Err(ResolutionProblem::Missing(normalized));
        };
        let candidates = self
            .by_simple
            .get(*simple)
            .ok_or_else(|| ResolutionProblem::Missing(normalized.clone()))?;
        let matching = candidates
            .iter()
            .filter_map(|index| self.layouts.get(*index))
            .filter(|layout| {
                let ancestors = layout
                    .qualified_name
                    .split('.')
                    .rev()
                    .skip(1)
                    .collect::<Vec<_>>();
                parts[1..].len() <= ancestors.len()
                    && parts[1..]
                        .iter()
                        .zip(ancestors)
                        .all(|(qualifier, ancestor)| *qualifier == ancestor)
            })
            .collect::<Vec<_>>();
        match matching.as_slice() {
            [layout] => Ok(*layout),
            [] => Err(ResolutionProblem::Missing(normalized)),
            _ => Err(ResolutionProblem::Ambiguous(normalized)),
        }
    }

    pub fn resolve_reference(
        &self,
        reference: &str,
        subscript: Option<usize>,
        modification: Option<(usize, usize)>,
    ) -> Result<DataReference, ResolutionProblem> {
        let layout = self.resolve(reference)?;
        let mut offset = layout.offset;
        let mut length = layout.length;
        if let Some(subscript) = subscript {
            if layout.occurs <= 1 || subscript == 0 || subscript > layout.occurs {
                return Err(ResolutionProblem::InvalidSubscript);
            }
            offset = offset
                .checked_add((subscript - 1) * layout.element_length)
                .ok_or(ResolutionProblem::InvalidSubscript)?;
            length = layout.element_length;
        }
        if let Some((start, requested)) = modification {
            if start == 0 || requested == 0 || start - 1 + requested > length {
                return Err(ResolutionProblem::InvalidReferenceModification);
            }
            offset += start - 1;
            length = requested;
        }
        Ok(DataReference {
            qualified_name: layout.qualified_name.clone(),
            offset,
            length,
        })
    }
}

#[derive(Clone, Debug)]
struct DataSpec {
    level: u8,
    name: String,
    qualified: String,
    parent: Option<usize>,
    children: Vec<usize>,
    section: StorageSection,
    words: Vec<String>,
    sentence: String,
    redefines: Option<String>,
    occurs_min: usize,
    occurs_max: usize,
    depending_on: Option<String>,
    indexes: Vec<String>,
}

fn parse_specs(sentences: &[String], max_items: usize) -> Result<Vec<DataSpec>, SemanticProblem> {
    let mut specs: Vec<DataSpec> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut section = StorageSection::Working;
    let mut filler_counter = 0usize;
    for sentence in sentences {
        let upper = sentence.to_ascii_uppercase();
        if upper.contains("FILE SECTION") {
            section = StorageSection::File;
            stack.clear();
            continue;
        }
        if upper.contains("WORKING-STORAGE SECTION") {
            section = StorageSection::Working;
            stack.clear();
            continue;
        }
        if upper.contains("LOCAL-STORAGE SECTION") {
            section = StorageSection::Local;
            stack.clear();
            continue;
        }
        if upper.contains("LINKAGE SECTION") {
            section = StorageSection::Linkage;
            stack.clear();
            continue;
        }
        let words = words(sentence)
            .into_iter()
            .map(str::to_ascii_uppercase)
            .collect::<Vec<_>>();
        let Some(level) = words.first().and_then(|word| word.parse::<u8>().ok()) else {
            continue;
        };
        if words.len() < 2 || !matches!(level, 1..=49 | 66 | 77 | 78 | 88) {
            continue;
        }
        if specs.len() >= max_items {
            return Err(SemanticProblem::ItemLimitExceeded);
        }
        let name = if matches!(words[1].as_str(), "PIC" | "PICTURE" | "VALUE") {
            "FILLER".to_string()
        } else {
            words[1].trim_matches([',', '.']).to_string()
        };
        if name.is_empty() {
            return Err(SemanticProblem::InvalidDeclaration(sentence.clone()));
        }
        let parent = if level == 88 {
            stack.last().copied()
        } else if level == 66 {
            stack.iter().copied().find(|index| specs[*index].level == 1)
        } else {
            if matches!(level, 1 | 77 | 78) {
                stack.clear();
            } else {
                while stack
                    .last()
                    .is_some_and(|index| specs[*index].level >= level)
                {
                    stack.pop();
                }
            }
            stack.last().copied()
        };
        let component = if name == "FILLER" {
            filler_counter += 1;
            format!("FILLER#{filler_counter:05}")
        } else {
            name.clone()
        };
        let mut qualified = parent.map_or_else(
            || component.clone(),
            |parent| format!("{}.{}", specs[parent].qualified, component),
        );
        if name != "FILLER" && specs.iter().any(|spec| spec.qualified == qualified) {
            if find_after_owned(&words, "REDEFINES").is_some() || level == 88 {
                qualified = format!("{qualified}#ALTERNATE{:05}", specs.len() + 1);
            } else {
                return Err(SemanticProblem::DuplicateQualifiedName(qualified));
            }
        }
        let (occurs_min, occurs_max) = occurs_range(&words)?;
        let spec = DataSpec {
            level,
            name,
            qualified,
            parent,
            children: Vec::new(),
            section,
            redefines: find_after_owned(&words, "REDEFINES"),
            depending_on: find_sequence_after(&words, &["DEPENDING", "ON"]),
            indexes: values_after(&words, "INDEXED", "BY"),
            words,
            sentence: sentence.clone(),
            occurs_min,
            occurs_max,
        };
        let index = specs.len();
        specs.push(spec);
        if let Some(parent) = parent
            && !matches!(level, 66 | 88)
        {
            specs[parent].children.push(index);
        }
        if !matches!(level, 66 | 78 | 88) {
            stack.push(index);
        }
    }
    Ok(specs)
}

fn layout_siblings(
    siblings: &[usize],
    specs: &[DataSpec],
    layouts: &mut [Option<CobolLayout>],
    cursor: &mut usize,
    max_storage: usize,
) -> Result<(), SemanticProblem> {
    for index in siblings {
        let spec = &specs[*index];
        let target = spec
            .redefines
            .as_deref()
            .map(|name| resolve_sibling_layout(name, spec.parent, specs, layouts))
            .transpose()?
            .map(|layout| (layout.offset, layout.qualified_name.clone()));
        let start = target.as_ref().map_or(*cursor, |(offset, _)| *offset);
        let (category, elementary_length) = picture(&spec.words);
        let mut child_cursor = start;
        if !spec.children.is_empty() {
            layout_siblings(
                &spec.children,
                specs,
                layouts,
                &mut child_cursor,
                max_storage,
            )?;
        }
        let element_length = if spec.children.is_empty() {
            elementary_length
        } else {
            child_cursor.saturating_sub(start).max(elementary_length)
        };
        let length = element_length
            .checked_mul(spec.occurs_max)
            .ok_or(SemanticProblem::StorageLimitExceeded)?;
        let end = start
            .checked_add(length)
            .ok_or(SemanticProblem::StorageLimitExceeded)?;
        if end > max_storage {
            return Err(SemanticProblem::StorageLimitExceeded);
        }
        let initial = if spec.children.is_empty() {
            initial_value(&spec.sentence, &spec.words, element_length, category)
                .repeat(spec.occurs_max)
        } else {
            group_initial(
                *index,
                start,
                element_length,
                spec.occurs_max,
                specs,
                layouts,
            )
        };
        layouts[*index] = Some(CobolLayout {
            name: spec.name.clone(),
            qualified_name: spec.qualified.clone(),
            level: spec.level,
            section: spec.section,
            parent: spec.parent.map(|parent| specs[parent].qualified.clone()),
            offset: start,
            length,
            element_length,
            category: if spec.children.is_empty() {
                category
            } else {
                DataCategory::Group
            },
            initial,
            alias_of: target.map(|(_, qualified)| qualified),
            occurs: spec.occurs_max,
            occurs_min: spec.occurs_min,
            depending_on: spec.depending_on.clone(),
            indexes: spec.indexes.clone(),
            condition_values: Vec::new(),
        });
        *cursor = (*cursor).max(end);
    }
    Ok(())
}

fn group_initial(
    parent: usize,
    start: usize,
    element_length: usize,
    occurs: usize,
    specs: &[DataSpec],
    layouts: &[Option<CobolLayout>],
) -> Vec<u8> {
    let mut element = vec![b' '; element_length];
    for layout in specs[parent]
        .children
        .iter()
        .filter_map(|index| layouts[*index].as_ref())
    {
        if layout.offset < start || layout.offset >= start + element_length || layout.length == 0 {
            continue;
        }
        let relative = layout.offset - start;
        let copy = layout
            .initial
            .len()
            .min(element_length.saturating_sub(relative));
        element[relative..relative + copy].copy_from_slice(&layout.initial[..copy]);
    }
    element.repeat(occurs)
}

fn layout_specials(
    specs: &[DataSpec],
    layouts: &mut [Option<CobolLayout>],
) -> Result<(), SemanticProblem> {
    for (index, spec) in specs.iter().enumerate() {
        if spec.level == 88 {
            let parent = spec
                .parent
                .ok_or_else(|| SemanticProblem::InvalidDeclaration(spec.sentence.clone()))?;
            let target = layouts[parent].as_ref().ok_or_else(|| {
                SemanticProblem::IncompleteLayout(specs[parent].qualified.clone())
            })?;
            layouts[index] = Some(CobolLayout {
                name: spec.name.clone(),
                qualified_name: spec.qualified.clone(),
                level: 88,
                section: spec.section,
                parent: Some(target.qualified_name.clone()),
                offset: target.offset,
                length: 0,
                element_length: 0,
                category: DataCategory::Condition,
                initial: Vec::new(),
                alias_of: Some(target.qualified_name.clone()),
                occurs: 1,
                occurs_min: 1,
                depending_on: None,
                indexes: Vec::new(),
                condition_values: values_clause(&spec.words),
            });
        } else if spec.level == 66 {
            let start_name = find_after_owned(&spec.words, "RENAMES")
                .ok_or_else(|| SemanticProblem::InvalidDeclaration(spec.sentence.clone()))?;
            let start = resolve_nearby_layout(&start_name, spec, specs, layouts)?;
            let end = find_after_owned(&spec.words, "THRU")
                .or_else(|| find_after_owned(&spec.words, "THROUGH"))
                .map(|name| resolve_nearby_layout(&name, spec, specs, layouts))
                .transpose()?
                .unwrap_or(start);
            let length = end
                .offset
                .checked_add(end.length)
                .and_then(|value| value.checked_sub(start.offset))
                .ok_or(SemanticProblem::InvalidRename)?;
            layouts[index] = Some(CobolLayout {
                name: spec.name.clone(),
                qualified_name: spec.qualified.clone(),
                level: 66,
                section: spec.section,
                parent: spec.parent.map(|parent| specs[parent].qualified.clone()),
                offset: start.offset,
                length,
                element_length: length,
                category: DataCategory::Rename,
                initial: Vec::new(),
                alias_of: Some(start.qualified_name.clone()),
                occurs: 1,
                occurs_min: 1,
                depending_on: None,
                indexes: Vec::new(),
                condition_values: Vec::new(),
            });
        } else if spec.level == 78 {
            layouts[index] = Some(CobolLayout {
                name: spec.name.clone(),
                qualified_name: spec.qualified.clone(),
                level: 78,
                section: spec.section,
                parent: None,
                offset: 0,
                length: 0,
                element_length: 0,
                category: DataCategory::Condition,
                initial: Vec::new(),
                alias_of: None,
                occurs: 1,
                occurs_min: 1,
                depending_on: None,
                indexes: Vec::new(),
                condition_values: values_clause(&spec.words),
            });
        }
    }
    Ok(())
}

fn resolve_sibling_layout<'a>(
    name: &str,
    parent: Option<usize>,
    specs: &[DataSpec],
    layouts: &'a [Option<CobolLayout>],
) -> Result<&'a CobolLayout, SemanticProblem> {
    specs
        .iter()
        .enumerate()
        .find(|(index, spec)| {
            spec.parent == parent
                && spec.name.eq_ignore_ascii_case(name)
                && layouts[*index].is_some()
        })
        .and_then(|(index, _)| layouts[index].as_ref())
        .ok_or_else(|| SemanticProblem::UnknownRedefines(name.to_string()))
}

fn resolve_nearby_layout<'a>(
    name: &str,
    spec: &DataSpec,
    specs: &[DataSpec],
    layouts: &'a [Option<CobolLayout>],
) -> Result<&'a CobolLayout, SemanticProblem> {
    specs
        .iter()
        .enumerate()
        .find(|(_, candidate)| {
            candidate.section == spec.section && candidate.name.eq_ignore_ascii_case(name)
        })
        .and_then(|(index, _)| layouts[index].as_ref())
        .ok_or(SemanticProblem::InvalidRename)
}

fn strip_comments(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("*>"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn declaration_sentences(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut sentences = Vec::new();
    let mut start = 0usize;
    let mut quote = None;
    let mut index = 0usize;
    while index < bytes.len() {
        if matches!(bytes[index], b'\'' | b'"') {
            if quote == Some(bytes[index]) {
                if bytes.get(index + 1) == Some(&bytes[index]) {
                    index += 2;
                    continue;
                }
                quote = None;
            } else if quote.is_none() {
                quote = Some(bytes[index]);
            }
        } else if bytes[index] == b'.'
            && quote.is_none()
            && bytes
                .get(index + 1)
                .is_none_or(|next| next.is_ascii_whitespace())
        {
            let sentence = source[start..index].trim();
            if !sentence.is_empty() {
                sentences.push(sentence.to_string());
            }
            start = index + 1;
        }
        index += 1;
    }
    let tail = source[start..].trim();
    if !tail.is_empty() {
        sentences.push(tail.to_string());
    }
    sentences
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

fn find_after_owned(words: &[String], name: &str) -> Option<String> {
    words
        .iter()
        .position(|word| word.eq_ignore_ascii_case(name))
        .and_then(|index| words.get(index + 1).cloned())
}

fn find_sequence_after(words: &[String], sequence: &[&str]) -> Option<String> {
    words
        .windows(sequence.len())
        .position(|window| {
            window
                .iter()
                .zip(sequence)
                .all(|(word, expected)| word.eq_ignore_ascii_case(expected))
        })
        .and_then(|index| words.get(index + sequence.len()).cloned())
}

fn values_after(words: &[String], first: &str, second: &str) -> Vec<String> {
    let Some(start) = words.windows(2).position(|window| {
        window[0].eq_ignore_ascii_case(first) && window[1].eq_ignore_ascii_case(second)
    }) else {
        return Vec::new();
    };
    words[start + 2..]
        .iter()
        .take_while(|word| !matches!(word.as_str(), "VALUE" | "PIC" | "PICTURE"))
        .cloned()
        .collect()
}

fn occurs_range(words: &[String]) -> Result<(usize, usize), SemanticProblem> {
    let Some(index) = words.iter().position(|word| word == "OCCURS") else {
        return Ok((1, 1));
    };
    let minimum = words
        .get(index + 1)
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or(SemanticProblem::InvalidOccurs)?;
    let maximum = if words.get(index + 2).is_some_and(|word| word == "TO") {
        words
            .get(index + 3)
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or(SemanticProblem::InvalidOccurs)?
    } else {
        minimum
    };
    if maximum == 0 || minimum > maximum || maximum > 1_000_000 {
        return Err(SemanticProblem::InvalidOccurs);
    }
    Ok((minimum, maximum))
}

fn picture(words: &[String]) -> (DataCategory, usize) {
    let Some(pic) = find_after_owned(words, "PIC").or_else(|| find_after_owned(words, "PICTURE"))
    else {
        if words
            .iter()
            .any(|word| word == "POINTER" || word == "INDEX")
        {
            return (DataCategory::Pointer, 8);
        }
        return (DataCategory::Group, 0);
    };
    let usage = words.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let details = picture_details(&pic);
    if usage.contains("COMP-3") || usage.contains("PACKED-DECIMAL") {
        return (DataCategory::PackedDecimal, (details.digits + 2) / 2);
    }
    if usage.contains("COMP") || usage.contains("BINARY") || usage.contains("COMP-5") {
        return (
            DataCategory::Binary,
            if details.digits <= 4 {
                2
            } else if details.digits <= 9 {
                4
            } else {
                8
            },
        );
    }
    if details.numeric && details.edited {
        return (DataCategory::NumericEdited, details.storage.max(1));
    }
    if details.numeric {
        let separate = words
            .windows(2)
            .any(|pair| pair[0] == "SIGN" && pair[1] == "SEPARATE")
            || words.iter().any(|word| word == "SEPARATE");
        return (
            DataCategory::NumericDisplay,
            details.storage.max(1) + usize::from(details.signed && separate),
        );
    }
    (DataCategory::Alphanumeric, details.storage.max(1))
}

struct PictureDetails {
    storage: usize,
    digits: usize,
    numeric: bool,
    edited: bool,
    signed: bool,
}

fn picture_details(pic: &str) -> PictureDetails {
    let bytes = pic.as_bytes();
    let mut storage = 0usize;
    let mut digits = 0usize;
    let mut numeric = false;
    let mut edited = false;
    let mut signed = false;
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = bytes[index].to_ascii_uppercase();
        let mut repeat = 1usize;
        if bytes.get(index + 1) == Some(&b'(')
            && let Some(close) = pic[index + 2..].find(')')
        {
            repeat = pic[index + 2..index + 2 + close].parse().unwrap_or(1);
            index += close + 2;
        }
        match byte {
            b'9' => {
                numeric = true;
                digits += repeat;
                storage += repeat;
            }
            b'X' | b'A' => storage += repeat,
            b'Z' | b'*' => {
                numeric = true;
                edited = true;
                digits += repeat;
                storage += repeat;
            }
            b'S' => signed = true,
            b'V' | b'P' => numeric = true,
            b'+' | b'-' => {
                numeric = true;
                edited = true;
                signed = true;
                storage += repeat;
            }
            b',' | b'.' | b'$' | b'/' | b'B' | b'0' => {
                edited = true;
                storage += repeat;
            }
            _ => {}
        }
        index += 1;
    }
    PictureDetails {
        storage,
        digits,
        numeric,
        edited,
        signed,
    }
}

fn initial_value(
    sentence: &str,
    words: &[String],
    length: usize,
    category: DataCategory,
) -> Vec<u8> {
    let mut result = vec![
        if matches!(
            category,
            DataCategory::NumericDisplay | DataCategory::NumericEdited
        ) {
            b'0'
        } else {
            b' '
        };
        length
    ];
    let Some(value_index) = keyword_index(sentence, "VALUE") else {
        return result;
    };
    let tail = sentence[value_index + "VALUE".len()..].trim();
    let upper_tail = tail.to_ascii_uppercase();
    if upper_tail.starts_with("SPACE") {
        return vec![b' '; length];
    }
    if upper_tail.starts_with("LOW-VALUE") {
        return vec![0; length];
    }
    if upper_tail.starts_with("HIGH-VALUE") {
        return vec![0xff; length];
    }
    let (clean, repeat_all) = if upper_tail.starts_with("ALL ") {
        (quoted_or_word(tail[4..].trim(), words), true)
    } else {
        (quoted_or_word(tail, words), false)
    };
    if repeat_all && !clean.is_empty() {
        for (index, byte) in result.iter_mut().enumerate() {
            *byte = clean.as_bytes()[index % clean.len()];
        }
        return result;
    }
    match category {
        DataCategory::PackedDecimal => packed_decimal(&clean, length),
        DataCategory::Binary => binary_integer(&clean, length),
        DataCategory::NumericDisplay => {
            let negative = clean.trim_start().starts_with('-');
            let digits = clean.bytes().filter(u8::is_ascii_digit).collect::<Vec<_>>();
            let copy = digits.len().min(result.len());
            let result_start = result.len() - copy;
            result[result_start..].copy_from_slice(&digits[digits.len() - copy..]);
            if negative && let Some(last) = result.last_mut() {
                *last = negative_overpunch(*last);
            }
            result
        }
        _ => {
            let bytes = clean.as_bytes();
            let copy = bytes.len().min(result.len());
            result[..copy].copy_from_slice(&bytes[..copy]);
            result
        }
    }
}

fn keyword_index(source: &str, keyword: &str) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index].is_ascii_alphabetic() {
            let start = index;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'-')
            {
                index += 1;
            }
            if source[start..index].eq_ignore_ascii_case(keyword) {
                return Some(start);
            }
        } else {
            index += 1;
        }
    }
    None
}

fn quoted_or_word(tail: &str, words: &[String]) -> String {
    if let Some(quote) = tail.chars().next().filter(|ch| matches!(ch, '\'' | '"')) {
        return tail[quote.len_utf8()..]
            .split(quote)
            .next()
            .unwrap_or_default()
            .to_string();
    }
    find_after_owned(words, "VALUE").unwrap_or_default()
}

fn packed_decimal(value: &str, length: usize) -> Vec<u8> {
    let negative = value.trim_start().starts_with('-');
    let mut nibbles = value
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|byte| byte - b'0')
        .collect::<Vec<_>>();
    let digits = length.saturating_mul(2).saturating_sub(1);
    if nibbles.len() > digits {
        nibbles = nibbles[nibbles.len() - digits..].to_vec();
    }
    while nibbles.len() < digits {
        nibbles.insert(0, 0);
    }
    nibbles.push(if negative { 0x0d } else { 0x0c });
    nibbles
        .chunks(2)
        .map(|pair| (pair[0] << 4) | pair.get(1).copied().unwrap_or(0))
        .collect()
}

fn binary_integer(value: &str, length: usize) -> Vec<u8> {
    let parsed = value.parse::<i128>().unwrap_or(0).to_be_bytes();
    parsed[parsed.len().saturating_sub(length)..].to_vec()
}

fn negative_overpunch(digit: u8) -> u8 {
    const NEGATIVE: &[u8; 10] = b"}JKLMNOPQR";
    NEGATIVE
        .get(usize::from(digit.saturating_sub(b'0')))
        .copied()
        .unwrap_or(digit)
}

fn values_clause(words: &[String]) -> Vec<String> {
    let start = words
        .iter()
        .position(|word| matches!(word.as_str(), "VALUE" | "VALUES"));
    start.map_or_else(Vec::new, |index| words[index + 1..].to_vec())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SemanticProblem {
    MissingProgramId,
    DuplicateQualifiedName(String),
    UnknownRedefines(String),
    InvalidDeclaration(String),
    InvalidOccurs,
    InvalidRename,
    IncompleteLayout(String),
    ItemLimitExceeded,
    StorageLimitExceeded,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hierarchical_duplicates_qualification_and_group_layout_are_exact() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. T. DATA DIVISION. WORKING-STORAGE SECTION. 01 ROOT-A. 05 VALUE-X PIC X(2) VALUE 'AA'. 01 ROOT-B. 05 VALUE-X PIC X(3) VALUE 'BBB'. PROCEDURE DIVISION. STOP RUN.";
        let model = SemanticModel::analyze(source, 1024, 32).unwrap();
        assert!(model.layout("VALUE-X").is_none());
        assert_eq!(model.layout("VALUE-X OF ROOT-A").unwrap().initial, b"AA");
        assert_eq!(model.layout("ROOT-A").unwrap().length, 2);
        assert_eq!(model.layout("ROOT-B").unwrap().offset, 2);
    }

    #[test]
    fn levels_redefines_occurs_conditions_and_references_are_explicit() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. T. DATA DIVISION. WORKING-STORAGE SECTION. 01 ROOT. 05 COUNT-X PIC 9 VALUE 2. 05 TABLE-X OCCURS 1 TO 3 TIMES DEPENDING ON COUNT-X INDEXED BY IX. 10 ITEM-X PIC X(2) VALUE 'AB'. 05 RAW-X PIC X(4). 05 NUM-X REDEFINES RAW-X PIC 9(4). 66 RANGE-X RENAMES COUNT-X THRU RAW-X. 77 SOLO-X PIC S9(4) COMP-3 VALUE -12. 88 SOLO-VALID VALUE 1 THRU 9. PROCEDURE DIVISION. STOP RUN.";
        let model = SemanticModel::analyze(source, 1024, 64).unwrap();
        let table = model.layout("TABLE-X").unwrap();
        assert_eq!(
            (table.occurs_min, table.occurs, table.element_length),
            (1, 3, 2)
        );
        assert_eq!(table.depending_on.as_deref(), Some("COUNT-X"));
        assert_eq!(table.indexes, ["IX"]);
        assert_eq!(
            model.layout("NUM-X").unwrap().offset,
            model.layout("RAW-X").unwrap().offset
        );
        assert_eq!(
            model.layout("RANGE-X").unwrap().category,
            DataCategory::Rename
        );
        assert_eq!(
            model.layout("SOLO-VALID").unwrap().condition_values,
            ["1", "THRU", "9"]
        );
        assert_eq!(
            model.layout("SOLO-X").unwrap().initial,
            vec![0x00, 0x01, 0x2d]
        );
        assert_eq!(
            model
                .resolve_reference("TABLE-X", Some(3), Some((2, 1)))
                .unwrap()
                .length,
            1
        );
    }

    #[test]
    fn display_binary_packed_and_edited_storage_lengths_are_bounded() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. T. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC S9(4). 01 B PIC S9(9) COMP. 01 C PIC S9(7)V99 COMP-3 VALUE -123.45. 01 D PIC ZZ,ZZ9.99-. PROCEDURE DIVISION. STOP RUN.";
        let model = SemanticModel::analyze(source, 1024, 32).unwrap();
        assert_eq!(model.layout("A").unwrap().length, 4);
        assert_eq!(model.layout("B").unwrap().length, 4);
        assert_eq!(model.layout("C").unwrap().length, 5);
        assert_eq!(
            model.layout("D").unwrap().category,
            DataCategory::NumericEdited
        );
        assert_eq!(model.layout("D").unwrap().length, 10);
    }

    #[test]
    fn file_and_linkage_roots_keep_section_and_reference_identity() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. T. DATA DIVISION. FILE SECTION. FD INPUT-FILE. 01 INPUT-RECORD. 05 INPUT-ID PIC X(8). WORKING-STORAGE SECTION. 77 FLAG-X PIC X. LINKAGE SECTION. 01 DFHCOMMAREA. 05 LINK-BYTE PIC X OCCURS 1 TO 8 TIMES DEPENDING ON FLAG-X. PROCEDURE DIVISION. STOP RUN.";
        let model = SemanticModel::analyze(source, 1024, 32).unwrap();
        assert_eq!(
            model.layout("INPUT-RECORD").unwrap().section,
            StorageSection::File
        );
        assert_eq!(
            model.layout("LINK-BYTE").unwrap().section,
            StorageSection::Linkage
        );
        assert_eq!(
            model
                .resolve_reference("LINK-BYTE", Some(8), None)
                .unwrap()
                .length,
            1
        );
    }
}
