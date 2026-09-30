//! EVENT and TIMER browse routes over the existing per-activity event authority.

use super::*;
use crate::service::handlers::event_control;

pub(super) const fn is_operation(operation: CicsOperation) -> bool {
    matches!(
        operation,
        CicsOperation::BtsStartBrowseEvent
            | CicsOperation::BtsGetNextEvent
            | CicsOperation::BtsEndBrowseEvent
            | CicsOperation::BtsInquireEvent
            | CicsOperation::BtsStartBrowseTimer
            | CicsOperation::BtsEndBrowseTimer
            | CicsOperation::BtsInquireTimer
    )
}

fn activity(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    authority: &BtsLifecycleStore<'_>,
    owner: &BrowseOwner,
) -> Result<(String, u64), HostProblem> {
    let context = event_control::context(service, run)?;
    let id = if has(request, "ACTIVITYID") {
        text_arg(request, "ACTIVITYID", 52)?
    } else {
        context.activity
    };
    super::super::super::bts_lifecycle::validate_activity_id(&id)?;
    let index = authority
        .load_activity_index(&id)?
        .ok_or_else(|| condition("ACTIVITYERR", 109, 1))?;
    let process = authority
        .load_process(&index.process_type, &index.process_name)?
        .filter(|process| process.visible_to(owner.run_unit.as_str()))
        .ok_or_else(|| condition("ACTIVITYERR", 109, 1))?;
    if !process.activities.contains_key(&id) {
        return Err(condition("ACTIVITYERR", 109, 1));
    }
    Ok((id, process.epoch))
}

fn event_scope(
    activity_id: &str,
    epoch: u64,
    kind: BrowseKind,
) -> Result<BrowseScope, HostProblem> {
    BrowseScope::new(
        kind,
        if kind == BrowseKind::Event {
            "BTSEVENT"
        } else {
            "BTSTIMER"
        },
        &format!("CICS.BTS.{activity_id}.BROWSE"),
        epoch,
    )?
    .with_activity(activity_id)
}

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    authority: &BtsLifecycleStore<'_>,
    cursors: &BtsBrowseStore<'_>,
    owner: &BrowseOwner,
) -> Result<Outputs, HostProblem> {
    match request.operation {
        CicsOperation::BtsStartBrowseEvent | CicsOperation::BtsStartBrowseTimer => {
            let kind = if request.operation == CicsOperation::BtsStartBrowseEvent {
                BrowseKind::Event
            } else {
                BrowseKind::Timer
            };
            let (id, epoch) = activity(service, run, request, authority, owner)?;
            let scope = event_scope(&id, epoch, kind)?;
            service.authorize(
                run,
                &scope.resource_class,
                &scope.resource_name,
                AccessIntent::Read,
            )?;
            let state = event_control::load_activity(service, &id)?;
            let version = state.version.max(1);
            let items = if kind == BrowseKind::Event {
                if state.events.keys().any(|name| is_system(name)) {
                    return Err(HostProblem::Unsupported);
                }
                state
                    .events
                    .iter()
                    .map(|(name, record)| {
                        Ok(BrowseItem::new(name, None, 0)?
                            .with_epoch(version)?
                            .with_event_metadata(event_metadata(record)))
                    })
                    .collect::<Result<Vec<_>, HostProblem>>()?
            } else {
                let timer = text_arg(request, "TIMER", 16)?;
                let name = state
                    .timers
                    .get_key_value(&timer)
                    .ok_or_else(|| condition("TIMERERR", 110, 13))?
                    .0;
                vec![BrowseItem::new(name, None, 0)?.with_epoch(version)?]
            };
            let token = start(cursors, owner, request, scope, items)?;
            Ok(vec![(
                "BROWSETOKEN",
                "mainframe-env.cics.decimal@1",
                token.to_string().into_bytes(),
            )])
        }
        CicsOperation::BtsGetNextEvent => {
            let token = token_arg(request)?;
            let scope = cursors.scope(owner, token, BrowseKind::Event)?;
            let id = scope
                .activity_id
                .as_deref()
                .ok_or(HostProblem::InfrastructureFailure)?;
            service.authorize(
                run,
                &scope.resource_class,
                &scope.resource_name,
                AccessIntent::Read,
            )?;
            let index = authority
                .load_activity_index(id)?
                .ok_or_else(|| condition("ACTIVITYERR", 109, 1))?;
            let process = authority
                .load_process(&index.process_type, &index.process_name)?
                .filter(|process| process.visible_to(owner.run_unit.as_str()))
                .ok_or_else(|| condition("ACTIVITYERR", 109, 1))?;
            if process.epoch != scope.epoch {
                return Err(super::super::token_error());
            }
            let item = cursors.peek(owner, token, BrowseKind::Event)?;
            if is_system(&item.name) {
                return Err(HostProblem::Unsupported);
            }
            let state = event_control::load_activity(service, id)?;
            if !state.events.contains_key(&item.name) {
                return Err(super::super::token_error());
            }
            let values = event_outputs(request, &item)?;
            let outcome = effect(
                cursors,
                owner,
                request,
                BrowseEffect::Next {
                    token,
                    kind: BrowseKind::Event,
                    live_epoch: state.version.max(1),
                    expected: item.clone(),
                },
            )?;
            if outcome != BrowseOutcome::Item(item.clone()) {
                return Err(HostProblem::InfrastructureFailure);
            }
            Ok(values)
        }
        CicsOperation::BtsEndBrowseEvent | CicsOperation::BtsEndBrowseTimer => {
            let kind = if request.operation == CicsOperation::BtsEndBrowseEvent {
                BrowseKind::Event
            } else {
                event_control::context(service, run)?;
                BrowseKind::Timer
            };
            let token = token_arg(request)?;
            let scope = cursors.scope(owner, token, kind)?;
            service.authorize(
                run,
                &scope.resource_class,
                &scope.resource_name,
                AccessIntent::Read,
            )?;
            if effect(cursors, owner, request, BrowseEffect::End { token, kind })?
                != BrowseOutcome::Ended
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            Ok(Vec::new())
        }
        CicsOperation::BtsInquireEvent | CicsOperation::BtsInquireTimer => {
            let (id, _) = activity(service, run, request, authority, owner)?;
            let event = request.operation == CicsOperation::BtsInquireEvent;
            let name = text_arg(request, if event { "EVENT" } else { "TIMER" }, 16)?;
            if event && is_system(&name) {
                return Err(HostProblem::Unsupported);
            }
            service.authorize(
                run,
                if event { "BTSEVENT" } else { "BTSTIMER" },
                &format!("CICS.BTS.{id}.{name}"),
                AccessIntent::Read,
            )?;
            let state = event_control::load_activity(service, &id)?;
            let mut outputs = Vec::new();
            if event {
                let record = state
                    .events
                    .get(&name)
                    .ok_or_else(|| condition("EVENTERR", 111, 4))?;
                let kind = event_type_code(&record.kind);
                for (field, code) in [
                    ("EVENTTYPE", kind),
                    ("FIRESTATUS", fire_status_code(record.fired)),
                ] {
                    if has(request, field) {
                        outputs.push((
                            field,
                            "mainframe-env.cics.decimal@1",
                            code.to_string().into_bytes(),
                        ));
                    }
                }
                if has(request, "COMPOSITE") {
                    outputs.push((
                        "COMPOSITE",
                        "mainframe-env.cics.payload@1",
                        padded(record.parent.as_deref().unwrap_or(""), 16)?,
                    ));
                }
                if has(request, "PREDICATE") {
                    outputs.push((
                        "PREDICATE",
                        "mainframe-env.cics.decimal@1",
                        predicate_code(&record.kind)?.to_string().into_bytes(),
                    ));
                }
                if has(request, "TIMER") {
                    let timer = match &record.kind {
                        event_control::EventKind::Timer { timer } => timer.as_str(),
                        _ => "",
                    };
                    outputs.push(("TIMER", "mainframe-env.cics.payload@1", padded(timer, 16)?));
                }
            } else {
                let record = state
                    .timers
                    .get(&name)
                    .ok_or_else(|| condition("TIMERERR", 110, 13))?;
                if has(request, "EVENT") {
                    outputs.push((
                        "EVENT",
                        "mainframe-env.cics.payload@1",
                        padded(&record.event, 16)?,
                    ));
                }
                if has(request, "STATUS") {
                    let code = timer_status_code(record.status);
                    outputs.push((
                        "STATUS",
                        "mainframe-env.cics.decimal@1",
                        code.to_string().into_bytes(),
                    ));
                }
                if has(request, "ABSTIME") {
                    let rounded = rounded_abstime(record.due_tick)?;
                    outputs.push((
                        "ABSTIME",
                        "mainframe-env.cics.decimal@1",
                        rounded.to_string().into_bytes(),
                    ));
                }
            }
            Ok(outputs)
        }
        _ => Err(HostProblem::Unsupported),
    }
}

fn is_system(name: &str) -> bool {
    name.to_ascii_uppercase().starts_with("DFH")
}

fn event_metadata(record: &event_control::EventRecord) -> BrowseEventMetadata {
    BrowseEventMetadata {
        event_type: event_type_code(&record.kind),
        fire_status: fire_status_code(record.fired),
        composite: record.parent.clone(),
        predicate: predicate_code(&record.kind).ok(),
        timer: match &record.kind {
            event_control::EventKind::Timer { timer } => Some(timer.clone()),
            _ => None,
        },
    }
}

pub(super) fn event_outputs(
    request: &CicsRequest,
    item: &BrowseItem,
) -> Result<Outputs, HostProblem> {
    let mut outputs = vec![(
        "EVENT",
        "mainframe-env.cics.payload@1",
        padded(&item.name, 16)?,
    )];
    let metadata_requested = ["EVENTTYPE", "FIRESTATUS", "COMPOSITE", "PREDICATE", "TIMER"]
        .iter()
        .any(|name| has(request, name));
    if !metadata_requested {
        return Ok(outputs);
    }
    let metadata = item
        .event_metadata
        .as_ref()
        .ok_or(HostProblem::Unsupported)?;
    for (field, value) in [
        ("EVENTTYPE", metadata.event_type),
        ("FIRESTATUS", metadata.fire_status),
    ] {
        if has(request, field) {
            outputs.push((
                field,
                "mainframe-env.cics.decimal@1",
                value.to_string().into_bytes(),
            ));
        }
    }
    if has(request, "COMPOSITE") {
        outputs.push((
            "COMPOSITE",
            "mainframe-env.cics.payload@1",
            padded(metadata.composite.as_deref().unwrap_or(""), 16)?,
        ));
    }
    if has(request, "PREDICATE") {
        outputs.push((
            "PREDICATE",
            "mainframe-env.cics.decimal@1",
            metadata
                .predicate
                .ok_or(HostProblem::Unsupported)?
                .to_string()
                .into_bytes(),
        ));
    }
    if has(request, "TIMER") {
        outputs.push((
            "TIMER",
            "mainframe-env.cics.payload@1",
            padded(metadata.timer.as_deref().unwrap_or(""), 16)?,
        ));
    }
    Ok(outputs)
}

fn event_type_code(kind: &event_control::EventKind) -> i32 {
    match kind {
        event_control::EventKind::Input => 226,
        event_control::EventKind::Activity { .. } => 1002,
        event_control::EventKind::Composite { .. } => 1003,
        event_control::EventKind::Timer { .. } => 1004,
    }
}

fn fire_status_code(fired: bool) -> i32 {
    if fired { 1001 } else { 1000 }
}

fn predicate_code(kind: &event_control::EventKind) -> Result<i32, HostProblem> {
    match kind {
        event_control::EventKind::Composite { all: true, .. } => Ok(1005),
        event_control::EventKind::Composite { all: false, .. } => Ok(1006),
        _ => Err(HostProblem::Unsupported),
    }
}

fn timer_status_code(status: event_control::TimerStatus) -> i32 {
    match status {
        event_control::TimerStatus::Pending => 1018,
        event_control::TimerStatus::Expired => 1017,
        event_control::TimerStatus::Forced => 1013,
    }
}

fn rounded_abstime(millis: u64) -> Result<u64, HostProblem> {
    millis
        .checked_add(5)
        .map(|value| value / 10 * 10)
        .ok_or(HostProblem::ResourceExhausted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[test]
    fn bts_browse_event_metadata_matches_each_event_kind() {
        use event_control::EventKind as K;
        let cases = [
            (K::Input, 226, None, None),
            (
                K::Activity {
                    child_id: "CHILD".into(),
                },
                1002,
                None,
                None,
            ),
            (
                K::Composite {
                    all: true,
                    children: vec![],
                    fired_queue: VecDeque::new(),
                },
                1003,
                Some(1005),
                None,
            ),
            (
                K::Timer {
                    timer: "WAKE".into(),
                },
                1004,
                None,
                Some("WAKE"),
            ),
        ];
        for (kind, event_type, predicate, timer) in cases {
            let record = event_control::EventRecord {
                kind,
                fired: true,
                parent: Some("PARENT".into()),
            };
            let metadata = event_metadata(&record);
            assert_eq!(metadata.event_type, event_type);
            assert_eq!(metadata.fire_status, 1001);
            assert_eq!(metadata.composite.as_deref(), Some("PARENT"));
            assert_eq!(metadata.predicate, predicate);
            assert_eq!(metadata.timer.as_deref(), timer);
        }
    }

    #[test]
    fn bts_browse_event_metadata_outputs_are_individual_and_system_is_fenced() {
        use mainframe_env_host_api::CicsConditionPolicy;
        let record = event_control::EventRecord {
            kind: event_control::EventKind::Composite {
                all: false,
                children: vec![],
                fired_queue: VecDeque::new(),
            },
            fired: false,
            parent: Some("PARENT".into()),
        };
        let item = BrowseItem::new("READY", None, 0)
            .unwrap()
            .with_epoch(1)
            .unwrap()
            .with_event_metadata(event_metadata(&record));
        for (field, expected) in [
            ("EVENTTYPE", b"1003".as_slice()),
            ("FIRESTATUS", b"1000".as_slice()),
            ("COMPOSITE", b"PARENT          ".as_slice()),
            ("PREDICATE", b"1006".as_slice()),
            ("TIMER", b"                ".as_slice()),
        ] {
            let request = CicsRequest {
                operation: CicsOperation::BtsGetNextEvent,
                arguments: [(
                    field.into(),
                    BoundedPayload::new(
                        "mainframe-env.cics.argument@1",
                        b"OUT".to_vec(),
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                )]
                .into(),
                condition_policy: CicsConditionPolicy::Default,
                mutation: None,
            };
            let outputs = event_outputs(&request, &item).unwrap();
            assert_eq!(outputs[0].2, b"READY           ");
            assert_eq!(outputs[1].2, expected, "{field}");
        }
        assert!(is_system("DFHINITIAL"));
        assert!(is_system("dfhOther"));
    }

    #[test]
    fn bts_browse_inquire_event_timer_cvda_table_and_1900_epoch() {
        use event_control::EventKind as K;
        for (kind, code) in [
            (K::Input, 226),
            (
                K::Activity {
                    child_id: "CHILD".into(),
                },
                1002,
            ),
            (
                K::Composite {
                    all: true,
                    children: vec![],
                    fired_queue: VecDeque::new(),
                },
                1003,
            ),
            (
                K::Timer {
                    timer: "WAKE".into(),
                },
                1004,
            ),
        ] {
            assert_eq!(event_type_code(&kind), code);
        }
        for (all, code) in [(true, 1005), (false, 1006)] {
            assert_eq!(
                predicate_code(&K::Composite {
                    all,
                    children: vec![],
                    fired_queue: VecDeque::new()
                }),
                Ok(code)
            );
        }
        assert_eq!(fire_status_code(false), 1000);
        assert_eq!(fire_status_code(true), 1001);
        assert_eq!(predicate_code(&K::Input), Err(HostProblem::Unsupported));
        for (status, code) in [
            (event_control::TimerStatus::Pending, 1018),
            (event_control::TimerStatus::Expired, 1017),
            (event_control::TimerStatus::Forced, 1013),
        ] {
            assert_eq!(timer_status_code(status), code);
        }
        assert_eq!(rounded_abstime(0), Ok(0));
        assert_eq!(
            crate::service::handlers::time::absolute_milliseconds(
                crate::service::handlers::time::parse_clock_timestamp("20000101000000000").unwrap()
            ),
            Ok(3_155_673_600_000)
        );
        assert_eq!(rounded_abstime(3_155_673_600_004), Ok(3_155_673_600_000));
        assert_eq!(rounded_abstime(3_155_673_600_005), Ok(3_155_673_600_010));
        assert_eq!(
            rounded_abstime(u64::MAX),
            Err(HostProblem::ResourceExhausted)
        );
    }
}
