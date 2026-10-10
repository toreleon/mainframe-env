use super::*;
use mainframe_env_host_api::{
    HostRequest, HostResult, canonical_request_digest, canonical_result_digest,
};
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateWrite};
use sha2::{Digest, Sha256};
use std::sync::atomic::Ordering;

const NAMESPACE: &str = "cics-web-channel-v1";

#[derive(Clone, Debug, Default)]
pub(super) struct WebState {
    pub(super) version: Option<u64>,
    pub(super) fields: BTreeMap<String, Vec<u8>>,
}

fn key(run: &Run, channel: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(run.invocation.run_unit_id.as_str().as_bytes());
    digest.update([0]);
    digest.update(channel.as_bytes());
    format!("{:x}", digest.finalize())
}

pub(super) fn load(
    service: &CicsService,
    run: &Run,
    channel: &str,
) -> Result<WebState, HostProblem> {
    let Some(record) = service
        .store
        .get_provider_state(NAMESPACE, &key(run, channel))
        .map_err(store_error)?
    else {
        return Ok(WebState::default());
    };
    if record.version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut input = Reader::new(&record.payload);
    if input.take(4)? != b"WSC1"
        || input.text(128)? != run.invocation.run_unit_id.as_str()
        || input.text(16)? != channel
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let count = usize::from(input.u16()?);
    if count > 64 || record.payload.len() > service.limits.max_transform_bytes {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut fields = BTreeMap::new();
    for _ in 0..count {
        let name = input.text(64)?;
        let value = input.bytes(service.limits.max_screen_bytes)?;
        if !valid_field_name(&name) || fields.insert(name, value).is_some() {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    if !input.finished() {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(WebState {
        version: Some(record.version),
        fields,
    })
}

pub(super) fn usage(service: &CicsService) -> Result<(usize, usize), HostProblem> {
    let rows = service
        .store
        .list_provider_state(NAMESPACE, service.limits.max_transform_containers)
        .map_err(store_error)?;
    let bytes = rows
        .iter()
        .try_fold(0usize, |sum, row| sum.checked_add(row.payload.len()))
        .ok_or(HostProblem::InfrastructureFailure)?;
    Ok((rows.len(), bytes))
}

pub(super) fn persist(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
    retention_tick: u64,
    channel: &str,
    state: &WebState,
    response: &CicsResponse,
) -> Result<(), HostProblem> {
    persist_with(
        service,
        run,
        request,
        retention_tick,
        channel,
        state,
        response,
        AdditionalWrites {
            writes: Vec::new(),
            replacement_transform_total: None,
        },
    )
}

pub(super) struct AdditionalWrites {
    pub writes: Vec<ProviderStateWrite>,
    pub replacement_transform_total: Option<usize>,
}

pub(super) fn persist_with(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
    retention_tick: u64,
    channel: &str,
    state: &WebState,
    response: &CicsResponse,
    additional: AdditionalWrites,
) -> Result<(), HostProblem> {
    let AdditionalWrites {
        writes: extra,
        replacement_transform_total,
    } = additional;
    let payload = encode(state, run, channel, service.limits)?;
    let rows = service
        .store
        .list_provider_state(NAMESPACE, service.limits.max_transform_containers)
        .map_err(store_error)?;
    let transform = service.lock()?;
    if state.version.is_none()
        && rows
            .len()
            .checked_add(transform.transform_containers.len())
            .is_none_or(|count| count >= service.limits.max_transform_containers)
    {
        return Err(HostProblem::ResourceExhausted);
    }
    let prior = rows
        .iter()
        .find(|row| row.key == key(run, channel))
        .map_or(0, |row| row.payload.len());
    let current = rows
        .iter()
        .try_fold(0usize, |sum, row| sum.checked_add(row.payload.len()))
        .ok_or(HostProblem::InfrastructureFailure)?;
    if current
        .checked_sub(prior)
        .and_then(|total| total.checked_add(payload.len()))
        .and_then(|total| {
            total.checked_add(replacement_transform_total.unwrap_or(transform.transform_bytes))
        })
        .is_none_or(|total| total > service.limits.max_transform_bytes)
    {
        return Err(HostProblem::ResourceExhausted);
    }
    drop(transform);
    let version = state
        .version
        .unwrap_or(0)
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    let mut writes = vec![
        ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: NAMESPACE.into(),
                key: key(run, channel),
                version,
                payload,
            },
            expected_version: state.version,
        },
        replay_write(run, request, retention_tick, response)?,
    ];
    writes.extend(extra);
    service
        .store
        .put_provider_states_atomic(writes)
        .map_err(store_error)?;
    if service
        .replay_unknown_after_persist
        .swap(false, Ordering::SeqCst)
    {
        Err(HostProblem::UnknownOutcome)
    } else {
        Ok(())
    }
}

pub(super) fn release_task(service: &CicsService, run: &Run) -> Result<(), HostProblem> {
    let rows = service
        .store
        .list_provider_state(NAMESPACE, service.limits.max_transform_containers)
        .map_err(store_error)?;
    for row in rows {
        let mut input = Reader::new(&row.payload);
        if input.take(4)? != b"WSC1" {
            return Err(HostProblem::InfrastructureFailure);
        }
        if input.text(128)? == run.invocation.run_unit_id.as_str() {
            let channel = input.text(16)?;
            let validated = load(service, run, &channel)?;
            if validated.version != Some(row.version) || row.key != key(run, &channel) {
                return Err(HostProblem::InfrastructureFailure);
            }
            service
                .store
                .delete_provider_state(NAMESPACE, &row.key, row.version)
                .map_err(store_error)?;
        }
    }
    Ok(())
}

fn replay_write(
    run: &Run,
    request: &CicsRequest,
    retention_tick: u64,
    response: &CicsResponse,
) -> Result<ProviderStateWrite, HostProblem> {
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let request_digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let result_digest = canonical_result_digest(&Ok(HostResult::Cics(response.clone())))
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let mut replay = CicsEffectReplay {
        effect_key: Some(mutation.idempotency_key.as_str().into()),
        owner_execution: Some(run.invocation.execution_id.as_str().into()),
        owner_run_unit: Some(run.invocation.run_unit_id.as_str().into()),
        sequence: Some(mutation.sequence),
        deadline_tick: Some(retention_tick),
        resolution_tick: None,
        request_digest,
        result_digest: Some(result_digest),
        binding_digest: None,
        response: response.clone(),
    };
    replay.binding_digest = Some(cics_effect_replay_binding_digest(&replay));
    Ok(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: "cics-effect-replay-v1".into(),
            key: mutation.idempotency_key.as_str().into(),
            version: 1,
            payload: encode_cics_effect_replay(&replay)?,
        },
        expected_version: None,
    })
}

fn encode(
    state: &WebState,
    run: &Run,
    channel: &str,
    limits: CicsLimits,
) -> Result<Vec<u8>, HostProblem> {
    if state.fields.len() > 64 {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut bytes = b"WSC1".to_vec();
    put_text(&mut bytes, run.invocation.run_unit_id.as_str(), 128)?;
    put_text(&mut bytes, channel, 16)?;
    bytes.extend_from_slice(
        &u16::try_from(state.fields.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for (name, value) in &state.fields {
        if !valid_field_name(name) || value.len() > limits.max_screen_bytes {
            return Err(HostProblem::Malformed);
        }
        put_text(&mut bytes, name, 64)?;
        bytes.extend_from_slice(
            &u32::try_from(value.len())
                .map_err(|_| HostProblem::ResourceExhausted)?
                .to_be_bytes(),
        );
        bytes.extend_from_slice(value);
    }
    if bytes.len() > limits.max_transform_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(bytes)
}

fn valid_field_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|byte| {
            byte.is_ascii_uppercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'_')
        })
}

fn put_text(bytes: &mut Vec<u8>, value: &str, max: usize) -> Result<(), HostProblem> {
    if value.len() > max {
        return Err(HostProblem::ResourceExhausted);
    }
    bytes.extend_from_slice(
        &u16::try_from(value.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    bytes.extend_from_slice(value.as_bytes());
    Ok(())
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }
    fn take(&mut self, length: usize) -> Result<&'a [u8], HostProblem> {
        let end = self
            .at
            .checked_add(length)
            .ok_or(HostProblem::InfrastructureFailure)?;
        let result = self
            .bytes
            .get(self.at..end)
            .ok_or(HostProblem::InfrastructureFailure)?;
        self.at = end;
        Ok(result)
    }
    fn u16(&mut self) -> Result<u16, HostProblem> {
        Ok(u16::from_be_bytes(
            self.take(2)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ))
    }
    fn bytes(&mut self, max: usize) -> Result<Vec<u8>, HostProblem> {
        let length = u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ) as usize;
        if length > max {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(self.take(length)?.to_vec())
    }
    fn text(&mut self, max: usize) -> Result<String, HostProblem> {
        let length = usize::from(self.u16()?);
        if length > max {
            return Err(HostProblem::InfrastructureFailure);
        }
        String::from_utf8(self.take(length)?.to_vec())
            .map_err(|_| HostProblem::InfrastructureFailure)
    }
    fn finished(&self) -> bool {
        self.at == self.bytes.len()
    }
}
