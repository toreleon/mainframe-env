use super::super::time::{
    ClockInstant, absolute_milliseconds, civil_from_days, days_from_civil, parse_clock_timestamp,
};
use super::{
    ActivityState, EventKind, EventOutput, EventRecord, MAX_EVENTS, MAX_TIMERS, TimerRecord,
    TimerStatus, context, event_error, event_name, fire_atomic, mutate_activity,
};
use crate::service::{CicsService, Run};
use mainframe_env_host_api::{
    AccessIntent, CicsOperation, CicsRequest, CicsResponse, ClockRequest, HostProblem, HostRequest,
    HostResult,
};
use std::collections::BTreeMap;

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    let context = context(service, run)?;
    let timer = name(request, "TIMER", 14)?;
    let activity = if request.operation == CicsOperation::ForceTimer {
        if request.arguments.contains_key("OPTION.ACQPROCESS") {
            context.acquired_process.ok_or_else(|| invreq(16))?
        } else if request.arguments.contains_key("OPTION.ACQACTIVITY") {
            context.acquired_activity.ok_or_else(|| invreq(17))?
        } else {
            context.activity
        }
    } else {
        context.activity
    };
    let event = if request.operation == CicsOperation::DefineTimer {
        request
            .arguments
            .get("EVENT")
            .map(|_| name(request, "EVENT", 6))
            .transpose()?
            .unwrap_or_else(|| timer.clone())
    } else {
        String::new()
    };
    let now = clock_millis(service, run)?;
    let due = if request.operation == CicsOperation::DefineTimer {
        Some(due_millis(request, now)?)
    } else {
        None
    };
    service.authorize(
        run,
        "BTSTIMER",
        &format!("CICS.BTS.{}.{}", activity, timer),
        AccessIntent::Update,
    )?;
    mutate_activity(service, run, request, &activity, |state| {
        refresh_due(state, now)?;
        let mut outputs = BTreeMap::new();
        match request.operation {
            CicsOperation::DefineTimer => {
                if state.timers.contains_key(&timer) {
                    return Err(timererr(15));
                }
                if event == "DFHINITIAL" || state.events.contains_key(&event) {
                    return Err(event_error(7));
                }
                if state.timers.len() == MAX_TIMERS || state.events.len() == MAX_EVENTS {
                    return Err(HostProblem::ResourceExhausted);
                }
                state.events.insert(
                    event.clone(),
                    EventRecord {
                        kind: EventKind::Timer {
                            timer: timer.clone(),
                        },
                        fired: false,
                        parent: None,
                    },
                );
                let due_tick = due.expect("validated DEFINE TIMER deadline");
                state.timers.insert(
                    timer.clone(),
                    TimerRecord {
                        event: event.clone(),
                        due_tick,
                        status: TimerStatus::Pending,
                        acknowledged: false,
                    },
                );
                if due_tick <= now {
                    expire(state, &timer, TimerStatus::Expired)?;
                }
            }
            CicsOperation::ForceTimer => {
                if !state.timers.contains_key(&timer) {
                    return Err(timererr(13));
                }
                expire(state, &timer, TimerStatus::Forced)?;
            }
            CicsOperation::CheckTimer => {
                let record = state
                    .timers
                    .get(&timer)
                    .ok_or_else(|| timererr(13))?
                    .clone();
                let status = match record.status {
                    TimerStatus::Pending => "UNEXPIRED",
                    TimerStatus::Expired => "EXPIRED",
                    TimerStatus::Forced => "FORCED",
                };
                outputs.insert(
                    "STATUS".into(),
                    EventOutput {
                        schema: "mainframe-env.cics.cvda@1".into(),
                        bytes: status.as_bytes().to_vec(),
                    },
                );
                if record.status != TimerStatus::Pending && !record.acknowledged {
                    discard_event(state, &record.event)?;
                    state
                        .timers
                        .get_mut(&timer)
                        .expect("checked timer")
                        .acknowledged = true;
                }
            }
            CicsOperation::DeleteTimer => {
                let record = state.timers.remove(&timer).ok_or_else(|| timererr(13))?;
                if !record.acknowledged {
                    discard_event(state, &record.event)?;
                }
            }
            _ => return Err(HostProblem::InfrastructureFailure),
        }
        Ok(outputs)
    })
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let allowed: &[&str] = match request.operation {
        CicsOperation::DefineTimer => &[
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
            "OPTION.AFTER",
            "OPTION.AT",
            "OPTION.ON",
            "OPTION.NOHANDLE",
            "RESP",
            "RESP2",
        ],
        CicsOperation::CheckTimer => &["TIMER", "STATUS", "OPTION.NOHANDLE", "RESP", "RESP2"],
        CicsOperation::DeleteTimer => &["TIMER", "OPTION.NOHANDLE", "RESP", "RESP2"],
        CicsOperation::ForceTimer => &[
            "TIMER",
            "OPTION.ACQACTIVITY",
            "OPTION.ACQPROCESS",
            "OPTION.NOHANDLE",
            "RESP",
            "RESP2",
        ],
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    for (name, value) in &request.arguments {
        if !allowed.contains(&name.as_str()) {
            return Err(HostProblem::Malformed);
        }
        let valid = match name.as_str() {
            "TIMER" | "EVENT" => matches!(
                value.schema(),
                "mainframe-env.cics.argument@1"
                    | "mainframe-env.cics.literal@1"
                    | "mainframe-env.cics.storage-value@1"
            ),
            "DAYS" | "HOURS" | "MINUTES" | "SECONDS" | "YEAR" | "MONTH" | "DAYOFMONTH"
            | "DAYOFYEAR" => value.schema() == "mainframe-env.cics.decimal@1",
            "STATUS" | "RESP" | "RESP2" => value.schema() == "mainframe-env.cics.argument@1",
            "OPTION.AFTER" | "OPTION.AT" | "OPTION.ON" | "OPTION.ACQACTIVITY"
            | "OPTION.ACQPROCESS" | "OPTION.NOHANDLE" => {
                value.schema() == "mainframe-env.cics.option@1" && value.bytes().is_empty()
            }
            _ => false,
        };
        if !valid {
            return Err(HostProblem::Malformed);
        }
    }
    if !request.arguments.contains_key("TIMER")
        || request.operation == CicsOperation::CheckTimer
            && !request.arguments.contains_key("STATUS")
        || request.arguments.contains_key("OPTION.ACQACTIVITY")
            && request.arguments.contains_key("OPTION.ACQPROCESS")
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn name(request: &CicsRequest, key: &str, invalid: i32) -> Result<String, HostProblem> {
    request
        .arguments
        .get(key)
        .ok_or(HostProblem::Malformed)
        .and_then(|value| std::str::from_utf8(value.bytes()).map_err(|_| timererr(invalid)))
        .and_then(|value| {
            event_name(value).map_err(|_| {
                if key == "EVENT" {
                    event_error(invalid)
                } else {
                    timererr(invalid)
                }
            })
        })
}

pub(super) fn clock_millis(service: &CicsService, run: &mut Run) -> Result<u64, HostProblem> {
    let timestamp = match service.nested(run, HostRequest::Clock(ClockRequest::UtcTimestamp))? {
        HostResult::Clock(value) => value,
        _ => return Err(HostProblem::ProviderFailure),
    };
    u64::try_from(absolute_milliseconds(parse_clock_timestamp(&timestamp)?)?)
        .map_err(|_| HostProblem::ProviderFailure)
}

fn number(
    request: &CicsRequest,
    name: &str,
    max: i64,
    response2: i32,
) -> Result<Option<i64>, HostProblem> {
    request
        .arguments
        .get(name)
        .map(|value| {
            let parsed = std::str::from_utf8(value.bytes())
                .ok()
                .and_then(|text| text.parse::<i64>().ok())
                .ok_or_else(|| invreq(response2))?;
            if !(0..=max).contains(&parsed) {
                return Err(invreq(response2));
            }
            Ok(parsed)
        })
        .transpose()
}

fn due_millis(request: &CicsRequest, now: u64) -> Result<u64, HostProblem> {
    let after = request.arguments.contains_key("OPTION.AFTER");
    let at = request.arguments.contains_key("OPTION.AT");
    let on = request.arguments.contains_key("OPTION.ON");
    if after == at || on && !at {
        return Err(invreq(if after { 11 } else { 12 }));
    }
    let response2 = if after { 11 } else { 12 };
    let days = number(request, "DAYS", 999, response2)?.unwrap_or(0);
    let hours = number(request, "HOURS", 23, response2)?.unwrap_or(0);
    let minutes = number(request, "MINUTES", 59, response2)?.unwrap_or(0);
    let seconds = number(request, "SECONDS", 59, response2)?.unwrap_or(0);
    if !["DAYS", "HOURS", "MINUTES", "SECONDS"]
        .iter()
        .any(|name| request.arguments.contains_key(*name))
        || at && request.arguments.contains_key("DAYS")
        || after
            && ["YEAR", "MONTH", "DAYOFMONTH", "DAYOFYEAR"]
                .iter()
                .any(|name| request.arguments.contains_key(*name))
    {
        return Err(invreq(response2));
    }
    if after {
        let interval = (days * 86_400 + hours * 3_600 + minutes * 60 + seconds)
            .checked_mul(1_000)
            .ok_or_else(|| invreq(11))?;
        return now.checked_add(interval as u64).ok_or_else(|| invreq(11));
    }
    let current = super::super::time::instant_from_absolute(now as i64)?;
    let year = number(request, "YEAR", 2110, 12)?.unwrap_or(current.year);
    let month = number(request, "MONTH", 12, 12)?.unwrap_or(i64::from(current.month));
    let day = number(request, "DAYOFMONTH", 31, 12)?.unwrap_or(i64::from(current.day));
    let ordinal = number(request, "DAYOFYEAR", 366, 12)?;
    if year < 1 || month < 1 || day < 1 || ordinal == Some(0) {
        return Err(invreq(12));
    }
    if !on
        && ["YEAR", "MONTH", "DAYOFMONTH", "DAYOFYEAR"]
            .iter()
            .any(|name| request.arguments.contains_key(*name))
        || ordinal.is_some() && request.arguments.contains_key("DAYOFMONTH")
    {
        return Err(invreq(12));
    }
    let date_days = if let Some(ordinal) = ordinal {
        let jan1 = days_from_civil(year, 1, 1).ok_or_else(|| invreq(12))?;
        let target = jan1.checked_add(ordinal - 1).ok_or_else(|| invreq(12))?;
        if civil_from_days(target).0 != year {
            return Err(invreq(12));
        }
        target
    } else {
        let target = days_from_civil(year, month as u32, day as u32).ok_or_else(|| invreq(12))?;
        if civil_from_days(target) != (year, month as u32, day as u32) {
            return Err(invreq(12));
        }
        target
    };
    let (_, m, d) = civil_from_days(date_days);
    let instant = ClockInstant {
        year,
        month: m,
        day: d,
        hour: hours as u32,
        minute: minutes as u32,
        second: seconds as u32,
        millisecond: 0,
    };
    u64::try_from(absolute_milliseconds(instant)?).map_err(|_| invreq(12))
}

pub(super) fn refresh_due(state: &mut ActivityState, now: u64) -> Result<(), HostProblem> {
    let expired = state
        .timers
        .iter()
        .filter(|(_, timer)| timer.status == TimerStatus::Pending && timer.due_tick <= now)
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    for name in expired {
        expire(state, &name, TimerStatus::Expired)?;
    }
    Ok(())
}

fn expire(state: &mut ActivityState, name: &str, status: TimerStatus) -> Result<(), HostProblem> {
    let timer = state.timers.get(name).ok_or_else(|| timererr(13))?;
    if timer.status != TimerStatus::Pending {
        return Ok(());
    }
    let event = timer.event.clone();
    fire_atomic(state, &event)?;
    state.timers.get_mut(name).expect("validated timer").status = status;
    Ok(())
}

fn discard_event(state: &mut ActivityState, name: &str) -> Result<(), HostProblem> {
    let Some(record) = state.events.remove(name) else {
        return Err(HostProblem::InfrastructureFailure);
    };
    state.reattach.retain(|item| item != name);
    if let Some(parent_name) = record.parent {
        let parent = state
            .events
            .get_mut(&parent_name)
            .ok_or(HostProblem::InfrastructureFailure)?;
        let EventKind::Composite {
            children,
            fired_queue,
            ..
        } = &mut parent.kind
        else {
            return Err(HostProblem::InfrastructureFailure);
        };
        children.retain(|item| item != name);
        fired_queue.retain(|item| item != name);
        super::composite::reevaluate(state, &parent_name)?;
    }
    Ok(())
}

fn invreq(response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: "INVREQ".into(),
        response: 16,
        response2,
    }
}

fn timererr(response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: "TIMERERR".into(),
        response: 115,
        response2,
    }
}
