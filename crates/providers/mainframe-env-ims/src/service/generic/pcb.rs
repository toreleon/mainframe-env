//! DB PCB selection, sensitivity and retained per-PCB position. The scheduled
//! PCB remains in Session.pcb/position; the map contains only other DB PCBs.

use super::*;
use serde::de::{Error, MapAccess, Visitor};
use std::fmt;

pub(in crate::service) fn position(session: &Session, number: u16) -> PcbPosition {
    if number == session.pcb {
        session.position.clone()
    } else {
        session
            .pcb_positions
            .get(&number)
            .cloned()
            .unwrap_or_default()
    }
}

pub(in crate::service) fn set_position(session: &mut Session, number: u16, position: PcbPosition) {
    if number == session.pcb {
        session.position = position;
    } else {
        session.pcb_positions.insert(number, position);
    }
}

pub(in crate::service) fn clear_positions(session: &mut Session) {
    session.position = PcbPosition::default();
    session.pcb_positions.clear();
}

pub(super) fn session_databases(state: &State, run: &str) -> Result<BTreeSet<String>, HostProblem> {
    let session = state.sessions.get(run).ok_or(HostProblem::NotFound)?;
    let mut databases = BTreeSet::new();
    for number in std::iter::once(session.pcb).chain(session.pcb_positions.keys().copied()) {
        databases.insert(normalize(
            &scheduled_pcb(state, &session.psb, number)?.1.database,
        ));
    }
    if let Some(pending) = state.generic_pending_undo.get(run) {
        databases.extend(pending.keys().cloned());
    }
    if let Some(pending) = state.pending_undo.get(run) {
        databases.extend(pending.keys().cloned());
    }
    Ok(databases)
}

pub(in crate::service) fn key_only(pcb: &ImsDatabasePcbMetadata, segment: &str) -> bool {
    pcb.sensitive_segments
        .iter()
        .find(|item| normalize(&item.name) == segment)
        .and_then(|item| item.processing_options.as_deref())
        .is_some_and(|options| options.contains('K'))
}

// GNP's parent-level qualification mismatch is GE with unchanged position;
// requesting a target at or above parentage is GP. The engine distinguishes
// both from absent parentage, but shares PathMismatch for these two failures.
pub(super) fn gnp_target_below_parent(
    engine: &DatabaseEngine,
    position: &PcbPosition,
    read: &ReadRequest,
) -> bool {
    if read.kind != ReadKind::NextInParent {
        return false;
    }
    let Some(parent) = position
        .parentage()
        .and_then(|id| engine.path_to(id).ok())
        .and_then(|path| path.last().cloned())
    else {
        return false;
    };
    let mut target = read.target.as_deref();
    while let Some(segment) = target.and_then(|name| {
        engine
            .definition()
            .segments
            .iter()
            .find(|segment| segment.name == name)
    }) {
        if segment.parent.as_deref() == Some(parent.segment.as_str()) {
            return true;
        }
        target = segment.parent.as_deref();
    }
    false
}

// DLET already resets other runs. In the deleting run, preserve unrelated PCB
// holds/positions and invalidate only another PCB whose referenced occurrence
// or parentage was removed. The selected PCB receives the engine's DLET position.
pub(super) fn reset_deleted_positions(
    state: &mut State,
    run: &str,
    selected: u16,
    database: &str,
    limits: ImsLimits,
) -> Result<(), HostProblem> {
    let session = state.sessions.get(run).ok_or(HostProblem::NotFound)?;
    let engine = restored(state, database, limits)?;
    let invalid = std::iter::once(session.pcb)
        .chain(session.pcb_positions.keys().copied())
        .filter(|number| *number != selected)
        .filter(|number| {
            scheduled_pcb(state, &session.psb, *number)
                .is_ok_and(|(_, pcb)| normalize(&pcb.database) == database)
        })
        .filter(|number| {
            engine
                .validate_position(&position(session, *number))
                .is_err()
        })
        .collect::<Vec<_>>();
    let session = Arc::make_mut(state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?);
    for number in invalid {
        if number == session.pcb {
            session.position = PcbPosition::default();
        } else {
            session.pcb_positions.remove(&number);
        }
    }
    Ok(())
}

pub(super) fn read_status(
    pcb: &ImsDatabasePcbMetadata,
    request: &ImsRequest,
    read: &ReadRequest,
) -> Option<&'static str> {
    // AC: an SSA names a segment absent from the selected PCB. Do not change
    // position, parentage or hold on this specification error.
    for segment in request
        .segments
        .iter()
        .chain(request.qualifiers.iter().map(|item| &item.segment))
    {
        if !pcb
            .sensitive_segments
            .iter()
            .any(|item| normalize(&item.name) == normalize(segment))
        {
            return Some("AC");
        }
    }
    if let Some(target) = &read.target {
        if !pcb
            .sensitive_segments
            .iter()
            .any(|item| normalize(&item.name) == *target)
        {
            return Some("AC");
        }
        // Explicit key sensitivity establishes position but never returns data.
        if !allowed(pcb, target, request.operation) && !key_only(pcb, target) {
            return Some("AM");
        }
    } else if !pcb.sensitive_segments.iter().any(|item| {
        allowed(pcb, &normalize(&item.name), request.operation)
            || key_only(pcb, &normalize(&item.name))
    }) {
        return Some("AM");
    }
    None
}

pub(super) fn reset_database_positions(state: &mut State, database: &str, except: Option<&str>) {
    let resets = state
        .sessions
        .iter()
        .filter(|(run, session)| except != Some(run.as_str()) && session.generic)
        .map(|(run, session)| {
            let numbers = std::iter::once(session.pcb)
                .chain(session.pcb_positions.keys().copied())
                .filter(|number| {
                    scheduled_pcb(state, &session.psb, *number)
                        .is_ok_and(|(_, pcb)| normalize(&pcb.database) == database)
                })
                .collect::<Vec<_>>();
            (run.clone(), numbers)
        })
        .collect::<Vec<_>>();
    for (run, numbers) in resets {
        let session = Arc::make_mut(state.sessions.get_mut(&run).expect("collected session"));
        for number in numbers {
            if number == session.pcb {
                session.position = PcbPosition::default();
            } else {
                session.pcb_positions.remove(&number);
            }
        }
    }
}

pub(super) fn validate_sessions(state: &State, limits: ImsLimits) -> Result<(), HostProblem> {
    for (session, live) in state
        .sessions
        .values()
        .map(|session| (session, true))
        .chain(state.checkpoints.values().map(|session| (session, false)))
    {
        if !session.generic {
            if !session.pcb_positions.is_empty() {
                return Err(HostProblem::InfrastructureFailure);
            }
            continue;
        }
        if session.pcb_positions.len() >= limits.max_pcbs
            || session.pcb_positions.contains_key(&session.pcb)
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        for number in std::iter::once(session.pcb).chain(session.pcb_positions.keys().copied()) {
            if usize::from(number) > limits.max_pcbs {
                return Err(HostProblem::InfrastructureFailure);
            }
            let (_, pcb) = scheduled_pcb(state, &session.psb, number)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            if !position(session, number).valid_retained_shape() {
                return Err(HostProblem::InfrastructureFailure);
            }
            if position(session, number)
                .secondary_index()
                .is_some_and(|name| {
                    pcb.secondary_index
                        .as_deref()
                        .is_none_or(|selected| normalize(selected) != name)
                })
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            if live {
                restored(state, &normalize(&pcb.database), limits)?
                    .validate_position(&position(session, number))
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
            }
        }
    }
    Ok(())
}

/// Reject duplicate and noncanonical map keys rather than letting serde's
/// BTreeMap reader overwrite an existing PCB position.
pub(in crate::service) fn read_positions<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<u16, PcbPosition>, D::Error> {
    struct Positions;
    impl<'de> Visitor<'de> for Positions {
        type Value = BTreeMap<u16, PcbPosition>;
        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("unique canonical DB PCB position keys")
        }
        fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
            let mut positions = BTreeMap::new();
            while let Some((key, position)) = map.next_entry::<String, PcbPosition>()? {
                let number = key.parse::<u16>().map_err(M::Error::custom)?;
                if number == 0
                    || key != number.to_string()
                    || positions.insert(number, position).is_some()
                {
                    return Err(M::Error::custom("invalid or duplicate DB PCB position key"));
                }
            }
            Ok(positions)
        }
    }
    deserializer.deserialize_map(Positions)
}
