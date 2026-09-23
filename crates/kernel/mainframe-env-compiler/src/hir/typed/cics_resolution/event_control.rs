use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, Resolution, ResolutionFailure,
};
use super::{Clauses, cics_value, shape::CommandShape};
use crate::SemanticModel;

pub(super) fn shape(operation: HirCicsOperation) -> Option<CommandShape> {
    match operation {
        HirCicsOperation::DefineInputEvent => Some(CommandShape {
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
    Ok(())
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if !matches!(
        operation,
        HirCicsOperation::DefineInputEvent | HirCicsOperation::DefineCompositeEvent
    ) {
        return Ok(Vec::new());
    }
    let mut operands = vec![HirCicsNamedOperand {
        name: HirCicsOperandName::Event,
        value: cics_value(&clauses["EVENT"], semantic)?,
    }];
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
    Ok(operands)
}
