use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsOutputBinding,
    HirCicsOutputName, HirCicsValue, HirDataReference, Resolution, ResolutionFailure,
    require_writable,
};
use super::{Clauses, cics_integer_value, cics_value, complete_data_reference};
use crate::{CobolUsage, SemanticModel};

pub(super) const QUERY_CLAUSES: &[&str] = &[
    "RESCLASS",
    "RESID",
    "RESIDLENGTH",
    "RESTYPE",
    "LOGMESSAGE",
    "USERID",
    "READ",
    "UPDATE",
    "CONTROL",
    "ALTER",
    "RESP",
    "RESP2",
];

pub(super) fn validate(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<()> {
    if operation != HirCicsOperation::QuerySecurity {
        return Ok(());
    }
    let custom = clauses.contains_key("RESCLASS");
    if !clauses.contains_key("RESID")
        || custom == clauses.contains_key("RESTYPE")
        || custom != clauses.contains_key("RESIDLENGTH")
        || !["READ", "UPDATE", "CONTROL", "ALTER"]
            .iter()
            .any(|name| clauses.contains_key(*name))
    {
        return Err(ResolutionFailure::Invalid(
            "CICS QUERY SECURITY requires RESID, exactly one of RESCLASS or RESTYPE, RESIDLENGTH with RESCLASS, and an access output".into(),
        ));
    }
    for (name, max) in [
        ("RESCLASS", 8),
        ("RESID", if custom { 246 } else { 12 }),
        ("RESTYPE", 12),
        ("USERID", 8),
    ] {
        if let Some(value) = clauses.get(name) {
            let resolved = cics_value(value, semantic)?;
            if matches!(resolved, HirCicsValue::Literal(ref literal) if literal.len() > max) {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS QUERY SECURITY {name} exceeds {max} bytes"
                )));
            }
        }
    }
    if let Some(value) = clauses.get("RESIDLENGTH") {
        let resolved = cics_integer_value(value, semantic)?;
        if matches!(resolved, HirCicsValue::Integer(length) if !(1..=246).contains(&length)) {
            return Err(ResolutionFailure::Invalid(
                "CICS QUERY SECURITY RESIDLENGTH must be 1 through 246".into(),
            ));
        }
        if let HirCicsValue::Data(reference) = resolved {
            fullword(&reference, "RESIDLENGTH")?;
        }
    }
    Ok(())
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if operation != HirCicsOperation::QuerySecurity {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for (name, identity) in [
        ("RESCLASS", HirCicsOperandName::ResClass),
        ("RESID", HirCicsOperandName::ResId),
        ("RESTYPE", HirCicsOperandName::ResType),
        ("USERID", HirCicsOperandName::UserId),
    ] {
        if let Some(value) = clauses.get(name) {
            out.push(HirCicsNamedOperand {
                name: identity,
                value: cics_value(value, semantic)?,
            });
        }
    }
    for (name, identity) in [
        ("RESIDLENGTH", HirCicsOperandName::ResIdLength),
        ("LOGMESSAGE", HirCicsOperandName::LogMessage),
    ] {
        if let Some(value) = clauses.get(name) {
            out.push(HirCicsNamedOperand {
                name: identity,
                value: super::cics_cvda_value(value, semantic)?,
            });
        }
    }
    Ok(out)
}

pub(super) fn outputs(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    if operation != HirCicsOperation::QuerySecurity {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for (name, identity) in [
        ("READ", HirCicsOutputName::SecurityRead),
        ("UPDATE", HirCicsOutputName::SecurityUpdate),
        ("CONTROL", HirCicsOutputName::SecurityControl),
        ("ALTER", HirCicsOutputName::SecurityAlter),
    ] {
        if let Some(value) = clauses.get(name) {
            let reference = complete_data_reference(value, semantic)?;
            require_writable(&reference)?;
            fullword(&reference, name)?;
            out.push(HirCicsOutputBinding {
                name: identity,
                target: reference,
            });
        }
    }
    Ok(out)
}

fn fullword(reference: &HirDataReference, name: &str) -> Resolution<()> {
    if reference.usage != CobolUsage::Binary || reference.length != 4 || reference.scale != 0 {
        return Err(ResolutionFailure::Invalid(format!(
            "CICS QUERY SECURITY {name} requires fullword binary storage"
        )));
    }
    Ok(())
}
