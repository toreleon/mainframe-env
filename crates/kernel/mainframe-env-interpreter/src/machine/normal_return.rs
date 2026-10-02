//! Pure observation of a successfully executed native installed-program return.
use super::*;

/// Native return operation actually completed by the live machine drive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstalledProgramReturnKind {
    /// A successfully completed GOBACK operation.
    Goback,
    /// A successfully completed EXIT PROGRAM operation.
    ExitProgram,
}

/// Live native-return observation, without durable or core terminal authority.
///
/// This value cannot be constructed or deserialized by callers. It grants no
/// admission, cleanup, redispatch or cached-reply permission. Its owner must
/// independently validate the core terminal event, attempt and checkpoint.
pub struct InstalledProgramReturn {
    invocation: Invocation,
    kind: InstalledProgramReturnKind,
    program_counter: usize,
    executed_steps: u64,
    return_code: i32,
}

impl InstalledProgramReturn {
    /// Exact invocation associated with the observed live return.
    pub fn invocation(&self) -> &Invocation {
        &self.invocation
    }

    /// Native return operation that successfully completed.
    pub fn kind(&self) -> InstalledProgramReturnKind {
        self.kind
    }

    /// Return code produced by that exact successful native completion.
    pub fn return_code(&self) -> i32 {
        self.return_code
    }

    /// Program counter of the executed terminal operation.
    pub fn program_counter(&self) -> usize {
        self.program_counter
    }

    /// Cumulative executed-step count at the successful terminal operation.
    pub fn executed_steps(&self) -> u64 {
        self.executed_steps
    }
}

#[derive(Clone, Copy)]
pub(super) struct NormalReturnMarker {
    kind: InstalledProgramReturnKind,
    pc: usize,
    steps: u64,
    return_code: i32,
}

impl NormalReturnMarker {
    pub(super) fn completed(
        operation: &Operation,
        pc: usize,
        steps: u64,
        return_code: i32,
    ) -> Option<Self> {
        Some(Self {
            kind: return_kind(operation)?,
            pc,
            steps,
            return_code,
        })
    }
}

fn return_kind(operation: &Operation) -> Option<InstalledProgramReturnKind> {
    match operation.identity.name() {
        "go_back" => Some(InstalledProgramReturnKind::Goback),
        "exit" if arguments(operation) == ["PROGRAM"] => {
            Some(InstalledProgramReturnKind::ExitProgram)
        }
        _ => None,
    }
}

impl ReferenceMachine {
    /// Observe a successful live GOBACK/EXIT PROGRAM, never a restored exit PC.
    ///
    /// This pure observation is not durable terminal proof or a permission to
    /// admit actors, retire instances, redispatch work or publish cached replies.
    pub fn attest_installed_program_return(
        &self,
    ) -> Result<InstalledProgramReturn, MachineProblem> {
        let marker = self.normal_return.ok_or(MachineProblem::UnsupportedForm)?;
        if marker.pc != self.pc
            || marker.steps != self.executed_steps
            || self.pending.is_some()
            || self.deferred_drive.is_some()
            || self.operations.get(self.pc).and_then(return_kind) != Some(marker.kind)
            || !self.dataset_cursors.is_empty()
        {
            return Err(MachineProblem::UnsupportedForm);
        }
        // Reuse all existing supported lifecycle/resource checks without changing
        // the older API's contract or retaining its serialized tuple in the token.
        self.retained_program_state()?;
        Ok(InstalledProgramReturn {
            invocation: self.invocation.clone(),
            kind: marker.kind,
            program_counter: marker.pc,
            executed_steps: marker.steps,
            return_code: marker.return_code,
        })
    }

    // Existing restore performs these fallible checks after writing earlier
    // fields. Validate the same conditions first, preserving state and marker on
    // error. No additional snapshot field, version, or accepted shape is added.
    pub(super) fn validate_return_restore(
        &self,
        snapshot: &MachineSnapshot,
    ) -> Result<(), MachineProblem> {
        if snapshot.schema_version >= 7
            && ConditionStatus::from_bits(snapshot.condition_statuses).is_none()
        {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
        if snapshot.schema_version >= 8 {
            let expected_dynamic = self
                .layouts
                .values()
                .filter(|layout| layout.dynamic)
                .map(|layout| layout.name.clone())
                .collect::<BTreeSet<_>>();
            if snapshot
                .dynamic_lengths
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>()
                != expected_dynamic
                || snapshot.dynamic_lengths.iter().any(|(name, length)| {
                    self.layouts
                        .get(name)
                        .is_none_or(|layout| *length > layout.dynamic_limit)
                })
                || snapshot
                    .search_results
                    .keys()
                    .any(|node| !self.control_nodes.contains_key(node))
                || snapshot
                    .active_sort_procedure
                    .as_ref()
                    .is_some_and(|(pc, _, phase, _)| {
                        *pc >= self.operations.len() || !matches!(phase, 0 | 1)
                    })
                || snapshot
                    .sort_io
                    .as_ref()
                    .is_some_and(|(pc, ..)| *pc >= self.operations.len())
            {
                return Err(MachineProblem::IncompatibleSnapshot);
            }
        }
        if snapshot.schema_version >= 9
            && (snapshot.linkage_addresses.len() > self.invocation.limits.max_frames as usize
                || snapshot.freed_allocations.len() > self.invocation.limits.max_frames as usize
                || snapshot.freed_allocations.iter().any(|base| {
                    *base < self.static_base_count || *base >= snapshot.base_storage.len()
                })
                || snapshot.linkage_addresses.iter().any(|(name, view)| {
                    !self.layouts.get(name).is_some_and(|layout| layout.linkage)
                        || self.views.get(name).is_none_or(|original| {
                            view.as_ref()
                                .is_some_and(|(_, _, length)| *length != original.length)
                        })
                        || view.as_ref().is_some_and(|(base, offset, length)| {
                            snapshot.base_storage.get(*base).is_none_or(|storage| {
                                offset
                                    .checked_add(*length)
                                    .is_none_or(|end| end > storage.len())
                            })
                        })
                }))
        {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
