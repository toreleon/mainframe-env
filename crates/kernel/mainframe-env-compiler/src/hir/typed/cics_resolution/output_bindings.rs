use super::super::{
    HirCicsOperation, HirCicsOutputBinding, HirCicsOutputName, Resolution, require_writable,
};
use super::{Clauses, complete_data_reference, format_time, program_control};
use crate::SemanticModel;

pub(super) fn resolve(
    clauses: &Clauses,
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
        ("TIME", HirCicsOutputName::Time),
        ("YYDDD", HirCicsOutputName::Yyddd),
        ("YYMMDD", HirCicsOutputName::Yymmdd),
        ("YYYYMMDD", HirCicsOutputName::Yyyymmdd),
    ] {
        if name == "ABSTIME" && operation == HirCicsOperation::FormatTime {
            continue;
        }
        if let Some(value) = clauses.get(name) {
            let target = complete_data_reference(value, semantic)?;
            require_writable(&target)?;
            format_time::require_output_shape(identity, &target)?;
            outputs.push(HirCicsOutputBinding {
                name: identity,
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
