use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, Resolution, ResolutionFailure,
};
use super::{Clauses, cics_integer_value, cics_value, shape::CommandShape};
use crate::SemanticModel;

pub(super) fn shape(operation: HirCicsOperation) -> Option<CommandShape> {
    match operation {
        HirCicsOperation::DefineInputEvent | HirCicsOperation::DeleteEvent => Some(CommandShape {
            clauses: &["EVENT", "RESP", "RESP2"],
            options: &["NOHANDLE"],
            required: &["EVENT"],
        }),
        HirCicsOperation::DefineCompositeEvent => Some(CommandShape {
            clauses: &[
                "EVENT",
                "SUBEVENT1",
                "SUBEVENT2",
                "SUBEVENT3",
                "SUBEVENT4",
                "SUBEVENT5",
                "SUBEVENT6",
                "SUBEVENT7",
                "SUBEVENT8",
                "RESP",
                "RESP2",
            ],
            options: &["AND", "OR", "NOHANDLE"],
            required: &["EVENT"],
        }),
        HirCicsOperation::AddSubevent | HirCicsOperation::RemoveSubevent => Some(CommandShape {
            clauses: &["EVENT", "SUBEVENT", "RESP", "RESP2"],
            options: &["NOHANDLE"],
            required: &["EVENT", "SUBEVENT"],
        }),
        HirCicsOperation::DefineTimer => Some(CommandShape {
            clauses: &[
                "TIMER",
                "EVENT",
                "DAYS",
                "HOURS",
                "MINUTES",
                "SECONDS",
                "YEAR",
                "MONTH",
                "DAYOFMONTH",
                "DAYOFYEAR",
                "RESP",
                "RESP2",
            ],
            options: &["AFTER", "AT", "ON", "NOHANDLE"],
            required: &["TIMER"],
        }),
        HirCicsOperation::ForceTimer => Some(CommandShape {
            clauses: &["TIMER", "RESP", "RESP2"],
            options: &["ACQACTIVITY", "ACQPROCESS", "NOHANDLE"],
            required: &["TIMER"],
        }),
        HirCicsOperation::CheckTimer => Some(CommandShape {
            clauses: &["TIMER", "STATUS", "RESP", "RESP2"],
            options: &["NOHANDLE"],
            required: &["TIMER", "STATUS"],
        }),
        HirCicsOperation::DeleteTimer => Some(CommandShape {
            clauses: &["TIMER", "RESP", "RESP2"],
            options: &["NOHANDLE"],
            required: &["TIMER"],
        }),
        HirCicsOperation::RetrieveReattachEvent => Some(CommandShape {
            clauses: &["EVENT", "EVENTTYPE", "RESP", "RESP2"],
            options: &["NOHANDLE"],
            required: &["EVENT", "EVENTTYPE"],
        }),
        HirCicsOperation::RetrieveSubevent => Some(CommandShape {
            clauses: &["EVENT", "SUBEVENT", "EVENTTYPE", "RESP", "RESP2"],
            options: &["NOHANDLE"],
            required: &["EVENT", "SUBEVENT", "EVENTTYPE"],
        }),
        HirCicsOperation::TestEvent => Some(CommandShape {
            clauses: &["EVENT", "FIRESTATUS", "RESP", "RESP2"],
            options: &["NOHANDLE"],
            required: &["EVENT", "FIRESTATUS"],
        }),
        _ => None,
    }
}

pub(super) fn validate_constraints(
    operation: HirCicsOperation,
    raw_options: &[String],
) -> Resolution<()> {
    if operation == HirCicsOperation::DefineCompositeEvent {
        let all = raw_options.iter().any(|option| option == "AND");
        let any = raw_options.iter().any(|option| option == "OR");
        if all == any {
            return Err(ResolutionFailure::Invalid(
                "CICS DEFINE COMPOSITE EVENT requires exactly one of AND or OR".into(),
            ));
        }
    }
    if operation == HirCicsOperation::DefineTimer {
        let after = raw_options.iter().any(|option| option == "AFTER");
        let at = raw_options.iter().any(|option| option == "AT");
        let on = raw_options.iter().any(|option| option == "ON");
        if after == at || on && !at {
            return Err(ResolutionFailure::Invalid(
                "CICS DEFINE TIMER requires AFTER or AT; ON requires AT".into(),
            ));
        }
    }
    if operation == HirCicsOperation::ForceTimer
        && raw_options.iter().any(|option| option == "ACQACTIVITY")
        && raw_options.iter().any(|option| option == "ACQPROCESS")
    {
        return Err(ResolutionFailure::Invalid(
            "CICS FORCE TIMER accepts only one acquired scope".into(),
        ));
    }
    Ok(())
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if !matches!(
        operation,
        HirCicsOperation::DefineInputEvent
            | HirCicsOperation::DeleteEvent
            | HirCicsOperation::DefineCompositeEvent
            | HirCicsOperation::AddSubevent
            | HirCicsOperation::RemoveSubevent
            | HirCicsOperation::DefineTimer
            | HirCicsOperation::CheckTimer
            | HirCicsOperation::DeleteTimer
            | HirCicsOperation::ForceTimer
            | HirCicsOperation::RetrieveReattachEvent
            | HirCicsOperation::RetrieveSubevent
            | HirCicsOperation::TestEvent
    ) {
        return Ok(Vec::new());
    }
    let mut operands = if matches!(
        operation,
        HirCicsOperation::DefineTimer
            | HirCicsOperation::CheckTimer
            | HirCicsOperation::DeleteTimer
            | HirCicsOperation::ForceTimer
    ) {
        vec![HirCicsNamedOperand {
            name: HirCicsOperandName::Timer,
            value: cics_value(&clauses["TIMER"], semantic)?,
        }]
    } else if operation == HirCicsOperation::RetrieveReattachEvent {
        Vec::new()
    } else {
        vec![HirCicsNamedOperand {
            name: HirCicsOperandName::Event,
            value: cics_value(&clauses["EVENT"], semantic)?,
        }]
    };
    if operation == HirCicsOperation::DefineTimer {
        if let Some(value) = clauses.get("EVENT") {
            operands.push(HirCicsNamedOperand {
                name: HirCicsOperandName::Event,
                value: cics_value(value, semantic)?,
            });
        }
        for (key, name) in [
            ("DAYS", HirCicsOperandName::TimerDays),
            ("HOURS", HirCicsOperandName::TimerHours),
            ("MINUTES", HirCicsOperandName::TimerMinutes),
            ("SECONDS", HirCicsOperandName::TimerSeconds),
            ("YEAR", HirCicsOperandName::TimerYear),
            ("MONTH", HirCicsOperandName::TimerMonth),
            ("DAYOFMONTH", HirCicsOperandName::TimerDayOfMonth),
            ("DAYOFYEAR", HirCicsOperandName::TimerDayOfYear),
        ] {
            if let Some(value) = clauses.get(key) {
                operands.push(HirCicsNamedOperand {
                    name,
                    value: cics_integer_value(value, semantic)?,
                });
            }
        }
    }
    if operation == HirCicsOperation::DefineCompositeEvent {
        for (clause, name) in [
            ("SUBEVENT1", HirCicsOperandName::SubEvent1),
            ("SUBEVENT2", HirCicsOperandName::SubEvent2),
            ("SUBEVENT3", HirCicsOperandName::SubEvent3),
            ("SUBEVENT4", HirCicsOperandName::SubEvent4),
            ("SUBEVENT5", HirCicsOperandName::SubEvent5),
            ("SUBEVENT6", HirCicsOperandName::SubEvent6),
            ("SUBEVENT7", HirCicsOperandName::SubEvent7),
            ("SUBEVENT8", HirCicsOperandName::SubEvent8),
        ] {
            if let Some(value) = clauses.get(clause) {
                operands.push(HirCicsNamedOperand {
                    name,
                    value: cics_value(value, semantic)?,
                });
            }
        }
    }
    if matches!(
        operation,
        HirCicsOperation::AddSubevent | HirCicsOperation::RemoveSubevent
    ) {
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::SubEvent,
            value: cics_value(&clauses["SUBEVENT"], semantic)?,
        });
    }
    Ok(operands)
}
