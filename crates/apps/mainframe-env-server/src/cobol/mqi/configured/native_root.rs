//! Genuine configured compiled parentNone drive and exclusive terminal hooks.
use super::*;
use crate::cobol::{
    DefaultProgramRouter,
    artifact::{AdmittedProgram, AdmittedProgramProvenance},
};
use mainframe_env_execution_api::{
    AuditDecision, ExecutionOutcome, RootTerminalAudit, RootTerminalAuditRole,
};
use mainframe_env_host_api::{
    RootTerminalMachineObservation, RootTerminalResource, RootTerminalResourceRow,
    RootTerminalSetup, canonical_root_terminal_resource_digest,
    canonical_root_terminal_setup_digest,
};
use mainframe_env_interpreter::{
    ExecutionCoordinator, NativeRootAdmission, NativeRootConfiguration, NativeRootHooks,
    NativeRootTermination, ReferenceMachine, WinningRootTerminal,
};
use mainframe_env_store_api::{
    ProviderStateIdentity, ProviderStateMutation, RootDriverClaim, RootTerminalCommit,
    TerminalRowDependency,
};

impl DefaultProgramRouter {
    /// Deliberate native-root program route, created only by this actual router.
    /// Register the returned physical Arc before binding the frozen host. Its
    /// closed weak implementation prevents a host/router reference cycle and
    /// forwards the unchanged original occurrence to this genuine router.
    /// Equal descriptors or an independently supplied forwarding wrapper cannot
    /// replace it. Legacy setup does not acquire this route automatically.
    pub fn native_root_program_provider(
        self: &Arc<Self>,
    ) -> Result<Arc<dyn HostProvider>, HostProblem> {
        let _setup = self
            .cobol
            .setup
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if self.cobol.host.get().is_some() || self.cobol.native_root_program.get().is_some() {
            return Err(HostProblem::IdempotencyConflict);
        }
        let provider: Arc<dyn HostProvider> = Arc::new(NativeProgramProvider {
            router: Arc::downgrade(self),
            descriptor: self.descriptor().clone(),
        });
        self.cobol
            .native_root_program
            .set(Arc::downgrade(&provider))
            .map_err(|_| HostProblem::IdempotencyConflict)?;
        Ok(provider)
    }
    /// Explicit native-root setup before frozen runtime publication. Retains a
    /// weak same-provider reference, not a second service or ownership directory.
    /// Legacy ProductServer/router defaults do not install this root route.
    pub fn bind_native_mq_root_host(
        &self,
        host: &Arc<ConfiguredInstalledMqHost>,
    ) -> Result<(), HostProblem> {
        let _setup = self
            .cobol
            .setup
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if self.cobol.host.get().is_some()
            || self
                .cobol
                .native_root_program
                .get()
                .and_then(Weak::upgrade)
                .is_none()
            || self
                .cobol
                .control
                .get()
                .is_none_or(|control| !Arc::ptr_eq(control, &host.control))
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        self.cobol
            .native_mq_host
            .set(Arc::downgrade(host))
            .map_err(|_| HostProblem::IdempotencyConflict)
    }
    /// PRIVILEGED compiled ordinary root driver, not application `final=true`
    /// or ProductServer fallback. Requires an already published/catalog-admitted
    /// artifact and SAME frozen host/store/control/physically selected MQ Arc.
    /// Preserves the original parentNone context; no root is rebuilt from a
    /// child, binding bytes, completed rows or historical handle observations.
    /// Unclassified exits retain/fence ownership without choosing commit/backout.
    pub fn execute_native_mq_root(
        &self,
        mq: &Arc<ConfiguredInstalledMqHost>,
        original: &Invocation,
        program: &str,
    ) -> Result<ExecutionOutcome, HostProblem> {
        let store = self
            .cobol
            .store
            .get()
            .ok_or(HostProblem::InfrastructureFailure)?;
        let host = self
            .cobol
            .host
            .get()
            .ok_or(HostProblem::InfrastructureFailure)?;
        let control = self
            .cobol
            .control
            .get()
            .ok_or(HostProblem::InfrastructureFailure)?;
        let native = self
            .cobol
            .native_mq_host
            .get()
            .and_then(Weak::upgrade)
            .ok_or(HostProblem::Unsupported)?;
        if original.parent_execution_id.is_some()
            || original.cancellation_requested()
            || !Arc::ptr_eq(store, &mq.store)
            || !Arc::ptr_eq(control, &mq.control)
            || !Arc::ptr_eq(&native, mq)
        {
            return Err(HostProblem::Unauthorized);
        }
        let expected: Arc<dyn HostProvider> = mq.clone();
        if !host.selects_same_provider(&mq.descriptor.capability, &expected)? {
            return Err(HostProblem::Unauthorized);
        }
        let program_provider = self
            .cobol
            .native_root_program
            .get()
            .and_then(Weak::upgrade)
            .ok_or(HostProblem::Unsupported)?;
        if !host
            .selects_same_provider(&program_provider.descriptor().capability, &program_provider)?
        {
            return Err(HostProblem::Unauthorized);
        }
        let admitted = self.cobol.preflight_installed_program(program, true)?;
        if original.artifact != admitted.artifact
            || original.selector.as_str() != format!("program:{}", admitted.name)
        {
            return Err(HostProblem::Unauthorized);
        }
        let AdmittedProgramProvenance::Catalog(catalog) = &admitted.provenance else {
            return Err(HostProblem::Unauthorized);
        };
        let limits = mq.mq_limits;
        let profile = [
            limits.max_queues as u64,
            limits.max_messages_per_queue as u64,
            limits.max_message_bytes as u64,
            limits.max_handles as u64,
            limits.max_pending_units as u64,
            limits.max_replays as u64,
            limits.max_state_bytes as u64,
        ];
        let semantic = &admitted.metadata.semantic_identity;
        let setup = canonical_root_terminal_setup_digest(
            &RootTerminalSetup {
                original,
                provider: &mq.descriptor,
                host_limits: mq.host_limits,
                mqi_limits: mq.mqi_limits,
                generation: mq.generation,
                fence: mq.fence,
                mq_limits: &profile,
                content_digest: admitted.executable.content_id().as_bytes(),
                semantic_identity: &semantic,
                manifest_payload_digest: &admitted.metadata.manifest_payload_digest,
                catalog: exact(catalog),
            },
            mainframe_env_host_api::MAX_CANONICAL_EFFECT_BYTES,
        )?;
        let mut namespaces = mq.runtime.native_terminal_namespaces();
        namespaces.push(format!(
            "{}{}",
            crate::cobol::retention::INSTANCE_NAMESPACE_PREFIX,
            crate::cobol::instance::run_key(original)
        ));
        namespaces.push(crate::cobol::retention::CALL_REPLAY_NAMESPACE.into());
        let configuration = NativeRootConfiguration {
            configuration_digest: setup,
            provider_namespaces: namespaces,
            provider_rows: vec![
                ProviderStateIdentity {
                    namespace: catalog.namespace.clone(),
                    key: catalog.key.clone(),
                },
                ProviderStateIdentity {
                    namespace: crate::cobol::retention::RUN_STATE_NAMESPACE.into(),
                    key: crate::cobol::instance::run_key(original),
                },
                ProviderStateIdentity {
                    namespace: crate::cobol::retention::CALL_PROTOCOL_NAMESPACE.into(),
                    key: crate::cobol::retention::protocol_key(original.run_unit_id.as_str()),
                },
            ],
        };
        let mut machine = ReferenceMachine::from_binary(
            admitted.executable.payload(),
            original.clone(),
            mainframe_env_ir::CodecLimits::default(),
        )
        .map_err(|_| HostProblem::ProviderFailure)?;
        let mut drive = Drive {
            router: self,
            mq,
            admitted: &admitted,
            original,
            setup,
            root: None,
        };
        let coordinator =
            ExecutionCoordinator::durable(host.clone(), store.clone(), Default::default());
        // Admission owns the directory root before the first machine drive.
        // The frame port borrows a shared slot populated only by that real hook.
        let slot = Arc::new(Mutex::new(None));
        let mut hooks = Hooks {
            drive: &mut drive,
            slot: slot.clone(),
        };
        Ok(coordinator.execute_root_with_control(
            &mut machine,
            original,
            configuration,
            &mut hooks,
            |machine| {
                machine
                    .bind_mqi_program_frame(Arc::new(RootObservation(slot)))
                    .map_err(|_| HostProblem::UnknownOutcome)
            },
            || control.observe(original),
        ))
    }
}

/// Closed same-router program path. No public constructor, JSON or lease input.
struct NativeProgramProvider {
    router: Weak<DefaultProgramRouter>,
    descriptor: CapabilityDescriptor,
}
impl HostProvider for NativeProgramProvider {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }
    fn invoke(&self, original: &Invocation, effect: EffectRequest) -> EffectResult {
        match self.router.upgrade() {
            Some(router) => router.invoke(original, effect),
            None => EffectResult {
                sequence: effect.sequence,
                outcome: Err(HostProblem::UnknownOutcome),
            },
        }
    }
}

fn exact(row: &mainframe_env_store_api::ProviderStateRecord) -> RootTerminalResourceRow<'_> {
    RootTerminalResourceRow::Exact {
        namespace: &row.namespace,
        key: &row.key,
        version: row.version,
        payload: &row.payload,
    }
}
fn dependency(row: &TerminalRowDependency) -> RootTerminalResourceRow<'_> {
    match row {
        TerminalRowDependency::Exact(row) => exact(row),
        TerminalRowDependency::Absent { namespace, key } => {
            RootTerminalResourceRow::Absent { namespace, key }
        }
    }
}
fn mutation(row: &ProviderStateMutation) -> RootTerminalResourceRow<'_> {
    match row {
        ProviderStateMutation::Put(write) => RootTerminalResourceRow::Put {
            namespace: &write.record.namespace,
            key: &write.record.key,
            version: write.record.version,
            expected: write.expected_version,
            payload: &write.record.payload,
        },
        ProviderStateMutation::Delete {
            namespace,
            key,
            expected_version,
        } => RootTerminalResourceRow::Delete {
            namespace,
            key,
            expected: *expected_version,
        },
        ProviderStateMutation::Move {
            record,
            old_key,
            expected_version,
        } => RootTerminalResourceRow::Move {
            namespace: &record.namespace,
            old_key,
            key: &record.key,
            version: record.version,
            expected: *expected_version,
            payload: &record.payload,
        },
    }
}

struct RootObservation(Arc<Mutex<Option<Arc<ClosedFrame>>>>);
impl mainframe_env_interpreter::MqMqiProgramFrame for RootObservation {
    fn profile(
        &self,
        original: &Invocation,
    ) -> Result<mainframe_env_interpreter::MqMqiProgramProfile, HostProblem> {
        let frame = self
            .0
            .lock()
            .map_err(|_| HostProblem::UnknownOutcome)?
            .clone()
            .ok_or(HostProblem::Unauthorized)?;
        frame::Observation(frame).profile(original)
    }
    fn local_unit(
        &self,
        original: &Invocation,
        connection: mainframe_env_host_api::MqHconn,
    ) -> Result<mainframe_env_host_api::mq_mqi::MqMqiUnitOfWork, HostProblem> {
        let frame = self
            .0
            .lock()
            .map_err(|_| HostProblem::UnknownOutcome)?
            .clone()
            .ok_or(HostProblem::Unauthorized)?;
        frame::Observation(frame).local_unit(original, connection)
    }
    fn connx_profile(
        &self,
        original: &Invocation,
    ) -> Result<mainframe_env_interpreter::MqMqiConnxProfile, HostProblem> {
        let frame = self
            .0
            .lock()
            .map_err(|_| HostProblem::UnknownOutcome)?
            .clone()
            .ok_or(HostProblem::Unauthorized)?;
        frame::Observation(frame).connx_profile(original)
    }
}

struct Drive<'a> {
    router: &'a DefaultProgramRouter,
    mq: &'a Arc<ConfiguredInstalledMqHost>,
    admitted: &'a AdmittedProgram,
    original: &'a Invocation,
    setup: [u8; 32],
    root: Option<Arc<MqTrustedBatchRoot>>,
}
struct Hooks<'a, 'b> {
    drive: &'a mut Drive<'b>,
    slot: Arc<Mutex<Option<Arc<ClosedFrame>>>>,
}

impl NativeRootHooks for Hooks<'_, '_> {
    fn admitted(&mut self, proof: &NativeRootAdmission<'_>) -> Result<(), HostProblem> {
        let d = &mut self.drive;
        if proof.original() != d.original
            || !Arc::ptr_eq(proof.store(), &d.mq.store)
            || proof.claim().admission().configuration_digest != d.setup
        {
            return Err(HostProblem::Unauthorized);
        }
        let current = d
            .router
            .cobol
            .preflight_installed_program(&d.admitted.name, true)?;
        if current.artifact != d.admitted.artifact
            || current.metadata != d.admitted.metadata
            || current.executable.content_id() != d.admitted.executable.content_id()
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let AdmittedProgramProvenance::Catalog(catalog) = &d.admitted.provenance else {
            return Err(HostProblem::Unauthorized);
        };
        if !matches!(current.provenance, AdmittedProgramProvenance::Catalog(ref now) if now == catalog)
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let charge = budget::charge(d.original, d.mq.host_limits.max_state_bytes)?;
        {
            let mut map =
                d.mq.topology
                    .lock()
                    .map_err(|_| HostProblem::UnknownOutcome)?;
            if map.roots.contains_key(&d.original.execution_id)
                || map.roots.len() >= d.mq.bounds.max_roots
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            map.reserve_bytes(charge, d.mq.host_limits.max_state_bytes)?;
            map.roots
                .insert(d.original.execution_id.clone(), RootEntry::Preparing);
        }
        // Failure keeps the reserved ownership; it is never a fresh root.
        let root = Arc::new(d.mq.runtime.admit_root(d.original.clone())?);
        let frame = Arc::new(ClosedFrame::new(
            root.frame(),
            d.mq.control.clone(),
            proof.claim().admission().event.tick,
        ));
        {
            let mut map =
                d.mq.topology
                    .lock()
                    .map_err(|_| HostProblem::UnknownOutcome)?;
            map.roots.insert(
                d.original.execution_id.clone(),
                RootEntry::Retained {
                    root: root.clone(),
                    frame: frame.clone(),
                    native: Some(proof.claim().clone()),
                },
            );
        }
        d.root = Some(root);
        *self.slot.lock().map_err(|_| HostProblem::UnknownOutcome)? = Some(frame);
        Ok(())
    }
    fn settle(
        &mut self,
        winner: &WinningRootTerminal<'_>,
    ) -> Result<RootTerminalCommit, HostProblem> {
        let d = &self.drive;
        if winner.admission().original() != d.original
            || !Arc::ptr_eq(winner.admission().store(), &d.mq.store)
            || winner.closure().claim.admission().configuration_digest != d.setup
        {
            return Err(HostProblem::Unauthorized);
        }
        let root = d.root.as_ref().ok_or(HostProblem::UnknownOutcome)?;
        let AdmittedProgramProvenance::Catalog(catalog) = &d.admitted.provenance else {
            return Err(HostProblem::Unauthorized);
        };
        if !winner
            .closure()
            .provider_dependencies
            .iter()
            .any(|row| matches!(row, TerminalRowDependency::Exact(now) if now == catalog))
        {
            return Err(HostProblem::UnknownOutcome);
        }
        // Re-admit the exact compiled provenance before taking the MQ mutex.
        // Map guards are released before artifact/catalog validation callbacks.
        let root_now = d
            .router
            .cobol
            .preflight_installed_program(&d.admitted.name, true)?;
        if root_now.metadata != d.admitted.metadata
            || root_now.executable.content_id() != d.admitted.executable.content_id()
            || !matches!(root_now.provenance, AdmittedProgramProvenance::Catalog(ref now) if now == catalog)
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let frames = {
            let map =
                d.mq.topology
                    .lock()
                    .map_err(|_| HostProblem::UnknownOutcome)?;
            winner
                .closure()
                .actors
                .iter()
                .skip(1)
                .map(
                    |actor| match map.frames.get(&actor.execution.execution_id) {
                        Some(FrameEntry::Retained(frame)) => Ok(frame.clone()),
                        _ => Err(HostProblem::UnknownOutcome),
                    },
                )
                .collect::<Result<Vec<_>, _>>()?
        };
        for (actor, frame) in winner.closure().actors.iter().skip(1).zip(&frames) {
            let compiled = frame.compiled().ok_or(HostProblem::UnknownOutcome)?;
            let call = actor.call.as_ref().ok_or(HostProblem::UnknownOutcome)?;
            if compiled.catalog != call.catalog {
                return Err(HostProblem::UnknownOutcome);
            }
            let current = d
                .router
                .cobol
                .preflight_installed_program(&compiled.catalog.key, true)?;
            if current.artifact != actor.execution.artifact
                || current.metadata != compiled.metadata
                || current.executable.content_id().as_bytes() != &compiled.content
                || !matches!(current.provenance, AdmittedProgramProvenance::Catalog(ref now) if now == &compiled.catalog)
            {
                return Err(HostProblem::UnknownOutcome);
            }
        }
        let prepared =
            root.prepare_native_terminal(winner.closure(), winner.termination().disposition())?;
        let mut plan = winner
            .publication_plan(prepared.observed_tick())
            .map_err(|_| HostProblem::UnknownOutcome)?;
        plan.mutations.extend_from_slice(prepared.mutations());
        // Pure shared COBOL parser/planner: no callback, reads or publication
        // under the MQ mutex. Its age uses the same actual final observation.
        plan.mutations.extend(d.router.cobol.prepare_native_run_end(
            d.original,
            winner.closure(),
            plan.observed_tick,
        )?);
        for mutation in &plan.mutations {
            let (namespace, key) = match mutation {
                ProviderStateMutation::Put(write) => (&write.record.namespace, &write.record.key),
                ProviderStateMutation::Move { record, .. } => (&record.namespace, &record.key),
                ProviderStateMutation::Delete { .. } => continue,
            };
            if !plan.closure.provider_dependencies.iter().any(|dependency| match dependency {
                TerminalRowDependency::Exact(row) => row.namespace == *namespace && row.key == *key,
                TerminalRowDependency::Absent { namespace: old, key: old_key } => old == namespace && old_key == key,
            }) && !plan.dependencies.iter().any(|dependency| matches!(dependency,
                TerminalRowDependency::Absent { namespace: old, key: old_key } if old == namespace && old_key == key)) {
                // Complete scoped capture + exact physical epoch establishes
                // observed absence; the final transaction rechecks it explicitly.
                if !plan.closure.claim.admission().provider_namespaces.contains(namespace) {
                    return Err(HostProblem::UnknownOutcome);
                }
                plan.dependencies.push(TerminalRowDependency::Absent { namespace: namespace.clone(), key: key.clone() });
            }
        }
        plan.validate_bounds()
            .map_err(|_| HostProblem::ResourceExhausted)?;
        let rows: Vec<_> = plan
            .closure
            .core_records
            .iter()
            .map(exact)
            .chain(plan.closure.provider_dependencies.iter().map(dependency))
            .chain(plan.dependencies.iter().map(dependency))
            .chain(plan.mutations.iter().map(mutation))
            .collect();
        let machine = match winner.termination() {
            NativeRootTermination::Completed(value) => {
                RootTerminalMachineObservation::Completed(value)
            }
            NativeRootTermination::Abended(value) => RootTerminalMachineObservation::Abended(value),
        };
        let subject = canonical_root_terminal_resource_digest(
            &RootTerminalResource {
                execution: &d.original.execution_id,
                run: &d.original.run_unit_id,
                principal: d.original.principal.id(),
                invocation_key: &d.original.idempotency_key,
                attempt: d.original.attempt,
                lifecycle_sequence: plan.steps[0].event.sequence,
                observed_tick: plan.observed_tick,
                configuration_digest: &d.setup,
                provider_epoch: plan.closure.provider_epoch,
                closing_version: plan.closure.closing.version,
                closing_payload: &plan.closure.closing.payload,
                disposition: plan.disposition,
                machine,
                rows: &rows,
            },
            mainframe_env_host_api::MAX_CANONICAL_EFFECT_BYTES,
        )?;
        plan.audits = [
            RootTerminalAuditRole::ProviderSettlement,
            RootTerminalAuditRole::CoreClosure,
        ]
        .into_iter()
        .map(|role| RootTerminalAudit {
            role,
            execution_id: d.original.execution_id.clone(),
            run_unit_id: d.original.run_unit_id.clone(),
            attempt: d.original.attempt,
            lifecycle_sequence: plan.steps[0].event.sequence,
            observed_tick: plan.observed_tick,
            principal: d.original.principal.id().clone(),
            invocation_key: d.original.idempotency_key.clone(),
            capability: d.mq.descriptor.capability.clone(),
            resource: subject,
            decision: AuditDecision::Success,
        })
        .collect();
        prepared.publish(plan)
    }
    fn retain_uncertain(
        &mut self,
        original: &Invocation,
        _claim: Option<&RootDriverClaim>,
    ) -> Result<(), HostProblem> {
        if original != self.drive.original {
            return Err(HostProblem::Unauthorized);
        }
        if let Some(root) = &self.drive.root {
            root.retain_native_uncertain()?;
        }
        Err(HostProblem::UnknownOutcome)
    }
}
