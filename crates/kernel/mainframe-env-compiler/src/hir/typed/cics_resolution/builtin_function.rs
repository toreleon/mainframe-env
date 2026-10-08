use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOutputBinding, HirCicsOutputName, HirCicsValue,
    Resolution, ResolutionFailure, require_writable,
};
use super::{Clauses, cics_integer_value, complete_data_reference};
use crate::{DataCategory, SemanticModel};
use mainframe_env_ir::{CicsApplicationOptionValueShape, CicsApplicationRegistryDescriptor};

pub(super) fn option_value_shape(
    descriptor: &CicsApplicationRegistryDescriptor,
    name: &str,
) -> Option<CicsApplicationOptionValueShape> {
    (descriptor.label_tokens == ["BIF", "DIGEST"])
        .then_some(match name {
            "HEX" | "BINARY" | "BASE64" => Some(CicsApplicationOptionValueShape::Flag),
            "DIGESTTYPE" => Some(CicsApplicationOptionValueShape::Value),
            _ => None,
        })
        .flatten()
}

pub(super) fn digest_operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let record = super::cics_value(&clauses["RECORD"], semantic).map_err(typed_form)?;
    let length = cics_integer_value(&clauses["RECORDLEN"], semantic).map_err(typed_form)?;
    if let HirCicsValue::Data(reference) = &length
        && !(reference.length == 4
            && reference.scale == 0
            && matches!(
                reference.usage,
                crate::CobolUsage::Binary | crate::CobolUsage::NativeBinary
            ))
    {
        return Err(ResolutionFailure::Invalid(
            "CICS BIF DIGEST RECORDLEN requires fullword binary storage".into(),
        ));
    }
    let mut operands = vec![
        HirCicsNamedOperand {
            name: HirCicsOperandName::Record,
            value: record,
        },
        HirCicsNamedOperand {
            name: HirCicsOperandName::RecordLength,
            value: length,
        },
    ];
    if let Some(tokens) = clauses.get("DIGESTTYPE") {
        let format = match tokens.as_slice() {
            [function, open, name, close]
                if function.eq_ignore_ascii_case("DFHVALUE") && open == "(" && close == ")" =>
            {
                name
            }
            [name] if name.starts_with(['\'', '"']) && name.len() > 2 => name,
            _ => {
                return Err(ResolutionFailure::Invalid(
                    "CICS BIF DIGEST DIGESTTYPE requires a named HEX, BINARY, or BASE64 CVDA"
                        .into(),
                ));
            }
        };
        let format = format.trim_matches(['\'', '"']).to_ascii_uppercase();
        if !matches!(format.as_str(), "HEX" | "BINARY" | "BASE64") {
            return Err(ResolutionFailure::Invalid(
                "CICS BIF DIGEST DIGESTTYPE has an unknown CVDA".into(),
            ));
        }
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::DigestType,
            value: HirCicsValue::Literal(format),
        });
    }
    Ok(operands)
}

pub(super) fn digest_output(
    clauses: &Clauses,
    options: &[String],
    semantic: &SemanticModel,
) -> Resolution<HirCicsOutputBinding> {
    let selectors = ["HEX", "BINARY", "BASE64"]
        .into_iter()
        .filter(|name| options.iter().any(|option| option == name))
        .collect::<Vec<_>>();
    if selectors.len() + usize::from(clauses.contains_key("DIGESTTYPE")) != 1 {
        return Err(ResolutionFailure::Invalid(
            "CICS BIF DIGEST permits one digest format".into(),
        ));
    }
    let selected = selectors
        .first()
        .copied()
        .or_else(|| {
            clauses
                .get("DIGESTTYPE")
                .and_then(|tokens| {
                    tokens
                        .iter()
                        .find(|token| matches!(token.as_str(), "HEX" | "BINARY" | "BASE64"))
                })
                .map(String::as_str)
        })
        .ok_or_else(|| ResolutionFailure::Invalid("CICS BIF DIGEST requires a format".into()))?;
    let required = match selected {
        "BINARY" => 20,
        "BASE64" => 28,
        _ => 40,
    };
    let target = complete_data_reference(&clauses["RESULT"], semantic).map_err(typed_form)?;
    require_writable(&target)?;
    if target.length < required
        || !matches!(
            target.category,
            DataCategory::Alphabetic | DataCategory::Alphanumeric
        )
    {
        return Err(ResolutionFailure::Invalid(format!(
            "CICS BIF DIGEST RESULT requires at least {required} character bytes"
        )));
    }
    Ok(HirCicsOutputBinding {
        name: HirCicsOutputName::DigestResult,
        target,
    })
}

fn typed_form(problem: ResolutionFailure) -> ResolutionFailure {
    match problem {
        ResolutionFailure::Unsupported => {
            ResolutionFailure::Invalid("CICS BIF DIGEST requires source-bounded data values".into())
        }
        other => other,
    }
}

pub(super) fn deedit_operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let field = field(clauses, semantic)?;
    let mut operands = vec![HirCicsNamedOperand {
        name: HirCicsOperandName::Field,
        value: HirCicsValue::Data(field),
    }];
    if let Some(tokens) = clauses.get("LENGTH") {
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::Length,
            value: cics_integer_value(tokens, semantic).map_err(|problem| match problem {
                ResolutionFailure::Unsupported => ResolutionFailure::Invalid(
                    "CICS BIF DEEDIT LENGTH requires an integer value".into(),
                ),
                other => other,
            })?,
        });
    }
    Ok(operands)
}

pub(super) fn deedit_output(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<HirCicsOutputBinding> {
    Ok(HirCicsOutputBinding {
        name: HirCicsOutputName::Field,
        target: field(clauses, semantic)?,
    })
}

fn field(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<super::super::HirDataReference> {
    let field =
        complete_data_reference(&clauses["FIELD"], semantic).map_err(|problem| match problem {
            ResolutionFailure::Unsupported => {
                ResolutionFailure::Invalid("CICS BIF DEEDIT FIELD requires a data area".into())
            }
            other => other,
        })?;
    require_writable(&field)?;
    if field.length == 0
        || !matches!(
            field.category,
            DataCategory::Alphabetic | DataCategory::Alphanumeric
        )
    {
        return Err(ResolutionFailure::Invalid(
            "CICS BIF DEEDIT FIELD requires writable character storage".into(),
        ));
    }
    Ok(field)
}
