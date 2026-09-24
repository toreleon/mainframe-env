//! Source-bounded compiler lowering for the 23 BTS lifecycle forms.

use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsOption, HirCicsOutputBinding,
    HirCicsOutputName, HirCicsValue, Resolution, ResolutionFailure, require_writable,
};
use super::{Clauses, cics_value, complete_data_reference};
use crate::{CobolUsage, DataCategory, SemanticModel};
use mainframe_env_ir::CicsApplicationRegistryDescriptor;

pub(super) fn reviewed_ambiguous_shape(
    descriptor: &CicsApplicationRegistryDescriptor,
    name: &str,
    has_value: bool,
) -> bool {
    has_value
        && matches!(
            descriptor.label_tokens,
            ["CHECK", "ACQACTIVITY"] | ["CHECK", "ACQPROCESS"] | ["CHECK", "ACTIVITY"]
        )
        && matches!(name, "COMPSTATUS" | "MODE" | "SUSPSTATUS")
}

pub(super) const fn is_bts(operation: HirCicsOperation) -> bool {
    matches!(
        operation,
        HirCicsOperation::AcquireActivityId
            | HirCicsOperation::AcquireProcess
            | HirCicsOperation::CancelAcqActivity
            | HirCicsOperation::CancelAcqProcess
            | HirCicsOperation::CancelActivity
            | HirCicsOperation::CheckAcqActivity
            | HirCicsOperation::CheckAcqProcess
            | HirCicsOperation::CheckActivity
            | HirCicsOperation::DefineActivity
            | HirCicsOperation::DefineProcess
            | HirCicsOperation::DeleteActivity
            | HirCicsOperation::ResetAcqProcess
            | HirCicsOperation::ResetActivity
            | HirCicsOperation::ResumeAcqActivity
            | HirCicsOperation::ResumeAcqProcess
            | HirCicsOperation::ResumeActivity
            | HirCicsOperation::RunAcqActivity
            | HirCicsOperation::RunAcqProcess
            | HirCicsOperation::RunActivity
            | HirCicsOperation::RunTransId
            | HirCicsOperation::SuspendAcqActivity
            | HirCicsOperation::SuspendAcqProcess
            | HirCicsOperation::SuspendActivity
    )
}

pub(super) fn allowed_clauses(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::AcquireActivityId => &["ACTIVITYID", "RESP", "RESP2"],
        HirCicsOperation::AcquireProcess => &["PROCESS", "PROCESSTYPE", "RESP", "RESP2"],
        HirCicsOperation::CancelActivity
        | HirCicsOperation::DeleteActivity
        | HirCicsOperation::ResetActivity
        | HirCicsOperation::ResumeActivity
        | HirCicsOperation::SuspendActivity => &["ACTIVITY", "RESP", "RESP2"],
        HirCicsOperation::CheckAcqActivity | HirCicsOperation::CheckAcqProcess => &[
            "COMPSTATUS",
            "ABCODE",
            "ABPROGRAM",
            "MODE",
            "SUSPSTATUS",
            "RESP",
            "RESP2",
        ],
        HirCicsOperation::CheckActivity => &[
            "ACTIVITY",
            "COMPSTATUS",
            "ABCODE",
            "ABPROGRAM",
            "MODE",
            "SUSPSTATUS",
            "RESP",
            "RESP2",
        ],
        HirCicsOperation::DefineActivity => &[
            "ACTIVITY",
            "EVENT",
            "TRANSID",
            "PROGRAM",
            "USERID",
            "ACTIVITYID",
            "RESP",
            "RESP2",
        ],
        HirCicsOperation::DefineProcess => &[
            "PROCESS",
            "PROCESSTYPE",
            "TRANSID",
            "PROGRAM",
            "USERID",
            "RESP",
            "RESP2",
        ],
        HirCicsOperation::RunAcqActivity | HirCicsOperation::RunAcqProcess => {
            &["INPUTEVENT", "FACILITYTOKN", "RESP", "RESP2"]
        }
        HirCicsOperation::RunActivity => {
            &["ACTIVITY", "INPUTEVENT", "FACILITYTOKN", "RESP", "RESP2"]
        }
        HirCicsOperation::RunTransId => &["TRANSID", "CHANNEL", "CHILD", "RESP", "RESP2"],
        _ => &["RESP", "RESP2"],
    }
}

pub(super) fn allowed_options(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::DefineProcess => &["NOCHECK", "NOHANDLE"],
        HirCicsOperation::CancelAcqActivity
        | HirCicsOperation::CheckAcqActivity
        | HirCicsOperation::ResumeAcqActivity
        | HirCicsOperation::SuspendAcqActivity => &["ACQACTIVITY", "NOHANDLE"],
        HirCicsOperation::CancelAcqProcess
        | HirCicsOperation::CheckAcqProcess
        | HirCicsOperation::ResetAcqProcess
        | HirCicsOperation::ResumeAcqProcess
        | HirCicsOperation::SuspendAcqProcess => &["ACQPROCESS", "NOHANDLE"],
        HirCicsOperation::RunAcqActivity => {
            &["ACQACTIVITY", "SYNCHRONOUS", "ASYNCHRONOUS", "NOHANDLE"]
        }
        HirCicsOperation::RunAcqProcess => {
            &["ACQPROCESS", "SYNCHRONOUS", "ASYNCHRONOUS", "NOHANDLE"]
        }
        HirCicsOperation::RunActivity => &["SYNCHRONOUS", "ASYNCHRONOUS", "NOHANDLE"],
        _ => &["NOHANDLE"],
    }
}

pub(super) fn required(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::AcquireActivityId => &["ACTIVITYID"],
        HirCicsOperation::AcquireProcess => &["PROCESS", "PROCESSTYPE"],
        HirCicsOperation::CancelActivity
        | HirCicsOperation::CheckActivity
        | HirCicsOperation::DeleteActivity
        | HirCicsOperation::ResetActivity
        | HirCicsOperation::ResumeActivity
        | HirCicsOperation::RunActivity
        | HirCicsOperation::SuspendActivity => &["ACTIVITY"],
        HirCicsOperation::DefineActivity => &["ACTIVITY", "TRANSID"],
        HirCicsOperation::DefineProcess => &["PROCESS", "PROCESSTYPE", "TRANSID"],
        HirCicsOperation::RunTransId => &["TRANSID", "CHILD"],
        _ => &[],
    }
}

pub(super) fn option(operation: HirCicsOperation, name: &str) -> Option<HirCicsOption> {
    if !is_bts(operation) {
        return None;
    }
    match name {
        "ACQACTIVITY" => Some(HirCicsOption::AcqActivity),
        "ACQPROCESS" => Some(HirCicsOption::AcqProcess),
        "SYNCHRONOUS" => Some(HirCicsOption::BtsSynchronous),
        "ASYNCHRONOUS" => Some(HirCicsOption::BtsAsynchronous),
        _ => None,
    }
}

pub(super) fn validate_constraints(
    operation: HirCicsOperation,
    clauses: &Clauses,
    options: &[String],
) -> Resolution<()> {
    if !is_bts(operation) {
        return Ok(());
    }
    let running = matches!(
        operation,
        HirCicsOperation::RunAcqActivity
            | HirCicsOperation::RunAcqProcess
            | HirCicsOperation::RunActivity
    );
    let sync = options.iter().any(|option| option == "SYNCHRONOUS");
    let asynchronous = options.iter().any(|option| option == "ASYNCHRONOUS");
    if running && sync == asynchronous || clauses.contains_key("FACILITYTOKN") && !asynchronous {
        return Err(ResolutionFailure::Invalid(
            "CICS BTS RUN requires one execution mode; FACILITYTOKN requires ASYNCHRONOUS".into(),
        ));
    }
    Ok(())
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if !is_bts(operation) {
        return Ok(Vec::new());
    }
    let mut operands = Vec::new();
    for &(label, name, maximum, exact) in &[
        ("ACTIVITYID", HirCicsOperandName::BtsActivityId, 52, false),
        ("PROCESS", HirCicsOperandName::BtsProcess, 36, false),
        ("PROCESSTYPE", HirCicsOperandName::BtsProcessType, 8, false),
        ("ACTIVITY", HirCicsOperandName::BtsActivity, 16, false),
        ("EVENT", HirCicsOperandName::BtsEvent, 16, false),
        ("INPUTEVENT", HirCicsOperandName::BtsInputEvent, 16, false),
        ("TRANSID", HirCicsOperandName::BtsTransId, 4, false),
        ("PROGRAM", HirCicsOperandName::BtsProgram, 8, false),
        ("USERID", HirCicsOperandName::BtsUserId, 8, false),
        (
            "FACILITYTOKN",
            HirCicsOperandName::BtsFacilityToken,
            8,
            true,
        ),
        ("CHANNEL", HirCicsOperandName::BtsChannel, 16, false),
    ] {
        if label == "ACTIVITYID"
            && matches!(
                operation,
                HirCicsOperation::AcquireProcess | HirCicsOperation::DefineActivity
            )
        {
            continue;
        }
        let Some(tokens) = clauses.get(label) else {
            continue;
        };
        let value = cics_value(tokens, semantic)?;
        let valid = match &value {
            HirCicsValue::Literal(text) => {
                if exact {
                    text.len() == maximum
                } else {
                    (1..=maximum).contains(&text.len())
                }
            }
            HirCicsValue::Data(reference) => {
                (if exact {
                    reference.length == maximum
                } else {
                    (1..=maximum).contains(&reference.length)
                }) && matches!(
                    reference.category,
                    DataCategory::Alphabetic | DataCategory::Alphanumeric
                )
            }
            _ => false,
        };
        if !valid {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS BTS {label} requires a bounded character field"
            )));
        }
        operands.push(HirCicsNamedOperand { name, value });
    }
    Ok(operands)
}

pub(super) fn outputs(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    if !is_bts(operation) {
        return Ok(Vec::new());
    }
    let mut outputs = Vec::new();
    for &(label, name, length, binary) in &[
        ("ACTIVITYID", HirCicsOutputName::BtsActivityId, 52, false),
        ("COMPSTATUS", HirCicsOutputName::BtsCompStatus, 4, true),
        ("MODE", HirCicsOutputName::BtsMode, 4, true),
        ("SUSPSTATUS", HirCicsOutputName::BtsSuspStatus, 4, true),
        ("ABCODE", HirCicsOutputName::BtsAbCode, 4, false),
        ("ABPROGRAM", HirCicsOutputName::BtsAbProgram, 8, false),
        ("CHILD", HirCicsOutputName::BtsChildToken, 16, false),
    ] {
        let allowed = match label {
            "ACTIVITYID" => operation == HirCicsOperation::DefineActivity,
            "CHILD" => operation == HirCicsOperation::RunTransId,
            _ => matches!(
                operation,
                HirCicsOperation::CheckAcqActivity
                    | HirCicsOperation::CheckAcqProcess
                    | HirCicsOperation::CheckActivity
            ),
        };
        if !allowed {
            continue;
        }
        let Some(tokens) = clauses.get(label) else {
            continue;
        };
        let target = complete_data_reference(tokens, semantic)?;
        require_writable(&target)?;
        let valid = if binary {
            target.usage == CobolUsage::Binary && target.length == length && target.scale == 0
        } else {
            target.length == length
                && matches!(
                    target.category,
                    DataCategory::Alphabetic | DataCategory::Alphanumeric
                )
        };
        if !valid {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS BTS {label} requires an exact {length}-byte receiver"
            )));
        }
        outputs.push(HirCicsOutputBinding { name, target });
    }
    Ok(outputs)
}
