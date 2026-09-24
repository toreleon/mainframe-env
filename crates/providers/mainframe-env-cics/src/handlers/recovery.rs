use super::super::{
    CicsService, DatasetUndo, Run, UowRecord, decode_uow, encode_uow,
    invocation_with_nested_origin, nested_key, nested_mutation, store_error,
};
use crate::retention::UowRetentionMetadata;
use mainframe_env_execution_api::{CapabilityId, InvocationLimits};
use mainframe_env_host_api::{
    CicsDisposition, CicsOperation, CicsRequest, CicsResponse, CicsUnitOfWorkOutcome,
    DatasetRequest, DatasetResult, Db2Operation, Db2Request, EffectRequest, HostProblem,
    HostRequest, HostResult, ImsOperation, ImsRequest, MqOperation, MqRequest, Mutation,
};
use mainframe_env_store_api::ProviderStateRecord;
use std::collections::BTreeMap;

const EXECUTION_CONTEXT_BINDING: &str = "cics.execution-context";
const EXECUTION_CONTEXT_SCHEMA: &str = "mainframe-env.cics.execution-context@1";
const REMOTE_OUTCOME_BINDING: &str = "cics.syncpoint.remote-outcome";
const REMOTE_OUTCOME_SCHEMA: &str = "mainframe-env.cics.syncpoint.remote-outcome@1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SyncpointOwner {
    Local,
    DplSynconreturn,
}

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    if request.operation != CicsOperation::Syncpoint {
        return Err(HostProblem::InfrastructureFailure);
    }
    syncpoint(service, run, request, retention_tick)
}

fn syncpoint(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    let owner = validate_syncpoint_owner(run)?;
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let requested_outcome = if request.arguments.contains_key("OPTION.ROLLBACK") {
        CicsUnitOfWorkOutcome::RolledBack
    } else {
        CicsUnitOfWorkOutcome::Committed
    };
    let remote_forced_rollback = remote_forced_rollback(run, owner, requested_outcome)?;
    let outcome = if remote_forced_rollback {
        CicsUnitOfWorkOutcome::RolledBack
    } else {
        requested_outcome
    };
    let key = mutation.idempotency_key.as_str();
    if retention_tick == 0 {
        return Err(HostProblem::Malformed);
    }
    let existing_deadline = if let Some(record) = service
        .store
        .get_provider_state("cics-uow", key)
        .map_err(store_error)?
    {
        let existing = decode_uow(&record.payload)?;
        if existing.transaction != run.transaction {
            return Err(HostProblem::IdempotencyConflict);
        }
        if existing.metadata.as_ref().is_some_and(|metadata| {
            metadata.owner_execution != run.invocation.execution_id.as_str()
                || metadata.owner_run_unit != run.invocation.run_unit_id.as_str()
        }) {
            return Err(HostProblem::IdempotencyConflict);
        }
        match (existing.finalized, existing.outcome) {
            (false, existing_outcome) if existing_outcome == outcome => {
                if record.version != 1 {
                    return Err(HostProblem::InfrastructureFailure);
                }
                if existing.metadata.is_none() {
                    return Err(HostProblem::UnknownOutcome);
                }
                Some(
                    existing
                        .metadata
                        .as_ref()
                        .map(|metadata| metadata.deadline_tick)
                        .unwrap_or(retention_tick),
                )
            }
            (true, existing_outcome) if existing_outcome == outcome => {
                super::interval_control::finish_protected_starts(service, run, outcome)?;
                return syncpoint_response(service, run, outcome, remote_forced_rollback);
            }
            _ => return Err(HostProblem::IdempotencyConflict),
        }
    } else {
        if outcome == CicsUnitOfWorkOutcome::Committed {
            super::bts_lifecycle::BtsLifecycleStore::new(service.store.as_ref())
                .preflight_pending_definition(
                    run.invocation.run_unit_id.as_str(),
                    run.invocation.execution_id.as_str(),
                    run.invocation.principal.id().as_str(),
                )?;
        }
        service
            .store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "cics-uow".into(),
                    key: key.into(),
                    version: 1,
                    payload: encode_uow(&UowRecord {
                        finalized: false,
                        outcome,
                        transaction: run.transaction.clone(),
                        metadata: Some(UowRetentionMetadata {
                            effect_key: key.into(),
                            owner_execution: run.invocation.execution_id.as_str().into(),
                            owner_run_unit: run.invocation.run_unit_id.as_str().into(),
                            deadline_tick: retention_tick,
                            terminal_tick: None,
                        }),
                    })?,
                },
                None,
            )
            .map_err(store_error)?;
        None
    };
    let uow_deadline = existing_deadline.unwrap_or(retention_tick);
    syncpoint_db2(service, run, outcome)?;
    syncpoint_ims(service, run, outcome)?;
    syncpoint_mq(service, run, outcome)?;
    super::bts_lifecycle::BtsLifecycleStore::new(service.store.as_ref()).finish_run_uow(
        run.invocation.run_unit_id.as_str(),
        run.invocation.execution_id.as_str(),
        run.invocation.principal.id().as_str(),
        outcome == CicsUnitOfWorkOutcome::Committed,
    )?;
    super::release_uow_enqueues(service, run)?;
    if outcome == CicsUnitOfWorkOutcome::RolledBack {
        rollback_run(service, run)?;
    } else {
        service.clear_undo(run)?;
    }
    // A syncpoint ends every no-token file update context regardless of
    // whether the unit of work commits or rolls back.
    run.current_records.clear();
    run.file_updates.current_record_values.clear();
    run.file_updates.file_tokens.clear();
    let terminal_tick = match &service.replay_clock {
        Some(clock) => match clock.now_tick() {
            Ok(tick) if tick != 0 => Some(tick.max(uow_deadline)),
            _ => return Err(HostProblem::UnknownOutcome),
        },
        None => None,
    };
    if service
        .store
        .put_provider_state(
            ProviderStateRecord {
                namespace: "cics-uow".into(),
                key: key.into(),
                version: 2,
                payload: encode_uow(&UowRecord {
                    finalized: true,
                    outcome,
                    transaction: run.transaction.clone(),
                    metadata: Some(UowRetentionMetadata {
                        effect_key: key.into(),
                        owner_execution: run.invocation.execution_id.as_str().into(),
                        owner_run_unit: run.invocation.run_unit_id.as_str().into(),
                        deadline_tick: uow_deadline,
                        terminal_tick,
                    }),
                })?,
            },
            Some(1),
        )
        .is_err()
    {
        return Err(HostProblem::UnknownOutcome);
    }
    super::interval_control::finish_protected_starts(service, run, outcome)?;
    syncpoint_response(service, run, outcome, remote_forced_rollback)
}

fn validate_syncpoint_owner(run: &Run) -> Result<SyncpointOwner, HostProblem> {
    let Some(context) = run.invocation.bindings.get(EXECUTION_CONTEXT_BINDING) else {
        return Ok(SyncpointOwner::Local);
    };
    if context.schema() != EXECUTION_CONTEXT_SCHEMA {
        return Err(HostProblem::Malformed);
    }
    match context.bytes() {
        b"local" => Ok(SyncpointOwner::Local),
        b"dpl-synconreturn" => Ok(SyncpointOwner::DplSynconreturn),
        // IBM topic dfhp4_syncpoint.html assigns INVREQ RESP2 200 when a DPL
        // server does not own the syncpoint or is constrained to DPLSUBSET.
        b"dpl-without-synconreturn" | b"dpl-executionset-subset" => Err(HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2: 200,
        }),
        _ => Err(HostProblem::Malformed),
    }
}

fn remote_forced_rollback(
    run: &Run,
    owner: SyncpointOwner,
    requested_outcome: CicsUnitOfWorkOutcome,
) -> Result<bool, HostProblem> {
    let Some(remote_outcome) = run.invocation.bindings.get(REMOTE_OUTCOME_BINDING) else {
        return Ok(false);
    };
    if owner != SyncpointOwner::DplSynconreturn || remote_outcome.schema() != REMOTE_OUTCOME_SCHEMA
    {
        return Err(HostProblem::Malformed);
    }
    match remote_outcome.bytes() {
        b"commit-capable" => Ok(false),
        b"unable-to-commit" => Ok(requested_outcome == CicsUnitOfWorkOutcome::Committed),
        _ => Err(HostProblem::Malformed),
    }
}

fn syncpoint_db2(
    service: &CicsService,
    run: &mut Run,
    outcome: CicsUnitOfWorkOutcome,
) -> Result<(), HostProblem> {
    let capability = CapabilityId::new("host.db2.write", InvocationLimits::default())
        .expect("static Db2 capability");
    if !service.host.capability_ready(capability.as_str())
        || !run.invocation.principal.has_grant(&capability)
    {
        return Ok(());
    }
    run.host_sequence = run
        .host_sequence
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    let key = nested_key(run, run.host_sequence)?;
    let nested_invocation = invocation_with_nested_origin(
        &run.invocation,
        &key,
        run.outer_effect_key
            .as_deref()
            .ok_or(HostProblem::InfrastructureFailure)?,
    )?;
    let result = service.invoke_host(
        &nested_invocation,
        run.invocation.deadline_tick.saturating_sub(1),
        false,
        EffectRequest {
            run_unit: run.invocation.run_unit_id.clone(),
            sequence: run.host_sequence,
            deadline_tick: run.invocation.deadline_tick,
            idempotency_key: Some(key.clone()),
            request: HostRequest::Db2(Db2Request {
                operation: if outcome == CicsUnitOfWorkOutcome::RolledBack {
                    Db2Operation::Rollback
                } else {
                    Db2Operation::Commit
                },
                statement: String::new(),
                cursor: None,
                inputs: BTreeMap::new(),
                outputs: Vec::new(),
                max_rows: 0,
                mutation: Some(Mutation {
                    sequence: run.host_sequence,
                    idempotency_key: key,
                    transaction: Some(run.transaction.clone()),
                }),
            }),
        },
    );
    match result.outcome? {
        HostResult::Db2(result) if result.sqlcode == 0 => Ok(()),
        HostResult::Db2(result) => Err(HostProblem::Condition {
            name: format!("SQLCODE{}", result.sqlcode),
            response: result.sqlcode,
            response2: 0,
        }),
        _ => Err(HostProblem::ProviderFailure),
    }
}

fn syncpoint_ims(
    service: &CicsService,
    run: &mut Run,
    outcome: CicsUnitOfWorkOutcome,
) -> Result<(), HostProblem> {
    let capability = CapabilityId::new("host.ims.write", InvocationLimits::default())
        .expect("static IMS capability");
    if !service.host.capability_ready(capability.as_str())
        || !run.invocation.principal.has_grant(&capability)
    {
        return Ok(());
    }
    run.host_sequence = run
        .host_sequence
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    let key = nested_key(run, run.host_sequence)?;
    let nested_invocation = invocation_with_nested_origin(
        &run.invocation,
        &key,
        run.outer_effect_key
            .as_deref()
            .ok_or(HostProblem::InfrastructureFailure)?,
    )?;
    let result = service.invoke_host(
        &nested_invocation,
        run.invocation.deadline_tick.saturating_sub(1),
        false,
        EffectRequest {
            run_unit: run.invocation.run_unit_id.clone(),
            sequence: run.host_sequence,
            deadline_tick: run.invocation.deadline_tick,
            idempotency_key: Some(key.clone()),
            request: HostRequest::Ims(ImsRequest {
                operation: if outcome == CicsUnitOfWorkOutcome::RolledBack {
                    ImsOperation::Rollback
                } else {
                    ImsOperation::Commit
                },
                psb: None,
                pcb: 1,
                segments: Vec::new(),
                data: Vec::new(),
                qualifiers: Vec::new(),
                checkpoint_id: None,
                max_segments: 1,
                mutation: Some(Mutation {
                    sequence: run.host_sequence,
                    idempotency_key: key,
                    transaction: Some(run.transaction.clone()),
                }),
            }),
        },
    );
    match result.outcome? {
        HostResult::Ims(result) if result.status.trim().is_empty() => Ok(()),
        HostResult::Ims(result) => Err(HostProblem::Condition {
            name: format!("IMS{}", result.status.trim()),
            response: 1,
            response2: 0,
        }),
        _ => Err(HostProblem::ProviderFailure),
    }
}

fn syncpoint_mq(
    service: &CicsService,
    run: &mut Run,
    outcome: CicsUnitOfWorkOutcome,
) -> Result<(), HostProblem> {
    let capability = CapabilityId::new("host.mq.write", InvocationLimits::default())
        .expect("static MQ capability");
    if !service.host.capability_ready(capability.as_str())
        || !run.invocation.principal.has_grant(&capability)
    {
        return Ok(());
    }
    run.host_sequence = run
        .host_sequence
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    let key = nested_key(run, run.host_sequence)?;
    let nested_invocation = invocation_with_nested_origin(
        &run.invocation,
        &key,
        run.outer_effect_key
            .as_deref()
            .ok_or(HostProblem::InfrastructureFailure)?,
    )?;
    let result = service.invoke_host(
        &nested_invocation,
        run.invocation.deadline_tick.saturating_sub(1),
        false,
        EffectRequest {
            run_unit: run.invocation.run_unit_id.clone(),
            sequence: run.host_sequence,
            deadline_tick: run.invocation.deadline_tick,
            idempotency_key: Some(key.clone()),
            request: HostRequest::Mq(MqRequest {
                operation: if outcome == CicsUnitOfWorkOutcome::RolledBack {
                    MqOperation::Rollback
                } else {
                    MqOperation::Commit
                },
                queue: None,
                handle: None,
                options: 0,
                message: Vec::new(),
                message_id: None,
                correlation_id: None,
                wait_ticks: 0,
                max_message_bytes: 1,
                mutation: Some(Mutation {
                    sequence: run.host_sequence,
                    idempotency_key: key,
                    transaction: Some(run.transaction.clone()),
                }),
            }),
        },
    );
    match result.outcome? {
        HostResult::Mq(result) if result.completion_code == 0 => Ok(()),
        HostResult::Mq(result) => Err(HostProblem::Condition {
            name: format!("MQRC{}", result.reason_code),
            response: result.completion_code,
            response2: result.reason_code,
        }),
        _ => Err(HostProblem::ProviderFailure),
    }
}

fn rollback_run(service: &CicsService, run: &mut Run) -> Result<(), HostProblem> {
    let undo = run.undo.clone();
    for operation in undo.into_iter().rev() {
        let sequence = run
            .host_sequence
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        let mutation = nested_mutation(run, sequence)?;
        let request = match operation {
            DatasetUndo::Restore {
                dataset,
                key,
                record,
            } => DatasetRequest::RewriteRecord {
                dataset,
                key,
                record,
                expected_version: None,
                mutation,
            },
            DatasetUndo::Delete { dataset, key } => DatasetRequest::DeleteRecord {
                dataset,
                key,
                expected_version: None,
                mutation,
            },
        };
        match service.nested(run, HostRequest::Dataset(request))? {
            HostResult::Dataset(DatasetResult::Mutated { .. }) => {}
            _ => return Err(HostProblem::ProviderFailure),
        }
    }
    run.current_records.clear();
    run.file_updates.current_record_values.clear();
    run.file_updates.file_tokens.clear();
    service.clear_undo(run)
}

fn uow_response(
    service: &CicsService,
    run: &Run,
    outcome: CicsUnitOfWorkOutcome,
) -> Result<CicsResponse, HostProblem> {
    let mut response = service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )?;
    response.unit_of_work = Some(outcome);
    Ok(response)
}

fn syncpoint_response(
    service: &CicsService,
    run: &Run,
    outcome: CicsUnitOfWorkOutcome,
    remote_forced_rollback: bool,
) -> Result<CicsResponse, HostProblem> {
    if remote_forced_rollback {
        Err(HostProblem::Condition {
            name: "ROLLEDBACK".into(),
            response: 82,
            response2: 0,
        })
    } else {
        uow_response(service, run, outcome)
    }
}
