//! Live lease validation before a manager-owned same-level disposition.
//! Busy legacy rows cannot reconstruct this lease after restart.
use super::*;

#[derive(Debug, PartialEq)]
#[allow(
    dead_code,
    reason = "read-only handoff prerequisite; manager owns runtime wiring"
)]
pub(in crate::cobol) struct TransferInstanceSnapshot {
    pub(in crate::cobol) run_version: u64,
    pub(in crate::cobol) source_version: u64,
    pub(in crate::cobol) target_version: Option<u64>,
}

impl Lease {
    /// Returns observed CAS tokens only. Does not release/acquire an instance,
    /// invent terminal source proof, close files, or authorize cold recovery.
    #[allow(
        dead_code,
        reason = "manager must serialize CICS admission and handoff first"
    )]
    pub(in crate::cobol) fn attest_transfer_instances(
        &self,
        store: &dyn PlatformStore,
        source: &Invocation,
        machine: &ReferenceMachine,
        target: &Invocation,
        executable: &mainframe_env_compiler_api::ValidatedArtifact,
    ) -> Result<TransferInstanceSnapshot, HostProblem> {
        let source_name = source
            .selector
            .as_str()
            .strip_prefix("program:")
            .ok_or(HostProblem::UnknownOutcome)?;
        let target_name = target
            .selector
            .as_str()
            .strip_prefix("program:")
            .ok_or(HostProblem::UnknownOutcome)?;
        if self.invocation != *source
            || self.run != run_key(source)
            || self.namespace != namespace(&self.run)
            || self.name != source_name
            || !valid_program(target_name)
            || target_name == source_name
            || target.parent_execution_id.as_ref() != Some(&source.execution_id)
            || target.run_unit_id != source.run_unit_id
            || target.principal != source.principal
            || target.provider_generations != source.provider_generations
            || target.cancellation != source.cancellation
            || target.cancellation_probe != source.cancellation_probe
            || target.deadline_tick > source.deadline_tick
            || target.limits.max_frames > source.limits.max_frames
            || target.limits.max_steps > source.limits.max_steps
            || target.limits.max_storage_bytes > source.limits.max_storage_bytes
            || target.limits.max_output_bytes > source.limits.max_output_bytes
            || target.limits.max_effects > source.limits.max_effects
            || target.limits.max_events > source.limits.max_events
            || super::super::replay::protocol_owner_execution(target)?
                != super::super::replay::protocol_owner_execution(source)?
            || !machine.dataset_cursors().is_empty()
            || target.artifact.as_str() != executable.content_id().to_reference()
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let (state, version) =
            load_run(store, &self.run).map_err(|_| HostProblem::UnknownOutcome)?;
        if state.schema_version != 2
            || state.ended
            || state.active == 0
            || !state.programs.contains(source_name)
            || state.owner_execution.as_deref()
                != Some(super::super::replay::protocol_owner_execution(source)?.as_str())
            || state.owner_run_unit.as_deref() != Some(source.run_unit_id.as_str())
            || state.owner_principal.as_deref() != Some(source.principal.id().as_str())
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let rows = store
            .list_provider_state(&self.namespace, MAX_INSTANCES + 1)
            .map_err(|_| HostProblem::UnknownOutcome)?;
        if rows.len() != state.instances {
            return Err(HostProblem::UnknownOutcome);
        }
        let mut active = 0;
        let mut source_found = false;
        let mut target_version = None;
        let mut target_state = None;
        for row in rows {
            let instance = load_instance(&row)?;
            if !state.programs.contains(&row.key) {
                return Err(HostProblem::UnknownOutcome);
            }
            active += usize::from(instance.busy);
            if row.key == self.name {
                if row.version != self.version
                    || !instance.busy
                    || instance.open_files
                    || instance.abend.is_some()
                    || instance.artifact != source.artifact.as_str()
                    || serde_json::to_vec(&instance).map_err(|_| HostProblem::UnknownOutcome)?
                        != serde_json::to_vec(&self.instance)
                            .map_err(|_| HostProblem::UnknownOutcome)?
                {
                    return Err(HostProblem::UnknownOutcome);
                }
                source_found = true;
            }
            if row.key == target_name {
                if instance.busy
                    || instance.open_files
                    || instance.abend.is_some()
                    || !instance.artifact.is_empty()
                        && instance.artifact != target.artifact.as_str()
                {
                    return Err(HostProblem::UnknownOutcome);
                }
                target_version = Some(row.version);
                target_state = instance.state;
            }
        }
        if !source_found
            || active != state.active
            || target_version.is_none() && state.instances >= MAX_INSTANCES
        {
            return Err(HostProblem::UnknownOutcome);
        }
        // Validate last-used storage on a disposable local constructor. Neither
        // the staged constructor nor either durable instance is changed here.
        let mut target_machine = ReferenceMachine::from_binary(
            executable.payload(),
            target.clone(),
            CodecLimits::default(),
        )
        .map_err(|_| HostProblem::UnknownOutcome)?;
        let initial = target_machine
            .installed_call_is_initial()
            .map_err(|_| HostProblem::UnknownOutcome)?;
        if !initial && let Some(state) = target_state {
            target_machine
                .install_retained_program_state(&state)
                .map_err(|_| HostProblem::UnknownOutcome)?;
        }
        if !target_machine.dataset_cursors().is_empty() {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(TransferInstanceSnapshot {
            run_version: version.ok_or(HostProblem::UnknownOutcome)?,
            source_version: self.version,
            target_version,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cobol::hardening::{Fixture, TestRoot, parent};
    use mainframe_env_store::MemoryStore;

    #[test]
    fn transfer_instance_attestation_keeps_live_source_busy_and_never_acquires_target() {
        let root = TestRoot::new();
        let fixture = Fixture::new(
            &root,
            Arc::new(MemoryStore::new(Default::default())),
            HostProblem::NotFound,
            false,
        );
        fixture.install(
            "MID",
            "IDENTIFICATION DIVISION. PROGRAM-ID. MID. PROCEDURE DIVISION. GOBACK.",
        );
        let admitted = fixture
            .router
            .cobol
            .preflight_installed_program("MID", false)
            .unwrap();
        let mut source = parent();
        source.selector = Selector::new("program:MID", InvocationLimits::default()).unwrap();
        source.artifact = admitted.artifact;
        let mut machine = ReferenceMachine::from_binary(
            admitted.executable.payload(),
            source.clone(),
            CodecLimits::default(),
        )
        .unwrap();
        let lease = Lease::acquire(fixture.store.as_ref(), &source, "MID", &mut machine).unwrap();
        let mut target = source.clone();
        target.execution_id =
            ExecutionId::new("target-execution", InvocationLimits::default()).unwrap();
        target.parent_execution_id = Some(source.execution_id.clone());
        target.selector = Selector::new("program:EXIT", InvocationLimits::default()).unwrap();
        super::super::super::replay::bind_protocol_owner(&source, &mut target.bindings).unwrap();
        let run = fixture
            .store
            .get_provider_state(RUN_STATE_NAMESPACE, &lease.run)
            .unwrap()
            .unwrap();
        let rows = fixture
            .store
            .list_provider_state(&lease.namespace, 8)
            .unwrap();
        for case in 0..6 {
            let mut forged = source.clone();
            match case {
                0 => {
                    forged.execution_id =
                        ExecutionId::new("foreign-source", InvocationLimits::default()).unwrap()
                }
                1 => forged.attempt += 1,
                2 => forged.priority += 1,
                3 => forged.audit_correlation = "foreign-audit".into(),
                4 => forged.deadline_tick -= 1,
                _ => {
                    forged.cancellation_probe =
                        Some(mainframe_env_execution_api::CancellationProbe::default())
                }
            }
            assert_eq!(
                lease.attest_transfer_instances(
                    fixture.store.as_ref(),
                    &forged,
                    &machine,
                    &target,
                    &admitted.executable
                ),
                Err(HostProblem::UnknownOutcome),
                "live source {case}",
            );
            assert_eq!(
                fixture
                    .store
                    .get_provider_state(RUN_STATE_NAMESPACE, &lease.run)
                    .unwrap(),
                Some(run.clone())
            );
            assert_eq!(
                fixture
                    .store
                    .list_provider_state(&lease.namespace, 8)
                    .unwrap(),
                rows
            );
        }
        assert_eq!(
            lease
                .attest_transfer_instances(
                    fixture.store.as_ref(),
                    &source,
                    &machine,
                    &target,
                    &admitted.executable
                )
                .unwrap(),
            TransferInstanceSnapshot {
                run_version: run.version,
                source_version: rows[0].version,
                target_version: None
            }
        );
        for case in 0..4 {
            let mut forged = target.clone();
            match case {
                0 => forged.parent_execution_id = None,
                1 => {
                    forged.run_unit_id =
                        RunUnitId::new("foreign-run", InvocationLimits::default()).unwrap()
                }
                2 => forged.selector = source.selector.clone(),
                _ => {
                    forged.bindings.remove("cobol.run-owner-execution");
                }
            }
            assert_eq!(
                lease.attest_transfer_instances(
                    fixture.store.as_ref(),
                    &source,
                    &machine,
                    &forged,
                    &admitted.executable
                ),
                Err(HostProblem::UnknownOutcome),
                "owner {case}"
            );
        }
        assert_eq!(
            fixture
                .store
                .get_provider_state(RUN_STATE_NAMESPACE, &lease.run)
                .unwrap(),
            Some(run.clone())
        );
        assert_eq!(
            fixture
                .store
                .list_provider_state(&lease.namespace, 8)
                .unwrap(),
            rows
        );
        let mut state = decode_run_state(&run).unwrap();
        state.instances += 1;
        state.programs.insert("EXIT".into());
        refresh_run_metadata(&mut state, &lease.run);
        let idle = Instance {
            schema_version: 1,
            artifact: target.artifact.as_str().into(),
            busy: false,
            open_files: false,
            state: None,
            abend: None,
        };
        fixture
            .store
            .put_provider_states_atomic(vec![
                write(RUN_STATE_NAMESPACE, &lease.run, &state, Some(run.version)).unwrap(),
                write(&lease.namespace, "EXIT", &idle, None).unwrap(),
            ])
            .unwrap();
        assert_eq!(
            lease
                .attest_transfer_instances(
                    fixture.store.as_ref(),
                    &source,
                    &machine,
                    &target,
                    &admitted.executable
                )
                .unwrap()
                .target_version,
            Some(1)
        );
        for case in 0..5 {
            let mut value = Instance {
                schema_version: 1,
                artifact: idle.artifact.clone(),
                busy: false,
                open_files: false,
                state: None,
                abend: None,
            };
            match case {
                0 => value.busy = true,
                1 => value.open_files = true,
                2 => value.artifact = format!("sha256:{}", "f".repeat(64)),
                3 => value.state = Some(b"corrupt-retained-state".to_vec()),
                _ => value.schema_version = 2,
            }
            let row = fixture
                .store
                .get_provider_state(&lease.namespace, "EXIT")
                .unwrap()
                .unwrap();
            fixture
                .store
                .put_provider_state(
                    ProviderStateRecord {
                        version: row.version + 1,
                        payload: serde_json::to_vec(&value).unwrap(),
                        ..row.clone()
                    },
                    Some(row.version),
                )
                .unwrap();
            let before = fixture
                .store
                .list_provider_state(&lease.namespace, 8)
                .unwrap();
            assert_eq!(
                lease.attest_transfer_instances(
                    fixture.store.as_ref(),
                    &source,
                    &machine,
                    &target,
                    &admitted.executable
                ),
                Err(HostProblem::UnknownOutcome),
                "target {case}"
            );
            assert_eq!(
                fixture
                    .store
                    .list_provider_state(&lease.namespace, 8)
                    .unwrap(),
                before
            );
        }
        let row = fixture
            .store
            .get_provider_state(&lease.namespace, "EXIT")
            .unwrap()
            .unwrap();
        fixture
            .store
            .put_provider_state(
                ProviderStateRecord {
                    version: row.version + 1,
                    payload: serde_json::to_vec(&idle).unwrap(),
                    ..row.clone()
                },
                Some(row.version),
            )
            .unwrap();
        // A later CAS invalidates the original live lease even with equal bytes.
        fixture
            .store
            .put_provider_state(
                ProviderStateRecord {
                    version: rows[0].version + 1,
                    ..rows[0].clone()
                },
                Some(rows[0].version),
            )
            .unwrap();
        let stale = fixture
            .store
            .list_provider_state(&lease.namespace, 8)
            .unwrap();
        assert_eq!(
            lease.attest_transfer_instances(
                fixture.store.as_ref(),
                &source,
                &machine,
                &target,
                &admitted.executable
            ),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(
            fixture
                .store
                .list_provider_state(&lease.namespace, 8)
                .unwrap(),
            stale
        );
    }
}
