//! Shared database, SSA and GSAM execution/replay pipeline.
use super::*;

impl ImsService {
    pub fn execute(
        &self,
        invocation: &Invocation,
        request: &ImsRequest,
    ) -> Result<ImsResult, HostProblem> {
        self.execute_at(invocation, request, invocation.deadline_tick)
    }

    pub fn execute_navigation(
        &self,
        invocation: &Invocation,
        request: &mainframe_env_host_api::ImsNavigationRequest,
    ) -> Result<ImsResult, HostProblem> {
        self.execute_operands_at(
            invocation,
            &request.request,
            invocation.deadline_tick,
            Some(request),
            None,
            None,
        )
        .map(|result| result.result)
    }

    pub(in crate::service) fn execute_at(
        &self,
        invocation: &Invocation,
        request: &ImsRequest,
        resolution_lower_bound: u64,
    ) -> Result<ImsResult, HostProblem> {
        self.execute_operands_at(
            invocation,
            request,
            resolution_lower_bound,
            None,
            None,
            None,
        )
        .map(|result| result.result)
    }

    pub(in crate::service) fn execute_operands_at(
        &self,
        invocation: &Invocation,
        request: &ImsRequest,
        resolution_lower_bound: u64,
        navigation: Option<&mainframe_env_host_api::ImsNavigationRequest>,
        gsam: Option<&mainframe_env_host_api::ImsGsamRequest>,
        feedback: Option<&mainframe_env_host_api::ImsPcbFeedbackRequestV1>,
    ) -> Result<feedback::ExecutionOutput, HostProblem> {
        if let Some(feedback) = feedback {
            feedback.validate(mainframe_env_host_api::HostLimits::default())?;
        }
        if let Some(navigation) = navigation {
            navigation.validate(mainframe_env_host_api::HostLimits::default())?;
        }
        if let Some(gsam) = gsam {
            gsam.validate(mainframe_env_host_api::HostLimits::default())?;
        }
        if resolution_lower_bound == 0 {
            return Err(HostProblem::Malformed);
        }
        let mut durable = self.lock()?;
        refresh_replay(&*self.store, self.limits, &mut durable)?;
        system::reservations::refresh(&*self.store, self.limits, &mut durable)?;
        let integrity_read = generic::integrity::is_read(request.operation);
        if integrity_read {
            generic::integrity::refresh_sessions(&*self.store, self.limits, &mut durable)?;
        } else if durable.state.metadata.is_some() {
            generic::refresh_databases(&*self.store, self.limits, &mut durable)?;
        }
        let system_resources = if request.operation == ImsOperation::System {
            Some(system::resources(&durable.state, invocation, request)?)
        } else {
            None
        };
        if let Some(authorizer) = &self.authorizer {
            for resource in if let Some(resources) = system_resources {
                resources
            } else {
                ims_resources(&durable.state, invocation, request)?
            } {
                authorizer.authorize(invocation.principal.id(), &resource)?;
            }
        }
        refresh_replay(&*self.store, self.limits, &mut durable)?;
        let request_sha256 = if let Some(feedback) = feedback {
            mainframe_env_host_api::canonical_request_digest(&HostRequest::ImsPcbFeedbackV1(
                feedback.clone(),
            ))?
        } else if let Some(gsam) = gsam {
            mainframe_env_host_api::canonical_request_digest(&HostRequest::ImsGsam(gsam.clone()))?
        } else if let Some(navigation) = navigation {
            mainframe_env_host_api::canonical_request_digest(&HostRequest::ImsNavigation(
                navigation.clone(),
            ))?
        } else {
            canonical_ims_request_digest(request)?
        };
        let replay_key = request
            .mutation
            .as_ref()
            .map(|mutation| mutation.idempotency_key.as_str());
        let sequence = request.mutation.as_ref().map(|mutation| mutation.sequence);
        if let Some(key) = replay_key
            && let Some(recorded) = durable.state.replay.get(key)
        {
            match recorded.request_digest_format {
                ReplayDigestFormat::LegacyDebugV0 => return Err(HostProblem::UnknownOutcome),
                ReplayDigestFormat::CanonicalHostV1
                    if recorded.request_sha256 == request_sha256 =>
                {
                    if feedback.is_some() != recorded.pcb_feedback_v1.is_some() {
                        return Err(HostProblem::UnknownOutcome);
                    }
                    let result = recorded.result();
                    let feedback = recorded.pcb_feedback_v1.clone();
                    let address = recorded
                        .gsam
                        .as_ref()
                        .and_then(|output| output.address.clone());
                    let pending = ims_pending_replay_matches(
                        recorded,
                        key,
                        invocation,
                        sequence.ok_or(HostProblem::MissingIdempotency)?,
                    )?;
                    if pending {
                        self.finalize_replay_metadata(&mut durable, key, resolution_lower_bound)
                            .map_err(|_| HostProblem::UnknownOutcome)?;
                    }
                    return Ok(feedback::ExecutionOutput {
                        result,
                        address,
                        feedback,
                    });
                }
                ReplayDigestFormat::CanonicalHostV1 => {
                    return Err(HostProblem::IdempotencyConflict);
                }
            }
        }
        let run = invocation.run_unit_id.as_str();
        if let Some(feedback) = feedback {
            feedback::prepare(&durable.state, run, feedback)?;
        }
        if integrity_read {
            if durable.state.metadata.is_some() {
                generic::refresh_databases(&*self.store, self.limits, &mut durable)?;
            }
            generic::integrity::refresh_legacy(&*self.store, self.limits, &mut durable)?;
        }
        let read_fence = generic::integrity::prepare(&durable.state, run, request)?;
        let prepared = navigation
            .map(|navigation| {
                generic::ssa::prepare(&durable.state, invocation, navigation, self.limits)
            })
            .transpose()?;
        if let Some(gsam) = gsam {
            generic::gsam::prepare(&durable.state, invocation, gsam, self.limits)?;
        }
        let mut next = durable.state.scoped_snapshot();
        application_backout::prepare_database_call(&next, run, request)?;
        let output = if let Some(gsam) = gsam {
            Some(generic::gsam::apply(
                &mut next,
                &durable.versions,
                run,
                gsam,
                request_sha256,
                self.limits,
            )?)
        } else {
            None
        };
        let result = if let Some(output) = &output {
            output.result.clone()
        } else if let Some(prepared) = prepared {
            generic::ssa::read(&mut next, run, request, &prepared, self.limits)?
        } else if request.operation == ImsOperation::System {
            system::apply_request(&mut next, run, request, self.limits)?
        } else if generic::is_generic(&next, run, request) {
            generic::apply_request(&mut next, run, request, self.limits)?
        } else {
            apply_request(&mut next, run, request, self.limits)?
        };
        system::reservations::ensure_legacy_changes(
            &durable.state,
            &mut next,
            run,
            request,
            &result,
        )?;
        if request.operation != ImsOperation::System {
            system::observe_database_call(&mut next, run, request, &result, self.limits)?;
        }
        application_backout::settle_database_call(&mut next, invocation, request, &result)?;
        let pcb_feedback = feedback
            .map(|feedback| feedback::project(&next, run, feedback, &result, self.limits))
            .transpose()?;
        if let Some(feedback) = &pcb_feedback {
            mainframe_env_host_api::ImsPcbFeedbackResultV1 {
                result: result.clone(),
                feedback: feedback.clone(),
            }
            .validate(mainframe_env_host_api::HostLimits::default())?;
        }
        let uow = request.operation == ImsOperation::Commit
            || request.operation == ImsOperation::Rollback;
        if uow
            && next.definitions.is_none()
            && next.metadata.is_none()
            && !durable.state.pending_undo.contains_key(run)
            && !durable.state.generic_pending_undo.contains_key(run)
        {
            return Ok(feedback::ExecutionOutput {
                result,
                address: None,
                feedback: None,
            });
        }
        if request.operation.is_mutating() {
            let key = replay_key.ok_or(HostProblem::MissingIdempotency)?;
            if next.replay.len() >= self.limits.max_replays {
                return Err(HostProblem::ResourceExhausted);
            }
            let mut recorded = RecordedResult::from_result(request_sha256, &result);
            recorded.pcb_feedback_v1 = pcb_feedback.clone();
            if let Some(output) = &output {
                recorded.gsam = Some(gsam::ReplayOutput {
                    address: output.address.clone(),
                });
            }
            prepare_ims_replay(
                &mut recorded,
                key,
                invocation,
                sequence.ok_or(HostProblem::MissingIdempotency)?,
                self.limits,
            )?;
            next.replay.insert(key.into(), Arc::new(recorded));
            validate_state(&next, self.limits)?;
            generic::integrity::persist(self, &mut durable, next, &read_fence)?;
            self.finalize_replay_metadata(&mut durable, key, resolution_lower_bound)
                .map_err(|_| HostProblem::UnknownOutcome)?;
        }
        Ok(feedback::ExecutionOutput {
            result,
            address: output.and_then(|o| o.address),
            feedback: pcb_feedback,
        })
    }

    pub(in crate::service) fn finalize_replay_metadata(
        &self,
        durable: &mut DurableState,
        key: &str,
        resolution_lower_bound: u64,
    ) -> Result<(), HostProblem> {
        let Some(clock) = &self.replay_clock else {
            return Ok(());
        };
        let observed_tick = clock.now_tick()?;
        if observed_tick == 0 {
            return Err(HostProblem::InfrastructureFailure);
        }
        let mut next = durable.state.scoped_snapshot();
        let recorded = next
            .replay
            .get_mut(key)
            .ok_or(HostProblem::InfrastructureFailure)?;
        resolve_ims_replay(
            Arc::make_mut(recorded),
            key,
            observed_tick,
            resolution_lower_bound,
        )?;
        validate_state(&next, self.limits)?;
        self.persist(durable, next)
    }
}

impl ImsService {
    /// Bind a retained pre-canonical replay receipt to a reviewed typed request.
    ///
    /// Legacy receipts are never replayed or redispatched implicitly. The caller
    /// must attest the exact retained digest before this metadata-only migration.
    pub fn reconcile_legacy_replay(
        &self,
        key: &IdempotencyKey,
        expected_legacy_digest: [u8; 32],
        request: &ImsRequest,
    ) -> Result<(), HostProblem> {
        if !request.operation.is_mutating()
            || request
                .mutation
                .as_ref()
                .map(|mutation| &mutation.idempotency_key)
                != Some(key)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let canonical = canonical_ims_request_digest(request)?;
        let mut durable = self.lock()?;
        refresh_replay(&*self.store, self.limits, &mut durable)?;
        let retained = durable
            .state
            .replay
            .get(key.as_str())
            .ok_or(HostProblem::NotFound)?;
        match retained.request_digest_format {
            ReplayDigestFormat::CanonicalHostV1 => {
                return if retained.request_sha256 == canonical {
                    Ok(())
                } else {
                    Err(HostProblem::IdempotencyConflict)
                };
            }
            ReplayDigestFormat::LegacyDebugV0
                if retained.request_sha256 != expected_legacy_digest =>
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            ReplayDigestFormat::LegacyDebugV0 => {}
        }
        let mut next = durable.state.scoped_snapshot();
        let retained = next
            .replay
            .get_mut(key.as_str())
            .ok_or(HostProblem::NotFound)?;
        let retained = Arc::make_mut(retained);
        retained.request_digest_format = ReplayDigestFormat::CanonicalHostV1;
        retained.request_sha256 = canonical;
        validate_state(&next, self.limits)?;
        self.persist(&mut durable, next)
    }
}

pub(super) fn refresh_replay(
    store: &dyn ProviderStateStore,
    limits: ImsLimits,
    durable: &mut DurableState,
) -> Result<(), HostProblem> {
    let mut replay_versions = RowVersions::new();
    let replay: BTreeMap<String, Arc<RecordedResult>> = load_row_map(
        store,
        REPLAY_NAMESPACE,
        limits.max_replays,
        limits,
        &mut replay_versions,
    )?;
    if replay
        .iter()
        .any(|(key, recorded)| validate_ims_recorded_result(key, recorded, limits).is_err())
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    durable
        .versions
        .retain(|(namespace, _), _| namespace != REPLAY_NAMESPACE);
    durable.versions.extend(replay_versions);
    durable.state.replay = replay;
    Ok(())
}
