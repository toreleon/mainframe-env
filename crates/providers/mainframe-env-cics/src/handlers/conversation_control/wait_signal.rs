//! Principal logical-unit WAIT SIGNAL selected over the shared durable ledger.

use super::{
    ConversationLedger, ConversationOwner, ConversationProblem, ConversationReplay,
    ConversationReply, SignalLuType, load_conversation_replay,
};
use crate::service::{CicsService, Run, mutation_problem, store_error};
use mainframe_env_execution_api::{AuditDecision, AuditRecord, InvocationLimits};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
    HostRequest, canonical_audit_resource_digest, canonical_request_digest,
};
use std::collections::BTreeMap;

const MAX_CAS_RETRIES: usize = 32;

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    let result = invoke_inner(service, run, request, retention_tick);
    let decision = match &result {
        Ok(_) => AuditDecision::Success,
        Err(HostProblem::Unauthorized) => AuditDecision::Deny,
        Err(HostProblem::Cancelled) => AuditDecision::Cancelled,
        Err(HostProblem::TimedOut) => AuditDecision::TimedOut,
        Err(HostProblem::UnknownOutcome) => AuditDecision::UnknownOutcome,
        Err(HostProblem::InfrastructureFailure) => AuditDecision::InfrastructureFailure,
        Err(_) => AuditDecision::ProviderFailure,
    };
    audit(service, run, request, decision)?;
    result
}

fn invoke_inner(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    if request.operation != CicsOperation::WaitSignal
        || request.mutation.is_none()
        || request
            .arguments
            .keys()
            .any(|name| !matches!(name.as_str(), "RESP" | "RESP2" | "OPTION.NOHANDLE"))
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
    {
        return Err(HostProblem::Malformed);
    }
    if super::context(run)? == super::ConversationContext::DplServer {
        return Err(condition("NOTALLOC", 61));
    }
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let effect_key = mutation.idempotency_key.as_str();
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let owner = ConversationOwner {
        execution: run.invocation.execution_id.as_str().into(),
        run_unit: run.invocation.run_unit_id.as_str().into(),
        lease_epoch: u64::from(run.invocation.attempt),
    };
    let principal = run.invocation.principal.id().as_str().to_owned();
    if let Some(saved) =
        load_conversation_replay(service.store.as_ref(), effect_key).map_err(store_error)?
    {
        let reply = saved
            .matches_request(
                &owner.execution,
                &owner.run_unit,
                &principal,
                owner.lease_epoch,
                mutation.sequence,
                digest,
            )
            .map_err(store_error)?;
        return response(service, run, reply);
    }
    service.authorize(run, "FACILITY", "CICS.TERMINAL.SIGNAL", AccessIntent::Read)?;
    for _ in 0..MAX_CAS_RETRIES {
        super::deadline(service, run)?;
        let current = ConversationLedger::load(service.store.as_ref()).map_err(store_error)?;
        let pending = current.signal_pending(&owner).map_err(signal_problem)?;
        if !pending {
            return service.response(
                run,
                CicsDisposition::Suspended,
                "NORMAL",
                0,
                0,
                None,
                None,
                Vec::new(),
            );
        }
        let mut next = current.clone();
        if !next.consume_signal(&owner).map_err(signal_problem)? {
            continue;
        }
        let delivered = super::super::condition::respond(
            service,
            run,
            &request.condition_policy,
            signal_condition(),
        )?;
        let disposition = match delivered.disposition {
            CicsDisposition::Complete => b"C".to_vec(),
            CicsDisposition::Ignored => b"I".to_vec(),
            CicsDisposition::Handler => b"H".to_vec(),
            _ => return Err(HostProblem::InfrastructureFailure),
        };
        let mut outputs = BTreeMap::from([("SIGNAL_DISPOSITION".into(), disposition)]);
        if let Some(target) = delivered.target.as_ref() {
            outputs.insert("SIGNAL_TARGET".into(), target.as_bytes().to_vec());
        }
        let reply = ConversationReply {
            condition: "SIGNAL".into(),
            response: 24,
            response2: 0,
            state: None,
            token: None,
            outputs,
        };
        let replay = ConversationReplay {
            schema_version: 1,
            effect_key: effect_key.into(),
            owner_execution: owner.execution.clone(),
            owner_run_unit: owner.run_unit.clone(),
            owner_principal: principal.clone(),
            owner_epoch: owner.lease_epoch,
            mutation_sequence: mutation.sequence,
            request_digest: digest,
            deadline_tick: retention_tick,
            retain_until_tick: retention_tick,
            reply: reply.clone(),
        };
        if current
            .persist_with_replay(&mut next, &replay, service.store.as_ref())
            .map_err(|error| mutation_problem(store_error(error)))?
        {
            if run.invocation.cancellation_requested() || super::deadline(service, run).is_err() {
                return Err(HostProblem::UnknownOutcome);
            }
            return response(service, run, &reply);
        }
    }
    Err(HostProblem::UnknownOutcome)
}

fn response(
    service: &CicsService,
    run: &Run,
    reply: &ConversationReply,
) -> Result<CicsResponse, HostProblem> {
    if reply.condition != "SIGNAL" || reply.response != 24 || reply.response2 != 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let disposition = match reply.outputs.get("SIGNAL_DISPOSITION").map(Vec::as_slice) {
        Some(b"C") => CicsDisposition::Complete,
        Some(b"I") => CicsDisposition::Ignored,
        Some(b"H") => CicsDisposition::Handler,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let target = reply
        .outputs
        .get("SIGNAL_TARGET")
        .map(|bytes| {
            String::from_utf8(bytes.clone()).map_err(|_| HostProblem::InfrastructureFailure)
        })
        .transpose()?;
    if (disposition == CicsDisposition::Handler) != target.is_some() {
        return Err(HostProblem::InfrastructureFailure);
    }
    service.response(run, disposition, "SIGNAL", 24, 0, target, None, Vec::new())
}

fn signal_problem(problem: ConversationProblem) -> HostProblem {
    match problem {
        ConversationProblem::NotOwned => condition("NOTALLOC", 61),
        ConversationProblem::WrongState => condition("TERMERR", 81),
        ConversationProblem::StaleOwner => HostProblem::IdempotencyConflict,
        _ => HostProblem::InfrastructureFailure,
    }
}

fn signal_condition() -> HostProblem {
    condition("SIGNAL", 24)
}

fn condition(name: &str, response: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2: 0,
    }
}

fn audit(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    decision: AuditDecision,
) -> Result<(), HostProblem> {
    run.host_sequence = run
        .host_sequence
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    let host_request = HostRequest::Cics(request.clone());
    service
        .store
        .record_audit(AuditRecord {
            execution_id: run.invocation.execution_id.clone(),
            run_unit_id: run.invocation.run_unit_id.clone(),
            attempt: run.invocation.attempt,
            effect_sequence: run.host_sequence,
            observed_tick: run.invocation.deadline_tick.saturating_sub(1),
            principal: run.invocation.principal.id().clone(),
            invocation_key: run.invocation.idempotency_key.clone(),
            capability: host_request.required_capability(InvocationLimits::default()),
            resource: canonical_audit_resource_digest(&host_request),
            decision,
        })
        .map_err(|_| HostProblem::UnknownOutcome)
}

impl CicsService {
    /// Trusted terminal ingress binds one supported LU signal facility to a
    /// coordinator-owned task; command execution cannot create this facility.
    pub fn install_principal_signal_facility(
        &self,
        run_unit: &mainframe_env_execution_api::RunUnitId,
        lu_type: SignalLuType,
    ) -> Result<(), HostProblem> {
        let owner = {
            let state = self.lock()?;
            let run = state.runs.get(run_unit).ok_or(HostProblem::Unauthorized)?;
            if super::context(run)? != super::ConversationContext::Local {
                return Err(HostProblem::Unauthorized);
            }
            ConversationOwner {
                execution: run.invocation.execution_id.as_str().into(),
                run_unit: run.invocation.run_unit_id.as_str().into(),
                lease_epoch: u64::from(run.invocation.attempt),
            }
        };
        for _ in 0..MAX_CAS_RETRIES {
            let current = ConversationLedger::load(self.store.as_ref()).map_err(store_error)?;
            let mut next = current.clone();
            next.install_signal_facility(owner.clone(), lu_type)
                .map_err(|_| HostProblem::IdempotencyConflict)?;
            if next == current {
                return Ok(());
            }
            if current
                .persist(&mut next, self.store.as_ref())
                .map_err(|error| mutation_problem(store_error(error)))?
            {
                return Ok(());
            }
        }
        Err(HostProblem::UnknownOutcome)
    }

    /// Trusted LU ingress posts a monotonic event without invoking the waiting
    /// program. Exact duplicates leave the durable pending bit unchanged.
    pub fn post_principal_signal(
        &self,
        owner: &ConversationOwner,
        event_sequence: u64,
    ) -> Result<(), HostProblem> {
        for _ in 0..MAX_CAS_RETRIES {
            let current = ConversationLedger::load(self.store.as_ref()).map_err(store_error)?;
            let mut next = current.clone();
            next.post_signal(owner, event_sequence)
                .map_err(signal_problem)?;
            if next == current {
                return Ok(());
            }
            if current
                .persist(&mut next, self.store.as_ref())
                .map_err(|error| mutation_problem(store_error(error)))?
            {
                return Ok(());
            }
        }
        Err(HostProblem::UnknownOutcome)
    }

    /// Trusted LU ingress records a terminal failure in the same ordered event
    /// stream. A waiting task observes TERMERR after this durable transition.
    pub fn fail_principal_signal(
        &self,
        owner: &ConversationOwner,
        event_sequence: u64,
    ) -> Result<(), HostProblem> {
        for _ in 0..MAX_CAS_RETRIES {
            let current = ConversationLedger::load(self.store.as_ref()).map_err(store_error)?;
            let mut next = current.clone();
            next.fail_signal_facility(owner, event_sequence)
                .map_err(signal_problem)?;
            if next == current {
                return Ok(());
            }
            if current
                .persist(&mut next, self.store.as_ref())
                .map_err(|error| mutation_problem(store_error(error)))?
            {
                return Ok(());
            }
        }
        Err(HostProblem::UnknownOutcome)
    }
}
