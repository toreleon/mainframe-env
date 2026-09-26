//! Source-selected task-channel and BTS container forms.

use super::super::{
    HirCicsNamedOperand, HirCicsOperandName as I, HirCicsOperation as P, HirCicsOption as F,
    HirCicsOutputBinding, HirCicsOutputName as O, HirCicsValue, Resolution, ResolutionFailure,
    require_writable,
};
use super::{Clauses, cics_integer_value, cics_value, complete_data_reference};
use crate::{CobolUsage, SemanticModel};
use mainframe_env_ir::CicsApplicationRegistryDescriptor;

pub(super) const fn is_channel_container(operation: P) -> bool {
    matches!(
        operation,
        P::DeleteChannel
            | P::DeleteContainer
            | P::GetContainer
            | P::MoveContainer
            | P::PutContainer
            | P::QueryChannel
    )
}

pub(super) fn reviewed_ambiguous_shape(
    descriptor: &CicsApplicationRegistryDescriptor,
    name: &str,
    has_value: bool,
) -> bool {
    has_value
        && matches!(
            (descriptor.label_tokens, name),
            (["GET", "CONTAINER"], "FLENGTH" | "CONVERTST") | (["PUT", "CONTAINER"], "DATATYPE")
        )
}

pub(super) fn allowed_clauses(operation: P) -> &'static [&'static str] {
    match operation {
        P::DeleteChannel => &["CHANNEL", "RESP", "RESP2"],
        P::DeleteContainer => &["CONTAINER", "CHANNEL", "ACTIVITY", "RESP", "RESP2"],
        P::GetContainer => &[
            "CONTAINER",
            "CHANNEL",
            "FLENGTH",
            "BYTEOFFSET",
            "INTO",
            "SET",
            "INTOCCSID",
            "INTOCODEPAGE",
            "CONVERTST",
            "CCSID",
            "ACTIVITY",
            "RESP",
            "RESP2",
        ],
        P::MoveContainer => &[
            "CONTAINER",
            "AS",
            "CHANNEL",
            "TOCHANNEL",
            "FROMACTIVITY",
            "TOACTIVITY",
            "RESP",
            "RESP2",
        ],
        P::PutContainer => &[
            "CONTAINER",
            "CHANNEL",
            "FROM",
            "FLENGTH",
            "DATATYPE",
            "FROMCCSID",
            "FROMCODEPAGE",
            "ACTIVITY",
            "RESP",
            "RESP2",
        ],
        P::QueryChannel => &["CHANNEL", "CONTAINERCNT", "RESP", "RESP2"],
        _ => unreachable!(),
    }
}

pub(super) fn allowed_options(operation: P) -> &'static [&'static str] {
    match operation {
        P::DeleteContainer => &["PROCESS", "ACQPROCESS", "ACQACTIVITY", "NOHANDLE"],
        P::GetContainer => &["PROCESS", "ACQPROCESS", "ACQACTIVITY", "NODATA", "NOHANDLE"],
        P::MoveContainer => &["FROMPROCESS", "TOPROCESS", "NOHANDLE"],
        P::PutContainer => &["PROCESS", "ACQPROCESS", "ACQACTIVITY", "APPEND", "NOHANDLE"],
        _ => &["NOHANDLE"],
    }
}

pub(super) fn required(operation: P) -> &'static [&'static str] {
    match operation {
        P::DeleteChannel | P::QueryChannel => &["CHANNEL"],
        P::DeleteContainer | P::GetContainer => &["CONTAINER"],
        P::MoveContainer => &["CONTAINER", "AS"],
        P::PutContainer => &["CONTAINER", "FROM"],
        _ => unreachable!(),
    }
}

pub(super) fn option(operation: P, name: &str) -> Option<F> {
    match (operation, name) {
        (P::GetContainer, "NODATA") => Some(F::ContainerNoData),
        (P::PutContainer, "APPEND") => Some(F::ContainerAppend),
        (P::DeleteContainer | P::GetContainer | P::PutContainer, "PROCESS") => {
            Some(F::ContainerProcess)
        }
        (P::DeleteContainer | P::GetContainer | P::PutContainer, "ACQPROCESS") => {
            Some(F::ContainerAcqProcess)
        }
        (P::DeleteContainer | P::GetContainer | P::PutContainer, "ACQACTIVITY") => {
            Some(F::ContainerAcqActivity)
        }
        (P::MoveContainer, "FROMPROCESS") => Some(F::ContainerFromProcess),
        (P::MoveContainer, "TOPROCESS") => Some(F::ContainerToProcess),
        _ => None,
    }
}

pub(super) fn validate(clauses: &Clauses, options: &[String], operation: P) -> Resolution<()> {
    if !is_channel_container(operation) {
        return Ok(());
    }
    let bts = clauses
        .keys()
        .any(|name| matches!(name.as_str(), "ACTIVITY" | "FROMACTIVITY" | "TOACTIVITY"))
        || options.iter().any(|name| {
            matches!(
                name.as_str(),
                "PROCESS" | "ACQPROCESS" | "ACQACTIVITY" | "FROMPROCESS" | "TOPROCESS"
            )
        });
    let channel = clauses.contains_key("CHANNEL") || clauses.contains_key("TOCHANNEL");
    let selectors = usize::from(clauses.contains_key("ACTIVITY"))
        + options
            .iter()
            .filter(|name| matches!(name.as_str(), "PROCESS" | "ACQPROCESS" | "ACQACTIVITY"))
            .count();
    let from_selectors = usize::from(clauses.contains_key("FROMACTIVITY"))
        + options
            .iter()
            .filter(|name| name.as_str() == "FROMPROCESS")
            .count();
    let to_selectors = usize::from(clauses.contains_key("TOACTIVITY"))
        + options
            .iter()
            .filter(|name| name.as_str() == "TOPROCESS")
            .count();
    if channel && bts
        || selectors > 1
        || from_selectors > 1
        || to_selectors > 1
        || matches!(operation, P::DeleteChannel | P::QueryChannel) && bts
    {
        return Err(ResolutionFailure::Invalid(
            "CICS container selectors cannot mix channel and BTS scopes".into(),
        ));
    }
    if bts
        && (clauses.contains_key("INTOCODEPAGE")
            || clauses.contains_key("CONVERTST")
            || clauses.contains_key("FROMCODEPAGE")
            || clauses.contains_key("DATATYPE")
            || clauses.contains_key("FROMCCSID")
            || clauses.contains_key("INTOCCSID")
            || clauses.contains_key("CCSID")
            || clauses.contains_key("BYTEOFFSET"))
    {
        return Err(ResolutionFailure::Invalid(
            "CICS BTS container conversion and metadata options are unsupported".into(),
        ));
    }
    if operation == P::GetContainer {
        let destinations = usize::from(clauses.contains_key("INTO"))
            + usize::from(clauses.contains_key("SET"))
            + usize::from(options.iter().any(|option| option == "NODATA"));
        if destinations != 1
            || (clauses.contains_key("SET") || options.iter().any(|option| option == "NODATA"))
                && !clauses.contains_key("FLENGTH")
        {
            return Err(ResolutionFailure::Invalid(
                "CICS GET CONTAINER requires one INTO, SET, or NODATA destination".into(),
            ));
        }
    }
    Ok(())
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: P,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if !is_channel_container(operation) {
        return Ok(Vec::new());
    }
    let names = [
        ("CHANNEL", I::BtsChannel),
        ("CONTAINER", I::ContainerName),
        ("AS", I::ContainerAs),
        ("ACTIVITY", I::ContainerActivity),
        ("FROMACTIVITY", I::ContainerFromActivity),
        ("TOACTIVITY", I::ContainerToActivity),
        ("TOCHANNEL", I::ContainerToChannel),
        ("FROM", I::ContainerFrom),
        ("FLENGTH", I::ContainerLength),
        ("DATATYPE", I::ContainerDatatype),
        ("FROMCCSID", I::ContainerCcsid),
        ("FROMCODEPAGE", I::ContainerFromCodepage),
        ("BYTEOFFSET", I::ContainerByteOffset),
        ("INTOCCSID", I::ContainerIntoCcsid),
        ("INTOCODEPAGE", I::ContainerIntoCodepage),
        ("CONVERTST", I::ContainerConvertst),
    ];
    let mut operands = Vec::new();
    for (label, name) in names {
        let Some(tokens) = clauses.get(label) else {
            continue;
        };
        if operation == P::GetContainer && label == "FLENGTH" && !clauses.contains_key("INTO") {
            continue;
        }
        let value = if matches!(label, "FLENGTH" | "BYTEOFFSET" | "FROMCCSID" | "INTOCCSID") {
            cics_integer_value(tokens, semantic)?
        } else if matches!(label, "DATATYPE" | "CONVERTST") {
            if let [function, open, word, close] = tokens.as_slice()
                && function.eq_ignore_ascii_case("DFHVALUE")
                && open == "("
                && close == ")"
            {
                HirCicsValue::Literal(word.clone())
            } else {
                cics_value(tokens, semantic)?
            }
        } else {
            cics_value(tokens, semantic)?
        };
        operands.push(HirCicsNamedOperand { name, value });
    }
    Ok(operands)
}

pub(super) fn outputs(
    clauses: &Clauses,
    operation: P,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    if !is_channel_container(operation) {
        return Ok(Vec::new());
    }
    let names: &[(&str, O)] = match operation {
        P::GetContainer => &[
            ("INTO", O::ContainerInto),
            ("SET", O::ContainerSet),
            ("FLENGTH", O::ContainerLength),
            ("CCSID", O::ContainerCcsid),
        ],
        P::QueryChannel => &[("CONTAINERCNT", O::ContainerCount)],
        _ => &[],
    };
    let mut outputs = Vec::new();
    for &(label, name) in names {
        let Some(tokens) = clauses.get(label) else {
            continue;
        };
        let target = complete_data_reference(tokens, semantic)?;
        require_writable(&target)?;
        if matches!(label, "FLENGTH" | "CCSID" | "CONTAINERCNT")
            && (target.usage != CobolUsage::Binary || target.length != 4 || target.scale != 0)
        {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS {label} requires fullword binary receiving storage"
            )));
        }
        outputs.push(HirCicsOutputBinding { name, target });
    }
    Ok(outputs)
}
