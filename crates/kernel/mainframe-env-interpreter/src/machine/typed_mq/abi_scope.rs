//! Volatile root-owned wire aliases, never a second executable handle registry.
use super::*;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, MutexGuard};

const LAST_ALIAS: i32 = 999_999_999; // Explicit PIC S9(9) BINARY product ABI.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Slot {
    Empty,
    Reserved(i32),
    Connection { alias: i32, token: MqHconn },
}
struct Table {
    slots: Vec<Slot>,
    next: i32,
    fenced: bool,
}

/// Privileged volatile ABI storage shared by one independently admitted task root
/// and its SAME TASK frames. Construction is not root, handle, SAF or UOW proof:
/// the installed frame must independently supply the same allocation. No value,
/// snapshot, receipt or integer can restore this table or mint a provider token.
/// Serialization cannot turn this volatile allocation into stored authority:
///
/// ```compile_fail
/// use mainframe_env_interpreter::MqMqiAbiScope;
/// let _ = serde_json::to_vec::<MqMqiAbiScope>;
/// ```
///
/// ```compile_fail
/// use mainframe_env_interpreter::MqMqiAbiScope;
/// let _ = serde_json::from_str::<MqMqiAbiScope>("{}");
/// ```
pub struct MqMqiAbiScope {
    context: MqMqiContext,
    table: Mutex<Table>,
}
impl std::fmt::Debug for MqMqiAbiScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MqMqiAbiScope { volatile: true }")
    }
}
impl PartialEq for MqMqiAbiScope {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}
impl Eq for MqMqiAbiScope {}

impl MqMqiAbiScope {
    /// Allocate the complete bounded alias capacity before binding any machine.
    /// `context` must come from admitted host lifecycle, not invocation bindings.
    /// Capacity is a product guard, not an IBM maximum. The caller owns root
    /// sharing and cold replacement; this object performs no lifecycle callback.
    pub fn new(context: MqMqiContext, capacity: usize) -> Result<Self, HostProblem> {
        if context.owner.environment != MqHostEnvironment::ZosBatch
            || context.syncpoint_owner != MqSyncpointOwner::QueueManager
            || capacity == 0
            || capacity > MQ_MAX_HANDLE_SLOTS
        {
            return Err(HostProblem::Unsupported);
        }
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(capacity)
            .map_err(|_| HostProblem::ResourceExhausted)?;
        slots.resize(capacity, Slot::Empty);
        Ok(Self {
            context,
            table: Mutex::new(Table {
                slots,
                next: 1,
                fenced: false,
            }),
        })
    }
    fn lock(&self) -> Result<MutexGuard<'_, Table>, HostProblem> {
        let table = self.table.lock().map_err(|_| HostProblem::UnknownOutcome)?;
        if table.fenced {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(table)
    }
    pub(super) fn require_context(&self, context: MqMqiContext) -> Result<(), HostProblem> {
        if context != self.context {
            return Err(HostProblem::Unauthorized);
        }
        drop(self.lock()?);
        Ok(())
    }
    pub(super) fn fence(&self) {
        if let Ok(mut table) = self.table.lock() {
            table.fenced = true;
        }
    }
    pub(super) fn connection(&self, alias: i32) -> Result<MqHconn, HostProblem> {
        self.lock()?
            .slots
            .iter()
            .find_map(|slot| match slot {
                Slot::Connection {
                    alias: current,
                    token,
                } if *current == alias => Some(*token),
                _ => None,
            })
            .ok_or(HostProblem::Malformed)
    }
    pub(super) fn reserve(self: &Arc<Self>) -> Result<Reservation, HostProblem> {
        let mut table = self.lock()?;
        if table.next > LAST_ALIAS {
            return Err(HostProblem::ResourceExhausted);
        }
        let slot = table
            .slots
            .iter()
            .position(|s| *s == Slot::Empty)
            .ok_or(HostProblem::ResourceExhausted)?;
        let alias = table.next;
        // No abort or retirement rewinds this high-water value.
        table.next += 1;
        table.slots[slot] = Slot::Reserved(alias);
        Ok(Reservation(Arc::new(Reserved {
            scope: self.clone(),
            slot,
            alias,
        })))
    }
    fn plan(
        &self,
        reservation: Option<&Reservation>,
        connection: Option<MqHconn>,
        retired: Option<i32>,
    ) -> Result<Plan, HostProblem> {
        if connection.is_some_and(|c| !matches!(c, MqHconn::Issued(_)) || c.is_historical()) {
            return Err(HostProblem::UnknownOutcome);
        }
        let table = self.lock()?;
        let reserved = reservation.map(|r| (r.0.slot, r.0.alias));
        if reservation.is_some_and(|r| !std::ptr::eq(self, r.0.scope.as_ref()))
            || reserved
                .is_some_and(|(slot, alias)| table.slots.get(slot) != Some(&Slot::Reserved(alias)))
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let live = connection.and_then(|token| {
            table
                .slots
                .iter()
                .enumerate()
                .find_map(|(slot, value)| match value {
                    Slot::Connection { alias, token: old } if *old == token => Some((slot, *alias)),
                    _ => None,
                })
        });
        let adoption = match (connection, live, reserved) {
            (Some(token), Some((slot, alias)), _) => Some((slot, alias, token, true)),
            (Some(token), None, Some((slot, alias))) => Some((slot, alias, token, false)),
            (Some(_), None, None) => return Err(HostProblem::UnknownOutcome),
            (None, _, _) => None,
        };
        let retired = retired
            .map(|alias| {
                table
                    .slots
                    .iter()
                    .enumerate()
                    .find_map(|(slot, value)| match value {
                        Slot::Connection { alias: old, token } if *old == alias => {
                            Some((slot, alias, *token))
                        }
                        _ => None,
                    })
                    .ok_or(HostProblem::UnknownOutcome)
            })
            .transpose()?;
        Ok(Plan {
            reserved,
            adoption,
            retired,
        })
    }
}

#[derive(Debug)]
struct Reserved {
    scope: Arc<MqMqiAbiScope>,
    slot: usize,
    alias: i32,
}
impl Drop for Reserved {
    fn drop(&mut self) {
        if let Ok(mut table) = self.scope.table.lock()
            && table.slots[self.slot] == Slot::Reserved(self.alias)
        {
            table.slots[self.slot] = Slot::Empty;
        }
    }
}
#[derive(Clone, Debug)]
pub(super) struct Reservation(Arc<Reserved>);
impl PartialEq for Reservation {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for Reservation {}

struct Plan {
    reserved: Option<(usize, i32)>,
    adoption: Option<(usize, i32, MqHconn, bool)>,
    retired: Option<(usize, i32, MqHconn)>,
}
impl Plan {
    fn wire(&self) -> Option<i32> {
        self.adoption.map(|(_, alias, _, _)| alias)
    }
    // The final alias check owns the table through the callback-free byte commit.
    fn guard<'a>(&self, scope: &'a MqMqiAbiScope) -> Result<MutexGuard<'a, Table>, HostProblem> {
        let table = scope.lock()?;
        if self
            .reserved
            .is_some_and(|(slot, alias)| table.slots[slot] != Slot::Reserved(alias))
            || self.adoption.is_some_and(|(slot, alias, token, existing)| {
                existing && table.slots[slot] != Slot::Connection { alias, token }
            })
            || self.adoption.is_some_and(|(_, _, token, existing)| {
                !existing
                    && table
                        .slots
                        .iter()
                        .any(|s| matches!(s, Slot::Connection { token: old, .. } if *old == token))
            })
            || self.retired.is_some_and(|(slot, alias, token)| {
                table.slots[slot] != Slot::Connection { alias, token }
            })
        {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(table)
    }
    fn commit(&self, table: &mut Table) {
        if let Some((slot, _)) = self.reserved {
            table.slots[slot] = Slot::Empty;
        }
        if let Some((slot, alias, token, _)) = self.adoption {
            table.slots[slot] = Slot::Connection { alias, token };
        }
        if let Some((slot, _, _)) = self.retired {
            table.slots[slot] = Slot::Empty;
        }
    }
}

pub(super) struct CompletionGuard {
    scope: Option<Arc<MqMqiAbiScope>>,
    known: bool,
}

// The pending machine owns this lease. Dropping/cancelling a dispatched call
// cannot silently release its alias reservation and let the root keep driving.
// It only fences volatile aliases: no provider/UOW or terminal callback runs.
#[derive(Clone, Debug)]
pub(super) struct PendingLease(Arc<Lease>);
#[derive(Debug)]
struct Lease {
    scope: Arc<MqMqiAbiScope>,
    state: AtomicU8, // 0 prepared, 1 dispatched, 2 known atomic writeback
}
impl PendingLease {
    pub(super) fn new(scope: Arc<MqMqiAbiScope>) -> Self {
        Self(Arc::new(Lease {
            scope,
            state: AtomicU8::new(0),
        }))
    }
    pub(super) fn arm(&self) {
        self.0.state.store(1, Ordering::Release);
    }
    pub(super) fn known(&self) {
        self.0.state.store(2, Ordering::Release);
    }
}
impl PartialEq for PendingLease {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for PendingLease {}
impl Drop for Lease {
    fn drop(&mut self) {
        if self.state.load(Ordering::Acquire) == 1 {
            self.scope.fence();
        }
    }
}
impl CompletionGuard {
    pub(super) fn new(scope: Option<Arc<MqMqiAbiScope>>) -> Self {
        Self {
            scope,
            known: false,
        }
    }
    pub(super) fn known(&mut self) {
        self.known = true;
    }
}
impl Drop for CompletionGuard {
    fn drop(&mut self) {
        if !self.known
            && let Some(scope) = &self.scope
        {
            scope.fence();
        }
    }
}

impl ReferenceMachine {
    pub(super) fn finish_scoped_mq(
        &mut self,
        targets: &Targets,
        connection: Option<MqHconn>,
        completion: i32,
        reason: i32,
        completed: bool,
    ) -> Result<(), MachineProblem> {
        let scope = targets
            .scope
            .as_ref()
            .ok_or(MachineProblem::UnexpectedHostResult)?;
        let plan = scope
            .plan(
                targets.reservation.as_ref(),
                connection,
                targets.disconnected.filter(|_| completed),
            )
            .map_err(MachineProblem::Host)?;
        let mut writes = Vec::new();
        writes
            .try_reserve_exact(3)
            .map_err(|_| MachineProblem::ResourceExhausted)?;
        for (target, value) in [
            (&targets.connection, plan.wire()),
            (&targets.completion, Some(completion)),
            (&targets.reason, Some(reason)),
        ] {
            let Some(value) = value else { continue };
            self.mq_long_target(target)?;
            let storage = self.connx_storage(target)?;
            if storage.layout.native_binary {
                return Err(MachineProblem::UnsupportedForm);
            }
            let bytes = encode_decimal(
                &storage.layout,
                Decimal {
                    coefficient: i128::from(value),
                    scale: 0,
                },
            )?;
            if bytes.len() != storage.view.length {
                return Err(MachineProblem::UnsupportedForm);
            }
            writes.push((storage.view, bytes));
        }
        if let Some(capture) = &targets.connx {
            self.recheck_connx(capture)?;
        }
        if let Some(capture) = &targets.connect {
            self.recheck_connect(capture)?;
        }
        connx::contained(|| {
            self.mqi
                .as_ref()
                .ok_or(MachineProblem::UnexpectedHostResult)?
                .current(&self.invocation)
        })?;
        // Pure final storage check AFTER every external observation; no copies,
        // allocation, callback or fallible view lookup follows the alias guard.
        for stored in &targets.scoped_arguments {
            if self.layout(&stored.layout.name) != Some(&stored.layout)
                || self.views.get(&stored.layout.name) != Some(&stored.view)
                || self.bases.get(stored.view.base).and_then(|b| {
                    b.get(stored.view.offset..stored.view.offset + stored.view.length)
                }) != Some(stored.bytes.as_slice())
            {
                return Err(MachineProblem::Host(HostProblem::UnknownOutcome));
            }
        }
        let mut guard = plan.guard(scope).map_err(MachineProblem::Host)?;
        for (view, bytes) in writes {
            self.bases[view.base][view.offset..view.offset + view.length].copy_from_slice(&bytes);
        }
        plan.commit(&mut guard);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
