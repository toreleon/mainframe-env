use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsOption, HirCicsValue,
    Resolution, ResolutionFailure, require_writable,
};
use super::{Clauses, complete_data_reference};
use crate::{CobolUsage, DataCategory, SemanticModel};

pub(super) fn allowed_clauses(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::SpoolClose => &["TOKEN", "RESP", "RESP2"],
        HirCicsOperation::SpoolOpenInput => &["TOKEN", "USERID", "CLASS", "RESP", "RESP2"],
        HirCicsOperation::SpoolOpenOutput => &[
            "TOKEN",
            "USERID",
            "NODE",
            "CLASS",
            "RECORDLENGTH",
            "OUTDESCR",
            "RESP",
            "RESP2",
        ],
        _ => panic!("non-spool CICS operation reached spool clause validation"),
    }
}

pub(super) fn allowed_options(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::SpoolClose => &["DELETE", "KEEP", "NOHANDLE"],
        HirCicsOperation::SpoolOpenInput => &["NOHANDLE"],
        HirCicsOperation::SpoolOpenOutput => &["NOHANDLE", "NOCC", "ASA", "MCC", "PRINT", "PUNCH"],
        _ => panic!("non-spool CICS operation reached spool option validation"),
    }
}

pub(super) fn required(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::SpoolClose => &["TOKEN"],
        HirCicsOperation::SpoolOpenInput => &["TOKEN", "USERID"],
        HirCicsOperation::SpoolOpenOutput => &["TOKEN", "USERID", "NODE"],
        _ => panic!("non-spool CICS operation reached spool required validation"),
    }
}

pub(super) fn option(name: &str) -> Option<HirCicsOption> {
    match name {
        "KEEP" => Some(HirCicsOption::SpoolKeep),
        "DELETE" => Some(HirCicsOption::SpoolDelete),
        "NOCC" => Some(HirCicsOption::SpoolNoCc),
        "ASA" => Some(HirCicsOption::SpoolAsa),
        "MCC" => Some(HirCicsOption::SpoolMcc),
        "PRINT" => Some(HirCicsOption::SpoolPrint),
        "PUNCH" => Some(HirCicsOption::SpoolPunch),
        _ => None,
    }
}

pub(super) fn operands(
    clauses: &Clauses,
    options: &[String],
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if !matches!(
        operation,
        HirCicsOperation::SpoolClose
            | HirCicsOperation::SpoolOpenInput
            | HirCicsOperation::SpoolOpenOutput
    ) {
        return Ok(Vec::new());
    }
    if !clauses.contains_key("RESP") && !options.iter().any(|option| option == "NOHANDLE") {
        return Err(ResolutionFailure::Invalid(format!(
            "CICS {operation:?} requires RESP or NOHANDLE"
        )));
    }
    if matches!(
        operation,
        HirCicsOperation::SpoolOpenInput | HirCicsOperation::SpoolOpenOutput
    ) {
        let token = complete_data_reference(&clauses["TOKEN"], semantic)?;
        require_writable(&token)?;
        if token.length != 8
            || !matches!(
                token.category,
                DataCategory::Alphabetic | DataCategory::Alphanumeric
            )
        {
            return Err(ResolutionFailure::Invalid(
                "CICS SPOOLOPEN TOKEN requires a writable 8-character data area".into(),
            ));
        }
        let mut operands = vec![text_operand(
            "USERID",
            HirCicsOperandName::SpoolUserId,
            8,
            &clauses["USERID"],
            semantic,
            operation == HirCicsOperation::SpoolOpenOutput,
        )?];
        if operation == HirCicsOperation::SpoolOpenOutput {
            operands.push(text_operand(
                "NODE",
                HirCicsOperandName::SpoolNode,
                8,
                &clauses["NODE"],
                semantic,
                true,
            )?);
            let carriage = options
                .iter()
                .filter(|option| matches!(option.as_str(), "NOCC" | "ASA" | "MCC"))
                .count();
            if carriage > 1
                || options.iter().any(|option| option == "PRINT")
                    && options.iter().any(|option| option == "PUNCH")
            {
                return Err(ResolutionFailure::Invalid(
                    "CICS SPOOLOPEN OUTPUT format options conflict".into(),
                ));
            }
            if let Some(length) = clauses.get("RECORDLENGTH") {
                let length = complete_data_reference(length, semantic)?;
                if length.category != DataCategory::Binary || length.length != 2 {
                    return Err(ResolutionFailure::Invalid(
                        "CICS SPOOLOPEN OUTPUT RECORDLENGTH requires halfword binary storage"
                            .into(),
                    ));
                }
                operands.push(HirCicsNamedOperand {
                    name: HirCicsOperandName::SpoolRecordLength,
                    value: HirCicsValue::Data(length),
                });
            }
            if let Some(pointer) = clauses.get("OUTDESCR") {
                let pointer = complete_data_reference(pointer, semantic)?;
                if !matches!(pointer.usage, CobolUsage::Pointer | CobolUsage::Pointer32) {
                    return Err(ResolutionFailure::Invalid(
                        "CICS SPOOLOPEN OUTPUT OUTDESCR requires POINTER storage".into(),
                    ));
                }
                operands.push(HirCicsNamedOperand {
                    name: HirCicsOperandName::SpoolOutDescr,
                    value: HirCicsValue::Data(pointer),
                });
            }
        }
        if let Some(class) = clauses.get("CLASS") {
            operands.push(text_operand(
                "CLASS",
                HirCicsOperandName::SpoolClass,
                1,
                class,
                semantic,
                false,
            )?);
        }
        return Ok(operands);
    }
    if options
        .iter()
        .filter(|option| matches!(option.as_str(), "KEEP" | "DELETE"))
        .count()
        > 1
    {
        return Err(ResolutionFailure::Invalid(
            "CICS SPOOLCLOSE accepts at most one of KEEP or DELETE".into(),
        ));
    }
    let token = complete_data_reference(&clauses["TOKEN"], semantic)?;
    if token.length != 8
        || !matches!(
            token.category,
            DataCategory::Alphabetic | DataCategory::Alphanumeric
        )
    {
        return Err(ResolutionFailure::Invalid(
            "CICS SPOOLCLOSE TOKEN requires an 8-character data area".into(),
        ));
    }
    Ok(vec![HirCicsNamedOperand {
        name: HirCicsOperandName::SpoolToken,
        value: HirCicsValue::Data(token),
    }])
}

fn text_operand(
    source_name: &str,
    name: HirCicsOperandName,
    width: usize,
    tokens: &[String],
    semantic: &SemanticModel,
    allow_star: bool,
) -> Resolution<HirCicsNamedOperand> {
    let value = if let [value] = tokens
        && value.len() >= 2
        && value.starts_with(['\'', '"'])
        && value.as_bytes().first() == value.as_bytes().last()
    {
        HirCicsValue::Literal(value[1..value.len() - 1].into())
    } else {
        HirCicsValue::Data(complete_data_reference(tokens, semantic)?)
    };
    let valid = match &value {
        HirCicsValue::Literal(value) => {
            (1..=width).contains(&value.len())
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || allow_star && byte == b'*')
        }
        HirCicsValue::Data(reference) => {
            reference.length == width
                && matches!(
                    reference.category,
                    DataCategory::Alphabetic | DataCategory::Alphanumeric
                )
        }
        _ => false,
    };
    if !valid {
        return Err(ResolutionFailure::Invalid(format!(
            "CICS SPOOLOPEN {source_name} requires a {width}-character value"
        )));
    }
    Ok(HirCicsNamedOperand { name, value })
}
