mod functions;
mod structure;

pub use functions::{
    CobolIntrinsicArgument, CobolIntrinsicCall, CobolSpecialRegisterReference, IntrinsicValueType,
};
pub use structure::{
    CobolClauseKind, CobolClauseNode, CobolDataDescription, CobolDivisionKind, CobolDivisionNode,
    CobolFileDescription, CobolScope, CobolScopeId, CobolScopeKind, CobolSectionKind,
    CobolSectionNode,
};

use crate::syntax::{SourceOrigin, SourceSpan};
use crate::{IntrinsicFunctionKind, SpecialRegisterKind};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataCategory {
    Alphabetic,
    Alphanumeric,
    AlphanumericEdited,
    Dbcs,
    National,
    NationalEdited,
    Utf8,
    NumericDisplay,
    NumericEdited,
    PackedDecimal,
    Binary,
    FloatShort,
    FloatLong,
    Index,
    Pointer,
    Pointer32,
    ProcedurePointer,
    FunctionPointer,
    ObjectReference,
    Group,
    NationalGroup,
    Utf8Group,
    Condition,
    Rename,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CobolUsage {
    Display,
    Display1,
    National,
    Utf8,
    Binary,
    NativeBinary,
    PackedDecimal,
    FloatShort,
    FloatLong,
    Index,
    Pointer,
    Pointer32,
    ProcedurePointer,
    FunctionPointer,
    ObjectReference,
    Group,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CobolTableKey {
    pub name: String,
    pub descending: bool,
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
    pub usage: CobolUsage,
    pub picture: Option<String>,
    pub byte_length: Option<usize>,
    pub digits: usize,
    pub scale: usize,
    pub signed: bool,
    pub sign_leading: bool,
    pub sign_separate: bool,
    pub justified_right: bool,
    pub blank_when_zero: bool,
    pub synchronized: bool,
    pub alignment: usize,
    pub initial: Vec<u8>,
    pub alias_of: Option<String>,
    pub occurs: usize,
    pub occurs_min: usize,
    pub unbounded: bool,
    pub depending_on: Option<String>,
    pub indexes: Vec<String>,
    pub keys: Vec<CobolTableKey>,
    pub condition_values: Vec<String>,
    pub dynamic: bool,
    pub dynamic_limit: Option<usize>,
    pub external_name: Option<String>,
    pub global: bool,
    pub volatile: bool,
    pub typedef: bool,
    pub type_name: Option<String>,
    pub object_class: Option<String>,
    pub allocated: bool,
    pub source: Vec<SourceSpan>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataReference {
    pub qualified_name: String,
    pub offset: usize,
    pub length: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CobolFileBinding {
    pub select_name: String,
    pub assignment: String,
    pub record_name: Option<String>,
    pub organization: String,
    pub access_mode: String,
    pub record_key: Option<String>,
    pub alternate_record_keys: Vec<String>,
    pub relative_key: Option<String>,
    pub file_status: Option<String>,
    pub sort_merge: bool,
    pub description: String,
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
    pub files: Vec<CobolFileBinding>,
    pub divisions: Vec<CobolDivisionNode>,
    pub sections: Vec<CobolSectionNode>,
    pub scopes: Vec<CobolScope>,
    pub file_descriptions: Vec<CobolFileDescription>,
    pub data_descriptions: Vec<CobolDataDescription>,
    pub intrinsic_calls: Vec<CobolIntrinsicCall>,
    pub special_registers: Vec<CobolSpecialRegisterReference>,
    by_qualified: BTreeMap<String, usize>,
    by_simple: BTreeMap<String, Vec<usize>>,
    pub storage_bytes: usize,
}

impl SemanticModel {
    #[cfg(test)]
    pub(crate) fn analyze(
        source: &str,
        max_storage: usize,
        max_items: usize,
    ) -> Result<Self, SemanticProblem> {
        Self::analyze_with_origins(source, &[], 4, max_storage, max_items)
    }

    pub(crate) fn analyze_with_origins(
        source: &str,
        origins: &[SourceOrigin],
        pointer_bytes: usize,
        max_storage: usize,
        max_items: usize,
    ) -> Result<Self, SemanticProblem> {
        let structure = structure::analyze(source, origins, max_items)?;
        let cleaned = strip_comments(source);
        let upper = cleaned.to_ascii_uppercase();
        let program_id = extract_program_id(&upper).ok_or(SemanticProblem::MissingProgramId)?;
        let mut files = file_bindings(&cleaned)?;
        let record_names = file_record_names(&cleaned);
        for description in structure
            .file_descriptions
            .iter()
            .filter(|description| description.sort_merge)
        {
            if files
                .iter()
                .any(|binding| binding.select_name == description.name)
            {
                if let Some(binding) = files
                    .iter_mut()
                    .find(|binding| binding.select_name == description.name)
                {
                    binding.sort_merge = true;
                }
            } else {
                files.push(CobolFileBinding {
                    select_name: description.name.clone(),
                    assignment: description.name.clone(),
                    record_name: record_names.get(&description.name).cloned(),
                    organization: "SORT-MERGE".into(),
                    access_mode: "SEQUENTIAL".into(),
                    record_key: None,
                    alternate_record_keys: Vec::new(),
                    relative_key: None,
                    file_status: None,
                    sort_merge: true,
                    description: format!("SD {}", description.name),
                });
            }
        }
        files.sort_by(|left, right| left.select_name.cmp(&right.select_name));
        for description in structure
            .file_descriptions
            .iter()
            .filter(|description| !description.sort_merge)
        {
            if !files
                .iter()
                .any(|binding| binding.select_name == description.name)
            {
                return Err(SemanticProblem::InvalidDeclaration(format!(
                    "FD {} has no matching SELECT",
                    description.name
                )));
            }
        }
        let data_start = upper.find("DATA DIVISION").unwrap_or(0);
        let data_end = upper[data_start..]
            .find("PROCEDURE DIVISION")
            .map_or(cleaned.len(), |relative| data_start + relative);
        let declarations = declaration_sentences(&cleaned[data_start..data_end]);
        let specs = parse_specs(&declarations, max_items)?;
        validate_spec_constraints(&specs)?;
        let mut layouts = vec![None; specs.len()];
        let mut cursor = 0usize;
        let roots = specs
            .iter()
            .enumerate()
            .filter(|(_, spec)| spec.parent.is_none() && !matches!(spec.level, 66 | 88))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        layout_siblings(
            &roots,
            &specs,
            &mut layouts,
            &mut cursor,
            pointer_bytes,
            None,
            None,
            false,
            false,
            false,
            true,
            max_storage,
        )?;
        layout_specials(&specs, &mut layouts)?;
        let mut layouts = layouts
            .into_iter()
            .enumerate()
            .map(|(index, layout)| {
                layout.ok_or_else(|| {
                    SemanticProblem::IncompleteLayout(specs[index].qualified.clone())
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        attach_layout_sources(&specs, &mut layouts, &structure.data_descriptions)?;
        validate_layout_relationships(&specs, &layouts, upper.contains("EXEC CICS"))?;
        validate_file_layouts(&files, &structure.file_descriptions, &layouts, &cleaned)?;
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
        let user_functions = structure
            .scopes
            .iter()
            .filter(|scope| scope.kind == CobolScopeKind::Function)
            .map(|scope| scope.name.clone())
            .collect::<BTreeSet<_>>();
        let (intrinsic_calls, special_registers) = functions::analyze(
            source,
            origins,
            &layouts,
            &files,
            &user_functions,
            pointer_bytes,
        )?;
        Ok(Self {
            program_id,
            layouts,
            files,
            divisions: structure.divisions,
            sections: structure.sections,
            scopes: structure.scopes,
            file_descriptions: structure.file_descriptions,
            data_descriptions: structure.data_descriptions,
            intrinsic_calls,
            special_registers,
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
            if (!layout.unbounded && layout.occurs <= 1)
                || subscript == 0
                || (!layout.unbounded && subscript > layout.occurs)
            {
                return Err(ResolutionProblem::InvalidSubscript);
            }
            offset = offset
                .checked_add(
                    (subscript - 1)
                        .checked_mul(layout.element_length)
                        .ok_or(ResolutionProblem::InvalidSubscript)?,
                )
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

    #[must_use]
    pub fn execution_incomplete_layouts(&self) -> BTreeSet<String> {
        BTreeSet::new()
    }

    #[must_use]
    pub fn execution_incomplete_intrinsics(&self) -> BTreeSet<IntrinsicFunctionKind> {
        self.intrinsic_calls
            .iter()
            .filter(|call| !call.runtime_supported)
            .map(|call| call.kind)
            .collect()
    }

    #[must_use]
    pub fn execution_incomplete_special_registers(&self) -> BTreeSet<SpecialRegisterKind> {
        self.special_registers
            .iter()
            .filter(|register| !register.runtime_supported)
            .map(|register| register.kind)
            .collect()
    }
}

fn file_bindings(source: &str) -> Result<Vec<CobolFileBinding>, SemanticProblem> {
    let upper = source.to_ascii_uppercase();
    let Some(start) = upper.find("FILE-CONTROL") else {
        return Ok(Vec::new());
    };
    let end = upper[start..]
        .find("DATA DIVISION")
        .map_or(source.len(), |offset| start + offset);
    let record_names = file_record_names(source);
    let descriptions = file_descriptions(source);
    let mut bindings = Vec::new();
    for sentence in source[start..end].split('.') {
        let words = declaration_words(sentence);
        let Some(select) = words.iter().position(|word| word == "SELECT") else {
            continue;
        };
        let select_name = words
            .get(select + 1)
            .cloned()
            .ok_or_else(|| SemanticProblem::InvalidDeclaration(sentence.into()))?;
        let assignment = find_after_owned(&words, "ASSIGN")
            .and_then(|value| (value != "TO").then_some(value))
            .or_else(|| {
                words
                    .iter()
                    .position(|word| word == "ASSIGN")
                    .and_then(|index| words.get(index + 2).cloned())
            })
            .ok_or_else(|| SemanticProblem::InvalidDeclaration(sentence.into()))?;
        let organization = find_after_owned(&words, "ORGANIZATION")
            .filter(|value| value != "IS")
            .or_else(|| word_after_optional_is(&words, "ORGANIZATION"))
            .unwrap_or_else(|| "SEQUENTIAL".into());
        let access_mode = words
            .iter()
            .position(|word| word == "ACCESS")
            .and_then(|index| {
                words[index + 1..]
                    .iter()
                    .position(|word| word == "MODE")
                    .map(|offset| index + 1 + offset)
            })
            .and_then(|index| {
                words
                    .get(index + 1 + usize::from(words.get(index + 1).is_some_and(|v| v == "IS")))
                    .cloned()
            })
            .unwrap_or_else(|| "SEQUENTIAL".into());
        bindings.push(CobolFileBinding {
            record_name: record_names.get(&select_name).cloned(),
            select_name: select_name.clone(),
            assignment,
            organization,
            access_mode,
            record_key: word_after_optional_is(&words, "KEY"),
            alternate_record_keys: words
                .windows(3)
                .enumerate()
                .filter(|(_, window)| {
                    window[0] == "ALTERNATE" && window[1] == "RECORD" && window[2] == "KEY"
                })
                .filter_map(|(index, _)| {
                    let value = index + 3;
                    words
                        .get(value + usize::from(words.get(value).is_some_and(|word| word == "IS")))
                        .cloned()
                })
                .collect(),
            relative_key: words
                .windows(2)
                .position(|pair| pair[0] == "RELATIVE" && pair[1] == "KEY")
                .and_then(|index| {
                    words.get(
                        index
                            + 2
                            + usize::from(words.get(index + 2).is_some_and(|word| word == "IS")),
                    )
                })
                .cloned(),
            file_status: words
                .iter()
                .position(|word| word == "FILE")
                .and_then(|index| words.get(index + 1).filter(|word| *word == "STATUS"))
                .and_then(|_| word_after_optional_is(&words, "STATUS")),
            sort_merge: false,
            description: descriptions.get(&select_name).cloned().unwrap_or_default(),
        });
    }
    bindings.sort_by(|left, right| left.select_name.cmp(&right.select_name));
    Ok(bindings)
}

fn file_descriptions(source: &str) -> BTreeMap<String, String> {
    let upper = source.to_ascii_uppercase();
    let Some(start) = upper.find("FILE SECTION") else {
        return BTreeMap::new();
    };
    let end = [
        "WORKING-STORAGE SECTION",
        "LOCAL-STORAGE SECTION",
        "LINKAGE SECTION",
        "PROCEDURE DIVISION",
    ]
    .into_iter()
    .filter_map(|marker| upper[start..].find(marker).map(|offset| start + offset))
    .min()
    .unwrap_or(source.len());
    source[start..end]
        .split('.')
        .filter_map(|sentence| {
            let words = declaration_words(sentence);
            let index = words
                .iter()
                .position(|word| matches!(word.as_str(), "FD" | "SD"))?;
            let name = words.get(index + 1)?.clone();
            Some((name, words[index..].join(" ")))
        })
        .collect()
}

fn file_record_names(source: &str) -> BTreeMap<String, String> {
    let upper = source.to_ascii_uppercase();
    let Some(start) = upper.find("FILE SECTION") else {
        return BTreeMap::new();
    };
    let end = [
        "WORKING-STORAGE SECTION",
        "LOCAL-STORAGE SECTION",
        "LINKAGE SECTION",
    ]
    .into_iter()
    .filter_map(|marker| upper[start..].find(marker).map(|offset| start + offset))
    .min()
    .unwrap_or(source.len());
    let mut current = None;
    let mut records = BTreeMap::new();
    for sentence in source[start..end].split('.') {
        let words = declaration_words(sentence);
        if let Some(index) = words
            .iter()
            .position(|word| matches!(word.as_str(), "FD" | "SD"))
        {
            current = words.get(index + 1).cloned();
            continue;
        }
        if let Some(file) = current.take()
            && let Some(index) = words.iter().position(|word| word == "01")
            && let Some(record) = words.get(index + 1)
        {
            records.insert(file, record.clone());
        }
    }
    records
}

fn word_after_optional_is(words: &[String], keyword: &str) -> Option<String> {
    let index = words.iter().position(|word| word == keyword)?;
    words
        .get(index + 1 + usize::from(words.get(index + 1).is_some_and(|word| word == "IS")))
        .cloned()
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
    unbounded: bool,
    depending_on: Option<String>,
    indexes: Vec<String>,
    keys: Vec<CobolTableKey>,
    typedef: bool,
    type_template: Option<usize>,
    from_type: bool,
    source_template: Option<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct OccursSpec {
    minimum: usize,
    maximum: usize,
    unbounded: bool,
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
        let words = declaration_words(sentence);
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
        let type_template = type_target(&words)
            .map(|target| {
                specs
                    .iter()
                    .enumerate()
                    .rev()
                    .find(|(_, spec)| spec.typedef && spec.name == target)
                    .map(|(index, _)| index)
                    .ok_or_else(|| {
                        SemanticProblem::InvalidDeclaration(format!(
                            "TYPE target {target} is not a prior TYPEDEF"
                        ))
                    })
            })
            .transpose()?;
        let own_occurs = occurs_range(&words)?;
        let occurs = if words.iter().any(|word| word == "OCCURS") {
            own_occurs
        } else {
            type_template.map_or(own_occurs, |template| OccursSpec {
                minimum: specs[template].occurs_min,
                maximum: specs[template].occurs_max,
                unbounded: specs[template].unbounded,
            })
        };
        let indexes = values_after(&words, "INDEXED", "BY");
        let indexes = if indexes.is_empty() {
            type_template.map_or_else(Vec::new, |template| specs[template].indexes.clone())
        } else {
            indexes
        };
        let keys = table_keys(&words)?;
        let keys = if keys.is_empty() {
            type_template.map_or_else(Vec::new, |template| specs[template].keys.clone())
        } else {
            keys
        };
        let spec = DataSpec {
            level,
            name,
            qualified,
            parent,
            children: Vec::new(),
            section,
            redefines: find_after_owned(&words, "REDEFINES"),
            depending_on: qualified_reference_after(&words, &["DEPENDING", "ON"]),
            indexes,
            keys,
            typedef: words.iter().any(|word| word == "TYPEDEF"),
            type_template,
            from_type: false,
            source_template: None,
            words,
            sentence: sentence.clone(),
            occurs_min: occurs.minimum,
            occurs_max: occurs.maximum,
            unbounded: occurs.unbounded,
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
        if let Some(template) = type_template {
            clone_type_children(&mut specs, template, index, max_items)?;
        }
    }
    validate_type_children(&specs)?;
    Ok(specs)
}

fn type_target(words: &[String]) -> Option<String> {
    let index = words.iter().position(|word| word == "TYPE")?;
    words
        .get(index + 1 + usize::from(words.get(index + 1).is_some_and(|word| word == "TO")))
        .cloned()
}

fn table_keys(words: &[String]) -> Result<Vec<CobolTableKey>, SemanticProblem> {
    let mut keys = Vec::new();
    let mut cursor = 0usize;
    while cursor < words.len() {
        let descending = match words[cursor].as_str() {
            "ASCENDING" => false,
            "DESCENDING" => true,
            _ => {
                cursor += 1;
                continue;
            }
        };
        if words.get(cursor + 1).is_none_or(|word| word != "KEY") {
            return Err(SemanticProblem::InvalidOccurs);
        }
        cursor += 2 + usize::from(words.get(cursor + 2).is_some_and(|word| word == "IS"));
        let start = keys.len();
        while let Some(name) = words.get(cursor) {
            if matches!(
                name.as_str(),
                "ASCENDING"
                    | "DESCENDING"
                    | "INDEXED"
                    | "DEPENDING"
                    | "PIC"
                    | "PICTURE"
                    | "VALUE"
                    | "VALUES"
                    | "REDEFINES"
                    | "TYPE"
                    | "TYPEDEF"
                    | "USAGE"
            ) || usage_word(name).is_some()
            {
                break;
            }
            if matches!(name.as_str(), "OF" | "IN") {
                return Err(SemanticProblem::InvalidOccurs);
            }
            let mut reference = name.clone();
            cursor += 1;
            while words
                .get(cursor)
                .is_some_and(|word| matches!(word.as_str(), "OF" | "IN"))
            {
                let qualifier = words
                    .get(cursor + 1)
                    .filter(|qualifier| {
                        !matches!(
                            qualifier.as_str(),
                            "ASCENDING"
                                | "DESCENDING"
                                | "INDEXED"
                                | "DEPENDING"
                                | "PIC"
                                | "PICTURE"
                                | "VALUE"
                                | "VALUES"
                                | "REDEFINES"
                                | "TYPE"
                                | "TYPEDEF"
                                | "USAGE"
                        )
                    })
                    .ok_or(SemanticProblem::InvalidOccurs)?;
                reference.push_str(" OF ");
                reference.push_str(qualifier);
                cursor += 2;
            }
            keys.push(CobolTableKey {
                name: reference,
                descending,
            });
        }
        if keys.len() == start {
            return Err(SemanticProblem::InvalidOccurs);
        }
    }
    if keys.len() > 12 {
        return Err(SemanticProblem::InvalidOccurs);
    }
    Ok(keys)
}

fn clone_type_children(
    specs: &mut Vec<DataSpec>,
    template: usize,
    instance: usize,
    max_items: usize,
) -> Result<(), SemanticProblem> {
    let children = specs
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.parent == Some(template))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    for child in children {
        if specs.len() >= max_items {
            return Err(SemanticProblem::ItemLimitExceeded);
        }
        let mut cloned = specs[child].clone();
        let level =
            if matches!(cloned.level, 66 | 88) {
                cloned.level
            } else {
                instance_level(specs, instance)?
                    .checked_add(cloned.level.checked_sub(specs[template].level).ok_or_else(
                        || SemanticProblem::InvalidDeclaration(cloned.sentence.clone()),
                    )?)
                    .filter(|level| *level <= 49)
                    .ok_or_else(|| SemanticProblem::InvalidDeclaration(cloned.sentence.clone()))?
            };
        let component = cloned
            .qualified
            .rsplit('.')
            .next()
            .unwrap_or(&cloned.name)
            .to_string();
        cloned.level = level;
        cloned.parent = Some(instance);
        cloned.qualified = format!("{}.{}", specs[instance].qualified, component);
        cloned.children.clear();
        cloned.typedef = false;
        cloned.from_type = true;
        cloned.source_template = Some(child);
        let cloned_index = specs.len();
        specs.push(cloned);
        if !matches!(level, 66 | 88) {
            specs[instance].children.push(cloned_index);
        }
        clone_type_children(specs, child, cloned_index, max_items)?;
    }
    Ok(())
}

fn instance_level(specs: &[DataSpec], instance: usize) -> Result<u8, SemanticProblem> {
    specs
        .get(instance)
        .map(|spec| spec.level)
        .ok_or_else(|| SemanticProblem::InvalidDeclaration("missing TYPE instance".into()))
}

fn validate_type_children(specs: &[DataSpec]) -> Result<(), SemanticProblem> {
    for (index, spec) in specs
        .iter()
        .enumerate()
        .filter(|(_, spec)| spec.type_template.is_some())
    {
        if specs
            .iter()
            .any(|candidate| candidate.parent == Some(index) && !candidate.from_type)
        {
            return Err(SemanticProblem::InvalidDeclaration(format!(
                "TYPE instance {} has explicit subordinate entries",
                spec.name
            )));
        }
        if spec.level == 77
            && specs
                .iter()
                .any(|candidate| candidate.parent == spec.type_template)
        {
            return Err(SemanticProblem::InvalidDeclaration(format!(
                "level 77 TYPE instance {} requires an elementary type",
                spec.name
            )));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn layout_siblings(
    siblings: &[usize],
    specs: &[DataSpec],
    layouts: &mut [Option<CobolLayout>],
    cursor: &mut usize,
    pointer_bytes: usize,
    inherited_usage: Option<CobolUsage>,
    inherited_external: Option<String>,
    inherited_global: bool,
    inherited_volatile: bool,
    force_synchronized: bool,
    allocated: bool,
    max_storage: usize,
) -> Result<(), SemanticProblem> {
    for index in siblings {
        if specs[*index].typedef {
            let mut template_cursor = 0usize;
            layout_one(
                *index,
                specs,
                layouts,
                &mut template_cursor,
                pointer_bytes,
                inherited_usage,
                inherited_external.clone(),
                inherited_global,
                inherited_volatile,
                force_synchronized,
                false,
                max_storage,
            )?;
        } else {
            layout_one(
                *index,
                specs,
                layouts,
                cursor,
                pointer_bytes,
                inherited_usage,
                inherited_external.clone(),
                inherited_global,
                inherited_volatile,
                force_synchronized,
                allocated,
                max_storage,
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn layout_one(
    index: usize,
    specs: &[DataSpec],
    layouts: &mut [Option<CobolLayout>],
    cursor: &mut usize,
    pointer_bytes: usize,
    inherited_usage: Option<CobolUsage>,
    inherited_external: Option<String>,
    inherited_global: bool,
    inherited_volatile: bool,
    force_synchronized: bool,
    allocated: bool,
    max_storage: usize,
) -> Result<(), SemanticProblem> {
    let spec = &specs[index];
    let description = effective_description(spec, specs);
    let words = &description.words;
    let is_group = !spec.children.is_empty();
    let picture = picture(words, is_group, inherited_usage, pointer_bytes)?;
    validate_elementary_clauses(spec, words, &picture, is_group)?;
    let own_synchronized = contains_word(words, "SYNC") || contains_word(words, "SYNCHRONIZED");
    let synchronized = own_synchronized || force_synchronized;
    let target = spec
        .redefines
        .as_deref()
        .map(|name| resolve_sibling_layout(name, spec.parent, specs, layouts))
        .transpose()?
        .map(|layout| (layout.offset, layout.qualified_name.clone()));
    let mut alignment = natural_alignment(picture.usage, picture.length, synchronized);
    if allocated && spec.level == 1 && spec.section == StorageSection::Working && target.is_none() {
        alignment = alignment.max(8);
    }
    let start = target
        .as_ref()
        .map_or_else(|| align_up(*cursor, alignment), |(offset, _)| *offset);
    if target.is_some() && start % alignment != 0 {
        return Err(SemanticProblem::InvalidRedefines(format!(
            "{} does not satisfy alignment {alignment}",
            spec.name
        )));
    }
    let own_external = external_name(&spec.words, &spec.name)?;
    let external = own_external.or(inherited_external);
    let global = inherited_global || contains_word(&spec.words, "GLOBAL");
    let volatile = inherited_volatile || contains_word(&spec.words, "VOLATILE");
    let child_usage = match picture.usage {
        CobolUsage::Group => inherited_usage,
        usage => Some(usage),
    };
    let mut child_cursor = start;
    if is_group {
        layout_siblings(
            &spec.children,
            specs,
            layouts,
            &mut child_cursor,
            pointer_bytes,
            child_usage,
            external.clone(),
            global,
            volatile,
            synchronized,
            allocated,
            max_storage,
        )?;
        alignment = alignment.max(
            spec.children
                .iter()
                .filter_map(|child| layouts[*child].as_ref())
                .map(|layout| layout.alignment)
                .max()
                .unwrap_or(1),
        );
    }
    let mut element_length = if is_group {
        child_cursor.saturating_sub(start).max(picture.length)
    } else {
        picture.length
    };
    if is_group && (spec.occurs_max > 1 || spec.unbounded) {
        element_length = align_up(element_length, alignment);
    }
    let dynamic = contains_word(words, "DYNAMIC");
    let dynamic_limit = dynamic.then(|| dynamic_limit(words)).transpose()?.flatten();
    let length = if dynamic || spec.unbounded {
        0
    } else {
        element_length
            .checked_mul(spec.occurs_max)
            .ok_or(SemanticProblem::StorageLimitExceeded)?
    };
    let end = start
        .checked_add(length)
        .ok_or(SemanticProblem::StorageLimitExceeded)?;
    if allocated && end > max_storage {
        return Err(SemanticProblem::StorageLimitExceeded);
    }
    let value_source =
        if contains_word(&spec.words, "VALUE") || contains_word(&spec.words, "VALUES") {
            spec
        } else {
            description
        };
    let initial = if dynamic || spec.unbounded {
        Vec::new()
    } else if !is_group {
        if spec.section == StorageSection::Linkage
            && keyword_index(&value_source.sentence, "VALUE").is_none()
        {
            vec![0; length]
        } else {
            initial_value(
                &value_source.sentence,
                &value_source.words,
                element_length,
                picture.category,
            )
            .repeat(spec.occurs_max)
        }
    } else {
        group_initial(
            index,
            start,
            element_length,
            spec.occurs_max,
            specs,
            layouts,
        )
    };
    let blank_when_zero = contains_sequence(words, &["BLANK", "WHEN"]);
    let category = if blank_when_zero && picture.category == DataCategory::NumericDisplay {
        DataCategory::NumericEdited
    } else {
        picture.category
    };
    layouts[index] = Some(CobolLayout {
        name: spec.name.clone(),
        qualified_name: spec.qualified.clone(),
        level: spec.level,
        section: spec.section,
        parent: spec.parent.map(|parent| specs[parent].qualified.clone()),
        offset: start,
        length,
        element_length,
        category,
        usage: picture.usage,
        picture: picture.picture,
        byte_length: picture.byte_length,
        digits: usize::from(!is_group) * picture.digits,
        scale: usize::from(!is_group) * picture.scale,
        signed: !is_group && picture.signed,
        sign_leading: !is_group && sign_leading(words),
        sign_separate: !is_group && picture.sign_separate,
        justified_right: !is_group
            && (contains_sequence(words, &["JUST", "RIGHT"])
                || contains_sequence(words, &["JUSTIFIED", "RIGHT"])
                || contains_word(words, "JUSTIFIED")),
        blank_when_zero,
        synchronized,
        alignment,
        initial,
        alias_of: target.map(|(_, qualified)| qualified),
        occurs: spec.occurs_max,
        occurs_min: spec.occurs_min,
        unbounded: spec.unbounded,
        depending_on: spec
            .depending_on
            .clone()
            .or_else(|| description.depending_on.clone()),
        indexes: spec.indexes.clone(),
        keys: spec.keys.clone(),
        condition_values: Vec::new(),
        dynamic,
        dynamic_limit,
        external_name: external,
        global,
        volatile,
        typedef: is_type_definition(index, specs),
        type_name: type_target(&spec.words),
        object_class: picture.object_class,
        allocated,
        source: Vec::new(),
    });
    *cursor = (*cursor).max(end);
    Ok(())
}

fn effective_description<'a>(spec: &'a DataSpec, specs: &'a [DataSpec]) -> &'a DataSpec {
    spec.type_template
        .and_then(|template| specs.get(template))
        .map_or(spec, |template| effective_description(template, specs))
}

fn is_type_definition(index: usize, specs: &[DataSpec]) -> bool {
    let mut current = Some(index);
    while let Some(index) = current {
        if specs[index].typedef {
            return true;
        }
        current = specs[index].parent;
    }
    false
}

fn contains_word(words: &[String], expected: &str) -> bool {
    words.iter().any(|word| word == expected)
}

fn contains_sequence(words: &[String], sequence: &[&str]) -> bool {
    words.windows(sequence.len()).any(|window| {
        window
            .iter()
            .zip(sequence)
            .all(|(word, expected)| word == expected)
    })
}

fn external_name(words: &[String], default: &str) -> Result<Option<String>, SemanticProblem> {
    let Some(index) = words.iter().position(|word| word == "EXTERNAL") else {
        return Ok(None);
    };
    let name = if words.get(index + 1).is_some_and(|word| word == "AS") {
        words
            .get(index + 2)
            .map(|name| name.trim_matches(['\'', '"']).to_string())
            .filter(|name| !name.is_empty())
            .ok_or_else(|| {
                SemanticProblem::InvalidDeclaration("EXTERNAL AS requires a name".into())
            })?
    } else {
        default.to_string()
    };
    Ok(Some(name))
}

fn dynamic_limit(words: &[String]) -> Result<Option<usize>, SemanticProblem> {
    let Some(index) = words.iter().position(|word| word == "LIMIT") else {
        return Ok(None);
    };
    words
        .get(index + 1 + usize::from(words.get(index + 1).is_some_and(|word| word == "IS")))
        .and_then(|word| word.parse::<usize>().ok())
        .filter(|value| (1..=999_999_999).contains(value))
        .map(Some)
        .ok_or_else(|| SemanticProblem::InvalidDeclaration("invalid DYNAMIC LIMIT".into()))
}

fn sign_leading(words: &[String]) -> bool {
    words
        .iter()
        .position(|word| word == "SIGN")
        .is_some_and(|index| {
            words[index + 1..]
                .iter()
                .take(2)
                .any(|word| word == "LEADING")
        })
}

fn natural_alignment(usage: CobolUsage, length: usize, synchronized: bool) -> usize {
    if !synchronized {
        return 1;
    }
    match usage {
        CobolUsage::Binary | CobolUsage::NativeBinary => length.clamp(1, 4),
        CobolUsage::FloatShort => 4,
        CobolUsage::FloatLong | CobolUsage::ProcedurePointer => 8,
        CobolUsage::Index
        | CobolUsage::Pointer
        | CobolUsage::Pointer32
        | CobolUsage::FunctionPointer
        | CobolUsage::ObjectReference => length.max(1),
        _ => 1,
    }
}

fn align_up(value: usize, alignment: usize) -> usize {
    let remainder = value % alignment.max(1);
    if remainder == 0 {
        value
    } else {
        value.saturating_add(alignment - remainder)
    }
}

fn validate_elementary_clauses(
    spec: &DataSpec,
    words: &[String],
    picture: &PictureSpec,
    is_group: bool,
) -> Result<(), SemanticProblem> {
    if contains_word(words, "DYNAMIC")
        && (is_group
            || !matches!(
                picture.category,
                DataCategory::Alphanumeric | DataCategory::Utf8
            )
            || picture
                .picture
                .as_deref()
                .is_none_or(|pic| !matches!(pic, "X" | "U"))
            || spec.unbounded)
    {
        return Err(SemanticProblem::InvalidUsage(
            "DYNAMIC requires an elementary PIC X or PIC U item".into(),
        ));
    }
    if contains_sequence(words, &["BLANK", "WHEN"])
        && (!matches!(
            picture.category,
            DataCategory::NumericDisplay
                | DataCategory::NumericEdited
                | DataCategory::National
                | DataCategory::NationalEdited
        ) || picture.signed
            || picture
                .picture
                .as_deref()
                .is_some_and(|value| value.contains('*')))
    {
        return Err(SemanticProblem::InvalidUsage(
            "BLANK WHEN ZERO requires unsigned DISPLAY/NATIONAL numeric data".into(),
        ));
    }
    if (contains_word(words, "JUST") || contains_word(words, "JUSTIFIED"))
        && !matches!(
            picture.category,
            DataCategory::Alphabetic
                | DataCategory::Alphanumeric
                | DataCategory::Dbcs
                | DataCategory::National
        )
    {
        return Err(SemanticProblem::InvalidUsage(
            "JUSTIFIED is incompatible with the data category".into(),
        ));
    }
    let values = values_clause(&spec.words);
    if matches!(
        picture.usage,
        CobolUsage::Pointer
            | CobolUsage::Pointer32
            | CobolUsage::ProcedurePointer
            | CobolUsage::FunctionPointer
            | CobolUsage::ObjectReference
    ) && values
        .iter()
        .any(|value| !matches!(value.as_str(), "IS" | "NULL" | "NULLS"))
    {
        return Err(SemanticProblem::InvalidUsage(
            "pointer and object VALUE must be NULL".into(),
        ));
    }
    if picture.usage == CobolUsage::Index && !values.is_empty() {
        return Err(SemanticProblem::InvalidUsage(
            "INDEX items cannot have VALUE".into(),
        ));
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
                usage: target.usage,
                picture: None,
                byte_length: None,
                digits: 0,
                scale: 0,
                signed: false,
                sign_leading: false,
                sign_separate: false,
                justified_right: false,
                blank_when_zero: false,
                synchronized: false,
                alignment: 1,
                initial: Vec::new(),
                alias_of: Some(target.qualified_name.clone()),
                occurs: 1,
                occurs_min: 1,
                unbounded: false,
                depending_on: None,
                indexes: Vec::new(),
                keys: Vec::new(),
                condition_values: values_clause(&spec.words),
                dynamic: false,
                dynamic_limit: None,
                external_name: target.external_name.clone(),
                global: target.global,
                volatile: target.volatile,
                typedef: is_type_definition(index, specs),
                type_name: None,
                object_class: None,
                allocated: false,
                source: Vec::new(),
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
            if end.offset < start.offset
                || end.offset.saturating_add(end.length) < start.offset.saturating_add(start.length)
            {
                return Err(SemanticProblem::InvalidRename);
            }
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
                usage: CobolUsage::Group,
                picture: None,
                byte_length: None,
                digits: 0,
                scale: 0,
                signed: false,
                sign_leading: false,
                sign_separate: false,
                justified_right: false,
                blank_when_zero: false,
                synchronized: false,
                alignment: 1,
                initial: Vec::new(),
                alias_of: Some(start.qualified_name.clone()),
                occurs: 1,
                occurs_min: 1,
                unbounded: false,
                depending_on: None,
                indexes: Vec::new(),
                keys: Vec::new(),
                condition_values: Vec::new(),
                dynamic: false,
                dynamic_limit: None,
                external_name: start.external_name.clone(),
                global: start.global,
                volatile: start.volatile,
                typedef: is_type_definition(index, specs),
                type_name: None,
                object_class: None,
                allocated: false,
                source: Vec::new(),
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
                usage: CobolUsage::Display,
                picture: None,
                byte_length: None,
                digits: 0,
                scale: 0,
                signed: false,
                sign_leading: false,
                sign_separate: false,
                justified_right: false,
                blank_when_zero: false,
                synchronized: false,
                alignment: 1,
                initial: Vec::new(),
                alias_of: None,
                occurs: 1,
                occurs_min: 1,
                unbounded: false,
                depending_on: None,
                indexes: Vec::new(),
                keys: Vec::new(),
                condition_values: values_clause(&spec.words),
                dynamic: false,
                dynamic_limit: None,
                external_name: None,
                global: false,
                volatile: false,
                typedef: false,
                type_name: None,
                object_class: None,
                allocated: false,
                source: Vec::new(),
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
        .find(|(index, candidate)| {
            candidate.section == spec.section
                && candidate.name.eq_ignore_ascii_case(name)
                && !matches!(candidate.level, 1 | 66 | 77 | 88)
                && spec
                    .parent
                    .is_some_and(|root| is_descendant_of(*index, root, specs))
                && !has_occurs_through(*index, spec.parent, specs)
                && !is_type_definition(*index, specs)
        })
        .and_then(|(index, _)| layouts[index].as_ref())
        .ok_or(SemanticProblem::InvalidRename)
}

fn is_descendant_of(mut index: usize, ancestor: usize, specs: &[DataSpec]) -> bool {
    while let Some(parent) = specs[index].parent {
        if parent == ancestor {
            return true;
        }
        index = parent;
    }
    false
}

fn has_occurs_through(mut index: usize, stop: Option<usize>, specs: &[DataSpec]) -> bool {
    loop {
        if specs[index].occurs_max != 1 || specs[index].unbounded {
            return true;
        }
        let Some(parent) = specs[index].parent else {
            return false;
        };
        if Some(parent) == stop {
            return false;
        }
        index = parent;
    }
}

fn validate_spec_constraints(specs: &[DataSpec]) -> Result<(), SemanticProblem> {
    for (index, spec) in specs.iter().enumerate() {
        let has_occurs = spec.occurs_min != 1 || spec.occurs_max != 1 || spec.unbounded;
        if has_occurs {
            if matches!(spec.level, 1 | 66 | 77 | 78 | 88)
                || (spec.redefines.is_some()
                    && (spec.occurs_min != spec.occurs_max || spec.unbounded))
                || (spec.occurs_min != spec.occurs_max && spec.depending_on.is_none())
                || (spec.unbounded && spec.depending_on.is_none())
            {
                return Err(SemanticProblem::InvalidOccurs);
            }
            let mut dimensions = 1usize;
            let mut parent = spec.parent;
            while let Some(ancestor) = parent {
                let candidate = &specs[ancestor];
                if candidate.occurs_max != 1 || candidate.unbounded {
                    dimensions += 1;
                }
                parent = candidate.parent;
            }
            if dimensions > 7
                || spec.indexes.len() > 12
                || spec.indexes.iter().collect::<BTreeSet<_>>().len() != spec.indexes.len()
            {
                return Err(SemanticProblem::InvalidOccurs);
            }
        } else if !spec.indexes.is_empty() || !spec.keys.is_empty() || spec.depending_on.is_some() {
            return Err(SemanticProblem::InvalidOccurs);
        }
        if contains_word(&spec.words, "DYNAMIC") {
            if spec.section == StorageSection::File && !matches!(spec.level, 1 | 77) {
                return Err(SemanticProblem::InvalidUsage(
                    "FILE SECTION dynamic item must be level 01 or 77".into(),
                ));
            }
            let mut parent = spec.parent;
            while let Some(ancestor) = parent {
                if specs[ancestor].occurs_min != specs[ancestor].occurs_max
                    || specs[ancestor].unbounded
                {
                    return Err(SemanticProblem::InvalidUsage(
                        "dynamic item cannot be subordinate to a variable table".into(),
                    ));
                }
                parent = specs[ancestor].parent;
            }
        }
        if let Some(target_name) = &spec.redefines {
            if contains_word(&spec.words, "EXTERNAL")
                || contains_word(&spec.words, "VALUE")
                || specs.iter().any(|candidate| {
                    candidate.parent == Some(index)
                        && candidate.level != 88
                        && contains_word(&candidate.words, "VALUE")
                })
            {
                return Err(SemanticProblem::InvalidRedefines(spec.name.clone()));
            }
            let target = specs[..index]
                .iter()
                .rev()
                .find(|target| target.parent == spec.parent && target.name == *target_name)
                .ok_or_else(|| SemanticProblem::UnknownRedefines(target_name.clone()))?;
            if target.occurs_max != 1 || target.unbounded || target.typedef {
                return Err(SemanticProblem::InvalidRedefines(spec.name.clone()));
            }
        }
    }
    Ok(())
}

fn attach_layout_sources(
    specs: &[DataSpec],
    layouts: &mut [CobolLayout],
    descriptions: &[CobolDataDescription],
) -> Result<(), SemanticProblem> {
    let originals = specs
        .iter()
        .enumerate()
        .filter(|(_, spec)| !spec.from_type)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if originals.len() != descriptions.len() {
        return Err(SemanticProblem::InvalidDeclaration(
            "semantic structure/layout declaration closure drifted".into(),
        ));
    }
    let mut sources = BTreeMap::new();
    for (index, description) in originals.into_iter().zip(descriptions) {
        sources.insert(index, description.source.clone());
    }
    for (index, layout) in layouts.iter_mut().enumerate() {
        let mut spans = sources.get(&index).cloned().unwrap_or_default();
        if let Some(template) = specs[index].type_template {
            for span in sources.get(&template).into_iter().flatten() {
                if !spans.contains(span) {
                    spans.push(span.clone());
                }
            }
        }
        if let Some(template) = specs[index].source_template {
            for span in sources.get(&template).into_iter().flatten() {
                if !spans.contains(span) {
                    spans.push(span.clone());
                }
            }
            let mut parent = specs[index].parent;
            while let Some(ancestor) = parent {
                if let Some(source) = sources.get(&ancestor) {
                    for span in source {
                        if !spans.contains(span) {
                            spans.push(span.clone());
                        }
                    }
                    break;
                }
                parent = specs[ancestor].parent;
            }
        }
        layout.source = spans;
    }
    Ok(())
}

fn validate_layout_relationships(
    specs: &[DataSpec],
    layouts: &[CobolLayout],
    cics_context: bool,
) -> Result<(), SemanticProblem> {
    for (index, spec) in specs.iter().enumerate() {
        if let Some(name) = &layouts[index].depending_on
            && !(cics_context && name.eq_ignore_ascii_case("EIBCALEN"))
        {
            let target = resolve_layout_name(name, spec, specs, layouts)?;
            if !matches!(
                target.category,
                DataCategory::NumericDisplay | DataCategory::PackedDecimal | DataCategory::Binary
            ) || target.scale != 0
                || target.dynamic
                || target
                    .qualified_name
                    .starts_with(&format!("{}.", spec.qualified))
            {
                return Err(SemanticProblem::InvalidOccurs);
            }
        }
        for key in &layouts[index].keys {
            let target = resolve_layout_name(&key.name, spec, specs, layouts)?;
            if target.qualified_name != spec.qualified
                && !target
                    .qualified_name
                    .starts_with(&format!("{}.", spec.qualified))
            {
                return Err(SemanticProblem::InvalidOccurs);
            }
        }
        if let Some(alias) = &layouts[index].alias_of {
            let target = layouts
                .iter()
                .find(|layout| layout.qualified_name == *alias)
                .ok_or_else(|| SemanticProblem::UnknownRedefines(alias.clone()))?;
            if target.external_name.is_some() && layouts[index].length > target.length {
                return Err(SemanticProblem::InvalidRedefines(
                    layouts[index].name.clone(),
                ));
            }
        }
    }
    Ok(())
}

fn resolve_layout_name<'a>(
    name: &str,
    spec: &DataSpec,
    specs: &[DataSpec],
    layouts: &'a [CobolLayout],
) -> Result<&'a CobolLayout, SemanticProblem> {
    let (normalized, explicitly_qualified) = normalize_data_reference(name)?;
    if explicitly_qualified {
        return specs
            .iter()
            .position(|candidate| candidate.qualified == normalized)
            .and_then(|index| layouts.get(index))
            .ok_or(SemanticProblem::InvalidReference(normalized));
    }
    let candidates = specs
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.name == normalized)
        .filter_map(|(index, candidate)| layouts.get(index).map(|layout| (candidate, layout)))
        .collect::<Vec<_>>();
    match candidates.as_slice() {
        [(_, layout)] => Ok(*layout),
        [] => Err(SemanticProblem::InvalidReference(normalized)),
        _ => {
            let owner = spec.qualified.split('.').collect::<Vec<_>>();
            let mut ranked = candidates
                .into_iter()
                .map(|(candidate, layout)| {
                    let proximity = owner
                        .iter()
                        .zip(candidate.qualified.split('.'))
                        .take_while(|(left, right)| **left == *right)
                        .count();
                    (proximity, layout)
                })
                .collect::<Vec<_>>();
            ranked.sort_by_key(|(proximity, _)| std::cmp::Reverse(*proximity));
            match ranked.as_slice() {
                [(best, layout), rest @ ..] if rest.first().is_none_or(|(next, _)| next < best) => {
                    Ok(*layout)
                }
                _ => Err(SemanticProblem::InvalidReference(format!(
                    "ambiguous {normalized}"
                ))),
            }
        }
    }
}

fn normalize_data_reference(name: &str) -> Result<(String, bool), SemanticProblem> {
    let upper = name.trim().to_ascii_uppercase();
    if upper.contains('.') {
        return Ok((upper, true));
    }
    let words = upper.split_whitespace().collect::<Vec<_>>();
    let Some(simple) = words.first() else {
        return Err(SemanticProblem::InvalidReference(upper));
    };
    if words.len() == 1 {
        return Ok(((*simple).to_string(), false));
    }
    if words.len().is_multiple_of(2)
        || !words
            .iter()
            .skip(1)
            .step_by(2)
            .all(|word| matches!(*word, "OF" | "IN"))
    {
        return Err(SemanticProblem::InvalidReference(upper));
    }
    let mut components = words.iter().skip(2).step_by(2).copied().collect::<Vec<_>>();
    components.reverse();
    components.push(simple);
    Ok((components.join("."), true))
}

fn validate_file_layouts(
    files: &[CobolFileBinding],
    descriptions: &[CobolFileDescription],
    layouts: &[CobolLayout],
    source: &str,
) -> Result<(), SemanticProblem> {
    for file in files {
        let Some(record_name) = &file.record_name else {
            continue;
        };
        let record = layouts
            .iter()
            .find(|layout| layout.name == *record_name && layout.section == StorageSection::File)
            .ok_or_else(|| SemanticProblem::InvalidFileLayout(record_name.clone()))?;
        if let Some(description) = descriptions
            .iter()
            .find(|entry| entry.name == file.select_name)
        {
            for clause in &description.clauses {
                if let CobolClauseKind::File(crate::FileDescriptionClauseKind::Record) = clause.kind
                    && let [minimum, maximum, ..] = clause.operands.as_slice()
                    && let (Ok(minimum), Ok(maximum)) =
                        (minimum.parse::<usize>(), maximum.parse::<usize>())
                    && !(minimum..=maximum).contains(&record.length)
                {
                    return Err(SemanticProblem::InvalidFileLayout(record_name.clone()));
                }
                if let CobolClauseKind::File(crate::FileDescriptionClauseKind::DataRecords) =
                    clause.kind
                    && clause.operands.iter().any(|name| {
                        !layouts.iter().any(|layout| {
                            layout.name == *name && layout.section == StorageSection::File
                        })
                    })
                {
                    return Err(SemanticProblem::InvalidFileLayout(record_name.clone()));
                }
            }
        }
        let record_end = record.offset.saturating_add(record.length);
        for key in file
            .record_key
            .iter()
            .chain(file.alternate_record_keys.iter())
        {
            let key = layouts
                .iter()
                .find(|layout| layout.name == *key)
                .ok_or_else(|| SemanticProblem::InvalidFileLayout(key.clone()))?;
            let contained = key.section == StorageSection::File
                && key.offset >= record.offset
                && key.offset.saturating_add(key.length) <= record_end;
            let source_root = key.qualified_name.split('.').next().unwrap_or(&key.name);
            let compatible_io_source = record.level == 1
                && record.category == DataCategory::Alphanumeric
                && key.section != StorageSection::File
                && every_file_io_uses_source(source, &file.select_name, &record.name, source_root);
            if !contained && !compatible_io_source {
                return Err(SemanticProblem::InvalidFileLayout(key.name.clone()));
            }
        }
        if let Some(relative_key) = &file.relative_key {
            let key = layouts
                .iter()
                .find(|layout| layout.name == *relative_key)
                .ok_or_else(|| SemanticProblem::InvalidFileLayout(relative_key.clone()))?;
            if !matches!(
                key.category,
                DataCategory::NumericDisplay | DataCategory::PackedDecimal | DataCategory::Binary
            ) || key.scale != 0
            {
                return Err(SemanticProblem::InvalidFileLayout(relative_key.clone()));
            }
        }
        if let Some(status) = &file.file_status {
            let status = layouts
                .iter()
                .find(|layout| layout.name == *status)
                .ok_or_else(|| SemanticProblem::InvalidFileLayout(status.clone()))?;
            if status.length != 2 {
                return Err(SemanticProblem::InvalidFileLayout(status.name.clone()));
            }
        }
    }
    Ok(())
}

fn every_file_io_uses_source(source: &str, file: &str, record: &str, source_root: &str) -> bool {
    let words = declaration_words(source);
    let writes = words
        .windows(2)
        .filter(|window| window[0] == "WRITE" && window[1] == record)
        .count();
    let mapped = words
        .windows(4)
        .filter(|window| {
            window[0] == "WRITE"
                && window[1] == record
                && window[2] == "FROM"
                && window[3] == source_root
        })
        .count();
    let reads = words
        .windows(2)
        .filter(|window| window[0] == "READ" && window[1] == file)
        .count();
    let mapped_reads = words
        .windows(4)
        .filter(|window| {
            window[0] == "READ"
                && window[1] == file
                && window[2] == "INTO"
                && window[3] == source_root
        })
        .count();
    writes + reads > 0 && writes + reads == mapped + mapped_reads
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
        if quote.is_none()
            && let Some(end) = embedded_exec_end(source, index)
        {
            let sentence = source[start..end].trim();
            if !sentence.is_empty() {
                sentences.push(sentence.to_string());
            }
            start = end;
            index = end;
            continue;
        }
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

fn declaration_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut characters = line.chars().peekable();
    while let Some(character) = characters.next() {
        if let Some(delimiter) = quote {
            word.push(character);
            if character == delimiter {
                if characters.peek() == Some(&delimiter) {
                    word.push(characters.next().expect("peeked quote"));
                } else {
                    quote = None;
                }
            }
        } else if matches!(character, '\'' | '"') {
            word.push(character);
            quote = Some(character);
        } else if character.is_whitespace() {
            if !word.is_empty() {
                let value = word.trim_matches([',', ';', '.']).to_ascii_uppercase();
                if !value.is_empty() {
                    words.push(value);
                }
                word.clear();
            }
        } else {
            word.push(character);
        }
    }
    if !word.is_empty() {
        let value = word.trim_matches([',', ';', '.']).to_ascii_uppercase();
        if !value.is_empty() {
            words.push(value);
        }
    }
    words
}

fn embedded_exec_end(source: &str, index: usize) -> Option<usize> {
    const MARKER: &str = "END-EXEC";
    let end = index.checked_add(MARKER.len())?;
    if !source.get(index..end)?.eq_ignore_ascii_case(MARKER)
        || index
            .checked_sub(1)
            .and_then(|at| source.as_bytes().get(at))
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        || source
            .as_bytes()
            .get(end)
            .is_some_and(|byte| !byte.is_ascii_whitespace() && *byte != b'.')
    {
        None
    } else {
        Some(end)
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

fn find_after_owned(words: &[String], name: &str) -> Option<String> {
    words
        .iter()
        .position(|word| word.eq_ignore_ascii_case(name))
        .and_then(|index| words.get(index + 1).cloned())
}

fn qualified_reference_after(words: &[String], sequence: &[&str]) -> Option<String> {
    let start = words.windows(sequence.len()).position(|window| {
        window
            .iter()
            .zip(sequence)
            .all(|(word, expected)| word.eq_ignore_ascii_case(expected))
    })? + sequence.len();
    let mut reference = words.get(start)?.clone();
    let mut cursor = start + 1;
    while words
        .get(cursor)
        .is_some_and(|word| matches!(word.as_str(), "OF" | "IN"))
    {
        reference.push_str(" OF ");
        reference.push_str(words.get(cursor + 1)?);
        cursor += 2;
    }
    Some(reference)
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

fn occurs_range(words: &[String]) -> Result<OccursSpec, SemanticProblem> {
    let Some(index) = words.iter().position(|word| word == "OCCURS") else {
        return Ok(OccursSpec {
            minimum: 1,
            maximum: 1,
            unbounded: false,
        });
    };
    let (minimum, maximum_word) = if words.get(index + 1).is_some_and(|word| word == "UNBOUNDED") {
        (1, words.get(index + 1))
    } else {
        let minimum = words
            .get(index + 1)
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or(SemanticProblem::InvalidOccurs)?;
        let maximum = if words.get(index + 2).is_some_and(|word| word == "TO") {
            words.get(index + 3)
        } else {
            words.get(index + 1)
        };
        (minimum, maximum)
    };
    let unbounded = maximum_word.is_some_and(|value| value == "UNBOUNDED");
    let maximum = if unbounded {
        0
    } else {
        maximum_word
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or(SemanticProblem::InvalidOccurs)?
    };
    let variable = words.iter().any(|word| word == "DEPENDING");
    if (!unbounded && (maximum == 0 || minimum > maximum || maximum > 1_000_000))
        || (!variable && minimum == 0)
        || (unbounded && !variable)
    {
        return Err(SemanticProblem::InvalidOccurs);
    }
    Ok(OccursSpec {
        minimum,
        maximum,
        unbounded,
    })
}

fn explicit_usage(words: &[String]) -> Result<Option<CobolUsage>, SemanticProblem> {
    let mut found = Vec::new();
    for (index, word) in words.iter().enumerate().skip(2) {
        if let Some(usage) = usage_word(word) {
            found.push((index, usage));
        }
    }
    found.sort_by_key(|(index, _)| *index);
    found.dedup_by_key(|(_, usage)| *usage);
    match found.as_slice() {
        [] => Ok(None),
        [(_, usage)] => Ok(Some(*usage)),
        multiple
            if multiple.len() == 2
                && multiple[0].1 == CobolUsage::ObjectReference
                && multiple[1].1 == CobolUsage::ObjectReference =>
        {
            Ok(Some(CobolUsage::ObjectReference))
        }
        _ => Err(SemanticProblem::InvalidUsage(
            "multiple incompatible USAGE representations".into(),
        )),
    }
}

fn usage_word(word: &str) -> Option<CobolUsage> {
    Some(match word {
        "BINARY" | "COMP" | "COMP-4" | "COMPUTATIONAL" | "COMPUTATIONAL-4" => CobolUsage::Binary,
        "COMP-5" | "COMPUTATIONAL-5" => CobolUsage::NativeBinary,
        "COMP-3" | "COMPUTATIONAL-3" | "PACKED-DECIMAL" => CobolUsage::PackedDecimal,
        "COMP-1" | "COMPUTATIONAL-1" => CobolUsage::FloatShort,
        "COMP-2" | "COMPUTATIONAL-2" => CobolUsage::FloatLong,
        "DISPLAY" => CobolUsage::Display,
        "DISPLAY-1" => CobolUsage::Display1,
        "INDEX" => CobolUsage::Index,
        "NATIONAL" => CobolUsage::National,
        "UTF-8" => CobolUsage::Utf8,
        "POINTER" => CobolUsage::Pointer,
        "POINTER-32" => CobolUsage::Pointer32,
        "PROCEDURE-POINTER" => CobolUsage::ProcedurePointer,
        "FUNCTION-POINTER" => CobolUsage::FunctionPointer,
        "OBJECT" => CobolUsage::ObjectReference,
        _ => return None,
    })
}

fn picture_operand_semantic(words: &[String]) -> Option<String> {
    let index = words
        .iter()
        .position(|word| matches!(word.as_str(), "PIC" | "PICTURE"))?;
    words
        .get(index + 1 + usize::from(words.get(index + 1).is_some_and(|word| word == "IS")))
        .cloned()
}

fn byte_length(words: &[String]) -> Result<Option<usize>, SemanticProblem> {
    let Some(index) = words.iter().position(|word| word == "BYTE-LENGTH") else {
        return Ok(None);
    };
    let value = words
        .get(index + 1 + usize::from(words.get(index + 1).is_some_and(|word| word == "IS")))
        .and_then(|word| word.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| SemanticProblem::InvalidPicture("invalid BYTE-LENGTH".into()))?;
    Ok(Some(value))
}

fn object_class(words: &[String]) -> Option<String> {
    let index = words
        .windows(2)
        .position(|pair| pair[0] == "OBJECT" && pair[1] == "REFERENCE")?;
    words
        .get(index + 2)
        .filter(|word| {
            !matches!(
                word.as_str(),
                "GLOBAL" | "EXTERNAL" | "OCCURS" | "VALUE" | "VOLATILE" | "SYNC" | "SYNCHRONIZED"
            )
        })
        .cloned()
}

fn require_numeric_picture(
    details: &PictureDetails,
    usage: CobolUsage,
) -> Result<(), SemanticProblem> {
    if details.numeric
        && !details.edited
        && !details.alphabetic
        && !details.national
        && !details.utf8
        && !details.dbcs
    {
        Ok(())
    } else {
        Err(SemanticProblem::InvalidUsage(format!(
            "{usage:?} requires an unedited numeric PICTURE"
        )))
    }
}

const fn binary_length(digits: usize) -> Option<usize> {
    match digits {
        1..=4 => Some(2),
        5..=9 => Some(4),
        10..=18 => Some(8),
        _ => None,
    }
}

struct PictureSpec {
    category: DataCategory,
    usage: CobolUsage,
    length: usize,
    picture: Option<String>,
    byte_length: Option<usize>,
    digits: usize,
    scale: usize,
    signed: bool,
    sign_separate: bool,
    object_class: Option<String>,
}

fn picture(
    words: &[String],
    is_group: bool,
    inherited_usage: Option<CobolUsage>,
    pointer_bytes: usize,
) -> Result<PictureSpec, SemanticProblem> {
    let explicit_usage = explicit_usage(words)?;
    if inherited_usage.is_some() && explicit_usage.is_some() && inherited_usage != explicit_usage {
        return Err(SemanticProblem::InvalidUsage(
            "subordinate USAGE contradicts group usage".into(),
        ));
    }
    let pic = picture_operand_semantic(words);
    let inferred_usage = pic.as_deref().and_then(|picture| {
        picture
            .bytes()
            .any(|byte| byte.eq_ignore_ascii_case(&b'U'))
            .then_some(CobolUsage::Utf8)
            .or_else(|| {
                picture
                    .bytes()
                    .any(|byte| byte.eq_ignore_ascii_case(&b'N'))
                    .then_some(CobolUsage::National)
            })
    });
    let usage = explicit_usage
        .or(inherited_usage)
        .or(inferred_usage)
        .unwrap_or(if is_group {
            CobolUsage::Group
        } else {
            CobolUsage::Display
        });
    if is_group {
        if pic.is_some() {
            return Err(SemanticProblem::InvalidPicture(
                "group item cannot have PICTURE".into(),
            ));
        }
        return Ok(PictureSpec {
            category: match usage {
                CobolUsage::National => DataCategory::NationalGroup,
                CobolUsage::Utf8 => DataCategory::Utf8Group,
                _ => DataCategory::Group,
            },
            usage,
            length: 0,
            picture: None,
            byte_length: None,
            digits: 0,
            scale: 0,
            signed: false,
            sign_separate: false,
            object_class: None,
        });
    }
    if matches!(
        usage,
        CobolUsage::FloatShort
            | CobolUsage::FloatLong
            | CobolUsage::Index
            | CobolUsage::Pointer
            | CobolUsage::Pointer32
            | CobolUsage::ProcedurePointer
            | CobolUsage::FunctionPointer
            | CobolUsage::ObjectReference
    ) {
        if pic.is_some() {
            return Err(SemanticProblem::InvalidUsage(format!(
                "{usage:?} cannot have PICTURE"
            )));
        }
        let (category, length) = match usage {
            CobolUsage::FloatShort => (DataCategory::FloatShort, 4),
            CobolUsage::FloatLong => (DataCategory::FloatLong, 8),
            CobolUsage::Index => (DataCategory::Index, pointer_bytes),
            CobolUsage::Pointer => (DataCategory::Pointer, pointer_bytes),
            CobolUsage::Pointer32 => (DataCategory::Pointer32, 4),
            CobolUsage::ProcedurePointer => (DataCategory::ProcedurePointer, 8),
            CobolUsage::FunctionPointer => (DataCategory::FunctionPointer, pointer_bytes),
            CobolUsage::ObjectReference => (DataCategory::ObjectReference, pointer_bytes),
            _ => unreachable!(),
        };
        return Ok(PictureSpec {
            category,
            usage,
            length,
            picture: None,
            byte_length: None,
            digits: 0,
            scale: 0,
            signed: false,
            sign_separate: false,
            object_class: object_class(words),
        });
    }
    let pic = pic.ok_or_else(|| {
        SemanticProblem::InvalidPicture("elementary item requires PICTURE or pointer usage".into())
    })?;
    let details = picture_details(&pic)?;
    let separate = words
        .windows(2)
        .any(|pair| pair[0] == "SIGN" && pair[1] == "SEPARATE")
        || words.iter().any(|word| word == "SEPARATE");
    let byte_length = byte_length(words)?;
    let (category, length) = match usage {
        CobolUsage::Binary | CobolUsage::NativeBinary => {
            require_numeric_picture(&details, usage)?;
            (
                DataCategory::Binary,
                binary_length(details.digits).ok_or_else(|| {
                    SemanticProblem::InvalidPicture("binary PICTURE exceeds 18 digits".into())
                })?,
            )
        }
        CobolUsage::PackedDecimal => {
            require_numeric_picture(&details, usage)?;
            if details.digits > 31 {
                return Err(SemanticProblem::InvalidPicture(
                    "packed PICTURE exceeds 31 digits".into(),
                ));
            }
            (DataCategory::PackedDecimal, (details.digits + 2) / 2)
        }
        CobolUsage::Display => {
            if byte_length.is_some() || details.utf8 || details.national || details.dbcs {
                return Err(SemanticProblem::InvalidUsage(
                    "DISPLAY usage contradicts PICTURE symbols".into(),
                ));
            }
            if details.numeric {
                (
                    if details.edited {
                        DataCategory::NumericEdited
                    } else {
                        DataCategory::NumericDisplay
                    },
                    details.storage.max(1) + usize::from(details.signed && separate),
                )
            } else if details.alphabetic {
                (DataCategory::Alphabetic, details.storage.max(1))
            } else {
                (
                    if details.edited {
                        DataCategory::AlphanumericEdited
                    } else {
                        DataCategory::Alphanumeric
                    },
                    details.storage.max(1),
                )
            }
        }
        CobolUsage::Display1 => {
            if !details.dbcs || details.numeric || byte_length.is_some() {
                return Err(SemanticProblem::InvalidUsage(
                    "DISPLAY-1 requires a DBCS PICTURE".into(),
                ));
            }
            (DataCategory::Dbcs, details.storage.max(1) * 2)
        }
        CobolUsage::National => {
            if details.utf8 || details.dbcs || byte_length.is_some() {
                return Err(SemanticProblem::InvalidUsage(
                    "NATIONAL usage contradicts PICTURE symbols".into(),
                ));
            }
            (
                if details.numeric && details.edited {
                    DataCategory::NationalEdited
                } else if details.numeric {
                    DataCategory::NumericDisplay
                } else {
                    DataCategory::National
                },
                details.storage.max(1) * 2 + usize::from(details.signed && separate) * 2,
            )
        }
        CobolUsage::Utf8 => {
            if !details.utf8 || details.storage == 0 || details.numeric || details.edited {
                return Err(SemanticProblem::InvalidUsage(
                    "UTF-8 usage requires only U PICTURE symbols".into(),
                ));
            }
            (
                DataCategory::Utf8,
                byte_length.unwrap_or(details.storage.saturating_mul(4)),
            )
        }
        _ => unreachable!(),
    };
    Ok(PictureSpec {
        category,
        usage,
        length,
        picture: Some(pic),
        byte_length,
        digits: details.digits,
        scale: details.scale,
        signed: details.signed,
        sign_separate: details.signed && separate,
        object_class: None,
    })
}

struct PictureDetails {
    storage: usize,
    digits: usize,
    scale: usize,
    numeric: bool,
    edited: bool,
    signed: bool,
    alphabetic: bool,
    national: bool,
    utf8: bool,
    dbcs: bool,
}

fn picture_details(pic: &str) -> Result<PictureDetails, SemanticProblem> {
    let bytes = expanded_picture_symbols(pic)?;
    let mut storage = 0usize;
    let mut digits = 0usize;
    let mut numeric = false;
    let mut edited = false;
    let mut signed = false;
    let mut scale = 0usize;
    let mut fractional = false;
    let mut alphabetic = true;
    let mut national = false;
    let mut utf8 = false;
    let mut dbcs = false;
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = bytes[index].to_ascii_uppercase();
        let repeat = 1usize;
        match byte {
            b'9' => {
                numeric = true;
                alphabetic = false;
                digits += repeat;
                if fractional {
                    scale += repeat;
                }
                storage += repeat;
            }
            b'X' => {
                alphabetic = false;
                storage += repeat;
            }
            b'A' => storage += repeat,
            b'G' => {
                alphabetic = false;
                dbcs = true;
                storage += repeat;
            }
            b'N' => {
                alphabetic = false;
                national = true;
                storage += repeat;
            }
            b'U' => {
                alphabetic = false;
                utf8 = true;
                storage += repeat;
            }
            b'Z' | b'*' => {
                numeric = true;
                alphabetic = false;
                edited = true;
                digits += repeat;
                if fractional {
                    scale += repeat;
                }
                storage += repeat;
            }
            b'S' => {
                alphabetic = false;
                signed = true;
            }
            b'V' => {
                numeric = true;
                alphabetic = false;
                fractional = true;
            }
            b'P' => {
                numeric = true;
                alphabetic = false;
                digits += repeat;
                if fractional {
                    scale += repeat;
                }
            }
            b'+' | b'-' => {
                numeric = true;
                alphabetic = false;
                edited = true;
                signed = true;
                if bytes.get(index.wrapping_sub(1)) == Some(&byte)
                    || bytes.get(index + 1) == Some(&byte)
                {
                    digits += 1;
                    if fractional {
                        scale += 1;
                    }
                }
                storage += repeat;
            }
            b'.' => {
                alphabetic = false;
                edited = true;
                fractional = true;
                storage += repeat;
            }
            b',' | b'$' | b'/' | b'B' | b'0' => {
                alphabetic = false;
                edited = true;
                storage += repeat;
            }
            b'C' | b'R' | b'D' | b'E' => {
                alphabetic = false;
                numeric = true;
                edited = true;
                storage += repeat;
            }
            _ => {
                return Err(SemanticProblem::InvalidPicture(format!(
                    "unsupported PICTURE symbol {}",
                    char::from(byte)
                )));
            }
        }
        index += 1;
    }
    if storage == 0 && digits == 0 {
        return Err(SemanticProblem::InvalidPicture(
            "PICTURE has no data positions".into(),
        ));
    }
    Ok(PictureDetails {
        storage,
        digits,
        scale,
        numeric,
        edited,
        signed,
        alphabetic,
        national,
        utf8,
        dbcs,
    })
}

fn expanded_picture_symbols(pic: &str) -> Result<Vec<u8>, SemanticProblem> {
    let bytes = pic.as_bytes();
    let mut symbols = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while index < bytes.len() {
        let symbol = bytes[index].to_ascii_uppercase();
        index += 1;
        let repeat = if bytes.get(index) == Some(&b'(') {
            let relative_close = bytes[index + 1..]
                .iter()
                .position(|byte| *byte == b')')
                .ok_or_else(|| SemanticProblem::InvalidPicture("unclosed repeat".into()))?;
            let close = index + 1 + relative_close;
            let repeat = std::str::from_utf8(&bytes[index + 1..close])
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|value| (1..=1_000_000).contains(value))
                .ok_or_else(|| SemanticProblem::InvalidPicture("invalid repeat".into()))?;
            index = close + 1;
            repeat
        } else {
            1
        };
        if symbols.len().saturating_add(repeat) > 1_000_000 {
            return Err(SemanticProblem::InvalidPicture(
                "expanded PICTURE exceeds limit".into(),
            ));
        }
        symbols.extend(std::iter::repeat_n(symbol, repeat));
    }
    Ok(symbols)
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
            DataCategory::Alphabetic
                | DataCategory::Alphanumeric
                | DataCategory::AlphanumericEdited
                | DataCategory::Dbcs
                | DataCategory::National
                | DataCategory::NationalEdited
                | DataCategory::Utf8
                | DataCategory::NumericDisplay
                | DataCategory::NumericEdited
        ) {
            if matches!(
                category,
                DataCategory::NumericDisplay
                    | DataCategory::NumericEdited
                    | DataCategory::NationalEdited
            ) {
                b'0'
            } else {
                b' '
            }
        } else {
            0
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
    if let Some(hex) = hexadecimal_literal(tail) {
        let copy = hex.len().min(result.len());
        result[..copy].copy_from_slice(&hex[..copy]);
        return result;
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

fn hexadecimal_literal(value: &str) -> Option<Vec<u8>> {
    let value = value.trim();
    let bytes = value.as_bytes();
    if bytes.len() < 3 || !matches!(bytes[0], b'X' | b'x') || !matches!(bytes[1], b'\'' | b'"') {
        return None;
    }
    let quote = bytes[1];
    let end = bytes[2..].iter().position(|byte| *byte == quote)? + 2;
    let digits = &bytes[2..end];
    if digits.is_empty()
        || !digits.len().is_multiple_of(2)
        || !digits.iter().all(u8::is_ascii_hexdigit)
    {
        return None;
    }
    digits
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let high = hex_nibble(pair[0])?;
            let low = hex_nibble(pair[1])?;
            Some((high << 4) | low)
        })
        .collect()
}

const fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
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
    InvalidClause(String),
    InvalidPicture(String),
    InvalidUsage(String),
    InvalidRedefines(String),
    InvalidReference(String),
    InvalidFileLayout(String),
    InvalidIntrinsic(String),
    InvalidSpecialRegister(String),
    ScopeLimitExceeded,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(declarations: &str) -> String {
        format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. TYPES. DATA DIVISION. WORKING-STORAGE SECTION. {declarations}. PROCEDURE DIVISION. STOP RUN."
        )
    }

    #[test]
    fn hierarchical_duplicates_qualification_and_group_layout_are_exact() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. T. DATA DIVISION. WORKING-STORAGE SECTION. 01 ROOT-A. 05 VALUE-X PIC X(2) VALUE 'AA'. 01 ROOT-B. 05 VALUE-X PIC X(3) VALUE 'BBB'. PROCEDURE DIVISION. STOP RUN.";
        let model = SemanticModel::analyze(source, 1024, 32).unwrap();
        assert!(model.layout("VALUE-X").is_none());
        assert_eq!(model.layout("VALUE-X OF ROOT-A").unwrap().initial, b"AA");
        assert_eq!(model.layout("ROOT-A").unwrap().length, 2);
        assert_eq!(model.layout("ROOT-B").unwrap().offset, 8);
    }

    #[test]
    fn comment_sentences_do_not_hide_following_data_declarations() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. T. DATA DIVISION. WORKING-STORAGE SECTION.\n*> banner with a quote ' and period.\n01 ROOT.\n*> another \"comment.\n05 CHILD PIC X. PROCEDURE DIVISION. STOP RUN.";
        let model = SemanticModel::analyze(source, 1024, 32).unwrap();
        assert_eq!(model.layout("ROOT").unwrap().length, 1);
        assert_eq!(model.layout("CHILD OF ROOT").unwrap().length, 1);
        assert_eq!(model.data_descriptions.len(), 2);
    }

    #[test]
    fn declaration_area_exec_sql_without_a_period_does_not_capture_the_next_data_item() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. SQLDECL. DATA DIVISION. WORKING-STORAGE SECTION. 01 BEFORE-X PIC X. EXEC SQL DECLARE C1 CURSOR FOR SELECT COL FROM TABLE-X END-EXEC\n01 AFTER-X PIC X. PROCEDURE DIVISION. GOBACK.";
        let model = SemanticModel::analyze(source, 1024, 32).unwrap();
        assert!(model.layout("BEFORE-X").is_some());
        assert!(model.layout("AFTER-X").is_some());
        assert_eq!(model.data_descriptions.len(), 2);
    }

    #[test]
    fn data_clause_keywords_inside_quoted_values_are_not_reinterpreted() {
        let source = program(
            "01 MESSAGE-X PIC X(40). 88 MESSAGE-SHOWN VALUE 'Selected transaction type shown above'",
        );
        let model = SemanticModel::analyze(&source, 1024, 32).unwrap();
        assert!(model.layout("MESSAGE-X").is_some());
        assert!(model.layout("MESSAGE-SHOWN").is_some());
        assert_eq!(model.data_descriptions.len(), 2);
    }

    #[test]
    fn raw_indexed_file_key_can_come_from_every_explicit_write_source_only() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. EXPORT. ENVIRONMENT DIVISION. INPUT-OUTPUT SECTION. FILE-CONTROL. SELECT EXPORT-OUTPUT ASSIGN TO EXPFILE ORGANIZATION IS INDEXED RECORD KEY IS EXPORT-KEY. DATA DIVISION. FILE SECTION. FD EXPORT-OUTPUT. 01 EXPORT-OUTPUT-RECORD PIC X(10). WORKING-STORAGE SECTION. 01 EXPORT-RECORD. 05 EXPORT-KEY PIC 9. 05 EXPORT-DATA PIC X(9). PROCEDURE DIVISION. WRITE EXPORT-OUTPUT-RECORD FROM EXPORT-RECORD. GOBACK.";
        let model = SemanticModel::analyze(source, 1024, 32).unwrap();
        assert_eq!(model.files[0].record_key.as_deref(), Some("EXPORT-KEY"));

        let unsafe_write = source.replace(
            "WRITE EXPORT-OUTPUT-RECORD FROM EXPORT-RECORD",
            "WRITE EXPORT-OUTPUT-RECORD",
        );
        assert_eq!(
            SemanticModel::analyze(&unsafe_write, 1024, 32),
            Err(SemanticProblem::InvalidFileLayout("EXPORT-KEY".into()))
        );

        let input = "IDENTIFICATION DIVISION. PROGRAM-ID. IMPORT. ENVIRONMENT DIVISION. INPUT-OUTPUT SECTION. FILE-CONTROL. SELECT EXPORT-INPUT ASSIGN TO EXPFILE ORGANIZATION IS INDEXED RECORD KEY IS EXPORT-KEY. DATA DIVISION. FILE SECTION. FD EXPORT-INPUT. 01 EXPORT-INPUT-RECORD PIC X(10). WORKING-STORAGE SECTION. 01 EXPORT-RECORD. 05 EXPORT-KEY PIC 9. 05 EXPORT-DATA PIC X(9). PROCEDURE DIVISION. READ EXPORT-INPUT INTO EXPORT-RECORD. GOBACK.";
        assert!(SemanticModel::analyze(input, 1024, 32).is_ok());
        let unsafe_read =
            input.replace("READ EXPORT-INPUT INTO EXPORT-RECORD", "READ EXPORT-INPUT");
        assert_eq!(
            SemanticModel::analyze(&unsafe_read, 1024, 32),
            Err(SemanticProblem::InvalidFileLayout("EXPORT-KEY".into()))
        );
    }

    #[test]
    fn cics_eibcalen_is_an_exact_implicit_occurs_dependency_only_in_cics_context() {
        let declarations = "LINKAGE SECTION. 01 DFHCOMMAREA. 05 DATA-BYTE PIC X OCCURS 1 TO 32767 TIMES DEPENDING ON EIBCALEN";
        let cics = format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSAPP. DATA DIVISION. {declarations}. PROCEDURE DIVISION. EXEC CICS RETURN END-EXEC."
        );
        let model = SemanticModel::analyze(&cics, 65_536, 32).unwrap();
        assert_eq!(
            model.layout("DATA-BYTE").unwrap().depending_on.as_deref(),
            Some("EIBCALEN")
        );

        let batch = format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. BATCHAPP. DATA DIVISION. {declarations}. PROCEDURE DIVISION. GOBACK."
        );
        assert_eq!(
            SemanticModel::analyze(&batch, 65_536, 32),
            Err(SemanticProblem::InvalidReference("EIBCALEN".into()))
        );
    }

    #[test]
    fn fixed_occurs_can_redefine_an_exact_overlay_but_variable_occurs_cannot() {
        let fixed = program(
            "01 ROOT. 05 RAW-DATA PIC X(20). 05 DATA-PART REDEFINES RAW-DATA OCCURS 10 TIMES PIC X(2) INDEXED BY PART-INDEX",
        );
        let model = SemanticModel::analyze(&fixed, 1024, 32).unwrap();
        let raw = model.layout("RAW-DATA").unwrap();
        let part = model.layout("DATA-PART").unwrap();
        assert_eq!(
            (part.offset, part.length, part.occurs),
            (raw.offset, 20, 10)
        );

        let variable = program(
            "01 TABLE-COUNT PIC 99. 01 ROOT. 05 RAW-DATA PIC X(20). 05 DATA-PART REDEFINES RAW-DATA OCCURS 1 TO 10 TIMES DEPENDING ON TABLE-COUNT PIC X(2)",
        );
        assert_eq!(
            SemanticModel::analyze(&variable, 1024, 32),
            Err(SemanticProblem::InvalidOccurs)
        );
    }

    #[test]
    fn intrinsic_arguments_resolve_subscripted_data_references_and_reject_missing_items() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. INTRINSIC. DATA DIVISION. WORKING-STORAGE SECTION. 01 TABLE-ROOT. 05 TABLE-ITEM PIC X(10) OCCURS 2 TIMES. 01 ITEM-INDEX PIC 9. 01 OUTPUT-X PIC X(10). PROCEDURE DIVISION. MOVE FUNCTION TRIM(TABLE-ITEM(ITEM-INDEX)) TO OUTPUT-X. GOBACK.";
        let model = SemanticModel::analyze(source, 1024, 32).unwrap();
        assert_eq!(model.intrinsic_calls.len(), 1);
        assert_eq!(
            model.intrinsic_calls[0].arguments[0].text,
            "TABLE-ITEM(ITEM-INDEX)"
        );

        let missing = source.replace("TABLE-ITEM(ITEM-INDEX)", "MISSING(ITEM-INDEX)");
        assert!(matches!(
            SemanticModel::analyze(&missing, 1024, 32),
            Err(SemanticProblem::InvalidIntrinsic(problem)) if problem.contains("unresolved intrinsic argument")
        ));
    }

    #[test]
    fn zero_argument_intrinsic_reference_modification_has_selected_length() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. DATEREF. DATA DIVISION. WORKING-STORAGE SECTION. 01 OUTPUT-X PIC X(4). PROCEDURE DIVISION. MOVE FUNCTION CURRENT-DATE(1:4) TO OUTPUT-X. GOBACK.";
        let model = SemanticModel::analyze(source, 1024, 32).unwrap();
        assert_eq!(model.intrinsic_calls.len(), 1);
        assert!(model.intrinsic_calls[0].arguments.is_empty());
        assert_eq!(model.intrinsic_calls[0].fixed_length, Some(4));

        let malformed = source.replace("(1:4)", "(0:4)");
        assert!(matches!(
            SemanticModel::analyze(&malformed, 1024, 32),
            Err(SemanticProblem::InvalidIntrinsic(_))
        ));
    }

    #[test]
    fn levels_redefines_occurs_conditions_and_references_are_explicit() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. T. DATA DIVISION. WORKING-STORAGE SECTION. 01 ROOT. 05 COUNT-X PIC 9 VALUE 2. 05 TABLE-X OCCURS 1 TO 3 TIMES DEPENDING ON COUNT-X INDEXED BY IX. 10 ITEM-X PIC X(2) VALUE 'AB'. 05 RAW-X PIC X(4). 05 NUM-X REDEFINES RAW-X PIC 9(4). 88 NUM-VALID VALUE 1 THRU 9. 66 RANGE-X RENAMES COUNT-X THRU RAW-X. 77 SOLO-X PIC S9(4) COMP-3 VALUE -12. 88 SOLO-VALID VALUE 1 THRU 9. PROCEDURE DIVISION. STOP RUN.";
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
            model.layout("NUM-VALID").unwrap().condition_values,
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
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. T. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC S9(4). 01 B PIC S9(9) COMP. 01 C PIC S9(7)V99 COMP-3 VALUE -123.45. 01 D PIC ZZ,ZZ9.99-. 01 E PIC ----9. PROCEDURE DIVISION. STOP RUN.";
        let model = SemanticModel::analyze(source, 1024, 32).unwrap();
        assert_eq!(model.layout("A").unwrap().length, 4);
        assert_eq!(model.layout("B").unwrap().length, 4);
        assert_eq!(model.layout("C").unwrap().length, 5);
        assert_eq!(
            model.layout("D").unwrap().category,
            DataCategory::NumericEdited
        );
        assert_eq!(model.layout("D").unwrap().length, 10);
        assert_eq!(model.layout("E").unwrap().digits, 5);
    }

    #[test]
    fn complete_usage_classes_have_stable_size_and_identity() {
        let source = program(
            "01 ALPHA PIC A(3). 01 ALNUM PIC X(3). 01 DBCS-ITEM PIC G(2) DISPLAY-1. 01 NATIONAL-ITEM PIC N(2) NATIONAL. 01 UTF8-ITEM PIC U(3) BYTE-LENGTH 9 UTF-8. 01 SHORT-FLOAT COMP-1. 01 LONG-FLOAT COMP-2. 01 INDEX-ITEM INDEX. 01 DATA-PTR POINTER. 01 PTR32 POINTER-32. 01 PROC-PTR PROCEDURE-POINTER. 01 FUNC-PTR FUNCTION-POINTER. 01 OBJ OBJECT REFERENCE. 01 DYNAMIC-ITEM PIC X DYNAMIC LENGTH LIMIT IS 100",
        );
        let model = SemanticModel::analyze(&source, 4096, 64).unwrap();
        for (name, category, usage, length) in [
            ("ALPHA", DataCategory::Alphabetic, CobolUsage::Display, 3),
            ("ALNUM", DataCategory::Alphanumeric, CobolUsage::Display, 3),
            ("DBCS-ITEM", DataCategory::Dbcs, CobolUsage::Display1, 4),
            (
                "NATIONAL-ITEM",
                DataCategory::National,
                CobolUsage::National,
                4,
            ),
            ("UTF8-ITEM", DataCategory::Utf8, CobolUsage::Utf8, 9),
            (
                "SHORT-FLOAT",
                DataCategory::FloatShort,
                CobolUsage::FloatShort,
                4,
            ),
            (
                "LONG-FLOAT",
                DataCategory::FloatLong,
                CobolUsage::FloatLong,
                8,
            ),
            ("INDEX-ITEM", DataCategory::Index, CobolUsage::Index, 4),
            ("DATA-PTR", DataCategory::Pointer, CobolUsage::Pointer, 4),
            ("PTR32", DataCategory::Pointer32, CobolUsage::Pointer32, 4),
            (
                "PROC-PTR",
                DataCategory::ProcedurePointer,
                CobolUsage::ProcedurePointer,
                8,
            ),
            (
                "FUNC-PTR",
                DataCategory::FunctionPointer,
                CobolUsage::FunctionPointer,
                4,
            ),
            (
                "OBJ",
                DataCategory::ObjectReference,
                CobolUsage::ObjectReference,
                4,
            ),
        ] {
            let layout = model.layout(name).unwrap();
            assert_eq!(
                (layout.category, layout.usage, layout.length),
                (category, usage, length)
            );
        }
        let dynamic = model.layout("DYNAMIC-ITEM").unwrap();
        assert!(dynamic.dynamic);
        assert_eq!((dynamic.length, dynamic.dynamic_limit), (0, Some(100)));
        assert_eq!(model.execution_incomplete_layouts(), BTreeSet::new());
    }

    #[test]
    fn occurs_dependencies_and_keys_resolve_in_the_owning_hierarchy() {
        let source = program(
            "01 GROUP-A. 05 N PIC 9. 05 TABLE-A OCCURS 1 TO 3 TIMES DEPENDING ON N ASCENDING KEY IS KEY-X. 10 KEY-X PIC X. 01 GROUP-B. 05 N PIC 9. 05 KEY-X PIC X. 05 TABLE-B OCCURS 1 TO 3 TIMES DEPENDING ON N OF GROUP-B. 10 ITEM-B PIC X",
        );
        let model = SemanticModel::analyze(&source, 4096, 128).unwrap();
        let table_a = model.layout("GROUP-A.TABLE-A").unwrap();
        assert_eq!(table_a.depending_on.as_deref(), Some("N"));
        assert_eq!(table_a.keys[0].name, "KEY-X");
        let table_b = model.layout("GROUP-B.TABLE-B").unwrap();
        assert_eq!(table_b.depending_on.as_deref(), Some("N OF GROUP-B"));
    }

    #[test]
    fn synchronized_groups_group_usage_and_types_preserve_layout_contracts() {
        let source = program(
            "01 ALIGNED. 05 PREFIX PIC X. 05 FULL PIC S9(9) BINARY SYNC. 05 HALF PIC S9(4) BINARY SYNC. 01 TABLE-ROOT. 05 TABLE-ITEM OCCURS 2 TIMES. 10 FLAG PIC X. 10 VALUE-X PIC S9(9) BINARY SYNC. 01 NATIONAL-ROOT GROUP-USAGE NATIONAL. 05 NATIONAL-CHAR PIC N(2). 01 UTF8-ROOT GROUP-USAGE UTF-8. 05 UTF8-CHAR PIC U(2) BYTE-LENGTH 6. 01 PART-T TYPEDEF. 05 CODE-X PIC X(2). 05 QTY-X PIC 9(3) COMP-3. 01 PART TYPE PART-T",
        );
        let model = SemanticModel::analyze(&source, 4096, 128).unwrap();
        assert_eq!(model.layout("FULL").unwrap().offset, 4);
        assert_eq!(model.layout("HALF").unwrap().offset, 8);
        assert_eq!(model.layout("ALIGNED").unwrap().length, 10);
        let table = model.layout("TABLE-ITEM").unwrap();
        assert_eq!(
            (table.element_length, table.length, table.alignment),
            (8, 16, 4)
        );
        assert_eq!(
            model.layout("NATIONAL-ROOT").unwrap().category,
            DataCategory::NationalGroup
        );
        assert_eq!(
            model.layout("NATIONAL-CHAR").unwrap().usage,
            CobolUsage::National
        );
        assert_eq!(
            model.layout("UTF8-ROOT").unwrap().category,
            DataCategory::Utf8Group
        );
        let template = model.layout("PART-T").unwrap();
        assert!(template.typedef && !template.allocated);
        assert_eq!(template.length, 4);
        let part = model.layout("PART").unwrap();
        assert!(part.allocated && !part.typedef);
        assert_eq!(part.length, 4);
        assert_eq!(model.layout("PART.CODE-X").unwrap().length, 2);
        assert_eq!(
            model.layout("PART.QTY-X").unwrap().category,
            DataCategory::PackedDecimal
        );
    }

    #[test]
    fn invalid_usage_table_alias_and_type_combinations_fail_closed() {
        for declarations in [
            "01 BAD PIC X COMP",
            "01 BAD PIC 9 COMP-1",
            "01 BAD PIC X POINTER",
            "01 BAD PIC X(2) DYNAMIC",
            "01 BAD OCCURS 2 TIMES PIC X",
            "01 COUNT-X PIC X. 01 ROOT. 05 BAD OCCURS 1 TO 2 TIMES DEPENDING ON COUNT-X PIC X",
            "01 ROOT GROUP-USAGE NATIONAL. 05 BAD PIC X DISPLAY",
            "01 BAD POINTER VALUE 1",
            "01 ROOT. 05 BASE PIC X VALUE 'A'. 05 BAD REDEFINES BASE PIC X VALUE 'B'",
            "01 PART-T TYPEDEF. 05 CODE-X PIC X. 01 PART TYPE PART-T. 05 EXTRA PIC X",
        ] {
            assert!(
                SemanticModel::analyze(&program(declarations), 4096, 128).is_err(),
                "accepted invalid declaration: {declarations}"
            );
        }
    }

    #[test]
    fn file_and_linkage_roots_keep_section_and_reference_identity() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. T. ENVIRONMENT DIVISION. INPUT-OUTPUT SECTION. FILE-CONTROL. SELECT INPUT-FILE ASSIGN TO INPUTDD ORGANIZATION IS INDEXED ACCESS MODE IS RANDOM RECORD KEY IS INPUT-ID FILE STATUS IS FILE-STATUS. SELECT REL-FILE ASSIGN TO RELDD ORGANIZATION IS RELATIVE ACCESS MODE IS RANDOM RELATIVE KEY IS REL-NUM FILE STATUS IS REL-STATUS. DATA DIVISION. FILE SECTION. FD INPUT-FILE. 01 INPUT-RECORD. 05 INPUT-ID PIC X(8). WORKING-STORAGE SECTION. 77 FLAG-X PIC 9. 77 FILE-STATUS PIC XX. 77 REL-NUM PIC 9(4). 77 REL-STATUS PIC XX. LINKAGE SECTION. 01 DFHCOMMAREA. 05 LINK-BYTE PIC X OCCURS 1 TO 8 TIMES DEPENDING ON FLAG-X. PROCEDURE DIVISION. STOP RUN.";
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
        assert_eq!(model.files.len(), 2);
        assert_eq!(model.files[0].select_name, "INPUT-FILE");
        assert_eq!(model.files[0].assignment, "INPUTDD");
        assert_eq!(model.files[0].record_name.as_deref(), Some("INPUT-RECORD"));
        assert_eq!(model.files[0].organization, "INDEXED");
        assert_eq!(model.files[0].access_mode, "RANDOM");
        assert_eq!(model.files[0].record_key.as_deref(), Some("INPUT-ID"));
        assert!(model.files[0].alternate_record_keys.is_empty());
        assert_eq!(model.files[0].file_status.as_deref(), Some("FILE-STATUS"));
        assert_eq!(model.files[1].organization, "RELATIVE");
        assert_eq!(model.files[1].relative_key.as_deref(), Some("REL-NUM"));
    }

    #[test]
    fn all_file_and_data_clause_identities_are_typed_and_validated() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CLAUSES. ENVIRONMENT DIVISION. INPUT-OUTPUT SECTION. FILE-CONTROL. SELECT PRINT-FILE ASSIGN TO PRINTDD. DATA DIVISION. FILE SECTION. FD PRINT-FILE EXTERNAL GLOBAL BLOCK CONTAINS 1 TO 10 CHARACTERS RECORD CONTAINS 1 TO 80 CHARACTERS LABEL RECORDS ARE STANDARD VALUE OF FILE-ID IS 'PRINT' DATA RECORDS ARE PRINT-REC LINAGE IS 60 LINES WITH FOOTING AT 55 LINES AT TOP 3 LINES AT BOTTOM 2 RECORDING MODE IS V CODE-SET IS EBCDIC. 01 PRINT-REC PIC X(80). WORKING-STORAGE SECTION. 01 EDITED PIC ZZ9 BLANK WHEN ZERO. 01 DYN-ITEM PIC X DYNAMIC LENGTH LIMIT IS 100. 01 EXTERNAL-REC PIC X EXTERNAL GLOBAL. 01 JUSTIFIED-ITEM PIC X JUST RIGHT. 01 NATIONAL-GROUP GROUP-USAGE IS NATIONAL. 05 NATIONAL-ITEM PIC N. 01 TABLE-REC. 05 TABLE-ITEM OCCURS 1 TO 3 TIMES DEPENDING ON TABLE-COUNT PIC X. 01 TABLE-COUNT PIC 9. 01 BASE-GROUP. 05 BASE-ITEM PIC X. 05 ALIAS-ITEM REDEFINES BASE-ITEM PIC X. 66 RENAMED-ITEM RENAMES BASE-ITEM THRU ALIAS-ITEM. 77 SIGNED-ITEM PIC S9(4) SIGN IS LEADING SEPARATE CHARACTER SYNC RIGHT USAGE DISPLAY VALUE -1. 01 PRICE-T TYPEDEF PIC 9(5). 01 PRICE TYPE PRICE-T VALUE 1. 01 VOLATILE-ITEM PIC X VOLATILE. PROCEDURE DIVISION. STOP RUN.";
        let model = SemanticModel::analyze(source, 4096, 128).unwrap();
        let file_kinds = model.file_descriptions[0]
            .clauses
            .iter()
            .map(|clause| clause.kind)
            .collect::<BTreeSet<_>>();
        assert_eq!(file_kinds.len(), crate::FILE_DESCRIPTION_CLAUSES.len());
        let data_kinds = model
            .data_descriptions
            .iter()
            .flat_map(|description| description.clauses.iter())
            .filter_map(|clause| match clause.kind {
                CobolClauseKind::Data(kind) => Some(kind),
                CobolClauseKind::File(_) => None,
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(data_kinds.len(), crate::DATA_DESCRIPTION_CLAUSES.len());
    }

    #[test]
    fn nested_program_function_class_and_method_scopes_have_stable_parents() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. OUTER. IDENTIFICATION DIVISION. PROGRAM-ID. INNER. END PROGRAM INNER. IDENTIFICATION DIVISION. FUNCTION-ID. FN. END FUNCTION FN. IDENTIFICATION DIVISION. CLASS-ID. CLS. IDENTIFICATION DIVISION. METHOD-ID. RUN. END METHOD RUN. END CLASS CLS. PROCEDURE DIVISION. STOP RUN.";
        let model = SemanticModel::analyze(source, 1024, 32).unwrap();
        assert_eq!(model.scopes.len(), 5);
        assert_eq!(model.scopes[0].parent, None);
        assert_eq!(model.scopes[1].parent, Some(model.scopes[0].id));
        assert_eq!(model.scopes[2].parent, Some(model.scopes[0].id));
        assert_eq!(model.scopes[4].parent, Some(model.scopes[3].id));
        assert_eq!(model.divisions.len(), 6);

        let literal = SemanticModel::analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. REAL. PROCEDURE DIVISION. DISPLAY 'PROGRAM-ID. FAKE. DATA DIVISION'. STOP RUN.",
            1024,
            16,
        )
        .unwrap();
        assert_eq!(literal.scopes.len(), 1);
        assert_eq!(literal.divisions.len(), 2);
    }
}
