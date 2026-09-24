use super::*;

pub(in crate::machine) fn write_output(
    machine: &mut ReferenceMachine,
    operation: CicsOperation,
    name: &str,
    target: &CicsTarget,
    value: &BoundedPayload,
    load_base: Option<usize>,
) -> Result<(), MachineProblem> {
    if web_service_control::write_output(machine, operation, name, target, value)? {
        return Ok(());
    }
    if matches!(name, "SET" | "ENTRY")
        && let Some(load_base) = load_base
    {
        return retrieve::write_load_pointer(machine, target, value, load_base);
    }
    if matches!(name, "SET" | "PIPLIST") {
        return retrieve::write_set_output(machine, operation, target, value);
    }
    if name == "PIPLENGTH"
        && matches!(
            operation,
            CicsOperation::ExtractProcess | CicsOperation::GdsExtractProcess
        )
    {
        if value.schema() != "mainframe-env.cics.decimal@1" {
            return Err(MachineProblem::UnexpectedHostResult);
        }
        let length = std::str::from_utf8(value.bytes())
            .map_err(|_| MachineProblem::UnexpectedHostResult)?
            .parse::<u16>()
            .map_err(|_| MachineProblem::UnexpectedHostResult)?;
        let maximum = if operation == CicsOperation::ExtractProcess {
            32_763
        } else {
            763
        };
        if length > maximum {
            return Err(MachineProblem::UnexpectedHostResult);
        }
        let CicsTarget::Resolved(slot) = target else {
            return Err(MachineProblem::UnexpectedHostResult);
        };
        let area = resolved_slot(machine, slot)?;
        if area.length != 2 {
            return Err(MachineProblem::UnexpectedHostResult);
        }
        return machine.write_reference(&area, &length.to_be_bytes());
    }
    if name == "SET64" {
        return retrieve::write_set64_output(machine, target, value);
    }
    if matches!(
        name,
        "ABSTIME"
            | "MILLISECONDS"
            | "LENGTH"
            | "FLENGTH"
            | "NUMITEMS"
            | "TOFLENGTH"
            | "ELEMNAMELEN"
            | "ELEMNSLEN"
            | "TYPENAMELEN"
            | "TYPENSLEN"
            | "STATE"
            | "IUTYPE"
            | "DATASTR"
            | "RECFM"
            | "PROCLENGTH"
            | "PIPLENGTH"
            | "SYNCLEVEL"
    ) && value.schema() != "mainframe-env.cics.decimal@1"
        || name == "TOKEN"
            && operation == CicsOperation::Read
            && value.schema() != "mainframe-env.cics.decimal@1"
        || matches!(
            name,
            "COMMAREA"
                | "RIDFLD"
                | "RTRANSID"
                | "RTERMID"
                | "QUEUE"
                | "PARTN"
                | "PROCESS"
                | "RESOURCE"
                | "RPROCESS"
                | "RRESOURCE"
                | "CONVDATA"
                | "RETCODE"
                | "PROCNAME"
                | "SYSID"
                | "TERMID"
                | "INTO"
        ) && value.schema() != "mainframe-env.cics.payload@1"
        || name == "TOKEN"
            && matches!(
                operation,
                CicsOperation::SpoolOpenInput | CicsOperation::SpoolOpenOutput
            )
            && value.schema() != "mainframe-env.cics.payload@1"
        || matches!(name, "ELEMNAME" | "ELEMNS" | "TYPENAME" | "TYPENS")
            && value.schema() != "mainframe-env.cics.payload@1"
        || matches!(
            name,
            "MMDDYY" | "MMDDYYYY" | "TIME" | "YYDDD" | "YYMMDD" | "YYYYMMDD"
        ) && value.schema() != "mainframe-env.cics.payload@1"
    {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    if matches!(
        name,
        "MMDDYY" | "MMDDYYYY" | "TIME" | "YYDDD" | "YYMMDD" | "YYYYMMDD"
    ) && let CicsTarget::Resolved(slot) = target
        && value.bytes().len() < resolved_slot(machine, slot)?.length
    {
        return write_resolved_prefix(machine, slot, value.bytes());
    }
    if value.schema() == "mainframe-env.cics.decimal@1" {
        let coefficient = String::from_utf8_lossy(value.bytes())
            .parse::<i128>()
            .map_err(|_| MachineProblem::UnexpectedHostResult)?;
        write_target(
            machine,
            target,
            &CobolValue::Decimal(Decimal {
                coefficient,
                scale: 0,
            }),
        )
    } else {
        write_target(machine, target, &CobolValue::Bytes(value.bytes().to_vec()))
    }
}

fn write_resolved_prefix(
    machine: &mut ReferenceMachine,
    slot: &CicsStorageSlot,
    value: &[u8],
) -> Result<(), MachineProblem> {
    let mut reference = resolved_slot(machine, slot)?;
    reference.length = value.len();
    machine.write_reference(&reference, value)
}
