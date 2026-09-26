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
    let issue_control = matches!(
        operation,
        CicsOperation::IssueAbend
            | CicsOperation::GdsIssueAbend
            | CicsOperation::IssueConfirmation
            | CicsOperation::GdsIssueConfirmation
            | CicsOperation::IssueError
            | CicsOperation::GdsIssueError
            | CicsOperation::IssuePrepare
            | CicsOperation::GdsIssuePrepare
            | CicsOperation::IssueSignal
            | CicsOperation::GdsIssueSignal
    );
    if issue_control
        && match name {
            "STATE" => value.schema() != "mainframe-env.cics.cvda@1" || value.bytes().len() != 4,
            "CONVDATA" => value.bytes().len() != 24,
            "RETCODE" => {
                value.schema() != "mainframe-env.cics.gds-retcode@1" || value.bytes().len() != 6
            }
            _ => false,
        }
    {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    if web_service_control::write_output(machine, operation, name, target, value)? {
        return Ok(());
    }
    if name == "STATE" && value.schema() == "mainframe-env.cics.cvda@1" {
        let bytes: [u8; 4] = value
            .bytes()
            .try_into()
            .map_err(|_| MachineProblem::UnexpectedHostResult)?;
        return write_target(
            machine,
            target,
            &CobolValue::Decimal(Decimal {
                coefficient: i128::from(i32::from_be_bytes(bytes)),
                scale: 0,
            }),
        );
    }
    if name == "COMPSTATUS"
        && matches!(
            operation,
            CicsOperation::FetchAny | CicsOperation::FetchChild
        )
    {
        if value.schema() != "mainframe-env.cics.cvda@1" {
            return Err(MachineProblem::UnexpectedHostResult);
        }
        let code = match value.bytes() {
            b"NORMAL" => 0,
            b"ABEND" => 1,
            b"SECERROR" => 2,
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
    if matches!(name, "SET" | "ENTRY")
        && let Some(load_base) = load_base
    {
        return retrieve::write_load_pointer(machine, target, value, load_base);
    }
    if matches!(name, "SET" | "OUTTOKEN" | "ENCRYPTPTKT" | "PIPLIST") {
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
    if matches!(name, "COMPSTATUS" | "MODE" | "SUSPSTATUS")
        && matches!(
            operation,
            CicsOperation::CheckAcqActivity
                | CicsOperation::CheckAcqProcess
                | CicsOperation::CheckActivity
        )
    {
        if value.schema() != "mainframe-env.cics.cvda@1" {
            return Err(MachineProblem::UnexpectedHostResult);
        }
        let code = match (name, value.bytes()) {
            ("COMPSTATUS", b"INCOMPLETE")
            | ("MODE", b"INITIAL")
            | ("SUSPSTATUS", b"NOTSUSPENDED") => 0,
            ("COMPSTATUS", b"NORMAL") | ("MODE", b"ACTIVE") | ("SUSPSTATUS", b"SUSPENDED") => 1,
            ("COMPSTATUS", b"ABEND") | ("MODE", b"DORMANT") => 2,
            ("COMPSTATUS", b"FORCED") | ("MODE", b"CANCELLING") => 3,
            ("MODE", b"COMPLETE") => 4,
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
    if matches!(name, "EVENTTYPE" | "FIRESTATUS") && operation != CicsOperation::BtsInquireEvent {
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
            | "STATE"
            | "IUTYPE"
            | "DATASTR"
            | "RECFM"
            | "PROCLENGTH"
            | "PIPLENGTH"
            | "SYNCLEVEL"
    ) && value.schema() != "mainframe-env.cics.decimal@1"
        && !(issue_control && name == "STATE")
        || operation == CicsOperation::ExtractCertificate
            && CicsCertificateOutput::from_name(name).is_some_and(CicsCertificateOutput::length)
            && value.schema() != "mainframe-env.cics.decimal@1"
        || operation == CicsOperation::ExtractCertificate
            && name == "USERID"
            && value.schema() != "mainframe-env.cics.payload@1"
        || operation == CicsOperation::ExtractTcpip
            && CicsTcpipOutput::from_name(name)
                .is_some_and(|output| output.fullword() || output.buffer_length())
            && value.schema() != "mainframe-env.cics.decimal@1"
        || operation == CicsOperation::ExtractTcpip
            && CicsTcpipOutput::from_name(name)
                .is_some_and(|output| !output.fullword() && !output.buffer_length())
            && value.schema() != "mainframe-env.cics.payload@1"
        || name == "TOKEN"
            && operation == CicsOperation::Read
            && value.schema() != "mainframe-env.cics.decimal@1"
        || matches!(
            name,
            "COMMAREA"
                | "FIELD"
                | "RESULT"
                | "RIDFLD"
                | "RTRANSID"
                | "RTERMID"
                | "QUEUE"
                | "PARTN"
                | "EVENT"
                | "SUBEVENT"
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
            && !(issue_control && matches!(name, "CONVDATA" | "RETCODE"))
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
        "MMDDYY" | "MMDDYYYY" | "TIME" | "YYDDD" | "YYMMDD" | "YYYYMMDD" | "RESULT"
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
