use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOutputName, HirCicsValue, HirDataReference,
    Resolution, ResolutionFailure, require_numeric,
};
use super::{Clauses, cics_value, complete_data_reference};
use crate::{CobolUsage, DataCategory, SemanticModel};

pub(super) fn operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let absolute = complete_data_reference(&clauses["ABSTIME"], semantic)?;
    require_absolute_time(&absolute)?;
    let mut operands = vec![HirCicsNamedOperand {
        name: HirCicsOperandName::Abstime,
        value: HirCicsValue::Data(absolute),
    }];
    for (name, identity) in [
        ("DATESEP", HirCicsOperandName::DateSep),
        ("TIMESEP", HirCicsOperandName::TimeSep),
    ] {
        if let Some(tokens) = clauses.get(name) {
            let value = cics_value(tokens, semantic)?;
            require_separator(name, &value)?;
            operands.push(HirCicsNamedOperand {
                name: identity,
                value,
            });
        }
    }
    Ok(operands)
}

fn require_absolute_time(reference: &HirDataReference) -> Resolution<()> {
    require_numeric(reference)?;
    if reference.usage == CobolUsage::PackedDecimal
        && reference.length == 8
        && reference.digits == 15
        && reference.scale == 0
        && reference.signed
    {
        Ok(())
    } else {
        Err(ResolutionFailure::Invalid(
            "CICS absolute time requires PIC S9(15) COMP-3 storage".into(),
        ))
    }
}

fn require_separator(name: &str, value: &HirCicsValue) -> Resolution<()> {
    let valid = match value {
        HirCicsValue::Literal(value) => value.len() == 1,
        HirCicsValue::Data(reference) => {
            reference.length == 1
                && matches!(
                    reference.category,
                    DataCategory::Alphabetic | DataCategory::Alphanumeric
                )
        }
        HirCicsValue::Integer(_) | HirCicsValue::LengthOf(_) => false,
    };
    if valid {
        Ok(())
    } else {
        Err(ResolutionFailure::Invalid(format!(
            "CICS FORMATTIME {name} requires one character"
        )))
    }
}

pub(super) fn require_output_shape(
    name: HirCicsOutputName,
    target: &HirDataReference,
    _clauses: &Clauses,
    _options: &[String],
) -> Resolution<()> {
    match name {
        HirCicsOutputName::Abstime => require_absolute_time(target),
        HirCicsOutputName::Milliseconds => {
            require_numeric(target)?;
            if matches!(target.usage, CobolUsage::Binary | CobolUsage::NativeBinary)
                && target.length == 4
                && target.scale == 0
            {
                Ok(())
            } else {
                Err(ResolutionFailure::Invalid(
                    "CICS FORMATTIME MILLISECONDS requires fullword binary storage".into(),
                ))
            }
        }
        HirCicsOutputName::Resp | HirCicsOutputName::Resp2 => require_numeric(target),
        HirCicsOutputName::Mmddyy
        | HirCicsOutputName::Mmddyyyy
        | HirCicsOutputName::Time
        | HirCicsOutputName::Yyddd
        | HirCicsOutputName::Yymmdd
        | HirCicsOutputName::Yyyymmdd => {
            let field = match name {
                HirCicsOutputName::Mmddyy | HirCicsOutputName::Time | HirCicsOutputName::Yymmdd => {
                    8
                }
                HirCicsOutputName::Mmddyyyy | HirCicsOutputName::Yyyymmdd => 10,
                HirCicsOutputName::Yyddd => 6,
                _ => unreachable!(),
            };
            if target.length == field
                && matches!(
                    target.category,
                    DataCategory::Alphabetic | DataCategory::Alphanumeric
                )
            {
                Ok(())
            } else {
                Err(ResolutionFailure::Invalid(format!(
                    "CICS FORMATTIME output has an invalid field length for {name:?}"
                )))
            }
        }
        HirCicsOutputName::Commarea
        | HirCicsOutputName::Into
        | HirCicsOutputName::SetPointer
        | HirCicsOutputName::Ridfld
        | HirCicsOutputName::Token
        | HirCicsOutputName::Length
        | HirCicsOutputName::ReturnTransId
        | HirCicsOutputName::ReturnTermId
        | HirCicsOutputName::Queue
        | HirCicsOutputName::NumItems
        | HirCicsOutputName::DocumentToken
        | HirCicsOutputName::DocumentSize
        | HirCicsOutputName::ElementName
        | HirCicsOutputName::ElementNameLength
        | HirCicsOutputName::ElementNamespace
        | HirCicsOutputName::ElementNamespaceLength
        | HirCicsOutputName::TypeName
        | HirCicsOutputName::TypeNameLength
        | HirCicsOutputName::TypeNamespace
        | HirCicsOutputName::TypeNamespaceLength
        | HirCicsOutputName::JournalReqId
        | HirCicsOutputName::SpoolToken
        | HirCicsOutputName::SpoolToFlength
        | HirCicsOutputName::WebAction
        | HirCicsOutputName::WebMessageId
        | HirCicsOutputName::WebRelatesUri
        | HirCicsOutputName::WebRelatesType
        | HirCicsOutputName::WebEprInto
        | HirCicsOutputName::WebEprSet
        | HirCicsOutputName::WebEprLength
        | HirCicsOutputName::Assign(_) => Ok(()),
    }
}
