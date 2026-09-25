use super::super::HirCicsOption;
use super::*;

pub(super) fn options(clauses: &Clauses) -> Resolution<Vec<HirCicsOption>> {
    let mut result = Vec::new();
    if let Some(tokens) = clauses.get("CLIENTCONV") {
        if tokens.as_slice() != ["NOCLICONVERT"] {
            return Err(ResolutionFailure::Invalid(
                "CICS WEB RECEIVE supports CLIENTCONV(NOCLICONVERT)".into(),
            ));
        }
        result.push(HirCicsOption::WebNoClientConvert);
    }
    if let Some(tokens) = clauses.get("SERVERCONV") {
        if tokens.as_slice() != ["NOSRVCONVERT"] {
            return Err(ResolutionFailure::Invalid(
                "CICS WEB RECEIVE supports SERVERCONV(NOSRVCONVERT)".into(),
            ));
        }
        result.push(HirCicsOption::WebNoServerConvert);
    }
    Ok(result)
}

pub(super) fn validate(
    clauses: &Clauses,
    _options: &[String],
    semantic: &SemanticModel,
) -> Resolution<()> {
    let client = clauses.contains_key("SESSTOKEN");
    if clauses.contains_key("STATUSTEXT") != clauses.contains_key("STATUSLEN")
        || !client
            && ["STATUSCODE", "STATUSTEXT", "STATUSLEN"]
                .iter()
                .any(|name| clauses.contains_key(*name))
        || client && clauses.contains_key("SERVERCONV")
        || !client && clauses.contains_key("CLIENTCONV")
    {
        return Err(ResolutionFailure::Invalid(
            "CICS WEB RECEIVE options conflict with client or server role".into(),
        ));
    }
    if let Some(tokens) = clauses.get("SESSTOKEN") {
        let value = cics_value(tokens, semantic)?;
        if !matches!(&value, HirCicsValue::Data(reference) if reference.length == 8)
            && !matches!(&value, HirCicsValue::Literal(bytes) if bytes.len() == 8)
        {
            return Err(ResolutionFailure::Invalid(
                "CICS WEB RECEIVE SESSTOKEN requires eight bytes".into(),
            ));
        }
    }
    let maximum = cics_integer_value(&clauses["MAXLENGTH"], semantic)?;
    if matches!(maximum, HirCicsValue::Integer(value) if value <= 0) {
        return Err(ResolutionFailure::Invalid(
            "CICS WEB RECEIVE MAXLENGTH must be positive".into(),
        ));
    }
    if let HirCicsValue::Data(reference) = maximum
        && (reference.usage != CobolUsage::Binary || reference.length != 4 || reference.scale != 0)
    {
        return Err(ResolutionFailure::Invalid(
            "CICS WEB RECEIVE MAXLENGTH requires fullword binary input".into(),
        ));
    }
    let into = complete_data_reference(&clauses["INTO"], semantic)?;
    require_writable(&into)?;
    if into.length == 0 {
        return Err(ResolutionFailure::Invalid(
            "CICS WEB RECEIVE INTO is empty".into(),
        ));
    }
    fullword_target(&clauses["LENGTH"], semantic, "LENGTH")?;
    if let Some(tokens) = clauses.get("STATUSLEN") {
        fullword_target(tokens, semantic, "STATUSLEN")?;
    }
    for (name, length) in [("STATUSCODE", 2), ("MEDIATYPE", 56), ("BODYCHARSET", 40)] {
        if let Some(tokens) = clauses.get(name) {
            let target = complete_data_reference(tokens, semantic)?;
            require_writable(&target)?;
            if target.length != length {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS WEB RECEIVE {name} requires {length}-byte storage"
                )));
            }
        }
    }
    if let Some(tokens) = clauses.get("STATUSTEXT") {
        let target = complete_data_reference(tokens, semantic)?;
        require_writable(&target)?;
        if target.length == 0 || target.length > 256 {
            return Err(ResolutionFailure::Invalid(
                "CICS WEB RECEIVE STATUSTEXT requires one to 256 bytes".into(),
            ));
        }
    }
    Ok(())
}

pub(super) fn operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let mut out = Vec::new();
    if let Some(tokens) = clauses.get("SESSTOKEN") {
        out.push(HirCicsNamedOperand {
            name: HirCicsOperandName::WebSessionToken,
            value: cics_value(tokens, semantic)?,
        });
    }
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
        ("INTO", HirCicsOutputName::WebReceiveInto),
        ("LENGTH", HirCicsOutputName::WebReceiveLength),
        ("STATUSCODE", HirCicsOutputName::WebReceiveStatusCode),
        ("STATUSTEXT", HirCicsOutputName::WebReceiveStatusText),
        ("STATUSLEN", HirCicsOutputName::WebReceiveStatusLength),
        ("MEDIATYPE", HirCicsOutputName::WebReceiveMediaType),
        ("BODYCHARSET", HirCicsOutputName::WebReceiveBodyCharset),
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
