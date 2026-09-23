//! COBOL source legality and resolved storage for WRITE OPERATOR.

use super::{
    Clauses, HirCicsNamedOperand, HirCicsOperandName, HirCicsOutputBinding, HirCicsOutputName,
    HirCicsValue, Resolution, ResolutionFailure, cics_integer_value, cics_value,
    complete_data_reference, require_writable,
};
use crate::{DataCategory, SemanticModel};

pub(super) fn validate(clauses: &Clauses, options: &[String]) -> Resolution<()> {
    let action_flags = ["IMMEDIATE", "EVENTUAL", "CRITICAL"]
        .into_iter()
        .filter(|name| options.iter().any(|option| option == name))
        .count();
    if clauses.contains_key("CONSNAME") && clauses.contains_key("ROUTECODES")
        || clauses.contains_key("NUMROUTES") && !clauses.contains_key("ROUTECODES")
        || clauses.contains_key("ACTION") && action_flags != 0
        || action_flags > 1
        || clauses.contains_key("REPLY") != clauses.contains_key("MAXLENGTH")
        || !clauses.contains_key("REPLY")
            && (clauses.contains_key("TIMEOUT") || clauses.contains_key("REPLYLENGTH"))
    {
        return Err(ResolutionFailure::Invalid(
            "CICS WRITE OPERATOR routing, action, or reply options conflict".into(),
        ));
    }
    Ok(())
}

pub(super) fn operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let mut out = vec![HirCicsNamedOperand {
        name: HirCicsOperandName::OperatorText,
        value: cics_value(&clauses["TEXT"], semantic)?,
    }];
    for (source, name) in [
        ("TEXTLENGTH", HirCicsOperandName::OperatorTextLength),
        ("NUMROUTES", HirCicsOperandName::OperatorNumRoutes),
        ("MAXLENGTH", HirCicsOperandName::OperatorMaxLength),
        ("TIMEOUT", HirCicsOperandName::OperatorTimeout),
    ] {
        if let Some(value) = clauses.get(source) {
            out.push(HirCicsNamedOperand {
                name,
                value: cics_integer_value(value, semantic)?,
            });
        }
    }
    if let Some(value) = clauses.get("ACTION") {
        out.push(HirCicsNamedOperand {
            name: HirCicsOperandName::OperatorAction,
            value: action(value, semantic)?,
        });
    }
    if let Some(value) = clauses.get("CONSNAME") {
        out.push(HirCicsNamedOperand {
            name: HirCicsOperandName::OperatorConsName,
            value: cics_value(value, semantic)?,
        });
    }
    if let Some(value) = clauses.get("ROUTECODES") {
        out.push(HirCicsNamedOperand {
            name: HirCicsOperandName::OperatorRouteCodes,
            value: HirCicsValue::Data(complete_data_reference(value, semantic)?),
        });
    }
    Ok(out)
}

fn action(tokens: &[String], semantic: &SemanticModel) -> Resolution<HirCicsValue> {
    if let [function, open, value, close] = tokens
        && function.eq_ignore_ascii_case("DFHVALUE")
        && open == "("
        && close == ")"
    {
        return match value.as_str() {
            "IMMEDIATE" => Ok(HirCicsValue::Integer(2)),
            "EVENTUAL" => Ok(HirCicsValue::Integer(3)),
            "CRITICAL" => Ok(HirCicsValue::Integer(11)),
            _ => Err(ResolutionFailure::Invalid(
                "CICS WRITE OPERATOR ACTION requires IMMEDIATE, EVENTUAL, or CRITICAL".into(),
            )),
        };
    }
    cics_integer_value(tokens, semantic)
}

pub(super) fn outputs(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    let mut out = Vec::new();
    if let Some(value) = clauses.get("REPLY") {
        let target = complete_data_reference(value, semantic)?;
        require_writable(&target)?;
        out.push(HirCicsOutputBinding {
            name: HirCicsOutputName::OperatorReply,
            target,
        });
    }
    if let Some(value) = clauses.get("REPLYLENGTH") {
        let target = complete_data_reference(value, semantic)?;
        require_writable(&target)?;
        if target.category != DataCategory::Binary || target.length != 4 || target.scale != 0 {
            return Err(ResolutionFailure::Invalid(
                "CICS WRITE OPERATOR REPLYLENGTH requires writable fullword binary storage".into(),
            ));
        }
        out.push(HirCicsOutputBinding {
            name: HirCicsOutputName::OperatorReplyLength,
            target,
        });
    }
    Ok(out)
}
