use super::*;

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    let token = spool_token(request)?;
    let from = request
        .arguments
        .get("FROM")
        .ok_or(HostProblem::Malformed)?
        .bytes();
    let length = request
        .arguments
        .get("FLENGTH")
        .map(|_| spool_decimal(request, "FLENGTH"))
        .transpose()?
        .unwrap_or_else(|| i64::try_from(from.len()).unwrap_or(i64::MAX));
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let request_digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let mut state = service.lock()?;
    if let Some(reply) = state
        .spool
        .replay(mutation.idempotency_key.as_str(), request_digest)?
    {
        return response(service, run, reply);
    }
    let report = state.spool.reports.get(&token).ok_or_else(not_open)?;
    if report.owner_run_unit.as_deref() != Some(run.invocation.run_unit_id.as_str())
        || report.owner_principal.as_deref() != Some(run.invocation.principal.id().as_str())
    {
        return Err(not_open());
    }
    if report.state == SpoolReportState::OpenInput {
        return Err(HostProblem::Condition {
            name: "NOTOPEN".into(),
            response: 19,
            response2: 16,
        });
    }
    if report.state != SpoolReportState::OpenOutput {
        return Err(not_open());
    }
    if !(1..=32_760).contains(&length)
        || usize::try_from(length).map_or(true, |length| length > from.len())
    {
        return Err(HostProblem::Condition {
            name: "LENGERR".into(),
            response: 22,
            response2: 0,
        });
    }
    if state
        .spool
        .reports
        .values()
        .map(|report| report.records.len())
        .sum::<usize>()
        >= service.limits.max_spool_records
    {
        return Err(HostProblem::ResourceExhausted);
    }
    let length = usize::try_from(length).map_err(|_| HostProblem::InfrastructureFailure)?;
    let accepted = length.min(report.record_length as usize);
    if report.user_id.eq_ignore_ascii_case("INTRDR") {
        if let Some(job_user) = job_card_user(&from[..accepted]) {
            if !job_user.eq_ignore_ascii_case(run.invocation.principal.id().as_str()) {
                service
                    .authorize(
                        run,
                        "SURROGAT",
                        &format!("{job_user}.SUBMIT"),
                        AccessIntent::Read,
                    )
                    .map_err(|problem| match problem {
                        HostProblem::Unauthorized => HostProblem::Condition {
                            name: "NOTAUTH".into(),
                            response: 70,
                            response2: 1,
                        },
                        other => other,
                    })?;
            }
        }
    }
    let current_version = state.spool.version;
    let mut next = state.spool.clone();
    let report = next
        .reports
        .get_mut(&token)
        .ok_or(HostProblem::InfrastructureFailure)?;
    if accepted != 0 {
        report.records.push(SpoolRecord {
            mode: if request.arguments.contains_key("OPTION.PAGE") {
                SpoolRecordMode::Page
            } else {
                SpoolRecordMode::Line
            },
            bytes: from[..accepted].to_vec(),
        });
    }
    let mut reply = SpoolReply::normal();
    if accepted < length {
        reply.condition = "LENGERR".into();
        reply.response = 22;
        reply.response2 =
            i32::try_from(length - accepted).map_err(|_| HostProblem::InfrastructureFailure)?;
    }
    next.record_replay(
        mutation.idempotency_key.as_str(),
        request_digest,
        reply.clone(),
        service.limits,
    )?;
    persist_spool_state(service, current_version, &mut next)?;
    state.spool = next;
    response(service, run, reply)
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    if !request.arguments.contains_key("FROM") {
        return Err(HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2: 28,
        });
    }
    if !request.arguments.contains_key("TOKEN")
        || (!request.arguments.contains_key("RESP")
            && !request.arguments.contains_key("OPTION.NOHANDLE"))
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request.arguments.contains_key("OPTION.LINE")
            && request.arguments.contains_key("OPTION.PAGE")
        || request
            .arguments
            .iter()
            .any(|(name, value)| match name.as_str() {
                "TOKEN" | "FROM" => value.schema() != "mainframe-env.cics.storage-value@1",
                "FLENGTH" => value.schema() != "mainframe-env.cics.decimal@1",
                "RESP" | "RESP2" => value.schema() != "mainframe-env.cics.argument@1",
                "OPTION.NOHANDLE" | "OPTION.LINE" | "OPTION.PAGE" => {
                    value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                }
                _ => true,
            })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn job_card_user(record: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(record)
        .ok()?
        .trim()
        .to_ascii_uppercase();
    if !text.starts_with("//") || !text.contains(" JOB ") || text.contains("PASSWORD=") {
        return None;
    }
    let tail = text.split_once("USER=")?.1;
    let user = tail
        .bytes()
        .take_while(u8::is_ascii_alphanumeric)
        .collect::<Vec<_>>();
    (1..=8)
        .contains(&user.len())
        .then(|| String::from_utf8(user).ok())
        .flatten()
}
