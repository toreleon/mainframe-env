//! Source-bounded lowering for BTS browse rows.

use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsOutputBinding,
    HirCicsOutputName, HirCicsValue, Resolution, ResolutionFailure, require_writable,
};
use super::{
    Clauses, cics_integer_value, cics_value, complete_data_reference, shape::CommandShape,
};
use crate::{CobolUsage, DataCategory, SemanticModel};
use mainframe_env_ir::{
    BtsBrowseInput as I, BtsBrowseOutput as O, CicsApplicationRegistryDescriptor,
};

pub(super) const fn is_browse(operation: HirCicsOperation) -> bool {
    matches!(
        operation,
        HirCicsOperation::BtsEndBrowseContainer
            | HirCicsOperation::BtsGetNextContainer
            | HirCicsOperation::BtsInquireContainer
            | HirCicsOperation::BtsStartBrowseContainer
            | HirCicsOperation::BtsStartBrowseActivity
            | HirCicsOperation::BtsEndBrowseEvent
            | HirCicsOperation::BtsGetNextEvent
            | HirCicsOperation::BtsInquireEvent
            | HirCicsOperation::BtsStartBrowseEvent
            | HirCicsOperation::BtsEndBrowseTimer
            | HirCicsOperation::BtsInquireTimer
            | HirCicsOperation::BtsStartBrowseTimer
            | HirCicsOperation::BtsGetNextActivity
            | HirCicsOperation::BtsEndBrowseActivity
            | HirCicsOperation::BtsInquireActivity
            | HirCicsOperation::BtsStartBrowseProcess
            | HirCicsOperation::BtsGetNextProcess
            | HirCicsOperation::BtsEndBrowseProcess
            | HirCicsOperation::BtsInquireProcess
    )
}

pub(super) fn selector_matches(
    descriptor: &CicsApplicationRegistryDescriptor,
    body: &[String],
) -> bool {
    let resource = match descriptor.label_tokens {
        ["STARTBROWSE" | "GETNEXT" | "ENDBROWSE", "ACTIVITY"] | ["INQUIRE", "ACTIVITYID"] => {
            Some(descriptor.label_tokens[1])
        }
        [
            "STARTBROWSE" | "GETNEXT" | "ENDBROWSE" | "INQUIRE",
            "PROCESS" | "EVENT" | "TIMER" | "CONTAINER",
        ] => Some(descriptor.label_tokens[1]),
        _ => None,
    };
    resource.is_none_or(|resource| {
        body.get(1)
            .is_some_and(|token| token.eq_ignore_ascii_case(resource))
    })
}

pub(super) fn shape(operation: HirCicsOperation) -> Option<CommandShape> {
    Some(match operation {
        HirCicsOperation::BtsStartBrowseContainer => CommandShape {
            clauses: &[
                "ACTIVITYID",
                "PROCESS",
                "PROCESSTYPE",
                "CHANNEL",
                "BROWSETOKEN",
                "RESP",
                "RESP2",
            ],
            options: &["NOHANDLE"],
            required: &["BROWSETOKEN"],
        },
        HirCicsOperation::BtsGetNextContainer => CommandShape {
            clauses: &["BROWSETOKEN", "CONTAINER", "RESP", "RESP2"],
            options: &["NOHANDLE"],
            required: &["BROWSETOKEN", "CONTAINER"],
        },
        HirCicsOperation::BtsEndBrowseContainer => CommandShape {
            clauses: &["BROWSETOKEN", "RESP", "RESP2"],
            options: &["NOHANDLE"],
            required: &["BROWSETOKEN"],
        },
        HirCicsOperation::BtsInquireContainer => CommandShape {
            clauses: &[
                "CONTAINER",
                "ACTIVITYID",
                "PROCESS",
                "PROCESSTYPE",
                "DATALENGTH",
                "SET",
                "RESP",
                "RESP2",
            ],
            options: &["NOHANDLE"],
            required: &["CONTAINER"],
        },
        HirCicsOperation::BtsStartBrowseEvent => CommandShape {
            clauses: &["ACTIVITYID", "BROWSETOKEN", "RESP", "RESP2"],
            options: &["NOHANDLE"],
            required: &["BROWSETOKEN"],
        },
        HirCicsOperation::BtsStartBrowseTimer => CommandShape {
            clauses: &["ACTIVITYID", "TIMER", "BROWSETOKEN", "RESP", "RESP2"],
            options: &["NOHANDLE"],
            required: &["TIMER", "BROWSETOKEN"],
        },
        HirCicsOperation::BtsGetNextEvent => CommandShape {
            clauses: &[
                "BROWSETOKEN",
                "EVENT",
                "EVENTTYPE",
                "FIRESTATUS",
                "COMPOSITE",
                "PREDICATE",
                "TIMER",
                "RESP",
                "RESP2",
            ],
            options: &["NOHANDLE"],
            required: &["BROWSETOKEN", "EVENT"],
        },
        HirCicsOperation::BtsEndBrowseEvent | HirCicsOperation::BtsEndBrowseTimer => CommandShape {
            clauses: &["BROWSETOKEN", "RESP", "RESP2"],
            options: &["NOHANDLE"],
            required: &["BROWSETOKEN"],
        },
        HirCicsOperation::BtsInquireEvent => CommandShape {
            clauses: &[
                "EVENT",
                "ACTIVITYID",
                "EVENTTYPE",
                "FIRESTATUS",
                "COMPOSITE",
                "PREDICATE",
                "TIMER",
                "RESP",
                "RESP2",
            ],
            options: &["NOHANDLE"],
            required: &["EVENT"],
        },
        HirCicsOperation::BtsInquireTimer => CommandShape {
            clauses: &[
                "TIMER",
                "ACTIVITYID",
                "EVENT",
                "STATUS",
                "ABSTIME",
                "RESP",
                "RESP2",
            ],
            options: &["NOHANDLE"],
            required: &["TIMER"],
        },
        HirCicsOperation::BtsStartBrowseActivity => CommandShape {
            clauses: &[
                "ACTIVITYID",
                "PROCESS",
                "PROCESSTYPE",
                "BROWSETOKEN",
                "RESP",
                "RESP2",
            ],
            options: &["NOHANDLE"],
            required: &["BROWSETOKEN"],
        },
        HirCicsOperation::BtsStartBrowseProcess => CommandShape {
            clauses: &["PROCESSTYPE", "BROWSETOKEN", "RESP", "RESP2"],
            options: &["NOHANDLE"],
            required: &["PROCESSTYPE", "BROWSETOKEN"],
        },
        HirCicsOperation::BtsGetNextActivity => CommandShape {
            clauses: &[
                "BROWSETOKEN",
                "ACTIVITY",
                "ACTIVITYID",
                "LEVEL",
                "RESP",
                "RESP2",
            ],
            options: &["NOHANDLE"],
            required: &["BROWSETOKEN", "ACTIVITY"],
        },
        HirCicsOperation::BtsGetNextProcess => CommandShape {
            clauses: &["BROWSETOKEN", "PROCESS", "ACTIVITYID", "RESP", "RESP2"],
            options: &["NOHANDLE"],
            required: &["BROWSETOKEN", "PROCESS"],
        },
        HirCicsOperation::BtsEndBrowseActivity | HirCicsOperation::BtsEndBrowseProcess => {
            CommandShape {
                clauses: &["BROWSETOKEN", "RESP", "RESP2"],
                options: &["NOHANDLE"],
                required: &["BROWSETOKEN"],
            }
        }
        HirCicsOperation::BtsInquireProcess => CommandShape {
            clauses: &["PROCESS", "PROCESSTYPE", "ACTIVITYID", "RESP", "RESP2"],
            options: &["NOHANDLE"],
            required: &["PROCESS", "PROCESSTYPE"],
        },
        HirCicsOperation::BtsInquireActivity => CommandShape {
            clauses: &[
                "ACTIVITYID",
                "ABCODE",
                "ABPROGRAM",
                "ACTIVITY",
                "COMPSTATUS",
                "EVENT",
                "MODE",
                "PROCESS",
                "PROCESSTYPE",
                "PROGRAM",
                "SUSPSTATUS",
                "TRANSID",
                "USERID",
                "RESP",
                "RESP2",
            ],
            options: &["NOHANDLE"],
            required: &["ACTIVITYID"],
        },
        _ => return None,
    })
}

pub(super) fn reviewed_ambiguous_shape(
    descriptor: &CicsApplicationRegistryDescriptor,
    name: &str,
    has_value: bool,
) -> bool {
    has_value
        && match descriptor.label_tokens {
            ["GETNEXT", "EVENT"] => matches!(
                name,
                "EVENTTYPE" | "FIRESTATUS" | "COMPOSITE" | "PREDICATE" | "TIMER"
            ),
            ["INQUIRE", "ACTIVITYID"] => matches!(name, "COMPSTATUS" | "MODE" | "SUSPSTATUS"),
            ["INQUIRE", "EVENT"] => matches!(
                name,
                "EVENTTYPE" | "FIRESTATUS" | "COMPOSITE" | "PREDICATE" | "TIMER"
            ),
            ["INQUIRE", "TIMER"] => matches!(name, "EVENT" | "STATUS" | "ABSTIME"),
            _ => false,
        }
}

pub(super) fn validate(
    clauses: &Clauses,
    _options: &[String],
    operation: HirCicsOperation,
) -> Resolution<()> {
    if operation == HirCicsOperation::BtsGetNextEvent
        && ["EVENTTYPE", "FIRESTATUS", "COMPOSITE", "PREDICATE", "TIMER"]
            .iter()
            .filter(|name| clauses.contains_key(**name))
            .count()
            > 1
    {
        return Err(ResolutionFailure::Invalid(
            "CICS GETNEXT EVENT metadata output combinations are unsupported".into(),
        ));
    }
    if matches!(
        operation,
        HirCicsOperation::BtsStartBrowseContainer | HirCicsOperation::BtsInquireContainer
    ) && (clauses.contains_key("PROCESS") != clauses.contains_key("PROCESSTYPE")
        || clauses.contains_key("PROCESS") && clauses.contains_key("ACTIVITYID")
        || clauses.contains_key("CHANNEL")
            && (clauses.contains_key("PROCESS") || clauses.contains_key("ACTIVITYID")))
    {
        return Err(ResolutionFailure::Invalid(
            "CICS CONTAINER browse selectors conflict".into(),
        ));
    }
    if operation == HirCicsOperation::BtsStartBrowseActivity
        && (clauses.contains_key("PROCESS") != clauses.contains_key("PROCESSTYPE")
            || clauses.contains_key("PROCESS") && clauses.contains_key("ACTIVITYID"))
    {
        return Err(ResolutionFailure::Invalid(
            "CICS STARTBROWSE ACTIVITY requires PROCESS with PROCESSTYPE, or ACTIVITYID".into(),
        ));
    }
    if operation == HirCicsOperation::BtsInquireEvent
        && clauses.get("EVENT").is_some_and(|tokens| {
            tokens.iter().any(|token| {
                token
                    .trim_matches('\'')
                    .to_ascii_uppercase()
                    .starts_with("DFH")
            })
        })
    {
        return Err(ResolutionFailure::Invalid(
            "CICS SYSTEM event inquiry is unsupported".into(),
        ));
    }
    if operation == HirCicsOperation::BtsInquireActivity {
        let cvdas = ["COMPSTATUS", "MODE", "SUSPSTATUS"];
        if cvdas.iter().any(|name| clauses.contains_key(*name))
            && clauses
                .keys()
                .filter(|name| !matches!(name.as_str(), "ACTIVITYID" | "RESP" | "RESP2"))
                .count()
                != 1
        {
            return Err(ResolutionFailure::Invalid(
                "CICS ACTIVITYID CVDA output combinations are unsupported".into(),
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
    if !is_browse(operation) {
        return Ok(Vec::new());
    }
    let mut values = Vec::new();
    for (name, identity, width) in [
        ("ACTIVITYID", I::ActivityId, 52),
        ("PROCESS", I::Process, 36),
        ("PROCESSTYPE", I::ProcessType, 8),
        ("BROWSETOKEN", I::BrowseToken, 4),
        ("EVENT", I::Event, 16),
        ("TIMER", I::Timer, 16),
        ("CONTAINER", I::Container, 16),
        ("CHANNEL", I::Channel, 16),
    ] {
        let input = matches!(
            (operation, identity),
            (
                HirCicsOperation::BtsStartBrowseContainer,
                I::ActivityId | I::Process | I::ProcessType | I::Channel
            ) | (
                HirCicsOperation::BtsInquireContainer,
                I::Container | I::ActivityId | I::Process | I::ProcessType
            ) | (
                HirCicsOperation::BtsGetNextContainer | HirCicsOperation::BtsEndBrowseContainer,
                I::BrowseToken
            ) | (
                HirCicsOperation::BtsStartBrowseActivity,
                I::ActivityId | I::Process | I::ProcessType,
            ) | (HirCicsOperation::BtsStartBrowseProcess, I::ProcessType)
                | (
                    HirCicsOperation::BtsGetNextActivity
                        | HirCicsOperation::BtsGetNextProcess
                        | HirCicsOperation::BtsEndBrowseActivity
                        | HirCicsOperation::BtsEndBrowseProcess,
                    I::BrowseToken,
                )
                | (HirCicsOperation::BtsInquireActivity, I::ActivityId)
                | (
                    HirCicsOperation::BtsStartBrowseEvent
                        | HirCicsOperation::BtsStartBrowseTimer
                        | HirCicsOperation::BtsInquireEvent
                        | HirCicsOperation::BtsInquireTimer,
                    I::ActivityId
                )
                | (
                    HirCicsOperation::BtsStartBrowseTimer | HirCicsOperation::BtsInquireTimer,
                    I::Timer
                )
                | (HirCicsOperation::BtsInquireEvent, I::Event)
                | (
                    HirCicsOperation::BtsGetNextEvent
                        | HirCicsOperation::BtsEndBrowseEvent
                        | HirCicsOperation::BtsEndBrowseTimer,
                    I::BrowseToken
                )
                | (
                    HirCicsOperation::BtsInquireProcess,
                    I::Process | I::ProcessType
                )
        );
        if !input {
            continue;
        }
        if let Some(tokens) = clauses.get(name) {
            let value = if identity == I::BrowseToken {
                cics_integer_value(tokens, semantic)?
            } else {
                cics_value(tokens, semantic)?
            };
            let valid = match &value {
                HirCicsValue::Literal(text) if identity != I::BrowseToken => {
                    !text.is_empty() && text.chars().count() <= width
                }
                HirCicsValue::Integer(number) if identity == I::BrowseToken => {
                    (1..=i32::MAX as i64).contains(number)
                }
                HirCicsValue::Data(reference) if identity == I::BrowseToken => fullword(reference),
                HirCicsValue::Data(reference) => {
                    (1..=width).contains(&reference.length)
                        && matches!(
                            reference.category,
                            DataCategory::Alphabetic | DataCategory::Alphanumeric
                        )
                }
                _ => false,
            };
            if !valid {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS BTS browse {name} requires a checked input area"
                )));
            }
            values.push(HirCicsNamedOperand {
                name: HirCicsOperandName::BtsBrowse(identity),
                value,
            });
        }
    }
    Ok(values)
}

pub(super) fn outputs(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    if !is_browse(operation) {
        return Ok(Vec::new());
    }
    let mut values = Vec::new();
    for field in [
        O::BrowseToken,
        O::Activity,
        O::ActivityId,
        O::Level,
        O::Process,
        O::Abcode,
        O::Abprogram,
        O::Event,
        O::ProcessType,
        O::Program,
        O::TransId,
        O::UserId,
        O::Container,
        O::DataLength,
        O::Set,
        O::CompStatus,
        O::Mode,
        O::SuspStatus,
        O::EventType,
        O::FireStatus,
        O::Composite,
        O::Predicate,
        O::Timer,
        O::Status,
        O::Abstime,
    ] {
        let Some(tokens) = clauses.get(field.name()) else {
            continue;
        };
        let output = matches!(
            (operation, field),
            (HirCicsOperation::BtsStartBrowseContainer, O::BrowseToken)
                | (HirCicsOperation::BtsGetNextContainer, O::Container)
                | (
                    HirCicsOperation::BtsInquireContainer,
                    O::DataLength | O::Set
                )
                | (
                    HirCicsOperation::BtsStartBrowseActivity
                        | HirCicsOperation::BtsStartBrowseProcess,
                    O::BrowseToken,
                )
                | (
                    HirCicsOperation::BtsStartBrowseEvent | HirCicsOperation::BtsStartBrowseTimer,
                    O::BrowseToken,
                )
                | (
                    HirCicsOperation::BtsGetNextEvent,
                    O::Event
                        | O::EventType
                        | O::FireStatus
                        | O::Composite
                        | O::Predicate
                        | O::Timer
                )
                | (
                    HirCicsOperation::BtsInquireEvent,
                    O::EventType | O::FireStatus | O::Composite | O::Predicate | O::Timer
                )
                | (
                    HirCicsOperation::BtsInquireTimer,
                    O::Event | O::Status | O::Abstime
                )
                | (
                    HirCicsOperation::BtsGetNextActivity,
                    O::Activity | O::ActivityId | O::Level
                )
                | (
                    HirCicsOperation::BtsGetNextProcess,
                    O::Process | O::ActivityId
                )
                | (HirCicsOperation::BtsInquireProcess, O::ActivityId)
                | (
                    HirCicsOperation::BtsInquireActivity,
                    O::Abcode
                        | O::Abprogram
                        | O::Activity
                        | O::Event
                        | O::Process
                        | O::ProcessType
                        | O::Program
                        | O::TransId
                        | O::UserId
                        | O::CompStatus
                        | O::Mode
                        | O::SuspStatus,
                )
        );
        if !output {
            continue;
        }
        let target = complete_data_reference(tokens, semantic)?;
        require_writable(&target)?;
        let valid = if field == O::Set {
            matches!(
                target.category,
                DataCategory::Pointer | DataCategory::Pointer32
            )
        } else if field == O::Abstime {
            target.length == 8
                && target.usage == CobolUsage::PackedDecimal
                && target.digits == 15
                && target.scale == 0
                && target.signed
        } else if matches!(
            field,
            O::BrowseToken
                | O::Level
                | O::DataLength
                | O::CompStatus
                | O::Mode
                | O::SuspStatus
                | O::EventType
                | O::FireStatus
                | O::Predicate
                | O::Status
        ) {
            fullword(&target)
        } else {
            target.length == field.width()
                && matches!(
                    target.category,
                    DataCategory::Alphabetic | DataCategory::Alphanumeric
                )
        };
        if !valid {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS BTS browse {} requires a checked {}-byte receiver",
                field.name(),
                field.width()
            )));
        }
        values.push(HirCicsOutputBinding {
            name: HirCicsOutputName::BtsBrowse(field),
            target,
        });
    }
    Ok(values)
}

fn fullword(reference: &super::super::HirDataReference) -> bool {
    reference.allocated
        && reference.length == 4
        && reference.category == DataCategory::Binary
        && reference.usage == CobolUsage::Binary
        && reference.scale == 0
}
