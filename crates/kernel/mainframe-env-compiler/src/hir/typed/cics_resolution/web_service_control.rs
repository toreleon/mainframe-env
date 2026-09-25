use super::super::{
    HirCicsNamedOperand, HirCicsOperandName as N, HirCicsOperation as P, HirCicsOutputBinding,
    HirCicsOutputName as O, HirCicsValue, Resolution, ResolutionFailure, require_writable,
};
use super::{Clauses, cics_integer_value, cics_value, complete_data_reference};
use crate::{CobolUsage, SemanticModel};

pub(super) struct Shape {
    pub clauses: &'static [&'static str],
    pub required: &'static [&'static str],
}

pub(super) fn shape(operation: P) -> Option<Shape> {
    let (clauses, required): (&[&str], &[&str]) = match operation {
        P::InvokeService => (
            &[
                "SERVICE",
                "OPERATION",
                "CHANNEL",
                "URI",
                "URIMAP",
                "SCOPE",
                "SCOPELEN",
                "RESP",
                "RESP2",
            ],
            &["SERVICE"],
        ),
        P::SoapFaultCreate => (
            &[
                "FAULTCODE",
                "FAULTCODESTR",
                "FAULTCODELEN",
                "FAULTSTRING",
                "FAULTSTRLEN",
                "NATLANG",
                "ROLE",
                "ROLELENGTH",
                "FAULTACTOR",
                "FAULTACTLEN",
                "DETAIL",
                "DETAILLENGTH",
                "FROMCCSID",
                "RESP",
                "RESP2",
            ],
            &["FAULTSTRING", "FAULTSTRLEN"],
        ),
        P::SoapFaultAdd => (
            &[
                "FAULTSTRING",
                "FAULTSTRLEN",
                "NATLANG",
                "SUBCODESTR",
                "SUBCODELEN",
                "FROMCCSID",
                "RESP",
                "RESP2",
            ],
            &[],
        ),
        P::SoapFaultDelete => (&["RESP", "RESP2"], &[]),
        P::WsaContextBuild => (
            &[
                "CHANNEL",
                "ACTION",
                "MESSAGEID",
                "RELATESURI",
                "RELATESTYPE",
                "EPRTYPE",
                "EPRFIELD",
                "EPRFROM",
                "EPRLENGTH",
                "FROMCCSID",
                "FROMCODEPAGE",
                "RESP",
                "RESP2",
            ],
            &[],
        ),
        P::WsaContextDelete => (&["CHANNEL", "RESP", "RESP2"], &[]),
        P::WsaContextGet => (
            &[
                "CONTEXTTYPE",
                "CHANNEL",
                "ACTION",
                "MESSAGEID",
                "RELATESURI",
                "RELATESTYPE",
                "RELATESINDEX",
                "EPRTYPE",
                "EPRFIELD",
                "EPRINTO",
                "EPRSET",
                "EPRLENGTH",
                "INTOCCSID",
                "INTOCODEPAGE",
                "RESP",
                "RESP2",
            ],
            &[],
        ),
        P::WsaEprCreate => (
            &[
                "EPRINTO",
                "EPRSET",
                "EPRLENGTH",
                "ADDRESS",
                "REFPARMS",
                "REFPARMSLEN",
                "METADATA",
                "METADATALEN",
                "FROMCCSID",
                "FROMCODEPAGE",
                "RESP",
                "RESP2",
            ],
            &["ADDRESS"],
        ),
        _ => return None,
    };
    Some(Shape { clauses, required })
}

pub(super) fn validate(clauses: &Clauses, operation: P) -> Resolution<()> {
    if shape(operation).is_none() {
        return Ok(());
    }
    for (data, length) in [
        ("SCOPE", "SCOPELEN"),
        ("FAULTCODESTR", "FAULTCODELEN"),
        ("FAULTSTRING", "FAULTSTRLEN"),
        ("ROLE", "ROLELENGTH"),
        ("FAULTACTOR", "FAULTACTLEN"),
        ("DETAIL", "DETAILLENGTH"),
        ("SUBCODESTR", "SUBCODELEN"),
        ("EPRFROM", "EPRLENGTH"),
        ("REFPARMS", "REFPARMSLEN"),
        ("METADATA", "METADATALEN"),
    ] {
        let applies = match (data, operation) {
            ("SCOPE", P::InvokeService)
            | ("FAULTCODESTR" | "ROLE" | "FAULTACTOR" | "DETAIL", P::SoapFaultCreate)
            | ("SUBCODESTR", P::SoapFaultAdd)
            | ("REFPARMS" | "METADATA", P::WsaEprCreate)
            | ("EPRFROM", P::WsaContextBuild) => true,
            ("FAULTSTRING", P::SoapFaultCreate | P::SoapFaultAdd) => true,
            _ => false,
        };
        if applies && clauses.contains_key(data) != clauses.contains_key(length) {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS {operation:?} requires {data} with {length}"
            )));
        }
    }
    if operation == P::SoapFaultCreate
        && clauses.contains_key("FAULTCODE") == clauses.contains_key("FAULTCODESTR")
        || operation == P::SoapFaultAdd
            && !clauses.contains_key("FAULTSTRING")
            && !clauses.contains_key("SUBCODESTR")
        || operation == P::InvokeService
            && clauses.contains_key("URI")
            && clauses.contains_key("URIMAP")
        || matches!(operation, P::WsaContextBuild | P::WsaEprCreate)
            && clauses.contains_key("FROMCCSID")
            && clauses.contains_key("FROMCODEPAGE")
        || operation == P::WsaContextGet
            && clauses.contains_key("INTOCCSID")
            && clauses.contains_key("INTOCODEPAGE")
        || operation == P::WsaContextBuild
            && clauses.contains_key("RELATESTYPE")
            && !clauses.contains_key("RELATESURI")
        || operation == P::WsaContextBuild
            && !["ACTION", "MESSAGEID", "RELATESURI", "EPRFROM"]
                .iter()
                .any(|name| clauses.contains_key(*name))
        || operation == P::WsaContextBuild
            && clauses.contains_key("EPRFROM")
            && (!clauses.contains_key("EPRTYPE") || !clauses.contains_key("EPRFIELD"))
        || operation == P::WsaEprCreate
            && clauses.contains_key("EPRINTO") == clauses.contains_key("EPRSET")
        || operation == P::WsaContextGet
            && clauses.contains_key("EPRINTO")
            && clauses.contains_key("EPRSET")
        || operation == P::WsaContextGet
            && (clauses.contains_key("EPRTYPE")
                != (clauses.contains_key("EPRINTO") || clauses.contains_key("EPRSET"))
                || clauses.contains_key("EPRFIELD")
                    != (clauses.contains_key("EPRINTO") || clauses.contains_key("EPRSET")))
        || matches!(operation, P::WsaContextGet | P::WsaEprCreate)
            && (clauses.contains_key("EPRINTO") || clauses.contains_key("EPRSET"))
            && !clauses.contains_key("EPRLENGTH")
    {
        return Err(ResolutionFailure::Invalid(format!(
            "CICS {operation:?} has an invalid web-service option combination"
        )));
    }
    Ok(())
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: P,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if shape(operation).is_none() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for (clause, name) in [
        ("SERVICE", N::Service),
        ("OPERATION", N::ServiceOperation),
        ("CHANNEL", N::Channel),
        ("URI", N::Uri),
        ("URIMAP", N::UriMap),
        ("SCOPE", N::Scope),
        ("FAULTCODESTR", N::FaultCodeStr),
        ("FAULTSTRING", N::FaultString),
        ("NATLANG", N::NatLang),
        ("ROLE", N::SoapRole),
        ("FAULTACTOR", N::FaultActor),
        ("DETAIL", N::Detail),
        ("SUBCODESTR", N::SubcodeStr),
        ("ACTION", N::Action),
        ("MESSAGEID", N::MessageId),
        ("RELATESURI", N::RelatesUri),
        ("RELATESTYPE", N::RelatesType),
        ("EPRFROM", N::EprFrom),
        ("FROMCODEPAGE", N::FromCodepage),
        ("INTOCODEPAGE", N::IntoCodepage),
        ("ADDRESS", N::Address),
        ("REFPARMS", N::RefParms),
        ("METADATA", N::Metadata),
    ] {
        if matches!(operation, P::WsaContextGet)
            && matches!(
                clause,
                "ACTION" | "MESSAGEID" | "RELATESURI" | "RELATESTYPE"
            )
        {
            continue;
        }
        if let Some(value) = clauses.get(clause) {
            result.push(HirCicsNamedOperand {
                name,
                value: cics_value(value, semantic)?,
            });
        }
    }
    for (clause, name) in [
        ("SCOPELEN", N::ScopeLen),
        ("FAULTCODELEN", N::FaultCodeLen),
        ("FAULTSTRLEN", N::FaultStrLen),
        ("ROLELENGTH", N::RoleLength),
        ("FAULTACTLEN", N::FaultActLen),
        ("DETAILLENGTH", N::DetailLength),
        ("FROMCCSID", N::FromCcsid),
        ("SUBCODELEN", N::SubcodeLen),
        ("RELATESINDEX", N::RelatesIndex),
        ("EPRLENGTH", N::EprLength),
        ("INTOCCSID", N::IntoCcsid),
        ("REFPARMSLEN", N::RefParmsLen),
        ("METADATALEN", N::MetadataLen),
    ] {
        if let Some(value) = clauses.get(clause) {
            result.push(HirCicsNamedOperand {
                name,
                value: cics_integer_value(value, semantic)?,
            });
        }
    }
    for (clause, name, allowed) in [
        (
            "FAULTCODE",
            N::FaultCode,
            &["CLIENT", "SERVER", "SENDER", "RECEIVER"][..],
        ),
        (
            "CONTEXTTYPE",
            N::ContextType,
            &["REQCONTEXT", "RESPCONTEXT"][..],
        ),
        (
            "EPRTYPE",
            N::EprType,
            &["TOEPR", "REPLYTOEPR", "FAULTTOEPR", "FROMEPR"][..],
        ),
        (
            "EPRFIELD",
            N::EprField,
            &["ADDRESS", "ALL", "METADATA", "REFPARMS"][..],
        ),
    ] {
        if let Some(tokens) = clauses.get(clause) {
            let [literal] = tokens.as_slice() else {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS {operation:?} {clause} requires a CVDA"
                )));
            };
            let literal = literal.trim_matches(['\'', '"']).to_ascii_uppercase();
            if !allowed.contains(&literal.as_str()) {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS {operation:?} {clause} has an invalid CVDA"
                )));
            }
            result.push(HirCicsNamedOperand {
                name,
                value: HirCicsValue::Literal(literal),
            });
        }
    }
    Ok(result)
}

pub(super) fn outputs(
    clauses: &Clauses,
    operation: P,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    if !matches!(operation, P::WsaContextGet | P::WsaEprCreate) {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for (clause, name) in [
        ("ACTION", O::WebAction),
        ("MESSAGEID", O::WebMessageId),
        ("RELATESURI", O::WebRelatesUri),
        ("RELATESTYPE", O::WebRelatesType),
        ("EPRINTO", O::WebEprInto),
        ("EPRSET", O::WebEprSet),
        ("EPRLENGTH", O::WebEprLength),
    ] {
        let Some(value) = clauses.get(clause) else {
            continue;
        };
        if operation == P::WsaEprCreate
            && matches!(
                clause,
                "ACTION" | "MESSAGEID" | "RELATESURI" | "RELATESTYPE"
            )
        {
            continue;
        }
        let target = complete_data_reference(value, semantic)?;
        require_writable(&target)?;
        if matches!(
            clause,
            "ACTION" | "MESSAGEID" | "RELATESURI" | "RELATESTYPE"
        ) && target.length != 255
            || clause == "EPRLENGTH"
                && (!matches!(target.usage, CobolUsage::Binary | CobolUsage::NativeBinary)
                    || target.length != 4
                    || target.scale != 0)
            || clause == "EPRSET"
                && !matches!(target.usage, CobolUsage::Pointer | CobolUsage::Pointer32)
        {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS {operation:?} {clause} has an invalid receiving storage shape"
            )));
        }
        result.push(HirCicsOutputBinding { name, target });
    }
    Ok(result)
}
