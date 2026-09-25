use super::*;

pub(super) fn validate(clauses: &Clauses, semantic: &SemanticModel) -> Resolution<()> {
    for name in ["HTTPHEADER", "NAMELENGTH", "VALUE", "VALUELENGTH"] {
        if !clauses.contains_key(name) {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS WEB WRITE requires {name}"
            )));
        }
    }
    for name in ["HTTPHEADER", "VALUE"] {
        let value = cics_value(&clauses[name], semantic)?;
        if !matches!(value, HirCicsValue::Literal(_) | HirCicsValue::Data(_)) {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS WEB WRITE {name} requires character input"
            )));
        }
    }
    for name in ["NAMELENGTH", "VALUELENGTH"] {
        let value = cics_integer_value(&clauses[name], semantic)?;
        if matches!(value, HirCicsValue::Integer(number) if number < 1 || name == "VALUELENGTH" && number > 32000)
        {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS WEB WRITE {name} is out of range"
            )));
        }
        if let HirCicsValue::Data(reference) = value
            && (reference.usage != CobolUsage::Binary
                || reference.length != 4
                || reference.scale != 0)
        {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS WEB WRITE {name} requires fullword binary input"
            )));
        }
    }
    if let Some(tokens) = clauses.get("SESSTOKEN") {
        let value = cics_value(tokens, semantic)?;
        if !matches!(&value, HirCicsValue::Data(reference) if reference.length == 8)
            && !matches!(&value, HirCicsValue::Literal(bytes) if bytes.len() == 8)
        {
            return Err(ResolutionFailure::Invalid(
                "CICS WEB WRITE SESSTOKEN requires eight bytes".into(),
            ));
        }
    }
    Ok(())
}

pub(super) fn operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let mut operands = vec![
        HirCicsNamedOperand {
            name: HirCicsOperandName::WebHttpHeaderName,
            value: cics_value(&clauses["HTTPHEADER"], semantic)?,
        },
        HirCicsNamedOperand {
            name: HirCicsOperandName::WebNameLength,
            value: cics_integer_value(&clauses["NAMELENGTH"], semantic)?,
        },
        HirCicsNamedOperand {
            name: HirCicsOperandName::WebHeaderValue,
            value: cics_value(&clauses["VALUE"], semantic)?,
        },
        HirCicsNamedOperand {
            name: HirCicsOperandName::WebValueLength,
            value: cics_integer_value(&clauses["VALUELENGTH"], semantic)?,
        },
    ];
    if let Some(tokens) = clauses.get("SESSTOKEN") {
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::WebSessionToken,
            value: cics_value(tokens, semantic)?,
        });
    }
    Ok(operands)
}
