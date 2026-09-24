use super::{CobolValue, Decimal, MachineProblem, ReferenceMachine};
use mainframe_env_execution_api::BoundedPayload;
use mainframe_env_host_api::{CicsOperation, CicsResponse};
use std::collections::BTreeMap;

pub(super) fn implicit_values(
    commarea_len: Option<usize>,
    aid: Option<u8>,
    transaction: Option<&[u8]>,
) -> Result<BTreeMap<String, CobolValue>, MachineProblem> {
    Ok(BTreeMap::from([
        (
            "EIBRESP".into(),
            CobolValue::Decimal(Decimal {
                coefficient: 0,
                scale: 0,
            }),
        ),
        (
            "EIBRESP2".into(),
            CobolValue::Decimal(Decimal {
                coefficient: 0,
                scale: 0,
            }),
        ),
        (
            "EIBDATE".into(),
            CobolValue::Decimal(Decimal {
                coefficient: 0,
                scale: 0,
            }),
        ),
        (
            "EIBTIME".into(),
            CobolValue::Decimal(Decimal {
                coefficient: 0,
                scale: 0,
            }),
        ),
        ("EIBFN".into(), CobolValue::Bytes(vec![0, 0])),
        ("EIBRSRCE".into(), CobolValue::Bytes(vec![b' '; 8])),
        (
            "EIBCPOSN".into(),
            CobolValue::Decimal(Decimal {
                coefficient: 0,
                scale: 0,
            }),
        ),
        ("EIBFMH".into(), CobolValue::Bytes(vec![0x00])),
        ("EIBRCODE".into(), CobolValue::Bytes(vec![0x00; 6])),
        ("EIBSIG".into(), CobolValue::Bytes(vec![0x00])),
        ("EIBREQID".into(), CobolValue::Bytes(vec![0x00; 8])),
        (
            "EIBCALEN".into(),
            CobolValue::Decimal(Decimal {
                coefficient: i128::try_from(commarea_len.unwrap_or(0))
                    .map_err(|_| MachineProblem::InvalidOperation)?,
                scale: 0,
            }),
        ),
        ("EIBAID".into(), CobolValue::Bytes(vec![aid.unwrap_or(0)])),
        (
            "EIBTRNID".into(),
            CobolValue::Bytes(transaction.unwrap_or_default().to_vec()),
        ),
    ]))
}

pub(super) fn write_context(
    machine: &mut ReferenceMachine,
    operation: CicsOperation,
    response: &CicsResponse,
) -> Result<(), MachineProblem> {
    for (name, value) in [
        ("EIBRESP", i128::from(response.response)),
        ("EIBRESP2", i128::from(response.response2)),
    ] {
        machine.write_decimal(
            name,
            Decimal {
                coefficient: value,
                scale: 0,
            },
        )?;
    }
    if let Some(descriptor) =
        mainframe_env_ir::cics_application_registry_for_runtime_operation(operation.runtime_name())
    {
        machine.write("EIBFN", &descriptor.eibfn)?;
    }
    machine.write(
        "EIBSIG",
        &[if response.condition == "SIGNAL" {
            0xff
        } else {
            0x00
        }],
    )?;
    if operation == CicsOperation::WaitSignal {
        let first = match response.condition.as_str() {
            "NORMAL" => 0,
            "SIGNAL" => 0xE5,
            "NOTALLOC" => 0xD5,
            "TERMERR" => 0xF1,
            _ => return Err(MachineProblem::UnexpectedHostResult),
        };
        let condition =
            u8::try_from(response.response).map_err(|_| MachineProblem::UnexpectedHostResult)?;
        machine.write("EIBRCODE", &[first, 0, condition, 0, 0, 0])?;
    }
    if operation == CicsOperation::AllocateConversation
        && let Some(value) = response.outputs.get("EIBRSRCE")
    {
        if value.schema() != "mainframe-env.cics.eib-rsrce@1" || value.bytes().len() != 8 {
            return Err(MachineProblem::UnexpectedHostResult);
        }
        machine.write("EIBRSRCE", value.bytes())?;
    }
    if matches!(
        operation,
        CicsOperation::ReceiveMap | CicsOperation::ReceivePartn
    ) {
        machine.write("EIBAID", &[response.aid])?;
    }
    if operation == CicsOperation::ReceivePartn
        && let Some(value) = response.outputs.get("EIBCPOSN")
    {
        if value.schema() != "mainframe-env.cics.decimal@1" {
            return Err(MachineProblem::UnexpectedHostResult);
        }
        let cursor = std::str::from_utf8(value.bytes())
            .map_err(|_| MachineProblem::UnexpectedHostResult)?
            .parse::<u16>()
            .map_err(|_| MachineProblem::UnexpectedHostResult)?;
        machine.write_decimal(
            "EIBCPOSN",
            Decimal {
                coefficient: i128::from(cursor),
                scale: 0,
            },
        )?;
    }
    if operation == CicsOperation::Retrieve
        && let Some(value) = response.outputs.get("EIBFMH")
    {
        if value.schema() != "mainframe-env.cics.eib-fmh@1"
            || !matches!(value.bytes(), [0x00] | [0xff])
        {
            return Err(MachineProblem::UnexpectedHostResult);
        }
        machine.write("EIBFMH", value.bytes())?;
    }
    if operation == CicsOperation::Start
        && let Some(value) = response.outputs.get("EIBREQID")
    {
        if value.schema() != "mainframe-env.cics.reqid@1"
            || value.bytes().len() != 8
            || !value.bytes().iter().all(|byte| {
                byte.is_ascii_uppercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'-' | b'_' | b'$' | b'#' | b'@')
            })
        {
            return Err(MachineProblem::UnexpectedHostResult);
        }
        machine.write("EIBREQID", value.bytes())?;
    }
    if matches!(
        operation,
        CicsOperation::Asktime | CicsOperation::AsktimeEib
    ) {
        match (
            response.outputs.get("EIBDATE"),
            response.outputs.get("EIBTIME"),
        ) {
            (Some(date), Some(time)) => {
                write_clock(machine, "EIBDATE", date)?;
                write_clock(machine, "EIBTIME", time)?;
            }
            // Retained ASKTIME ABSTIME responses from before the implicit EIB
            // output contract remain replayable with their historical state.
            (None, None) if operation == CicsOperation::Asktime => {}
            _ => return Err(MachineProblem::UnexpectedHostResult),
        }
    }
    machine.write("EIBTRNID", response.transaction.as_bytes())?;
    Ok(())
}

fn write_clock(
    machine: &mut ReferenceMachine,
    name: &str,
    value: &BoundedPayload,
) -> Result<(), MachineProblem> {
    if value.schema() != "mainframe-env.cics.decimal@1" {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    let coefficient = String::from_utf8_lossy(value.bytes())
        .parse::<i128>()
        .map_err(|_| MachineProblem::UnexpectedHostResult)?;
    machine.write_decimal(
        name,
        Decimal {
            coefficient,
            scale: 0,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_execution_api::{InvocationLimits, Machine};
    use mainframe_env_host_api::CicsDisposition;
    use mainframe_env_ir::CodecLimits;

    #[test]
    fn signal_indicator_is_written_and_restored_without_stale_flag() {
        let mut machine = ReferenceMachine::from_binary(
            &super::super::tests::binary(),
            super::super::tests::invocation(),
            CodecLimits::default(),
        )
        .unwrap();
        assert_eq!(machine.read("EIBSIG").unwrap(), &[0]);
        let mut response = CicsResponse {
            disposition: CicsDisposition::Ignored,
            condition: "SIGNAL".into(),
            response: 24,
            response2: 0,
            applid: "MEAPPL".into(),
            sysid: "MESYS".into(),
            transaction: "MENU".into(),
            aid: 0,
            target: None,
            next_transaction: None,
            payload: BoundedPayload::new(
                "mainframe-env.cics.test@1",
                Vec::new(),
                InvocationLimits::default(),
            )
            .unwrap(),
            outputs: BTreeMap::new(),
            unit_of_work: None,
        };
        write_context(&mut machine, CicsOperation::WaitSignal, &response).unwrap();
        assert_eq!(machine.read("EIBSIG").unwrap(), &[0xff]);
        assert_eq!(machine.read("EIBRCODE").unwrap(), &[0xE5, 0, 24, 0, 0, 0]);
        let checkpoint = machine.checkpoint().unwrap();
        let mut restored = ReferenceMachine::from_binary(
            &super::super::tests::binary(),
            super::super::tests::invocation(),
            CodecLimits::default(),
        )
        .unwrap();
        restored.restore_checkpoint(&checkpoint).unwrap();
        assert_eq!(restored.read("EIBSIG").unwrap(), &[0xff]);
        response.condition = "NORMAL".into();
        response.response = 0;
        write_context(&mut restored, CicsOperation::WaitSignal, &response).unwrap();
        assert_eq!(restored.read("EIBSIG").unwrap(), &[0]);
        assert_eq!(restored.read("EIBRCODE").unwrap(), &[0; 6]);
    }
}
