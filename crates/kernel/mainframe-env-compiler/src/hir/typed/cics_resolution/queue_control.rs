use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsOutputBinding,
    HirCicsOutputName, HirCicsValue, Resolution, ResolutionFailure, require_writable,
};
use super::{Clauses, cics_integer_value, cics_value, complete_data_reference};
use crate::{CobolUsage, DataCategory, SemanticModel};

pub(super) fn validate_constraints(
    clauses: &Clauses,
    options: &[String],
    operation: HirCicsOperation,
) -> Resolution<()> {
    if matches!(
        operation,
        HirCicsOperation::DeleteTemporaryStorage
            | HirCicsOperation::ReadTemporaryStorage
            | HirCicsOperation::WriteTemporaryStorage
    ) {
        if usize::from(clauses.contains_key("QUEUE")) + usize::from(clauses.contains_key("QNAME"))
            != 1
        {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS {operation:?} requires exactly one of QUEUE or QNAME"
            )));
        }
        if operation == HirCicsOperation::ReadTemporaryStorage {
            if usize::from(clauses.contains_key("INTO")) + usize::from(clauses.contains_key("SET"))
                != 1
            {
                return Err(ResolutionFailure::Invalid(
                    "CICS READQ TS requires exactly one of INTO or SET".into(),
                ));
            }
            if clauses.contains_key("SET") && !clauses.contains_key("LENGTH") {
                return Err(ResolutionFailure::Invalid(
                    "CICS READQ TS SET requires LENGTH".into(),
                ));
            }
            if clauses.contains_key("ITEM") && options.iter().any(|option| option == "NEXT") {
                return Err(ResolutionFailure::Invalid(
                    "CICS READQ TS ITEM and NEXT are mutually exclusive".into(),
                ));
            }
        }
        if operation == HirCicsOperation::WriteTemporaryStorage {
            if !clauses.contains_key("FROM") {
                return Err(ResolutionFailure::Invalid(
                    "CICS WRITEQ TS requires FROM".into(),
                ));
            }
            let rewrite = options.iter().any(|option| option == "REWRITE");
            if rewrite && !clauses.contains_key("ITEM") {
                return Err(ResolutionFailure::Invalid(
                    "CICS WRITEQ TS REWRITE requires ITEM".into(),
                ));
            }
            if rewrite && clauses.contains_key("NUMITEMS") {
                return Err(ResolutionFailure::Invalid(
                    "CICS WRITEQ TS NUMITEMS is not valid with REWRITE".into(),
                ));
            }
            if clauses.contains_key("SYSID") && !clauses.contains_key("LENGTH") {
                return Err(ResolutionFailure::Invalid(
                    "CICS WRITEQ TS SYSID requires LENGTH".into(),
                ));
            }
            if options.iter().any(|option| option == "AUXILIARY")
                && options.iter().any(|option| option == "MAIN")
            {
                return Err(ResolutionFailure::Invalid(
                    "CICS WRITEQ TS AUXILIARY and MAIN are mutually exclusive".into(),
                ));
            }
        }
        return Ok(());
    }
    let (command, required) = match operation {
        HirCicsOperation::WriteTransientData => ("CICS WRITEQ TD", &["QUEUE", "FROM"][..]),
        HirCicsOperation::ReadTransientData => ("CICS READQ TD", &["QUEUE"][..]),
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
    if operation == HirCicsOperation::ReadTransientData
        && usize::from(clauses.contains_key("INTO")) + usize::from(clauses.contains_key("SET")) != 1
    {
        return Err(ResolutionFailure::Invalid(
            "CICS READQ TD requires exactly one of INTO or SET".into(),
        ));
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
            | HirCicsOperation::ReadTemporaryStorage
            | HirCicsOperation::WriteTemporaryStorage
    ) {
        return Ok(Vec::new());
    }
    let (name, identity, maximum) = if matches!(
        operation,
        HirCicsOperation::DeleteTemporaryStorage
            | HirCicsOperation::ReadTemporaryStorage
            | HirCicsOperation::WriteTemporaryStorage
    ) && clauses.contains_key("QNAME")
    {
        ("QNAME", HirCicsOperandName::Qname, 16)
    } else if matches!(
        operation,
        HirCicsOperation::DeleteTemporaryStorage
            | HirCicsOperation::ReadTemporaryStorage
            | HirCicsOperation::WriteTemporaryStorage
    ) {
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
            } else if matches!(
                operation,
                HirCicsOperation::ReadTemporaryStorage | HirCicsOperation::WriteTemporaryStorage
            ) {
                reference.length == 8
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
    let mut operands = vec![HirCicsNamedOperand {
        name: identity,
        value: queue,
    }];
    if let Some(system) = system_operand(clauses, operation, semantic)? {
        operands.push(system);
    }
    if matches!(
        operation,
        HirCicsOperation::DeleteTransientData | HirCicsOperation::DeleteTemporaryStorage
    ) {
        return Ok(operands);
    }
    if operation == HirCicsOperation::ReadTransientData {
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
    if operation == HirCicsOperation::ReadTemporaryStorage {
        let into = clauses
            .get("INTO")
            .map(|tokens| complete_data_reference(tokens, semantic))
            .transpose()?;
        let length = if let Some(tokens) = clauses.get("LENGTH") {
            let value = complete_data_reference(tokens, semantic)?;
            require_halfword("READQ TS", "LENGTH", &value)?;
            require_writable(&value)?;
            HirCicsValue::Data(value)
        } else {
            HirCicsValue::LengthOf(into.ok_or_else(|| {
                ResolutionFailure::Invalid("CICS READQ TS SET requires LENGTH".into())
            })?)
        };
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::Length,
            value: length,
        });
        if let Some(tokens) = clauses.get("ITEM") {
            let value = cics_integer_value(tokens, semantic)?;
            match &value {
                HirCicsValue::Integer(-32_768..=32_767) => {}
                HirCicsValue::Data(reference) => require_halfword("READQ TS", "ITEM", reference)?,
                _ => {
                    return Err(ResolutionFailure::Invalid(
                        "CICS READQ TS ITEM requires a halfword value".into(),
                    ));
                }
            }
            operands.push(HirCicsNamedOperand {
                name: HirCicsOperandName::Item,
                value,
            });
        }
        return Ok(operands);
    }
    if operation == HirCicsOperation::WriteTemporaryStorage {
        let from = complete_data_reference(&clauses["FROM"], semantic)?;
        let length = if let Some(tokens) = clauses.get("LENGTH") {
            let value = cics_integer_value(tokens, semantic)?;
            match &value {
                HirCicsValue::Integer(length @ -32_768..=32_767) => {
                    if (1..=32_763).contains(length)
                        && usize::try_from(*length).is_ok_and(|length| length > from.length)
                    {
                        return Err(ResolutionFailure::Invalid(
                            "CICS WRITEQ TS LENGTH exceeds the FROM data area".into(),
                        ));
                    }
                }
                HirCicsValue::Data(reference) => {
                    require_halfword("WRITEQ TS", "LENGTH", reference)?
                }
                HirCicsValue::LengthOf(_) => {}
                _ => {
                    return Err(ResolutionFailure::Invalid(
                        "CICS WRITEQ TS LENGTH requires a halfword value".into(),
                    ));
                }
            }
            value
        } else {
            HirCicsValue::LengthOf(from.clone())
        };
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::From,
            value: HirCicsValue::Data(from),
        });
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::Length,
            value: length,
        });
        if let Some(tokens) = clauses.get("ITEM") {
            let reference = complete_data_reference(tokens, semantic)?;
            require_halfword("WRITEQ TS", "ITEM", &reference)?;
            require_writable(&reference)?;
            operands.push(HirCicsNamedOperand {
                name: HirCicsOperandName::Item,
                value: HirCicsValue::Data(reference),
            });
        }
        return Ok(operands);
    }
    let HirCicsValue::Data(from) = cics_value(&clauses["FROM"], semantic)? else {
        return Err(ResolutionFailure::Invalid(
            "CICS WRITEQ TD FROM requires a data area".into(),
        ));
    };
    operands.push(HirCicsNamedOperand {
        name: HirCicsOperandName::From,
        value: HirCicsValue::Data(from),
    });
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

fn system_operand(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Option<HirCicsNamedOperand>> {
    let Some(tokens) = clauses.get("SYSID") else {
        return Ok(None);
    };
    let value = cics_value(tokens, semantic)?;
    let valid = match &value {
        HirCicsValue::Literal(value) => {
            matches!(value.len(), 1..=4) && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
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
        return Err(ResolutionFailure::Invalid(format!(
            "CICS {operation:?} SYSID requires a 1-4 character name"
        )));
    }
    Ok(Some(HirCicsNamedOperand {
        name: HirCicsOperandName::SysId,
        value,
    }))
}

pub(super) fn outputs(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    if !matches!(
        operation,
        HirCicsOperation::ReadTemporaryStorage | HirCicsOperation::WriteTemporaryStorage
    ) {
        return Ok(Vec::new());
    }
    let command = if operation == HirCicsOperation::ReadTemporaryStorage {
        "READQ TS"
    } else {
        "WRITEQ TS"
    };
    let mut outputs = Vec::new();
    if let Some(tokens) = clauses.get("NUMITEMS") {
        let target = complete_data_reference(tokens, semantic)?;
        require_halfword(command, "NUMITEMS", &target)?;
        require_writable(&target)?;
        outputs.push(HirCicsOutputBinding {
            name: HirCicsOutputName::NumItems,
            target,
        });
    }
    Ok(outputs)
}

fn require_halfword(
    command: &str,
    name: &str,
    reference: &super::super::HirDataReference,
) -> Resolution<()> {
    if reference.category != DataCategory::Binary || reference.length != 2 || reference.scale != 0 {
        return Err(ResolutionFailure::Invalid(format!(
            "CICS {command} {name} requires a halfword binary data item"
        )));
    }
    Ok(())
}
