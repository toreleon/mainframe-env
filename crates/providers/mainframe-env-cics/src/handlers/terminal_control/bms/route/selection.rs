//! Source-bounded route operands and local terminal selection.

use super::*;
use mainframe_env_host_api::AccessIntent;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const CLASS_NAMESPACE: &str = "cics-route-operator-classes-v1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct OperatorClasses(Vec<u8>);

impl CicsService {
    /// Register immutable operator classes (1–24) for local ROUTE OPCLASS selection.
    pub fn register_route_operator_classes(
        &self,
        assignments: &[(String, Vec<u8>)],
    ) -> Result<(), HostProblem> {
        if assignments.is_empty() || assignments.len() > self.limits.max_sessions {
            return Err(HostProblem::Malformed);
        }
        let mut seen = BTreeSet::new();
        let mut writes = Vec::new();
        for (principal, classes) in assignments {
            if principal.is_empty()
                || principal.len() > 64
                || !principal.bytes().all(|byte| byte.is_ascii_alphanumeric())
                || classes.is_empty()
                || classes.len() > 24
                || classes.iter().any(|class| !(1..=24).contains(class))
                || classes.iter().copied().collect::<BTreeSet<_>>().len() != classes.len()
                || !seen.insert(principal.to_ascii_uppercase())
            {
                return Err(HostProblem::Malformed);
            }
            let key = principal.to_ascii_uppercase();
            let payload = serde_json::to_vec(&OperatorClasses(classes.clone()))
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            if let Some(existing) = self
                .store
                .get_provider_state(CLASS_NAMESPACE, &key)
                .map_err(store_error)?
            {
                if existing.version != 1 || existing.payload != payload {
                    return Err(HostProblem::IdempotencyConflict);
                }
                continue;
            }
            writes.push(ProviderStateMutation::Put(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: CLASS_NAMESPACE.into(),
                    key,
                    version: 1,
                    payload,
                },
                expected_version: None,
            }));
        }
        if !writes.is_empty() {
            self.store
                .mutate_provider_states_atomic(writes)
                .map_err(store_error)?;
        }
        Ok(())
    }
}

fn classes_for(service: &CicsService, principal: &str) -> Result<Vec<u8>, HostProblem> {
    let Some(row) = service
        .store
        .get_provider_state(CLASS_NAMESPACE, principal)
        .map_err(store_error)?
    else {
        return Ok(Vec::new());
    };
    if row.version != 1 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let classes: OperatorClasses =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    if classes.0.is_empty()
        || classes.0.len() > 24
        || classes.0.iter().any(|class| !(1..=24).contains(class))
        || classes.0.iter().copied().collect::<BTreeSet<_>>().len() != classes.0.len()
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(classes.0)
}

pub(super) fn validate_request(
    request: &CicsRequest,
    max_screen: usize,
) -> Result<(), HostProblem> {
    if request.operation != CicsOperation::Route
        || request.mutation.is_none()
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
    {
        return Err(HostProblem::Malformed);
    }
    let mut timing = 0;
    for name in ["INTERVAL", "TIME", "OPTION.AFTER", "OPTION.AT"] {
        timing += usize::from(request.arguments.contains_key(name));
    }
    if timing > 1 {
        return Err(HostProblem::Malformed);
    }
    let has_component = ["HOURS", "MINUTES", "SECONDS"]
        .iter()
        .any(|name| request.arguments.contains_key(*name));
    if has_component
        != (request.arguments.contains_key("OPTION.AFTER")
            || request.arguments.contains_key("OPTION.AT"))
    {
        return Err(HostProblem::Malformed);
    }
    for (name, value) in &request.arguments {
        let valid = match name.as_str() {
            "RESP" | "RESP2" => value.schema() == "mainframe-env.cics.argument@1",
            "OPTION.AFTER" | "OPTION.AT" | "OPTION.NLEOM" | "OPTION.NOHANDLE" => {
                value.schema() == "mainframe-env.cics.option@1" && value.bytes().is_empty()
            }
            "INTERVAL" | "TIME" | "HOURS" | "MINUTES" | "SECONDS" => {
                value.schema() == "mainframe-env.cics.decimal@1"
            }
            "ERRTERM" | "REQID" | "LDC" => matches!(
                value.schema(),
                "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
            ),
            "TITLE" | "LIST" | "OPCLASS" => value.schema() == "mainframe-env.cics.storage-value@1",
            _ => false,
        };
        if !valid {
            return Err(HostProblem::Malformed);
        }
    }
    if request
        .arguments
        .get("TITLE")
        .is_some_and(|x| x.bytes().len() > 256)
        || request.arguments.get("LIST").is_some_and(|x| {
            x.bytes().is_empty() || x.bytes().len() % 16 != 0 || x.bytes().len() > max_screen
        })
        || request
            .arguments
            .get("OPCLASS")
            .is_some_and(|x| x.bytes().len() != 3)
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

pub(super) fn text_name(
    request: &CicsRequest,
    name: &str,
    length: usize,
) -> Result<Option<String>, HostProblem> {
    request
        .arguments
        .get(name)
        .map(|value| {
            let bytes = value.bytes();
            if bytes.len() != length
                || !bytes
                    .iter()
                    .all(|b| b.is_ascii_alphanumeric() || *b == b'*')
            {
                return Err(HostProblem::Malformed);
            }
            String::from_utf8(bytes.to_ascii_uppercase()).map_err(|_| HostProblem::Malformed)
        })
        .transpose()
}

pub(super) fn recipients(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    sessions: &BTreeMap<String, Session>,
) -> Result<(Vec<(String, Session)>, usize), HostProblem> {
    if request.arguments.contains_key("LDC") {
        return Err(condition("INVLDC", 41));
    }
    if let Some(errterm) = text_name(request, "ERRTERM", 4)?
        && !sessions.values().any(|session| {
            session.connected && session.input.terminal_id.as_deref() == Some(errterm.as_str())
        })
    {
        return Err(condition("INVERRTERM", 37));
    }
    let mut candidates = BTreeSet::new();
    let mut failed = 0;
    if let Some(list) = request.arguments.get("LIST") {
        for entry in list.bytes().as_chunks::<16>().0 {
            if entry[10..16] != [b' '; 6] {
                return Err(condition("INVREQ", 16));
            }
            if entry[8..10] != [b' '; 2] {
                return Err(condition("INVLDC", 41));
            }
            let terminal = std::str::from_utf8(&entry[0..4])
                .map_err(|_| HostProblem::Malformed)?
                .trim();
            let operator = std::str::from_utf8(&entry[4..8])
                .map_err(|_| HostProblem::Malformed)?
                .trim();
            if terminal.is_empty() == operator.is_empty()
                || !terminal
                    .bytes()
                    .chain(operator.bytes())
                    .all(|b| b.is_ascii_alphanumeric())
            {
                return Err(condition("INVREQ", 16));
            }
            let matches = sessions
                .iter()
                .filter(|(_, session)| {
                    session.connected
                        && if !terminal.is_empty() {
                            session.input.terminal_id.as_deref() == Some(terminal)
                        } else {
                            session.principal == operator
                        }
                })
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            if matches.is_empty() {
                failed += 1;
            }
            candidates.extend(matches);
        }
    } else {
        candidates.extend(
            sessions
                .iter()
                .filter(|(_, session)| session.connected && session.input.terminal_id.is_some())
                .map(|(key, _)| key.clone()),
        );
    }
    if let Some(mask) = request.arguments.get("OPCLASS") {
        for key in candidates.clone() {
            let session = &sessions[&key];
            let classes = classes_for(service, &session.principal)?;
            let selected = classes.into_iter().any(|class| {
                let index = usize::from((24 - class) / 8);
                let bit = (class - 1) % 8;
                mask.bytes()[index] & (1 << bit) != 0
            });
            if !selected {
                candidates.remove(&key);
            }
        }
    }
    candidates.remove(&run.session);
    let mut selected = Vec::new();
    for key in candidates {
        let session = sessions
            .get(&key)
            .ok_or(HostProblem::InfrastructureFailure)?;
        let Some(terminal) = session.input.terminal_id.as_deref() else {
            failed += 1;
            continue;
        };
        match service.authorize(
            run,
            "FACILITY",
            &format!("CICS.TERMINAL.ROUTE.{terminal}"),
            AccessIntent::Update,
        ) {
            Ok(()) => selected.push((key, session.clone())),
            Err(HostProblem::Unauthorized) => failed += 1,
            Err(error) => return Err(error),
        }
    }
    Ok((selected, failed))
}
