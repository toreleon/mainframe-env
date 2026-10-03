//! Private checked root retirement, separate from the durable native decision.
use super::*;

/// Issued only from the live exact root under the sole selected mutex. This
/// token cannot attest a machine outcome or make a durable UOW decision.
pub(crate) struct TerminalRoot {
    frame: FrameLease,
    process: ProcessLease,
    owner: MqHandleOwner,
}

impl MqLifecycleDirectory {
    pub(crate) fn prepare_terminal_root(
        &self,
        frame: FrameLease,
        original: &Invocation,
        now: u64,
    ) -> Result<TerminalRoot, HostProblem> {
        let owner = self.owner_for(frame, original, now)?;
        let logical = self.logical_batch_owner(frame, original, now)?;
        if original.parent_execution_id.is_some()
            || logical.is_child()
            || logical.execution() != original.execution_id.as_str()
            || owner.environment != MqHostEnvironment::ZosBatch
            || self
                .frames
                .values()
                .filter(|other| other.owner.process_id == frame.process)
                .count()
                != 1
        {
            return Err(HostProblem::Unsupported);
        }
        let process = ProcessLease {
            directory: frame.directory,
            process: frame.process,
        };
        if !self.processes.contains_key(&process.process) {
            return Err(HostProblem::Unauthorized);
        }
        Ok(TerminalRoot {
            frame,
            process,
            owner,
        })
    }

    /// Must run only after the same physical terminal decision is known and
    /// its live control window remains admitted. No Drop path calls this.
    pub(crate) fn retire_terminal_root(
        &mut self,
        proof: TerminalRoot,
        registry: &mut MqHandleRegistry,
    ) -> Result<(), HostProblem> {
        let frame = self.frame(proof.frame)?;
        if frame.owner != proof.owner
            || frame.invocation.parent_execution_id.is_some()
            || self
                .frames
                .values()
                .filter(|other| other.owner.process_id == proof.process.process)
                .count()
                != 1
            || !self.processes.contains_key(&proof.process.process)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        self.retire_frame(proof.frame, registry)?;
        self.retire_process(proof.process, registry)
    }
}
