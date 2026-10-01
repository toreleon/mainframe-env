//! Exact-artifact BRXA launch for a local START BREXIT request.

use super::{JesWorkOutcome, ProductServer, store_error};
use mainframe_env_cics::{
    BrxaEndFrame, BrxaInitFrame, CicsBridgeAbiProfile, CicsBridgeExitDefault, CicsBridgeRuntime,
    CicsBridgeStartIntent, CicsStartTask,
};
use mainframe_env_execution_api::{
    ArtifactRef, BoundedPayload, ExecutionId, IdempotencyKey, Invocation, InvocationLimits,
    RunUnitId,
};
use mainframe_env_host_api::{
    EffectRequest, HostLimits, HostProblem, HostProvider, HostRequest, HostResult,
    ProgramLinkSelection, ProgramName, ProgramRequest, SessionId,
};
use mainframe_env_ir::{CodecLimits, decode_binary};
use mainframe_env_store_api::{ArtifactStore, ExecutionState, WorkRecord};
use std::collections::BTreeMap;
use std::num::NonZeroU32;

impl ProductServer {
    /// Bind a transaction's default exit to an installed local program.
    pub fn register_bridge_exit_defaults(
        &self,
        definitions: &[CicsBridgeExitDefault],
    ) -> Result<(), HostProblem> {
        self.cics.register_bridge_exit_defaults(definitions)
    }

    /// Bind deployment reviewed BRXA constants to an installed exit artifact.
    pub fn register_bridge_abi_profiles(
        &self,
        profiles: &[CicsBridgeAbiProfile],
    ) -> Result<(), HostProblem> {
        self.cics.register_bridge_abi_profiles(profiles)
    }

    pub(super) fn launch_bridge_task(
        &self,
        work: &WorkRecord,
        intent: &CicsBridgeStartIntent,
        now_tick: u64,
    ) -> Result<JesWorkOutcome, HostProblem> {
        let target = CicsStartTask {
            request_id: intent.request_id.clone(),
            transaction: intent.transaction.clone(),
            principal: intent.principal.clone(),
            terminal: None,
            attached: false,
        };
        let artifact = ArtifactRef::new(&intent.target_artifact, InvocationLimits::default())
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let target_invocation =
            super::interval_wakeup::started_task_invocation(work, &target, artifact)?;
        let runtime = match self.cics.bridge_runtime(&intent.request_id) {
            Ok(runtime) => runtime,
            Err(HostProblem::InfrastructureFailure) => {
                self.initialize_bridge(intent, &target_invocation)?
            }
            Err(problem) => return Err(problem),
        };
        if runtime.run_unit != target_invocation.run_unit_id.as_str()
            || runtime.principal != intent.principal
            || runtime.artifact != intent.exit_artifact
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        let outcome = self.launch_started_task(
            work,
            &target,
            now_tick,
            Some((&intent.target_program, &intent.target_artifact)),
        )?;
        if matches!(outcome, JesWorkOutcome::Completed) {
            let session = SessionId::new(
                format!("cics-start-task-{}", work.execution_id),
                InvocationLimits::default().max_binding_bytes,
            )
            .map_err(|_| HostProblem::InfrastructureFailure)?;
            if self.online_exchange(&session)?.is_none() {
                let execution = self
                    .store
                    .get_execution(&work.execution_id)
                    .map_err(store_error)?;
                if let Some(execution) = execution.filter(|execution| execution.state.terminal()) {
                    let end = BrxaEndFrame::new(
                        &runtime.bound_frame,
                        execution.state != ExecutionState::Completed,
                    )?;
                    let parent = bridge_parent(&target_invocation, &intent.request_id)?;
                    let reply = self.call_bridge(&parent, intent, 3, end.bytes())?;
                    end.validate_reply(&reply)?;
                    self.cics.release_bridge_runtime(&intent.request_id)?;
                }
            }
        }
        Ok(outcome)
    }

    fn initialize_bridge(
        &self,
        intent: &CicsBridgeStartIntent,
        target: &Invocation,
    ) -> Result<CicsBridgeRuntime, HostProblem> {
        let limits = InvocationLimits::default();
        let artifact = ArtifactRef::new(&intent.exit_artifact, limits)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let record = self
            .artifacts
            .get_artifact(&artifact)
            .map_err(store_error)?
            .ok_or(HostProblem::NotFound)?;
        let admitted = crate::cobol::artifact::admit_executable_artifact(&record)?;
        let module = decode_binary(admitted.payload(), CodecLimits::default())
            .map_err(|_| HostProblem::ProviderFailure)?;
        let mut base = 0u32;
        let mut views = BTreeMap::<mainframe_env_ir::StorageId, (u32, usize, usize)>::new();
        let mut commarea = None;
        for region in module.storage() {
            let (index, offset, size) = if let Some(alias) = &region.alias_of {
                let (index, parent_offset, _) = views
                    .get(&alias.storage)
                    .copied()
                    .ok_or(HostProblem::ProviderFailure)?;
                (
                    index,
                    parent_offset
                        + usize::try_from(alias.offset)
                            .map_err(|_| HostProblem::ResourceExhausted)?,
                    usize::try_from(alias.length).map_err(|_| HostProblem::ResourceExhausted)?,
                )
            } else {
                base = base.checked_add(1).ok_or(HostProblem::ResourceExhausted)?;
                (
                    base,
                    0,
                    usize::try_from(region.size).map_err(|_| HostProblem::ResourceExhausted)?,
                )
            };
            views.insert(region.id, (index, offset, size));
            if region.name.eq_ignore_ascii_case("DFHCOMMAREA") {
                let address = index
                    .checked_shl(20)
                    .and_then(|base| base.checked_add(u32::try_from(offset).ok()?))
                    .ok_or(HostProblem::ResourceExhausted)?;
                commarea = Some((address, size));
            }
        }
        let (commarea_address, capacity) = commarea.ok_or(HostProblem::ProviderFailure)?;
        let transid = |value: &str| -> Result<[u8; 4], HostProblem> {
            let mut bytes = [b' '; 4];
            let value = value.as_bytes();
            if value.is_empty() || value.len() > 4 {
                return Err(HostProblem::Malformed);
            }
            bytes[..value.len()].copy_from_slice(value);
            Ok(bytes)
        };
        let mut userid = [b' '; 8];
        let user = intent.principal.as_bytes();
        if user.len() > userid.len() {
            return Err(HostProblem::Malformed);
        }
        userid[..user.len()].copy_from_slice(user);
        let init = BrxaInitFrame::new(
            commarea_address,
            capacity,
            NonZeroU32::new(intent.abi_version).ok_or(HostProblem::Malformed)?,
            transid(&intent.bridge_transaction)?,
            transid(&intent.transaction)?,
            userid,
            &intent.data,
        )?;
        let parent = bridge_parent(target, &intent.request_id)?;
        let init_returned = self.call_bridge(&parent, intent, 1, init.bytes())?;
        let init_reply = init.validate_reply(&init_returned)?;
        if init_reply.user_abend_code != *b"    " {
            return Err(HostProblem::ProviderFailure);
        }
        let bind = init.bind(&init_returned, intent.bind_code)?;
        let bound = self.call_bridge(&parent, intent, 2, bind.bytes())?;
        let bind_reply = bind.validate_reply(&bound)?;
        if bind_reply.user_abend_code != *b"    " {
            return Err(HostProblem::ProviderFailure);
        }
        let runtime = CicsBridgeRuntime::new(
            intent.request_id.clone(),
            target.run_unit_id.as_str().into(),
            intent.principal.clone(),
            intent.exit.clone(),
            intent.exit_artifact.clone(),
            intent.abi_version,
            intent.bind_code,
            commarea_address,
            capacity.min(32_767),
            bound,
            bind_reply.start_code,
        )?;
        self.cics.register_bridge_runtime(runtime.clone())?;
        Ok(runtime)
    }

    fn call_bridge(
        &self,
        parent: &Invocation,
        intent: &CicsBridgeStartIntent,
        sequence: u64,
        bytes: &[u8],
    ) -> Result<Vec<u8>, HostProblem> {
        let limits = InvocationLimits::default();
        let payload = BoundedPayload::new("mainframe-env.cics.brxa@1", bytes.to_vec(), limits)
            .map_err(|_| HostProblem::ResourceExhausted)?;
        let artifact = ArtifactRef::new(&intent.exit_artifact, limits)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let request = HostRequest::Program(ProgramRequest::Link {
            program: ProgramName::new(&intent.exit, HostLimits::default().max_name_bytes)
                .map_err(|_| HostProblem::Malformed)?,
            payload,
            selection: Some(ProgramLinkSelection {
                artifact,
                generation: 1,
                content_identity: intent.exit_artifact.clone(),
            }),
        });
        let result = self.program.invoke(
            parent,
            EffectRequest {
                run_unit: parent.run_unit_id.clone(),
                sequence,
                deadline_tick: parent.deadline_tick,
                idempotency_key: Some(
                    IdempotencyKey::new(format!("bridge-{}-{sequence}", intent.request_id), limits)
                        .map_err(|_| HostProblem::InfrastructureFailure)?,
                ),
                request,
            },
        );
        match result.outcome? {
            HostResult::Program(payload) => Ok(payload.bytes().to_vec()),
            _ => Err(HostProblem::ProviderFailure),
        }
    }

    pub(super) fn restore_bridge_binding(
        &self,
        invocation: &mut Invocation,
    ) -> Result<(), HostProblem> {
        let Some(request_id) = invocation
            .run_unit_id
            .as_str()
            .strip_prefix("run-cics-bridge-")
        else {
            return Ok(());
        };
        let runtime = self.cics.bridge_runtime(request_id)?;
        if runtime.run_unit != invocation.run_unit_id.as_str()
            || runtime.principal != invocation.principal.id().as_str()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        invocation.bindings.insert(
            "cics.bridge-request".into(),
            BoundedPayload::new(
                "mainframe-env.cics.bridge-request@1",
                request_id.as_bytes().to_vec(),
                InvocationLimits::default(),
            )
            .map_err(|_| HostProblem::ResourceExhausted)?,
        );
        invocation.bindings.insert(
            "cics.start-code".into(),
            BoundedPayload::new(
                "mainframe-env.cics.start-code@1",
                runtime.start_code.to_vec(),
                InvocationLimits::default(),
            )
            .map_err(|_| HostProblem::ResourceExhausted)?,
        );
        Ok(())
    }
}

fn bridge_parent(target: &Invocation, request_id: &str) -> Result<Invocation, HostProblem> {
    let limits = InvocationLimits::default();
    let mut parent = target.clone();
    parent.execution_id = ExecutionId::new(format!("bridge-exit-{request_id}"), limits)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    parent.run_unit_id = RunUnitId::new(format!("run-bridge-exit-{request_id}"), limits)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    Ok(parent)
}
