use super::{CobolFileBinding, CobolLayout, DataCategory, SemanticProblem, StorageSection};
use crate::syntax::{SourceOrigin, SourceSpan};
use crate::{
    IntrinsicArgumentClass, IntrinsicFunctionKind, IntrinsicResultRule, IntrinsicSignature,
    SPECIAL_REGISTERS, SpecialRegisterKind, SpecialRegisterLengthKind, SpecialRegisterOperand,
    SpecialRegisterValueType, intrinsic_function_named, special_register_named,
};
use std::collections::BTreeSet;
use std::ops::Range;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum IntrinsicValueType {
    Alphabetic,
    Alphanumeric,
    Dbcs,
    Integer,
    Numeric,
    National,
    Utf8,
    Other,
    Keyword,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CobolIntrinsicArgument {
    pub text: String,
    pub value_type: IntrinsicValueType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CobolIntrinsicCall {
    pub kind: IntrinsicFunctionKind,
    pub arguments: Vec<CobolIntrinsicArgument>,
    pub result_type: IntrinsicValueType,
    pub fixed_length: Option<usize>,
    pub runtime_supported: bool,
    pub source: Vec<SourceSpan>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CobolSpecialRegisterReference {
    pub kind: SpecialRegisterKind,
    pub operand: Option<String>,
    pub value_type: IntrinsicValueType,
    pub length: Option<usize>,
    pub writable: bool,
    pub source: Vec<SourceSpan>,
}

pub(super) fn analyze(
    source: &str,
    origins: &[SourceOrigin],
    layouts: &[CobolLayout],
    files: &[CobolFileBinding],
    user_functions: &BTreeSet<String>,
    pointer_bytes: usize,
) -> Result<(Vec<CobolIntrinsicCall>, Vec<CobolSpecialRegisterReference>), SemanticProblem> {
    Ok((
        intrinsic_calls(source, origins, layouts, pointer_bytes, user_functions)?,
        special_registers(source, origins, layouts, files, pointer_bytes)?,
    ))
}

fn intrinsic_calls(
    source: &str,
    origins: &[SourceOrigin],
    layouts: &[CobolLayout],
    pointer_bytes: usize,
    user_functions: &BTreeSet<String>,
) -> Result<Vec<CobolIntrinsicCall>, SemanticProblem> {
    let mut calls = Vec::new();
    let mut cursor = 0usize;
    while let Some(start) = next_phrase(source, cursor, "FUNCTION") {
        let name_start = skip_space(source, start + "FUNCTION".len());
        let name_end = word_end(source, name_start);
        if name_end == name_start {
            return Err(SemanticProblem::InvalidIntrinsic(
                "FUNCTION name missing".into(),
            ));
        }
        let name = source[name_start..name_end].to_ascii_uppercase();
        cursor = name_end;
        let Some(descriptor) = intrinsic_function_named(&name) else {
            if user_functions.contains(&name) {
                continue;
            }
            return Err(SemanticProblem::InvalidIntrinsic(format!(
                "unknown intrinsic function {name}"
            )));
        };
        let after_name = skip_space(source, name_end);
        let (argument_ranges, end) = if source.as_bytes().get(after_name) == Some(&b'(') {
            let close = matching_close(source, after_name).ok_or_else(|| {
                SemanticProblem::InvalidIntrinsic(format!("unterminated FUNCTION {name}"))
            })?;
            (split_arguments(source, after_name + 1..close), close + 1)
        } else {
            (Vec::new(), name_end)
        };
        let arguments = argument_ranges
            .into_iter()
            .map(|range| {
                let text = source[range].trim().to_string();
                let value_type = argument_type(&text, layouts, pointer_bytes, user_functions)?;
                Ok(CobolIntrinsicArgument { text, value_type })
            })
            .collect::<Result<Vec<_>, SemanticProblem>>()?;
        let signature = descriptor
            .signatures
            .iter()
            .find(|signature| signature_matches(signature, &arguments))
            .ok_or_else(|| {
                SemanticProblem::InvalidIntrinsic(format!(
                    "{} argument signature is invalid",
                    descriptor.name
                ))
            })?;
        validate_function_context(descriptor.kind, descriptor.literal_arguments, &arguments)?;
        calls.push(CobolIntrinsicCall {
            kind: descriptor.kind,
            result_type: result_type(signature.result, &arguments)?,
            fixed_length: descriptor.fixed_length,
            runtime_supported: descriptor.runtime_supported,
            arguments,
            source: source_spans(origins, start..end),
        });
    }
    Ok(calls)
}

fn argument_type(
    text: &str,
    layouts: &[CobolLayout],
    pointer_bytes: usize,
    user_functions: &BTreeSet<String>,
) -> Result<IntrinsicValueType, SemanticProblem> {
    let text = text.trim();
    let upper = text.to_ascii_uppercase();
    if matches!(upper.as_str(), "LEADING" | "TRAILING") {
        return Ok(IntrinsicValueType::Keyword);
    }
    if upper.starts_with("FUNCTION ") {
        return infer_nested_function(text, layouts, pointer_bytes, user_functions);
    }
    if quoted_literal(text) {
        return Ok(if upper.starts_with("N'") || upper.starts_with("N\"") {
            IntrinsicValueType::National
        } else if upper.starts_with("U'") || upper.starts_with("U\"") {
            IntrinsicValueType::Utf8
        } else if upper.starts_with("G'") || upper.starts_with("G\"") {
            IntrinsicValueType::Dbcs
        } else {
            IntrinsicValueType::Alphanumeric
        });
    }
    if text.parse::<i128>().is_ok() {
        return Ok(IntrinsicValueType::Integer);
    }
    if text.parse::<f64>().is_ok() {
        return Ok(IntrinsicValueType::Numeric);
    }
    if let Some(register) = special_register_named(&upper) {
        return Ok(register_value_type(register.value_type));
    }
    let candidates = layouts
        .iter()
        .filter(|layout| {
            layout.name == upper
                || layout.qualified_name == upper
                || qualified_reference_matches(&upper, &layout.qualified_name)
        })
        .collect::<Vec<_>>();
    match candidates.as_slice() {
        [layout] => Ok(layout_value_type(layout)),
        [] => Err(SemanticProblem::InvalidIntrinsic(format!(
            "unresolved intrinsic argument {text}"
        ))),
        _ => Err(SemanticProblem::InvalidIntrinsic(format!(
            "ambiguous intrinsic argument {text}"
        ))),
    }
}

fn infer_nested_function(
    text: &str,
    layouts: &[CobolLayout],
    pointer_bytes: usize,
    user_functions: &BTreeSet<String>,
) -> Result<IntrinsicValueType, SemanticProblem> {
    let name_start = skip_space(text, "FUNCTION".len());
    let name_end = word_end(text, name_start);
    let name = text[name_start..name_end].to_ascii_uppercase();
    if user_functions.contains(&name) {
        return Err(SemanticProblem::InvalidIntrinsic(
            "user-defined function result metadata is unavailable".into(),
        ));
    }
    let descriptor = intrinsic_function_named(&name)
        .ok_or_else(|| SemanticProblem::InvalidIntrinsic(format!("unknown intrinsic {name}")))?;
    let open = skip_space(text, name_end);
    let ranges = if text.as_bytes().get(open) == Some(&b'(') {
        let close = matching_close(text, open).ok_or_else(|| {
            SemanticProblem::InvalidIntrinsic("unterminated nested function".into())
        })?;
        split_arguments(text, open + 1..close)
    } else {
        Vec::new()
    };
    let arguments = ranges
        .into_iter()
        .map(|range| {
            let argument = text[range].trim().to_string();
            Ok(CobolIntrinsicArgument {
                value_type: argument_type(&argument, layouts, pointer_bytes, user_functions)?,
                text: argument,
            })
        })
        .collect::<Result<Vec<_>, SemanticProblem>>()?;
    let signature = descriptor
        .signatures
        .iter()
        .find(|signature| signature_matches(signature, &arguments))
        .ok_or_else(|| SemanticProblem::InvalidIntrinsic(format!("invalid nested {name}")))?;
    validate_function_context(descriptor.kind, descriptor.literal_arguments, &arguments)?;
    result_type(signature.result, &arguments)
}

fn validate_function_context(
    kind: IntrinsicFunctionKind,
    literal_arguments: &[usize],
    arguments: &[CobolIntrinsicArgument],
) -> Result<(), SemanticProblem> {
    if literal_arguments.iter().any(|index| {
        arguments
            .get(*index)
            .is_none_or(|argument| !quoted_literal(&argument.text))
    }) {
        return Err(SemanticProblem::InvalidIntrinsic(
            "format argument must be a literal".into(),
        ));
    }
    if matches!(
        kind,
        IntrinsicFunctionKind::IntegerOfFormattedDate
            | IntrinsicFunctionKind::SecondsFromFormattedTime
            | IntrinsicFunctionKind::TestFormattedDatetime
    ) && arguments.len() >= 2
        && arguments[0].value_type != arguments[1].value_type
    {
        return Err(SemanticProblem::InvalidIntrinsic(
            "format and value arguments must have the same class".into(),
        ));
    }
    Ok(())
}

fn signature_matches(signature: &IntrinsicSignature, arguments: &[CobolIntrinsicArgument]) -> bool {
    let required = signature.arguments.len();
    if (!signature.variadic && arguments.len() != required)
        || (signature.variadic && arguments.len() < required)
        || (signature.variadic && required == 0)
    {
        return false;
    }
    if !arguments.iter().enumerate().all(|(index, argument)| {
        let position = index.min(required.saturating_sub(1));
        signature.arguments[position]
            .iter()
            .any(|class| argument_matches(*class, argument.value_type))
    }) {
        return false;
    }
    !signature.homogeneous
        || arguments.first().is_none_or(|first| {
            arguments
                .iter()
                .all(|argument| homogeneous(first.value_type, argument.value_type))
        })
}

const fn argument_matches(class: IntrinsicArgumentClass, value: IntrinsicValueType) -> bool {
    match class {
        IntrinsicArgumentClass::Alphabetic => matches!(value, IntrinsicValueType::Alphabetic),
        IntrinsicArgumentClass::Alphanumeric => matches!(value, IntrinsicValueType::Alphanumeric),
        IntrinsicArgumentClass::Dbcs => matches!(value, IntrinsicValueType::Dbcs),
        IntrinsicArgumentClass::Integer => matches!(value, IntrinsicValueType::Integer),
        IntrinsicArgumentClass::Numeric => {
            matches!(
                value,
                IntrinsicValueType::Integer | IntrinsicValueType::Numeric
            )
        }
        IntrinsicArgumentClass::National => matches!(value, IntrinsicValueType::National),
        IntrinsicArgumentClass::Utf8 => matches!(value, IntrinsicValueType::Utf8),
        IntrinsicArgumentClass::Other => matches!(value, IntrinsicValueType::Other),
        IntrinsicArgumentClass::Keyword => matches!(value, IntrinsicValueType::Keyword),
    }
}

fn homogeneous(left: IntrinsicValueType, right: IntrinsicValueType) -> bool {
    left == right
        || matches!(
            (left, right),
            (IntrinsicValueType::Integer, IntrinsicValueType::Numeric)
                | (IntrinsicValueType::Numeric, IntrinsicValueType::Integer)
        )
}

fn result_type(
    rule: IntrinsicResultRule,
    arguments: &[CobolIntrinsicArgument],
) -> Result<IntrinsicValueType, SemanticProblem> {
    let first = || {
        arguments
            .first()
            .map(|argument| argument.value_type)
            .ok_or_else(|| SemanticProblem::InvalidIntrinsic("result requires an argument".into()))
    };
    Ok(match rule {
        IntrinsicResultRule::Integer => IntrinsicValueType::Integer,
        IntrinsicResultRule::Numeric => IntrinsicValueType::Numeric,
        IntrinsicResultRule::Alphanumeric => IntrinsicValueType::Alphanumeric,
        IntrinsicResultRule::National => IntrinsicValueType::National,
        IntrinsicResultRule::Utf8 => IntrinsicValueType::Utf8,
        IntrinsicResultRule::PreserveNumeric => first()?,
        IntrinsicResultRule::Content => match first()? {
            IntrinsicValueType::Alphabetic => IntrinsicValueType::Alphanumeric,
            value => value,
        },
        IntrinsicResultRule::StringFirst => match first()? {
            IntrinsicValueType::Alphabetic => IntrinsicValueType::Alphanumeric,
            value => value,
        },
        IntrinsicResultRule::Comparable => match first()? {
            IntrinsicValueType::Alphabetic => IntrinsicValueType::Alphanumeric,
            IntrinsicValueType::Integer
                if arguments
                    .iter()
                    .any(|argument| argument.value_type == IntrinsicValueType::Numeric) =>
            {
                IntrinsicValueType::Numeric
            }
            value => value,
        },
        IntrinsicResultRule::RangeSum => {
            if arguments
                .iter()
                .all(|argument| argument.value_type == IntrinsicValueType::Integer)
            {
                IntrinsicValueType::Integer
            } else {
                IntrinsicValueType::Numeric
            }
        }
    })
}

fn special_registers(
    source: &str,
    origins: &[SourceOrigin],
    layouts: &[CobolLayout],
    files: &[CobolFileBinding],
    pointer_bytes: usize,
) -> Result<Vec<CobolSpecialRegisterReference>, SemanticProblem> {
    let mut descriptors = SPECIAL_REGISTERS.iter().collect::<Vec<_>>();
    descriptors.sort_by_key(|descriptor| std::cmp::Reverse(descriptor.name.len()));
    let mut references = Vec::new();
    let mut claimed = Vec::<Range<usize>>::new();
    for descriptor in descriptors {
        let mut cursor = 0usize;
        while let Some(start) = next_phrase(source, cursor, descriptor.name) {
            let phrase_end = start + descriptor.name.len();
            cursor = phrase_end;
            if claimed
                .iter()
                .any(|range| range.start <= start && start < range.end)
                || preceded_by_function(source, start)
            {
                continue;
            }
            let (operand, end) = match descriptor.operand {
                SpecialRegisterOperand::DataItem => {
                    let operand_start = skip_space(source, phrase_end);
                    let operand_end = word_end(source, operand_start);
                    if operand_end == operand_start {
                        return Err(SemanticProblem::InvalidSpecialRegister(format!(
                            "{} requires a data item",
                            descriptor.name
                        )));
                    }
                    let operand = source[operand_start..operand_end].to_ascii_uppercase();
                    let layout = unique_layout(&operand, layouts)?;
                    if descriptor.kind == SpecialRegisterKind::AddressOf
                        && layout.section == StorageSection::File
                    {
                        return Err(SemanticProblem::InvalidSpecialRegister(
                            "ADDRESS OF cannot name FILE SECTION data".into(),
                        ));
                    }
                    (Some(operand), operand_end)
                }
                SpecialRegisterOperand::File => {
                    let after = skip_space(source, phrase_end);
                    let after = if source[after..].to_ascii_uppercase().starts_with("OF ") {
                        skip_space(source, after + 2)
                    } else {
                        after
                    };
                    let file_end = word_end(source, after);
                    let file =
                        (file_end > after).then(|| source[after..file_end].to_ascii_uppercase());
                    if let Some(file) = &file
                        && !files.iter().any(|binding| binding.select_name == *file)
                    {
                        return Err(SemanticProblem::InvalidSpecialRegister(format!(
                            "unknown LINAGE file {file}"
                        )));
                    }
                    (file, file_end.max(phrase_end))
                }
                SpecialRegisterOperand::None | SpecialRegisterOperand::DebugContext => {
                    (None, phrase_end)
                }
            };
            if is_receiving_position(source, start) && !descriptor.writable {
                return Err(SemanticProblem::InvalidSpecialRegister(format!(
                    "{} is not a receiving item",
                    descriptor.name
                )));
            }
            claimed.push(start..end);
            references.push(CobolSpecialRegisterReference {
                kind: descriptor.kind,
                operand,
                value_type: register_value_type(descriptor.value_type),
                length: match descriptor.length_kind {
                    SpecialRegisterLengthKind::Fixed => descriptor.fixed_length,
                    SpecialRegisterLengthKind::Lp => Some(pointer_bytes),
                    SpecialRegisterLengthKind::Dynamic | SpecialRegisterLengthKind::Dependent => {
                        None
                    }
                },
                writable: descriptor.writable,
                source: source_spans(origins, start..end),
            });
        }
    }
    references.sort_by_key(|reference| {
        reference
            .source
            .first()
            .map_or(usize::MAX, |span| span.source_start)
    });
    Ok(references)
}

fn unique_layout<'a>(
    name: &str,
    layouts: &'a [CobolLayout],
) -> Result<&'a CobolLayout, SemanticProblem> {
    let candidates = layouts
        .iter()
        .filter(|layout| layout.name == name || layout.qualified_name == name)
        .collect::<Vec<_>>();
    match candidates.as_slice() {
        [layout] => Ok(*layout),
        [] => Err(SemanticProblem::InvalidSpecialRegister(format!(
            "unknown data item {name}"
        ))),
        _ => Err(SemanticProblem::InvalidSpecialRegister(format!(
            "ambiguous data item {name}"
        ))),
    }
}

const fn register_value_type(value: SpecialRegisterValueType) -> IntrinsicValueType {
    match value {
        SpecialRegisterValueType::Alphanumeric | SpecialRegisterValueType::Group => {
            IntrinsicValueType::Alphanumeric
        }
        SpecialRegisterValueType::Integer => IntrinsicValueType::Integer,
        SpecialRegisterValueType::National => IntrinsicValueType::National,
        SpecialRegisterValueType::Other => IntrinsicValueType::Other,
    }
}

const fn layout_value_type(layout: &CobolLayout) -> IntrinsicValueType {
    match layout.category {
        DataCategory::Alphabetic => IntrinsicValueType::Alphabetic,
        DataCategory::Alphanumeric | DataCategory::AlphanumericEdited | DataCategory::Group => {
            IntrinsicValueType::Alphanumeric
        }
        DataCategory::Dbcs => IntrinsicValueType::Dbcs,
        DataCategory::National | DataCategory::NationalEdited | DataCategory::NationalGroup => {
            IntrinsicValueType::National
        }
        DataCategory::Utf8 | DataCategory::Utf8Group => IntrinsicValueType::Utf8,
        DataCategory::NumericDisplay
        | DataCategory::NumericEdited
        | DataCategory::PackedDecimal
        | DataCategory::Binary
        | DataCategory::FloatShort
        | DataCategory::FloatLong => {
            if layout.scale == 0
                && !matches!(
                    layout.category,
                    DataCategory::FloatShort | DataCategory::FloatLong
                )
            {
                IntrinsicValueType::Integer
            } else {
                IntrinsicValueType::Numeric
            }
        }
        DataCategory::Index
        | DataCategory::Pointer
        | DataCategory::Pointer32
        | DataCategory::ProcedurePointer
        | DataCategory::FunctionPointer
        | DataCategory::ObjectReference
        | DataCategory::Condition
        | DataCategory::Rename => IntrinsicValueType::Other,
    }
}

fn split_arguments(source: &str, range: Range<usize>) -> Vec<Range<usize>> {
    let bytes = source.as_bytes();
    let mut result = Vec::new();
    let mut start = range.start;
    let mut depth = 0usize;
    let mut quote = None;
    for index in range.clone() {
        let byte = bytes[index];
        if matches!(byte, b'\'' | b'"') {
            if quote == Some(byte) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(byte);
            }
        } else if quote.is_none() {
            match byte {
                b'(' => depth += 1,
                b')' => depth = depth.saturating_sub(1),
                b',' if depth == 0 => {
                    if let Some(argument) = trim_range(source, start..index) {
                        result.push(argument);
                    }
                    start = index + 1;
                }
                _ => {}
            }
        }
    }
    if let Some(argument) = trim_range(source, start..range.end) {
        result.push(argument);
    }
    result
}

fn trim_range(source: &str, mut range: Range<usize>) -> Option<Range<usize>> {
    while range.start < range.end && source.as_bytes()[range.start].is_ascii_whitespace() {
        range.start += 1;
    }
    while range.start < range.end && source.as_bytes()[range.end - 1].is_ascii_whitespace() {
        range.end -= 1;
    }
    (range.start < range.end).then_some(range)
}

fn matching_close(source: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote = None;
    for (relative, byte) in source.as_bytes()[open..].iter().copied().enumerate() {
        if matches!(byte, b'\'' | b'"') {
            if quote == Some(byte) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(byte);
            }
            continue;
        }
        if quote.is_some() {
            continue;
        }
        if byte == b'(' {
            depth += 1;
        } else if byte == b')' {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(open + relative);
            }
        }
    }
    None
}

fn next_phrase(source: &str, from: usize, phrase: &str) -> Option<usize> {
    let upper = source[from..].to_ascii_uppercase();
    let mut relative = 0usize;
    while let Some(found) = upper[relative..].find(phrase) {
        let start = from + relative + found;
        let end = start + phrase.len();
        if boundary(source, start, end)
            && !inside_quote(source, start)
            && !inside_line_comment(source, start)
        {
            return Some(start);
        }
        relative += found + 1;
    }
    None
}

fn boundary(source: &str, start: usize, end: usize) -> bool {
    let is_word = |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_');
    source
        .as_bytes()
        .get(start.wrapping_sub(1))
        .is_none_or(|byte| !is_word(*byte))
        && source
            .as_bytes()
            .get(end)
            .is_none_or(|byte| !is_word(*byte))
}

fn inside_quote(source: &str, position: usize) -> bool {
    let mut quote = None;
    for byte in source.as_bytes()[..position].iter().copied() {
        if matches!(byte, b'\'' | b'"') {
            if quote == Some(byte) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(byte);
            }
        }
    }
    quote.is_some()
}

fn inside_line_comment(source: &str, position: usize) -> bool {
    let start = source[..position].rfind('\n').map_or(0, |index| index + 1);
    source[start..position].contains("*>")
}

fn skip_space(source: &str, mut index: usize) -> usize {
    while source
        .as_bytes()
        .get(index)
        .is_some_and(u8::is_ascii_whitespace)
    {
        index += 1;
    }
    index
}

fn word_end(source: &str, mut index: usize) -> usize {
    while source
        .as_bytes()
        .get(index)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'_'))
    {
        index += 1;
    }
    index
}

fn quoted_literal(text: &str) -> bool {
    let bytes = text.as_bytes();
    matches!(bytes.first(), Some(b'\'' | b'"'))
        || (bytes.len() > 1
            && matches!(bytes[0].to_ascii_uppercase(), b'N' | b'U' | b'G' | b'X')
            && matches!(bytes[1], b'\'' | b'"'))
}

fn qualified_reference_matches(reference: &str, qualified: &str) -> bool {
    let parts = reference
        .split_whitespace()
        .filter(|word| !matches!(*word, "OF" | "IN"))
        .collect::<Vec<_>>();
    let qualified = qualified.split('.').rev().collect::<Vec<_>>();
    !parts.is_empty()
        && parts.len() <= qualified.len()
        && parts
            .iter()
            .zip(qualified)
            .all(|(left, right)| *left == right)
}

fn preceded_by_function(source: &str, start: usize) -> bool {
    source[..start]
        .trim_end()
        .rsplit(|character: char| {
            character.is_ascii_whitespace() || matches!(character, '.' | ',' | '(')
        })
        .next()
        .is_some_and(|word| word.eq_ignore_ascii_case("FUNCTION"))
}

fn is_receiving_position(source: &str, start: usize) -> bool {
    let sentence_start = source[..start].rfind('.').map_or(0, |index| index + 1);
    let prefix = source[sentence_start..start].to_ascii_uppercase();
    prefix.rfind(" TO ").is_some_and(|to| {
        prefix[..to].trim_start().starts_with("MOVE ")
            || prefix[..to].trim_start().starts_with("SET ")
            || prefix[..to].trim_start().starts_with("ADD ")
    })
}

fn source_spans(origins: &[SourceOrigin], range: Range<usize>) -> Vec<SourceSpan> {
    crate::syntax::source_spans(origins, range)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::SemanticModel;

    fn program(procedure: &str) -> String {
        format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. FUNCTIONS. DATA DIVISION. WORKING-STORAGE SECTION. 01 TEXT PIC X(8). 01 INT PIC 9(4) BINARY. 01 NUM PIC 9(4)V99 COMP-3. 01 PTR POINTER. LINKAGE SECTION. 01 LINK-X PIC X. PROCEDURE DIVISION. {procedure}. STOP RUN."
        )
    }

    #[test]
    fn intrinsic_calls_infer_overloads_and_reject_bad_signatures() {
        let source = program(
            "MOVE FUNCTION ABS(NUM) TO NUM. MOVE FUNCTION UPPER-CASE(TEXT) TO TEXT. MOVE FUNCTION UUID4 TO TEXT",
        );
        let model = SemanticModel::analyze(&source, 4096, 128).unwrap();
        assert_eq!(model.intrinsic_calls.len(), 3);
        assert_eq!(
            model.intrinsic_calls[0].result_type,
            IntrinsicValueType::Numeric
        );
        assert_eq!(
            model.intrinsic_calls[1].result_type,
            IntrinsicValueType::Alphanumeric
        );
        assert_eq!(model.intrinsic_calls[2].fixed_length, Some(36));
        assert_eq!(
            model.execution_incomplete_intrinsics(),
            BTreeSet::from([IntrinsicFunctionKind::Abs, IntrinsicFunctionKind::Uuid4])
        );

        for invalid in [
            "MOVE FUNCTION ABS(TEXT) TO NUM",
            "MOVE FUNCTION MOD(INT) TO INT",
            "MOVE FUNCTION CURRENT-DATE(TEXT) TO TEXT",
            "MOVE FUNCTION TRIM(NUM, LEADING) TO TEXT",
        ] {
            assert!(SemanticModel::analyze(&program(invalid), 4096, 128).is_err());
        }
    }

    #[test]
    fn special_register_operands_types_and_receiving_rules_are_explicit() {
        let source = program(
            "MOVE LENGTH OF TEXT TO INT. SET ADDRESS OF LINK-X TO PTR. MOVE 4 TO RETURN-CODE. DISPLAY WHEN-COMPILED",
        );
        let model = SemanticModel::analyze(&source, 4096, 128).unwrap();
        assert_eq!(model.special_registers.len(), 4);
        assert!(model.special_registers.iter().any(|reference| {
            reference.kind == SpecialRegisterKind::AddressOf
                && reference.operand.as_deref() == Some("LINK-X")
                && reference.value_type == IntrinsicValueType::Other
                && reference.length == Some(4)
        }));
        assert!(model.special_registers.iter().any(|reference| {
            reference.kind == SpecialRegisterKind::LengthOf
                && reference.value_type == IntrinsicValueType::Integer
        }));
        assert!(SemanticModel::analyze(&program("MOVE 'X' TO WHEN-COMPILED"), 4096, 128).is_err());
    }
}
