use super::*;

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_read_request(request)?;
    let token = spool_token(request)?;
    let maxflength = spool_decimal(request, "MAXFLENGTH")?;
    let into_capacity = request
        .arguments
        .get("INTO.MAXLENGTH")
        .map(|_| spool_decimal(request, "INTO.MAXLENGTH"))
        .transpose()?
        .unwrap_or(maxflength);
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
    if report.state == SpoolReportState::OpenOutput {
        return Err(HostProblem::Condition {
            name: "NOTOPEN".into(),
            response: 19,
            response2: 12,
        });
    }
    if report.state != SpoolReportState::OpenInput {
        return Err(not_open());
    }
    if report.eof_seen {
        return Err(HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2: 12,
        });
    }
    if maxflength < 0 || maxflength > 32_760 || into_capacity < 0 {
        return Err(HostProblem::Condition {
            name: "LENGERR".into(),
            response: 22,
            response2: 0,
        });
    }
    let current_version = state.spool.version;
    let mut next = state.spool.clone();
    let report = next
        .reports
        .get_mut(&token)
        .ok_or(HostProblem::InfrastructureFailure)?;
    let mut reply = SpoolReply::normal();
    if report.next_record == report.records.len() {
        report.eof_seen = true;
        reply.condition = "ENDFILE".into();
        reply.response = 20;
    } else {
        let record = &report.records[report.next_record].bytes;
        let actual = record.len();
        let capacity = usize::try_from(maxflength.min(into_capacity))
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        reply.toflength = Some(actual as i64);
        reply.payload = record[..actual.min(capacity)].to_vec();
        if capacity < actual {
            reply.condition = "LENGERR".into();
            reply.response = 22;
            reply.response2 =
                i32::try_from(actual - capacity).map_err(|_| HostProblem::InfrastructureFailure)?;
        } else {
            report.next_record += 1;
        }
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

fn validate_read_request(request: &CicsRequest) -> Result<(), HostProblem> {
    if !request.arguments.contains_key("INTO") {
        return Err(HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2: 24,
        });
    }
    if !request.arguments.contains_key("TOKEN")
        || !request.arguments.contains_key("MAXFLENGTH")
        || (!request.arguments.contains_key("RESP")
            && !request.arguments.contains_key("OPTION.NOHANDLE"))
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request
            .arguments
            .iter()
            .any(|(name, value)| match name.as_str() {
                "TOKEN" => value.schema() != "mainframe-env.cics.storage-value@1",
                "MAXFLENGTH" | "INTO.MAXLENGTH" => value.schema() != "mainframe-env.cics.decimal@1",
                "INTO" | "TOFLENGTH" | "RESP" | "RESP2" => {
                    value.schema() != "mainframe-env.cics.argument@1"
                }
                "OPTION.NOHANDLE" => {
                    value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                }
                _ => true,
            })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn spool_decimal(request: &CicsRequest, name: &str) -> Result<i64, HostProblem> {
    let value = request
        .arguments
        .get(name)
        .ok_or(HostProblem::Malformed)?
        .bytes();
    std::str::from_utf8(value)
        .map_err(|_| HostProblem::Malformed)?
        .parse::<i64>()
        .map_err(|_| HostProblem::Malformed)
}
