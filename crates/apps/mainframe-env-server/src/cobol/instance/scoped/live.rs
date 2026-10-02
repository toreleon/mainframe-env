//! Thread-confined live instance lease, never reconstructed by a row reader.
use super::*;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

#[cfg(test)]
mod tests;

thread_local! {
    static SOURCES: RefCell<Vec<Weak<LiveSource>>> = const { RefCell::new(Vec::new()) };
}

struct LiveSource {
    router: usize,
    invocation: Invocation,
    entry: Entry,
    active: Cell<bool>,
    uncertain: Cell<bool>,
    row: RefCell<ProviderStateRecord>,
}

/// Owned only by the currently executing admitted installed instance. Its Rc
/// makes crossing threads impossible; dropping it removes all lookup authority.
pub(super) struct LiveLease(Rc<LiveSource>);

/// Prepared exact source guard. A cloned observation cannot survive lease exit.
pub(super) struct SourceFence {
    source: Rc<LiveSource>,
    staged: ProviderStateWrite,
}

fn current(source: &Rc<LiveSource>) -> bool {
    SOURCES.with(|sources| {
        let Ok(mut sources) = sources.try_borrow_mut() else {
            return false;
        };
        sources.retain(|node| node.upgrade().is_some_and(|node| node.active.get()));
        sources
            .last()
            .and_then(Weak::upgrade)
            .is_some_and(|node| Rc::ptr_eq(&node, source) && !node.uncertain.get())
    })
}

impl LiveLease {
    /// The serialized admission owner calls this only after winning the original
    /// pending CALL + root/member/source transaction. It is private to the scoped
    /// implementation; no durable decoder or public invocation can mint a lease.
    pub(super) fn register_known_reservation(
        router: usize,
        invocation: Invocation,
        entry: Entry,
        row: ProviderStateRecord,
    ) -> Result<Self, HostProblem> {
        entry.validate_for(&invocation)?;
        if Entry::read(&invocation)?.as_ref() != Some(&entry) {
            return Err(HostProblem::UnknownOutcome);
        }
        let instance = ScopedInstance::decode(&row)?;
        if instance.owner.as_ref() != Some(&entry) {
            return Err(HostProblem::UnknownOutcome);
        }
        let source = Rc::new(LiveSource {
            router,
            invocation,
            entry,
            active: Cell::new(true),
            uncertain: Cell::new(false),
            row: RefCell::new(row),
        });
        SOURCES.with(|sources| {
            let mut sources = sources
                .try_borrow_mut()
                .map_err(|_| HostProblem::UnknownOutcome)?;
            sources.retain(|node| node.upgrade().is_some_and(|node| node.active.get()));
            let run = source.entry.creation_identity();
            let count = sources
                .iter()
                .filter_map(Weak::upgrade)
                .filter(|node| {
                    node.router == router
                        && node.entry.creation_identity().0 == run.0
                        && node.invocation.run_unit_id == source.invocation.run_unit_id
                })
                .count();
            if count >= MAX_INSTANCES
                || (count as u64)
                    .checked_add(2)
                    .is_none_or(|n| n > u64::from(source.invocation.limits.max_frames))
                || sources.iter().filter_map(Weak::upgrade).any(|node| {
                    node.router == router
                        && node.invocation.execution_id == source.invocation.execution_id
                })
            {
                return Err(HostProblem::UnknownOutcome);
            }
            sources.push(Rc::downgrade(&source));
            Ok(Self(source))
        })
    }

    pub(super) fn record(&self) -> Result<ProviderStateRecord, HostProblem> {
        if !current(&self.0) {
            return Err(HostProblem::UnknownOutcome);
        }
        self.0
            .row
            .try_borrow()
            .map(|row| row.clone())
            .map_err(|_| HostProblem::UnknownOutcome)
    }

    pub(super) fn entry(&self) -> &Entry {
        &self.0.entry
    }
}

impl Drop for LiveLease {
    fn drop(&mut self) {
        self.0.active.set(false);
        SOURCES.with(|sources| {
            if let Ok(mut sources) = sources.try_borrow_mut() {
                sources.retain(|node| {
                    node.upgrade()
                        .is_some_and(|node| !Rc::ptr_eq(&node, &self.0))
                });
            }
        });
    }
}

impl SourceFence {
    /// Observe only the current stack entry of this exact router and invocation.
    /// An existing serialized busy row alone can never supply this observation.
    pub(super) fn prepare(
        router: usize,
        invocation: &Invocation,
        store: &dyn PlatformStore,
    ) -> Result<Self, HostProblem> {
        let source = SOURCES.with(|sources| {
            let mut sources = sources
                .try_borrow_mut()
                .map_err(|_| HostProblem::UnknownOutcome)?;
            sources.retain(|node| node.upgrade().is_some_and(|node| node.active.get()));
            sources
                .last()
                .and_then(Weak::upgrade)
                .ok_or(HostProblem::UnknownOutcome)
        })?;
        if source.uncertain.get() || source.router != router || source.invocation != *invocation {
            return Err(HostProblem::UnknownOutcome);
        }
        source.entry.validate_for(invocation)?;
        let row = source
            .row
            .try_borrow()
            .map_err(|_| HostProblem::UnknownOutcome)?;
        let observed = store
            .get_provider_state(&row.namespace, &row.key)
            .map_err(|_| HostProblem::UnknownOutcome)?
            .ok_or(HostProblem::UnknownOutcome)?;
        if observed != *row {
            return Err(HostProblem::UnknownOutcome);
        }
        let instance = ScopedInstance::decode(&observed)?;
        if instance.owner.as_ref() != Some(&source.entry) {
            return Err(HostProblem::UnknownOutcome);
        }
        let version = observed
            .version
            .checked_add(1)
            .filter(|v| *v <= i64::MAX as u64)
            .ok_or(HostProblem::ResourceExhausted)?;
        let staged = ProviderStateWrite {
            expected_version: Some(observed.version),
            record: ProviderStateRecord {
                version,
                ..observed
            },
        };
        drop(row);
        Ok(Self { source, staged })
    }

    pub(super) fn staged_write(&self) -> &ProviderStateWrite {
        &self.staged
    }

    /// Publish the exact source fence in the SAME transaction as the root
    /// index/charge, target member and original pending CALL updates. Adopt its
    /// version only on known success; all failures retain the old live token.
    pub(super) fn publish(
        self,
        store: &dyn PlatformStore,
        other: Vec<ProviderStateMutation>,
    ) -> Result<(), HostProblem> {
        self.publish_with(other, |mutations| {
            store.mutate_provider_states_atomic(mutations)
        })
    }

    fn publish_with(
        self,
        mut other: Vec<ProviderStateMutation>,
        publish: impl FnOnce(Vec<ProviderStateMutation>) -> Result<(), StoreError>,
    ) -> Result<(), HostProblem> {
        if !current(&self.source) {
            return Err(HostProblem::UnknownOutcome);
        }
        let mut row = self
            .source
            .row
            .try_borrow_mut()
            .map_err(|_| HostProblem::UnknownOutcome)?;
        if Some(row.version) != self.staged.expected_version
            || row.namespace != self.staged.record.namespace
            || row.key != self.staged.record.key
            || row.payload != self.staged.record.payload
        {
            return Err(HostProblem::UnknownOutcome);
        }
        if other.iter().any(|mutation| match mutation {
            ProviderStateMutation::Put(write) => {
                write.record.namespace == row.namespace && write.record.key == row.key
            }
            ProviderStateMutation::Delete { namespace, key, .. } => {
                *namespace == row.namespace && *key == row.key
            }
            ProviderStateMutation::Move {
                record, old_key, ..
            } => {
                record.namespace == row.namespace && (record.key == row.key || *old_key == row.key)
            }
        }) {
            return Err(HostProblem::UnknownOutcome);
        }
        let (_, run, principal, _) = self.source.entry.creation_identity();
        let root_key = super::super::super::retention::run_state_key(run, principal);
        let roots = other
            .iter()
            .filter_map(|mutation| match mutation {
                ProviderStateMutation::Put(write)
                    if write.record.namespace == RUN_STATE_NAMESPACE
                        && write.record.key == root_key =>
                {
                    Some(write)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        if roots.len() != 1
            || roots[0]
                .expected_version
                .is_none_or(|v| v == 0 || v.checked_add(1) != Some(roots[0].record.version))
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let root = ScopedRun::decode(&roots[0].record)?;
        let member = root
            .members
            .get(&row.key)
            .ok_or(HostProblem::UnknownOutcome)?;
        let scope = root
            .scopes
            .get(self.source.entry.scope_id())
            .ok_or(HostProblem::UnknownOutcome)?;
        if !scope.same_scope(&self.source.entry)
            || !member.busy
            || member.row_version != self.staged.record.version
            || member.payload_digest != payload_digest(&row.payload)
            || member.program != self.source.entry.actor_identity().1
            || member.artifact != self.source.entry.actor_identity().2
        {
            return Err(HostProblem::UnknownOutcome);
        }
        other.push(ProviderStateMutation::Put(self.staged.clone()));
        if publish(other).is_err() {
            self.source.uncertain.set(true);
            return Err(HostProblem::UnknownOutcome);
        }
        *row = self.staged.record;
        Ok(())
    }
}
