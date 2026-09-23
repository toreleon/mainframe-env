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
pub(super) const CHANGE_PASSWORD_CLAUSES: &[&str] = &[
    "PASSWORD",
    "NEWPASSWORD",
    "USERID",
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
pub(super) const CHANGE_PHRASE_CLAUSES: &[&str] = &[
    "PHRASE",
    "PHRASELEN",
    "NEWPHRASE",
    "NEWPHRASELEN",
    "USERID",
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
pub(super) const PASSTICKET_CLAUSES: &[&str] = &[
    "PASSTICKET",
    "ESMAPPNAME",
    "ESMRESP",
    "ESMREASON",
    "RESP",
    "RESP2",
];
pub(super) const SIGNON_CLAUSES: &[&str] = &[
    "USERID",
    "GROUPID",
    "PASSWORD",
    "NEWPASSWORD",
    "PHRASE",
    "PHRASELEN",
    "NEWPHRASE",
    "NEWPHRASELEN",
    "LANGUAGECODE",
    "NATLANG",
    "OIDCARD",
    "CHANGETIME",
    "DAYSLEFT",
    "ESMRESP",
    "ESMREASON",
    "EXPIRYTIME",
    "INVALIDCOUNT",
    "LASTUSETIME",
    "LANGINUSE",
    "NATLANGINUSE",
    "RESP",
    "RESP2",
];
pub(super) const VERIFY_PHRASE_CLAUSES: &[&str] = &[
    "PHRASE",
    "PHRASELEN",
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
    if operation == HirCicsOperation::ChangePassword {
        return validate_change_password(clauses, semantic);
    }
    if operation == HirCicsOperation::ChangePhrase {
        return validate_change_phrase(clauses, semantic);
    }
    if operation == HirCicsOperation::RequestPassTicket {
        return validate_passticket(clauses, semantic);
    }
    if operation == HirCicsOperation::Signon {
        return validate_signon(clauses, semantic);
    }
    if operation == HirCicsOperation::VerifyPhrase {
        return validate_verify_phrase(clauses, semantic);
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
    if operation == HirCicsOperation::ChangePassword {
        return change_password_operands(clauses, semantic);
    }
    if operation == HirCicsOperation::ChangePhrase {
        return change_phrase_operands(clauses, semantic);
    }
    if operation == HirCicsOperation::RequestPassTicket {
        let HirCicsValue::Data(application) = cics_value(&clauses["ESMAPPNAME"], semantic)? else {
            return Err(ResolutionFailure::Invalid(
                "CICS REQUEST PASSTICKET ESMAPPNAME requires storage".into(),
            ));
        };
        return Ok(vec![HirCicsNamedOperand {
            name: HirCicsOperandName::SecurityEsmAppName,
            value: HirCicsValue::Data(application),
        }]);
    }
    if operation == HirCicsOperation::Signon {
        return signon_operands(clauses, semantic);
    }
    if operation == HirCicsOperation::VerifyPhrase {
        return verify_phrase_operands(clauses, semantic);
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
    if operation == HirCicsOperation::RequestPassTicket {
        return passticket_outputs(clauses, semantic);
    }
    if operation == HirCicsOperation::Signon {
        let mut out = verify_credential_outputs(clauses, semantic, operation)?;
        for (name, identity, width) in [
            ("LANGINUSE", HirCicsOutputName::SecurityLangInUse, 3),
            ("NATLANGINUSE", HirCicsOutputName::SecurityNatLangInUse, 1),
        ] {
            if let Some(value) = clauses.get(name) {
                let reference = complete_data_reference(value, semantic)?;
                require_writable(&reference)?;
                if reference.length != width {
                    return Err(ResolutionFailure::Invalid(format!(
                        "CICS SIGNON {name} requires {width}-character storage"
                    )));
                }
                out.push(HirCicsOutputBinding {
                    name: identity,
                    target: reference,
                });
            }
        }
        return Ok(out);
    }
    if matches!(
        operation,
        HirCicsOperation::VerifyPassword
            | HirCicsOperation::VerifyPhrase
            | HirCicsOperation::ChangePassword
            | HirCicsOperation::ChangePhrase
    ) {
        return verify_credential_outputs(clauses, semantic, operation);
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

fn validate_passticket(clauses: &Clauses, semantic: &SemanticModel) -> Resolution<()> {
    if !clauses.contains_key("PASSTICKET") || !clauses.contains_key("ESMAPPNAME") {
        return Err(ResolutionFailure::Invalid(
            "CICS REQUEST PASSTICKET requires PASSTICKET and ESMAPPNAME".into(),
        ));
    }
    for name in ["PASSTICKET", "ESMAPPNAME"] {
        if name == "ESMAPPNAME"
            && !matches!(cics_value(&clauses[name], semantic)?, HirCicsValue::Data(_))
        {
            return Err(ResolutionFailure::Invalid(
                "CICS REQUEST PASSTICKET ESMAPPNAME requires storage".into(),
            ));
        }
        let reference = complete_data_reference(&clauses[name], semantic)?;
        if reference.length != 8
            || !matches!(
                reference.category,
                DataCategory::Alphabetic | DataCategory::Alphanumeric
            )
        {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS REQUEST PASSTICKET {name} requires an 8-character data area"
            )));
        }
        if name == "PASSTICKET" {
            require_writable(&reference)?;
        }
    }
    Ok(())
}

fn passticket_outputs(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    let mut out = Vec::new();
    for (name, identity) in [
        ("PASSTICKET", HirCicsOutputName::SecurityPassTicket),
        ("ESMRESP", HirCicsOutputName::SecurityEsmResp),
        ("ESMREASON", HirCicsOutputName::SecurityEsmReason),
    ] {
        if let Some(value) = clauses.get(name) {
            let reference = complete_data_reference(value, semantic)?;
            require_writable(&reference)?;
            if name == "PASSTICKET" {
                if reference.length != 8 {
                    return Err(ResolutionFailure::Invalid(
                        "CICS REQUEST PASSTICKET output must be eight characters".into(),
                    ));
                }
            } else {
                fullword(&reference, name)?;
            }
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
            "CICS {name} requires fullword binary storage"
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

fn validate_change_password(clauses: &Clauses, semantic: &SemanticModel) -> Resolution<()> {
    if !clauses.contains_key("NEWPASSWORD") {
        return Err(ResolutionFailure::Invalid(
            "CICS CHANGE PASSWORD requires NEWPASSWORD".into(),
        ));
    }
    validate_verify_password(clauses, semantic)?;
    let new_password =
        complete_data_reference(&clauses["NEWPASSWORD"], semantic).map_err(|_| {
            ResolutionFailure::Invalid(
                "CICS CHANGE PASSWORD requires resolved new password storage".into(),
            )
        })?;
    if new_password.length != 8
        || !matches!(
            new_password.category,
            DataCategory::Alphabetic | DataCategory::Alphanumeric
        )
    {
        return Err(ResolutionFailure::Invalid(
            "CICS CHANGE PASSWORD requires an 8-character new password data area".into(),
        ));
    }
    Ok(())
}

fn change_password_operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let mut out = verify_password_operands(clauses, semantic)?;
    out.push(HirCicsNamedOperand {
        name: HirCicsOperandName::SecurityNewPassword,
        value: HirCicsValue::Data(complete_data_reference(&clauses["NEWPASSWORD"], semantic)?),
    });
    Ok(out)
}

fn verify_credential_outputs(
    clauses: &Clauses,
    semantic: &SemanticModel,
    operation: HirCicsOperation,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    let label = match operation {
        HirCicsOperation::VerifyPhrase => "VERIFY PHRASE",
        HirCicsOperation::ChangePassword => "CHANGE PASSWORD",
        HirCicsOperation::ChangePhrase => "CHANGE PHRASE",
        HirCicsOperation::Signon => "SIGNON",
        _ => "VERIFY PASSWORD",
    };
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
                        "CICS {label} {name} requires PIC S9(15) COMP-3 storage"
                    )));
                }
            } else if reference.usage != CobolUsage::Binary
                || reference.length != width
                || reference.scale != 0
            {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS {label} {name} requires {width}-byte binary storage"
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

fn validate_verify_phrase(clauses: &Clauses, semantic: &SemanticModel) -> Resolution<()> {
    if !["PHRASE", "PHRASELEN", "USERID"]
        .iter()
        .all(|name| clauses.contains_key(*name))
    {
        return Err(ResolutionFailure::Invalid(
            "CICS VERIFY PHRASE requires PHRASE, PHRASELEN, and USERID".into(),
        ));
    }
    let phrase = complete_data_reference(&clauses["PHRASE"], semantic).map_err(|_| {
        ResolutionFailure::Invalid("CICS VERIFY PHRASE requires resolved phrase storage".into())
    })?;
    if !(1..=100).contains(&phrase.length)
        || !matches!(
            phrase.category,
            DataCategory::Alphabetic | DataCategory::Alphanumeric
        )
    {
        return Err(ResolutionFailure::Invalid(
            "CICS VERIFY PHRASE requires a 1- to 100-character data area".into(),
        ));
    }
    let length = cics_integer_value(&clauses["PHRASELEN"], semantic)?;
    match length {
        HirCicsValue::Integer(value)
            if !(1..=100).contains(&value) || value as usize > phrase.length =>
        {
            return Err(ResolutionFailure::Invalid(
                "CICS VERIFY PHRASE PHRASELEN must fit the phrase data area and be 1 through 100"
                    .into(),
            ));
        }
        HirCicsValue::Data(ref reference) => fullword(reference, "PHRASELEN")?,
        _ => {}
    }
    for name in ["USERID", "GROUPID"] {
        if let Some(value) = clauses.get(name) {
            let resolved = cics_value(value, semantic)?;
            if matches!(resolved, HirCicsValue::Literal(ref literal) if literal.is_empty() || literal.len() > 8)
                || matches!(resolved, HirCicsValue::Data(ref reference) if reference.length != 8)
            {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS VERIFY PHRASE {name} requires up to eight characters"
                )));
            }
        }
    }
    Ok(())
}

fn verify_phrase_operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let mut out = vec![
        HirCicsNamedOperand {
            name: HirCicsOperandName::SecurityPhrase,
            value: HirCicsValue::Data(complete_data_reference(&clauses["PHRASE"], semantic)?),
        },
        HirCicsNamedOperand {
            name: HirCicsOperandName::SecurityPhraseLen,
            value: cics_integer_value(&clauses["PHRASELEN"], semantic)?,
        },
    ];
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

fn validate_change_phrase(clauses: &Clauses, semantic: &SemanticModel) -> Resolution<()> {
    if !clauses.contains_key("NEWPHRASE") || !clauses.contains_key("NEWPHRASELEN") {
        return Err(ResolutionFailure::Invalid(
            "CICS CHANGE PHRASE requires NEWPHRASE and NEWPHRASELEN".into(),
        ));
    }
    validate_verify_phrase(clauses, semantic)?;
    let new_phrase = complete_data_reference(&clauses["NEWPHRASE"], semantic).map_err(|_| {
        ResolutionFailure::Invalid("CICS CHANGE PHRASE requires resolved new phrase storage".into())
    })?;
    if !(1..=100).contains(&new_phrase.length)
        || !matches!(
            new_phrase.category,
            DataCategory::Alphabetic | DataCategory::Alphanumeric
        )
    {
        return Err(ResolutionFailure::Invalid(
            "CICS CHANGE PHRASE requires a 1- to 100-character new phrase data area".into(),
        ));
    }
    let length = cics_integer_value(&clauses["NEWPHRASELEN"], semantic)?;
    match length {
        HirCicsValue::Integer(value)
            if !(1..=100).contains(&value) || value as usize > new_phrase.length =>
        {
            return Err(ResolutionFailure::Invalid(
                "CICS CHANGE PHRASE NEWPHRASELEN must fit the data area and be 1 through 100"
                    .into(),
            ));
        }
        HirCicsValue::Data(ref reference) => fullword(reference, "NEWPHRASELEN")?,
        _ => {}
    }
    Ok(())
}

fn change_phrase_operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let mut out = verify_phrase_operands(clauses, semantic)?;
    out.push(HirCicsNamedOperand {
        name: HirCicsOperandName::SecurityNewPhrase,
        value: HirCicsValue::Data(complete_data_reference(&clauses["NEWPHRASE"], semantic)?),
    });
    out.push(HirCicsNamedOperand {
        name: HirCicsOperandName::SecurityNewPhraseLen,
        value: cics_integer_value(&clauses["NEWPHRASELEN"], semantic)?,
    });
    Ok(out)
}

fn validate_signon(clauses: &Clauses, semantic: &SemanticModel) -> Resolution<()> {
    let password = clauses.contains_key("PASSWORD");
    let phrase = clauses.contains_key("PHRASE");
    if !clauses.contains_key("USERID")
        || password == phrase
        || phrase != clauses.contains_key("PHRASELEN")
        || clauses.contains_key("NEWPASSWORD") && !password
        || clauses.contains_key("NEWPHRASE") && !phrase
        || clauses.contains_key("NEWPHRASE") != clauses.contains_key("NEWPHRASELEN")
        || clauses.contains_key("LANGUAGECODE") && clauses.contains_key("NATLANG")
    {
        return Err(ResolutionFailure::Invalid(
            "CICS SIGNON requires USERID, one of PASSWORD or PHRASE, and matching length/new-credential options".into(),
        ));
    }
    for (name, width) in [("PASSWORD", 8), ("NEWPASSWORD", 8), ("OIDCARD", 65)] {
        if let Some(value) = clauses.get(name) {
            secret_storage(value, semantic, name, width, width)?;
        }
    }
    for name in ["PHRASE", "NEWPHRASE"] {
        if let Some(value) = clauses.get(name) {
            secret_storage(value, semantic, name, 1, 100)?;
        }
    }
    for (name, minimum, maximum, storage_name) in [
        ("PHRASELEN", 1, 100, "PHRASE"),
        ("NEWPHRASELEN", 0, 100, "NEWPHRASE"),
    ] {
        if let Some(value) = clauses.get(name) {
            let storage = complete_data_reference(&clauses[storage_name], semantic)?;
            match cics_integer_value(value, semantic)? {
                HirCicsValue::Integer(length)
                    if !(minimum..=maximum).contains(&length)
                        || length as usize > storage.length =>
                {
                    return Err(ResolutionFailure::Invalid(format!(
                        "CICS SIGNON {name} is out of range or exceeds storage"
                    )));
                }
                HirCicsValue::Data(reference) => fullword(&reference, name)?,
                _ => {}
            }
        }
    }
    for (name, width) in [
        ("USERID", 8),
        ("GROUPID", 8),
        ("LANGUAGECODE", 3),
        ("NATLANG", 1),
    ] {
        if let Some(value) = clauses.get(name) {
            match cics_value(value, semantic)? {
                HirCicsValue::Literal(ref literal)
                    if literal.is_empty() || literal.len() > width =>
                {
                    return Err(ResolutionFailure::Invalid(format!(
                        "CICS SIGNON {name} exceeds {width} characters"
                    )));
                }
                HirCicsValue::Data(ref reference) if reference.length != width => {
                    return Err(ResolutionFailure::Invalid(format!(
                        "CICS SIGNON {name} requires {width}-character storage"
                    )));
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn secret_storage(
    value: &[String],
    semantic: &SemanticModel,
    name: &str,
    minimum: usize,
    maximum: usize,
) -> Resolution<()> {
    let HirCicsValue::Data(reference) = cics_value(value, semantic)? else {
        return Err(ResolutionFailure::Invalid(format!(
            "CICS SIGNON {name} requires resolved secret storage"
        )));
    };
    if !(minimum..=maximum).contains(&reference.length)
        || !matches!(
            reference.category,
            DataCategory::Alphabetic | DataCategory::Alphanumeric
        )
    {
        return Err(ResolutionFailure::Invalid(format!(
            "CICS SIGNON {name} storage shape is invalid"
        )));
    }
    Ok(())
}

fn signon_operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let mut out = Vec::new();
    for (name, identity) in [
        ("USERID", HirCicsOperandName::SecurityUserId),
        ("GROUPID", HirCicsOperandName::SecurityGroupId),
        ("LANGUAGECODE", HirCicsOperandName::SecurityLanguageCode),
        ("NATLANG", HirCicsOperandName::SecurityNatLang),
    ] {
        if let Some(value) = clauses.get(name) {
            out.push(HirCicsNamedOperand {
                name: identity,
                value: cics_value(value, semantic)?,
            });
        }
    }
    for (name, identity) in [
        ("PASSWORD", HirCicsOperandName::SecurityPassword),
        ("NEWPASSWORD", HirCicsOperandName::SecurityNewPassword),
        ("PHRASE", HirCicsOperandName::SecurityPhrase),
        ("NEWPHRASE", HirCicsOperandName::SecurityNewPhrase),
        ("OIDCARD", HirCicsOperandName::SecurityOidCard),
    ] {
        if let Some(value) = clauses.get(name) {
            out.push(HirCicsNamedOperand {
                name: identity,
                value: HirCicsValue::Data(complete_data_reference(value, semantic)?),
            });
        }
    }
    for (name, identity) in [
        ("PHRASELEN", HirCicsOperandName::SecurityPhraseLen),
        ("NEWPHRASELEN", HirCicsOperandName::SecurityNewPhraseLen),
    ] {
        if let Some(value) = clauses.get(name) {
            out.push(HirCicsNamedOperand {
                name: identity,
                value: cics_integer_value(value, semantic)?,
            });
        }
    }
    Ok(out)
}
