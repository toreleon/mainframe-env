use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsValue, Resolution,
    ResolutionFailure,
};
use super::{Clauses, cics_value, complete_data_reference};
use crate::{CobolUsage, SemanticModel};

pub(super) fn allowed_clauses(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::WaitJournalName => &["JOURNALNAME", "REQID", "RESP", "RESP2"],
        _ => unreachable!("only journal operations delegate clause shape"),
    }
}

pub(super) fn allowed_options(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::WaitJournalName => &["NOHANDLE"],
        _ => unreachable!("only journal operations delegate option shape"),
    }
}

pub(super) fn required_clauses(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::WaitJournalName => &["JOURNALNAME"],
        _ => unreachable!("only journal operations delegate required clauses"),
    }
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if operation != HirCicsOperation::WaitJournalName {
        return Ok(Vec::new());
    }
    let journal_name = cics_value(&clauses["JOURNALNAME"], semantic)?;
    if let HirCicsValue::Literal(value) = &journal_name
        && (!(1..=8).contains(&value.len())
            || !value.bytes().all(|byte| {
                byte.is_ascii_uppercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'$' | b'@' | b'#')
            }))
    {
        return Err(ResolutionFailure::Invalid(
            "CICS WAIT JOURNALNAME requires a 1- to 8-character journal name".into(),
        ));
    }
    let mut operands = vec![HirCicsNamedOperand {
        name: HirCicsOperandName::JournalName,
        value: journal_name,
    }];
    if let Some(tokens) = clauses.get("REQID") {
        let reference = complete_data_reference(tokens, semantic).map_err(|_| {
            ResolutionFailure::Invalid(
                "CICS WAIT JOURNALNAME REQID requires fullword binary storage".into(),
            )
        })?;
        if reference.usage != CobolUsage::Binary || reference.length != 4 || reference.scale != 0 {
            return Err(ResolutionFailure::Invalid(
                "CICS WAIT JOURNALNAME REQID requires fullword binary storage".into(),
            ));
        }
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::JournalReqId,
            value: HirCicsValue::Data(reference),
        });
    }
    Ok(operands)
}
