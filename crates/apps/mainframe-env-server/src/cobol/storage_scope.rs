//! Canonical language storage identity. Decoding never authorizes execution.
//! Immutable LINK creation is separate from each native CALL entry actor.
#![allow(
    dead_code,
    reason = "manager-owned reader-before-writer storage prerequisite"
)]
use super::*;
use mainframe_env_host_api::ProgramLinkSelection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[cfg(test)]
mod tests;

pub(super) const BINDING: &str = "cobol.storage-entry";
const SCHEMA: &str = "mainframe-env.cobol.storage-entry@1";
const MAX_BINDING_BYTES: usize = 8192;
const MAX_LOGICAL_LEVEL: u32 = 16;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum EntryKind {
    Root,
    NativeCall,
    CicsLink,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    artifact: String,
    generation: u64,
    content_identity: String,
}

impl Selection {
    fn capture(value: &ProgramLinkSelection) -> Result<Self, HostProblem> {
        let value = Self {
            artifact: value.artifact.as_str().into(),
            generation: value.generation,
            content_identity: value.content_identity.clone(),
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), HostProblem> {
        if !valid_content(&self.artifact)
            || self.generation == 0
            || self.generation > i64::MAX as u64
            || !valid_content(&self.content_identity)
        {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(())
    }
}

/// The creator never changes when a native CALL inherits this scope.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Scope {
    root_execution: String,
    task_run: String,
    principal: String,
    id: String,
    owner_execution: String,
    owner_selector: String,
    owner_artifact: String,
    owner_attempt: u32,
    parent_scope: Option<String>,
    source_execution: Option<String>,
    creation_call: Option<String>,
    logical_level: u32,
    selection: Option<Selection>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Entry {
    schema_version: u32,
    scope: Scope,
    kind: EntryKind,
    execution: String,
    source_execution: Option<String>,
    call_key: Option<String>,
    program: String,
    artifact: String,
    attempt: u32,
    metadata_digest: String,
}

fn valid_content(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(retention::valid_digest)
}

fn valid_program(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 246
        && value.bytes().all(|b| {
            b.is_ascii_uppercase()
                || b.is_ascii_digit()
                || matches!(b, b'@' | b'#' | b'$' | b'-' | b'_')
        })
}

fn framed(domain: &[u8], fields: &[&[u8]]) -> String {
    let mut hash = Sha256::new();
    hash.update(domain);
    hash.update([0]);
    for field in fields {
        hash.update((field.len() as u64).to_be_bytes());
        hash.update(field);
    }
    format!("{:x}", hash.finalize())
}

fn encoded<T: Serialize>(value: &T) -> Result<Vec<u8>, HostProblem> {
    serde_json::to_vec(value).map_err(|_| HostProblem::UnknownOutcome)
}

fn child_execution(key: &str) -> String {
    format!("online-call-execution-{key}")
}

impl Scope {
    fn expected_id(&self) -> Result<String, HostProblem> {
        let mut identity = self.clone();
        identity.id.clear();
        Ok(framed(
            b"mainframe-env.cobol-storage-scope@1",
            &[&encoded(&identity)?],
        ))
    }

    fn validate(&self) -> Result<(), HostProblem> {
        let valid = retention::valid_identity;
        let digest = retention::valid_digest;
        if !valid(&self.root_execution)
            || !valid(&self.task_run)
            || !valid(&self.principal)
            || !valid(&self.owner_execution)
            || !self
                .owner_selector
                .strip_prefix("program:")
                .is_some_and(valid_program)
            || !valid_content(&self.owner_artifact)
            || self.owner_attempt == 0
            || !digest(&self.id)
            || self.logical_level == 0
            || self.logical_level > MAX_LOGICAL_LEVEL
            || self.id != self.expected_id()?
        {
            return Err(HostProblem::UnknownOutcome);
        }
        match (
            &self.parent_scope,
            &self.source_execution,
            &self.creation_call,
            &self.selection,
        ) {
            (None, None, None, None)
                if self.owner_execution == self.root_execution && self.logical_level == 1 => {}
            (Some(parent), Some(source), Some(call), Some(selection))
                if digest(parent)
                    && parent != &self.id
                    && valid(source)
                    && source != &self.owner_execution
                    && digest(call)
                    && self.owner_execution == child_execution(call)
                    && self.logical_level > 1
                    && selection.artifact == self.owner_artifact =>
            {
                selection.validate()?
            }
            _ => return Err(HostProblem::UnknownOutcome),
        }
        Ok(())
    }
}

impl Entry {
    fn from_actor(
        scope: Scope,
        kind: EntryKind,
        actor: &Invocation,
        call: Option<&str>,
    ) -> Result<Self, HostProblem> {
        let program = actor
            .selector
            .as_str()
            .strip_prefix("program:")
            .ok_or(HostProblem::UnknownOutcome)?
            .to_owned();
        let source_execution = (kind != EntryKind::Root)
            .then(|| {
                actor
                    .parent_execution_id
                    .as_ref()
                    .map(|id| id.as_str().to_owned())
            })
            .flatten();
        let mut value = Self {
            schema_version: 1,
            scope,
            kind,
            execution: actor.execution_id.as_str().into(),
            source_execution,
            call_key: call.map(str::to_owned),
            program,
            artifact: actor.artifact.as_str().into(),
            attempt: actor.attempt,
            metadata_digest: String::new(),
        };
        value.metadata_digest = value.expected_metadata()?;
        value.validate_for(actor)?;
        Ok(value)
    }

    /// A root constructor describes identities only; its caller must prove root authority.
    pub(super) fn root(actor: &Invocation) -> Result<Self, HostProblem> {
        let owner = replay::protocol_owner_execution(actor)?;
        if owner != actor.execution_id.as_str() {
            return Err(HostProblem::UnknownOutcome);
        }
        let mut scope = Scope {
            root_execution: owner.clone(),
            task_run: actor.run_unit_id.as_str().into(),
            principal: actor.principal.id().as_str().into(),
            id: String::new(),
            owner_execution: owner,
            owner_selector: actor.selector.as_str().into(),
            owner_artifact: actor.artifact.as_str().into(),
            owner_attempt: actor.attempt,
            parent_scope: None,
            source_execution: None,
            creation_call: None,
            logical_level: 1,
            selection: None,
        };
        scope.id = scope.expected_id()?;
        Self::from_actor(scope, EntryKind::Root, actor, None)
    }

    /// Syntax cannot replace the source's live lease or command origin.
    pub(super) fn native_call(
        &self,
        source: &Invocation,
        target: &Invocation,
        call: &str,
    ) -> Result<Self, HostProblem> {
        self.validate_child(source, target, call)?;
        Self::from_actor(
            self.scope.clone(),
            EntryKind::NativeCall,
            target,
            Some(call),
        )
    }

    /// The future writer must also require the provider's live LINK attestation.
    pub(super) fn cics_link(
        &self,
        source: &Invocation,
        target: &Invocation,
        call: &str,
        selection: &ProgramLinkSelection,
        logical_level: u32,
    ) -> Result<Self, HostProblem> {
        self.validate_child(source, target, call)?;
        let selection = Selection::capture(selection)?;
        if selection.artifact != target.artifact.as_str()
            || logical_level
                != self
                    .scope
                    .logical_level
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let mut scope = Scope {
            root_execution: self.scope.root_execution.clone(),
            task_run: self.scope.task_run.clone(),
            principal: self.scope.principal.clone(),
            id: String::new(),
            owner_execution: target.execution_id.as_str().into(),
            owner_selector: target.selector.as_str().into(),
            owner_artifact: target.artifact.as_str().into(),
            owner_attempt: target.attempt,
            parent_scope: Some(self.scope.id.clone()),
            source_execution: Some(source.execution_id.as_str().into()),
            creation_call: Some(call.into()),
            logical_level,
            selection: Some(selection),
        };
        scope.id = scope.expected_id()?;
        Self::from_actor(scope, EntryKind::CicsLink, target, Some(call))
    }

    fn validate_child(
        &self,
        source: &Invocation,
        target: &Invocation,
        call: &str,
    ) -> Result<(), HostProblem> {
        self.validate_for(source)?;
        if !retention::valid_digest(call)
            || target.execution_id.as_str() != child_execution(call)
            || target.parent_execution_id.as_ref() != Some(&source.execution_id)
            || target.run_unit_id != source.run_unit_id
            || target.principal != source.principal
            || target.attempt != source.attempt
            || replay::protocol_owner_execution(target)? != self.scope.root_execution
        {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(())
    }

    fn expected_metadata(&self) -> Result<String, HostProblem> {
        let mut value = self.clone();
        value.metadata_digest.clear();
        Ok(framed(
            b"mainframe-env.cobol-storage-entry@1",
            &[&encoded(&value)?],
        ))
    }

    pub(super) fn validate_for(&self, actor: &Invocation) -> Result<(), HostProblem> {
        self.scope.validate()?;
        if self.schema_version != 1
            || !valid_program(&self.program)
            || !valid_content(&self.artifact)
            || self.attempt == 0
            || self.metadata_digest != self.expected_metadata()?
            || self.scope.root_execution != replay::protocol_owner_execution(actor)?
            || self.scope.task_run != actor.run_unit_id.as_str()
            || self.scope.principal != actor.principal.id().as_str()
            || self.execution != actor.execution_id.as_str()
            || self.artifact != actor.artifact.as_str()
            || actor.selector.as_str() != format!("program:{}", self.program)
            || self.attempt != actor.attempt
            || self.scope.owner_attempt != actor.attempt
        {
            return Err(HostProblem::UnknownOutcome);
        }
        match self.kind {
            EntryKind::Root
                if self.scope.parent_scope.is_none()
                    && self.execution == self.scope.root_execution
                    && self.source_execution.is_none()
                    && self.call_key.is_none()
                    && self.scope.owner_selector == actor.selector.as_str()
                    && self.scope.owner_artifact == self.artifact => {}
            EntryKind::NativeCall | EntryKind::CicsLink => {
                let call = self
                    .call_key
                    .as_deref()
                    .ok_or(HostProblem::UnknownOutcome)?;
                let source = self
                    .source_execution
                    .as_deref()
                    .ok_or(HostProblem::UnknownOutcome)?;
                if !retention::valid_digest(call)
                    || !retention::valid_identity(source)
                    || self.execution != child_execution(call)
                    || source == self.execution
                    || actor.parent_execution_id.as_ref().map(ExecutionId::as_str) != Some(source)
                    || self.kind == EntryKind::NativeCall
                        && self.scope.owner_execution == self.execution
                    || self.kind == EntryKind::CicsLink
                        && (self.scope.owner_execution != self.execution
                            || self.scope.owner_selector != actor.selector.as_str()
                            || self.scope.owner_artifact != self.artifact
                            || self.scope.source_execution.as_deref() != Some(source)
                            || self.scope.creation_call.as_deref() != Some(call)
                            || self
                                .scope
                                .selection
                                .as_ref()
                                .is_none_or(|selection| selection.artifact != self.artifact))
                {
                    return Err(HostProblem::UnknownOutcome);
                }
            }
            _ => return Err(HostProblem::UnknownOutcome),
        }
        Ok(())
    }

    pub(super) fn scope_id(&self) -> &str {
        &self.scope.id
    }
    pub(super) fn logical_level(&self) -> u32 {
        self.scope.logical_level
    }

    /// Inspect immutable creation metadata without granting invocation authority.
    pub(super) fn creation_identity(&self) -> (&str, &str, &str, Option<&str>) {
        (
            &self.scope.root_execution,
            &self.scope.task_run,
            &self.scope.principal,
            self.scope.parent_scope.as_deref(),
        )
    }

    pub(super) fn same_scope(&self, other: &Self) -> bool {
        self.scope == other.scope
    }

    pub(super) fn is_creator(&self) -> bool {
        matches!(self.kind, EntryKind::Root | EntryKind::CicsLink)
    }

    pub(super) fn actor_identity(&self) -> (&str, &str, &str, u32) {
        (&self.execution, &self.program, &self.artifact, self.attempt)
    }

    pub(super) fn call_key(&self) -> Option<&str> {
        self.call_key.as_deref()
    }

    /// Validate a stored entry's syntax/integrity, never live or core authority.
    pub(super) fn validate_stored(&self) -> Result<(), HostProblem> {
        self.scope.validate()?;
        if self.schema_version != 1
            || !retention::valid_identity(&self.execution)
            || !valid_program(&self.program)
            || !valid_content(&self.artifact)
            || self.attempt == 0
            || self.attempt != self.scope.owner_attempt
            || self.metadata_digest != self.expected_metadata()?
        {
            return Err(HostProblem::UnknownOutcome);
        }
        match self.kind {
            EntryKind::Root
                if self.scope.parent_scope.is_none()
                    && self.execution == self.scope.root_execution
                    && self.source_execution.is_none()
                    && self.call_key.is_none()
                    && self.scope.owner_selector == format!("program:{}", self.program)
                    && self.scope.owner_artifact == self.artifact =>
            {
                Ok(())
            }
            EntryKind::NativeCall | EntryKind::CicsLink => {
                let call = self
                    .call_key
                    .as_deref()
                    .ok_or(HostProblem::UnknownOutcome)?;
                let source = self
                    .source_execution
                    .as_deref()
                    .ok_or(HostProblem::UnknownOutcome)?;
                if !retention::valid_digest(call)
                    || !retention::valid_identity(source)
                    || source == self.execution
                    || self.execution != child_execution(call)
                    || self.kind == EntryKind::NativeCall
                        && self.scope.owner_execution == self.execution
                    || self.kind == EntryKind::CicsLink
                        && (self.scope.owner_execution != self.execution
                            || self.scope.owner_selector != format!("program:{}", self.program)
                            || self.scope.owner_artifact != self.artifact
                            || self.scope.source_execution.as_deref() != Some(source)
                            || self.scope.creation_call.as_deref() != Some(call)
                            || self
                                .scope
                                .selection
                                .as_ref()
                                .is_none_or(|s| s.artifact != self.artifact))
                {
                    Err(HostProblem::UnknownOutcome)
                } else {
                    Ok(())
                }
            }
            _ => Err(HostProblem::UnknownOutcome),
        }
    }

    pub(super) fn member_key(&self, program: &str) -> Result<String, HostProblem> {
        self.scope.validate()?;
        if !valid_program(program) {
            return Err(HostProblem::Malformed);
        }
        let root = retention::run_state_key(&self.scope.task_run, &self.scope.principal);
        Ok(framed(
            b"mainframe-env.cobol-storage-member@1",
            &[
                root.as_bytes(),
                self.scope.id.as_bytes(),
                program.as_bytes(),
            ],
        ))
    }

    pub(super) fn bind(&self, actor: &mut Invocation) -> Result<(), HostProblem> {
        self.validate_for(actor)?;
        let bytes = encoded(self)?;
        if bytes.len() > MAX_BINDING_BYTES {
            return Err(HostProblem::ResourceExhausted);
        }
        let payload = BoundedPayload::new(SCHEMA, bytes, InvocationLimits::default())
            .map_err(|_| HostProblem::ResourceExhausted)?;
        if let Some(existing) = actor.bindings.get(BINDING) {
            return if existing == &payload {
                Ok(())
            } else {
                Err(HostProblem::UnknownOutcome)
            };
        }
        if actor.bindings.len() >= InvocationLimits::default().max_bindings {
            return Err(HostProblem::ResourceExhausted);
        }
        actor.bindings.insert(BINDING.into(), payload);
        Ok(())
    }

    pub(super) fn read(actor: &Invocation) -> Result<Option<Self>, HostProblem> {
        let Some(payload) = actor.bindings.get(BINDING) else {
            return Ok(None);
        };
        if payload.schema() != SCHEMA || payload.bytes().len() > MAX_BINDING_BYTES {
            return Err(HostProblem::UnknownOutcome);
        }
        let value: Self =
            serde_json::from_slice(payload.bytes()).map_err(|_| HostProblem::UnknownOutcome)?;
        value.validate_for(actor)?;
        if encoded(&value)? != payload.bytes() {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(Some(value))
    }
}
