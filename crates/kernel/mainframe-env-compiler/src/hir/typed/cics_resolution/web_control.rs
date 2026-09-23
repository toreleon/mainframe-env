use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsOutputBinding,
    HirCicsOutputName, HirCicsValue, Resolution, ResolutionFailure, require_writable,
};
use super::{Clauses, cics_integer_value, cics_value, complete_data_reference};
use crate::{CobolUsage, DataCategory, SemanticModel};

pub(super) const PARSE_URL_CLAUSES: &[&str] = &[
    "URL",
    "URLLENGTH",
    "SCHEMENAME",
    "HOST",
    "HOSTLENGTH",
    "HOSTTYPE",
    "PORTNUMBER",
    "PATH",
    "PATHLENGTH",
    "QUERYSTRING",
    "QUERYSTRLEN",
    "RESP",
    "RESP2",
];

pub(super) fn validate(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<()> {
    if operation != HirCicsOperation::WebParseUrl {
        return Ok(());
    }
    for (buffer, length) in [
        ("HOST", "HOSTLENGTH"),
        ("PATH", "PATHLENGTH"),
        ("QUERYSTRING", "QUERYSTRLEN"),
    ] {
        if clauses.contains_key(buffer) != clauses.contains_key(length) {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS WEB PARSE URL {buffer} and {length} must occur together"
            )));
        }
        if let Some(tokens) = clauses.get(length) {
            fullword_target(tokens, semantic, length)?;
        }
    }
    if let Some(tokens) = clauses.get("SCHEMENAME") {
        let target = complete_data_reference(tokens, semantic)?;
        require_writable(&target)?;
        if target.length != 16 {
            return Err(ResolutionFailure::Invalid(
                "CICS WEB PARSE URL SCHEMENAME requires 16-byte storage".into(),
            ));
        }
    }
    for name in ["HOST", "PATH", "QUERYSTRING"] {
        if let Some(tokens) = clauses.get(name) {
            let target = complete_data_reference(tokens, semantic)?;
            require_writable(&target)?;
            if target.length == 0 {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS WEB PARSE URL {name} requires receiving storage"
                )));
            }
        }
    }
    for name in ["HOSTTYPE", "PORTNUMBER"] {
        if let Some(tokens) = clauses.get(name) {
            fullword_target(tokens, semantic, name)?;
        }
    }
    let url_tokens = clauses
        .get("URL")
        .ok_or_else(|| ResolutionFailure::Invalid("CICS WEB PARSE URL requires URL".into()))?;
    let length_tokens = clauses.get("URLLENGTH").ok_or_else(|| {
        ResolutionFailure::Invalid("CICS WEB PARSE URL requires URLLENGTH".into())
    })?;
    let url = cics_value(url_tokens, semantic)?;
    if !matches!(url, HirCicsValue::Literal(_) | HirCicsValue::Data(_)) {
        return Err(ResolutionFailure::Invalid(
            "CICS WEB PARSE URL URL requires character input".into(),
        ));
    }
    let length = cics_integer_value(length_tokens, semantic)?;
    if matches!(length, HirCicsValue::Integer(value) if value < 1) {
        return Err(ResolutionFailure::Invalid(
            "CICS WEB PARSE URL URLLENGTH must be positive".into(),
        ));
    }
    if let HirCicsValue::Data(reference) = length
        && (reference.usage != CobolUsage::Binary || reference.length != 4 || reference.scale != 0)
    {
        return Err(ResolutionFailure::Invalid(
            "CICS WEB PARSE URL URLLENGTH requires fullword binary input".into(),
        ));
    }
    if ![
        "SCHEMENAME",
        "HOST",
        "HOSTTYPE",
        "PORTNUMBER",
        "PATH",
        "QUERYSTRING",
    ]
    .iter()
    .any(|name| clauses.contains_key(*name))
    {
        return Err(ResolutionFailure::Invalid(
            "CICS WEB PARSE URL requires at least one result area".into(),
        ));
    }
    Ok(())
}

fn fullword_target(tokens: &[String], semantic: &SemanticModel, name: &str) -> Resolution<()> {
    let target = complete_data_reference(tokens, semantic)?;
    require_writable(&target)?;
    if target.usage != CobolUsage::Binary
        || target.category != DataCategory::Binary
        || target.length != 4
        || target.scale != 0
    {
        return Err(ResolutionFailure::Invalid(format!(
            "CICS WEB PARSE URL {name} requires writable fullword binary storage"
        )));
    }
    Ok(())
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if operation != HirCicsOperation::WebParseUrl {
        return Ok(Vec::new());
    }
    let mut operands = vec![
        HirCicsNamedOperand {
            name: HirCicsOperandName::WebUrl,
            value: cics_value(&clauses["URL"], semantic)?,
        },
        HirCicsNamedOperand {
            name: HirCicsOperandName::WebUrlLength,
            value: cics_integer_value(&clauses["URLLENGTH"], semantic)?,
        },
    ];
    for (source, name) in [
        ("HOSTLENGTH", HirCicsOperandName::WebHostLength),
        ("PATHLENGTH", HirCicsOperandName::WebPathLength),
        ("QUERYSTRLEN", HirCicsOperandName::WebQueryStringLength),
    ] {
        if let Some(tokens) = clauses.get(source) {
            operands.push(HirCicsNamedOperand {
                name,
                value: HirCicsValue::Data(complete_data_reference(tokens, semantic)?),
            });
        }
    }
    Ok(operands)
}

pub(super) fn outputs(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    if operation != HirCicsOperation::WebParseUrl {
        return Ok(Vec::new());
    }
    let mut outputs = Vec::new();
    for (source, name) in [
        ("SCHEMENAME", HirCicsOutputName::WebSchemeName),
        ("HOST", HirCicsOutputName::WebHost),
        ("HOSTLENGTH", HirCicsOutputName::WebHostLength),
        ("HOSTTYPE", HirCicsOutputName::WebHostType),
        ("PORTNUMBER", HirCicsOutputName::WebPortNumber),
        ("PATH", HirCicsOutputName::WebPath),
        ("PATHLENGTH", HirCicsOutputName::WebPathLength),
        ("QUERYSTRING", HirCicsOutputName::WebQueryString),
        ("QUERYSTRLEN", HirCicsOutputName::WebQueryStringLength),
    ] {
        if let Some(tokens) = clauses.get(source) {
            outputs.push(HirCicsOutputBinding {
                name,
                target: complete_data_reference(tokens, semantic)?,
            });
        }
    }
    Ok(outputs)
}
