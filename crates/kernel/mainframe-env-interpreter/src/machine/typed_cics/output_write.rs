//! Checked host output conversion for the typed CICS plan route.

use super::*;

pub(in crate::machine) fn write_output(
    machine: &mut ReferenceMachine,
    operation: CicsOperation,
    name: &str,
    target: &CicsTarget,
    value: &BoundedPayload,
    load_base: Option<usize>,
) -> Result<(), MachineProblem> {
    if matches!(name, "SET" | "ENTRY")
        && let Some(load_base) = load_base
    {
        return retrieve::write_load_pointer(machine, target, value, load_base);
    }
    if matches!(name, "SET" | "OUTTOKEN" | "ENCRYPTPTKT") {
        return retrieve::write_set_output(machine, operation, target, value);
    }
    if name == "SET64" {
        return retrieve::write_set64_output(machine, target, value);
    }
    if name == "STATUS" && operation == CicsOperation::CheckTimer {
        if value.schema() != "mainframe-env.cics.cvda@1" {
            return Err(MachineProblem::UnexpectedHostResult);
        }
        let code = match value.bytes() {
            b"UNEXPIRED" => 0,
            b"EXPIRED" => 1,
            b"FORCED" => 2,
            _ => return Err(MachineProblem::UnexpectedHostResult),
        };
        return write_target(
            machine,
            target,
            &CobolValue::Decimal(Decimal {
                coefficient: code,
                scale: 0,
            }),
        );
    }
    if matches!(name, "EVENTTYPE" | "FIRESTATUS") {
        if value.schema() != "mainframe-env.cics.cvda@1" {
            return Err(MachineProblem::UnexpectedHostResult);
        }
        let code = match (name, value.bytes()) {
            ("FIRESTATUS", b"NOTFIRED") => 0,
            ("FIRESTATUS", b"FIRED") => 1,
            ("EVENTTYPE", b"INPUT") => 1,
            ("EVENTTYPE", b"COMPOSITE") => 2,
            ("EVENTTYPE", b"TIMER") => 3,
            ("EVENTTYPE", b"ACTIVITY") => 4,
            ("EVENTTYPE", b"SYSTEM") => 5,
            _ => return Err(MachineProblem::UnexpectedHostResult),
        };
        return write_target(
            machine,
            target,
            &CobolValue::Decimal(Decimal {
                coefficient: code,
                scale: 0,
            }),
        );
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
    ) && value.schema() != "mainframe-env.cics.decimal@1"
        || name == "TOKEN"
            && operation == CicsOperation::Read
            && value.schema() != "mainframe-env.cics.decimal@1"
        || matches!(
            name,
            "COMMAREA" | "RIDFLD" | "RTRANSID" | "RTERMID" | "QUEUE" | "EVENT" | "SUBEVENT"
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
