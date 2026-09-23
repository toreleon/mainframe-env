use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsValue, Resolution,
    ResolutionFailure,
};
use super::{Clauses, cics_integer_value, cics_value, complete_data_reference};
use crate::{CobolUsage, SemanticModel};
use mainframe_env_ir::{CicsApplicationOptionValueShape, CicsApplicationRegistryDescriptor};

pub(super) fn synthetic_selector(
    descriptor: &CicsApplicationRegistryDescriptor,
    last: &str,
) -> bool {
    descriptor.label_tokens == ["WAIT", "JOURNALNUM"] && last == "JOURNALNUM"
}

pub(super) fn option_value_shape(
    descriptor: &CicsApplicationRegistryDescriptor,
    name: &str,
) -> Option<CicsApplicationOptionValueShape> {
    // Row 0236 inherits the named WAIT syntax but its short compatibility
    // topic omits JOURNALNUM and REQID from the structural option projection.
    (descriptor.label_tokens == ["WAIT", "JOURNALNUM"] && matches!(name, "JOURNALNUM" | "REQID"))
        .then_some(CicsApplicationOptionValueShape::Value)
}

pub(super) fn validate_candidate(
    descriptor: &CicsApplicationRegistryDescriptor,
    clauses: &Clauses,
) -> Result<(), String> {
    if descriptor.label_tokens == ["WAIT", "JOURNALNUM"] && !clauses.contains_key("JOURNALNUM") {
        return Err("CICS WAIT JOURNALNUM requires JOURNALNUM".into());
    }
    Ok(())
}

pub(super) fn allowed_clauses(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::WaitJournalName => &["JOURNALNAME", "REQID", "RESP", "RESP2"],
        HirCicsOperation::WaitJournalNum => &["JOURNALNUM", "REQID", "RESP", "RESP2"],
        _ => unreachable!("only journal operations delegate clause shape"),
    }
}

pub(super) fn allowed_options(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::WaitJournalName => &["NOHANDLE"],
        HirCicsOperation::WaitJournalNum => &["NOHANDLE"],
        _ => unreachable!("only journal operations delegate option shape"),
    }
}

pub(super) fn required_clauses(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::WaitJournalName => &["JOURNALNAME"],
        HirCicsOperation::WaitJournalNum => &["JOURNALNUM"],
        _ => unreachable!("only journal operations delegate required clauses"),
    }
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if !matches!(
        operation,
        HirCicsOperation::WaitJournalName | HirCicsOperation::WaitJournalNum
    ) {
        return Ok(Vec::new());
    }
    let mut operands = if operation == HirCicsOperation::WaitJournalName {
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
        vec![HirCicsNamedOperand {
            name: HirCicsOperandName::JournalName,
            value: journal_name,
        }]
    } else {
        let journal_num = cics_integer_value(&clauses["JOURNALNUM"], semantic)?;
        if let HirCicsValue::Integer(value) = &journal_num
            && !(1..=99).contains(value)
        {
            return Err(ResolutionFailure::Invalid(
                "CICS WAIT JOURNALNUM requires a journal number from 1 to 99".into(),
            ));
        }
        vec![HirCicsNamedOperand {
            name: HirCicsOperandName::JournalNum,
            value: journal_num,
        }]
    };
    if let Some(tokens) = clauses.get("REQID") {
        let reference = complete_data_reference(tokens, semantic).map_err(|_| {
            ResolutionFailure::Invalid(
                "CICS journal WAIT REQID requires fullword binary storage".into(),
            )
        })?;
        if reference.usage != CobolUsage::Binary || reference.length != 4 || reference.scale != 0 {
            return Err(ResolutionFailure::Invalid(
                "CICS journal WAIT REQID requires fullword binary storage".into(),
            ));
        }
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::JournalReqId,
            value: HirCicsValue::Data(reference),
        });
    }
    Ok(operands)
}
