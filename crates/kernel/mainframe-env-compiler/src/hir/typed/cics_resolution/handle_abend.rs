use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsValue, Resolution, ResolutionFailure,
};
use super::{Clauses, cics_value, is_single_condition_label};
use crate::{DataCategory, SemanticModel};

pub(super) fn operands(
    clauses: &Clauses,
    options: &[String],
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let action_count = usize::from(clauses.contains_key("LABEL"))
        + usize::from(clauses.contains_key("PROGRAM"))
        + usize::from(options.iter().any(|option| option == "CANCEL"))
        + usize::from(options.iter().any(|option| option == "RESET"));
    if action_count > 1 {
        return Err(ResolutionFailure::Invalid(
            "CICS HANDLE ABEND action options are mutually exclusive".into(),
        ));
    }
    if let Some(tokens) = clauses.get("LABEL") {
        if !is_single_condition_label(tokens) {
            return Err(ResolutionFailure::Invalid(
                "CICS HANDLE ABEND LABEL requires one COBOL label".into(),
            ));
        }
        return Ok(vec![HirCicsNamedOperand {
            name: HirCicsOperandName::Label,
            value: HirCicsValue::Literal(tokens[0].to_ascii_uppercase()),
        }]);
    }
    let Some(tokens) = clauses.get("PROGRAM") else {
        return Ok(Vec::new());
    };
    let value = cics_value(tokens, semantic)?;
    let valid = match &value {
        HirCicsValue::Literal(value) => {
            matches!(value.len(), 1..=8) && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
        }
        HirCicsValue::Data(reference) => {
            matches!(reference.length, 1..=8)
                && matches!(
                    reference.category,
                    DataCategory::Alphabetic | DataCategory::Alphanumeric
                )
        }
        HirCicsValue::Integer(_) => false,
    };
    if !valid {
        return Err(ResolutionFailure::Invalid(
            "CICS HANDLE ABEND PROGRAM requires a 1-8 character name".into(),
        ));
    }
    Ok(vec![HirCicsNamedOperand {
        name: HirCicsOperandName::Program,
        value,
    }])
}
