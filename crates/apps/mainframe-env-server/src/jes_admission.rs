//! Durable JES work-record admission, shared by the z/OSMF submit gateway
//! and by internal-reader child admission.
//!
//! Split out of `product.rs` to keep it under its ADR-0010 module-review
//! budget (`conformance/0.9/inventory/module-budgets.json`). See
//! `docs/architecture/JES-EXECUTION.md` for the admission contract this
//! implements: every worker-run job, including an internal-reader child, gets
//! exactly one durable work record with the same validity and authority
//! rules as a z/OSMF submission.

use crate::jes_worker::{JES_WORK_DEADLINE_TICKS, JES_WORK_GENERATION, JesWorkPayload};
use crate::product::{JES_ALLOWED_WORK_CAPABILITIES, ProductServer, job_capabilities, store_error};
use mainframe_env_batch::JobSnapshot;
use mainframe_env_execution_api::{Invocation, ServiceClass};
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::{StoreError, WorkRecord, WorkState};

/// Whether the parent's own claimed-work item should retire once every
/// internal-reader child has an admission outcome (`Ok`), or must be
/// released for a later retry (`Retry`) because at least one child hit a
/// transient failure and still has no work record.
pub(crate) enum ChildAdmissionResult {
    Ok,
    Retry,
}

/// A failure admitting one internal-reader child's durable work record,
/// classified by whether retrying can help.
enum ChildAdmissionFailure {
    /// Not retryable: capabilities outside `JES_ALLOWED_WORK_CAPABILITIES`,
    /// an owner mismatch, or another malformed/unauthorized admission. The
    /// child is cancelled explicitly so it never sits `Queued` forever with
    /// no work record and nothing to retry it.
    Permanent,
    /// Retryable: store capacity or another infrastructure problem. The
    /// child keeps no work record while the parent still has a retry. On the
    /// parent's last attempt, the child is cancelled and verified instead of
    /// being stranded in `Queued` after the parent becomes terminal.
    Transient,
}

fn classify(problem: &HostProblem) -> ChildAdmissionFailure {
    match problem {
        HostProblem::Malformed
        | HostProblem::Unauthorized
        | HostProblem::UnsupportedCapability { .. } => ChildAdmissionFailure::Permanent,
        _ => ChildAdmissionFailure::Transient,
    }
}

impl ProductServer {
    /// Construct one durable JES work record for `job` and enqueue it. Both
    /// the z/OSMF submit gateway and internal-reader child admission call
    /// this so their validity and authority rules can't drift.
    ///
    /// `AlreadyExists` is success only when the existing record's frozen
    /// admission identity still matches: work ID, job ID, owner, priority,
    /// selector, generation and artifact, with every capability within
    /// `JES_ALLOWED_WORK_CAPABILITIES`. The existing record's capabilities
    /// are validated against that fixed allow-list, never against a fresh
    /// recomputation of `capabilities` — recomputing from a mutable registry
    /// (for example a `batch-program` binding) can differ from the payload
    /// admitted earlier, and comparing against it would wrongly fail an
    /// already-correct idempotent replay.
    pub(crate) fn enqueue_jes_work(
        &self,
        invocation: &Invocation,
        job: &JobSnapshot,
        capabilities: &[&'static str],
        now_tick: u64,
    ) -> Result<(), HostProblem> {
        if invocation.principal.id().as_str() != job.owner {
            return Err(HostProblem::Unauthorized);
        }
        if invocation.selector.as_str() != "zosmf:job-submit"
            || invocation.artifact.as_str() != "artifact:none"
            || capabilities
                .iter()
                .any(|capability| !JES_ALLOWED_WORK_CAPABILITIES.contains(capability))
        {
            return Err(HostProblem::Malformed);
        }
        let deadline_tick = now_tick
            .checked_add(JES_WORK_DEADLINE_TICKS)
            .ok_or(HostProblem::ResourceExhausted)?;
        let work_id = format!("jes:{}", job.id);
        let payload = JesWorkPayload::new(&job.id, &job.owner, capabilities.iter().copied())
            .and_then(|payload| payload.encode())
            .map_err(store_error)?;
        let work = WorkRecord {
            work_id: work_id.clone(),
            execution_id: invocation.execution_id.clone(),
            required_selector: invocation.selector.clone(),
            required_generation: JES_WORK_GENERATION.into(),
            artifact: invocation.artifact.clone(),
            state: WorkState::Queued,
            priority: job.priority,
            attempt: 0,
            max_attempts: 3,
            available_tick: now_tick,
            deadline_tick,
            cancellation_requested: false,
            worker_id: None,
            lease_id: None,
            lease_epoch: 0,
            lease_expiry_tick: None,
            heartbeat_tick: None,
            terminal_tick: None,
            checkpoint_id: None,
            effect_sequence: 0,
            payload,
        };
        match self.store.enqueue(work) {
            Ok(()) => {}
            Err(StoreError::AlreadyExists) => {
                let existing = self
                    .store
                    .get_work(&work_id)
                    .map_err(store_error)?
                    .ok_or(HostProblem::InfrastructureFailure)?;
                let existing_payload = JesWorkPayload::decode(&existing.payload)
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
                if existing.work_id != work_id
                    || existing.required_selector.as_str() != "zosmf:job-submit"
                    || existing.required_generation != JES_WORK_GENERATION
                    || existing.artifact.as_str() != "artifact:none"
                    || existing.max_attempts != 3
                    || existing.priority != job.priority
                    || existing_payload.job_id != job.id
                    || existing_payload.owner != job.owner
                    || !existing_payload.capabilities.iter().all(|capability| {
                        JES_ALLOWED_WORK_CAPABILITIES.contains(&capability.as_str())
                    })
                {
                    return Err(HostProblem::InfrastructureFailure);
                }
            }
            Err(problem) => return Err(store_error(problem)),
        }
        self.jes_worker_notify.notify_one();
        Ok(())
    }

    /// Admit exactly one durable work record for every internal-reader child
    /// of `parent_job_id`, with the child's own authority: capabilities from
    /// `job_capabilities` over the child's own admitted plan, and the
    /// child's own owner and priority — never the parent's.
    ///
    /// Every child gets an admission attempt even if an earlier one fails,
    /// so one bad child can never leave a later sibling `Queued` forever
    /// with no work record. See `classify` for the transient/permanent
    /// split and `ChildAdmissionResult` for how the caller should treat the
    /// parent's own work afterward.
    pub(crate) fn admit_internal_reader_children(
        &self,
        parent_job_id: &str,
        retry_allowed: bool,
    ) -> Result<ChildAdmissionResult, HostProblem> {
        let mut needs_retry = false;
        let mut unresolved_children: Vec<String> = Vec::new();
        for (child, plan) in self.batch.internal_reader_children(parent_job_id)? {
            let admission = (|| {
                let capabilities = job_capabilities(self.store.as_ref(), &plan)?;
                let invocation = self.invocation(
                    &child.owner,
                    "zosmf:job-submit",
                    ServiceClass::Batch,
                    &capabilities,
                )?;
                let now_tick = self.jes_tick()?;
                self.enqueue_jes_work(&invocation, &child, &capabilities, now_tick)
            })();
            let Err(problem) = admission else {
                continue;
            };
            match classify(&problem) {
                ChildAdmissionFailure::Transient if retry_allowed => needs_retry = true,
                ChildAdmissionFailure::Transient => {
                    if !self.cancel_internal_reader_child(&child) {
                        unresolved_children.push(child.id.clone());
                    }
                }
                ChildAdmissionFailure::Permanent => {
                    if !self.cancel_internal_reader_child(&child) {
                        unresolved_children.push(child.id.clone());
                    }
                }
            }
        }
        // A permanent failure whose cancellation could not be verified is
        // named here rather than through a logging dependency this crate
        // does not otherwise take on; the caller's `?` dead-letters the
        // parent's own work immediately (not a bounded retry), which is
        // conservative: we can no longer prove the child won't sit `Queued`
        // forever with no record.
        if !unresolved_children.is_empty() {
            return Err(HostProblem::InfrastructureFailure);
        }
        if needs_retry {
            return Ok(ChildAdmissionResult::Retry);
        }
        Ok(ChildAdmissionResult::Ok)
    }

    /// Cancel an internal-reader child whose admission permanently failed,
    /// and verify the cancellation took effect. Returns `true` once the
    /// child is confirmed `Cancelled`, regardless of whether `batch.cancel`
    /// itself returned `Ok` (a racing terminal transition is still a safe
    /// outcome as long as the child is not left `Queued`).
    pub(crate) fn cancel_internal_reader_child(&self, child: &JobSnapshot) -> bool {
        let Ok(invocation) = self.invocation(
            &child.owner,
            "zosmf:job-submit",
            ServiceClass::Batch,
            &[
                "host.security.authorize",
                "host.program.invoke",
                "host.spool.read",
                "host.spool.write",
            ],
        ) else {
            return false;
        };
        let _ = self.batch.cancel(&invocation, &child.id);
        matches!(
            self.batch.get(&child.id),
            Ok(job) if job.state == mainframe_env_batch::JobState::Cancelled
        )
    }
}
