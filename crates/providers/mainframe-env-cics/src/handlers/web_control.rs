use super::super::{CicsService, Run, store_error};
use mainframe_env_host_api::{CicsOperation, CicsRequest, CicsResponse, HostProblem};
use mainframe_env_store_api::{ProviderStateMutation, ProviderStateRecord, ProviderStateWrite};
use std::collections::BTreeMap;
use std::sync::Arc;

mod model;
mod open;
mod parse_url;
pub use model::{CicsWebEndpoint, CicsWebTransport, CicsWebUriMapDefinition, CicsWebVersion};
pub(in crate::service) use model::{WebState, load as load_web_state};

impl CicsService {
    /// Install the reviewed outbound HTTP transport for CICS web sessions.
    pub fn install_web_transport(
        &self,
        transport: Arc<dyn CicsWebTransport>,
    ) -> Result<(), HostProblem> {
        let mut state = self.lock()?;
        if let Some(existing) = state.web.transport.as_ref() {
            return if Arc::ptr_eq(existing, &transport) {
                Ok(())
            } else {
                Err(HostProblem::IdempotencyConflict)
            };
        }
        state.web.transport = Some(transport);
        Ok(())
    }

    /// Install bounded USAGE(CLIENT) URIMAP definitions durably and atomically.
    pub fn register_web_urimaps(
        &self,
        definitions: &[CicsWebUriMapDefinition],
    ) -> Result<(), HostProblem> {
        if definitions.is_empty() || definitions.len() > self.limits.max_web_sessions {
            return Err(HostProblem::Malformed);
        }
        let mut normalized = BTreeMap::new();
        for definition in definitions {
            let mut definition = definition.clone();
            definition.name = definition.name.trim().to_ascii_uppercase();
            definition.scheme = definition.scheme.trim().to_ascii_uppercase();
            definition.host = definition.host.trim().to_ascii_lowercase();
            if !definition.path.starts_with('/') {
                definition.path.insert(0, '/');
            }
            if definition.name.is_empty()
                || definition.name.len() > 8
                || !definition
                    .name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric())
                || !matches!(definition.scheme.as_str(), "HTTP" | "HTTPS")
                || definition.host.is_empty()
                || definition.host.len() > 255
                || !definition
                    .host
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b':'))
                || definition.path.len() > 4096
                || definition.path.contains(['?', '#'])
                || definition.certificate.as_ref().is_some_and(|label| {
                    definition.scheme != "HTTPS"
                        || label.is_empty()
                        || label.len() > 32
                        || !label.bytes().all(|byte| byte.is_ascii_alphanumeric())
                })
                || normalized
                    .insert(definition.name.clone(), definition)
                    .is_some()
            {
                return Err(HostProblem::Malformed);
            }
        }
        let mut state = self.lock()?;
        let mut writes = Vec::new();
        let mut additional_bytes = 0usize;
        for (name, definition) in &normalized {
            if let Some(existing) = state.web.urimaps.get(name) {
                if existing != definition {
                    return Err(HostProblem::IdempotencyConflict);
                }
                continue;
            }
            let payload = model::encode_urimap(definition)?;
            additional_bytes = additional_bytes
                .checked_add(payload.len())
                .ok_or(HostProblem::ResourceExhausted)?;
            writes.push(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: model::URIMAP_NAMESPACE.into(),
                    key: name.clone(),
                    version: 1,
                    payload,
                },
                expected_version: None,
            });
        }
        if state.web.urimaps.len() + writes.len() > self.limits.max_web_sessions
            || state
                .web
                .bytes
                .checked_add(additional_bytes)
                .is_none_or(|total| total > self.limits.max_web_bytes)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        if !writes.is_empty() {
            self.store
                .put_provider_states_atomic(writes)
                .map_err(store_error)?;
            state.web.bytes += additional_bytes;
            state.web.urimaps.extend(normalized);
        }
        Ok(())
    }
}

pub(super) fn release_task(service: &CicsService, run: &Run) -> Result<(), HostProblem> {
    let mut state = service.lock()?;
    let owned = state
        .web
        .sessions
        .iter()
        .filter(|(_, session)| {
            session.owner_execution == run.invocation.execution_id.as_str()
                && session.owner_run_unit == run.invocation.run_unit_id.as_str()
        })
        .map(|(key, session)| (key.clone(), session.clone()))
        .collect::<Vec<_>>();
    if owned.is_empty() {
        return Ok(());
    }
    let deletes = owned
        .iter()
        .map(|(key, session)| ProviderStateMutation::Delete {
            namespace: model::SESSION_NAMESPACE.into(),
            key: key.clone(),
            expected_version: session.version,
        })
        .collect();
    service
        .store
        .mutate_provider_states_atomic(deletes)
        .map_err(store_error)?;
    let transport = state.web.transport.clone();
    for (key, session) in &owned {
        state.web.bytes = state
            .web
            .bytes
            .checked_sub(model::encode_session(session)?.len())
            .ok_or(HostProblem::InfrastructureFailure)?;
        state.web.sessions.remove(key);
    }
    drop(state);
    if let Some(transport) = transport {
        for (_, session) in owned {
            transport
                .release(&session.endpoint, session.token, false, &run.invocation)
                .map_err(|_| HostProblem::UnknownOutcome)?;
        }
    }
    Ok(())
}

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    match request.operation {
        CicsOperation::WebParseUrl => parse_url::invoke(service, run, request),
        CicsOperation::WebOpen => open::invoke(service, run, request, run.invocation.deadline_tick),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}
