use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsOutputBinding,
    HirCicsOutputName, HirCicsValue, HirDataReference, Resolution, require_writable,
};
use super::{Clauses, complete_data_reference, format_time, program_control};
use crate::{CobolUsage, DataCategory, SemanticModel};
use mainframe_env_ir::CicsAssignOutput;

pub(super) fn inout_length(
    operands: &[HirCicsNamedOperand],
    operation: HirCicsOperation,
) -> Option<&HirDataReference> {
    matches!(
        operation,
        HirCicsOperation::Read
            | HirCicsOperation::ReadNext
            | HirCicsOperation::ReadPrev
            | HirCicsOperation::ReadTransientData
            | HirCicsOperation::ReadTemporaryStorage
            | HirCicsOperation::ReceivePartn
            | HirCicsOperation::IssueReceive
    )
    .then(|| {
        operands.iter().find_map(|operand| match &operand.value {
            HirCicsValue::Data(target) if operand.name == HirCicsOperandName::Length => {
                Some(target)
            }
            _ => None,
        })
    })
    .flatten()
}

pub(super) fn resolve(
    clauses: &Clauses,
    options: &[String],
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    let mut outputs = Vec::new();
    if operation == HirCicsOperation::CheckTimer {
        let target = complete_data_reference(&clauses["STATUS"], semantic)?;
        require_writable(&target)?;
        if target.category != DataCategory::Binary || target.length != 4 || target.scale != 0 {
            return Err(super::super::ResolutionFailure::Invalid(
                "CICS CHECK TIMER STATUS requires writable fullword binary storage".into(),
            ));
        }
        outputs.push(HirCicsOutputBinding {
            name: HirCicsOutputName::TimerStatus,
            target,
        });
    }
    let event_outputs: &[(&str, HirCicsOutputName, bool)] = match operation {
        HirCicsOperation::RetrieveReattachEvent => &[
            ("EVENT", HirCicsOutputName::EventName, false),
            ("EVENTTYPE", HirCicsOutputName::EventType, true),
        ],
        HirCicsOperation::RetrieveSubevent => &[
            ("SUBEVENT", HirCicsOutputName::SubEventName, false),
            ("EVENTTYPE", HirCicsOutputName::EventType, true),
        ],
        HirCicsOperation::TestEvent => &[("FIRESTATUS", HirCicsOutputName::FireStatus, true)],
        _ => &[],
    };
    for (name, identity, binary) in event_outputs {
        let target = complete_data_reference(&clauses[*name], semantic)?;
        require_writable(&target)?;
        let valid = if *binary {
            target.category == DataCategory::Binary && target.length == 4 && target.scale == 0
        } else {
            matches!(
                target.category,
                DataCategory::Alphabetic | DataCategory::Alphanumeric
            ) && target.length == 16
        };
        if !valid {
            return Err(super::super::ResolutionFailure::Invalid(format!(
                "CICS {operation:?} {name} requires {}",
                if *binary {
                    "writable fullword binary storage"
                } else {
                    "writable 16-byte character storage"
                }
            )));
        }
        outputs.push(HirCicsOutputBinding {
            name: *identity,
            target,
        });
    }
    for (name, identity) in [
        ("ABSTIME", HirCicsOutputName::Abstime),
        ("INTO", HirCicsOutputName::Into),
        ("PARTN", HirCicsOutputName::Partn),
        ("SET", HirCicsOutputName::SetPointer),
        ("MILLISECONDS", HirCicsOutputName::Milliseconds),
        ("MMDDYY", HirCicsOutputName::Mmddyy),
        ("MMDDYYYY", HirCicsOutputName::Mmddyyyy),
        ("RESP", HirCicsOutputName::Resp),
        ("RESP2", HirCicsOutputName::Resp2),
        ("RIDFLD", HirCicsOutputName::Ridfld),
        ("TOKEN", HirCicsOutputName::Token),
        ("RTRANSID", HirCicsOutputName::ReturnTransId),
        ("RTERMID", HirCicsOutputName::ReturnTermId),
        ("QUEUE", HirCicsOutputName::Queue),
        ("TOKEN", HirCicsOutputName::SpoolToken),
        ("TOFLENGTH", HirCicsOutputName::SpoolToFlength),
        ("TIME", HirCicsOutputName::Time),
        ("YYDDD", HirCicsOutputName::Yyddd),
        ("YYMMDD", HirCicsOutputName::Yymmdd),
        ("YYYYMMDD", HirCicsOutputName::Yyyymmdd),
    ] {
        if name == "ABSTIME" && operation == HirCicsOperation::FormatTime {
            continue;
        }
        if name == "PARTN" && operation != HirCicsOperation::ReceivePartn {
            continue;
        }
        if name == "INTO"
            && matches!(
                operation,
                HirCicsOperation::WebReceive
                    | HirCicsOperation::WebConverse
                    | HirCicsOperation::Converse
            )
        {
            continue;
        }
        if name == "TIME" && operation != HirCicsOperation::FormatTime {
            continue;
        }
        if name == "RIDFLD"
            && !matches!(
                operation,
                HirCicsOperation::ReadNext
                    | HirCicsOperation::ReadPrev
                    | HirCicsOperation::IssueNote
            )
        {
            continue;
        }
        if identity == HirCicsOutputName::Token && operation != HirCicsOperation::Read {
            continue;
        }
        if name == "SET"
            && !matches!(
                operation,
                HirCicsOperation::Retrieve
                    | HirCicsOperation::Getmain
                    | HirCicsOperation::ReadTransientData
                    | HirCicsOperation::ReadTemporaryStorage
                    | HirCicsOperation::ReceivePartn
                    | HirCicsOperation::IssueReceive
                    | HirCicsOperation::SendControl
                    | HirCicsOperation::SendPage
            )
        {
            continue;
        }
        if matches!(name, "RTRANSID" | "RTERMID" | "QUEUE")
            && operation != HirCicsOperation::Retrieve
        {
            continue;
        }
        if identity == HirCicsOutputName::SpoolToken
            && !matches!(
                operation,
                HirCicsOperation::SpoolOpenInput | HirCicsOperation::SpoolOpenOutput
            )
        {
            continue;
        }
        if name == "TOFLENGTH" && operation != HirCicsOperation::SpoolRead {
            continue;
        }
        if let Some(value) = clauses.get(name) {
            let target = complete_data_reference(value, semantic)?;
            require_writable(&target)?;
            if identity == HirCicsOutputName::Token
                && (target.category != DataCategory::Binary
                    || target.length != 4
                    || target.scale != 0)
            {
                return Err(super::super::ResolutionFailure::Invalid(
                    "CICS TOKEN requires a fullword binary data area".into(),
                ));
            }
            if name == "TOFLENGTH"
                && (target.category != DataCategory::Binary || target.length != 4)
            {
                return Err(super::super::ResolutionFailure::Invalid(
                    "CICS SPOOLREAD TOFLENGTH requires writable fullword binary storage".into(),
                ));
            }
            if name == "SET" && !matches!(target.usage, CobolUsage::Pointer | CobolUsage::Pointer32)
            {
                return Err(super::super::ResolutionFailure::Invalid(format!(
                    "CICS {operation:?} SET requires a POINTER or POINTER-32 reference"
                )));
            }
            if name == "PARTN"
                && (!matches!(
                    target.category,
                    DataCategory::Alphabetic | DataCategory::Alphanumeric
                ) || !matches!(target.length, 1..=2))
            {
                return Err(super::super::ResolutionFailure::Invalid(
                    "CICS RECEIVE PARTN PARTN requires a 1-2 character writable area".into(),
                ));
            }
            format_time::require_output_shape(identity, &target, clauses, options)?;
            outputs.push(HirCicsOutputBinding {
                name: identity,
                target,
            });
        }
    }
    if operation == HirCicsOperation::Assign {
        for (name, value) in clauses {
            let Some(identity) = CicsAssignOutput::from_name(name) else {
                continue;
            };
            let target = complete_data_reference(value, semantic)?;
            require_writable(&target)?;
            outputs.push(HirCicsOutputBinding {
                name: HirCicsOutputName::Assign(identity),
                target,
            });
        }
    }
    if matches!(
        operation,
        HirCicsOperation::Link | HirCicsOperation::InvokeApplication
    ) && let Some(output) = program_control::link_commarea_output(clauses, semantic)?
    {
        outputs.push(output);
    }
    if matches!(
        operation,
        HirCicsOperation::WriteJournalName | HirCicsOperation::WriteJournalNum
    ) && let Some(value) = clauses.get("REQID")
    {
        if options.iter().any(|option| option == "WAIT") {
            return Err(super::super::ResolutionFailure::Invalid(
                "CICS journal WRITE REQID is valid only without WAIT".into(),
            ));
        }
        let target = complete_data_reference(value, semantic)?;
        require_writable(&target)?;
        if target.usage != CobolUsage::Binary || target.length != 4 || target.scale != 0 {
            return Err(super::super::ResolutionFailure::Invalid(
                "CICS journal WRITE REQID requires writable fullword binary storage".into(),
            ));
        }
        outputs.push(HirCicsOutputBinding {
            name: HirCicsOutputName::JournalReqId,
            target,
        });
    }
    Ok(outputs)
}
