//! CHKP/XRST adaptation over actual selected Session, database and undo rows.
use super::*;
use crate::database::{
    DatabaseEngine, EngineProblem, FieldPredicate, ReadKind, ReadRequest, Relation, SegmentSelector,
};
use crate::recovery::{
    APPLICATION_RESULT_DOMAIN, CheckpointKind, CheckpointRequest, PositionAttempt, RecoveryContext,
    RepositionStatus, RestartSelection, SavedPcbPosition,
};
use mainframe_env_host_api::ImsRestartSelection;

/// Receipt bytes are private replay data under RecoverySession's existing digest.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ApplicationResult {
    checkpoint: bool,
    sequence: u64,
    id: Option<String>,
    areas: Vec<Vec<u8>>,
    pcbs: Vec<(u16, String)>,
}

fn encode_result(result: &ImsRecoveryResult) -> Result<Vec<u8>, HostProblem> {
    let value = match result {
        ImsRecoveryResult::Checkpointed { id, sequence, .. } => ApplicationResult {
            checkpoint: true,
            sequence: *sequence,
            id: Some(id.clone()),
            areas: vec![],
            pcbs: vec![],
        },
        ImsRecoveryResult::Restarted {
            checkpoint_id,
            user_areas,
            pcb_statuses,
            ..
        } => ApplicationResult {
            checkpoint: false,
            sequence: 0,
            id: checkpoint_id.clone(),
            areas: user_areas.clone(),
            pcbs: pcb_statuses.clone(),
        },
        _ => return Err(HostProblem::Malformed),
    };
    let mut bytes = APPLICATION_RESULT_DOMAIN.to_vec();
    bytes.extend(serde_json::to_vec(&value).map_err(|_| HostProblem::InfrastructureFailure)?);
    Ok(bytes)
}

fn decode_result(bytes: &[u8]) -> Result<ImsRecoveryResult, HostProblem> {
    let value: ApplicationResult = serde_json::from_slice(
        bytes
            .strip_prefix(APPLICATION_RESULT_DOMAIN)
            .ok_or(HostProblem::ProviderFailure)?,
    )
    .map_err(|_| HostProblem::ProviderFailure)?;
    let result = if value.checkpoint {
        if !value.areas.is_empty() || !value.pcbs.is_empty() {
            return Err(HostProblem::ProviderFailure);
        }
        ImsRecoveryResult::Checkpointed {
            status: "  ".into(),
            id: value.id.ok_or(HostProblem::ProviderFailure)?,
            sequence: value.sequence,
        }
    } else {
        if value.sequence != 0 {
            return Err(HostProblem::ProviderFailure);
        }
        ImsRecoveryResult::Restarted {
            status: "  ".into(),
            checkpoint_id: value.id,
            user_areas: value.areas,
            pcb_statuses: value.pcbs,
        }
    };
    result
        .validate()
        .map_err(|_| HostProblem::ProviderFailure)?;
    Ok(result)
}

fn selection(value: &ImsRestartSelection) -> RestartSelection {
    match value {
        ImsRestartSelection::Normal => RestartSelection::Normal,
        ImsRestartSelection::Checkpoint(id) => RestartSelection::Id(id.clone()),
        ImsRestartSelection::Timestamp(value) => RestartSelection::Timestamp(value.clone()),
        ImsRestartSelection::Last => RestartSelection::Last,
    }
}

impl ImsService {
    /// Read-only authoritative observation for the coordinator's fenced resolver.
    /// Ambiguous/missing receipts stay None. This never redispatches CHKP/XRST,
    /// writes a journal result or accepts caller-created store mutations.
    pub fn observe_application_recovery(
        &self,
        invocation: &Invocation,
        request: &ImsRecoveryRequest,
    ) -> Result<Option<ImsRecoveryResult>, HostProblem> {
        request.validate(HostLimits::default())?;
        if matches!(request.call, ImsRecoveryCall::Log { .. }) {
            return Err(HostProblem::Unsupported);
        }
        let capability = CapabilityId::new("host.ims.write", InvocationLimits::default())
            .map_err(|_| HostProblem::Malformed)?;
        if invocation.service_class != ServiceClass::Batch
            || !invocation.principal.has_grant(&capability)
        {
            return Err(HostProblem::Unauthorized);
        }
        let authorizer = self.authorizer.as_ref().ok_or(HostProblem::Unauthorized)?;
        authorizer.authorize(
            invocation.principal.id(),
            &EnterpriseResource::new(
                EnterpriseResourceClass::ImsPsb,
                request.psb.clone(),
                AccessIntent::Update,
            )?,
        )?;
        let selected = self
            .selected_metadata_generation(&request.application)?
            .ok_or(HostProblem::NotFound)?;
        if selected.package_identity != request.package_identity {
            return Err(HostProblem::IdempotencyConflict);
        }
        if !selected.catalog.psbs.iter().find(|psb| psb.name == request.psb).ok_or(HostProblem::NotFound)?.pcbs.iter().any(|pcb| matches!(pcb, ImsPcbMetadata::Database(pcb) if pcb.database == request.database)) {
            return Err(HostProblem::Malformed);
        }
        self.authorize_checkpoint_scope(invocation, &selected, &request.psb)?;
        let recovery = RecoverySession::load(
            &*self.store,
            &recovery_address(invocation, request, &selected.application),
            RecoveryLimits::default(),
        )
        .map_err(recovery_error)?;
        let digest = mainframe_env_host_api::canonical_request_digest(&HostRequest::ImsRecovery(
            request.clone(),
        ))?;
        recovery
            .application_replay(&recovery_effect_address(invocation, request), digest)
            .map_err(recovery_error)?
            .map(|(_, bytes)| decode_result(&bytes))
            .transpose()
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn application_checkpoint(
        &self,
        invocation: &Invocation,
        request: &ImsRecoveryRequest,
        effects: &dyn IdempotencyStore,
        digest: [u8; 32],
        selected: &crate::ImsMetadataGeneration,
        recovery: RecoverySession,
        effect_id: &str,
    ) -> Result<ImsRecoveryResult, HostProblem> {
        self.authorize_checkpoint_scope(invocation, selected, &request.psb)?;
        // Canonical receipt lookup precedes observations of later UOW/position.
        // A replay must not regenerate its identity from today's database image.
        if let Some((_, bytes)) = recovery
            .application_replay(effect_id, digest)
            .map_err(recovery_error)?
        {
            return decode_result(&bytes);
        }
        let mut durable = self.lock()?;
        // The row may have advanced while this dispatch waited on the session
        // fence. Observe the authoritative receipt again before order checks.
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
        if durable.state.metadata.as_ref() != Some(&selected.catalog) {
            return Err(HostProblem::IdempotencyConflict);
        }
        let run = invocation.run_unit_id.as_str();
        let session = durable
            .state
            .sessions
            .get(run)
            .ok_or(HostProblem::NotFound)?;
        if !session.generic || session.psb != request.psb {
            return Err(HostProblem::Malformed);
        }
        generic::isolation::ensure_backout(&durable.state, run)?;
        let mut next = durable.state.scoped_snapshot();
        let marker =
            &mut Arc::make_mut(next.sessions.get_mut(run).ok_or(HostProblem::NotFound)?).recovery;
        if marker.execution != invocation.execution_id.as_str()
            || marker.attempt != invocation.attempt
        {
            *marker = ExecutionRecovery {
                execution: invocation.execution_id.to_string(),
                attempt: invocation.attempt,
                ..Default::default()
            };
        }
        let prior_xrst = marker.xrst;
        let prior_kind = marker.checkpoint_kind;
        let (transition, result) = match &request.call {
            ImsRecoveryCall::BasicCheckpoint { id }
            | ImsRecoveryCall::SymbolicCheckpoint { id, .. } => {
                let (kind, areas) = match &request.call {
                    ImsRecoveryCall::BasicCheckpoint { .. } => (CheckpointKind::Basic, vec![]),
                    ImsRecoveryCall::SymbolicCheckpoint { user_areas, .. } => {
                        (CheckpointKind::Symbolic, user_areas.clone())
                    }
                    _ => unreachable!(),
                };
                if (kind == CheckpointKind::Symbolic && !prior_xrst)
                    || (kind == CheckpointKind::Basic && prior_xrst)
                    || prior_kind.is_some_and(|prior| prior != kind)
                {
                    return Err(HostProblem::Malformed);
                }
                // Include every pending dependency in the authorization set.
                for name in next
                    .generic_pending_undo
                    .get(run)
                    .into_iter()
                    .flat_map(|undo| undo.keys())
                    .chain(
                        next.pending_undo
                            .get(run)
                            .into_iter()
                            .flat_map(|undo| undo.keys()),
                    )
                {
                    self.authorizer
                        .as_ref()
                        .ok_or(HostProblem::Unauthorized)?
                        .authorize(
                            invocation.principal.id(),
                            &EnterpriseResource::new(
                                EnterpriseResourceClass::ImsDatabase,
                                name.clone(),
                                AccessIntent::Update,
                            )?,
                        )?;
                }
                let positions = if kind == CheckpointKind::Symbolic {
                    save_positions(&next, run, self.limits)?
                } else {
                    vec![]
                };
                let committed_digest = database_digest(&next, &request.psb)?;
                let transition = recovery
                    .checkpoint(
                        effect_id,
                        CheckpointRequest {
                            id: id.clone(),
                            kind,
                            context: RecoveryContext::Batch,
                            prior_xrst,
                            user_areas: areas,
                            positions,
                        },
                        committed_digest,
                    )
                    .map_err(recovery_error)?;
                next.pending_undo.remove(run);
                next.generic_pending_undo.remove(run);
                let session =
                    Arc::make_mut(next.sessions.get_mut(run).ok_or(HostProblem::NotFound)?);
                generic::pcb::clear_positions(session);
                session.recovery.checkpoint_kind = Some(kind);
                // Reuse the existing Q release observer without changing its ordinary-write owner.
                system::observe_database_call(
                    &mut next,
                    run,
                    &checkpoint_observation(),
                    &status("  "),
                    self.limits,
                )?;
                let result = ImsRecoveryResult::Checkpointed {
                    status: "  ".into(),
                    id: id.clone(),
                    sequence: transition.sequence(),
                };
                (transition, result)
            }
            ImsRecoveryCall::Restart {
                selection: selector,
                area_lengths,
            } => {
                if prior_xrst || prior_kind.is_some() {
                    return Err(HostProblem::Malformed);
                }
                // XRST cannot commit or erase an unsettled pre-restart UOW.
                if next.generic_pending_undo.contains_key(run)
                    || next.pending_undo.contains_key(run)
                {
                    return Err(HostProblem::IdempotencyConflict);
                }
                let selected_image = recovery
                    .restart(selection(selector), RecoveryContext::Batch)
                    .map_err(recovery_error)?;
                if selected_image.checkpoint_id.is_some()
                    && (area_lengths.len() > selected_image.user_areas.len()
                        || area_lengths
                            .iter()
                            .zip(&selected_image.user_areas)
                            .any(|(length, area)| *length != area.len()))
                {
                    return Err(HostProblem::Malformed);
                }
                if selected_image.checkpoint_id.is_some() {
                    generic::pcb::clear_positions(Arc::make_mut(
                        next.sessions.get_mut(run).ok_or(HostProblem::NotFound)?,
                    ));
                }
                let mut pcb_statuses = vec![];
                let plan = recovery
                    .xrst_staged(effect_id, selection(selector), |saved| {
                        let (number, position, observed) =
                            restore_position(&next, &request.psb, saved, self.limits)
                                .map_err(host_to_recovery)?;
                        generic::pcb::set_position(
                            Arc::make_mut(
                                next.sessions
                                    .get_mut(run)
                                    .ok_or(RecoveryProblem::NotFound)?,
                            ),
                            number,
                            position,
                        );
                        let status = match observed.as_deref() {
                            Some("  ") => RepositionStatus::Reestablished,
                            Some(_) => RepositionStatus::NotFound,
                            None => RepositionStatus::NotAttempted,
                        };
                        if let Some(observed) = observed {
                            pcb_statuses.push((number, observed));
                        }
                        Ok(PositionAttempt {
                            status,
                            mutation: None,
                        })
                    })
                    .map_err(recovery_error)?;
                pcb_statuses.sort_by_key(|(pcb, _)| *pcb);
                for (number, observed) in &pcb_statuses {
                    let mut observation = checkpoint_observation();
                    observation.operation = ImsOperation::GetUnique;
                    observation.pcb = *number;
                    system::observe_database_call(
                        &mut next,
                        run,
                        &observation,
                        &status(observed),
                        self.limits,
                    )?;
                }
                Arc::make_mut(next.sessions.get_mut(run).ok_or(HostProblem::NotFound)?)
                    .recovery
                    .xrst = true;
                let result = ImsRecoveryResult::Restarted {
                    status: "  ".into(),
                    checkpoint_id: plan.result.checkpoint_id,
                    user_areas: plan
                        .result
                        .user_areas
                        .into_iter()
                        .take(area_lengths.len())
                        .collect(),
                    pcb_statuses,
                };
                (plan.transition, result)
            }
            ImsRecoveryCall::Log { .. } => return Err(HostProblem::Malformed),
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

    fn authorize_checkpoint_scope(
        &self,
        invocation: &Invocation,
        selected: &crate::ImsMetadataGeneration,
        psb_name: &str,
    ) -> Result<(), HostProblem> {
        let authorizer = self.authorizer.as_ref().ok_or(HostProblem::Unauthorized)?;
        let psb = selected
            .catalog
            .psbs
            .iter()
            .find(|psb| psb.name == psb_name)
            .ok_or(HostProblem::NotFound)?;
        for pcb in &psb.pcbs {
            if let ImsPcbMetadata::Database(pcb) = pcb {
                authorizer.authorize(
                    invocation.principal.id(),
                    &EnterpriseResource::new(
                        EnterpriseResourceClass::ImsDatabase,
                        pcb.database.clone(),
                        AccessIntent::Update,
                    )?,
                )?;
                if selected
                    .catalog
                    .databases
                    .iter()
                    .find(|db| db.name == pcb.database)
                    .is_none_or(|db| {
                        db.organization == mainframe_env_host_api::ImsDatabaseOrganization::Gsam
                    })
                {
                    // GSAM restart needs the RSA/file authority owned by the
                    // GSAM lane. Never substitute an empty DB position for it.
                    return Err(HostProblem::Unsupported);
                }
            }
        }
        Ok(())
    }
}

fn refresh_system(
    store: &dyn ProviderStateStore,
    limits: ImsLimits,
    durable: &mut DurableState,
) -> Result<(), HostProblem> {
    let mut versions = RowVersions::new();
    durable.state.system = load_row_map(
        store,
        SYSTEM_NAMESPACE,
        limits.max_sessions,
        limits,
        &mut versions,
    )?;
    durable
        .versions
        .retain(|(namespace, _), _| namespace != SYSTEM_NAMESPACE);
    durable.versions.extend(versions);
    Ok(())
}

fn checkpoint_observation() -> ImsRequest {
    ImsRequest {
        operation: ImsOperation::Checkpoint,
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
    }
}

fn database_digest(state: &State, psb: &str) -> Result<[u8; 32], HostProblem> {
    let psb = state
        .metadata
        .as_ref()
        .and_then(|catalog| catalog.psbs.iter().find(|item| item.name == psb))
        .ok_or(HostProblem::NotFound)?;
    let mut images = BTreeMap::new();
    for pcb in &psb.pcbs {
        if let ImsPcbMetadata::Database(pcb) = pcb {
            images.insert(
                &pcb.database,
                state
                    .generic_databases
                    .get(&pcb.database)
                    .ok_or(HostProblem::NotFound)?,
            );
        }
    }
    let bytes = serde_json::to_vec(&images).map_err(|_| HostProblem::InfrastructureFailure)?;
    let mut hasher = Sha256::new();
    hasher.update(b"mainframe-env.ims-checkpoint-databases@1\0");
    hasher.update(bytes);
    Ok(hasher.finalize().into())
}

// The saved bytes contain only provider-derived segment/key tuples, never
// caller-selected namespaces, record occurrence IDs or arbitrary snapshots.
type KeyPath = Vec<(String, Vec<u8>)>;

fn record_key(
    engine: &DatabaseEngine,
    id: crate::database::RecordId,
) -> Result<KeyPath, HostProblem> {
    engine
        .path_to(id)
        .map_err(|_| HostProblem::ProviderFailure)?
        .into_iter()
        .map(|record| {
            let segment = engine
                .definition()
                .segments
                .iter()
                .find(|segment| segment.name == record.segment)
                .ok_or(HostProblem::ProviderFailure)?;
            let field = segment
                .fields
                .iter()
                .find(|field| Some(&field.name) == segment.key_field.as_ref())
                .ok_or(HostProblem::Unsupported)?;
            let key = record
                .data
                .get(field.offset..field.offset + field.length)
                .ok_or(HostProblem::ProviderFailure)?
                .to_vec();
            Ok((record.segment, key))
        })
        .collect()
}

fn save_positions(
    state: &State,
    run: &str,
    limits: ImsLimits,
) -> Result<Vec<SavedPcbPosition>, HostProblem> {
    let session = state.sessions.get(run).ok_or(HostProblem::NotFound)?;
    let psb = state
        .metadata
        .as_ref()
        .and_then(|catalog| catalog.psbs.iter().find(|psb| psb.name == session.psb))
        .ok_or(HostProblem::NotFound)?;
    let mut saved = vec![];
    for (index, pcb) in psb.pcbs.iter().enumerate() {
        let ImsPcbMetadata::Database(pcb) = pcb else {
            continue;
        };
        let number = u16::try_from(index + 1).map_err(|_| HostProblem::ResourceExhausted)?;
        let Some(id) = generic::pcb::position(session, number).current() else {
            continue;
        };
        // Physical key paths do not retain the selected index's source pointer.
        // Never publish a primary-order substitute for a secondary resume point.
        if pcb.secondary_index.is_some() {
            return Err(HostProblem::Unsupported);
        }
        let engine = generic::restored(state, &pcb.database, limits)?;
        let path = match record_key(&engine, id) {
            Ok(path) => path,
            Err(HostProblem::Unsupported) => vec![],
            Err(problem) => return Err(problem),
        };
        let segment_key =
            serde_json::to_vec(&path).map_err(|_| HostProblem::InfrastructureFailure)?;
        saved.push(SavedPcbPosition {
            pcb: number.to_string(),
            database: pcb.database.clone(),
            segment_key,
        });
    }
    Ok(saved)
}

fn restore_position(
    state: &State,
    psb: &str,
    saved: &SavedPcbPosition,
    limits: ImsLimits,
) -> Result<(u16, PcbPosition, Option<String>), HostProblem> {
    let number = saved
        .pcb
        .parse::<u16>()
        .map_err(|_| HostProblem::ProviderFailure)?;
    if number.to_string() != saved.pcb {
        return Err(HostProblem::ProviderFailure);
    }
    let (_, pcb) = generic::scheduled_pcb(state, psb, number)?;
    if pcb.database != saved.database {
        return Err(HostProblem::IdempotencyConflict);
    }
    if pcb.secondary_index.is_some() {
        return Err(HostProblem::Unsupported);
    }
    let engine = generic::restored(state, &saved.database, limits)?;
    let path: KeyPath =
        serde_json::from_slice(&saved.segment_key).map_err(|_| HostProblem::ProviderFailure)?;
    if path.is_empty() {
        return Ok((number, PcbPosition::default(), None));
    }
    if path.len() > limits.max_segments {
        return Err(HostProblem::ProviderFailure);
    }
    let metadata = state
        .metadata
        .as_ref()
        .and_then(|catalog| {
            catalog
                .databases
                .iter()
                .find(|db| db.name == saved.database)
        })
        .ok_or(HostProblem::NotFound)?;
    if path.iter().any(|(name, _)| {
        metadata
            .segments
            .iter()
            .find(|segment| &segment.name == name)
            .is_none_or(|segment| {
                !segment
                    .fields
                    .iter()
                    .any(|field| field.sequence && field.unique)
            })
    }) {
        return Ok((number, PcbPosition::default(), None));
    }
    let mut selectors = vec![];
    for (name, key) in &path {
        if !pcb
            .sensitive_segments
            .iter()
            .any(|segment| &segment.name == name)
            || (!generic::allowed(pcb, name, ImsOperation::GetUnique)
                && !generic::pcb::key_only(pcb, name))
        {
            return Err(HostProblem::Unsupported);
        }
        let segment = engine
            .definition()
            .segments
            .iter()
            .find(|segment| &segment.name == name)
            .ok_or(HostProblem::ProviderFailure)?;
        let field = segment
            .fields
            .iter()
            .find(|field| Some(&field.name) == segment.key_field.as_ref())
            .ok_or(HostProblem::Unsupported)?;
        if field.length != key.len() {
            return Err(HostProblem::ProviderFailure);
        }
        selectors.push(SegmentSelector {
            segment: name.clone(),
            predicates: vec![FieldPredicate {
                field: field.name.clone(),
                relation: Relation::Equal,
                value: key.clone(),
            }],
        });
    }
    let mut position = PcbPosition::default();
    let request = ReadRequest {
        kind: ReadKind::Unique,
        target: path.last().map(|(name, _)| name.clone()),
        path: selectors,
        hold: false,
    };
    let observed = match engine.read(&mut position, &request) {
        Ok(_) => "  ",
        Err(EngineProblem::NotFound) => {
            // Reuse real GU on the last satisfied prefix, then locate the
            // predecessor in the engine's own ordered record stream. GN after
            // XRST must continue after the missing key, not restart at root one.
            for depth in (1..path.len()).rev() {
                let prefix = ReadRequest {
                    kind: ReadKind::Unique,
                    target: Some(path[depth - 1].0.clone()),
                    path: request.path[..depth].to_vec(),
                    hold: false,
                };
                if engine.read(&mut position, &prefix).is_ok() {
                    break;
                }
            }
            if !crate::database::keyed_root_order(engine.definition().organization) {
                return Err(HostProblem::Unsupported);
            }
            let boundary = key_order(&engine, &path)?;
            let mut predecessor = None;
            for record in engine.export_records() {
                let current = record_key(&engine, record.id)?;
                if key_order(&engine, &current)? >= boundary {
                    break;
                }
                predecessor = Some(record.id);
            }
            position = PcbPosition::default();
            if let Some(id) = predecessor {
                position.set_current(id);
            }
            "GE"
        }
        Err(_) => return Err(HostProblem::ProviderFailure),
    };
    Ok((number, position, Some(observed.into())))
}

fn key_order(
    engine: &DatabaseEngine,
    path: &KeyPath,
) -> Result<Vec<(usize, Vec<u8>)>, HostProblem> {
    path.iter()
        .map(|(name, key)| {
            let ordinal = engine
                .definition()
                .segments
                .iter()
                .position(|segment| &segment.name == name)
                .ok_or(HostProblem::ProviderFailure)?;
            Ok((ordinal, key.clone()))
        })
        .collect()
}

fn host_to_recovery(problem: HostProblem) -> RecoveryProblem {
    match problem {
        HostProblem::Unsupported => RecoveryProblem::Unsupported,
        HostProblem::Unauthorized => RecoveryProblem::Unauthorized,
        HostProblem::ResourceExhausted => RecoveryProblem::LimitExceeded,
        HostProblem::IdempotencyConflict => RecoveryProblem::Conflict,
        _ => RecoveryProblem::CorruptImage,
    }
}
