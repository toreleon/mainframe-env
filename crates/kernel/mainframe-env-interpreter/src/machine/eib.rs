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
        ("EIBEOC".into(), CobolValue::Bytes(vec![0x00])),
        ("EIBERR".into(), CobolValue::Bytes(vec![0x00])),
        ("EIBERRCD".into(), CobolValue::Bytes(vec![0x00; 4])),
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
    let gds_signal = if matches!(
        operation,
        CicsOperation::GdsReceiveConversation | CicsOperation::GdsWaitConversation
    ) {
        match response.outputs.get("CONVDATA") {
            Some(block)
                if block.schema() == "mainframe-env.cics.payload@1"
                    && block.bytes().len() == 24 =>
            {
                block.bytes()[4] == 0xff
            }
            None => false,
            _ => return Err(MachineProblem::UnexpectedHostResult),
        }
    } else {
        false
    };
    if matches!(
        operation,
        CicsOperation::ReceiveConversation
            | CicsOperation::GdsReceiveConversation
            | CicsOperation::SendConversation
            | CicsOperation::GdsWaitConversation
            | CicsOperation::WaitConvid
            | CicsOperation::WaitSignal
            | CicsOperation::WaitTerminal
    ) {
        machine.write(
            "EIBSIG",
            &[u8::from(response.condition == "SIGNAL" || gds_signal) * 0xff],
        )?;
    }
    if operation == CicsOperation::SendConversation {
        let (flag, code) = match (
            response.outputs.get("EIBERR"),
            response.outputs.get("EIBERRCD"),
        ) {
            (None, None) => (0, [0; 4]),
            (Some(flag), Some(code))
                if flag.schema() == "mainframe-env.cics.payload@1"
                    && flag.bytes() == [0xff]
                    && code.schema() == "mainframe-env.cics.payload@1"
                    && code.bytes().len() == 4 =>
            {
                (0xff, code.bytes().try_into().unwrap())
            }
            _ => return Err(MachineProblem::UnexpectedHostResult),
        };
        machine.write("EIBERR", &[flag])?;
        machine.write("EIBERRCD", &code)?;
    }
    if operation == CicsOperation::WaitSignal {
        let first = match response.condition.as_str() {
            "NORMAL" => 0,
            "SIGNAL" => 0xe5,
            "NOTALLOC" => 0xd5,
            "TERMERR" => 0xf1,
            _ => return Err(MachineProblem::UnexpectedHostResult),
        };
        let condition =
            u8::try_from(response.response).map_err(|_| MachineProblem::UnexpectedHostResult)?;
        machine.write("EIBRCODE", &[first, 0, condition, 0, 0, 0])?;
    }
    if matches!(
        operation,
        CicsOperation::ReceiveConversation
            | CicsOperation::SendConversation
            | CicsOperation::WaitConvid
            | CicsOperation::WaitTerminal
    ) {
        let first = match response.condition.as_str() {
            "INVREQ" => 0xe0,
            "LENGERR" => 0xe1,
            "NOTALLOC" => 0xd5,
            "SIGNAL" => 0xe5,
            "CBIDERR" => 0xeb,
            "TERMERR" => 0xf1,
            _ => 0,
        };
        let flag = |name: &str| -> Result<bool, MachineProblem> {
            match response.outputs.get(name) {
                None => Ok(false),
                Some(value)
                    if value.schema() == "mainframe-env.cics.payload@1"
                        && matches!(value.bytes(), [0] | [0xff]) =>
                {
                    Ok(value.bytes()[0] == 0xff)
                }
                _ => Err(MachineProblem::UnexpectedHostResult),
            }
        };
        let eoc = flag("EIBEOC")?;
        let fmh = flag("EIBFMH")?;
        if matches!(
            operation,
            CicsOperation::ReceiveConversation | CicsOperation::WaitTerminal
        ) {
            machine.write("EIBEOC", &[u8::from(eoc) * 0xff])?;
        }
        if operation == CicsOperation::ReceiveConversation {
            machine.write("EIBFMH", &[u8::from(fmh) * 0xff])?;
        }
        let condition =
            u8::try_from(response.response).map_err(|_| MachineProblem::UnexpectedHostResult)?;
        machine.write(
            "EIBRCODE",
            &[
                first,
                (u8::from(eoc) * 0x20) | (u8::from(fmh) * 0x40),
                condition,
                0,
                0,
                0,
            ],
        )?;
    }
    if matches!(
        operation,
        CicsOperation::GdsReceiveConversation | CicsOperation::GdsWaitConversation
    ) {
        machine.write("EIBRCODE", &[0; 6])?;
    }
    if operation == CicsOperation::AllocateConversation
        && let Some(value) = response.outputs.get("EIBRSRCE")
    {
        if value.schema() != "mainframe-env.cics.eib-rsrce@1" || value.bytes().len() != 8 {
            return Err(MachineProblem::UnexpectedHostResult);
        }
        machine.write("EIBRSRCE", value.bytes())?;
    }
    if operation == CicsOperation::Converse {
        for name in ["EIBEOC", "EIBFMH", "EIBSIG"] {
            if let Some(value) = response.outputs.get(name) {
                if value.schema() != "mainframe-env.cics.eib-flag@1"
                    || !matches!(value.bytes(), [0] | [0xff])
                {
                    return Err(MachineProblem::UnexpectedHostResult);
                }
                machine.write(name, value.bytes())?;
            }
        }
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
        response.condition = "TERMERR".into();
        response.response = 81;
        write_context(&mut restored, CicsOperation::WaitSignal, &response).unwrap();
        assert_eq!(restored.read("EIBRCODE").unwrap(), &[0xF1, 0, 81, 0, 0, 0]);
    }

    #[test]
    fn conversation_receive_preserves_combined_eoc_fmh_and_gds_signal_indicators() {
        let mut machine = ReferenceMachine::from_binary(
            &super::super::tests::binary(),
            super::super::tests::invocation(),
            CodecLimits::default(),
        )
        .unwrap();
        let payload = |bytes: Vec<u8>| {
            BoundedPayload::new(
                "mainframe-env.cics.payload@1",
                bytes,
                InvocationLimits::default(),
            )
            .unwrap()
        };
        let mut response = CicsResponse {
            disposition: CicsDisposition::Complete,
            condition: "INBFMH".into(),
            response: 7,
            response2: 0,
            applid: "MEAPPL".into(),
            sysid: "MESYS".into(),
            transaction: "MENU".into(),
            aid: 0,
            target: None,
            next_transaction: None,
            payload: payload(Vec::new()),
            outputs: BTreeMap::from([
                ("EIBEOC".into(), payload(vec![0xff])),
                ("EIBFMH".into(), payload(vec![0xff])),
            ]),
            unit_of_work: None,
        };
        write_context(&mut machine, CicsOperation::ReceiveConversation, &response).unwrap();
        assert_eq!(machine.read("EIBEOC").unwrap(), &[0xff]);
        assert_eq!(machine.read("EIBFMH").unwrap(), &[0xff]);
        assert_eq!(machine.read("EIBRCODE").unwrap(), &[0, 0x60, 7, 0, 0, 0]);
        response.condition = "NORMAL".into();
        response.response = 0;
        response.outputs.clear();
        write_context(&mut machine, CicsOperation::ReceiveConversation, &response).unwrap();
        assert_eq!(machine.read("EIBEOC").unwrap(), &[0]);
        assert_eq!(machine.read("EIBFMH").unwrap(), &[0]);
        assert_eq!(machine.read("EIBRCODE").unwrap(), &[0; 6]);
        let mut block = vec![0; 24];
        block[4] = 0xff;
        response.outputs.insert("CONVDATA".into(), payload(block));
        write_context(
            &mut machine,
            CicsOperation::GdsReceiveConversation,
            &response,
        )
        .unwrap();
        assert_eq!(machine.read("EIBSIG").unwrap(), &[0xff]);
        assert_eq!(machine.read("EIBRCODE").unwrap(), &[0; 6]);
        response.outputs = BTreeMap::from([("EIBEOC".into(), payload(vec![0xff]))]);
        response.condition = "EOC".into();
        response.response = 6;
        write_context(&mut machine, CicsOperation::WaitTerminal, &response).unwrap();
        assert_eq!(machine.read("EIBEOC").unwrap(), &[0xff]);
        assert_eq!(machine.read("EIBRCODE").unwrap(), &[0, 0x20, 6, 0, 0, 0]);
        response.outputs = BTreeMap::from([
            ("EIBERR".into(), payload(vec![0xff])),
            ("EIBERRCD".into(), payload(vec![0x08, 0x89, 0, 0])),
        ]);
        response.condition = "NORMAL".into();
        response.response = 0;
        write_context(&mut machine, CicsOperation::SendConversation, &response).unwrap();
        assert_eq!(machine.read("EIBERR").unwrap(), &[0xff]);
        assert_eq!(machine.read("EIBERRCD").unwrap(), &[0x08, 0x89, 0, 0]);
        response.outputs.clear();
        write_context(&mut machine, CicsOperation::SendConversation, &response).unwrap();
        assert_eq!(machine.read("EIBERR").unwrap(), &[0]);
        assert_eq!(machine.read("EIBERRCD").unwrap(), &[0; 4]);
    }
}
