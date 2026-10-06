//! Read-only accessor for a worker-run parent's internal-reader child jobs.
//!
//! Split out of `service.rs` to keep `BatchService`'s core state machine
//! under its ADR-0010 module-review budget
//! (`conformance/subsystems/cics/application/inventory/module-budgets.json`). The gateway's JES
//! admission helper (`mainframe-env-server`) uses this to find every child a
//! worker-run parent wrote to `SYSOUT=(A,INTRDR)` and to read each child's own
//! admitted plan, so it can compute the child's own capabilities rather than
//! inheriting the parent's.

use crate::service::{BatchService, JobSnapshot, snapshot};
use crate::{JesSubmissionOrigin, JobPlan};
use mainframe_env_host_api::HostProblem;

impl BatchService {
    /// Return every internal-reader child of `parent_job_id` together with its
    /// immutable admitted plan, without silent truncation.
    pub fn internal_reader_children(
        &self,
        parent_job_id: &str,
    ) -> Result<Vec<(JobSnapshot, JobPlan)>, HostProblem> {
        Ok(self
            .lock()?
            .jobs
            .values()
            .filter_map(|job| match &job.origin {
                JesSubmissionOrigin::InternalReader {
                    parent_job_id: parent,
                    ..
                } if parent == parent_job_id => Some((snapshot(job), job.plan.clone())),
                _ => None,
            })
            .collect())
    }
}
