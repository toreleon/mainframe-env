//! Durable local LUTYPE4 and batch data interchange commands.

use super::super::super::{CicsService, Run, mutation_problem};
use super::super::store_error;
use mainframe_env_execution_api::InvocationLimits;
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
    HostRequest, canonical_request_digest,
};
use mainframe_env_store_api::{
    ProviderStateMutation, ProviderStateRecord, ProviderStateWrite, StoreError,
};
use std::collections::{BTreeMap, BTreeSet};

mod receive;
mod records;
mod selector;
mod state;

pub use state::{
    CicsOutboardDestinationDefinition, CicsOutboardKind, CicsOutboardRecord, CicsOutboardSnapshot,
};
use state::{DataState, Receipt, ReceiptOutput, TaskState};

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    selector::validate_request(request)?;
    super::validate_purge_message_context(run)?;
    if let Some(response) = replay(service, run, request)? {
        return Ok(response);
    }
    match request.operation {
        CicsOperation::IssueAdd
        | CicsOperation::IssueErase
        | CicsOperation::IssueReplace
        | CicsOperation::IssueNote
        | CicsOperation::IssueSend => records::invoke(service, run, request),
        CicsOperation::IssueQuery | CicsOperation::IssueReceive => {
            receive::invoke(service, run, request)
        }
        CicsOperation::IssueAbort | CicsOperation::IssueEnd | CicsOperation::IssueWait => {
            selection_control(service, run, request)
        }
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

pub(in crate::service) fn release_task(
    service: &CicsService,
    run: &Run,
) -> Result<(), HostProblem> {
    let key = run.invocation.run_unit_id.as_str();
    if let Some(row) = service
        .store
        .get_provider_state(state::TASK_NAMESPACE, key)
        .map_err(store_error)?
    {
        service
            .store
            .mutate_provider_states_atomic(vec![ProviderStateMutation::Delete {
                namespace: state::TASK_NAMESPACE.into(),
                key: key.into(),
                expected_version: row.version,
            }])
            .map_err(store_error)
            .map_err(mutation_problem)?;
    }
    Ok(())
}

fn selection_control(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let owner = run.invocation.run_unit_id.as_str().to_owned();
    let mut task = state::read_task(service, &owner)?;
    let selected = selector::select(service, request, &task)?;
    let intent = if request.operation == CicsOperation::IssueWait {
        AccessIntent::Read
    } else {
        AccessIntent::Update
    };
    service.authorize(
        run,
        "FACILITY",
        &format!("CICS.OUTBOARD.{}", selected.name),
        intent,
    )?;
    let mut receipt = receipt(run, request)?;
    let mut data = selected.data;
    match request.operation {
        CicsOperation::IssueWait => {
            if task.pending_destination.as_deref() != Some(&selected.name) {
                return Err(condition("INVREQ", 16, 0));
            }
            task.pending_destination = None;
        }
        CicsOperation::IssueAbort => {
            data.closed = true;
            task.selected = None;
            task.pending_destination = None;
            if task.query_destination.as_deref() == Some(&selected.name) {
                task.query_aborted = true;
            }
        }
        CicsOperation::IssueEnd => {
            data.closed = true;
            task.selected = None;
            task.pending_destination = None;
            if task.query_destination.as_deref() == Some(&selected.name) {
                task.query_records.clear();
                task.query_index = 0;
                task.query_destination = None;
            }
        }
        _ => return Err(HostProblem::InfrastructureFailure),
    }
    if request.operation != CicsOperation::IssueWait {
        commit(
            service,
            run,
            request,
            Some((&selected.name, &selected.definition, &data)),
            &task,
            &receipt,
        )
    } else {
        receipt.condition = "NORMAL".into();
        commit(service, run, request, None, &task, &receipt)
    }
}

fn receipt(run: &Run, request: &CicsRequest) -> Result<Receipt, HostProblem> {
    Ok(Receipt {
        owner: run.invocation.run_unit_id.as_str().into(),
        digest: canonical_request_digest(&HostRequest::Cics(request.clone()))
            .map_err(|_| HostProblem::ResourceExhausted)?,
        condition: "NORMAL".into(),
        response: 0,
        response2: 0,
        payload: Vec::new(),
        outputs: BTreeMap::new(),
    })
}

fn output(receipt: &mut Receipt, name: &str, schema: &str, bytes: Vec<u8>) {
    receipt.outputs.insert(
        name.into(),
        ReceiptOutput {
            schema: schema.into(),
            bytes,
        },
    );
}

fn receipt_response(
    service: &CicsService,
    run: &Run,
    receipt: &Receipt,
) -> Result<CicsResponse, HostProblem> {
    let mut response = service.response(
        run,
        CicsDisposition::Complete,
        &receipt.condition,
        receipt.response,
        receipt.response2,
        None,
        None,
        receipt.payload.clone(),
    )?;
    for (name, value) in &receipt.outputs {
        response.outputs.insert(
            name.clone(),
            mainframe_env_execution_api::BoundedPayload::new(
                &value.schema,
                value.bytes.clone(),
                InvocationLimits::default(),
            )
            .map_err(|_| HostProblem::InfrastructureFailure)?,
        );
    }
    Ok(response)
}

fn replay(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<Option<CicsResponse>, HostProblem> {
    let key = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?
        .idempotency_key
        .as_str();
    let Some(receipt) = state::read_receipt(service, key)? else {
        return Ok(None);
    };
    if receipt.owner != run.invocation.run_unit_id.as_str()
        || receipt.digest
            != canonical_request_digest(&HostRequest::Cics(request.clone()))
                .map_err(|_| HostProblem::ResourceExhausted)?
    {
        return Err(HostProblem::IdempotencyConflict);
    }
    Ok(Some(receipt_response(service, run, &receipt)?))
}

fn commit(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
    data: Option<(&str, &CicsOutboardDestinationDefinition, &DataState)>,
    task: &TaskState,
    receipt: &Receipt,
) -> Result<CicsResponse, HostProblem> {
    let key = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?
        .idempotency_key
        .as_str();
    let mut mutations = Vec::new();
    if let Some((name, definition, data)) = data {
        mutations.push(state::data_write(name, data, definition, service)?);
    }
    mutations.push(state::task_write(
        run.invocation.run_unit_id.as_str(),
        task,
        service,
    )?);
    mutations.push(state::receipt_write(key, receipt)?);
    match service.store.mutate_provider_states_atomic(mutations) {
        Ok(()) => receipt_response(service, run, receipt),
        Err(StoreError::Conflict | StoreError::AlreadyExists) => {
            replay(service, run, request)?.ok_or(HostProblem::IdempotencyConflict)
        }
        Err(error) => Err(mutation_problem(store_error(error))),
    }
}

fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}
