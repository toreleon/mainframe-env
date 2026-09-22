use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsValue, Resolution,
    ResolutionFailure,
};
use super::{Clauses, cics_integer_value, cics_value, complete_data_reference};
use crate::{DataCategory, SemanticModel};

pub(super) fn validate_constraints(
    clauses: &Clauses,
    operation: HirCicsOperation,
) -> Resolution<()> {
    let (command, required) = match operation {
        HirCicsOperation::WriteTransientData => ("CICS WRITEQ TD", &["QUEUE", "FROM"][..]),
        HirCicsOperation::DeleteTransientData => ("CICS DELETEQ TD", &["QUEUE"][..]),
        HirCicsOperation::DeleteTemporaryStorage => ("CICS DELETEQ TS", &["QUEUE"][..]),
        _ => return Ok(()),
    };
    for name in required {
        if !clauses.contains_key(*name) {
            return Err(ResolutionFailure::Invalid(format!(
                "{command} requires {name}"
            )));
        }
    }
    Ok(())
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if !matches!(
        operation,
        HirCicsOperation::WriteTransientData
            | HirCicsOperation::DeleteTransientData
            | HirCicsOperation::DeleteTemporaryStorage
    ) {
        return Ok(Vec::new());
    }
    let queue = cics_value(&clauses["QUEUE"], semantic)?;
    let maximum = if operation == HirCicsOperation::DeleteTemporaryStorage {
        8
    } else {
        4
    };
    let valid_queue = match &queue {
        HirCicsValue::Literal(value) => {
            (1..=maximum).contains(&value.len())
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        }
        HirCicsValue::Data(reference) => {
            (1..=maximum).contains(&reference.length)
                && matches!(
                    reference.category,
                    DataCategory::Alphabetic | DataCategory::Alphanumeric
                )
        }
        HirCicsValue::Integer(_) | HirCicsValue::LengthOf(_) => false,
    };
    if !valid_queue {
        return Err(ResolutionFailure::Invalid(format!(
            "CICS {operation:?} QUEUE requires a 1-{maximum} character name"
        )));
    }
    if matches!(
        operation,
        HirCicsOperation::DeleteTransientData | HirCicsOperation::DeleteTemporaryStorage
    ) {
        return Ok(vec![HirCicsNamedOperand {
            name: HirCicsOperandName::Queue,
            value: queue,
        }]);
    }
    let HirCicsValue::Data(from) = cics_value(&clauses["FROM"], semantic)? else {
        return Err(ResolutionFailure::Invalid(
            "CICS WRITEQ TD FROM requires a data area".into(),
        ));
    };
    let mut operands = vec![
        HirCicsNamedOperand {
            name: HirCicsOperandName::Queue,
            value: queue,
        },
        HirCicsNamedOperand {
            name: HirCicsOperandName::From,
            value: HirCicsValue::Data(from),
        },
    ];
    if let Some(length) = clauses.get("LENGTH") {
        let value = if length
            .first()
            .is_some_and(|token| token.eq_ignore_ascii_case("LENGTH"))
            && length
                .get(1)
                .is_some_and(|token| token.eq_ignore_ascii_case("OF"))
        {
            HirCicsValue::LengthOf(complete_data_reference(&length[2..], semantic)?)
        } else {
            cics_integer_value(length, semantic)?
        };
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::Length,
            value,
        });
    }
    Ok(operands)
}
