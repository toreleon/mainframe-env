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
    if let Some(number) = bts_event_number(operation, name, value)? {
        return write_target(machine, target, &CobolValue::Decimal(number));
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
    if operation == CicsOperation::GetContainer
        && name == "INTO"
        && let CicsTarget::Resolved(slot) = target
    {
        // Both channel and BTS retrieval copy a prefix without blank padding.
        // A host response must not exceed the checked receiving area.
        return write_resolved_prefix(machine, slot, value.bytes());
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

// The BTS browse and inquiry providers emit numeric CVDAs. They do not use
// the legacy symbolic EVENTTYPE/FIRESTATUS payloads handled above.
fn bts_event_number(
    operation: CicsOperation,
    name: &str,
    value: &BoundedPayload,
) -> Result<Option<Decimal>, MachineProblem> {
    if !matches!(
        operation,
        CicsOperation::BtsGetNextEvent | CicsOperation::BtsInquireEvent
    ) || !matches!(name, "EVENTTYPE" | "FIRESTATUS")
    {
        return Ok(None);
    }
    if value.schema() != "mainframe-env.cics.decimal@1" {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    let coefficient = std::str::from_utf8(value.bytes())
        .map_err(|_| MachineProblem::UnexpectedHostResult)?
        .parse::<i128>()
        .map_err(|_| MachineProblem::UnexpectedHostResult)?;
    Ok(Some(Decimal {
        coefficient,
        scale: 0,
    }))
}

fn write_resolved_prefix(
    machine: &mut ReferenceMachine,
    slot: &CicsStorageSlot,
    value: &[u8],
) -> Result<(), MachineProblem> {
    let mut reference = resolved_slot(machine, slot)?;
    if value.len() > reference.length {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    reference.length = value.len();
    machine.write_reference(&reference, value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_into_copies_only_returned_bytes_without_padding() {
        let (mut machine, slot) = super::super::tests::machine_with_alphanumeric_slot("OUT", 4);
        machine.write("OUT", b"ZZZZ").unwrap();
        let target = CicsTarget::Resolved(slot);
        for (bytes, expected) in [(b"DA".as_slice(), b"DAZZ".as_slice()), (b"", b"DAZZ")] {
            write_output(
                &mut machine,
                CicsOperation::GetContainer,
                "INTO",
                &target,
                &payload("mainframe-env.cics.payload@1", bytes.to_vec()).unwrap(),
                None,
            )
            .unwrap();
            assert_eq!(machine.read("OUT").unwrap(), expected);
        }
    }

    #[test]
    fn container_into_rejects_an_overlong_host_payload_without_writing() {
        let (mut machine, slot) = super::super::tests::machine_with_alphanumeric_slot("OUT", 4);
        machine.write("OUT", b"ZZZZ").unwrap();
        assert_eq!(
            write_output(
                &mut machine,
                CicsOperation::GetContainer,
                "INTO",
                &CicsTarget::Resolved(slot),
                &payload("mainframe-env.cics.payload@1", b"TOOLONG".to_vec()).unwrap(),
                None
            ),
            Err(MachineProblem::UnexpectedHostResult)
        );
        assert_eq!(machine.read("OUT").unwrap(), b"ZZZZ");
    }

    #[test]
    fn bts_event_metadata_accepts_numeric_provider_payloads() {
        for operation in [
            CicsOperation::BtsGetNextEvent,
            CicsOperation::BtsInquireEvent,
        ] {
            for (name, coefficient) in [
                ("EVENTTYPE", 226i128),
                ("EVENTTYPE", 1002),
                ("EVENTTYPE", 1003),
                ("EVENTTYPE", 1004),
                ("FIRESTATUS", 1000),
                ("FIRESTATUS", 1001),
            ] {
                let value = payload(
                    "mainframe-env.cics.decimal@1",
                    coefficient.to_string().into_bytes(),
                )
                .unwrap();
                assert_eq!(
                    bts_event_number(operation, name, &value),
                    Ok(Some(Decimal {
                        coefficient,
                        scale: 0,
                    }))
                );
            }
        }
    }

    #[test]
    fn bts_event_metadata_rejects_wrong_schema_and_malformed_numbers() {
        for operation in [
            CicsOperation::BtsGetNextEvent,
            CicsOperation::BtsInquireEvent,
        ] {
            for name in ["EVENTTYPE", "FIRESTATUS"] {
                for (schema, bytes) in [
                    ("mainframe-env.cics.cvda@1", b"INPUT".as_slice()),
                    ("mainframe-env.cics.payload@1", b"226".as_slice()),
                    ("mainframe-env.cics.decimal@1", b"INPUT".as_slice()),
                    ("mainframe-env.cics.decimal@1", b"".as_slice()),
                    ("mainframe-env.cics.decimal@1", b"\xff".as_slice()),
                ] {
                    let value = payload(schema, bytes.to_vec()).unwrap();
                    assert_eq!(
                        bts_event_number(operation, name, &value),
                        Err(MachineProblem::UnexpectedHostResult)
                    );
                }
            }
        }
    }

    #[test]
    fn bts_event_metadata_does_not_intercept_other_output_contracts() {
        let value = payload("mainframe-env.cics.payload@1", b"READY".to_vec()).unwrap();
        assert_eq!(
            bts_event_number(CicsOperation::BtsGetNextEvent, "EVENT", &value),
            Ok(None)
        );
        assert_eq!(
            bts_event_number(CicsOperation::CheckTimer, "EVENTTYPE", &value),
            Ok(None)
        );
    }
}
