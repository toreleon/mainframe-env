//! Source-bound MQI call-shape validation. This module has no dispatch or coverage authority.

use crate::{
    MqMqiContractDescriptor, MqMqiHandleRole, MqMqiParameterDescriptor, MqMqiParameterDirection,
    MqMqiParameterRole, MqMqiSignatureStatus, mq_mqi_contract_by_label,
};

/// The source of a validation rule. The call signature binds every generic rule
/// to one official row and exact pinned topic; special rules check their pin below.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqValidationSource {
    pub official_row: &'static str,
    pub topic_path: &'static str,
    pub topic_sha256: &'static str,
}

impl From<&'static MqMqiContractDescriptor> for MqValidationSource {
    fn from(call: &'static MqMqiContractDescriptor) -> Self {
        Self {
            official_row: call.official_row,
            topic_path: call.topic_path,
            topic_sha256: call.topic_sha256,
        }
    }
}

/// Values are shape observations, not MQI execution arguments. A byte area is
/// its supplied capacity, and an array is its element count.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MqArgumentValue<'a> {
    /// A source-permitted null pointer in an otherwise present parameter slot.
    Null,
    Handle(MqMqiHandleRole),
    Scalar(i64),
    SymbolicLength(&'a str),
    ByteArea(usize),
    ArraySlots(usize),
    Structure {
        identity: &'a str,
        version: MqStructureVersion<'a>,
        options: &'a [&'a str],
    },
    Symbols(&'a [&'a str]),
    OutputSlot,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MqStructureVersion<'a> {
    /// A symbolic family identity only; no numeric version is inferred.
    Symbolic(&'a str),
    /// A numeric version needs a separately pinned legality source.
    Numeric(i32),
    NotApplicable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqArgument<'a> {
    pub name: &'a str,
    pub value: MqArgumentValue<'a>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqPendingDisposition {
    NumericVersionNotStated,
    OptionValueLegalityNotStated,
    SelectorValueLegalityNotStated,
    ObjectApplicabilityNotChecked,
    PartialAttributeOutput,
    AttributeWidthNotResolved,
    NullTerminationNotInspected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqExecutionDisposition {
    /// The identity and shape contract does not register an MQI handler.
    Unsupported,
}

/// A successful report confirms only the listed source-backed shape checks.
/// It never authorizes an MQI handler or grants conformance/licensed credit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqValidationReport {
    pub source: MqValidationSource,
    pub pending: Vec<MqPendingDisposition>,
    pub execution: MqExecutionDisposition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqValidationProblem {
    UnknownCall,
    PendingSignature,
    SourcePinMismatch,
    ParameterCount,
    ParameterName,
    ParameterShape,
    StructureIdentity,
    StructureVersionIdentity,
    SymbolFamily,
    InvalidCombination,
    LengthRelationship,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqValidationError {
    pub source: Option<MqValidationSource>,
    pub parameter: Option<&'static str>,
    pub problem: MqValidationProblem,
}

fn error(
    call: &'static MqMqiContractDescriptor,
    parameter: Option<&'static str>,
    problem: MqValidationProblem,
) -> MqValidationError {
    MqValidationError {
        source: Some(call.into()),
        parameter,
        problem,
    }
}

fn pin(
    call: &'static MqMqiContractDescriptor,
    row_suffix: &str,
    digest: &str,
) -> Result<(), MqValidationError> {
    if call.official_row.ends_with(row_suffix) && call.topic_sha256 == digest {
        Ok(())
    } else {
        Err(error(call, None, MqValidationProblem::SourcePinMismatch))
    }
}

fn record(pending: &mut Vec<MqPendingDisposition>, item: MqPendingDisposition) {
    if !pending.contains(&item) {
        pending.push(item);
    }
}

fn symbol_family(symbol: &str, family: &str) -> bool {
    symbol
        .strip_prefix(family)
        .is_some_and(|tail| tail.starts_with('_') && tail.len() > 1)
}

fn long_array(data_type: &str) -> bool {
    data_type.starts_with("MQLONGx") || data_type.starts_with("MQLONG x ")
}

fn check_symbols(
    call: &'static MqMqiContractDescriptor,
    parameter: &'static MqMqiParameterDescriptor,
    symbols: &[&str],
    pending: &mut Vec<MqPendingDisposition>,
) -> Result<(), MqValidationError> {
    if symbols.iter().any(|symbol| {
        !parameter
            .symbolic_identities
            .iter()
            .any(|family| symbol_family(symbol, family))
    }) {
        return Err(error(
            call,
            Some(parameter.name),
            MqValidationProblem::SymbolFamily,
        ));
    }
    let disposition = if parameter.has_role(MqMqiParameterRole::Selector) {
        MqPendingDisposition::SelectorValueLegalityNotStated
    } else {
        MqPendingDisposition::OptionValueLegalityNotStated
    };
    if !symbols.is_empty() {
        record(pending, disposition);
    }
    Ok(())
}

fn check_parameter(
    call: &'static MqMqiContractDescriptor,
    parameter: &'static MqMqiParameterDescriptor,
    value: &MqArgumentValue<'_>,
    pending: &mut Vec<MqPendingDisposition>,
) -> Result<(), MqValidationError> {
    use MqArgumentValue as V;
    use MqMqiParameterRole as R;
    if matches!(value, V::OutputSlot) && parameter.direction == MqMqiParameterDirection::Output {
        return Ok(());
    }
    match value {
        V::Null
            if (call.label == "MQCB" && parameter.name == "CallbackDesc")
                || (call.label == "MQSUBRQ" && parameter.name == "SubRqOpts") => {}
        V::Handle(role)
            if parameter.has_role(R::Handle) && parameter.handle_role == Some(*role) => {}
        V::Scalar(number)
            if (parameter.has_role(R::Length) || parameter.has_role(R::Scalar))
                && i32::try_from(*number).is_ok() => {}
        V::SymbolicLength("MQVL_NULL_TERMINATED")
            if call.label == "MQSETMP" && parameter.name == "ValueLength" => {}
        V::ByteArea(size) if parameter.data_type == "MQCHAR48" && *size != 48 => {
            return Err(error(
                call,
                Some(parameter.name),
                MqValidationProblem::LengthRelationship,
            ));
        }
        V::ByteArea(_)
            if !long_array(parameter.data_type)
                && (parameter.has_role(R::Data) || parameter.has_role(R::Name)) => {}
        V::ArraySlots(_) if long_array(parameter.data_type) => {}
        V::Symbols(symbols)
            if parameter.has_role(R::Options) || parameter.has_role(R::Selector) =>
        {
            check_symbols(call, parameter, symbols, pending)?;
        }
        V::Structure {
            identity,
            version,
            options,
        } if parameter.has_role(R::Structure) => {
            if identity != &parameter.data_type {
                return Err(error(
                    call,
                    Some(parameter.name),
                    MqValidationProblem::StructureIdentity,
                ));
            }
            match (parameter.structure_version_identity, version) {
                (Some(expected), MqStructureVersion::Symbolic(actual)) if expected == *actual => {}
                (Some(_), MqStructureVersion::Numeric(_)) => {
                    record(pending, MqPendingDisposition::NumericVersionNotStated);
                }
                (None, MqStructureVersion::NotApplicable) => {}
                _ => {
                    return Err(error(
                        call,
                        Some(parameter.name),
                        MqValidationProblem::StructureVersionIdentity,
                    ));
                }
            }
            if !options.is_empty() {
                if !parameter.has_role(R::Options) {
                    return Err(error(
                        call,
                        Some(parameter.name),
                        MqValidationProblem::ParameterShape,
                    ));
                }
                // The signature pins a structure identity, but its field option
                // constants and legal bitsets require separate pinned definitions.
                record(pending, MqPendingDisposition::OptionValueLegalityNotStated);
            }
        }
        _ => {
            return Err(error(
                call,
                Some(parameter.name),
                MqValidationProblem::ParameterShape,
            ));
        }
    }
    Ok(())
}

fn scalar(args: &[MqArgument<'_>], name: &str) -> Option<i64> {
    args.iter()
        .find(|arg| arg.name == name)
        .and_then(|arg| match arg.value {
            MqArgumentValue::Scalar(value) => Some(value),
            _ => None,
        })
}

fn extent(args: &[MqArgument<'_>], name: &str) -> Option<usize> {
    args.iter()
        .find(|arg| arg.name == name)
        .and_then(|arg| match arg.value {
            MqArgumentValue::ByteArea(value) | MqArgumentValue::ArraySlots(value) => Some(value),
            MqArgumentValue::Symbols(value) => Some(value.len()),
            _ => None,
        })
}

fn symbols<'a>(args: &'a [MqArgument<'a>], name: &str) -> &'a [&'a str] {
    args.iter()
        .find(|arg| arg.name == name)
        .map_or(&[], |arg| match &arg.value {
            MqArgumentValue::Symbols(value) => value,
            _ => &[],
        })
}

fn length_pair(
    call: &'static MqMqiContractDescriptor,
    args: &[MqArgument<'_>],
    length: &'static str,
    area: &'static str,
) -> Result<(), MqValidationError> {
    if call.label == "MQSETMP"
        && length == "ValueLength"
        && args.iter().any(|arg| {
            arg.name == length
                && arg.value == MqArgumentValue::SymbolicLength("MQVL_NULL_TERMINATED")
        })
    {
        return Ok(());
    }
    let Some(count) = scalar(args, length) else {
        return Err(error(
            call,
            Some(length),
            MqValidationProblem::ParameterShape,
        ));
    };
    let Ok(count) = usize::try_from(count) else {
        return Err(error(
            call,
            Some(length),
            MqValidationProblem::LengthRelationship,
        ));
    };
    if extent(args, area) != Some(count) {
        return Err(error(
            call,
            Some(area),
            MqValidationProblem::LengthRelationship,
        ));
    }
    Ok(())
}

fn has(symbols: &[&str], symbol: &str) -> bool {
    symbols.contains(&symbol)
}

fn check_special(
    call: &'static MqMqiContractDescriptor,
    args: &[MqArgument<'_>],
    pending: &mut Vec<MqPendingDisposition>,
) -> Result<(), MqValidationError> {
    let opts = symbols(args, "Options");
    match call.label {
        "MQOPEN" => {
            pin(
                call,
                ":0019",
                "b6f2f3659ca1e91fff52918d17a607bab29ef45cbd473b4a89c1d1c1c2ff8347",
            )?;
            let input = opts.iter().filter(|s| s.starts_with("MQOO_INPUT_")).count();
            let bind = opts.iter().filter(|s| s.starts_with("MQOO_BIND_")).count();
            if opts.is_empty()
                || input > 1
                || bind > 1
                || opts.len() != opts.iter().collect::<std::collections::BTreeSet<_>>().len()
            {
                return Err(error(
                    call,
                    Some("Options"),
                    MqValidationProblem::InvalidCombination,
                ));
            }
            record(pending, MqPendingDisposition::ObjectApplicabilityNotChecked);
        }
        "MQCLOSE" => {
            pin(
                call,
                ":0006",
                "28003da6981b7913cb0d88f8b250d3debc14682bac929a895e7e1be765853b7b",
            )?;
            if opts.len() != 1 {
                return Err(error(
                    call,
                    Some("Options"),
                    MqValidationProblem::InvalidCombination,
                ));
            }
            record(pending, MqPendingDisposition::ObjectApplicabilityNotChecked);
        }
        "MQCTL" => {
            pin(
                call,
                ":0011",
                "777347fd5adbf8febb091c0097f941645a889ec2dc3a21c585edbbd5f965d9d3",
            )?;
            let operation = symbols(args, "Operation");
            if operation.len() != 1
                || !matches!(
                    operation[0],
                    "MQOP_START" | "MQOP_START_WAIT" | "MQOP_STOP" | "MQOP_SUSPEND" | "MQOP_RESUME"
                )
            {
                return Err(error(
                    call,
                    Some("Operation"),
                    MqValidationProblem::InvalidCombination,
                ));
            }
        }
        "MQCB" => {
            pin(
                call,
                ":0004",
                "a6ca56ecd421cf8997e66853902cae59812c810d9ce6de5abd2f99a326498b8a",
            )?;
            let operation = symbols(args, "Operation");
            if operation.is_empty()
                || operation.iter().any(|s| {
                    !matches!(
                        *s,
                        "MQOP_REGISTER" | "MQOP_DEREGISTER" | "MQOP_SUSPEND" | "MQOP_RESUME"
                    )
                })
                || operation.len()
                    != operation
                        .iter()
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
            {
                return Err(error(
                    call,
                    Some("Operation"),
                    MqValidationProblem::InvalidCombination,
                ));
            }
            if has(operation, "MQOP_REGISTER")
                && args
                    .iter()
                    .any(|arg| arg.name == "CallbackDesc" && arg.value == MqArgumentValue::Null)
            {
                return Err(error(
                    call,
                    Some("CallbackDesc"),
                    MqValidationProblem::ParameterShape,
                ));
            }
            if let Some(MqArgument {
                value: MqArgumentValue::Structure { options, .. },
                ..
            }) = args.iter().find(|arg| arg.name == "GetMsgOpts")
            {
                if options.iter().any(|s| *s == "MQGMO_SET_SIGNAL")
                    || (has(options, "MQGMO_BROWSE_FIRST")
                        && has(options, "MQGMO_BROWSE_NEXT")
                        && options.iter().any(|s| s.starts_with("MQGMO_MARK_")))
                {
                    return Err(error(
                        call,
                        Some("GetMsgOpts"),
                        MqValidationProblem::InvalidCombination,
                    ));
                }
            }
        }
        "MQSUBRQ" => {
            pin(
                call,
                ":0026",
                "a603f61abc6bdd4ca8f5b76bef36470edc6475363dd2bda640780b46e4565160",
            )?;
            if symbols(args, "Action") != ["MQSR_ACTION_PUBLICATION"] {
                return Err(error(
                    call,
                    Some("Action"),
                    MqValidationProblem::InvalidCombination,
                ));
            }
        }
        "MQSTAT" => {
            pin(
                call,
                ":0024",
                "4f19dab47ed3e325ec894cc74a8d88db265a7c27f8ff2a7c507f94cf4bedd1d9",
            )?;
            let kind = symbols(args, "Type");
            if kind.len() != 1
                || !matches!(
                    kind[0],
                    "MQSTAT_TYPE_ASYNC_ERROR"
                        | "MQSTAT_TYPE_RECONNECTION"
                        | "MQSTAT_TYPE_RECONNECTION_ERROR"
                )
            {
                return Err(error(
                    call,
                    Some("Type"),
                    MqValidationProblem::InvalidCombination,
                ));
            }
            record(pending, MqPendingDisposition::ObjectApplicabilityNotChecked);
        }
        "MQINQ" | "MQSET" => {
            let digest = if call.label == "MQINQ" {
                "03e3347bbf16d2f8e3a9061e921dbfca7a3afd0fe3bc13418ebdf47bb652ce1b"
            } else {
                "f3d9ef2ad2cc795d374167c6356651d6747c939d2044b316e1d8cc3e06acf54e"
            };
            pin(
                call,
                if call.label == "MQINQ" {
                    ":0016"
                } else {
                    ":0022"
                },
                digest,
            )?;
            length_pair(call, args, "SelectorCount", "Selectors")?;
            length_pair(call, args, "IntAttrCount", "IntAttrs")?;
            length_pair(call, args, "CharAttrLength", "CharAttrs")?;
            if scalar(args, "SelectorCount").is_some_and(|n| n > 256) {
                return Err(error(
                    call,
                    Some("SelectorCount"),
                    MqValidationProblem::LengthRelationship,
                ));
            }
            let integer_count = symbols(args, "Selectors")
                .iter()
                .filter(|s| s.starts_with("MQIA_"))
                .count();
            if scalar(args, "IntAttrCount").is_none_or(|count| count < integer_count as i64) {
                if call.label == "MQSET" {
                    return Err(error(
                        call,
                        Some("IntAttrCount"),
                        MqValidationProblem::LengthRelationship,
                    ));
                }
                record(pending, MqPendingDisposition::PartialAttributeOutput);
            }
            if symbols(args, "Selectors")
                .iter()
                .any(|s| s.starts_with("MQCA_"))
            {
                record(pending, MqPendingDisposition::AttributeWidthNotResolved);
            }
            record(pending, MqPendingDisposition::ObjectApplicabilityNotChecked);
        }
        "MQSETMP" => {
            pin(
                call,
                ":0023",
                "5c1eddf9f87db568f941fcb283eb1e64c724eae5e64cb56a20f134a481a27a30",
            )?;
            length_pair(call, args, "ValueLength", "Value")?;
            let typ = symbols(args, "Type");
            if args.iter().any(|arg| {
                arg.name == "ValueLength"
                    && arg.value == MqArgumentValue::SymbolicLength("MQVL_NULL_TERMINATED")
            }) {
                if typ != ["MQTYPE_STRING"] {
                    return Err(error(
                        call,
                        Some("ValueLength"),
                        MqValidationProblem::InvalidCombination,
                    ));
                }
                record(pending, MqPendingDisposition::NullTerminationNotInspected);
                return Ok(());
            }
            let required = match typ {
                ["MQTYPE_BOOLEAN" | "MQTYPE_INT32" | "MQTYPE_FLOAT32"] => Some(4),
                ["MQTYPE_INT8"] => Some(1),
                ["MQTYPE_INT16"] => Some(2),
                ["MQTYPE_INT64" | "MQTYPE_FLOAT64"] => Some(8),
                ["MQTYPE_NULL"] => Some(0),
                ["MQTYPE_BYTE_STRING" | "MQTYPE_STRING"] => None,
                _ => {
                    return Err(error(
                        call,
                        Some("Type"),
                        MqValidationProblem::InvalidCombination,
                    ));
                }
            };
            if required.is_some_and(|n| scalar(args, "ValueLength") != Some(n)) {
                return Err(error(
                    call,
                    Some("ValueLength"),
                    MqValidationProblem::LengthRelationship,
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

/// Validate an ordered MQI signature without registering or invoking a handler.
/// Numeric version and option values without a pinned legal-value source are
/// reported pending. Callers must not treat this report as execution authority.
pub fn validate_mqi_call(
    label: &str,
    args: &[MqArgument<'_>],
) -> Result<MqValidationReport, MqValidationError> {
    let Some(call) = mq_mqi_contract_by_label(label) else {
        return Err(MqValidationError {
            source: None,
            parameter: None,
            problem: MqValidationProblem::UnknownCall,
        });
    };
    if call.signature_status != MqMqiSignatureStatus::SourceVerified {
        return Err(error(call, None, MqValidationProblem::PendingSignature));
    }
    if args.len() != call.parameters.len() {
        return Err(error(call, None, MqValidationProblem::ParameterCount));
    }
    let mut pending = Vec::new();
    for (arg, parameter) in args.iter().zip(call.parameters) {
        if arg.name != parameter.name {
            return Err(error(
                call,
                Some(parameter.name),
                MqValidationProblem::ParameterName,
            ));
        }
        check_parameter(call, parameter, &arg.value, &mut pending)?;
    }
    for (length, area) in [("BufferLength", "Buffer"), ("ValueLength", "Value")] {
        if call.parameter(length).is_some() && call.parameter(area).is_some() {
            length_pair(call, args, length, area)?;
        }
    }
    check_special(call, args, &mut pending)?;
    Ok(MqValidationReport {
        source: call.into(),
        pending,
        execution: MqExecutionDisposition::Unsupported,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mq_mqi_contracts;

    fn baseline(call: &'static MqMqiContractDescriptor) -> Vec<MqArgument<'static>> {
        call.parameters
            .iter()
            .map(|p| {
                let value = if p.direction == MqMqiParameterDirection::Output
                    && !p.has_role(MqMqiParameterRole::Data)
                {
                    MqArgumentValue::OutputSlot
                } else if let Some(identity) = p.structure_version_identity {
                    MqArgumentValue::Structure {
                        identity: p.data_type,
                        version: MqStructureVersion::Symbolic(identity),
                        options: &[],
                    }
                } else if p.has_role(MqMqiParameterRole::Structure) {
                    MqArgumentValue::Structure {
                        identity: p.data_type,
                        version: MqStructureVersion::NotApplicable,
                        options: &[],
                    }
                } else if p.has_role(MqMqiParameterRole::Handle) {
                    MqArgumentValue::Handle(p.handle_role.unwrap())
                } else if p.has_role(MqMqiParameterRole::Options)
                    || p.has_role(MqMqiParameterRole::Selector)
                {
                    MqArgumentValue::Symbols(&[])
                } else if p.has_role(MqMqiParameterRole::Length) {
                    MqArgumentValue::Scalar(0)
                } else if long_array(p.data_type) {
                    MqArgumentValue::ArraySlots(0)
                } else if p.data_type == "MQCHAR48" {
                    MqArgumentValue::ByteArea(48)
                } else if p.has_role(MqMqiParameterRole::Data)
                    || p.has_role(MqMqiParameterRole::Name)
                {
                    MqArgumentValue::ByteArea(0)
                } else {
                    MqArgumentValue::Scalar(0)
                };
                MqArgument {
                    name: p.name,
                    value,
                }
            })
            .collect()
    }

    fn replace<'a>(args: &mut [MqArgument<'a>], name: &str, value: MqArgumentValue<'a>) {
        args.iter_mut().find(|arg| arg.name == name).unwrap().value = value;
    }

    fn valid_args(call: &'static MqMqiContractDescriptor) -> Vec<MqArgument<'static>> {
        let mut args = baseline(call);
        match call.label {
            "MQOPEN" => replace(
                &mut args,
                "Options",
                MqArgumentValue::Symbols(&["MQOO_OUTPUT"]),
            ),
            "MQCLOSE" => replace(
                &mut args,
                "Options",
                MqArgumentValue::Symbols(&["MQCO_NONE"]),
            ),
            "MQCTL" => replace(
                &mut args,
                "Operation",
                MqArgumentValue::Symbols(&["MQOP_STOP"]),
            ),
            "MQCB" => replace(
                &mut args,
                "Operation",
                MqArgumentValue::Symbols(&["MQOP_REGISTER"]),
            ),
            "MQSUBRQ" => replace(
                &mut args,
                "Action",
                MqArgumentValue::Symbols(&["MQSR_ACTION_PUBLICATION"]),
            ),
            "MQSTAT" => replace(
                &mut args,
                "Type",
                MqArgumentValue::Symbols(&["MQSTAT_TYPE_ASYNC_ERROR"]),
            ),
            "MQSETMP" => replace(
                &mut args,
                "Type",
                MqArgumentValue::Symbols(&["MQTYPE_NULL"]),
            ),
            _ => {}
        }
        args
    }

    fn reject(call: &str, args: &[MqArgument<'_>], expected: MqValidationProblem) {
        assert_eq!(validate_mqi_call(call, args).unwrap_err().problem, expected);
    }

    #[test]
    fn all_26_pinned_signatures_validate_without_execution_authority() {
        for call in mq_mqi_contracts() {
            let report = validate_mqi_call(call.label, &valid_args(call)).unwrap();
            assert_eq!(report.source.official_row, call.official_row);
            assert_eq!(report.source.topic_sha256, call.topic_sha256);
            assert_eq!(report.source.topic_path, call.topic_path);
            assert_eq!(report.execution, MqExecutionDisposition::Unsupported);
        }
        reject("MQXCNVC", &[], MqValidationProblem::UnknownCall);
    }

    #[test]
    fn ordered_shape_and_version_identity_fail_closed() {
        let call = mq_mqi_contract_by_label("MQGET").unwrap();
        let mut args = valid_args(call);
        reject(
            "MQGET",
            &args[..args.len() - 1],
            MqValidationProblem::ParameterCount,
        );
        args[0].name = "HconnWrong";
        reject("MQGET", &args, MqValidationProblem::ParameterName);
        args[0].name = "Hconn";
        replace(
            &mut args,
            "MsgDesc",
            MqArgumentValue::Structure {
                identity: "MQOD",
                version: MqStructureVersion::Symbolic("MQMD_VERSION"),
                options: &[],
            },
        );
        reject("MQGET", &args, MqValidationProblem::StructureIdentity);
        replace(
            &mut args,
            "MsgDesc",
            MqArgumentValue::Structure {
                identity: "MQMD",
                version: MqStructureVersion::Symbolic("MQOD_VERSION"),
                options: &[],
            },
        );
        reject(
            "MQGET",
            &args,
            MqValidationProblem::StructureVersionIdentity,
        );
        replace(
            &mut args,
            "MsgDesc",
            MqArgumentValue::Structure {
                identity: "MQMD",
                version: MqStructureVersion::Numeric(2),
                options: &[],
            },
        );
        let report = validate_mqi_call("MQGET", &args).unwrap();
        assert!(
            report
                .pending
                .contains(&MqPendingDisposition::NumericVersionNotStated)
        );
    }

    #[test]
    fn length_and_selector_relationships_follow_pinned_call_topics() {
        let call = mq_mqi_contract_by_label("MQINQ").unwrap();
        let mut args = valid_args(call);
        replace(&mut args, "SelectorCount", MqArgumentValue::Scalar(1));
        reject("MQINQ", &args, MqValidationProblem::LengthRelationship);
        replace(
            &mut args,
            "Selectors",
            MqArgumentValue::Symbols(&["MQIA_MAX_MSG_LENGTH"]),
        );
        replace(&mut args, "IntAttrCount", MqArgumentValue::Scalar(1));
        replace(&mut args, "IntAttrs", MqArgumentValue::ArraySlots(1));
        assert!(validate_mqi_call("MQINQ", &args).is_ok());
        replace(&mut args, "IntAttrCount", MqArgumentValue::Scalar(0));
        replace(&mut args, "IntAttrs", MqArgumentValue::ArraySlots(0));
        assert!(
            validate_mqi_call("MQINQ", &args)
                .unwrap()
                .pending
                .contains(&MqPendingDisposition::PartialAttributeOutput)
        );
        replace(
            &mut args,
            "Selectors",
            MqArgumentValue::Symbols(&["MQOO_OUTPUT"]),
        );
        reject("MQINQ", &args, MqValidationProblem::SymbolFamily);
        let call = mq_mqi_contract_by_label("MQPUT").unwrap();
        let mut args = valid_args(call);
        replace(&mut args, "BufferLength", MqArgumentValue::Scalar(3));
        reject("MQPUT", &args, MqValidationProblem::LengthRelationship);
        replace(&mut args, "Buffer", MqArgumentValue::ByteArea(3));
        assert!(validate_mqi_call("MQPUT", &args).is_ok());
        replace(&mut args, "BufferLength", MqArgumentValue::Scalar(-1));
        reject("MQPUT", &args, MqValidationProblem::LengthRelationship);
    }

    #[test]
    fn documented_option_combinations_and_property_widths() {
        let call = mq_mqi_contract_by_label("MQOPEN").unwrap();
        let mut args = valid_args(call);
        replace(
            &mut args,
            "Options",
            MqArgumentValue::Symbols(&["MQOO_INPUT_SHARED", "MQOO_INPUT_EXCLUSIVE"]),
        );
        reject("MQOPEN", &args, MqValidationProblem::InvalidCombination);
        replace(&mut args, "Options", MqArgumentValue::Symbols(&[]));
        reject("MQOPEN", &args, MqValidationProblem::InvalidCombination);
        let call = mq_mqi_contract_by_label("MQCB").unwrap();
        let mut args = valid_args(call);
        replace(
            &mut args,
            "GetMsgOpts",
            MqArgumentValue::Structure {
                identity: "MQGMO",
                version: MqStructureVersion::Symbolic("MQGMO_VERSION"),
                options: &[
                    "MQGMO_BROWSE_FIRST",
                    "MQGMO_BROWSE_NEXT",
                    "MQGMO_MARK_BROWSE_HANDLE",
                ],
            },
        );
        reject("MQCB", &args, MqValidationProblem::InvalidCombination);
        let call = mq_mqi_contract_by_label("MQSETMP").unwrap();
        let mut args = valid_args(call);
        replace(
            &mut args,
            "Type",
            MqArgumentValue::Symbols(&["MQTYPE_INT64"]),
        );
        replace(&mut args, "ValueLength", MqArgumentValue::Scalar(4));
        replace(&mut args, "Value", MqArgumentValue::ByteArea(4));
        reject("MQSETMP", &args, MqValidationProblem::LengthRelationship);
        replace(&mut args, "ValueLength", MqArgumentValue::Scalar(8));
        replace(&mut args, "Value", MqArgumentValue::ByteArea(8));
        assert!(validate_mqi_call("MQSETMP", &args).is_ok());
        replace(
            &mut args,
            "Type",
            MqArgumentValue::Symbols(&["MQTYPE_STRING"]),
        );
        replace(
            &mut args,
            "ValueLength",
            MqArgumentValue::SymbolicLength("MQVL_NULL_TERMINATED"),
        );
        assert!(
            validate_mqi_call("MQSETMP", &args)
                .unwrap()
                .pending
                .contains(&MqPendingDisposition::NullTerminationNotInspected)
        );
        replace(
            &mut args,
            "Type",
            MqArgumentValue::Symbols(&["MQTYPE_INT64"]),
        );
        reject("MQSETMP", &args, MqValidationProblem::InvalidCombination);
    }

    #[test]
    fn fixed_name_width_and_set_attribute_count() {
        let call = mq_mqi_contract_by_label("MQCONN").unwrap();
        let mut args = valid_args(call);
        replace(&mut args, "QMgrName", MqArgumentValue::ByteArea(47));
        reject("MQCONN", &args, MqValidationProblem::LengthRelationship);
        let call = mq_mqi_contract_by_label("MQSET").unwrap();
        let mut args = valid_args(call);
        replace(&mut args, "SelectorCount", MqArgumentValue::Scalar(1));
        replace(
            &mut args,
            "Selectors",
            MqArgumentValue::Symbols(&["MQIA_TRIGGER_CONTROL"]),
        );
        reject("MQSET", &args, MqValidationProblem::LengthRelationship);
    }

    #[test]
    fn optional_pointer_shape_is_tied_to_documented_operation() {
        let call = mq_mqi_contract_by_label("MQCB").unwrap();
        let mut args = valid_args(call);
        replace(&mut args, "CallbackDesc", MqArgumentValue::Null);
        reject("MQCB", &args, MqValidationProblem::ParameterShape);
        replace(
            &mut args,
            "Operation",
            MqArgumentValue::Symbols(&["MQOP_DEREGISTER"]),
        );
        assert!(validate_mqi_call("MQCB", &args).is_ok());
        let call = mq_mqi_contract_by_label("MQSUBRQ").unwrap();
        let mut args = valid_args(call);
        replace(&mut args, "SubRqOpts", MqArgumentValue::Null);
        assert!(validate_mqi_call("MQSUBRQ", &args).is_ok());
    }
}
