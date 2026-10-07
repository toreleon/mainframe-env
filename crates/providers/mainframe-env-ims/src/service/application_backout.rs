//! Selected DB-batch application backout. RecoverySession owns points/replay;
//! actual images and witnessed local undo remain the generic database authority.
use super::application_recovery::{recovery_address, recovery_error, refresh_system};
use super::*;
use crate::recovery::{
    APPLICATION_RESULT_DOMAIN, BackoutPointKind, RecoveryLimits, RecoverySession,
};
use mainframe_env_host_api::{
    ImsDatabaseOrganization, ImsPcbMetadata, ImsRecoveryCall, ImsRecoveryRequest, ImsRecoveryResult,
};
use mainframe_env_store_api::IdempotencyStore;

const RECEIPT_KIND: &[u8] = b"backout@1\0";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    kind: String,
    status: String,
    data: Vec<u8>,
    code: String,
}

pub(super) fn is_backout(call: &ImsRecoveryCall) -> bool {
    matches!(
        call,
        ImsRecoveryCall::Sets { .. }
            | ImsRecoveryCall::Setu { .. }
            | ImsRecoveryCall::Rols { .. }
            | ImsRecoveryCall::Rolb
            | ImsRecoveryCall::Roll
    )
}

pub(super) fn is_receipt(bytes: &[u8]) -> bool {
    bytes
        .strip_prefix(APPLICATION_RESULT_DOMAIN)
        .is_some_and(|bytes| bytes.starts_with(RECEIPT_KIND))
}

fn encode_result(result: &ImsRecoveryResult) -> Result<Vec<u8>, HostProblem> {
    let mut receipt = Receipt {
        kind: String::new(),
        status: String::new(),
        data: vec![],
        code: String::new(),
    };
    match result {
        ImsRecoveryResult::Savepoint { status } => {
            receipt.kind = "point".into();
            receipt.status = status.clone();
        }
        ImsRecoveryResult::BackedOut { status, user_data } => {
            receipt.kind = "backout".into();
            receipt.status = status.clone();
            receipt.data = user_data.clone();
        }
        ImsRecoveryResult::Abended { code } => {
            receipt.kind = "abend".into();
            receipt.code = code.clone();
        }
        _ => return Err(HostProblem::Malformed),
    }
    let mut bytes = APPLICATION_RESULT_DOMAIN.to_vec();
    bytes.extend(RECEIPT_KIND);
    bytes.extend(serde_json::to_vec(&receipt).map_err(|_| HostProblem::InfrastructureFailure)?);
    Ok(bytes)
}

pub(super) fn decode_result(bytes: &[u8]) -> Result<ImsRecoveryResult, HostProblem> {
    let receipt: Receipt = serde_json::from_slice(
        bytes
            .strip_prefix(APPLICATION_RESULT_DOMAIN)
            .and_then(|bytes| bytes.strip_prefix(RECEIPT_KIND))
            .ok_or(HostProblem::ProviderFailure)?,
    )
    .map_err(|_| HostProblem::ProviderFailure)?;
    let result = match receipt.kind.as_str() {
        "point" if receipt.data.is_empty() && receipt.code.is_empty() => {
            ImsRecoveryResult::Savepoint {
                status: receipt.status,
            }
        }
        "backout" if receipt.code.is_empty() => ImsRecoveryResult::BackedOut {
            status: receipt.status,
            user_data: receipt.data,
        },
        "abend" if receipt.data.is_empty() && receipt.status.is_empty() => {
            ImsRecoveryResult::Abended { code: receipt.code }
        }
        _ => return Err(HostProblem::ProviderFailure),
    };
    result
        .validate()
        .map_err(|_| HostProblem::ProviderFailure)?;
    Ok(result)
}

pub(super) fn ensure_active(state: &State, run: &str) -> Result<(), HostProblem> {
    if state
        .sessions
        .get(run)
        .is_some_and(|s| s.recovery.terminated.is_some())
    {
        Err(HostProblem::Unsupported)
    } else {
        Ok(())
    }
}

pub(super) fn prepare_database_call(
    state: &State,
    run: &str,
    request: &ImsRequest,
) -> Result<(), HostProblem> {
    ensure_active(state, run)?;
    if request.operation == ImsOperation::Checkpoint
        && state.sessions.get(run).is_some_and(|s| s.generic)
    {
        generic::isolation::ensure_backout(state, run)?;
    }
    Ok(())
}

/// Preserve legacy immediate Batch commits; real generic DB work stays in its
/// existing local UOW until a commit/checkpoint/backout boundary.
pub(super) fn settle_database_call(
    state: &mut State,
    invocation: &Invocation,
    request: &ImsRequest,
    result: &ImsResult,
) -> Result<(), HostProblem> {
    let run = invocation.run_unit_id.as_str();
    let generic = state.sessions.get(run).is_some_and(|s| s.generic);
    if generic && result.status == "  " && request.operation == ImsOperation::Schedule {
        Arc::make_mut(state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?)
            .recovery
            .uow_incarnation = request
            .mutation
            .as_ref()
            .ok_or(HostProblem::MissingIdempotency)?
            .idempotency_key
            .to_string();
    }
    if generic
        && result.status == "  "
        && matches!(
            request.operation,
            ImsOperation::Commit | ImsOperation::Rollback | ImsOperation::Checkpoint
        )
    {
        let marker =
            &mut Arc::make_mut(state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?).recovery;
        marker.uow_epoch = marker
            .uow_epoch
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
    }
    if !generic
        && invocation.service_class == ServiceClass::Batch
        && matches!(
            request.operation,
            ImsOperation::Insert | ImsOperation::Replace | ImsOperation::Delete
        )
    {
        state.pending_undo.remove(run);
        state.generic_pending_undo.remove(run);
    }
    Ok(())
}

fn scope(state: &State, psb: &str) -> Result<BTreeSet<String>, HostProblem> {
    let psb = state
        .metadata
        .as_ref()
        .and_then(|catalog| catalog.psbs.iter().find(|item| item.name == psb))
        .ok_or(HostProblem::NotFound)?;
    let mut names = BTreeSet::new();
    for pcb in &psb.pcbs {
        if let ImsPcbMetadata::Database(pcb) = pcb {
            names.extend(generic::isolation::dependencies(state, &pcb.database)?);
        }
    }
    Ok(names)
}

pub(super) fn ensure_scope(state: &State, run: &str, psb: &str) -> Result<(), HostProblem> {
    let names = scope(state, psb)?;
    if state.pending_undo.contains_key(run)
        || state
            .generic_pending_undo
            .get(run)
            .is_some_and(|undo| undo.keys().any(|name| !names.contains(name)))
    {
        return Err(HostProblem::Unsupported);
    }
    Ok(())
}

impl ImsService {
    pub(super) fn authorize_backout_scope(
        &self,
        invocation: &Invocation,
        selected: &crate::ImsMetadataGeneration,
        psb: &str,
    ) -> Result<(), HostProblem> {
        let authorizer = self.authorizer.as_ref().ok_or(HostProblem::Unauthorized)?;
        let state = State {
            metadata: Some(selected.catalog.clone()),
            ..State::default()
        };
        for name in scope(&state, psb)? {
            authorizer.authorize(
                invocation.principal.id(),
                &EnterpriseResource::new(
                    EnterpriseResourceClass::ImsDatabase,
                    name,
                    AccessIntent::Update,
                )?,
            )?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn application_backout(
        &self,
        invocation: &Invocation,
        request: &ImsRecoveryRequest,
        effects: &dyn IdempotencyStore,
        digest: [u8; 32],
        selected: &crate::ImsMetadataGeneration,
        recovery: RecoverySession,
        effect_id: &str,
    ) -> Result<ImsRecoveryResult, HostProblem> {
        self.authorize_backout_scope(invocation, selected, &request.psb)?;
        if let Some((_, bytes)) = recovery
            .application_replay(effect_id, digest)
            .map_err(recovery_error)?
        {
            return decode_result(&bytes);
        }
        let mut durable = self.lock()?;
        let recovery = RecoverySession::load(
            &*self.store,
            &recovery_address(invocation, request, &selected.application),
            RecoveryLimits::default(),
        )
        .map_err(recovery_error)?;
        if let Some((_, bytes)) = recovery
            .application_replay(effect_id, digest)
            .map_err(recovery_error)?
        {
            return decode_result(&bytes);
        }
        generic::refresh_databases(&*self.store, self.limits, &mut durable)?;
        refresh_system(&*self.store, self.limits, &mut durable)?;
        let run = invocation.run_unit_id.as_str();
        ensure_active(&durable.state, run)?;
        if crate::tm::has_session(&*self.store, run, self.limits.max_state_bytes)? {
            return Err(HostProblem::Unsupported);
        }
        if durable.state.metadata.as_ref() != Some(&selected.catalog) {
            return Err(HostProblem::IdempotencyConflict);
        }
        let session = durable
            .state
            .sessions
            .get(run)
            .ok_or(HostProblem::NotFound)?;
        if !session.generic || session.psb != request.psb {
            return Err(HostProblem::Malformed);
        }
        ensure_scope(&durable.state, run, &request.psb)?;
        generic::isolation::ensure_backout(&durable.state, run)?;
        let epoch = crate::recovery::ApplicationEpoch {
            incarnation: if session.recovery.uow_incarnation.is_empty() {
                // Migrate a live historical Session using its real CAS generation;
                // this value is persisted with the point in the same atomic batch.
                format!(
                    "legacy:{}",
                    durable
                        .versions
                        .get(&(SESSION_NAMESPACE.into(), run.into()))
                        .ok_or(HostProblem::NotFound)?
                )
            } else {
                session.recovery.uow_incarnation.clone()
            },
            sequence: session.recovery.uow_epoch,
        };
        let all_names = scope(&durable.state, &request.psb)?;
        let names = all_names
            .iter()
            .filter(|name| {
                selected
                    .catalog
                    .databases
                    .iter()
                    .find(|db| &db.name == *name)
                    .is_some_and(|db| {
                        !matches!(
                            db.organization,
                            ImsDatabaseOrganization::Dedb
                                | ImsDatabaseOrganization::Msdb
                                | ImsDatabaseOrganization::Gsam
                        )
                    })
            })
            .cloned()
            .collect::<BTreeSet<_>>();
        let unsupported = names != all_names;
        let mut next = durable.state.scoped_snapshot();
        Arc::make_mut(next.sessions.get_mut(run).ok_or(HostProblem::NotFound)?)
            .recovery
            .uow_incarnation = epoch.incarnation.clone();
        let (transition, result) = match &request.call {
            ImsRecoveryCall::Sets { token, user_data }
            | ImsRecoveryCall::Setu { token, user_data } => {
                let kind = if matches!(request.call, ImsRecoveryCall::Sets { .. }) {
                    BackoutPointKind::Sets
                } else {
                    BackoutPointKind::Setu
                };
                if unsupported && kind == BackoutPointKind::Sets {
                    (
                        recovery
                            .application_condition(effect_id)
                            .map_err(recovery_error)?,
                        ImsRecoveryResult::Savepoint {
                            status: "SC".into(),
                        },
                    )
                } else if token.is_some_and(|token| {
                    recovery.application_point_count(&epoch)
                        >= RecoveryLimits::default().max_backout_points
                        && recovery
                            .application_point(token, &epoch)
                            .is_ok_and(|point| point.is_none())
                }) {
                    (
                        recovery
                            .application_condition(effect_id)
                            .map_err(recovery_error)?,
                        ImsRecoveryResult::Savepoint {
                            status: "SB".into(),
                        },
                    )
                } else {
                    let mut rows = vec![];
                    if token.is_some() {
                        for name in &names {
                            // Reserve the current image with the existing UOW witness,
                            // so a later foreign UOW cannot slip behind this point.
                            let image = next
                                .generic_databases
                                .get(name)
                                .ok_or(HostProblem::NotFound)?
                                .clone();
                            generic::isolation::publish_image(
                                &mut next,
                                run,
                                name,
                                (*image).clone(),
                                self.limits,
                            )?;
                            rows.push(ProviderStateRecord {
                                namespace: GENERIC_DATABASE_NAMESPACE.into(),
                                key: name.clone(),
                                version: *durable
                                    .versions
                                    .get(&(GENERIC_DATABASE_NAMESPACE.into(), name.clone()))
                                    .ok_or(HostProblem::NotFound)?,
                                payload: encode_object_row(name, &image)?,
                            });
                        }
                    }
                    match recovery.application_set_point(
                        effect_id,
                        &epoch,
                        kind,
                        *token,
                        user_data.clone().unwrap_or_default(),
                        rows,
                    ) {
                        Ok(transition) => (
                            transition,
                            ImsRecoveryResult::Savepoint {
                                status: if unsupported { "SC" } else { "  " }.into(),
                            },
                        ),
                        Err(crate::recovery::RecoveryProblem::LimitExceeded) => {
                            next = durable.state.scoped_snapshot();
                            (
                                recovery
                                    .application_condition(effect_id)
                                    .map_err(recovery_error)?,
                                ImsRecoveryResult::Savepoint {
                                    status: "SA".into(),
                                },
                            )
                        }
                        Err(problem) => return Err(recovery_error(problem)),
                    }
                }
            }
            ImsRecoveryCall::Rols {
                token: Some(token),
                area_length,
            } => {
                if let Some(point) = recovery
                    .application_point(*token, &epoch)
                    .map_err(recovery_error)?
                {
                    let data = point.user_data;
                    if Some(data.len()) != *area_length {
                        return Err(HostProblem::Malformed);
                    }
                    for row in point.rows {
                        if row.namespace != GENERIC_DATABASE_NAMESPACE
                            || !names.contains(&row.key)
                            || !next
                                .generic_pending_undo
                                .get(run)
                                .is_some_and(|undo| undo.contains_key(&row.key))
                        {
                            return Err(HostProblem::UnknownOutcome);
                        }
                        let value: ObjectRow<DatabaseEngineImage> =
                            serde_json::from_slice(&row.payload)
                                .map_err(|_| HostProblem::ProviderFailure)?;
                        if value.schema_version != OBJECT_ROW_SCHEMA || value.object_key != row.key
                        {
                            return Err(HostProblem::ProviderFailure);
                        }
                        generic::isolation::publish_backout_image(
                            &mut next,
                            run,
                            &row.key,
                            value.value,
                            self.limits,
                        )?;
                        generic::reset_positions(&mut next, &row.key, None);
                    }
                    lose_position(&mut next, run)?;
                    (
                        recovery
                            .application_backout(effect_id, Some(*token))
                            .map_err(recovery_error)?,
                        ImsRecoveryResult::BackedOut {
                            status: "  ".into(),
                            user_data: data,
                        },
                    )
                } else {
                    (
                        recovery
                            .application_condition(effect_id)
                            .map_err(recovery_error)?,
                        ImsRecoveryResult::BackedOut {
                            status: if unsupported { "RC" } else { "RA" }.into(),
                            user_data: vec![],
                        },
                    )
                }
            }
            ImsRecoveryCall::Rols { token: None, .. }
            | ImsRecoveryCall::Rolb
            | ImsRecoveryCall::Roll => {
                if unsupported {
                    return Err(HostProblem::Unsupported);
                }
                if let Some(undo) = next.generic_pending_undo.get(run).cloned() {
                    for (name, image) in undo.iter() {
                        generic::isolation::publish_backout_image(
                            &mut next,
                            run,
                            name,
                            (**image).clone(),
                            self.limits,
                        )?;
                        generic::reset_positions(&mut next, name, None);
                    }
                }
                next.generic_pending_undo.remove(run);
                lose_position(&mut next, run)?;
                let session =
                    Arc::make_mut(next.sessions.get_mut(run).ok_or(HostProblem::NotFound)?);
                session.recovery.uow_epoch = epoch
                    .sequence
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                let code = match request.call {
                    ImsRecoveryCall::Roll => Some("U0778"),
                    ImsRecoveryCall::Rols { .. } => Some("U3303"),
                    _ => None,
                };
                session.recovery.terminated = code.map(str::to_string);
                let observation = ImsRequest {
                    operation: ImsOperation::Rollback,
                    psb: None,
                    pcb: 0,
                    segments: vec![],
                    data: vec![],
                    qualifiers: vec![],
                    checkpoint_id: None,
                    max_segments: 1,
                    system: None,
                    q_class: None,
                    mutation: None,
                };
                system::observe_database_call(
                    &mut next,
                    run,
                    &observation,
                    &status("  "),
                    self.limits,
                )?;
                let result = match code {
                    Some(code) => ImsRecoveryResult::Abended { code: code.into() },
                    None => ImsRecoveryResult::BackedOut {
                        status: "  ".into(),
                        user_data: vec![],
                    },
                };
                (
                    recovery
                        .application_backout(effect_id, None)
                        .map_err(recovery_error)?,
                    result,
                )
            }
            _ => return Err(HostProblem::Malformed),
        };
        result.validate()?;
        let transition = recovery
            .bind_application_result(transition, effect_id, digest, encode_result(&result)?)
            .map_err(recovery_error)?;
        self.publish_application_recovery(
            invocation,
            request,
            &mut durable,
            next,
            transition,
            effects,
            digest,
        )?;
        Ok(result)
    }
}

fn lose_position(state: &mut State, run: &str) -> Result<(), HostProblem> {
    generic::pcb::clear_positions(Arc::make_mut(
        state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?,
    ));
    for system in state.system.values_mut() {
        Arc::make_mut(system).lose_backout_position(run);
    }
    Ok(())
}
