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
    (descriptor.label_tokens == ["WAIT", "JOURNALNUM"]
        || descriptor.label_tokens == ["WRITE", "JOURNALNUM"])
        && last == "JOURNALNUM"
}

pub(super) fn option_value_shape(
    descriptor: &CicsApplicationRegistryDescriptor,
    name: &str,
) -> Option<CicsApplicationOptionValueShape> {
    // Row 0236 inherits the named WAIT syntax. Row 0255 names the legacy
    // WRITE compatibility form without a full syntax diagram; this bounded
    // route accepts the row 0254 record options with a numeric selector.
    if descriptor.label_tokens == ["WAIT", "JOURNALNUM"] && matches!(name, "JOURNALNUM" | "REQID") {
        return Some(CicsApplicationOptionValueShape::Value);
    }
    if descriptor.label_tokens == ["WRITE", "JOURNALNUM"] {
        return match name {
            "JOURNALNUM" | "JTYPEID" | "FROM" | "FLENGTH" | "REQID" | "PREFIX" | "PFXLENG" => {
                Some(CicsApplicationOptionValueShape::Value)
            }
            "WAIT" | "NOSUSPEND" => Some(CicsApplicationOptionValueShape::Flag),
            _ => None,
        };
    }
    None
}

pub(super) fn validate_candidate(
    descriptor: &CicsApplicationRegistryDescriptor,
    clauses: &Clauses,
) -> Result<(), String> {
    if descriptor.label_tokens == ["WAIT", "JOURNALNUM"] && !clauses.contains_key("JOURNALNUM") {
        return Err("CICS WAIT JOURNALNUM requires JOURNALNUM".into());
    }
    if descriptor.label_tokens == ["WRITE", "JOURNALNUM"] {
        for name in ["JOURNALNUM", "JTYPEID", "FROM"] {
            if !clauses.contains_key(name) {
                return Err(format!("CICS WRITE JOURNALNUM requires {name}"));
            }
        }
    }
    Ok(())
}

pub(super) fn allowed_clauses(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::WaitJournalName => &["JOURNALNAME", "REQID", "RESP", "RESP2"],
        HirCicsOperation::WaitJournalNum => &["JOURNALNUM", "REQID", "RESP", "RESP2"],
        HirCicsOperation::WriteJournalName => &[
            "JOURNALNAME",
            "JTYPEID",
            "FROM",
            "FLENGTH",
            "REQID",
            "PREFIX",
            "PFXLENG",
            "RESP",
            "RESP2",
        ],
        HirCicsOperation::WriteJournalNum => &[
            "JOURNALNUM",
            "JTYPEID",
            "FROM",
            "FLENGTH",
            "REQID",
            "PREFIX",
            "PFXLENG",
            "RESP",
            "RESP2",
        ],
        _ => unreachable!("only journal operations delegate clause shape"),
    }
}

pub(super) fn allowed_options(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::WaitJournalName => &["NOHANDLE"],
        HirCicsOperation::WaitJournalNum => &["NOHANDLE"],
        HirCicsOperation::WriteJournalName => &["WAIT", "NOSUSPEND", "NOHANDLE"],
        HirCicsOperation::WriteJournalNum => &["WAIT", "NOSUSPEND", "NOHANDLE"],
        _ => unreachable!("only journal operations delegate option shape"),
    }
}

pub(super) fn required_clauses(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::WaitJournalName => &["JOURNALNAME"],
        HirCicsOperation::WaitJournalNum => &["JOURNALNUM"],
        HirCicsOperation::WriteJournalName => &["JOURNALNAME", "JTYPEID", "FROM"],
        HirCicsOperation::WriteJournalNum => &["JOURNALNUM", "JTYPEID", "FROM"],
        _ => unreachable!("only journal operations delegate required clauses"),
    }
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if matches!(
        operation,
        HirCicsOperation::WriteJournalName | HirCicsOperation::WriteJournalNum
    ) {
        return write_operands(clauses, operation, semantic);
    }
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

fn write_operands(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let selector = if operation == HirCicsOperation::WriteJournalName {
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
                "CICS WRITE JOURNALNAME requires a 1- to 8-character journal name".into(),
            ));
        }
        HirCicsNamedOperand {
            name: HirCicsOperandName::JournalName,
            value: journal_name,
        }
    } else {
        let journal_num = cics_integer_value(&clauses["JOURNALNUM"], semantic)?;
        if let HirCicsValue::Integer(value) = &journal_num
            && !(1..=99).contains(value)
        {
            return Err(ResolutionFailure::Invalid(
                "CICS WRITE JOURNALNUM requires a journal number from 1 to 99".into(),
            ));
        }
        HirCicsNamedOperand {
            name: HirCicsOperandName::JournalNum,
            value: journal_num,
        }
    };
    let type_id = cics_value(&clauses["JTYPEID"], semantic)?;
    let type_length = match &type_id {
        HirCicsValue::Literal(value) => value.len(),
        HirCicsValue::Data(reference) => reference.length,
        _ => 0,
    };
    if type_length != 2 {
        return Err(ResolutionFailure::Invalid(
            "CICS journal WRITE JTYPEID requires two characters".into(),
        ));
    }
    let from = complete_data_reference(&clauses["FROM"], semantic)?;
    let mut operands = vec![
        selector,
        HirCicsNamedOperand {
            name: HirCicsOperandName::JournalTypeId,
            value: type_id,
        },
        HirCicsNamedOperand {
            name: HirCicsOperandName::JournalFrom,
            value: HirCicsValue::Data(from),
        },
    ];
    if let Some(tokens) = clauses.get("FLENGTH") {
        let value = cics_integer_value(tokens, semantic)?;
        match &value {
            HirCicsValue::Integer(value) if *value >= 0 => {}
            HirCicsValue::Data(reference)
                if reference.usage == CobolUsage::Binary
                    && reference.length == 4
                    && reference.scale == 0 => {}
            HirCicsValue::LengthOf(_) => {}
            _ => {
                return Err(ResolutionFailure::Invalid(
                    "CICS journal WRITE FLENGTH requires nonnegative fullword binary value".into(),
                ));
            }
        }
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::JournalFlength,
            value,
        });
    }
    if let Some(tokens) = clauses.get("PREFIX") {
        let prefix = complete_data_reference(tokens, semantic)?;
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::JournalPrefix,
            value: HirCicsValue::Data(prefix),
        });
    }
    if let Some(tokens) = clauses.get("PFXLENG") {
        if !clauses.contains_key("PREFIX") {
            return Err(ResolutionFailure::Invalid(
                "CICS journal WRITE PFXLENG requires PREFIX".into(),
            ));
        }
        let value = cics_integer_value(tokens, semantic)?;
        match &value {
            HirCicsValue::Integer(value) if (0..=65_535).contains(value) => {}
            HirCicsValue::Data(reference)
                if reference.usage == CobolUsage::Binary
                    && reference.length == 2
                    && reference.scale == 0 => {}
            _ => {
                return Err(ResolutionFailure::Invalid(
                    "CICS journal WRITE PFXLENG requires nonnegative halfword binary value".into(),
                ));
            }
        }
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::JournalPfxLeng,
            value,
        });
    }
    Ok(operands)
}
