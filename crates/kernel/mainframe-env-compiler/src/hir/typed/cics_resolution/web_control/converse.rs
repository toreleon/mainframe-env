use super::*;

fn send_clauses(clauses: &Clauses) -> Clauses {
    clauses
        .iter()
        .filter(|(name, _)| {
            SEND_CLAUSES.contains(&name.as_str())
                && !["STATUSCODE", "STATUSTEXT", "STATUSLEN", "ACTION"].contains(&name.as_str())
        })
        .map(|(name, tokens)| (name.clone(), tokens.clone()))
        .collect()
}

pub(super) fn validate(
    clauses: &Clauses,
    _options: &[String],
    semantic: &SemanticModel,
) -> Resolution<()> {
    send::validate(&send_clauses(clauses), semantic)?;
    if clauses.contains_key("STATUSTEXT") != clauses.contains_key("STATUSLEN") {
        return Err(ResolutionFailure::Invalid(
            "CICS WEB CONVERSE STATUSTEXT and STATUSLEN must occur together".into(),
        ));
    }
    let maximum = cics_integer_value(&clauses["MAXLENGTH"], semantic)?;
    if matches!(maximum, HirCicsValue::Integer(value) if value <= 0) {
        return Err(ResolutionFailure::Invalid(
            "CICS WEB CONVERSE MAXLENGTH must be positive".into(),
        ));
    }
    if let HirCicsValue::Data(reference) = maximum
        && (reference.usage != CobolUsage::Binary || reference.length != 4 || reference.scale != 0)
    {
        return Err(ResolutionFailure::Invalid(
            "CICS WEB CONVERSE MAXLENGTH requires fullword binary input".into(),
        ));
    }
    let into = complete_data_reference(&clauses["INTO"], semantic)?;
    require_writable(&into)?;
    if into.length == 0 {
        return Err(ResolutionFailure::Invalid(
            "CICS WEB CONVERSE INTO is empty".into(),
        ));
    }
    fullword_target(&clauses["TOLENGTH"], semantic, "TOLENGTH")?;
    if let Some(tokens) = clauses.get("STATUSLEN") {
        fullword_target(tokens, semantic, "STATUSLEN")?;
    }
    for (name, length) in [("STATUSCODE", 2), ("BODYCHARSET", 40)] {
        if let Some(tokens) = clauses.get(name) {
            let target = complete_data_reference(tokens, semantic)?;
            require_writable(&target)?;
            if target.length != length {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS WEB CONVERSE {name} requires {length}-byte storage"
                )));
            }
        }
    }
    if let Some(tokens) = clauses.get("STATUSTEXT") {
        let target = complete_data_reference(tokens, semantic)?;
        require_writable(&target)?;
        if target.length == 0 || target.length > 256 {
            return Err(ResolutionFailure::Invalid(
                "CICS WEB CONVERSE STATUSTEXT requires one to 256 bytes".into(),
            ));
        }
    }
    if let Some(tokens) = clauses.get("MEDIATYPE") {
        let target = complete_data_reference(tokens, semantic)?;
        require_writable(&target)?;
        if target.length != 56 {
            return Err(ResolutionFailure::Invalid(
                "CICS WEB CONVERSE MEDIATYPE requires 56-byte storage".into(),
            ));
        }
    }
    Ok(())
}

pub(super) fn operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let mut out = send::operands(&send_clauses(clauses), semantic)?;
    out.push(HirCicsNamedOperand {
        name: HirCicsOperandName::WebReceiveMaxLength,
        value: cics_integer_value(&clauses["MAXLENGTH"], semantic)?,
    });
    if let Some(tokens) = clauses.get("STATUSLEN") {
        out.push(HirCicsNamedOperand {
            name: HirCicsOperandName::WebReceiveStatusLength,
            value: cics_integer_value(tokens, semantic)?,
        });
    }
    Ok(out)
}

pub(super) fn outputs(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    let mut out = Vec::new();
    for (source, name) in [
        ("INTO", HirCicsOutputName::WebConverseInto),
        ("TOLENGTH", HirCicsOutputName::WebConverseToLength),
        ("STATUSCODE", HirCicsOutputName::WebConverseStatusCode),
        ("STATUSTEXT", HirCicsOutputName::WebConverseStatusText),
        ("STATUSLEN", HirCicsOutputName::WebConverseStatusLength),
        ("MEDIATYPE", HirCicsOutputName::WebConverseMediaType),
        ("BODYCHARSET", HirCicsOutputName::WebConverseBodyCharset),
    ] {
        if let Some(tokens) = clauses.get(source) {
            out.push(HirCicsOutputBinding {
                name,
                target: complete_data_reference(tokens, semantic)?,
            });
        }
    }
    Ok(out)
}
