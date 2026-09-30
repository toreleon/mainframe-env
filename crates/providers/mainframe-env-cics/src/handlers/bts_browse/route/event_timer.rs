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
            let names: Vec<&str> = if kind == BrowseKind::Event {
                if state.events.keys().any(|name| is_system(name)) {
                    return Err(HostProblem::Unsupported);
                }
                state.events.keys().map(String::as_str).collect()
            } else {
                let timer = text_arg(request, "TIMER", 16)?;
                let name = state
                    .timers
                    .get_key_value(&timer)
                    .ok_or_else(|| condition("TIMERERR", 110, 13))?
                    .0;
                vec![name.as_str()]
            };
            let items = names
                .into_iter()
                .map(|name| BrowseItem::new(name, None, 0)?.with_epoch(version))
                .collect::<Result<Vec<_>, _>>()?;
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
            Ok(vec![(
                "EVENT",
                "mainframe-env.cics.payload@1",
                padded(&item.name, 16)?,
            )])
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
            if event && !state.events.contains_key(&name) {
                return Err(condition("EVENTERR", 111, 4));
            }
            if !event && !state.timers.contains_key(&name) {
                return Err(condition("TIMERERR", 110, 13));
            }
            Ok(Vec::new())
        }
        _ => Err(HostProblem::Unsupported),
    }
}

fn is_system(name: &str) -> bool {
    name.to_ascii_uppercase().starts_with("DFH")
}
