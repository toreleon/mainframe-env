//! Durable pending deliveries and explicit virtual-time advancement.

use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const QUEUE_NAMESPACE: &str = "cics-bms-route-queue-v1";
const QUEUE_KEY: &str = "pending";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PendingRoute {
    pub due_tick: u64,
    pub session: String,
    pub terminal: String,
    pub image: Vec<u8>,
    pub title: Vec<u8>,
    pub errterm: Option<String>,
    pub frames: Vec<ControlFrame>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RouteQueue {
    pub pending: Vec<PendingRoute>,
    #[serde(skip)]
    pub version: u64,
}

pub(super) fn read(service: &CicsService) -> Result<RouteQueue, HostProblem> {
    let Some(row) = service
        .store
        .get_provider_state(QUEUE_NAMESPACE, QUEUE_KEY)
        .map_err(store_error)?
    else {
        return Ok(RouteQueue::default());
    };
    if row.version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut queue: RouteQueue =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    validate(service, &queue).map_err(|_| HostProblem::InfrastructureFailure)?;
    queue.version = row.version;
    Ok(queue)
}

fn validate(service: &CicsService, queue: &RouteQueue) -> Result<(), HostProblem> {
    if queue.pending.len() > service.limits.max_queue_records
        || queue
            .pending
            .iter()
            .try_fold(0usize, |total, item| {
                total
                    .checked_add(item.image.len())?
                    .checked_add(item.title.len())
            })
            .is_none_or(|total| total > service.limits.max_queue_bytes)
        || queue.pending.iter().any(|item| {
            item.image.len() > service.limits.max_screen_bytes
                || item.title.len() > 256
                || item.frames.len() > service.limits.max_fields
                || item.frames.iter().any(|frame| {
                    frame.flags
                        & !(ERASE
                            | ERASEAUP
                            | FRSET
                            | FREEKB
                            | ALARM
                            | PRINT
                            | FORMFEED
                            | DEFAULT
                            | ALTERNATE
                            | HONEOM
                            | L40
                            | L64
                            | L80)
                        != 0
                        || frame.outpartn.as_ref().is_some_and(|name| name.len() > 2)
                        || frame.actpartn.as_ref().is_some_and(|name| name.len() > 2)
                })
                || item.session.is_empty()
                || item.session.len() > 64
                || item.terminal.len() != 4
                || !item
                    .terminal
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric())
                || item.errterm.as_ref().is_some_and(|x| x.len() != 4)
        })
    {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(())
}

pub(super) fn write(
    service: &CicsService,
    queue: &RouteQueue,
) -> Result<ProviderStateMutation, HostProblem> {
    validate(service, queue)?;
    Ok(ProviderStateMutation::Put(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: QUEUE_NAMESPACE.into(),
            key: QUEUE_KEY.into(),
            version: queue
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?,
            payload: serde_json::to_vec(queue).map_err(|_| HostProblem::InfrastructureFailure)?,
        },
        expected_version: (queue.version != 0).then_some(queue.version),
    }))
}

impl CicsService {
    /// Number of bounded BMS route deliveries awaiting an explicit virtual-time advance.
    pub fn pending_route_count(&self) -> Result<usize, HostProblem> {
        Ok(read(self)?.pending.len())
    }

    /// Deliver due BMS routes atomically after the caller advances virtual time.
    pub fn deliver_due_routes(&self, now_tick: u64) -> Result<usize, HostProblem> {
        let mut queue = read(self)?;
        if !queue.pending.iter().any(|item| item.due_tick <= now_tick) {
            return Ok(0);
        }
        let sessions = self.lock()?.sessions.clone();
        let mut staged = BTreeMap::<String, Session>::new();
        let mut controls = BTreeMap::<String, BmsState>::new();
        let mut delivered = 0usize;
        let mut remaining = Vec::new();
        let mut notices = Vec::new();
        for item in queue.pending.drain(..) {
            if item.due_tick > now_tick {
                remaining.push(item);
                continue;
            }
            let Some(current) = sessions.get(&item.session) else {
                notices.push((item.errterm, item.terminal, item.title));
                continue;
            };
            if !current.connected
                || current.expires_at_tick < now_tick
                || current.input.terminal_id.as_deref() != Some(item.terminal.as_str())
            {
                notices.push((item.errterm, item.terminal, item.title));
                continue;
            }
            let mut proposed = staged
                .get(&item.session)
                .cloned()
                .unwrap_or_else(|| current.clone());
            let mut proposed_state = controls
                .get(&item.session)
                .cloned()
                .map(Ok)
                .unwrap_or_else(|| read_state(self, &item.session))?;
            if item
                .frames
                .iter()
                .try_for_each(|frame| {
                    control::apply_frame(&mut proposed_state, &mut proposed, frame)
                })
                .is_err()
            {
                notices.push((item.errterm, item.terminal, item.title));
                continue;
            }
            proposed.screen = item.image;
            staged.insert(item.session.clone(), proposed);
            controls.insert(item.session, proposed_state);
            delivered += 1;
        }
        for (errterm, terminal, title) in notices {
            let Some(errterm) = errterm else { continue };
            let Some((key, current)) = sessions.iter().find(|(_, session)| {
                session.connected
                    && session.expires_at_tick >= now_tick
                    && session.input.terminal_id.as_deref() == Some(errterm.as_str())
            }) else {
                continue;
            };
            let mut notice = b"ROUTE FAILED ".to_vec();
            notice.extend_from_slice(terminal.as_bytes());
            notice.push(b' ');
            notice.extend_from_slice(&title);
            notice.truncate(self.limits.max_screen_bytes);
            staged
                .entry(key.clone())
                .or_insert_with(|| current.clone())
                .screen = notice;
        }
        queue.pending = remaining;
        let mut writes = Vec::new();
        for (key, next) in &mut staged {
            next.version = sessions[key]
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            writes.push(session_write(key, &sessions[key], next)?);
            if let Some(control) = controls.get(key) {
                writes.push(state_write(key, control, self)?);
            }
        }
        writes.push(write(self, &queue)?);
        self.store
            .mutate_provider_states_atomic(writes)
            .map_err(store_error)
            .map_err(mutation_problem)?;
        let mut live = self.lock().map_err(mutation_problem)?;
        for (key, next) in staged {
            live.sessions.insert(key, next);
        }
        Ok(delivered)
    }
}
