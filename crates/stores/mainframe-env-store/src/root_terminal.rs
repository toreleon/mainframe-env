//! Core-owned root codec and validators shared by Memory and SQLite.
//! No callbacks, host attestation or application-effect fabrication.
use crate::{durable, validation};
use mainframe_env_execution_api::{
    AuditDecision, InvocationLimits, LifecycleEventKind, RootTerminalAudit, RootTerminalAuditRole,
    RootTerminalDisposition,
};
use mainframe_env_store_api::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
mod preparation;
mod provider_writer;
pub(crate) use provider_writer::mutation_endpoints;

pub(crate) const ACTOR_NAMESPACE: &str = "durable-root-actor-v1";
pub(crate) const RUN_NAMESPACE: &str = "durable-root-run-v1";
pub(crate) const SCOPE_NAMESPACE: &str = "durable-root-scope-v1";
pub(crate) const ROW_SCOPE_NAMESPACE: &str = "durable-root-row-scope-v1";
const SCHEMA: &str = "mainframe-env.core-root-ownership@1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Phase {
    Open,
    Closing,
    Terminal,
    Uncertain,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Actor {
    pub(crate) execution: String,
    pub(crate) artifact: String,
    pub(crate) selector: String,
    pub(crate) attempt: u32,
    pub(crate) parent: Option<String>,
    pub(crate) call: Option<RootCallBinding>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Document {
    schema: String,
    pub(crate) root: String,
    pub(crate) run: String,
    pub(crate) principal: String,
    fingerprint: [u8; 32],
    pub(crate) deadline: u64,
    pub(crate) phase: Phase,
    pub(crate) actors: Vec<Actor>,
    pub(crate) provider_namespaces: Vec<String>,
    pub(crate) provider_rows: Vec<(String, String)>,
    pub(crate) winning_resource: Option<[u8; 32]>,
}

fn fingerprint(admission: &RootDriverAdmission) -> Result<[u8; 32], StoreError> {
    admission.validate()?;
    validation::admission(
        &admission.execution,
        &admission.event,
        &admission.notification,
    )?;
    // Storage equality over the existing full codecs, not another host canonical digest.
    let mut hash = Sha256::new();
    hash.update(b"mainframe-env.core-root-admission-storage@1\0");
    for bytes in [
        durable::encode_execution(&admission.execution)?,
        durable::encode_event(&admission.event)?,
        durable::encode_outbox(&admission.notification)?,
    ] {
        hash.update((bytes.len() as u64).to_be_bytes());
        hash.update(bytes);
    }
    hash.update((admission.invocation_key.as_str().len() as u64).to_be_bytes());
    hash.update(admission.invocation_key.as_str().as_bytes());
    hash.update(admission.configuration_digest);
    hash.update(admission.deadline_tick.to_be_bytes());
    for namespace in &admission.provider_namespaces {
        hash.update((namespace.len() as u64).to_be_bytes());
        hash.update(namespace.as_bytes());
    }
    hash.update((admission.provider_namespaces.len() as u64).to_be_bytes());
    for identity in &admission.provider_rows {
        for field in [&identity.namespace, &identity.key] {
            hash.update((field.len() as u64).to_be_bytes());
            hash.update(field.as_bytes());
        }
    }
    hash.update((admission.provider_rows.len() as u64).to_be_bytes());
    Ok(hash.finalize().into())
}

impl Document {
    pub(crate) fn new(admission: &RootDriverAdmission) -> Result<Self, StoreError> {
        Ok(Self {
            schema: SCHEMA.into(),
            root: admission.execution.execution_id.as_str().into(),
            run: admission.execution.run_unit_id.as_str().into(),
            principal: admission.execution.principal.as_str().into(),
            fingerprint: fingerprint(admission)?,
            deadline: admission.deadline_tick,
            provider_namespaces: admission.provider_namespaces.clone(),
            provider_rows: admission
                .provider_rows
                .iter()
                .map(|i| (i.namespace.clone(), i.key.clone()))
                .collect(),
            phase: Phase::Open,
            winning_resource: None,
            actors: vec![Actor {
                execution: admission.execution.execution_id.as_str().into(),
                artifact: admission.execution.artifact.as_str().into(),
                selector: admission.execution.selector.as_str().into(),
                attempt: admission.execution.attempt,
                parent: None,
                call: None,
            }],
        })
    }
    pub(crate) fn row(
        &self,
        version: u64,
        limit: usize,
    ) -> Result<ProviderStateRecord, StoreError> {
        self.validate()?;
        // JSON may escape each UTF-8 byte to six bytes. Refuse a conservative
        // complete bound before json! clones strings or builds value arrays.
        let mut bound = 4096_usize;
        let mut add = |text: &str| -> Result<(), StoreError> {
            bound = bound
                .checked_add(
                    text.len()
                        .checked_mul(6)
                        .ok_or(StoreError::CapacityExceeded)?,
                )
                .ok_or(StoreError::CapacityExceeded)?;
            Ok(())
        };
        for text in [&self.schema, &self.root, &self.run, &self.principal] {
            add(text)?;
        }
        for actor in &self.actors {
            for text in [&actor.execution, &actor.artifact, &actor.selector] {
                add(text)?;
            }
            if let Some(parent) = &actor.parent {
                add(parent)?;
            }
            if let Some(call) = &actor.call {
                add(&call.namespace)?;
                add(&call.key)?;
                add(call.effect_key.as_str())?;
                add(&call.catalog.namespace)?;
                add(&call.catalog.key)?;
            }
        }
        for namespace in &self.provider_namespaces {
            add(namespace)?;
        }
        for (namespace, key) in &self.provider_rows {
            add(namespace)?;
            add(key)?;
        }
        bound = bound
            .checked_add(
                self.actors
                    .len()
                    .checked_mul(1024)
                    .ok_or(StoreError::CapacityExceeded)?,
            )
            .and_then(|n| n.checked_add(self.provider_rows.len().checked_mul(16)?))
            .ok_or(StoreError::CapacityExceeded)?;
        if bound > (256 << 10).min(limit) {
            return Err(StoreError::CapacityExceeded);
        }
        // All strings/counts are bounded before serialization; the exact encoded
        // result is then bounded by the backend's actual row quota.
        let actors = self.actors.iter().map(|actor| json!({ "execution": actor.execution,
            "artifact": actor.artifact, "selector": actor.selector, "attempt": actor.attempt,
            "parent": actor.parent, "call": actor.call.as_ref().map(|call| json!({
                "namespace": call.namespace, "key": call.key, "effect_key": call.effect_key.as_str(),
                "request_digest": call.request_digest.as_slice(), "catalog_namespace": call.catalog.namespace,
                "catalog_key": call.catalog.key, "catalog_version": call.catalog.version })) })).collect::<Vec<_>>();
        let payload = serde_json::to_vec(&json!({ "schema": self.schema, "root": self.root,
            "run": self.run, "principal": self.principal, "fingerprint": self.fingerprint.as_slice(),
            "deadline": self.deadline, "phase": match self.phase { Phase::Open => 0, Phase::Closing => 1, Phase::Terminal => 2, Phase::Uncertain => 3 },
            "winning_resource": self.winning_resource.as_ref().map(|r| r.as_slice()),
            "actors": actors, "provider_namespaces": self.provider_namespaces,
            "provider_rows": self.provider_rows })).map_err(|_| StoreError::IncompatibleVersion)?;
        if payload.len() > (256 << 10) {
            return Err(StoreError::CapacityExceeded);
        }
        let row = ProviderStateRecord {
            namespace: ROOT_DRIVER_NAMESPACE.into(),
            key: self.root.clone(),
            version,
            payload,
        };
        row.validate_write(limit)?;
        Ok(row)
    }
    pub(crate) fn read(row: &ProviderStateRecord) -> Result<Self, StoreError> {
        row.validate_write(MAX_ROOT_PAYLOAD_BYTES)?;
        if row.namespace != ROOT_DRIVER_NAMESPACE || row.payload.len() > (256 << 10) {
            return Err(StoreError::IncompatibleVersion);
        }
        let value: Value =
            serde_json::from_slice(&row.payload).map_err(|_| StoreError::IncompatibleVersion)?;
        let strings = |v: &Value, key: &str, limit: usize| -> Result<String, StoreError> {
            let text = v
                .get(key)
                .and_then(Value::as_str)
                .ok_or(StoreError::IncompatibleVersion)?;
            if text.is_empty() || text.len() > limit {
                return Err(StoreError::IncompatibleVersion);
            }
            Ok(text.into())
        };
        let values = value
            .get("actors")
            .and_then(Value::as_array)
            .ok_or(StoreError::IncompatibleVersion)?;
        if values.is_empty() || values.len() > MAX_ROOT_ACTORS {
            return Err(StoreError::CapacityExceeded);
        }
        let mut actors = Vec::with_capacity(values.len());
        for actor in values {
            let parent = match actor.get("parent") {
                Some(Value::Null) => None,
                Some(_) => Some(strings(actor, "parent", 128)?),
                None => return Err(StoreError::IncompatibleVersion),
            };
            let artifact = strings(actor, "artifact", 128)?;
            let call = match actor.get("call") {
                Some(Value::Null) => None,
                Some(call @ Value::Object(_)) => {
                    let fields = call
                        .get("request_digest")
                        .and_then(Value::as_array)
                        .filter(|a| a.len() == 32)
                        .ok_or(StoreError::IncompatibleVersion)?;
                    let mut request_digest = [0; 32];
                    for (byte, field) in request_digest.iter_mut().zip(fields) {
                        *byte =
                            u8::try_from(field.as_u64().ok_or(StoreError::IncompatibleVersion)?)
                                .map_err(|_| StoreError::IncompatibleVersion)?;
                    }
                    Some(RootCallBinding {
                        namespace: strings(call, "namespace", MAX_PROVIDER_NAMESPACE_BYTES)?,
                        key: strings(call, "key", MAX_PROVIDER_KEY_BYTES)?,
                        effect_key: mainframe_env_execution_api::IdempotencyKey::new(
                            strings(call, "effect_key", 128)?,
                            InvocationLimits::default(),
                        )
                        .map_err(|_| StoreError::IncompatibleVersion)?,
                        request_digest,
                        catalog: ProviderStateRecord {
                            namespace: strings(
                                call,
                                "catalog_namespace",
                                MAX_PROVIDER_NAMESPACE_BYTES,
                            )?,
                            key: strings(call, "catalog_key", MAX_PROVIDER_KEY_BYTES)?,
                            version: call
                                .get("catalog_version")
                                .and_then(Value::as_u64)
                                .ok_or(StoreError::IncompatibleVersion)?,
                            payload: artifact.as_bytes().to_vec(),
                        },
                    })
                }
                _ => return Err(StoreError::IncompatibleVersion),
            };
            actors.push(Actor {
                execution: strings(actor, "execution", 128)?,
                artifact,
                selector: strings(actor, "selector", 128)?,
                attempt: u32::try_from(
                    actor
                        .get("attempt")
                        .and_then(Value::as_u64)
                        .ok_or(StoreError::IncompatibleVersion)?,
                )
                .map_err(|_| StoreError::IncompatibleVersion)?,
                parent,
                call,
            });
        }
        let fingerprint_fields = value
            .get("fingerprint")
            .and_then(Value::as_array)
            .filter(|v| v.len() == 32)
            .ok_or(StoreError::IncompatibleVersion)?;
        let mut fingerprint = [0; 32];
        for (byte, field) in fingerprint.iter_mut().zip(fingerprint_fields) {
            *byte = u8::try_from(field.as_u64().ok_or(StoreError::IncompatibleVersion)?)
                .map_err(|_| StoreError::IncompatibleVersion)?;
        }
        let winning_resource = match value.get("winning_resource") {
            Some(Value::Null) => None,
            Some(Value::Array(fields)) if fields.len() == 32 => {
                let mut bytes = [0; 32];
                for (byte, field) in bytes.iter_mut().zip(fields) {
                    *byte = u8::try_from(field.as_u64().ok_or(StoreError::IncompatibleVersion)?)
                        .map_err(|_| StoreError::IncompatibleVersion)?;
                }
                Some(bytes)
            }
            _ => return Err(StoreError::IncompatibleVersion),
        };
        let namespaces = value
            .get("provider_namespaces")
            .and_then(Value::as_array)
            .filter(|v| v.len() <= 16)
            .ok_or(StoreError::IncompatibleVersion)?;
        let mut provider_namespaces = Vec::with_capacity(namespaces.len());
        for field in namespaces {
            let namespace = field
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= MAX_PROVIDER_NAMESPACE_BYTES)
                .ok_or(StoreError::IncompatibleVersion)?;
            provider_namespaces.push(namespace.into());
        }
        let rows = value
            .get("provider_rows")
            .and_then(Value::as_array)
            .filter(|v| v.len() <= MAX_ROOT_OPERATIONS)
            .ok_or(StoreError::IncompatibleVersion)?;
        let mut provider_rows = Vec::with_capacity(rows.len());
        for field in rows {
            let pair = field
                .as_array()
                .filter(|v| v.len() == 2)
                .ok_or(StoreError::IncompatibleVersion)?;
            let namespace = pair[0]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= MAX_PROVIDER_NAMESPACE_BYTES)
                .ok_or(StoreError::IncompatibleVersion)?;
            let key = pair[1]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= MAX_PROVIDER_KEY_BYTES)
                .ok_or(StoreError::IncompatibleVersion)?;
            provider_rows.push((namespace.into(), key.into()));
        }
        let document = Self {
            schema: strings(&value, "schema", 64)?,
            root: strings(&value, "root", 128)?,
            run: strings(&value, "run", 128)?,
            principal: strings(&value, "principal", 128)?,
            fingerprint,
            deadline: value
                .get("deadline")
                .and_then(Value::as_u64)
                .ok_or(StoreError::IncompatibleVersion)?,
            phase: match value.get("phase").and_then(Value::as_u64) {
                Some(0) => Phase::Open,
                Some(1) => Phase::Closing,
                Some(2) => Phase::Terminal,
                Some(3) => Phase::Uncertain,
                _ => return Err(StoreError::IncompatibleVersion),
            },
            actors,
            provider_namespaces,
            provider_rows,
            winning_resource,
        };
        document.validate()?;
        if document.root != row.key
            || document.row(row.version, MAX_ROOT_PAYLOAD_BYTES)?.payload != row.payload
        {
            return Err(StoreError::IncompatibleVersion);
        }
        Ok(document)
    }
    pub(crate) fn validate(&self) -> Result<(), StoreError> {
        let limits = InvocationLimits::default();
        if self.schema != SCHEMA
            || self.deadline == 0
            || self.deadline > i64::MAX as u64
            || self.actors.is_empty()
            || self.actors.len() > MAX_ROOT_ACTORS
            || self.provider_namespaces.len() > 16
            || self.provider_rows.len() > MAX_ROOT_OPERATIONS
        {
            return Err(StoreError::IncompatibleVersion);
        }
        if (self.phase == Phase::Terminal) != self.winning_resource.is_some() {
            return Err(StoreError::IncompatibleVersion);
        }
        mainframe_env_execution_api::ExecutionId::new(&self.root, limits)
            .map_err(|_| StoreError::IncompatibleVersion)?;
        mainframe_env_execution_api::RunUnitId::new(&self.run, limits)
            .map_err(|_| StoreError::IncompatibleVersion)?;
        mainframe_env_execution_api::PrincipalId::new(&self.principal, limits)
            .map_err(|_| StoreError::IncompatibleVersion)?;
        for (i, namespace) in self.provider_namespaces.iter().enumerate() {
            if namespace.is_empty()
                || namespace.len() > MAX_PROVIDER_NAMESPACE_BYTES
                || namespace.starts_with("durable-")
                || self.provider_namespaces[..i].contains(namespace)
            {
                return Err(StoreError::IncompatibleVersion);
            }
        }
        for (i, (namespace, key)) in self.provider_rows.iter().enumerate() {
            if namespace.is_empty()
                || namespace.len() > MAX_PROVIDER_NAMESPACE_BYTES
                || namespace.starts_with("durable-")
                || key.is_empty()
                || key.len() > MAX_PROVIDER_KEY_BYTES
                || self.provider_rows[..i].contains(&(namespace.clone(), key.clone()))
            {
                return Err(StoreError::IncompatibleVersion);
            }
        }
        for (index, actor) in self.actors.iter().enumerate() {
            mainframe_env_execution_api::ExecutionId::new(&actor.execution, limits)
                .map_err(|_| StoreError::IncompatibleVersion)?;
            mainframe_env_execution_api::ArtifactRef::new(&actor.artifact, limits)
                .map_err(|_| StoreError::IncompatibleVersion)?;
            mainframe_env_execution_api::Selector::new(&actor.selector, limits)
                .map_err(|_| StoreError::IncompatibleVersion)?;
            if actor.attempt == 0 {
                return Err(StoreError::IncompatibleVersion);
            }
            if self.actors[..index]
                .iter()
                .any(|a| a.execution == actor.execution)
            {
                return Err(StoreError::IncompatibleVersion);
            }
            if index == 0 {
                if actor.execution != self.root || actor.parent.is_some() || actor.call.is_some() {
                    return Err(StoreError::IncompatibleVersion);
                }
            } else {
                if !actor
                    .parent
                    .as_ref()
                    .is_some_and(|p| self.actors[..index].iter().any(|a| &a.execution == p))
                {
                    return Err(StoreError::IncompatibleVersion);
                }
                let call = actor.call.as_ref().ok_or(StoreError::IncompatibleVersion)?;
                if call.namespace.is_empty()
                    || call.namespace.len() > MAX_PROVIDER_NAMESPACE_BYTES
                    || call.key.is_empty()
                    || call.key.len() > MAX_PROVIDER_KEY_BYTES
                    || call.namespace.starts_with("durable-")
                    || call.catalog.namespace != "batch-program"
                    || call.catalog.key.is_empty()
                    || call.catalog.key.len() > 128
                    || call.catalog.version == 0
                    || call.catalog.version > i64::MAX as u64
                    || call.catalog.payload != actor.artifact.as_bytes()
                    || actor.selector != format!("program:{}", call.catalog.key)
                {
                    return Err(StoreError::IncompatibleVersion);
                }
            }
        }
        Ok(())
    }
    pub(crate) fn require_claim(&self, claim: &RootDriverClaim) -> Result<(), StoreError> {
        let original = Self::read(claim.inserted_row())?;
        if original.phase != Phase::Open
            || original.actors.len() != 1
            || original.fingerprint != fingerprint(claim.admission())?
            || self.root != original.root
            || self.run != original.run
            || self.principal != original.principal
            || self.deadline != original.deadline
            || self.fingerprint != original.fingerprint
        {
            return Err(StoreError::Conflict);
        }
        if self.provider_namespaces != original.provider_namespaces
            || !self.provider_rows.starts_with(&original.provider_rows)
        {
            return Err(StoreError::Conflict);
        }
        Ok(())
    }
    pub(crate) fn require_live(&self, tick: u64, floor: u64) -> Result<(), StoreError> {
        if tick == 0 || tick > i64::MAX as u64 || tick < floor || tick >= self.deadline {
            Err(StoreError::LeaseConflict)
        } else {
            Ok(())
        }
    }
    pub(crate) fn enroll(
        &mut self,
        admission: &RootChildAdmission,
        intent: &EffectRecord,
    ) -> Result<(), StoreError> {
        self.require_claim(&admission.claim)?;
        self.require_occurrence(&admission.parent_occurrence, intent)?;
        validation::admission(
            &admission.execution,
            &admission.event,
            &admission.notification,
        )?;
        admission.call.validate_write(MAX_ROOT_PAYLOAD_BYTES)?;
        admission.catalog.validate_write(128)?;
        if self.phase != Phase::Open
            || self.actors.len() >= MAX_ROOT_ACTORS
            || !self
                .actors
                .iter()
                .any(|a| a.execution == admission.parent.as_str())
            || self
                .actors
                .iter()
                .any(|a| a.execution == admission.execution.execution_id.as_str())
            || admission.execution.run_unit_id.as_str() != self.run
            || admission.execution.principal.as_str() != self.principal
            || admission.execution.owner_lease.is_some()
            || admission.execution.lease_expiry_tick.is_some()
            || admission.parent_occurrence.claim != admission.claim
            || admission.parent_occurrence.execution.execution_id != admission.parent
            || admission.parent_occurrence.identity.namespace != admission.call.namespace
            || admission.parent_occurrence.identity.key != admission.call.key
            || admission.parent_occurrence.observed_tick != admission.event.tick
            || admission.call.namespace.starts_with("durable-")
            || admission.catalog.namespace != "batch-program"
            || admission.catalog.payload != admission.execution.artifact.as_str().as_bytes()
            || admission.execution.selector.as_str() != format!("program:{}", admission.catalog.key)
            || !self
                .provider_rows
                .iter()
                .any(|(n, k)| n == &admission.catalog.namespace && k == &admission.catalog.key)
            || !self
                .provider_rows
                .iter()
                .any(|(n, k)| n == &admission.call.namespace && k == &admission.call.key)
        {
            return Err(StoreError::Conflict);
        }
        self.actors.push(Actor {
            execution: admission.execution.execution_id.as_str().into(),
            artifact: admission.execution.artifact.as_str().into(),
            selector: admission.execution.selector.as_str().into(),
            attempt: admission.execution.attempt,
            parent: Some(admission.parent.as_str().into()),
            call: Some(RootCallBinding {
                namespace: admission.call.namespace.clone(),
                key: admission.call.key.clone(),
                effect_key: admission.parent_occurrence.effect_key.clone(),
                request_digest: admission.parent_occurrence.request_digest,
                catalog: admission.catalog.clone(),
            }),
        });
        self.validate()
    }
    pub(crate) fn register_row(
        &mut self,
        admission: &RootProviderRowAdmission,
        intent: &EffectRecord,
    ) -> Result<(), StoreError> {
        self.require_occurrence(admission, intent)?;
        let identity = (
            admission.identity.namespace.clone(),
            admission.identity.key.clone(),
        );
        if !self.provider_rows.contains(&identity) {
            if self.provider_rows.len() >= MAX_ROOT_OPERATIONS {
                return Err(StoreError::CapacityExceeded);
            }
            self.provider_rows.push(identity);
        }
        self.validate()
    }
    fn require_occurrence(
        &self,
        admission: &RootProviderRowAdmission,
        intent: &EffectRecord,
    ) -> Result<(), StoreError> {
        self.require_claim(&admission.claim)?;
        validation::new_intent(intent)?;
        validation::effect_execution(&admission.execution, intent)?;
        if self.phase != Phase::Open
            || admission.execution.state != ExecutionState::Running
            || !self
                .actors
                .iter()
                .any(|a| a.execution == admission.execution.execution_id.as_str())
            || intent.key != admission.effect_key
            || intent.sequence != admission.effect_sequence
            || intent.request_digest != admission.request_digest
            || intent.digest_format != EffectDigestFormat::CanonicalHostV1
            || intent.intent.created_tick == 0
            || admission.observed_tick < intent.intent.created_tick
            || admission.observed_tick >= intent.intent.recovery_after_tick
            || intent.intent.recovery_lease.is_some()
            || intent.intent.capability.is_none()
            || intent.intent.audit_resource.is_none()
            || intent.intent.audit_invocation_key.is_none()
            || admission.identity.namespace.is_empty()
            || admission.identity.namespace.len() > MAX_PROVIDER_NAMESPACE_BYTES
            || admission.identity.namespace.starts_with("durable-")
            || admission.identity.key.is_empty()
            || admission.identity.key.len() > MAX_PROVIDER_KEY_BYTES
        {
            return Err(StoreError::Conflict);
        }
        Ok(())
    }
}

pub(crate) fn row_scope_key(namespace: &str, key: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"mainframe-env.core-root-row-scope@1\0");
    for field in [namespace, key] {
        hash.update((field.len() as u64).to_be_bytes());
        hash.update(field.as_bytes());
    }
    format!("{:x}", hash.finalize())
}

pub(crate) fn validate_known_closure(
    snapshot: &RootClosureSnapshot,
) -> Result<Document, StoreError> {
    snapshot.validate_bounds()?;
    let doc = Document::read(&snapshot.closing)?;
    doc.require_claim(&snapshot.claim)?;
    if doc.phase != Phase::Closing || doc.actors.len() != snapshot.actors.len() {
        return Err(StoreError::Conflict);
    }
    for (index, (identity, actor)) in doc.actors.iter().zip(&snapshot.actors).enumerate() {
        if actor.execution.execution_id.as_str() != identity.execution
            || actor.execution.artifact.as_str() != identity.artifact
            || actor.execution.selector.as_str() != identity.selector
            || actor.execution.attempt != identity.attempt
            || actor.parent.as_ref().map(|p| p.as_str()) != identity.parent.as_deref()
            || actor.call != identity.call
            || actor.execution.run_unit_id.as_str() != doc.run
            || actor.execution.principal.as_str() != doc.principal
            || actor.execution.owner_lease.is_some()
            || actor.execution.lease_expiry_tick.is_some()
            || actor.last_event.sequence != actor.execution.version
            || actor.last_event.execution_id != actor.execution.execution_id
            || actor.last_event.run_unit_id != actor.execution.run_unit_id
            || actor.last_event.attempt != actor.execution.attempt
            || actor.last_event.tick == 0
            || actor.last_event.tick > snapshot.observed_tick
            || actor.checkpoint.is_some()
            || !actor.work.is_empty()
            || actor.effects.iter().any(|e| {
                !matches!(e.state, EffectState::Completed | EffectState::Failed)
                    || e.resolved_tick.is_none()
                    || e.result_digest.is_none()
                    || e.intent.capability.is_none()
                    || e.intent.audit_resource.is_none()
                    || e.intent.audit_invocation_key.is_none()
                    || e.digest_format != EffectDigestFormat::CanonicalHostV1
            })
        {
            return Err(StoreError::Conflict);
        }
        for effect in &actor.effects {
            validation::effect(effect)?;
            validation::effect_execution(&actor.execution, effect)?;
            if effect.sequence > actor.effects.len() as u64
                || actor
                    .effects
                    .iter()
                    .filter(|other| other.sequence == effect.sequence)
                    .count()
                    != 1
                || effect.intent.created_tick > snapshot.observed_tick
                || effect
                    .resolved_tick
                    .is_none_or(|tick| tick == 0 || tick > snapshot.observed_tick)
                || effect.intent.recovery_lease.is_some()
            {
                return Err(StoreError::Conflict);
            }
        }
        if index == 0 {
            if actor.execution.state != ExecutionState::Running
                || actor.execution.terminal_tick.is_some()
            {
                return Err(StoreError::InvalidTransition);
            }
        } else if !(actor.execution.state == ExecutionState::Completed
            && matches!(actor.last_event.kind, LifecycleEventKind::Completed { .. }))
            && !(actor.execution.state == ExecutionState::Failed
                && matches!(actor.last_event.kind, LifecycleEventKind::Abend))
        {
            return Err(StoreError::InvalidTransition);
        }
    }
    Ok(doc)
}

pub(crate) fn validate_publication(
    request: &RootTerminalPublication,
    limit: usize,
) -> Result<Document, StoreError> {
    request.validate_bounds()?;
    let doc = validate_known_closure(&request.closure)?;
    let root = &request.closure.actors[0].execution;
    if request.observed_tick < request.closure.observed_tick
        || request.observed_tick == 0
        || request.observed_tick >= doc.deadline
        || request.observed_tick > i64::MAX as u64
    {
        return Err(StoreError::LeaseConflict);
    }
    let count = request
        .steps
        .len()
        .checked_add(request.audits.len())
        .and_then(|n| n.checked_add(request.dependencies.len()))
        .and_then(|n| n.checked_add(request.mutations.len()))
        .and_then(|n| n.checked_add(request.closure.provider_dependencies.len()))
        .and_then(|n| {
            n.checked_add(
                request
                    .closure
                    .actors
                    .iter()
                    .map(|a| 1 + a.effects.len() + a.work.len())
                    .sum::<usize>(),
            )
        })
        .ok_or(StoreError::CapacityExceeded)?;
    if count > MAX_ROOT_OPERATIONS || request.mutations.len() > 1024 || request.audits.len() != 2 {
        return Err(StoreError::CapacityExceeded);
    }
    match (request.disposition, request.steps.as_slice()) {
        (RootTerminalDisposition::Normal { return_code }, [a, b])
            if a.next_state == ExecutionState::Completing
                && matches!(a.event.kind, LifecycleEventKind::Completing)
                && b.next_state == ExecutionState::Completed
                && b.event.kind == (LifecycleEventKind::Completed { return_code }) => {}
        (RootTerminalDisposition::KnownAbnormal, [a])
            if a.next_state == ExecutionState::Failed
                && matches!(a.event.kind, LifecycleEventKind::Abend) => {}
        _ => return Err(StoreError::InvalidTransition),
    }
    let first_sequence = root
        .version
        .checked_add(1)
        .ok_or(StoreError::InvalidSequence)?;
    for step in &request.steps {
        if step.notification.topic != "execution.lifecycle.v1"
            || step.notification.notification_id
                != format!("{}:{:020}", root.execution_id, step.event.sequence)
            || step.notification.payload
                != mainframe_env_execution_api::lifecycle_notification_payload(&step.event.kind)
        {
            return Err(StoreError::Conflict);
        }
    }
    if request.audits[0].role != RootTerminalAuditRole::ProviderSettlement
        || request.audits[1].role != RootTerminalAuditRole::CoreClosure
        || request.audits[0].resource != request.audits[1].resource
    {
        return Err(StoreError::Conflict);
    }
    for audit in &request.audits {
        validate_audit(audit)?;
        if audit.execution_id != root.execution_id
            || audit.run_unit_id != root.run_unit_id
            || audit.principal != root.principal
            || audit.attempt != root.attempt
            || audit.invocation_key != request.closure.claim.admission().invocation_key
            || audit.lifecycle_sequence != first_sequence
            || audit.observed_tick != request.observed_tick
            || audit.decision != AuditDecision::Success
        {
            return Err(StoreError::Conflict);
        }
    }
    // Reuse the shared provider shape/namespace authority; its intent is never
    // fabricated here. This method performs only row legality and byte checks.
    let mut bytes = 0_usize;
    for mutation in &request.mutations {
        let (namespace, key, payload) = match mutation {
            ProviderStateMutation::Put(w) => {
                w.record.validate_write(limit)?;
                if w.expected_version.map_or(w.record.version != 1, |v| {
                    v == 0 || v.checked_add(1) != Some(w.record.version)
                }) {
                    return Err(StoreError::Conflict);
                }
                (&w.record.namespace, &w.record.key, w.record.payload.len())
            }
            ProviderStateMutation::Delete {
                namespace,
                key,
                expected_version,
            } => {
                if *expected_version == 0 || *expected_version > i64::MAX as u64 {
                    return Err(StoreError::Conflict);
                }
                (namespace, key, 0)
            }
            ProviderStateMutation::Move {
                record,
                old_key,
                expected_version,
            } => {
                record.validate_move(old_key, *expected_version, limit)?;
                (&record.namespace, &record.key, record.payload.len())
            }
        };
        if namespace.starts_with("durable-")
            || namespace.is_empty()
            || namespace.len() > MAX_PROVIDER_NAMESPACE_BYTES
            || key.is_empty()
            || key.len() > MAX_PROVIDER_KEY_BYTES
            || (namespace == "jes-worker-meta" && key == "logical-clock")
        {
            return Err(StoreError::InvalidTransition);
        }
        if !doc.provider_namespaces.contains(namespace)
            && !doc
                .provider_rows
                .iter()
                .any(|(n, k)| n == namespace && k == key)
        {
            return Err(StoreError::InvalidTransition);
        }
        let dependencies = request
            .dependencies
            .iter()
            .chain(&request.closure.provider_dependencies);
        let covers = |namespace: &str, key: &str| {
            dependencies.clone().any(|dependency| match dependency {
                TerminalRowDependency::Exact(row) => row.namespace == namespace && row.key == key,
                TerminalRowDependency::Absent {
                    namespace: n,
                    key: k,
                } => n == namespace && k == key,
            })
        };
        if !covers(namespace, key) {
            return Err(StoreError::Conflict);
        }
        if let ProviderStateMutation::Move { old_key, .. } = mutation {
            if !covers(namespace, old_key) {
                return Err(StoreError::Conflict);
            }
        }
        bytes = bytes
            .checked_add(payload)
            .ok_or(StoreError::CapacityExceeded)?;
    }
    if bytes > MAX_ROOT_PAYLOAD_BYTES {
        return Err(StoreError::CapacityExceeded);
    }
    Ok(doc)
}

pub(crate) fn validate_audit(audit: &RootTerminalAudit) -> Result<(), StoreError> {
    if audit.attempt == 0
        || audit.lifecycle_sequence == 0
        || audit.lifecycle_sequence > i64::MAX as u64
        || audit.observed_tick == 0
        || audit.observed_tick > i64::MAX as u64
    {
        return Err(StoreError::InvalidSequence);
    }
    Ok(())
}
