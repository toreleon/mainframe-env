//! Per-original-occurrence CAS metadata, not another result/journal codec.
//! Handle-bearing replies decode as historical observations. Only the separately
//! admitted live registry may resolve an already-existing entry for delivery.

use super::*;
use crate::mqi_admission::MqMqiAdmitted;
use mainframe_env_host_api::mq_mqi::MqMqiCall;
use mainframe_env_host_api::{HostResult, canonical_result_digest};
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateWrite};

pub(in crate::service) const NAMESPACE: &str = "mq-selected-v1-occurrence";
const SCHEMA: &str = "mainframe-env.mq-selected-occurrence@1";
#[path = "receipt/profile.rs"]
mod profile;
use profile::Profiles;

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(tag = "kind", deny_unknown_fields)]
enum StoredReply {
    Lossless {
        profiles: Profiles,
        #[serde(deserialize_with = "bounded_bytes")]
        bytes: Vec<u8>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(in crate::service) struct OccurrenceReceipt {
    schema_version: String,
    key: String,
    execution: String,
    run: String,
    principal: String,
    invocation_key: String,
    attempt: u32,
    sequence: u64,
    deadline: u64,
    observed_tick: u64,
    generation: u64,
    fence: u64,
    registry_epoch: u64,
    call: String,
    request_digest: [u8; 32],
    pub(super) result_digest: [u8; 32],
    /// Explicit authority requirement, not stored numeric handle components.
    reply: StoredReply,
    #[serde(deserialize_with = "authorization::bounded_resources")]
    resources: Vec<authorization::StoredResource>,
}

impl OccurrenceReceipt {
    /// Attribution for conservative retention protection after full restore.
    /// This supplies neither an execution permit nor a terminal age.
    pub(in crate::service) fn retained_dependency(&self) -> (&str, &str) {
        (&self.execution, &self.key)
    }

    pub(super) fn capture(
        admitted: &MqMqiAdmitted<'_>,
        reply: &EffectResult,
        control: &Control,
        now: u64,
        host_limits: HostLimits,
        byte_ceiling: usize,
        resources: Vec<authorization::StoredResource>,
    ) -> Result<Self, HostProblem> {
        let Ok(HostResult::MqMqi(result)) = &reply.outcome else {
            return Err(HostProblem::Unsupported);
        };
        let profiles = Profiles::new(host_limits, result.limits);
        profiles.values()?;
        let bytes =
            crate::mqi_replay::encode(&result.result, host_limits, result.limits, byte_ceiling)
                .map_err(codec_error)?;
        let reply_storage = StoredReply::Lossless { profiles, bytes };
        let inv = admitted.invocation();
        Ok(Self {
            schema_version: SCHEMA.into(),
            key: admitted.mutation.idempotency_key.as_str().into(),
            execution: inv.execution_id.as_str().into(),
            run: inv.run_unit_id.as_str().into(),
            principal: inv.principal.id().as_str().into(),
            invocation_key: inv.idempotency_key.as_str().into(),
            attempt: inv.attempt,
            sequence: admitted.effect().sequence,
            deadline: inv.deadline_tick.min(admitted.effect().deadline_tick),
            observed_tick: now,
            generation: control.generation,
            fence: control.fence,
            registry_epoch: control.registry_epoch,
            call: result.result.call.label().into(),
            request_digest: admitted.host_request_digest,
            result_digest: canonical_result_digest(&reply.outcome)?,
            reply: reply_storage,
            resources,
        })
    }

    pub(in crate::service) fn validate(
        &self,
        key: &str,
        control: &Control,
    ) -> Result<(), HostProblem> {
        authorization::validate(&self.resources)?;
        use mainframe_env_execution_api::{ExecutionId, IdempotencyKey, PrincipalId, RunUnitId};
        let l = InvocationLimits::default();
        if self.schema_version != SCHEMA
            || self.key != key
            || self.attempt == 0
            || self.sequence == 0
            || self.deadline == 0
            || self.deadline > i64::MAX as u64
            || self.observed_tick == 0
            || self.observed_tick >= self.deadline
            || self.generation != control.generation
            || self.fence != control.fence
            || self.registry_epoch == 0
            || self.registry_epoch > control.registry_epoch
            || !MqMqiCall::ALL
                .iter()
                .any(|c| c.label() == self.call && *c != MqMqiCall::CallbackFunction)
        {
            return Err(HostProblem::Malformed);
        }
        match &self.reply {
            StoredReply::Lossless { profiles, bytes } => {
                let (host, mqi) = profiles.values()?;
                let result = crate::mqi_replay::decode(bytes, host, mqi, mqi.canonical_bytes)
                    .map_err(codec_error)?;
                if result.call.label() != self.call
                    || canonical_result_digest(&Ok(HostResult::MqMqi(MqMqiHostResult {
                        limits: mqi,
                        result,
                    })))?
                        != self.result_digest
                {
                    return Err(HostProblem::Malformed);
                }
            }
        }
        ExecutionId::new(&self.execution, l).map_err(|_| HostProblem::Malformed)?;
        RunUnitId::new(&self.run, l).map_err(|_| HostProblem::Malformed)?;
        PrincipalId::new(&self.principal, l).map_err(|_| HostProblem::Malformed)?;
        IdempotencyKey::new(&self.invocation_key, l).map_err(|_| HostProblem::Malformed)?;
        IdempotencyKey::new(&self.key, l).map_err(|_| HostProblem::Malformed)?;
        Ok(())
    }

    pub(super) fn require_observed_time(&self, now: u64) -> Result<(), HostProblem> {
        if now < self.observed_tick {
            return Err(HostProblem::Malformed);
        }
        Ok(())
    }

    pub(super) fn matches(&self, admitted: &MqMqiAdmitted<'_>) -> bool {
        let inv = admitted.invocation();
        self.key == admitted.mutation.idempotency_key.as_str()
            && self.execution == inv.execution_id.as_str()
            && self.run == inv.run_unit_id.as_str()
            && self.principal == inv.principal.id().as_str()
            && self.invocation_key == inv.idempotency_key.as_str()
            && self.attempt == inv.attempt
            && self.sequence == admitted.effect().sequence
            && self.deadline == inv.deadline_tick.min(admitted.effect().deadline_tick)
            && self.call == admitted.envelope.request.call().label()
            && self.request_digest == admitted.host_request_digest
    }

    pub(super) fn authorize_replay(
        &self,
        authorizer: &dyn EnterpriseAuthorizer,
        invocation: &Invocation,
    ) -> Result<(), HostProblem> {
        authorization::replay(&self.resources, authorizer, invocation.principal.id())
    }

    pub(super) fn replay(
        &self,
        host: HostLimits,
        mqi: MqMqiLimits,
    ) -> Result<EffectResult, HostProblem> {
        let expected = Profiles::new(host, mqi);
        match &self.reply {
            StoredReply::Lossless { profiles, bytes } if *profiles == expected => {
                let result = crate::mqi_replay::decode(bytes, host, mqi, mqi.canonical_bytes)
                    .map_err(codec_error)?;
                Ok(EffectResult {
                    sequence: self.sequence,
                    outcome: Ok(HostResult::MqMqi(MqMqiHostResult {
                        limits: mqi,
                        result,
                    })),
                })
            }
            _ => Err(HostProblem::IdempotencyConflict),
        }
    }

    pub(super) fn insertion(&self, limits: MqLimits) -> Result<ProviderStateMutation, HostProblem> {
        let payload = encode_object_row(&self.key, self)?;
        if payload.len() > limits.max_state_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: NAMESPACE.into(),
                key: self.key.clone(),
                version: 1,
                payload,
            },
            expected_version: None,
        }))
    }
}

fn codec_error(error: crate::mqi_replay::ReplayError) -> HostProblem {
    match error {
        crate::mqi_replay::ReplayError::Bounds => HostProblem::ResourceExhausted,
        crate::mqi_replay::ReplayError::Unsupported(_) => HostProblem::Unsupported,
        _ => HostProblem::Malformed,
    }
}

fn bounded_bytes<'de, D: serde::Deserializer<'de>>(decoder: D) -> Result<Vec<u8>, D::Error> {
    struct Bytes;
    impl<'de> serde::de::Visitor<'de> for Bytes {
        type Value = Vec<u8>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a bounded MQI replay byte array")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<u8>, A::Error> {
            let cap = MqMqiLimits::default().canonical_bytes;
            if seq.size_hint().is_some_and(|n| n > cap) {
                return Err(serde::de::Error::custom("receipt byte ceiling"));
            }
            let mut bytes = Vec::new();
            while let Some(byte) = seq.next_element::<u8>()? {
                if bytes.len() == cap {
                    return Err(serde::de::Error::custom("receipt byte ceiling"));
                }
                bytes.push(byte);
            }
            Ok(bytes)
        }
    }
    decoder.deserialize_seq(Bytes)
}

pub(in crate::service) fn restore(
    records: &[ProviderStateRecord],
    control: Option<&Control>,
    limits: MqLimits,
) -> Result<BTreeMap<String, OccurrenceReceipt>, HostProblem> {
    let mut receipts = BTreeMap::new();
    let mut bytes = 0usize;
    for record in records.iter().filter(|r| r.namespace == NAMESPACE) {
        bytes = bytes
            .checked_add(record.payload.len())
            .ok_or(HostProblem::ResourceExhausted)?;
        if receipts.len() >= limits.max_replays
            || bytes > limits.max_state_bytes
            || record.payload.len() > limits.max_state_bytes
            || record.version != 1
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let row: ObjectRow<OccurrenceReceipt> =
            serde_json::from_slice(&record.payload).map_err(|_| HostProblem::Malformed)?;
        if row.schema_version != OBJECT_ROW_SCHEMA || row.object_key != record.key {
            return Err(HostProblem::Malformed);
        }
        row.value
            .validate(&record.key, control.ok_or(HostProblem::Malformed)?)?;
        if receipts.insert(record.key.clone(), row.value).is_some() {
            return Err(HostProblem::Malformed);
        }
    }
    Ok(receipts)
}
