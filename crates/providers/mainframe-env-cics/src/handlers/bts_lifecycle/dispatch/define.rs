//! DEFINE PROCESS and DEFINE ACTIVITY with installed catalog and SAF checks.

use super::super::children::BtsChildDefinition;
use super::*;
use mainframe_env_host_api::{HostRequest, canonical_request_digest};
use std::collections::BTreeMap;

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    match request.operation {
        CicsOperation::DefineProcess => define_process(service, run, request),
        CicsOperation::DefineActivity => define_activity(service, run, request),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

fn define_process(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let process_type = argument_name(request, "PROCESSTYPE", 8)?;
    let process_name =
        argument_name(request, "PROCESS", 36).map_err(|_| condition("PROCESSERR", 108, 16))?;
    let authority = BtsLifecycleStore::new(service.store.as_ref());
    let definition = authority
        .load_process_type(&process_type)?
        .ok_or_else(|| condition("PROCESSERR", 108, 9))?;
    if !definition.enabled {
        return Err(condition("INVREQ", 16, 12));
    }
    let transaction = transaction(&authority, request)?;
    let program = request
        .arguments
        .contains_key("PROGRAM")
        .then(|| argument_name(request, "PROGRAM", 8))
        .transpose()?
        .unwrap_or(transaction.program);
    let user = execution_user(service, run, request)?;
    authorize_definition(
        service,
        run,
        &definition.repository_resource,
        request,
        &user,
    )?;
    let run_unit = run.invocation.run_unit_id.as_str();
    let root = BtsLifecycleStore::root_id(&process_type, &process_name, run_unit)?;
    let process = BtsProcess::new(
        &process_type,
        &process_name,
        &root,
        &program,
        &transaction.transid,
        &user,
        run_unit,
    )?;
    let (effect_key, digest) = effect(request)?;
    let define = if request.arguments.contains_key("OPTION.NOCHECK") {
        authority.define_process_nocheck_exact(
            process,
            &definition.repository_resource,
            run_unit,
            run.invocation.execution_id.as_str(),
            run.invocation.principal.id().as_str(),
            effect_key,
            digest,
        )
    } else {
        authority.define_process_in_repository_exact(
            process,
            &definition.repository_resource,
            run_unit,
            run.invocation.execution_id.as_str(),
            run.invocation.principal.id().as_str(),
            effect_key,
            digest,
        )
    };
    define.map_err(|problem| match problem {
        HostProblem::NotFound => condition("PROCESSERR", 108, 9),
        other => other,
    })?;
    response(service, run, BTreeMap::new())
}

fn define_activity(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let context = active_context(service, run)?;
    let name = argument_name(request, "ACTIVITY", 16).map_err(|_| condition("INVREQ", 16, 17))?;
    let event = if request.arguments.contains_key("EVENT") {
        argument_name(request, "EVENT", 16).map_err(|_| condition("INVREQ", 16, 17))?
    } else {
        name.clone()
    };
    if super::super::super::event_control::event_name(&event).is_err() {
        return Err(condition("INVREQ", 16, 17));
    }
    let authority = BtsLifecycleStore::new(service.store.as_ref());
    let process_type = authority
        .load_process_type(&context.process_type)?
        .ok_or_else(|| condition("PROCESSERR", 108, 9))?;
    let transaction = transaction(&authority, request)?;
    let program = request
        .arguments
        .contains_key("PROGRAM")
        .then(|| argument_name(request, "PROGRAM", 8))
        .transpose()?
        .unwrap_or(transaction.program);
    let user = execution_user(service, run, request)?;
    authorize_definition(
        service,
        run,
        &process_type.repository_resource,
        request,
        &user,
    )?;
    let pool = super::super::super::event_control::load_activity(service, &context.activity_id)?;
    if pool.events.contains_key(&event) {
        return Err(condition("EVENTERR", 111, 7));
    }
    let child = BtsChildDefinition {
        name,
        completion_event: event,
        program,
        transid: transaction.transid,
        userid: user,
    };
    let (effect_key, digest) = effect(request)?;
    let id = authority.define_child(
        &context.process_type,
        &context.process_name,
        &context.activity_id,
        &child,
        context.run_unit.as_str(),
        context.owner_execution.as_str(),
        context.owner_principal.as_str(),
        effect_key,
        digest,
    )?;
    let outputs = if request.arguments.contains_key("ACTIVITYID") {
        BTreeMap::from([(
            "ACTIVITYID".into(),
            ("mainframe-env.cics.payload@1", id.into_bytes()),
        )])
    } else {
        BTreeMap::new()
    };
    response(service, run, outputs)
}

fn transaction(
    authority: &BtsLifecycleStore<'_>,
    request: &CicsRequest,
) -> Result<BtsTransactionDefinition, HostProblem> {
    let transid =
        argument_name(request, "TRANSID", 4).map_err(|_| condition("TRANSIDERR", 28, 0))?;
    authority
        .load_transaction(&transid)?
        .ok_or_else(|| condition("TRANSIDERR", 28, 0))
}

fn execution_user(
    _service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<String, HostProblem> {
    if request.arguments.contains_key("USERID") {
        argument_name(request, "USERID", 8)
    } else {
        Ok(run.invocation.principal.id().as_str().into())
    }
}

fn authorize_definition(
    service: &CicsService,
    run: &mut Run,
    repository: &str,
    request: &CicsRequest,
    user: &str,
) -> Result<(), HostProblem> {
    service
        .authorize(run, "BTSREPO", repository, AccessIntent::Update)
        .map_err(|problem| match problem {
            HostProblem::Unauthorized => condition("NOTAUTH", 70, 101),
            other => other,
        })?;
    if request.arguments.contains_key("USERID") {
        service
            .authorize(
                run,
                "SURROGAT",
                &format!("{user}.DFHSTART"),
                AccessIntent::Read,
            )
            .map_err(|problem| match problem {
                HostProblem::Unauthorized => condition("NOTAUTH", 70, 102),
                other => other,
            })?;
    }
    Ok(())
}

fn effect(request: &CicsRequest) -> Result<(&str, [u8; 32]), HostProblem> {
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    Ok((mutation.idempotency_key.as_str(), digest))
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let (required, allowed): (&[&str], &[&str]) = match request.operation {
        CicsOperation::DefineProcess => (
            &["PROCESS", "PROCESSTYPE", "TRANSID"],
            &[
                "PROCESS",
                "PROCESSTYPE",
                "TRANSID",
                "PROGRAM",
                "USERID",
                "OPTION.NOCHECK",
            ],
        ),
        CicsOperation::DefineActivity => (
            &["ACTIVITY", "TRANSID"],
            &[
                "ACTIVITY",
                "EVENT",
                "TRANSID",
                "PROGRAM",
                "USERID",
                "ACTIVITYID",
            ],
        ),
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    if required
        .iter()
        .any(|name| !request.arguments.contains_key(*name))
    {
        return Err(HostProblem::Malformed);
    }
    for (name, value) in &request.arguments {
        if name.starts_with("OPTION.") {
            if !matches!(name.as_str(), "OPTION.NOHANDLE") && !allowed.contains(&name.as_str())
                || value.schema() != "mainframe-env.cics.option@1"
                || !value.bytes().is_empty()
            {
                return Err(HostProblem::Malformed);
            }
        } else if matches!(name.as_str(), "RESP" | "RESP2")
            || name == "ACTIVITYID" && request.operation == CicsOperation::DefineActivity
        {
            if value.schema() != "mainframe-env.cics.argument@1" {
                return Err(HostProblem::Malformed);
            }
        } else if allowed.contains(&name.as_str()) {
            if !matches!(
                value.schema(),
                "mainframe-env.cics.argument@1"
                    | "mainframe-env.cics.literal@1"
                    | "mainframe-env.cics.storage-value@1"
            ) {
                return Err(HostProblem::Malformed);
            }
        } else {
            return Err(HostProblem::Malformed);
        }
    }
    Ok(())
}
