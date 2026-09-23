use super::*;

pub(super) fn validate(clauses: &Clauses, semantic: &SemanticModel) -> Resolution<()> {
    let client = clauses.contains_key("SESSTOKEN");
    let body = clauses.contains_key("FROM");
    let document = clauses.contains_key("DOCTOKEN");
    if client != clauses.contains_key("METHOD") {
        return Err(ResolutionFailure::Invalid(
            "CICS WEB SEND client form requires SESSTOKEN and METHOD".into(),
        ));
    }
    if body == document && (body || !client) {
        return Err(ResolutionFailure::Invalid(
            "CICS WEB SEND requires exactly one FROM or DOCTOKEN body in server form".into(),
        ));
    }
    for (area, length) in [
        ("FROM", "FROMLENGTH"),
        ("PATH", "PATHLENGTH"),
        ("QUERYSTRING", "QUERYSTRLEN"),
        ("STATUSTEXT", "STATUSLEN"),
    ] {
        if clauses.contains_key(area) != clauses.contains_key(length) {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS WEB SEND {area} and {length} must occur together"
            )));
        }
    }
    if client
        && ["STATUSCODE", "STATUSTEXT", "STATUSLEN", "ACTION"]
            .iter()
            .any(|name| clauses.contains_key(*name))
        || !client
            && ["PATH", "PATHLENGTH", "QUERYSTRING", "QUERYSTRLEN", "URIMAP"]
                .iter()
                .any(|name| clauses.contains_key(*name))
    {
        return Err(ResolutionFailure::Invalid(
            "CICS WEB SEND options conflict with client or server role".into(),
        ));
    }
    if client && clauses.contains_key("PATH") && clauses.contains_key("URIMAP") {
        return Err(ResolutionFailure::Invalid(
            "CICS WEB SEND PATH and URIMAP are exclusive".into(),
        ));
    }
    for (name, allowed) in [
        (
            "METHOD",
            &[
                "GET", "HEAD", "PATCH", "POST", "PUT", "TRACE", "OPTIONS", "DELETE",
            ][..],
        ),
        ("ACTION", &["IMMEDIATE", "EVENTUAL"][..]),
        ("CLOSESTATUS", &["CLOSE", "NOCLOSE"][..]),
    ] {
        if let Some(tokens) = clauses.get(name) {
            cvda_literal(tokens, allowed, name)?;
        }
    }
    for (name, length) in [("SESSTOKEN", 8), ("DOCTOKEN", 16)] {
        if let Some(tokens) = clauses.get(name) {
            let value = cics_value(tokens, semantic)?;
            if !matches!(&value, HirCicsValue::Data(reference) if reference.length == length)
                && !matches!(&value, HirCicsValue::Literal(bytes) if bytes.len() == length)
            {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS WEB SEND {name} requires {length} bytes"
                )));
            }
        }
    }
    for name in [
        "FROM",
        "PATH",
        "QUERYSTRING",
        "STATUSTEXT",
        "MEDIATYPE",
        "URIMAP",
    ] {
        if let Some(tokens) = clauses.get(name) {
            let value = cics_value(tokens, semantic)?;
            if !matches!(value, HirCicsValue::Literal(_) | HirCicsValue::Data(_)) {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS WEB SEND {name} requires character or byte input"
                )));
            }
        }
    }
    for name in ["FROMLENGTH", "PATHLENGTH", "QUERYSTRLEN", "STATUSLEN"] {
        if let Some(tokens) = clauses.get(name) {
            let value = cics_integer_value(tokens, semantic)?;
            if matches!(&value, HirCicsValue::Integer(number) if *number < 1) {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS WEB SEND {name} must be positive"
                )));
            }
            if let HirCicsValue::Data(reference) = value
                && (reference.usage != CobolUsage::Binary
                    || reference.length != 4
                    || reference.scale != 0)
            {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS WEB SEND {name} requires fullword binary input"
                )));
            }
        }
    }
    if let Some(tokens) = clauses.get("STATUSCODE") {
        let value = cics_integer_value(tokens, semantic)?;
        if matches!(&value, HirCicsValue::Integer(number) if !(100..=599).contains(number)) {
            return Err(ResolutionFailure::Invalid(
                "CICS WEB SEND STATUSCODE is out of range".into(),
            ));
        }
        if let HirCicsValue::Data(reference) = value
            && (reference.usage != CobolUsage::Binary
                || reference.length != 2
                || reference.scale != 0)
        {
            return Err(ResolutionFailure::Invalid(
                "CICS WEB SEND STATUSCODE requires halfword binary input".into(),
            ));
        }
    }
    if client {
        let method = cvda_literal(
            &clauses["METHOD"],
            &[
                "GET", "HEAD", "PATCH", "POST", "PUT", "TRACE", "OPTIONS", "DELETE",
            ],
            "METHOD",
        )?;
        let HirCicsValue::Literal(method) = method else {
            unreachable!()
        };
        if matches!(method.as_str(), "GET" | "HEAD" | "TRACE" | "DELETE") && (body || document)
            || matches!(method.as_str(), "PATCH" | "POST" | "PUT") && !(body || document)
        {
            return Err(ResolutionFailure::Invalid(
                "CICS WEB SEND METHOD body legality failed".into(),
            ));
        }
    }
    Ok(())
}

pub(super) fn operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let mut operands = Vec::new();
    for (source, name) in [
        ("SESSTOKEN", HirCicsOperandName::WebSessionToken),
        ("DOCTOKEN", HirCicsOperandName::WebDocumentToken),
        ("FROM", HirCicsOperandName::WebFrom),
        ("PATH", HirCicsOperandName::WebPathInput),
        ("QUERYSTRING", HirCicsOperandName::WebQueryInput),
        ("MEDIATYPE", HirCicsOperandName::WebMediaType),
        ("URIMAP", HirCicsOperandName::WebSendUriMap),
        ("STATUSTEXT", HirCicsOperandName::WebStatusText),
    ] {
        if let Some(tokens) = clauses.get(source) {
            operands.push(HirCicsNamedOperand {
                name,
                value: cics_value(tokens, semantic)?,
            });
        }
    }
    for (source, name, allowed) in [
        (
            "METHOD",
            HirCicsOperandName::WebMethod,
            &[
                "GET", "HEAD", "PATCH", "POST", "PUT", "TRACE", "OPTIONS", "DELETE",
            ][..],
        ),
        (
            "ACTION",
            HirCicsOperandName::WebAction,
            &["IMMEDIATE", "EVENTUAL"][..],
        ),
        (
            "CLOSESTATUS",
            HirCicsOperandName::WebCloseStatus,
            &["CLOSE", "NOCLOSE"][..],
        ),
    ] {
        if let Some(tokens) = clauses.get(source) {
            operands.push(HirCicsNamedOperand {
                name,
                value: cvda_literal(tokens, allowed, source)?,
            });
        }
    }
    for (source, name) in [
        ("FROMLENGTH", HirCicsOperandName::WebFromLength),
        ("PATHLENGTH", HirCicsOperandName::WebPathLength),
        ("QUERYSTRLEN", HirCicsOperandName::WebQueryStringLength),
        ("STATUSLEN", HirCicsOperandName::WebStatusLength),
        ("STATUSCODE", HirCicsOperandName::WebStatusCode),
    ] {
        if let Some(tokens) = clauses.get(source) {
            operands.push(HirCicsNamedOperand {
                name,
                value: cics_integer_value(tokens, semantic)?,
            });
        }
    }
    Ok(operands)
}

fn cvda_literal(tokens: &[String], allowed: &[&str], name: &str) -> Resolution<HirCicsValue> {
    let value = match tokens {
        [value] => value.as_str(),
        [function, open, value, close] if function == "DFHVALUE" && open == "(" && close == ")" => {
            value.as_str()
        }
        _ => {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS WEB SEND {name} requires a reviewed CVDA"
            )));
        }
    };
    if !allowed.contains(&value) {
        return Err(ResolutionFailure::Invalid(format!(
            "CICS WEB SEND {name} is unsupported"
        )));
    }
    Ok(HirCicsValue::Literal(value.into()))
}
