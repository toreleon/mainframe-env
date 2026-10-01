//! Deterministic route deadlines in the service's virtual tick domain.

use super::*;

fn number(request: &CicsRequest, name: &str) -> Result<Option<u64>, HostProblem> {
    request
        .arguments
        .get(name)
        .map(|value| {
            let text = std::str::from_utf8(value.bytes()).map_err(|_| HostProblem::Malformed)?;
            text.parse::<u64>().map_err(|_| HostProblem::Malformed)
        })
        .transpose()
}

fn hhmmss(value: u64, absolute: bool) -> Result<u64, HostProblem> {
    let hours = value / 10_000;
    let minutes = value / 100 % 100;
    let seconds = value % 100;
    if hours > if absolute { 23 } else { 99 } || minutes > 59 || seconds > 59 {
        return Err(HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2: if hours > if absolute { 23 } else { 99 } {
                4
            } else if minutes > 59 {
                5
            } else {
                6
            },
        });
    }
    hours
        .checked_mul(3_600)
        .and_then(|v| v.checked_add(minutes * 60))
        .and_then(|v| v.checked_add(seconds))
        .ok_or(HostProblem::ResourceExhausted)
}

pub(super) fn due_tick(
    service: &CicsService,
    request: &CicsRequest,
) -> Result<(u64, u64), HostProblem> {
    let now = service
        .replay_clock
        .as_ref()
        .map(|clock| clock.now_tick())
        .transpose()?
        .unwrap_or(0);
    if let Some(value) = number(request, "INTERVAL")? {
        return Ok((
            now,
            now.checked_add(hhmmss(value, false)?)
                .ok_or(HostProblem::ResourceExhausted)?,
        ));
    }
    if let Some(value) = number(request, "TIME")? {
        return Ok((now, at(now, hhmmss(value, true)?)?));
    }
    let after = request.arguments.contains_key("OPTION.AFTER");
    let absolute = request.arguments.contains_key("OPTION.AT");
    if !after && !absolute {
        return Ok((now, now));
    }
    let h = number(request, "HOURS")?;
    let m = number(request, "MINUTES")?;
    let s = number(request, "SECONDS")?;
    let count = [h, m, s].iter().flatten().count();
    let hours = h.unwrap_or(0);
    let minutes = m.unwrap_or(0);
    let seconds = s.unwrap_or(0);
    if hours > if absolute { 23 } else { 99 } {
        return Err(condition2(4));
    }
    if minutes > if count == 1 { 5_999 } else { 59 } {
        return Err(condition2(5));
    }
    if seconds > if count == 1 { 359_999 } else { 59 } {
        return Err(condition2(6));
    }
    let offset = hours
        .checked_mul(3_600)
        .and_then(|v| v.checked_add(minutes * 60))
        .and_then(|v| v.checked_add(seconds))
        .ok_or(HostProblem::ResourceExhausted)?;
    if absolute {
        Ok((now, at(now, offset)?))
    } else {
        Ok((
            now,
            now.checked_add(offset)
                .ok_or(HostProblem::ResourceExhausted)?,
        ))
    }
}

fn at(now: u64, seconds: u64) -> Result<u64, HostProblem> {
    let day = now / 86_400;
    let mut due = day
        .checked_mul(86_400)
        .and_then(|v| v.checked_add(seconds))
        .ok_or(HostProblem::ResourceExhausted)?;
    if due < now {
        due = due
            .checked_add(86_400)
            .ok_or(HostProblem::ResourceExhausted)?;
    }
    Ok(due)
}

fn condition2(response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: "INVREQ".into(),
        response: 16,
        response2,
    }
}
