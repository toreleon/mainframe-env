use super::selector::decimal;
use super::*;

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let owner = run.invocation.run_unit_id.as_str().to_owned();
    let mut task = state::read_task(service, &owner)?;
    if request.operation == CicsOperation::IssueQuery {
        let selected = selector::select(service, request, &task)?;
        if selected.definition.kind != CicsOutboardKind::Sequential {
            return Err(condition("FUNCERR", 48, 0));
        }
        service.authorize(
            run,
            "FACILITY",
            &format!("CICS.OUTBOARD.{}", selected.name),
            AccessIntent::Read,
        )?;
        task.selected = Some(selected.name.clone());
        task.query_destination = Some(selected.name);
        task.query_records = selected
            .data
            .records
            .iter()
            .map(|record| record.data.clone())
            .collect();
        task.query_index = 0;
        task.query_aborted = false;
        let receipt = receipt(run, request)?;
        return commit(service, run, request, None, &task, &receipt);
    }
    if request.operation != CicsOperation::IssueReceive {
        return Err(HostProblem::InfrastructureFailure);
    }
    let destination = task
        .query_destination
        .clone()
        .ok_or_else(|| condition("INVREQ", 16, 0))?;
    service.authorize(
        run,
        "FACILITY",
        &format!("CICS.OUTBOARD.{destination}"),
        AccessIntent::Read,
    )?;
    let mut receipt = receipt(run, request)?;
    if task.query_aborted {
        receipt.condition = "DSSTAT".into();
        receipt.response = 46;
    } else if let Some(record) = task.query_records.get(task.query_index).cloned() {
        let actual = record.len();
        let input_limit = receive_length(request)?;
        output(
            &mut receipt,
            "LENGTH",
            "mainframe-env.cics.decimal@1",
            actual.to_string().into_bytes(),
        );
        if request.arguments.contains_key("INTO") {
            let capacity = decimal(request, "INTO.MAXLENGTH")?.unwrap_or(input_limit);
            let length = actual.min(input_limit).min(capacity);
            receipt.payload = record[..length].to_vec();
            if length < actual {
                receipt.condition = "LENGERR".into();
                receipt.response = 22;
            }
        } else {
            let capacity = decimal(request, "SET.MAXLENGTH")?.ok_or(HostProblem::Malformed)?;
            if actual > capacity {
                return Err(HostProblem::ResourceExhausted);
            }
            output(&mut receipt, "SET", "mainframe-env.cics.payload@1", record);
        }
        task.query_index = task
            .query_index
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        task.last_inbound_destination = Some(destination);
    } else {
        receipt.condition = "EODS".into();
        receipt.response = 5;
    }
    commit(service, run, request, None, &task, &receipt)
}

fn receive_length(request: &CicsRequest) -> Result<usize, HostProblem> {
    let bytes = request
        .arguments
        .get("LENGTH")
        .ok_or(HostProblem::Malformed)?
        .bytes();
    let value = std::str::from_utf8(bytes)
        .map_err(|_| HostProblem::Malformed)?
        .parse::<i64>()
        .map_err(|_| HostProblem::Malformed)?;
    if value < 0 {
        Ok(0)
    } else {
        usize::try_from(value).map_err(|_| HostProblem::Malformed)
    }
}
