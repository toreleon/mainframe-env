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
        ("EIBFMH".into(), CobolValue::Bytes(vec![0x00])),
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
    if operation == CicsOperation::ReceiveMap {
        machine.write("EIBAID", &[response.aid])?;
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
