use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsValue, Resolution,
    ResolutionFailure, require_writable,
};
use super::{Clauses, cics_integer_value, cics_value, complete_data_reference};
use crate::{CobolUsage, DataCategory, SemanticModel};

pub(super) fn validate_constraints(
    clauses: &Clauses,
    operation: HirCicsOperation,
) -> Resolution<()> {
    if operation == HirCicsOperation::DeleteTemporaryStorage {
        if usize::from(clauses.contains_key("QUEUE")) + usize::from(clauses.contains_key("QNAME"))
            != 1
        {
            return Err(ResolutionFailure::Invalid(
                "CICS DELETEQ TS requires exactly one of QUEUE or QNAME".into(),
            ));
        }
        return Ok(());
    }
    let (command, required) = match operation {
        HirCicsOperation::WriteTransientData => ("CICS WRITEQ TD", &["QUEUE", "FROM"][..]),
        HirCicsOperation::ReadTransientData => ("CICS READQ TD", &["QUEUE", "INTO"][..]),
        HirCicsOperation::DeleteTransientData => ("CICS DELETEQ TD", &["QUEUE"][..]),
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
            | HirCicsOperation::ReadTransientData
            | HirCicsOperation::DeleteTransientData
            | HirCicsOperation::DeleteTemporaryStorage
    ) {
        return Ok(Vec::new());
    }
    let (name, identity, maximum) =
        if operation == HirCicsOperation::DeleteTemporaryStorage && clauses.contains_key("QNAME") {
            ("QNAME", HirCicsOperandName::Qname, 16)
        } else if operation == HirCicsOperation::DeleteTemporaryStorage {
            ("QUEUE", HirCicsOperandName::Queue, 8)
        } else {
            ("QUEUE", HirCicsOperandName::Queue, 4)
        };
    let queue = cics_value(&clauses[name], semantic)?;
    let valid_queue = match &queue {
        HirCicsValue::Literal(value) => {
            (1..=maximum).contains(&value.len())
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        }
        HirCicsValue::Data(reference) => {
            (if name == "QNAME" {
                reference.length == 16
            } else {
                (1..=maximum).contains(&reference.length)
            }) && matches!(
                reference.category,
                DataCategory::Alphabetic | DataCategory::Alphanumeric
            )
        }
        HirCicsValue::Integer(_) | HirCicsValue::LengthOf(_) => false,
    };
    if !valid_queue {
        return Err(ResolutionFailure::Invalid(format!(
            "CICS {operation:?} {name} requires a 1-{maximum} character name"
        )));
    }
    if matches!(
        operation,
        HirCicsOperation::DeleteTransientData | HirCicsOperation::DeleteTemporaryStorage
    ) {
        let mut operands = vec![HirCicsNamedOperand {
            name: identity,
            value: queue,
        }];
        if let Some(tokens) = clauses.get("SYSID") {
            let value = cics_value(tokens, semantic)?;
            let valid = match &value {
                HirCicsValue::Literal(value) => {
                    matches!(value.len(), 1..=4)
                        && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
                }
                HirCicsValue::Data(reference) => {
                    matches!(reference.length, 1..=4)
                        && matches!(
                            reference.category,
                            DataCategory::Alphabetic | DataCategory::Alphanumeric
                        )
                }
                HirCicsValue::Integer(_) | HirCicsValue::LengthOf(_) => false,
            };
            if !valid {
                return Err(ResolutionFailure::Invalid(
                    "CICS DELETEQ TS SYSID requires a 1-4 character name".into(),
                ));
            }
            operands.push(HirCicsNamedOperand {
                name: HirCicsOperandName::SysId,
                value,
            });
        }
        return Ok(operands);
    }
    if operation == HirCicsOperation::ReadTransientData {
        let mut operands = vec![HirCicsNamedOperand {
            name: HirCicsOperandName::Queue,
            value: queue,
        }];
        if let Some(length) = clauses.get("LENGTH") {
            let value = cics_integer_value(length, semantic)?;
            let HirCicsValue::Data(reference) = &value else {
                return Err(ResolutionFailure::Invalid(
                    "CICS READQ TD LENGTH requires writable halfword binary storage".into(),
                ));
            };
            require_writable(reference)?;
            if reference.usage != CobolUsage::Binary
                || reference.length != 2
                || reference.scale != 0
            {
                return Err(ResolutionFailure::Invalid(
                    "CICS READQ TD LENGTH requires writable halfword binary storage".into(),
                ));
            }
            operands.push(HirCicsNamedOperand {
                name: HirCicsOperandName::Length,
                value,
            });
        }
        return Ok(operands);
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
