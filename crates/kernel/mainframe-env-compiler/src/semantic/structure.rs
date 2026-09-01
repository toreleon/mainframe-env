use super::{SemanticProblem, StorageSection};
use crate::generated::cobol_language::{DataDescriptionClauseKind, FileDescriptionClauseKind};
use crate::syntax::{SourceOrigin, SourceSpan};
use std::collections::BTreeSet;
use std::ops::Range;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CobolDivisionKind {
    Identification,
    Environment,
    Data,
    Procedure,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CobolDivisionNode {
    pub kind: CobolDivisionKind,
    pub ordinal: u32,
    pub source: Vec<SourceSpan>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CobolSectionKind {
    Configuration,
    InputOutput,
    File,
    WorkingStorage,
    LocalStorage,
    Linkage,
    Communication,
    Report,
    Screen,
    Declaratives,
    Procedure,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CobolSectionNode {
    pub kind: CobolSectionKind,
    pub name: String,
    pub ordinal: u32,
    pub source: Vec<SourceSpan>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CobolScopeId(u32);

impl CobolScopeId {
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CobolScopeKind {
    Program,
    Function,
    Class,
    Method,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CobolScope {
    pub id: CobolScopeId,
    pub kind: CobolScopeKind,
    pub name: String,
    pub parent: Option<CobolScopeId>,
    pub source: Vec<SourceSpan>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CobolClauseKind {
    File(FileDescriptionClauseKind),
    Data(DataDescriptionClauseKind),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CobolClauseNode {
    pub kind: CobolClauseKind,
    pub operands: Vec<String>,
    pub source: Vec<SourceSpan>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CobolFileDescription {
    pub name: String,
    pub sort_merge: bool,
    pub clauses: Vec<CobolClauseNode>,
    pub source: Vec<SourceSpan>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CobolDataDescription {
    pub name: String,
    pub qualified_name: String,
    pub level: u8,
    pub section: StorageSection,
    pub clauses: Vec<CobolClauseNode>,
    pub source: Vec<SourceSpan>,
}

pub(super) struct SemanticStructure {
    pub divisions: Vec<CobolDivisionNode>,
    pub sections: Vec<CobolSectionNode>,
    pub scopes: Vec<CobolScope>,
    pub file_descriptions: Vec<CobolFileDescription>,
    pub data_descriptions: Vec<CobolDataDescription>,
}

pub(super) fn analyze(
    source: &str,
    origins: &[SourceOrigin],
    max_items: usize,
) -> Result<SemanticStructure, SemanticProblem> {
    let upper = source.to_ascii_uppercase();
    let divisions = divisions(source, origins)?;
    let sections = sections(source, origins)?;
    let scopes = scopes(source, origins, max_items)?;
    let data_start = upper.find("DATA DIVISION").unwrap_or(0);
    let data_end = upper[data_start..]
        .find("PROCEDURE DIVISION")
        .map_or(source.len(), |offset| data_start + offset);
    let mut file_descriptions = Vec::new();
    let mut data_descriptions = Vec::new();
    let mut type_names = BTreeSet::new();
    let mut section = StorageSection::Working;
    let mut hierarchy = Vec::<(u8, String)>::new();
    for range in sentence_ranges(&source[data_start..data_end]) {
        let range = data_start + range.start..data_start + range.end;
        let Some((sentence, range)) = sentence_content(source, range) else {
            continue;
        };
        let words = words(&sentence);
        let upper_sentence = sentence.to_ascii_uppercase();
        if upper_sentence.contains("FILE SECTION") {
            section = StorageSection::File;
            hierarchy.clear();
            continue;
        }
        if upper_sentence.contains("WORKING-STORAGE SECTION") {
            section = StorageSection::Working;
            hierarchy.clear();
            continue;
        }
        if upper_sentence.contains("LOCAL-STORAGE SECTION") {
            section = StorageSection::Local;
            hierarchy.clear();
            continue;
        }
        if upper_sentence.contains("LINKAGE SECTION") {
            section = StorageSection::Linkage;
            hierarchy.clear();
            continue;
        }
        if matches!(words.first().map(String::as_str), Some("FD" | "SD")) {
            if file_descriptions.len() + data_descriptions.len() >= max_items {
                return Err(SemanticProblem::ItemLimitExceeded);
            }
            file_descriptions.push(parse_file_description(&sentence, &words, range, origins)?);
            hierarchy.clear();
            continue;
        }
        let Some(level) = words.first().and_then(|word| word.parse::<u8>().ok()) else {
            continue;
        };
        if !matches!(level, 1..=49 | 66 | 77 | 78 | 88) || words.len() < 2 {
            return Err(SemanticProblem::InvalidDeclaration(sentence));
        }
        if file_descriptions.len() + data_descriptions.len() >= max_items {
            return Err(SemanticProblem::ItemLimitExceeded);
        }
        let name = if clause_word(&words[1]) {
            "FILLER".into()
        } else {
            words[1].clone()
        };
        if matches!(level, 1 | 77 | 78) {
            hierarchy.clear();
        } else if !matches!(level, 66 | 88) {
            while hierarchy.last().is_some_and(|(parent, _)| *parent >= level) {
                hierarchy.pop();
            }
        }
        let qualified_name = hierarchy
            .last()
            .map_or_else(|| name.clone(), |(_, parent)| format!("{parent}.{name}"));
        let clauses =
            parse_data_clauses(&sentence, &words, level, section, range.clone(), origins)?;
        if let Some(type_clause) = clauses
            .iter()
            .find(|clause| clause.kind == CobolClauseKind::Data(DataDescriptionClauseKind::Type))
        {
            let target = type_clause
                .operands
                .first()
                .ok_or_else(|| SemanticProblem::InvalidClause("TYPE target missing".into()))?;
            if !type_names.contains(target) {
                return Err(SemanticProblem::InvalidClause(format!(
                    "TYPE target {target} is not a prior TYPEDEF"
                )));
            }
        }
        if clauses
            .iter()
            .any(|clause| clause.kind == CobolClauseKind::Data(DataDescriptionClauseKind::Typedef))
            && !type_names.insert(name.clone())
        {
            return Err(SemanticProblem::InvalidClause(format!(
                "duplicate TYPEDEF {name}"
            )));
        }
        data_descriptions.push(CobolDataDescription {
            name,
            qualified_name: qualified_name.clone(),
            level,
            section,
            clauses,
            source: spans(origins, range),
        });
        if !matches!(level, 66 | 78 | 88) {
            hierarchy.push((level, qualified_name));
        }
    }
    Ok(SemanticStructure {
        divisions,
        sections,
        scopes,
        file_descriptions,
        data_descriptions,
    })
}

fn divisions(
    source: &str,
    origins: &[SourceOrigin],
) -> Result<Vec<CobolDivisionNode>, SemanticProblem> {
    let mut found = Vec::new();
    for (marker, kind) in [
        ("IDENTIFICATION DIVISION", CobolDivisionKind::Identification),
        ("ENVIRONMENT DIVISION", CobolDivisionKind::Environment),
        ("DATA DIVISION", CobolDivisionKind::Data),
        ("PROCEDURE DIVISION", CobolDivisionKind::Procedure),
    ] {
        for start in marker_positions(source, marker) {
            found.push((start, marker.len(), kind));
        }
    }
    found.sort_by_key(|(start, _, _)| *start);
    found
        .into_iter()
        .enumerate()
        .map(|(ordinal, (start, length, kind))| {
            Ok(CobolDivisionNode {
                kind,
                ordinal: u32::try_from(ordinal).map_err(|_| SemanticProblem::ScopeLimitExceeded)?,
                source: spans(origins, start..start + length),
            })
        })
        .collect()
}

fn sections(
    source: &str,
    origins: &[SourceOrigin],
) -> Result<Vec<CobolSectionNode>, SemanticProblem> {
    let mut result = Vec::new();
    for range in sentence_ranges(source) {
        let sentence = source[range.clone()].trim();
        let words = words(sentence);
        let Some(index) = words.iter().position(|word| word == "SECTION") else {
            continue;
        };
        let name = words[..index]
            .last()
            .cloned()
            .unwrap_or_else(|| "SECTION".into());
        let kind = match name.as_str() {
            "CONFIGURATION" => CobolSectionKind::Configuration,
            "INPUT-OUTPUT" => CobolSectionKind::InputOutput,
            "FILE" => CobolSectionKind::File,
            "WORKING-STORAGE" => CobolSectionKind::WorkingStorage,
            "LOCAL-STORAGE" => CobolSectionKind::LocalStorage,
            "LINKAGE" => CobolSectionKind::Linkage,
            "COMMUNICATION" => CobolSectionKind::Communication,
            "REPORT" => CobolSectionKind::Report,
            "SCREEN" => CobolSectionKind::Screen,
            "DECLARATIVES" => CobolSectionKind::Declaratives,
            _ if source[..range.start]
                .to_ascii_uppercase()
                .contains("PROCEDURE DIVISION") =>
            {
                CobolSectionKind::Procedure
            }
            _ => CobolSectionKind::Other,
        };
        result.push(CobolSectionNode {
            kind,
            name,
            ordinal: u32::try_from(result.len())
                .map_err(|_| SemanticProblem::ScopeLimitExceeded)?,
            source: spans(origins, range),
        });
    }
    Ok(result)
}

fn scopes(
    source: &str,
    origins: &[SourceOrigin],
    max_items: usize,
) -> Result<Vec<CobolScope>, SemanticProblem> {
    let mut events = Vec::<(usize, bool, CobolScopeKind, usize)>::new();
    for (marker, kind) in [
        ("PROGRAM-ID", CobolScopeKind::Program),
        ("FUNCTION-ID", CobolScopeKind::Function),
        ("CLASS-ID", CobolScopeKind::Class),
        ("METHOD-ID", CobolScopeKind::Method),
    ] {
        for start in marker_positions(source, marker) {
            events.push((start, true, kind, marker.len()));
        }
    }
    for (marker, kind) in [
        ("END PROGRAM", CobolScopeKind::Program),
        ("END FUNCTION", CobolScopeKind::Function),
        ("END CLASS", CobolScopeKind::Class),
        ("END METHOD", CobolScopeKind::Method),
    ] {
        for start in marker_positions(source, marker) {
            events.push((start, false, kind, marker.len()));
        }
    }
    events.sort_by_key(|(start, opening, _, _)| (*start, !*opening));
    let mut result = Vec::new();
    let mut stack = Vec::<(CobolScopeId, CobolScopeKind)>::new();
    for (start, opening, kind, marker_len) in events {
        if !opening {
            let Some((_, open_kind)) = stack.pop() else {
                return Err(SemanticProblem::InvalidDeclaration(format!(
                    "unmatched END {kind:?}"
                )));
            };
            if open_kind != kind {
                return Err(SemanticProblem::InvalidDeclaration(format!(
                    "END {kind:?} closes {open_kind:?}"
                )));
            }
            continue;
        }
        let tail = &source[start + marker_len..];
        let trimmed = tail.trim_start_matches(|character: char| {
            character.is_ascii_whitespace() || character == '.'
        });
        let skipped = tail.len() - trimmed.len();
        let name = trimmed
            .split(|character: char| {
                character.is_ascii_whitespace() || matches!(character, '.' | ',' | ';')
            })
            .next()
            .filter(|name| !name.is_empty())
            .ok_or_else(|| SemanticProblem::InvalidDeclaration(format!("missing {kind:?} name")))?
            .trim_matches(['\'', '"'])
            .to_ascii_uppercase();
        validate_name(&name)?;
        let end = start + marker_len + skipped + name.len();
        if result.len() >= max_items {
            return Err(SemanticProblem::ScopeLimitExceeded);
        }
        let id = CobolScopeId(
            u32::try_from(result.len()).map_err(|_| SemanticProblem::ScopeLimitExceeded)?,
        );
        result.push(CobolScope {
            id,
            kind,
            name,
            parent: stack.last().map(|(id, _)| *id),
            source: spans(origins, start..end),
        });
        stack.push((id, kind));
    }
    Ok(result)
}

fn parse_file_description(
    sentence: &str,
    words: &[String],
    range: Range<usize>,
    origins: &[SourceOrigin],
) -> Result<CobolFileDescription, SemanticProblem> {
    let name = words
        .get(1)
        .cloned()
        .ok_or_else(|| SemanticProblem::InvalidDeclaration(sentence.into()))?;
    validate_name(&name)?;
    let mut kinds = Vec::new();
    for kind in FileDescriptionClauseKind::ALL {
        let occurrences = file_clause_occurrences(words, kind);
        if occurrences > 1 {
            return Err(SemanticProblem::InvalidClause(format!(
                "duplicate {kind:?}"
            )));
        }
        if occurrences == 1 {
            kinds.push(kind);
        }
    }
    let mut seen = BTreeSet::new();
    let mut clauses = Vec::new();
    for kind in kinds {
        if !seen.insert(kind) {
            return Err(SemanticProblem::InvalidClause(format!(
                "duplicate {kind:?}"
            )));
        }
        let operands = validate_file_clause(kind, words)?;
        clauses.push(CobolClauseNode {
            kind: CobolClauseKind::File(kind),
            operands,
            source: spans(origins, range.clone()),
        });
    }
    if words[0] == "SD"
        && clauses.iter().any(|clause| {
            matches!(
                clause.kind,
                CobolClauseKind::File(
                    FileDescriptionClauseKind::External | FileDescriptionClauseKind::Global
                )
            )
        })
    {
        return Err(SemanticProblem::InvalidClause(
            "EXTERNAL/GLOBAL requires an FD entry".into(),
        ));
    }
    Ok(CobolFileDescription {
        name,
        sort_merge: words[0] == "SD",
        clauses,
        source: spans(origins, range),
    })
}

fn file_clause_occurrences(words: &[String], kind: FileDescriptionClauseKind) -> usize {
    use FileDescriptionClauseKind as K;
    match kind {
        K::External => count(words, &["EXTERNAL"]),
        K::Global => count(words, &["GLOBAL"]),
        K::BlockContains => count(words, &["BLOCK", "CONTAINS"]),
        K::Record => {
            count(words, &["RECORD", "CONTAINS"]) + count(words, &["RECORD", "IS", "VARYING"])
        }
        K::LabelRecords => count(words, &["LABEL", "RECORDS"]) + count(words, &["LABEL", "RECORD"]),
        K::ValueOf => count(words, &["VALUE", "OF"]),
        K::DataRecords => count(words, &["DATA", "RECORDS"]) + count(words, &["DATA", "RECORD"]),
        K::Linage => count(words, &["LINAGE"]),
        K::RecordingMode => count(words, &["RECORDING", "MODE"]),
        K::CodeSet => count(words, &["CODE-SET"]),
    }
}

fn validate_file_clause(
    kind: FileDescriptionClauseKind,
    words: &[String],
) -> Result<Vec<String>, SemanticProblem> {
    use FileDescriptionClauseKind as K;
    let invalid = || SemanticProblem::InvalidClause(format!("invalid {kind:?}"));
    match kind {
        K::External | K::Global => Ok(Vec::new()),
        K::BlockContains => {
            let index = sequence(words, &["BLOCK", "CONTAINS"]).ok_or_else(invalid)? + 2;
            let (values, unit) = numeric_range(words, index)?;
            if !matches!(unit.as_deref(), None | Some("CHARACTERS" | "RECORDS")) {
                return Err(invalid());
            }
            Ok(values)
        }
        K::Record => {
            if let Some(index) = sequence(words, &["RECORD", "CONTAINS"]) {
                return numeric_range(words, index + 2).map(|(values, _)| values);
            }
            let index = sequence(words, &["RECORD", "IS", "VARYING"]).ok_or_else(invalid)?;
            Ok(words[index + 3..].to_vec())
        }
        K::LabelRecords => {
            let index = words
                .iter()
                .position(|word| word == "LABEL")
                .ok_or_else(invalid)?;
            let operand = words
                .get(
                    index
                        + 2
                        + usize::from(
                            words
                                .get(index + 2)
                                .is_some_and(|word| matches!(word.as_str(), "IS" | "ARE")),
                        ),
                )
                .ok_or_else(invalid)?;
            if !matches!(operand.as_str(), "STANDARD" | "OMITTED") {
                validate_name(operand)?;
            }
            Ok(vec![operand.clone()])
        }
        K::ValueOf => {
            let index = sequence(words, &["VALUE", "OF"]).ok_or_else(invalid)?;
            let name = words.get(index + 2).ok_or_else(invalid)?;
            validate_name(name)?;
            let value_index =
                index + 3 + usize::from(words.get(index + 3).is_some_and(|word| word == "IS"));
            let value = words.get(value_index).ok_or_else(invalid)?;
            Ok(vec![name.clone(), value.clone()])
        }
        K::DataRecords => {
            let index = words
                .iter()
                .position(|word| word == "DATA")
                .ok_or_else(invalid)?;
            let start =
                index + 2 + usize::from(words.get(index + 2).is_some_and(|word| word == "ARE"));
            let names = words[start..]
                .iter()
                .take_while(|word| !file_clause_start(word))
                .cloned()
                .collect::<Vec<_>>();
            if names.is_empty() {
                return Err(invalid());
            }
            for name in &names {
                validate_name(name)?;
            }
            Ok(names)
        }
        K::Linage => {
            let index = words
                .iter()
                .position(|word| word == "LINAGE")
                .ok_or_else(invalid)?;
            let first =
                index + 1 + usize::from(words.get(index + 1).is_some_and(|word| word == "IS"));
            let body = words.get(first).ok_or_else(invalid)?;
            if body.parse::<usize>().is_ok_and(|value| value == 0) {
                return Err(invalid());
            }
            validate_unsigned_or_name(body)?;
            if let Some(footing) =
                sequence(words, &["WITH", "FOOTING", "AT"]).and_then(|index| words.get(index + 3))
            {
                validate_unsigned_or_name(footing)?;
                if let (Ok(body), Ok(footing)) = (body.parse::<usize>(), footing.parse::<usize>())
                    && (footing == 0 || footing > body)
                {
                    return Err(invalid());
                }
            }
            Ok(words[first..].to_vec())
        }
        K::RecordingMode => {
            let index = sequence(words, &["RECORDING", "MODE"]).ok_or_else(invalid)?;
            let value = words
                .get(index + 2 + usize::from(words.get(index + 2).is_some_and(|word| word == "IS")))
                .ok_or_else(invalid)?;
            if !matches!(value.as_str(), "F" | "V" | "U" | "S") {
                return Err(invalid());
            }
            Ok(vec![value.clone()])
        }
        K::CodeSet => {
            let index = words
                .iter()
                .position(|word| word == "CODE-SET")
                .ok_or_else(invalid)?;
            let value = words
                .get(index + 1 + usize::from(words.get(index + 1).is_some_and(|word| word == "IS")))
                .ok_or_else(invalid)?;
            validate_name(value)?;
            Ok(vec![value.clone()])
        }
    }
}

fn parse_data_clauses(
    sentence: &str,
    words: &[String],
    level: u8,
    section: StorageSection,
    range: Range<usize>,
    origins: &[SourceOrigin],
) -> Result<Vec<CobolClauseNode>, SemanticProblem> {
    let mut clauses = Vec::new();
    let mut seen = BTreeSet::new();
    for kind in DataDescriptionClauseKind::ALL {
        let occurrences = data_clause_occurrences(words, kind);
        if occurrences > 1 {
            return Err(SemanticProblem::InvalidClause(format!(
                "duplicate {kind:?}"
            )));
        }
        if occurrences == 0 {
            continue;
        }
        if !seen.insert(kind) {
            return Err(SemanticProblem::InvalidClause(format!(
                "duplicate {kind:?}"
            )));
        }
        let operands = validate_data_clause(kind, sentence, words, level, section)?;
        clauses.push(CobolClauseNode {
            kind: CobolClauseKind::Data(kind),
            operands,
            source: spans(origins, range.clone()),
        });
    }
    Ok(clauses)
}

fn data_clause_occurrences(words: &[String], kind: DataDescriptionClauseKind) -> usize {
    use DataDescriptionClauseKind as K;
    match kind {
        K::BlankWhenZero => count(words, &["BLANK", "WHEN"]),
        K::DynamicLength => count(words, &["DYNAMIC"]),
        K::External => count(words, &["EXTERNAL"]),
        K::Global => count(words, &["GLOBAL"]),
        K::Justified => count(words, &["JUSTIFIED"]) + count(words, &["JUST"]),
        K::GroupUsage => count(words, &["GROUP-USAGE"]),
        K::Occurs => count(words, &["OCCURS"]),
        K::Picture => count(words, &["PIC"]) + count(words, &["PICTURE"]),
        K::Redefines => count(words, &["REDEFINES"]),
        K::Renames => count(words, &["RENAMES"]),
        K::Sign => count(words, &["SIGN"]),
        K::Synchronized => count(words, &["SYNCHRONIZED"]) + count(words, &["SYNC"]),
        K::Typedef => count(words, &["TYPEDEF"]),
        K::Type => count(words, &["TYPE"]),
        K::Usage => {
            let explicit = count(words, &["USAGE"]);
            explicit.max(words.iter().filter(|word| usage_value(word)).count())
        }
        K::Value => count(words, &["VALUE"]) + count(words, &["VALUES"]),
        K::Volatile => count(words, &["VOLATILE"]),
    }
}

fn validate_data_clause(
    kind: DataDescriptionClauseKind,
    sentence: &str,
    words: &[String],
    level: u8,
    section: StorageSection,
) -> Result<Vec<String>, SemanticProblem> {
    use DataDescriptionClauseKind as K;
    let invalid = || SemanticProblem::InvalidClause(format!("invalid {kind:?}: {sentence}"));
    let named = words
        .get(1)
        .is_some_and(|word| !clause_word(word) && word != "FILLER");
    match kind {
        K::BlankWhenZero => {
            let index = words
                .iter()
                .position(|word| word == "BLANK")
                .ok_or_else(invalid)?;
            if words.get(index + 1).is_none_or(|word| word != "WHEN")
                || words
                    .get(index + 2)
                    .is_none_or(|word| !matches!(word.as_str(), "ZERO" | "ZEROS" | "ZEROES"))
            {
                return Err(invalid());
            }
            Ok(vec![words[index + 2].clone()])
        }
        K::DynamicLength => {
            let index = words
                .iter()
                .position(|word| word == "DYNAMIC")
                .ok_or_else(invalid)?;
            let mut cursor = index + 1;
            if words.get(cursor).is_some_and(|word| word == "LENGTH") {
                cursor += 1;
            }
            if words.get(cursor).is_some_and(|word| word == "LIMIT") {
                cursor += 1 + usize::from(words.get(cursor + 1).is_some_and(|word| word == "IS"));
                let limit = words
                    .get(cursor)
                    .and_then(|word| word.parse::<usize>().ok())
                    .ok_or_else(invalid)?;
                if !(1..=999_999_999).contains(&limit) {
                    return Err(invalid());
                }
            }
            let picture = picture_operand(words).ok_or_else(invalid)?;
            if !matches!(picture.as_str(), "X" | "U") {
                return Err(invalid());
            }
            Ok(words[index + 1..].to_vec())
        }
        K::External => {
            if level != 1
                || section != StorageSection::Working
                || !named
                || contains(words, &["TYPEDEF"])
            {
                return Err(invalid());
            }
            Ok(Vec::new())
        }
        K::Global => {
            if level != 1 || !named {
                return Err(invalid());
            }
            Ok(Vec::new())
        }
        K::Justified => {
            let index = words
                .iter()
                .position(|word| matches!(word.as_str(), "JUST" | "JUSTIFIED"))
                .ok_or_else(invalid)?;
            if picture_operand(words).is_none()
                || words.get(index + 1).is_some_and(|word| word != "RIGHT")
                || contains(words, &["TYPE"])
            {
                return Err(invalid());
            }
            Ok(vec!["RIGHT".into()])
        }
        K::GroupUsage => {
            let index = words
                .iter()
                .position(|word| word == "GROUP-USAGE")
                .ok_or_else(invalid)?;
            let value = words
                .get(index + 1 + usize::from(words.get(index + 1).is_some_and(|word| word == "IS")))
                .ok_or_else(invalid)?;
            if !matches!(value.as_str(), "NATIONAL" | "UTF-8") || picture_operand(words).is_some() {
                return Err(invalid());
            }
            Ok(vec![value.clone()])
        }
        K::Occurs => {
            let index = words
                .iter()
                .position(|word| word == "OCCURS")
                .ok_or_else(invalid)?;
            if matches!(level, 1 | 66 | 77 | 78 | 88) {
                return Err(invalid());
            }
            let (minimum, maximum, unbounded) =
                if words.get(index + 1).is_some_and(|word| word == "UNBOUNDED") {
                    (1, 0, true)
                } else {
                    let minimum = words
                        .get(index + 1)
                        .and_then(|word| word.parse::<usize>().ok())
                        .ok_or_else(invalid)?;
                    let maximum_word = if words.get(index + 2).is_some_and(|word| word == "TO") {
                        words.get(index + 3).ok_or_else(invalid)?
                    } else {
                        words.get(index + 1).ok_or_else(invalid)?
                    };
                    if maximum_word == "UNBOUNDED" {
                        (minimum, 0, true)
                    } else {
                        (
                            minimum,
                            maximum_word.parse::<usize>().map_err(|_| invalid())?,
                            false,
                        )
                    }
                };
            let variable = contains(words, &["DEPENDING", "ON"]);
            if (!unbounded && (maximum == 0 || minimum > maximum))
                || (!variable && minimum == 0)
                || (unbounded && !variable)
            {
                return Err(invalid());
            }
            Ok(vec![
                minimum.to_string(),
                if unbounded {
                    "UNBOUNDED".into()
                } else {
                    maximum.to_string()
                },
            ])
        }
        K::Picture => {
            let picture = picture_operand(words).ok_or_else(invalid)?;
            if picture.is_empty() || contains(words, &["TYPE"]) || matches!(level, 66 | 88) {
                return Err(invalid());
            }
            Ok(vec![picture])
        }
        K::Redefines => {
            let index = words
                .iter()
                .position(|word| word == "REDEFINES")
                .ok_or_else(invalid)?;
            if index > 2 || matches!(level, 66 | 88) {
                return Err(invalid());
            }
            let target = words.get(index + 1).ok_or_else(invalid)?;
            validate_name(target)?;
            Ok(vec![target.clone()])
        }
        K::Renames => {
            let index = words
                .iter()
                .position(|word| word == "RENAMES")
                .ok_or_else(invalid)?;
            if level != 66 {
                return Err(invalid());
            }
            let start = words.get(index + 1).ok_or_else(invalid)?;
            validate_name(start)?;
            Ok(words[index + 1..].to_vec())
        }
        K::Sign => {
            let index = words
                .iter()
                .position(|word| word == "SIGN")
                .ok_or_else(invalid)?;
            let mut cursor =
                index + 1 + usize::from(words.get(index + 1).is_some_and(|word| word == "IS"));
            let position = words.get(cursor).ok_or_else(invalid)?;
            if !matches!(position.as_str(), "LEADING" | "TRAILING")
                || picture_operand(words).is_none_or(|picture| !picture.contains('S'))
            {
                return Err(invalid());
            }
            cursor += 1;
            Ok(words[index + 1..cursor].to_vec())
        }
        K::Synchronized => {
            let index = words
                .iter()
                .position(|word| matches!(word.as_str(), "SYNC" | "SYNCHRONIZED"))
                .ok_or_else(invalid)?;
            if contains(words, &["TYPE"]) || matches!(level, 66 | 88) {
                return Err(invalid());
            }
            if words
                .get(index + 1)
                .is_some_and(|word| !matches!(word.as_str(), "LEFT" | "RIGHT"))
            {
                return Err(invalid());
            }
            Ok(words.get(index + 1).into_iter().cloned().collect())
        }
        K::Typedef => {
            if level != 1
                || contains(words, &["EXTERNAL"])
                || contains(words, &["VOLATILE"])
                || contains(words, &["REDEFINES"])
            {
                return Err(invalid());
            }
            Ok(Vec::new())
        }
        K::Type => {
            let index = words
                .iter()
                .position(|word| word == "TYPE")
                .ok_or_else(invalid)?;
            let target = words
                .get(index + 1 + usize::from(words.get(index + 1).is_some_and(|word| word == "TO")))
                .ok_or_else(invalid)?;
            validate_name(target)?;
            if [
                "BLANK",
                "JUST",
                "JUSTIFIED",
                "PIC",
                "PICTURE",
                "RENAMES",
                "SIGN",
                "SYNC",
                "SYNCHRONIZED",
                "DYNAMIC",
                "USAGE",
            ]
            .iter()
            .any(|clause| contains(words, &[*clause]))
            {
                return Err(invalid());
            }
            Ok(vec![target.clone()])
        }
        K::Usage => {
            if matches!(level, 66 | 88) {
                return Err(invalid());
            }
            let representation = words
                .iter()
                .skip(2)
                .find(|word| usage_value(word))
                .ok_or_else(invalid)?;
            if representation == "OBJECT"
                && !words.windows(2).any(|pair| pair == ["OBJECT", "REFERENCE"])
            {
                return Err(invalid());
            }
            Ok(vec![representation.clone()])
        }
        K::Value => {
            let index = words
                .iter()
                .position(|word| matches!(word.as_str(), "VALUE" | "VALUES"))
                .ok_or_else(invalid)?;
            let start = index
                + 1
                + usize::from(
                    words
                        .get(index + 1)
                        .is_some_and(|word| matches!(word.as_str(), "IS" | "ARE")),
                );
            if start >= words.len() {
                return Err(invalid());
            }
            if level == 88 && words[..index].iter().any(|word| clause_word(word)) {
                return Err(invalid());
            }
            Ok(words[start..].to_vec())
        }
        K::Volatile => {
            if matches!(level, 66 | 88) || contains(words, &["TYPEDEF"]) {
                return Err(invalid());
            }
            Ok(Vec::new())
        }
    }
}

fn picture_operand(words: &[String]) -> Option<String> {
    let index = words
        .iter()
        .position(|word| matches!(word.as_str(), "PIC" | "PICTURE"))?;
    words
        .get(index + 1 + usize::from(words.get(index + 1).is_some_and(|word| word == "IS")))
        .cloned()
}

fn usage_value(word: &str) -> bool {
    matches!(
        word,
        "BINARY"
            | "COMP"
            | "COMP-1"
            | "COMP-2"
            | "COMP-3"
            | "COMP-4"
            | "COMP-5"
            | "COMPUTATIONAL"
            | "COMPUTATIONAL-1"
            | "COMPUTATIONAL-2"
            | "COMPUTATIONAL-3"
            | "COMPUTATIONAL-4"
            | "COMPUTATIONAL-5"
            | "DISPLAY"
            | "DISPLAY-1"
            | "INDEX"
            | "NATIONAL"
            | "UTF-8"
            | "PACKED-DECIMAL"
            | "POINTER"
            | "POINTER-32"
            | "PROCEDURE-POINTER"
            | "FUNCTION-POINTER"
            | "OBJECT"
    )
}

fn numeric_range(
    words: &[String],
    start: usize,
) -> Result<(Vec<String>, Option<String>), SemanticProblem> {
    let first = words
        .get(start)
        .and_then(|word| word.parse::<usize>().ok())
        .ok_or_else(|| SemanticProblem::InvalidClause("unsigned integer required".into()))?;
    let (second, cursor) = if words.get(start + 1).is_some_and(|word| word == "TO") {
        (
            words
                .get(start + 2)
                .and_then(|word| word.parse::<usize>().ok())
                .ok_or_else(|| SemanticProblem::InvalidClause("range maximum required".into()))?,
            start + 3,
        )
    } else {
        (first, start + 1)
    };
    if first > second {
        return Err(SemanticProblem::InvalidClause("descending range".into()));
    }
    Ok((
        vec![first.to_string(), second.to_string()],
        words.get(cursor).cloned(),
    ))
}

fn validate_unsigned_or_name(value: &str) -> Result<(), SemanticProblem> {
    if value.parse::<usize>().is_ok() {
        Ok(())
    } else {
        validate_name(value)
    }
}

fn validate_name(value: &str) -> Result<(), SemanticProblem> {
    let value = value.trim_matches(['\'', '"']);
    if value.is_empty()
        || value.len() > 160
        || value.starts_with('-')
        || value.ends_with('-')
        || !value.bytes().any(|byte| byte.is_ascii_alphabetic())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        Err(SemanticProblem::InvalidClause(format!(
            "invalid name {value}"
        )))
    } else {
        Ok(())
    }
}

fn clause_word(word: &str) -> bool {
    matches!(
        word,
        "BLANK"
            | "DYNAMIC"
            | "EXTERNAL"
            | "GLOBAL"
            | "JUST"
            | "JUSTIFIED"
            | "GROUP-USAGE"
            | "OCCURS"
            | "PIC"
            | "PICTURE"
            | "REDEFINES"
            | "RENAMES"
            | "SIGN"
            | "SYNC"
            | "SYNCHRONIZED"
            | "TYPEDEF"
            | "TYPE"
            | "USAGE"
            | "VALUE"
            | "VALUES"
            | "VOLATILE"
    )
}

fn file_clause_start(word: &str) -> bool {
    matches!(
        word,
        "EXTERNAL"
            | "GLOBAL"
            | "BLOCK"
            | "RECORD"
            | "LABEL"
            | "VALUE"
            | "DATA"
            | "LINAGE"
            | "RECORDING"
            | "CODE-SET"
    )
}

fn contains(words: &[String], sequence_value: &[&str]) -> bool {
    sequence(words, sequence_value).is_some()
}

fn count(words: &[String], value: &[&str]) -> usize {
    words
        .windows(value.len())
        .filter(|window| {
            window
                .iter()
                .zip(value)
                .all(|(word, expected)| word == expected)
        })
        .count()
}

fn sequence(words: &[String], value: &[&str]) -> Option<usize> {
    words.windows(value.len()).position(|window| {
        window
            .iter()
            .zip(value)
            .all(|(word, expected)| word == expected)
    })
}

fn words(sentence: &str) -> Vec<String> {
    super::declaration_words(sentence)
}

fn sentence_content(source: &str, range: Range<usize>) -> Option<(String, Range<usize>)> {
    let mut text = String::new();
    let mut first = None;
    let mut last = range.start;
    let mut offset = 0usize;
    for part in source[range.clone()].split_inclusive('\n') {
        let line = part.strip_suffix('\n').unwrap_or(part);
        let line = line.strip_suffix('\r').unwrap_or(line);
        let trimmed = line.trim();
        if !trimmed.is_empty() && !trimmed.starts_with("*>") {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(line);
            let leading = line.len() - line.trim_start().len();
            first.get_or_insert(range.start + offset + leading);
            last = range.start + offset + line.trim_end().len();
        }
        offset += part.len();
    }
    Some((text.trim().into(), first?..last))
}

fn sentence_ranges(source: &str) -> Vec<Range<usize>> {
    let bytes = source.as_bytes();
    let mut ranges = Vec::new();
    let mut start = 0usize;
    let mut quote = None;
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if cursor == 0 || bytes[cursor - 1] == b'\n' {
            let line_end = source[cursor..]
                .find('\n')
                .map_or(bytes.len(), |offset| cursor + offset);
            if source[cursor..line_end].trim_start().starts_with("*>") {
                cursor = line_end;
                continue;
            }
        }
        if quote.is_none()
            && let Some(end) = super::embedded_exec_end(source, cursor)
        {
            ranges.push(start..end);
            start = end;
            cursor = end;
            continue;
        }
        if matches!(bytes[cursor], b'\'' | b'"') {
            if quote == Some(bytes[cursor]) {
                if bytes.get(cursor + 1) == Some(&bytes[cursor]) {
                    cursor += 2;
                    continue;
                }
                quote = None;
            } else if quote.is_none() {
                quote = Some(bytes[cursor]);
            }
        } else if bytes[cursor] == b'.'
            && quote.is_none()
            && bytes
                .get(cursor + 1)
                .is_none_or(|next| next.is_ascii_whitespace())
        {
            ranges.push(start..cursor);
            start = cursor + 1;
        }
        cursor += 1;
    }
    if source[start..]
        .bytes()
        .any(|byte| !byte.is_ascii_whitespace())
    {
        ranges.push(start..source.len());
    }
    ranges
}

fn marker_positions(source: &str, marker: &str) -> Vec<usize> {
    let bytes = source.as_bytes();
    let marker_bytes = marker.as_bytes();
    let mut positions = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if source[cursor..].starts_with("*>") {
            cursor = source[cursor..]
                .find('\n')
                .map_or(bytes.len(), |offset| cursor + offset + 1);
            continue;
        }
        if matches!(bytes[cursor], b'\'' | b'"') {
            let quote = bytes[cursor];
            cursor += 1;
            while cursor < bytes.len() {
                if bytes[cursor] == quote {
                    cursor += 1;
                    if bytes.get(cursor) == Some(&quote) {
                        cursor += 1;
                        continue;
                    }
                    break;
                }
                cursor += source[cursor..].chars().next().map_or(1, char::len_utf8);
            }
            continue;
        }
        if bytes
            .get(cursor..cursor + marker_bytes.len())
            .is_some_and(|value| value.eq_ignore_ascii_case(marker_bytes))
            && cursor
                .checked_sub(1)
                .and_then(|index| bytes.get(index))
                .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'-')
            && bytes
                .get(cursor + marker_bytes.len())
                .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'-')
        {
            positions.push(cursor);
            cursor += marker_bytes.len();
        } else {
            cursor += source[cursor..].chars().next().map_or(1, char::len_utf8);
        }
    }
    positions
}

fn spans(origins: &[SourceOrigin], range: Range<usize>) -> Vec<SourceSpan> {
    origins
        .iter()
        .filter_map(|origin| {
            let start = origin.output_start.max(range.start);
            let end = origin.output_end.min(range.end);
            if start >= end {
                return None;
            }
            let exact =
                origin.output_end - origin.output_start == origin.source_end - origin.source_start;
            Some(SourceSpan {
                source: origin.source.clone(),
                source_start: if exact {
                    origin.source_start + start - origin.output_start
                } else {
                    origin.source_start
                },
                source_end: if exact {
                    origin.source_start + end - origin.output_start
                } else {
                    origin.source_end
                },
            })
        })
        .collect()
}
