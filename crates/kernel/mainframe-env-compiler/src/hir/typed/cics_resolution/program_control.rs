use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsOutputBinding,
    HirCicsOutputName, HirCicsValue, Resolution, ResolutionFailure, require_writable,
};
use super::{Clauses, cics_integer_value, complete_data_reference, program_name, transaction_name};
use crate::SemanticModel;

pub(super) fn validate_constraints(
    operation: HirCicsOperation,
    clauses: &Clauses,
) -> Resolution<()> {
    if matches!(
        operation,
        HirCicsOperation::Link | HirCicsOperation::Xctl | HirCicsOperation::Return
    ) && clauses.contains_key("LENGTH")
        && !clauses.contains_key("COMMAREA")
    {
        Err(ResolutionFailure::Invalid(format!(
            "CICS {operation:?} LENGTH requires COMMAREA"
        )))
    } else if operation == HirCicsOperation::Return
        && clauses.contains_key("COMMAREA")
        && !clauses.contains_key("TRANSID")
    {
        Err(ResolutionFailure::Invalid(
            "CICS RETURN COMMAREA requires TRANSID in the typed local subset".into(),
        ))
    } else {
        Ok(())
    }
}

pub(super) fn operands(
    operation: HirCicsOperation,
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    match operation {
        HirCicsOperation::Link => transfer_operands(clauses, semantic, "LINK"),
        HirCicsOperation::Xctl => transfer_operands(clauses, semantic, "XCTL"),
        HirCicsOperation::Return => return_operands(clauses, semantic),
        _ => Ok(Vec::new()),
    }
}

fn transfer_operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
    command: &str,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let mut operands = vec![HirCicsNamedOperand {
        name: HirCicsOperandName::Program,
        value: program_name::value(&clauses["PROGRAM"], semantic, command)?,
    }];
    if let Some(tokens) = clauses.get("COMMAREA") {
        let reference = complete_data_reference(tokens, semantic)?;
        require_writable(&reference)?;
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::Commarea,
            value: HirCicsValue::Data(reference),
        });
    }
    if let Some(tokens) = clauses.get("LENGTH") {
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::Length,
            value: commarea_length(tokens, semantic)?,
        });
    }
    Ok(operands)
}

pub(super) fn link_commarea_output(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Option<HirCicsOutputBinding>> {
    let Some(tokens) = clauses.get("COMMAREA") else {
        return Ok(None);
    };
    let target = complete_data_reference(tokens, semantic)?;
    require_writable(&target)?;
    Ok(Some(HirCicsOutputBinding {
        name: HirCicsOutputName::Commarea,
        target,
    }))
}

fn return_operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let mut operands = Vec::new();
    if let Some(tokens) = clauses.get("TRANSID") {
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::TransId,
            value: transaction_name::value(tokens, semantic)?,
        });
    }
    if let Some(tokens) = clauses.get("COMMAREA") {
        let reference = complete_data_reference(tokens, semantic)?;
        require_writable(&reference)?;
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::Commarea,
            value: HirCicsValue::Data(reference),
        });
    }
    if let Some(tokens) = clauses.get("LENGTH") {
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::Length,
            value: commarea_length(tokens, semantic)?,
        });
    }
    Ok(operands)
}

fn commarea_length(tokens: &[String], semantic: &SemanticModel) -> Resolution<HirCicsValue> {
    if tokens
        .first()
        .is_some_and(|token| token.eq_ignore_ascii_case("LENGTH"))
        && tokens
            .get(1)
            .is_some_and(|token| token.eq_ignore_ascii_case("OF"))
    {
        complete_data_reference(&tokens[2..], semantic).map(HirCicsValue::LengthOf)
    } else {
        cics_integer_value(tokens, semantic)
    }
}
