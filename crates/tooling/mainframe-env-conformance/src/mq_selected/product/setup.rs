use super::*;
use mainframe_env_encoding::{AsciiEncodingProblem, encode_ascii};
use mainframe_env_host_api::mq_md_value::MqMdCharacterEncoding;
use mainframe_env_mq::{MqReplayClock, MqTrustedBatchProducerSource};
use mainframe_env_racf::{
    MemorySecretResolver, RacfManifest, RacfProfileDefinition, RacfService, RacfUserDefinition,
};
use mainframe_env_store::{MemoryStore, SqliteStateStore, StoreLimits};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);
pub(super) struct Backend {
    directory: Option<PathBuf>,
}
impl Backend {
    pub(super) fn new(sqlite: bool) -> Result<Self, String> {
        let directory = if sqlite {
            let p = std::env::temp_dir().join(format!(
                "mq-ir-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
            std::fs::create_dir(&p).map_err(|e| e.to_string())?;
            Some(p.canonicalize().map_err(|e| e.to_string())?)
        } else {
            None
        };
        Ok(Self { directory })
    }
    pub(super) fn open(&self) -> Result<Arc<dyn PlatformStore>, String> {
        match &self.directory {
            Some(p) => Ok(Arc::new(
                SqliteStateStore::open(
                    &format!("sqlite://{}?mode=rwc", p.join("state.sqlite").display()),
                    64 << 20,
                    256,
                )
                .map_err(|e| e.to_string())?,
            )),
            None => Ok(Arc::new(MemoryStore::new(StoreLimits {
                max_audits: 256,
                ..Default::default()
            }))),
        }
    }
}
impl Drop for Backend {
    fn drop(&mut self) {
        if let Some(p) = &self.directory {
            let _ = std::fs::remove_dir_all(p);
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetupRow {
    namespace: String,
    key: String,
    version: u64,
    payload: Vec<u8>,
}
pub(super) fn initialize(store: &dyn PlatformStore) -> Result<(), String> {
    // Generated ONLY by existing owner import/profile plan, before admission.
    // No live handle, intent, message, result or pending-unit seed is accepted.
    let stages: Vec<Vec<SetupRow>> = serde_json::from_str(SETUP).map_err(|e| e.to_string())?;
    if stages.len() != 4 || SETUP.len() > 128 << 10 {
        return Err("setup budget".into());
    }
    let mut prior = BTreeMap::new();
    for rows in stages {
        if rows.len() > 16 {
            return Err("setup count".into());
        }
        let mut next = BTreeMap::new();
        let mut changes = Vec::new();
        for row in rows {
            if !matches!(
                row.namespace.as_str(),
                "mq-state"
                    | "mq-v1-object-catalog"
                    | "mq-v1-queue"
                    | "mq-delivery-live-v1-meta"
                    | "mq-delivery-live-v1-queue"
            ) {
                return Err("non-quiescent setup fixture".into());
            }
            let identity = (row.namespace.clone(), row.key.clone());
            let record = ProviderStateRecord {
                namespace: row.namespace,
                key: row.key,
                version: row.version,
                payload: row.payload,
            };
            if prior.get(&identity) != Some(&record) {
                changes.push(ProviderStateMutation::Put(ProviderStateWrite {
                    expected_version: prior
                        .get(&identity)
                        .map(|r: &ProviderStateRecord| r.version),
                    record: record.clone(),
                }));
            }
            if next.insert(identity, record).is_some() {
                return Err("duplicate fixture row".into());
            }
        }
        for (identity, record) in &prior {
            if !next.contains_key(identity) {
                changes.push(ProviderStateMutation::Delete {
                    namespace: record.namespace.clone(),
                    key: record.key.clone(),
                    expected_version: record.version,
                });
            }
        }
        if !changes.is_empty() {
            store
                .mutate_provider_states_atomic(changes)
                .map_err(|e| e.to_string())?;
        }
        prior = next;
    }
    Ok(())
}
pub(super) struct Saf(
    pub(super) Mutex<super::security::Trace>,
    pub(super) Arc<RacfService>,
);
impl Saf {
    pub(super) fn new(store: Arc<dyn PlatformStore>) -> Result<Self, String> {
        let secrets = Arc::new(MemorySecretResolver::default());
        secrets.insert("mq-ir-fixture-password", b"MQIR-TEST-PASSWORD".to_vec());
        let service =
            RacfService::open(store, secrets, Default::default()).map_err(|e| e.to_string())?;
        service
            .install_manifest(RacfManifest {
                groups: BTreeSet::new(),
                users: vec![RacfUserDefinition {
                    user: "MQIR".into(),
                    credential: SecretRef::new("mq-ir-fixture-password", Default::default())
                        .unwrap(),
                    groups: BTreeSet::new(),
                }],
                profiles: [("MQQUEUE", "Q"), ("MQUOW", "CURRENT")]
                    .into_iter()
                    .map(|(class, pattern)| RacfProfileDefinition {
                        class: class.into(),
                        pattern: pattern.into(),
                        owner: "MQIR".into(),
                        uacc: None,
                        permissions: BTreeMap::from([("MQIR".into(), AccessIntent::Alter)]),
                    })
                    .collect(),
            })
            .map_err(|e| e.to_string())?;
        Ok(Self(Mutex::new(Default::default()), service))
    }
    pub(super) fn revoke(&self) -> Result<(), HostProblem> {
        self.1.set_user_state("MQIR", false, true, false)
    }
}
pub(super) struct Clock(pub(super) Arc<dyn PlatformStore>);
impl MqReplayClock for Clock {
    fn now_tick(&self) -> Result<u64, HostProblem> {
        let floor = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .as_millis();
        let floor = u64::try_from(floor).map_err(|_| HostProblem::ResourceExhausted)?;
        self.0
            .advance_logical_clock(floor)
            .map_err(|_| HostProblem::InfrastructureFailure)
    }
}
pub(super) struct Source(pub(super) Invocation);
impl MqTrustedBatchProducerSource for Source {
    fn encode_structure(
        &self,
        text: &str,
        chars: MqMdCharacterEncoding,
    ) -> Result<Vec<u8>, HostProblem> {
        if chars != MqMdCharacterEncoding::AsciiCompatible {
            return Err(HostProblem::Unsupported);
        }
        encode_ascii(text, 48).map_err(ascii_problem)
    }
    fn check_live(&self, invocation: &Invocation) -> Result<(), HostProblem> {
        if invocation != &self.0 || invocation.cancellation_requested() {
            return Err(HostProblem::Unauthorized);
        }
        Ok(())
    }
}
fn ascii_problem(problem: AsciiEncodingProblem) -> HostProblem {
    match problem {
        AsciiEncodingProblem::NonAscii | AsciiEncodingProblem::OutputLimit => {
            HostProblem::Unsupported
        }
        AsciiEncodingProblem::Allocation => HostProblem::ResourceExhausted,
    }
}

#[cfg(test)]
mod tests;
pub(super) fn descriptor() -> CapabilityDescriptor {
    CapabilityDescriptor {
        capability: CapabilityId::new("host.mq.write", Default::default()).unwrap(),
        provider_id: "mainframe-env-mq".into(),
        generation: "ir-foundation-v1".into(),
        request_schema: "mainframe-env.mq-request@1".into(),
        result_schema: "mainframe-env.mq-result@1".into(),
        max_request_bytes: 4 << 20,
        max_result_bytes: 4 << 20,
        ready: true,
    }
}
pub(super) fn invocation(now: u64) -> Invocation {
    let l = InvocationLimits::default();
    Invocation::new(
        RequestId::new("mq-ir-request", l).unwrap(),
        ExecutionId::new("mq-ir-execution", l).unwrap(),
        RunUnitId::new("mq-ir-run", l).unwrap(),
        None,
        Selector::new("conformance:mq-selected-v1", l).unwrap(),
        ArtifactRef::new("mq-selected-consumer@1", l).unwrap(),
        Principal::new(
            PrincipalId::new("MQIR", l).unwrap(),
            BTreeSet::from([CapabilityId::new("host.mq.write", l).unwrap()]),
            l,
        )
        .unwrap(),
        ServiceClass::System,
        0,
        now.checked_add(60_000).unwrap(),
        TraceId::new("mq-ir-trace", l).unwrap(),
        IdempotencyKey::new("mq-ir-invocation", l).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        l,
    )
    .unwrap()
    .with_cancellation_probe(CancellationProbe::new())
}
pub(super) fn key(sequence: u64) -> IdempotencyKey {
    IdempotencyKey::new(format!("mq-ir-{sequence}"), Default::default()).unwrap()
}
