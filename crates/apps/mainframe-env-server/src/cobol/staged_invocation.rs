//! Bounded owned invocation DTO; live cancellation probes are never serialized.
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use mainframe_env_execution_api::{
    Cancellation, CancellationId, CancellationProbe, CapabilityId, PrincipalId, ResourceLimits,
    ServiceClass,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    schema: String,
    bytes: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cobol::hardening::parent;
    use serde_json::json;

    fn context() -> Invocation {
        let mut invocation = parent();
        invocation.parent_execution_id = Some(invocation.execution_id.clone());
        invocation.priority = 87;
        invocation.audit_correlation = "inherited-audit".into();
        invocation.bindings.insert(
            "custom.context".into(),
            BoundedPayload::new("custom@1", vec![0, 255, 1], InvocationLimits::default()).unwrap(),
        );
        invocation.provider_generations.insert(
            CapabilityId::new("host.program.invoke", InvocationLimits::default()).unwrap(),
            "generation-17".into(),
        );
        invocation.cancellation = Some(Cancellation {
            id: CancellationId::new("cancel-7", InvocationLimits::default()).unwrap(),
            reason: "requested".into(),
            requested_at_tick: 23,
        });
        invocation.cancellation_probe = Some(CancellationProbe::default());
        invocation
    }

    #[test]
    fn staged_invocation_roundtrips_all_context_and_reinstalls_only_explicit_live_probe() {
        let original = context();
        let saved = StagedInvocation::capture(&original).unwrap();
        let encoded = serde_json::to_vec(&saved).unwrap();
        let reopened: StagedInvocation = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(
            reopened
                .restore(original.cancellation_probe.clone())
                .unwrap(),
            original
        );
        assert!(reopened.restore(None).is_err());
        assert!(reopened.validate_syntax().is_ok());
        original.cancellation_probe.as_ref().unwrap().request();
        assert!(
            reopened
                .restore(original.cancellation_probe.clone())
                .unwrap()
                .cancellation_probe
                .unwrap()
                .is_requested()
        );
        assert_eq!(
            encoded,
            serde_json::to_vec(&reopened).unwrap(),
            "live state is not serialized"
        );
    }

    #[test]
    fn staged_invocation_rejects_malformed_bounded_context_and_ambiguous_json() {
        let value = serde_json::to_value(StagedInvocation::capture(&context()).unwrap()).unwrap();
        for (field, bad) in [
            ("class", json!(0)),
            ("attempt", json!(0)),
            ("parent", json!("")),
            ("deadline", json!(0)),
            ("audit", json!("")),
            (
                "grants",
                json!(["host.program.invoke", "host.program.invoke"]),
            ),
            ("limits", json!([1, 1, 1, u64::MAX, 1, 1])),
            (
                "cancellation",
                json!({"id":"cancel-7", "reason":"x", "tick":0}),
            ),
        ] {
            let mut invalid = value.clone();
            invalid[field] = bad;
            let saved: StagedInvocation = serde_json::from_value(invalid).unwrap();
            assert!(saved.validate_syntax().is_err(), "{field}");
        }
        let mut invalid = value.clone();
        invalid["bindings"]["custom.context"]["bytes"] = json!("AP8B\n");
        assert!(
            serde_json::from_value::<StagedInvocation>(invalid)
                .unwrap()
                .validate_syntax()
                .is_err()
        );
        let mut invalid = value.clone();
        invalid["bindings"]["custom.context"]["extra"] = json!(true);
        assert!(serde_json::from_value::<StagedInvocation>(invalid).is_err());
        let mut encoded = serde_json::to_vec(&value).unwrap();
        encoded.pop();
        encoded.extend_from_slice(b",\"attempt\":1}");
        assert!(serde_json::from_slice::<StagedInvocation>(&encoded).is_err());
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SavedCancellation {
    id: String,
    reason: String,
    tick: u64,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StagedInvocation {
    request: String,
    execution: String,
    run: String,
    parent: String,
    selector: String,
    artifact: String,
    principal: String,
    grants: Vec<String>,
    class: u8,
    priority: u8,
    deadline: u64,
    trace: String,
    key: String,
    attempt: u32,
    limits: [u64; 6],
    bindings: BTreeMap<String, Payload>,
    generations: BTreeMap<String, String>,
    audit: String,
    cancellation: Option<SavedCancellation>,
    probe: bool,
}

impl StagedInvocation {
    pub(super) fn capture(invocation: &Invocation) -> Result<Self, HostProblem> {
        let limits = invocation.limits;
        let saved = Self {
            request: invocation.request_id.as_str().into(),
            execution: invocation.execution_id.as_str().into(),
            run: invocation.run_unit_id.as_str().into(),
            parent: invocation
                .parent_execution_id
                .as_ref()
                .ok_or(HostProblem::UnknownOutcome)?
                .as_str()
                .into(),
            selector: invocation.selector.as_str().into(),
            artifact: invocation.artifact.as_str().into(),
            principal: invocation.principal.id().as_str().into(),
            grants: invocation
                .principal
                .grants()
                .iter()
                .map(|id| id.as_str().into())
                .collect(),
            class: match invocation.service_class {
                ServiceClass::Interactive => 1,
                ServiceClass::Batch => 2,
                ServiceClass::Compiler => 3,
                ServiceClass::Blocking => 4,
                ServiceClass::System => 5,
            },
            priority: invocation.priority,
            deadline: invocation.deadline_tick,
            trace: invocation.trace_id.as_str().into(),
            key: invocation.idempotency_key.as_str().into(),
            attempt: invocation.attempt,
            limits: [
                limits.max_steps,
                limits.max_storage_bytes,
                limits.max_output_bytes,
                u64::from(limits.max_frames),
                limits.max_effects,
                limits.max_events,
            ],
            bindings: invocation
                .bindings
                .iter()
                .map(|(key, payload)| {
                    (
                        key.clone(),
                        Payload {
                            schema: payload.schema().into(),
                            bytes: STANDARD.encode(payload.bytes()),
                        },
                    )
                })
                .collect(),
            generations: invocation
                .provider_generations
                .iter()
                .map(|(key, value)| (key.as_str().into(), value.clone()))
                .collect(),
            audit: invocation.audit_correlation.clone(),
            cancellation: invocation
                .cancellation
                .as_ref()
                .map(|cancel| SavedCancellation {
                    id: cancel.id.as_str().into(),
                    reason: cancel.reason.clone(),
                    tick: cancel.requested_at_tick,
                }),
            probe: invocation.cancellation_probe.is_some(),
        };
        saved.restore(invocation.cancellation_probe.clone())?;
        Ok(saved)
    }

    /// The caller reinstalls the trusted run-scoped live probe, not a guessed one.
    pub(super) fn restore(
        &self,
        probe: Option<CancellationProbe>,
    ) -> Result<Invocation, HostProblem> {
        self.restore_inner(probe, true)
    }

    pub(super) fn validate_syntax(&self) -> Result<(), HostProblem> {
        self.restore_inner(None, false).map(|_| ())
    }

    fn restore_inner(
        &self,
        probe: Option<CancellationProbe>,
        require_probe: bool,
    ) -> Result<Invocation, HostProblem> {
        fn bad<T>(_: T) -> HostProblem {
            HostProblem::UnknownOutcome
        }
        let limits = InvocationLimits::default();
        if require_probe && probe.is_some() != self.probe
            || self.bindings.len() > limits.max_bindings
            || self.grants.len() > limits.max_capabilities
            || self.generations.len() > limits.max_capabilities
            || !self.grants.windows(2).all(|pair| pair[0] < pair[1])
            || self.audit.is_empty()
            || self.audit.len() > limits.max_identity_bytes
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let bindings = self
            .bindings
            .iter()
            .map(|(key, payload)| {
                if payload.bytes.len() > 4 * limits.max_payload_bytes / 3 + 4 {
                    return Err(HostProblem::UnknownOutcome);
                }
                let bytes = STANDARD.decode(&payload.bytes).map_err(bad)?;
                if STANDARD.encode(&bytes) != payload.bytes {
                    return Err(HostProblem::UnknownOutcome);
                }
                Ok((
                    key.clone(),
                    BoundedPayload::new(&payload.schema, bytes, limits).map_err(bad)?,
                ))
            })
            .collect::<Result<_, HostProblem>>()?;
        let mut invocation = Invocation::new(
            RequestId::new(&self.request, limits).map_err(bad)?,
            ExecutionId::new(&self.execution, limits).map_err(bad)?,
            RunUnitId::new(&self.run, limits).map_err(bad)?,
            Some(ExecutionId::new(&self.parent, limits).map_err(bad)?),
            Selector::new(&self.selector, limits).map_err(bad)?,
            ArtifactRef::new(&self.artifact, limits).map_err(bad)?,
            Principal::new(
                PrincipalId::new(&self.principal, limits).map_err(bad)?,
                self.grants
                    .iter()
                    .map(|grant| CapabilityId::new(grant, limits).map_err(bad))
                    .collect::<Result<_, HostProblem>>()?,
                limits,
            )
            .map_err(bad)?,
            match self.class {
                1 => ServiceClass::Interactive,
                2 => ServiceClass::Batch,
                3 => ServiceClass::Compiler,
                4 => ServiceClass::Blocking,
                5 => ServiceClass::System,
                _ => return Err(HostProblem::UnknownOutcome),
            },
            self.priority,
            self.deadline,
            TraceId::new(&self.trace, limits).map_err(bad)?,
            IdempotencyKey::new(&self.key, limits).map_err(bad)?,
            self.attempt,
            ResourceLimits {
                max_steps: self.limits[0],
                max_storage_bytes: self.limits[1],
                max_output_bytes: self.limits[2],
                max_frames: u32::try_from(self.limits[3]).map_err(bad)?,
                max_effects: self.limits[4],
                max_events: self.limits[5],
            },
            bindings,
            limits,
        )
        .map_err(bad)?
        .with_provider_generations(
            self.generations
                .iter()
                .map(|(key, value)| {
                    Ok((CapabilityId::new(key, limits).map_err(bad)?, value.clone()))
                })
                .collect::<Result<_, HostProblem>>()?,
            limits,
        )
        .map_err(bad)?;
        invocation.audit_correlation = self.audit.clone();
        invocation.cancellation = self
            .cancellation
            .as_ref()
            .map(|cancel| {
                if cancel.reason.len() > limits.max_identity_bytes || cancel.tick == 0 {
                    return Err(HostProblem::UnknownOutcome);
                }
                Ok(Cancellation {
                    id: CancellationId::new(&cancel.id, limits).map_err(bad)?,
                    reason: cancel.reason.clone(),
                    requested_at_tick: cancel.tick,
                })
            })
            .transpose()?;
        invocation.cancellation_probe = probe;
        Ok(invocation)
    }

    pub(super) fn digest(&self) -> Result<String, HostProblem> {
        Ok(replay::digest(&[
            b"staged-invocation@1",
            &serde_json::to_vec(self).map_err(|_| HostProblem::UnknownOutcome)?,
        ]))
    }

    pub(super) fn identity(&self) -> (&str, &str, &str, &str, &str, &str) {
        (
            &self.execution,
            &self.parent,
            &self.run,
            &self.principal,
            &self.selector,
            &self.artifact,
        )
    }
}
