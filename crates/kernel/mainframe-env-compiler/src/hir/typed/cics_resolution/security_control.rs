use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsOutputBinding,
    HirCicsOutputName, HirCicsValue, HirDataReference, Resolution, ResolutionFailure,
    require_writable,
};
use super::{Clauses, cics_integer_value, cics_value, complete_data_reference};
use crate::{CobolUsage, DataCategory, SemanticModel};

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
pub(super) const VERIFY_PASSWORD_CLAUSES: &[&str] = &[
    "PASSWORD",
    "USERID",
    "GROUPID",
    "CHANGETIME",
    "DAYSLEFT",
    "ESMREASON",
    "ESMRESP",
    "EXPIRYTIME",
    "INVALIDCOUNT",
    "LASTUSETIME",
    "RESP",
    "RESP2",
];

pub(super) fn validate(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<()> {
    if operation == HirCicsOperation::VerifyPassword {
        return validate_verify_password(clauses, semantic);
    }
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
    if operation == HirCicsOperation::VerifyPassword {
        return verify_password_operands(clauses, semantic);
    }
    if operation != HirCicsOperation::QuerySecurity {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for (name, identity) in [
        ("RESCLASS", HirCicsOperandName::ResClass),
        ("RESID", HirCicsOperandName::ResId),
        ("RESTYPE", HirCicsOperandName::ResType),
        ("USERID", HirCicsOperandName::SecurityUserId),
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
    if operation == HirCicsOperation::VerifyPassword {
        return verify_password_outputs(clauses, semantic);
    }
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

fn validate_verify_password(clauses: &Clauses, semantic: &SemanticModel) -> Resolution<()> {
    if !clauses.contains_key("PASSWORD") || !clauses.contains_key("USERID") {
        return Err(ResolutionFailure::Invalid(
            "CICS VERIFY PASSWORD requires PASSWORD and USERID".into(),
        ));
    }
    let password = complete_data_reference(&clauses["PASSWORD"], semantic).map_err(|_| {
        ResolutionFailure::Invalid("CICS VERIFY PASSWORD requires resolved password storage".into())
    })?;
    if password.length != 8
        || !matches!(
            password.category,
            DataCategory::Alphabetic | DataCategory::Alphanumeric
        )
    {
        return Err(ResolutionFailure::Invalid(
            "CICS VERIFY PASSWORD requires an 8-character password data area".into(),
        ));
    }
    for name in ["USERID", "GROUPID"] {
        if let Some(value) = clauses.get(name) {
            let resolved = cics_value(value, semantic)?;
            if matches!(resolved, HirCicsValue::Literal(ref literal) if literal.is_empty() || literal.len() > 8)
                || matches!(resolved, HirCicsValue::Data(ref reference) if reference.length != 8)
            {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS VERIFY PASSWORD {name} requires up to eight characters"
                )));
            }
        }
    }
    Ok(())
}

fn verify_password_operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let mut out = vec![HirCicsNamedOperand {
        name: HirCicsOperandName::SecurityPassword,
        value: HirCicsValue::Data(complete_data_reference(&clauses["PASSWORD"], semantic)?),
    }];
    for (name, identity) in [
        ("USERID", HirCicsOperandName::SecurityUserId),
        ("GROUPID", HirCicsOperandName::SecurityGroupId),
    ] {
        if let Some(value) = clauses.get(name) {
            out.push(HirCicsNamedOperand {
                name: identity,
                value: cics_value(value, semantic)?,
            });
        }
    }
    Ok(out)
}

fn verify_password_outputs(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    let mut out = Vec::new();
    for (name, identity, width) in [
        ("CHANGETIME", HirCicsOutputName::SecurityChangeTime, 8),
        ("DAYSLEFT", HirCicsOutputName::SecurityDaysLeft, 2),
        ("ESMREASON", HirCicsOutputName::SecurityEsmReason, 4),
        ("ESMRESP", HirCicsOutputName::SecurityEsmResp, 4),
        ("EXPIRYTIME", HirCicsOutputName::SecurityExpiryTime, 8),
        ("INVALIDCOUNT", HirCicsOutputName::SecurityInvalidCount, 2),
        ("LASTUSETIME", HirCicsOutputName::SecurityLastUseTime, 8),
    ] {
        if let Some(value) = clauses.get(name) {
            let reference = complete_data_reference(value, semantic)?;
            require_writable(&reference)?;
            if width == 8 {
                if reference.usage != CobolUsage::PackedDecimal
                    || reference.length != 8
                    || reference.digits != 15
                    || reference.scale != 0
                    || !reference.signed
                {
                    return Err(ResolutionFailure::Invalid(format!(
                        "CICS VERIFY PASSWORD {name} requires PIC S9(15) COMP-3 storage"
                    )));
                }
            } else if reference.usage != CobolUsage::Binary
                || reference.length != width
                || reference.scale != 0
            {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS VERIFY PASSWORD {name} requires {width}-byte binary storage"
                )));
            }
            out.push(HirCicsOutputBinding {
                name: identity,
                target: reference,
            });
        }
    }
    Ok(out)
}
