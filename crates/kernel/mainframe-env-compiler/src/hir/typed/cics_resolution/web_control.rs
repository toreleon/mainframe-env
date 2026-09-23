use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsOutputBinding,
    HirCicsOutputName, HirCicsValue, Resolution, ResolutionFailure, require_writable,
};
use super::{Clauses, cics_integer_value, cics_value, complete_data_reference};
use crate::{CobolUsage, DataCategory, SemanticModel};
use mainframe_env_ir::CicsApplicationRegistryDescriptor;

pub(super) fn reviewed_ambiguous_shape(
    descriptor: &CicsApplicationRegistryDescriptor,
    name: &str,
    has_value: bool,
) -> bool {
    if has_value
        && matches!(
            descriptor.label_tokens,
            ["WEB", "EXTRACT"] | ["EXTRACT", "WEB"]
        )
        && EXTRACT_CLAUSES.contains(&name)
    {
        return true;
    }
    if has_value && descriptor.label_tokens == ["WEB", "READ"] && READ_CLAUSES.contains(&name) {
        return true;
    }
    has_value
        && matches!(
            (descriptor.label_tokens, name),
            (["WEB", "OPEN"], "SCHEME") | (["WEB", "PARSE", "URL"], "HOSTTYPE")
        )
}

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
pub(super) const OPEN_CLAUSES: &[&str] = &[
    "URIMAP",
    "HOST",
    "HOSTLENGTH",
    "PORTNUMBER",
    "SCHEME",
    "CERTIFICATE",
    "CODEPAGE",
    "SESSTOKEN",
    "HTTPVNUM",
    "HTTPRNUM",
    "RESP",
    "RESP2",
];
pub(super) const CLOSE_CLAUSES: &[&str] = &["SESSTOKEN", "RESP", "RESP2"];
pub(super) const EXTRACT_CLAUSES: &[&str] = &[
    "SESSTOKEN",
    "SCHEME",
    "HOST",
    "HOSTLENGTH",
    "HOSTTYPE",
    "HTTPMETHOD",
    "METHODLENGTH",
    "HTTPVERSION",
    "VERSIONLEN",
    "PATH",
    "PATHLENGTH",
    "PORTNUMBER",
    "QUERYSTRING",
    "QUERYSTRLEN",
    "REQUESTTYPE",
    "URIMAP",
    "REALM",
    "REALMLEN",
    "RESP",
    "RESP2",
];
pub(super) const READ_CLAUSES: &[&str] = &[
    "HTTPHEADER",
    "QUERYPARM",
    "FORMFIELD",
    "NAMELENGTH",
    "SESSTOKEN",
    "VALUE",
    "VALUELENGTH",
    "RESP",
    "RESP2",
];

pub(super) fn validate(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<()> {
    if matches!(
        operation,
        HirCicsOperation::WebExtract | HirCicsOperation::ExtractWeb
    ) {
        return validate_extract(clauses, semantic);
    }
    if operation == HirCicsOperation::WebRead {
        return validate_read(clauses, semantic);
    }
    if operation == HirCicsOperation::WebClose {
        let tokens = clauses.get("SESSTOKEN").ok_or_else(|| {
            ResolutionFailure::Invalid("CICS WEB CLOSE requires SESSTOKEN".into())
        })?;
        let value = cics_value(tokens, semantic)?;
        if let HirCicsValue::Data(reference) = &value
            && reference.length != 8
        {
            return Err(ResolutionFailure::Invalid(
                "CICS WEB CLOSE SESSTOKEN requires eight bytes".into(),
            ));
        }
        return Ok(());
    }
    if operation == HirCicsOperation::WebOpen {
        return validate_open(clauses, semantic);
    }
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

fn validate_open(clauses: &Clauses, semantic: &SemanticModel) -> Resolution<()> {
    let urimap = clauses.contains_key("URIMAP");
    let host = clauses.contains_key("HOST");
    if urimap == host {
        return Err(ResolutionFailure::Invalid(
            "CICS WEB OPEN requires exactly one URIMAP or HOST endpoint".into(),
        ));
    }
    if urimap {
        if ["HOSTLENGTH", "PORTNUMBER", "SCHEME", "CERTIFICATE"]
            .iter()
            .any(|name| clauses.contains_key(*name))
        {
            return Err(ResolutionFailure::Invalid(
                "CICS WEB OPEN URIMAP forbids direct-host endpoint options".into(),
            ));
        }
    } else if !clauses.contains_key("HOSTLENGTH") || !clauses.contains_key("SCHEME") {
        return Err(ResolutionFailure::Invalid(
            "CICS WEB OPEN HOST requires HOSTLENGTH and SCHEME".into(),
        ));
    }
    let token = clauses
        .get("SESSTOKEN")
        .ok_or_else(|| ResolutionFailure::Invalid("CICS WEB OPEN requires SESSTOKEN".into()))?;
    let target = complete_data_reference(token, semantic)?;
    require_writable(&target)?;
    if target.length != 8
        || !matches!(
            target.category,
            DataCategory::Alphabetic | DataCategory::Alphanumeric | DataCategory::Group
        )
    {
        return Err(ResolutionFailure::Invalid(
            "CICS WEB OPEN SESSTOKEN requires an eight-byte area".into(),
        ));
    }
    for name in ["HTTPVNUM", "HTTPRNUM"] {
        if let Some(tokens) = clauses.get(name) {
            let target = complete_data_reference(tokens, semantic)?;
            require_writable(&target)?;
            if target.usage != CobolUsage::Binary || target.length != 2 || target.scale != 0 {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS WEB OPEN {name} requires writable halfword binary storage"
                )));
            }
        }
    }
    if host {
        let host_value = cics_value(&clauses["HOST"], semantic)?;
        if !matches!(host_value, HirCicsValue::Literal(_) | HirCicsValue::Data(_)) {
            return Err(ResolutionFailure::Invalid(
                "CICS WEB OPEN HOST requires character input".into(),
            ));
        }
        let length = cics_integer_value(&clauses["HOSTLENGTH"], semantic)?;
        if matches!(length, HirCicsValue::Integer(value) if !(1..=255).contains(&value)) {
            return Err(ResolutionFailure::Invalid(
                "CICS WEB OPEN HOSTLENGTH is out of range".into(),
            ));
        }
        if let HirCicsValue::Data(reference) = length
            && (reference.usage != CobolUsage::Binary
                || reference.length != 4
                || reference.scale != 0)
        {
            return Err(ResolutionFailure::Invalid(
                "CICS WEB OPEN HOSTLENGTH requires fullword binary input".into(),
            ));
        }
        scheme_value(&clauses["SCHEME"])?;
    }
    if let Some(tokens) = clauses.get("PORTNUMBER") {
        let port = cics_integer_value(tokens, semantic)?;
        if matches!(port, HirCicsValue::Integer(number) if !(1..=65535).contains(&number)) {
            return Err(ResolutionFailure::Invalid(
                "CICS WEB OPEN PORTNUMBER is out of range".into(),
            ));
        }
    }
    for name in ["URIMAP", "CERTIFICATE", "CODEPAGE"] {
        if let Some(tokens) = clauses.get(name) {
            let value = cics_value(tokens, semantic)?;
            if matches!(value, HirCicsValue::Literal(ref value) if value.is_empty() || value.len() > if name == "CERTIFICATE" { 32 } else { 8 })
            {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS WEB OPEN {name} exceeds its source bound"
                )));
            }
        }
    }
    Ok(())
}

fn scheme_value(tokens: &[String]) -> Resolution<HirCicsValue> {
    let name = match tokens {
        [name] => name.as_str(),
        [function, open, name, close] if function == "DFHVALUE" && open == "(" && close == ")" => {
            name.as_str()
        }
        _ => {
            return Err(ResolutionFailure::Invalid(
                "CICS WEB OPEN SCHEME requires HTTP or HTTPS".into(),
            ));
        }
    };
    if !matches!(name, "HTTP" | "HTTPS") {
        return Err(ResolutionFailure::Invalid(
            "CICS WEB OPEN SCHEME requires HTTP or HTTPS".into(),
        ));
    }
    Ok(HirCicsValue::Literal(name.into()))
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
    if matches!(
        operation,
        HirCicsOperation::WebExtract | HirCicsOperation::ExtractWeb
    ) {
        return extract_operands(clauses, semantic);
    }
    if operation == HirCicsOperation::WebRead {
        return read_operands(clauses, semantic);
    }
    if operation == HirCicsOperation::WebClose {
        return Ok(vec![HirCicsNamedOperand {
            name: HirCicsOperandName::WebSessionToken,
            value: cics_value(&clauses["SESSTOKEN"], semantic)?,
        }]);
    }
    if operation == HirCicsOperation::WebOpen {
        return open_operands(clauses, semantic);
    }
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

fn open_operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let mut operands = Vec::new();
    for (source, name) in [
        ("HOST", HirCicsOperandName::WebHost),
        ("URIMAP", HirCicsOperandName::WebUriMap),
        ("CERTIFICATE", HirCicsOperandName::WebCertificate),
        ("CODEPAGE", HirCicsOperandName::WebCodePage),
    ] {
        if let Some(tokens) = clauses.get(source) {
            operands.push(HirCicsNamedOperand {
                name,
                value: cics_value(tokens, semantic)?,
            });
        }
    }
    if let Some(tokens) = clauses.get("SCHEME") {
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::WebScheme,
            value: scheme_value(tokens)?,
        });
    }
    for (source, name) in [
        ("HOSTLENGTH", HirCicsOperandName::WebHostLength),
        ("PORTNUMBER", HirCicsOperandName::WebPortNumber),
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

pub(super) fn outputs(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    if matches!(
        operation,
        HirCicsOperation::WebExtract | HirCicsOperation::ExtractWeb
    ) {
        return extract_outputs(clauses, semantic);
    }
    if operation == HirCicsOperation::WebRead {
        return read_outputs(clauses, semantic);
    }
    if operation == HirCicsOperation::WebOpen {
        return open_outputs(clauses, semantic);
    }
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

fn open_outputs(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    let mut outputs = Vec::new();
    for (source, name) in [
        ("SESSTOKEN", HirCicsOutputName::WebSessionToken),
        ("HTTPVNUM", HirCicsOutputName::WebHttpVNum),
        ("HTTPRNUM", HirCicsOutputName::WebHttpRNum),
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

fn validate_extract(clauses: &Clauses, semantic: &SemanticModel) -> Resolution<()> {
    let client = clauses.contains_key("SESSTOKEN");
    if client {
        let value = cics_value(&clauses["SESSTOKEN"], semantic)?;
        if !matches!(
            &value,
            HirCicsValue::Data(reference) if reference.length == 8
        ) && !matches!(&value, HirCicsValue::Literal(bytes) if bytes.len() == 8)
        {
            return Err(ResolutionFailure::Invalid(
                "CICS WEB EXTRACT SESSTOKEN requires eight bytes".into(),
            ));
        }
        if [
            "HTTPMETHOD",
            "METHODLENGTH",
            "QUERYSTRING",
            "QUERYSTRLEN",
            "REQUESTTYPE",
        ]
        .iter()
        .any(|name| clauses.contains_key(*name))
        {
            return Err(ResolutionFailure::Invalid(
                "CICS WEB EXTRACT client form forbids server-only results".into(),
            ));
        }
    } else if ["REALM", "REALMLEN"]
        .iter()
        .any(|name| clauses.contains_key(*name))
    {
        return Err(ResolutionFailure::Invalid(
            "CICS WEB EXTRACT server form forbids client REALM".into(),
        ));
    }
    for (area, length) in [
        ("HOST", "HOSTLENGTH"),
        ("HTTPMETHOD", "METHODLENGTH"),
        ("HTTPVERSION", "VERSIONLEN"),
        ("PATH", "PATHLENGTH"),
        ("QUERYSTRING", "QUERYSTRLEN"),
        ("REALM", "REALMLEN"),
    ] {
        if clauses.contains_key(area) != clauses.contains_key(length) {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS WEB EXTRACT {area} and {length} must occur together"
            )));
        }
        if let Some(tokens) = clauses.get(area) {
            let target = complete_data_reference(tokens, semantic)?;
            require_writable(&target)?;
            if target.length == 0 {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS WEB EXTRACT {area} requires receiving storage"
                )));
            }
        }
        if let Some(tokens) = clauses.get(length) {
            fullword_target(tokens, semantic, length)?;
        }
    }
    for name in ["SCHEME", "HOSTTYPE", "PORTNUMBER", "REQUESTTYPE"] {
        if let Some(tokens) = clauses.get(name) {
            fullword_target(tokens, semantic, name)?;
        }
    }
    if let Some(tokens) = clauses.get("URIMAP") {
        let target = complete_data_reference(tokens, semantic)?;
        require_writable(&target)?;
        if target.length != 8 {
            return Err(ResolutionFailure::Invalid(
                "CICS WEB EXTRACT URIMAP requires eight-byte storage".into(),
            ));
        }
    }
    if !EXTRACT_CLAUSES
        .iter()
        .filter(|name| !matches!(**name, "SESSTOKEN" | "RESP" | "RESP2"))
        .any(|name| clauses.contains_key(*name))
    {
        return Err(ResolutionFailure::Invalid(
            "CICS WEB EXTRACT requires a result area".into(),
        ));
    }
    Ok(())
}

fn extract_operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let mut operands = Vec::new();
    if let Some(tokens) = clauses.get("SESSTOKEN") {
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::WebSessionToken,
            value: cics_value(tokens, semantic)?,
        });
    }
    for (source, name) in [
        ("HOSTLENGTH", HirCicsOperandName::WebHostLength),
        ("METHODLENGTH", HirCicsOperandName::WebMethodLength),
        ("VERSIONLEN", HirCicsOperandName::WebVersionLength),
        ("PATHLENGTH", HirCicsOperandName::WebPathLength),
        ("QUERYSTRLEN", HirCicsOperandName::WebQueryStringLength),
        ("REALMLEN", HirCicsOperandName::WebRealmLength),
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

fn extract_outputs(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    let mut outputs = Vec::new();
    for (source, name) in [
        ("SCHEME", HirCicsOutputName::WebScheme),
        ("HOST", HirCicsOutputName::WebHost),
        ("HOSTLENGTH", HirCicsOutputName::WebHostLength),
        ("HOSTTYPE", HirCicsOutputName::WebHostType),
        ("HTTPMETHOD", HirCicsOutputName::WebHttpMethod),
        ("METHODLENGTH", HirCicsOutputName::WebMethodLength),
        ("HTTPVERSION", HirCicsOutputName::WebHttpVersion),
        ("VERSIONLEN", HirCicsOutputName::WebVersionLength),
        ("PATH", HirCicsOutputName::WebPath),
        ("PATHLENGTH", HirCicsOutputName::WebPathLength),
        ("PORTNUMBER", HirCicsOutputName::WebPortNumber),
        ("QUERYSTRING", HirCicsOutputName::WebQueryString),
        ("QUERYSTRLEN", HirCicsOutputName::WebQueryStringLength),
        ("REQUESTTYPE", HirCicsOutputName::WebRequestType),
        ("URIMAP", HirCicsOutputName::WebUriMap),
        ("REALM", HirCicsOutputName::WebRealm),
        ("REALMLEN", HirCicsOutputName::WebRealmLength),
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

fn validate_read(clauses: &Clauses, semantic: &SemanticModel) -> Resolution<()> {
    let selectors = ["HTTPHEADER", "QUERYPARM", "FORMFIELD"];
    let selected = selectors
        .iter()
        .filter(|name| clauses.contains_key(**name))
        .count();
    if selected != 1 {
        return Err(ResolutionFailure::Invalid(
            "CICS WEB READ requires exactly one HTTPHEADER, QUERYPARM, or FORMFIELD".into(),
        ));
    }
    if clauses.contains_key("SESSTOKEN") && !clauses.contains_key("HTTPHEADER") {
        return Err(ResolutionFailure::Invalid(
            "CICS WEB READ SESSTOKEN applies only to HTTPHEADER".into(),
        ));
    }
    for name in ["NAMELENGTH", "VALUE", "VALUELENGTH"] {
        if !clauses.contains_key(name) {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS WEB READ requires {name}"
            )));
        }
    }
    let selector = selectors
        .into_iter()
        .find(|name| clauses.contains_key(*name))
        .ok_or(ResolutionFailure::Unsupported)?;
    let name = cics_value(&clauses[selector], semantic)?;
    if !matches!(name, HirCicsValue::Literal(_) | HirCicsValue::Data(_)) {
        return Err(ResolutionFailure::Invalid(
            "CICS WEB READ name requires character input".into(),
        ));
    }
    let length = cics_integer_value(&clauses["NAMELENGTH"], semantic)?;
    if matches!(length, HirCicsValue::Integer(number) if number < 1) {
        return Err(ResolutionFailure::Invalid(
            "CICS WEB READ NAMELENGTH must be positive".into(),
        ));
    }
    if let HirCicsValue::Data(reference) = length
        && (reference.usage != CobolUsage::Binary || reference.length != 4 || reference.scale != 0)
    {
        return Err(ResolutionFailure::Invalid(
            "CICS WEB READ NAMELENGTH requires fullword binary input".into(),
        ));
    }
    fullword_target(&clauses["VALUELENGTH"], semantic, "VALUELENGTH")?;
    let value = complete_data_reference(&clauses["VALUE"], semantic)?;
    require_writable(&value)?;
    if value.length == 0 {
        return Err(ResolutionFailure::Invalid(
            "CICS WEB READ VALUE requires receiving storage".into(),
        ));
    }
    if let Some(tokens) = clauses.get("SESSTOKEN") {
        let value = cics_value(tokens, semantic)?;
        if !matches!(&value, HirCicsValue::Data(reference) if reference.length == 8)
            && !matches!(&value, HirCicsValue::Literal(bytes) if bytes.len() == 8)
        {
            return Err(ResolutionFailure::Invalid(
                "CICS WEB READ SESSTOKEN requires eight bytes".into(),
            ));
        }
    }
    Ok(())
}

fn read_operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let mut operands = Vec::new();
    for (source, name) in [
        ("HTTPHEADER", HirCicsOperandName::WebHttpHeaderName),
        ("QUERYPARM", HirCicsOperandName::WebQueryParmName),
        ("FORMFIELD", HirCicsOperandName::WebFormFieldName),
        ("SESSTOKEN", HirCicsOperandName::WebSessionToken),
    ] {
        if let Some(tokens) = clauses.get(source) {
            operands.push(HirCicsNamedOperand {
                name,
                value: cics_value(tokens, semantic)?,
            });
        }
    }
    operands.push(HirCicsNamedOperand {
        name: HirCicsOperandName::WebNameLength,
        value: cics_integer_value(&clauses["NAMELENGTH"], semantic)?,
    });
    operands.push(HirCicsNamedOperand {
        name: HirCicsOperandName::WebValueLength,
        value: HirCicsValue::Data(complete_data_reference(&clauses["VALUELENGTH"], semantic)?),
    });
    Ok(operands)
}

fn read_outputs(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    Ok(vec![
        HirCicsOutputBinding {
            name: HirCicsOutputName::WebValue,
            target: complete_data_reference(&clauses["VALUE"], semantic)?,
        },
        HirCicsOutputBinding {
            name: HirCicsOutputName::WebValueLength,
            target: complete_data_reference(&clauses["VALUELENGTH"], semantic)?,
        },
    ])
}
