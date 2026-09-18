use super::super::{
    HirCicsOperation, HirCicsOutputBinding, HirCicsOutputName, Resolution, require_writable,
};
use super::{Clauses, complete_data_reference, format_time, program_control};
use crate::SemanticModel;
use mainframe_env_ir::CicsAssignOutput;

pub(super) fn resolve(
    clauses: &Clauses,
    options: &[String],
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    let mut outputs = Vec::new();
    for (name, identity) in [
        ("ABSTIME", HirCicsOutputName::Abstime),
        ("INTO", HirCicsOutputName::Into),
        ("MILLISECONDS", HirCicsOutputName::Milliseconds),
        ("MMDDYY", HirCicsOutputName::Mmddyy),
        ("MMDDYYYY", HirCicsOutputName::Mmddyyyy),
        ("RESP", HirCicsOutputName::Resp),
        ("RESP2", HirCicsOutputName::Resp2),
        ("RIDFLD", HirCicsOutputName::Ridfld),
        ("RTRANSID", HirCicsOutputName::ReturnTransId),
        ("RTERMID", HirCicsOutputName::ReturnTermId),
        ("QUEUE", HirCicsOutputName::Queue),
        ("TIME", HirCicsOutputName::Time),
        ("YYDDD", HirCicsOutputName::Yyddd),
        ("YYMMDD", HirCicsOutputName::Yymmdd),
        ("YYYYMMDD", HirCicsOutputName::Yyyymmdd),
    ] {
        if name == "ABSTIME" && operation == HirCicsOperation::FormatTime {
            continue;
        }
        if name == "RIDFLD"
            && !matches!(
                operation,
                HirCicsOperation::ReadNext | HirCicsOperation::ReadPrev
            )
        {
            continue;
        }
        if matches!(name, "RTRANSID" | "RTERMID" | "QUEUE")
            && operation != HirCicsOperation::Retrieve
        {
            continue;
        }
        if let Some(value) = clauses.get(name) {
            let target = complete_data_reference(value, semantic)?;
            require_writable(&target)?;
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
    if operation == HirCicsOperation::Link
        && let Some(output) = program_control::link_commarea_output(clauses, semantic)?
    {
        outputs.push(output);
    }
    Ok(outputs)
}
