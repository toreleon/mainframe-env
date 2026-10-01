//! Container browse and inquiry through the owner-scoped command-data read port.

use super::*;
use crate::service::handlers::bts_container::{
    ContainerReadError, ContainerSelector, ReadReply, ReadRequest, valid_task_channel_name,
};

pub(super) const fn is_operation(operation: CicsOperation) -> bool {
    matches!(
        operation,
        CicsOperation::BtsStartBrowseContainer
            | CicsOperation::BtsGetNextContainer
            | CicsOperation::BtsInquireContainer
            | CicsOperation::BtsEndBrowseContainer
    )
}

fn read_error(error: ContainerReadError, channel: bool) -> HostProblem {
    match error {
        ContainerReadError::Unauthorized => condition("NOTAUTH", 70, 101),
        ContainerReadError::StaleEpoch => HostProblem::IdempotencyConflict,
        ContainerReadError::Bounds => HostProblem::ResourceExhausted,
        ContainerReadError::NotFound if channel => condition("CHANNELERR", 122, 2),
        ContainerReadError::NotFound => condition("ACTIVITYERR", 109, 2),
        ContainerReadError::Changed => HostProblem::IdempotencyConflict,
        ContainerReadError::Backend(problem) => problem,
    }
}

fn data_name(request: &CicsRequest, key: &str) -> Result<String, HostProblem> {
    let value = request.arguments.get(key).ok_or(HostProblem::Malformed)?;
    let text = std::str::from_utf8(value.bytes())
        .map_err(|_| HostProblem::Malformed)?
        .trim_end_matches(' ');
    if !valid_task_channel_name(text) {
        return Err(HostProblem::Malformed);
    }
    Ok(text.into())
}

fn selector_from_scope(scope: &BrowseScope) -> Result<ContainerSelector<'_>, HostProblem> {
    if scope.resource_class == "CICSCHAN" {
        return scope
            .resource_name
            .strip_prefix("CICS.CHANNEL.")
            .map(ContainerSelector::Channel)
            .ok_or(HostProblem::InfrastructureFailure);
    }
    let process_type = scope
        .process_type
        .as_deref()
        .ok_or(HostProblem::InfrastructureFailure)?;
    let process_name = scope
        .process_name
        .as_deref()
        .ok_or(HostProblem::InfrastructureFailure)?;
    Ok(if let Some(activity_id) = scope.activity_id.as_deref() {
        ContainerSelector::Activity {
            process_type,
            process_name,
            activity_id,
            epoch: scope.epoch,
        }
    } else {
        ContainerSelector::Process {
            process_type,
            process_name,
            epoch: scope.epoch,
        }
    })
}

fn selected_scope(
    run: &Run,
    request: &CicsRequest,
    authority: &BtsLifecycleStore<'_>,
    owner: &BrowseOwner,
) -> Result<BrowseScope, HostProblem> {
    let channel = if request.arguments.contains_key("CHANNEL") {
        Some(data_name(request, "CHANNEL")?)
    } else if request.operation == CicsOperation::BtsStartBrowseContainer
        && !request.arguments.contains_key("PROCESS")
        && !request.arguments.contains_key("ACTIVITYID")
    {
        run.current_program.channel.clone()
    } else {
        None
    };
    if let Some(channel) = channel {
        return BrowseScope::new(
            BrowseKind::Container,
            "CICSCHAN",
            &format!("CICS.CHANNEL.{channel}"),
            1,
        );
    }
    let held = authority
        .acquired_process_container_scope(&owner.run_unit, &owner.execution, &owner.principal)?
        .ok_or_else(|| condition("ACTIVITYERR", 109, 2))?;
    if request.arguments.contains_key("PROCESS") {
        if text_arg(request, "PROCESS", 36)? != held.process_name
            || text_arg(request, "PROCESSTYPE", 8)? != held.process_type
        {
            return Err(condition("PROCESSERR", 108, 3));
        }
    } else if request.arguments.contains_key("ACTIVITYID")
        && text_arg(request, "ACTIVITYID", 52)? != held.acquired_activity_id
    {
        return Err(condition("ACTIVITYERR", 109, 1));
    }
    let resource = BtsLifecycleStore::saf_resource(&held.process_type, &held.process_name)?;
    let scope = BrowseScope::new(
        BrowseKind::Container,
        "BTSLIFE",
        &resource,
        held.acquisition_epoch,
    )?
    .with_process(&held.process_type, &held.process_name)?;
    if request.arguments.contains_key("PROCESS") {
        Ok(scope)
    } else {
        scope.with_activity(&held.acquired_activity_id)
    }
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
        CicsOperation::BtsStartBrowseContainer => {
            let scope = selected_scope(run, request, authority, owner)?;
            let channel = scope.resource_class == "CICSCHAN";
            let ReadReply::Names(names) = service
                .read_bts_container(
                    run,
                    selector_from_scope(&scope)?,
                    ReadRequest::Names {
                        max: MAX_CURSOR_ITEMS,
                    },
                )
                .map_err(|error| read_error(error, channel))?
            else {
                return Err(HostProblem::InfrastructureFailure);
            };
            let mut names = names;
            names.sort();
            if names.windows(2).any(|pair| pair[0] == pair[1]) {
                return Err(HostProblem::InfrastructureFailure);
            }
            let items = names
                .iter()
                .map(|name| BrowseItem::container(name)?.with_epoch(scope.epoch))
                .collect::<Result<Vec<_>, _>>()?;
            let token = start(cursors, owner, request, scope, items)?;
            Ok(vec![(
                "BROWSETOKEN",
                "mainframe-env.cics.decimal@1",
                token.to_string().into_bytes(),
            )])
        }
        CicsOperation::BtsGetNextContainer => {
            let token = token_arg(request)?;
            let scope = cursors.scope(owner, token, BrowseKind::Container)?;
            let remaining = cursors.remaining(owner, token, BrowseKind::Container)?;
            let channel = scope.resource_class == "CICSCHAN";
            service.authorize(
                run,
                &scope.resource_class,
                &scope.resource_name,
                AccessIntent::Read,
            )?;
            let mut selected = None;
            for (index, item) in remaining.into_iter().enumerate() {
                let ReadReply::Exists(exists) = service
                    .read_bts_container(
                        run,
                        selector_from_scope(&scope)?,
                        ReadRequest::Exists(&item.name),
                    )
                    .map_err(|error| read_error(error, channel))?
                else {
                    return Err(HostProblem::InfrastructureFailure);
                };
                if exists {
                    selected = Some((index, item));
                    break;
                }
            }
            let (skipped, item) = selected.ok_or_else(end_of_browse)?;
            let outcome = effect(
                cursors,
                owner,
                request,
                BrowseEffect::NextSkipping {
                    token,
                    live_epoch: scope.epoch,
                    expected: item.clone(),
                    skipped,
                },
            )?;
            if outcome != BrowseOutcome::Item(item.clone()) {
                return Err(HostProblem::InfrastructureFailure);
            }
            Ok(vec![(
                "CONTAINER",
                "mainframe-env.cics.payload@1",
                padded(&item.name, 16)?,
            )])
        }
        CicsOperation::BtsInquireContainer => {
            let scope = selected_scope(run, request, authority, owner)?;
            let name = data_name(request, "CONTAINER")?;
            let channel = scope.resource_class == "CICSCHAN";
            let ReadReply::Value(value) = service
                .read_bts_container(run, selector_from_scope(&scope)?, ReadRequest::Value(&name))
                .map_err(|error| read_error(error, channel))?
            else {
                return Err(HostProblem::InfrastructureFailure);
            };
            let value = value.ok_or_else(|| condition("CONTAINERERR", 110, 1))?;
            let mut outputs = Vec::new();
            if has(request, "DATALENGTH") {
                outputs.push((
                    "DATALENGTH",
                    "mainframe-env.cics.decimal@1",
                    value.bytes.len().to_string().into_bytes(),
                ));
            }
            if has(request, "SET") {
                let capacity = request
                    .arguments
                    .get("SET.MAXLENGTH")
                    .and_then(|value| std::str::from_utf8(value.bytes()).ok())
                    .and_then(|value| value.parse::<usize>().ok())
                    .ok_or(HostProblem::Malformed)?;
                if value.bytes.len() > capacity {
                    return Err(HostProblem::ResourceExhausted);
                }
                outputs.push(("SET", "mainframe-env.cics.payload@1", value.bytes));
            }
            Ok(outputs)
        }
        CicsOperation::BtsEndBrowseContainer => {
            let token = token_arg(request)?;
            let scope = cursors.scope(owner, token, BrowseKind::Container)?;
            service.authorize(
                run,
                &scope.resource_class,
                &scope.resource_name,
                AccessIntent::Read,
            )?;
            if effect(
                cursors,
                owner,
                request,
                BrowseEffect::End {
                    token,
                    kind: BrowseKind::Container,
                },
            )? != BrowseOutcome::Ended
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            Ok(Vec::new())
        }
        _ => Err(HostProblem::Unsupported),
    }
}
