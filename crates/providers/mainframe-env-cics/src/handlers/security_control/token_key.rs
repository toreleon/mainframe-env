//! Durable, task-scoped one-use key handles from VERIFY TOKEN.

use super::super::super::{CicsService, Reader, Run, field, store_error};
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::ProviderStateRecord;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const NAMESPACE: &str = "cics-token-key-v1";
const SCHEMA: &[u8; 8] = b"MECTKEY1";

pub(super) struct TaskEncryptKey {
    pub owner: String,
    pub handle: [u8; 4],
    pub key: Zeroizing<[u8; 32]>,
    pub effect_key: String,
    pub request_digest: [u8; 32],
    pub issued_tick: u64,
    pub expires_tick: u64,
    pub consumed: bool,
    pub version: u64,
}

pub(super) fn install(
    service: &CicsService,
    run: &Run,
    token: &[u8],
    effect_key: &str,
    request_digest: [u8; 32],
    tick: u64,
) -> Result<[u8; 4], HostProblem> {
    let old = load(service, run)?;
    if old.as_ref().is_some_and(|old| old.effect_key == effect_key) {
        return Err(HostProblem::UnknownOutcome);
    }
    let version = old.as_ref().map_or(Ok(1), |old| {
        old.version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)
    })?;
    let key: [u8; 32] = Sha256::digest(token).into();
    let mut digest = Sha256::new();
    digest.update(b"mainframe-env.cics-token-handle@1\0");
    digest.update(key);
    digest.update(run.invocation.run_unit_id.as_str().as_bytes());
    digest.update(effect_key.as_bytes());
    let tag = digest.finalize();
    let handle: [u8; 4] = tag[..4]
        .try_into()
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let record = TaskEncryptKey {
        owner: run.invocation.principal.id().as_str().into(),
        handle,
        key: Zeroizing::new(key),
        effect_key: effect_key.into(),
        request_digest,
        issued_tick: tick,
        expires_tick: run.invocation.deadline_tick,
        consumed: false,
        version,
    };
    service
        .store
        .put_provider_state(
            ProviderStateRecord {
                namespace: NAMESPACE.into(),
                key: run.invocation.run_unit_id.as_str().into(),
                version,
                payload: encode(&record)?,
            },
            old.as_ref().map(|old| old.version),
        )
        .map_err(store_error)
        .map_err(|_| HostProblem::UnknownOutcome)?;
    Ok(handle)
}

pub(super) fn valid(
    service: &CicsService,
    run: &Run,
    supplied: &[u8],
    tick: u64,
) -> Result<Option<TaskEncryptKey>, HostProblem> {
    let Some(record) = load(service, run)? else {
        return Ok(None);
    };
    Ok((supplied == record.handle
        && record.owner == run.invocation.principal.id().as_str()
        && !record.consumed
        && tick < record.expires_tick)
        .then_some(record))
}

pub(super) fn consume(
    service: &CicsService,
    run: &Run,
    expected: &TaskEncryptKey,
) -> Result<(), HostProblem> {
    let Some(mut current) = load(service, run)? else {
        return Err(HostProblem::UnknownOutcome);
    };
    if current.version != expected.version || current.handle != expected.handle || current.consumed
    {
        return Err(HostProblem::UnknownOutcome);
    }
    current.version = current
        .version
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    current.consumed = true;
    service
        .store
        .put_provider_state(
            ProviderStateRecord {
                namespace: NAMESPACE.into(),
                key: run.invocation.run_unit_id.as_str().into(),
                version: current.version,
                payload: encode(&current)?,
            },
            Some(expected.version),
        )
        .map_err(store_error)
        .map_err(|_| HostProblem::UnknownOutcome)
}

pub(super) fn release_task(service: &CicsService, run: &Run) -> Result<(), HostProblem> {
    if let Some(record) = load(service, run)? {
        service
            .store
            .delete_provider_state(
                NAMESPACE,
                run.invocation.run_unit_id.as_str(),
                record.version,
            )
            .map_err(store_error)?;
    }
    Ok(())
}

fn load(service: &CicsService, run: &Run) -> Result<Option<TaskEncryptKey>, HostProblem> {
    service
        .store
        .get_provider_state(NAMESPACE, run.invocation.run_unit_id.as_str())
        .map_err(store_error)?
        .map(|row| decode(&row.payload, row.version))
        .transpose()
}

fn encode(record: &TaskEncryptKey) -> Result<Vec<u8>, HostProblem> {
    if record.owner.is_empty()
        || record.owner.len() > 8
        || record.effect_key.is_empty()
        || record.effect_key.len() > 128
        || record.issued_tick >= record.expires_tick
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut out = SCHEMA.to_vec();
    field(&mut out, record.owner.as_bytes())?;
    field(&mut out, record.effect_key.as_bytes())?;
    out.extend_from_slice(&record.handle);
    out.extend_from_slice(record.key.as_ref());
    out.extend_from_slice(&record.request_digest);
    out.extend_from_slice(&record.issued_tick.to_be_bytes());
    out.extend_from_slice(&record.expires_tick.to_be_bytes());
    out.push(u8::from(record.consumed));
    Ok(out)
}

fn decode(bytes: &[u8], version: u64) -> Result<TaskEncryptKey, HostProblem> {
    let mut reader = Reader { bytes, at: 0 };
    if reader.take(8)? != SCHEMA {
        return Err(HostProblem::InfrastructureFailure);
    }
    let owner =
        String::from_utf8(reader.field(8)?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let effect_key =
        String::from_utf8(reader.field(128)?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let handle = reader
        .take(4)?
        .try_into()
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let key = reader
        .take(32)?
        .try_into()
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let request_digest = reader
        .take(32)?
        .try_into()
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let issued_tick = u64::from_be_bytes(
        reader
            .take(8)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    );
    let expires_tick = u64::from_be_bytes(
        reader
            .take(8)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    );
    let consumed = match reader.take(1)?[0] {
        0 => false,
        1 => true,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    if reader.at != bytes.len() {
        return Err(HostProblem::InfrastructureFailure);
    }
    let record = TaskEncryptKey {
        owner,
        handle,
        key: Zeroizing::new(key),
        effect_key,
        request_digest,
        issued_tick,
        expires_tick,
        consumed,
        version,
    };
    encode(&record)?;
    Ok(record)
}
