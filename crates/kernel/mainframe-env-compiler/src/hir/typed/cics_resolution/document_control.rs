use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsOutputBinding,
    HirCicsOutputName, HirCicsValue, Resolution, ResolutionFailure, require_writable,
};
use super::{Clauses, cics_integer_value, cics_value, complete_data_reference};
use crate::{CobolUsage, DataCategory, SemanticModel};

pub(super) const ALLOWED_CLAUSES: &[&str] = &[
    "BINARY",
    "DELIMITER",
    "DOCSIZE",
    "DOCTOKEN",
    "FROM",
    "FROMDOC",
    "HOSTCODEPAGE",
    "LENGTH",
    "LISTLENGTH",
    "RESP",
    "RESP2",
    "SYMBOLLIST",
    "TEMPLATE",
    "TEXT",
];
pub(super) const ALLOWED_OPTIONS: &[&str] = &["NOHANDLE", "UNESCAPED"];
pub(super) const DELETE_CLAUSES: &[&str] = &["DOCTOKEN", "RESP", "RESP2"];
pub(super) const INSERT_CLAUSES: &[&str] = &[
    "AT",
    "BINARY",
    "BOOKMARK",
    "DOCSIZE",
    "DOCTOKEN",
    "FROM",
    "FROMDOC",
    "HOSTCODEPAGE",
    "LENGTH",
    "RESP",
    "RESP2",
    "SYMBOL",
    "TEMPLATE",
    "TEXT",
    "TO",
];

pub(super) fn validate_constraints(
    clauses: &Clauses,
    options: &[String],
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<()> {
    if operation == HirCicsOperation::DocumentInsert {
        return validate_insert(clauses, semantic);
    }
    if operation == HirCicsOperation::DocumentDelete {
        let tokens = clauses.get("DOCTOKEN").ok_or_else(|| {
            ResolutionFailure::Invalid("CICS DOCUMENT DELETE requires DOCTOKEN".into())
        })?;
        let token = complete_data_reference(tokens, semantic)?;
        if token.length != 16
            || !matches!(
                token.category,
                DataCategory::Alphabetic | DataCategory::Alphanumeric | DataCategory::Group
            )
        {
            return Err(ResolutionFailure::Invalid(
                "CICS DOCUMENT DELETE DOCTOKEN requires a 16-byte area".into(),
            ));
        }
        return Ok(());
    }
    if operation != HirCicsOperation::DocumentCreate {
        return Ok(());
    }
    if !clauses.contains_key("DOCTOKEN") {
        return Err(ResolutionFailure::Invalid(
            "CICS DOCUMENT CREATE requires DOCTOKEN".into(),
        ));
    }
    let sources = ["FROM", "TEXT", "BINARY", "FROMDOC", "TEMPLATE"]
        .into_iter()
        .filter(|name| clauses.contains_key(*name))
        .count();
    if sources > 1 {
        return Err(ResolutionFailure::Invalid(
            "CICS DOCUMENT CREATE permits at most one content source".into(),
        ));
    }
    let buffered = ["FROM", "TEXT", "BINARY"]
        .into_iter()
        .any(|name| clauses.contains_key(name));
    if clauses.contains_key("LENGTH") != buffered {
        return Err(ResolutionFailure::Invalid(
            "CICS DOCUMENT CREATE FROM, TEXT, or BINARY requires LENGTH".into(),
        ));
    }
    let symbols = clauses.contains_key("SYMBOLLIST");
    if clauses.contains_key("LISTLENGTH") != symbols {
        return Err(ResolutionFailure::Invalid(
            "CICS DOCUMENT CREATE SYMBOLLIST requires LISTLENGTH".into(),
        ));
    }
    if (clauses.contains_key("DELIMITER") || options.iter().any(|name| name == "UNESCAPED"))
        && !symbols
    {
        return Err(ResolutionFailure::Invalid(
            "CICS DOCUMENT CREATE DELIMITER and UNESCAPED require SYMBOLLIST".into(),
        ));
    }
    if clauses.contains_key("HOSTCODEPAGE")
        && !["FROM", "TEXT", "TEMPLATE"]
            .into_iter()
            .any(|name| clauses.contains_key(name))
    {
        return Err(ResolutionFailure::Invalid(
            "CICS DOCUMENT CREATE HOSTCODEPAGE requires FROM, TEXT, or TEMPLATE".into(),
        ));
    }
    let token = complete_data_reference(&clauses["DOCTOKEN"], semantic)?;
    require_writable(&token)?;
    if token.length != 16
        || !matches!(
            token.category,
            DataCategory::Alphabetic | DataCategory::Alphanumeric | DataCategory::Group
        )
    {
        return Err(ResolutionFailure::Invalid(
            "CICS DOCUMENT CREATE DOCTOKEN requires a writable 16-byte area".into(),
        ));
    }
    if let Some(tokens) = clauses.get("DOCSIZE") {
        require_fullword_output(tokens, semantic, "CREATE", "DOCSIZE")?;
    }
    Ok(())
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if operation == HirCicsOperation::DocumentInsert {
        return insert_operands(clauses, semantic);
    }
    if operation == HirCicsOperation::DocumentDelete {
        return Ok(vec![HirCicsNamedOperand {
            name: HirCicsOperandName::DocumentToken,
            value: HirCicsValue::Data(complete_data_reference(&clauses["DOCTOKEN"], semantic)?),
        }]);
    }
    if operation != HirCicsOperation::DocumentCreate {
        return Ok(Vec::new());
    }
    let mut operands = Vec::new();
    for (source, name) in [
        ("FROM", HirCicsOperandName::From),
        ("TEXT", HirCicsOperandName::Text),
        ("BINARY", HirCicsOperandName::Binary),
        ("FROMDOC", HirCicsOperandName::FromDocument),
        ("TEMPLATE", HirCicsOperandName::Template),
        ("SYMBOLLIST", HirCicsOperandName::SymbolList),
        ("DELIMITER", HirCicsOperandName::Delimiter),
        ("HOSTCODEPAGE", HirCicsOperandName::HostCodePage),
    ] {
        if let Some(tokens) = clauses.get(source) {
            let value = cics_value(tokens, semantic)?;
            if matches!(
                source,
                "FROM" | "TEXT" | "BINARY" | "FROMDOC" | "SYMBOLLIST"
            ) && !matches!(value, HirCicsValue::Data(_))
            {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS DOCUMENT CREATE {source} requires a data area"
                )));
            }
            validate_name_shape(source, &value)?;
            operands.push(HirCicsNamedOperand { name, value });
        }
    }
    for (source, name, allow_zero) in [
        ("LENGTH", HirCicsOperandName::Length, true),
        ("LISTLENGTH", HirCicsOperandName::ListLength, false),
    ] {
        if let Some(tokens) = clauses.get(source) {
            let value = length_value(tokens, semantic)?;
            if matches!(value, HirCicsValue::Integer(value) if value < i64::from(!allow_zero)) {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS DOCUMENT CREATE {source} is out of range"
                )));
            }
            operands.push(HirCicsNamedOperand { name, value });
        }
    }
    Ok(operands)
}

pub(super) fn outputs(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    if operation == HirCicsOperation::DocumentInsert {
        return clauses
            .get("DOCSIZE")
            .map(|tokens| {
                Ok(vec![HirCicsOutputBinding {
                    name: HirCicsOutputName::DocumentSize,
                    target: complete_data_reference(tokens, semantic)?,
                }])
            })
            .unwrap_or(Ok(Vec::new()));
    }
    if operation != HirCicsOperation::DocumentCreate {
        return Ok(Vec::new());
    }
    let token = complete_data_reference(&clauses["DOCTOKEN"], semantic)?;
    let mut outputs = vec![HirCicsOutputBinding {
        name: HirCicsOutputName::DocumentToken,
        target: token,
    }];
    if let Some(tokens) = clauses.get("DOCSIZE") {
        outputs.push(HirCicsOutputBinding {
            name: HirCicsOutputName::DocumentSize,
            target: complete_data_reference(tokens, semantic)?,
        });
    }
    Ok(outputs)
}

fn validate_insert(clauses: &Clauses, semantic: &SemanticModel) -> Resolution<()> {
    let tokens = clauses.get("DOCTOKEN").ok_or_else(|| {
        ResolutionFailure::Invalid("CICS DOCUMENT INSERT requires DOCTOKEN".into())
    })?;
    let token = complete_data_reference(tokens, semantic)?;
    if token.length != 16
        || !matches!(
            token.category,
            DataCategory::Alphabetic | DataCategory::Alphanumeric | DataCategory::Group
        )
    {
        return Err(ResolutionFailure::Invalid(
            "CICS DOCUMENT INSERT DOCTOKEN requires a 16-byte area".into(),
        ));
    }
    if let Some(tokens) = clauses.get("FROMDOC") {
        let source = complete_data_reference(tokens, semantic)?;
        if source.length != 16 {
            return Err(ResolutionFailure::Invalid(
                "CICS DOCUMENT INSERT FROMDOC requires a 16-byte area".into(),
            ));
        }
    }
    let sources = ["FROM", "TEXT", "BINARY", "FROMDOC", "TEMPLATE", "SYMBOL"]
        .into_iter()
        .filter(|name| clauses.contains_key(*name))
        .count();
    if sources > 1 || sources == 0 && !clauses.contains_key("BOOKMARK") {
        return Err(ResolutionFailure::Invalid(
            "CICS DOCUMENT INSERT requires one content source or BOOKMARK".into(),
        ));
    }
    let buffered = ["FROM", "TEXT", "BINARY"]
        .into_iter()
        .any(|name| clauses.contains_key(name));
    if clauses.contains_key("LENGTH") != buffered {
        return Err(ResolutionFailure::Invalid(
            "CICS DOCUMENT INSERT FROM, TEXT, or BINARY requires LENGTH".into(),
        ));
    }
    if clauses.contains_key("HOSTCODEPAGE")
        && !["TEXT", "SYMBOL", "TEMPLATE"]
            .into_iter()
            .any(|name| clauses.contains_key(name))
    {
        return Err(ResolutionFailure::Invalid(
            "CICS DOCUMENT INSERT HOSTCODEPAGE requires TEXT, SYMBOL, or TEMPLATE".into(),
        ));
    }
    if let Some(tokens) = clauses.get("DOCSIZE") {
        require_fullword_output(tokens, semantic, "INSERT", "DOCSIZE")?;
    }
    for (name, maximum) in [
        ("SYMBOL", 32),
        ("TEMPLATE", 48),
        ("BOOKMARK", 16),
        ("AT", 16),
        ("TO", 16),
        ("HOSTCODEPAGE", 8),
    ] {
        if let Some(tokens) = clauses.get(name) {
            let value = cics_value(tokens, semantic)?;
            let length = match value {
                HirCicsValue::Literal(value) => value.len(),
                HirCicsValue::Data(reference) => reference.length,
                _ => 0,
            };
            if !(1..=maximum).contains(&length) {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS DOCUMENT INSERT {name} requires 1-{maximum} bytes"
                )));
            }
        }
    }
    Ok(())
}

fn insert_operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let mut operands = vec![HirCicsNamedOperand {
        name: HirCicsOperandName::DocumentToken,
        value: HirCicsValue::Data(complete_data_reference(&clauses["DOCTOKEN"], semantic)?),
    }];
    for (source, name) in [
        ("FROM", HirCicsOperandName::From),
        ("TEXT", HirCicsOperandName::Text),
        ("BINARY", HirCicsOperandName::Binary),
        ("FROMDOC", HirCicsOperandName::FromDocument),
        ("TEMPLATE", HirCicsOperandName::Template),
        ("SYMBOL", HirCicsOperandName::Symbol),
        ("BOOKMARK", HirCicsOperandName::Bookmark),
        ("AT", HirCicsOperandName::AtBookmark),
        ("TO", HirCicsOperandName::ToBookmark),
        ("HOSTCODEPAGE", HirCicsOperandName::HostCodePage),
    ] {
        if let Some(tokens) = clauses.get(source) {
            let value = cics_value(tokens, semantic)?;
            if matches!(source, "FROM" | "TEXT" | "BINARY" | "FROMDOC")
                && !matches!(value, HirCicsValue::Data(_))
            {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS DOCUMENT INSERT {source} requires a data area"
                )));
            }
            operands.push(HirCicsNamedOperand { name, value });
        }
    }
    if let Some(tokens) = clauses.get("LENGTH") {
        let value = length_value(tokens, semantic)?;
        if matches!(value, HirCicsValue::Integer(value) if value < 0) {
            return Err(ResolutionFailure::Invalid(
                "CICS DOCUMENT INSERT LENGTH is out of range".into(),
            ));
        }
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::Length,
            value,
        });
    }
    Ok(operands)
}

fn length_value(tokens: &[String], semantic: &SemanticModel) -> Resolution<HirCicsValue> {
    if tokens
        .first()
        .is_some_and(|token| token.eq_ignore_ascii_case("LENGTH"))
        && tokens
            .get(1)
            .is_some_and(|token| token.eq_ignore_ascii_case("OF"))
    {
        return Ok(HirCicsValue::LengthOf(complete_data_reference(
            &tokens[2..],
            semantic,
        )?));
    }
    let value = cics_integer_value(tokens, semantic)?;
    if let HirCicsValue::Data(reference) = &value
        && (reference.usage != CobolUsage::Binary || reference.length != 4 || reference.scale != 0)
    {
        return Err(ResolutionFailure::Invalid(
            "CICS document lengths require fullword binary storage".into(),
        ));
    }
    Ok(value)
}

fn require_fullword_output(
    tokens: &[String],
    semantic: &SemanticModel,
    operation: &str,
    name: &str,
) -> Resolution<()> {
    let reference = complete_data_reference(tokens, semantic)?;
    require_writable(&reference)?;
    if reference.usage != CobolUsage::Binary || reference.length != 4 || reference.scale != 0 {
        return Err(ResolutionFailure::Invalid(format!(
            "CICS DOCUMENT {operation} {name} requires writable fullword binary storage"
        )));
    }
    Ok(())
}

fn validate_name_shape(name: &str, value: &HirCicsValue) -> Resolution<()> {
    let maximum = match name {
        "TEMPLATE" => 48,
        "HOSTCODEPAGE" => 8,
        "DELIMITER" => 1,
        _ => return Ok(()),
    };
    let length = match value {
        HirCicsValue::Literal(value) => value.len(),
        HirCicsValue::Data(reference) => reference.length,
        HirCicsValue::Integer(_) | HirCicsValue::LengthOf(_) => 0,
    };
    if !(1..=maximum).contains(&length) {
        return Err(ResolutionFailure::Invalid(format!(
            "CICS DOCUMENT CREATE {name} requires 1-{maximum} bytes"
        )));
    }
    Ok(())
}
