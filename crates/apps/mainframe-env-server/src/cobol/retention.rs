//! Fail-closed retention metadata shared by the installed-COBOL protocols.

use mainframe_env_store_api::ProviderStateRecord;
use std::fmt;

pub(crate) const CALL_REPLAY_NAMESPACE: &str = "cobol-call-replay@1";
pub(crate) const CALL_PROTOCOL_NAMESPACE: &str = "cobol-call-protocol@2";
pub(crate) const LEGACY_CALL_PROTOCOL_NAMESPACE: &str = "cobol-call-protocol@1";
pub(crate) const RUN_STATE_NAMESPACE: &str = "cobol-run-state@1";
pub(crate) const CANCEL_NAMESPACE: &str = "cobol-cancel@1";
pub(crate) const INSTANCE_NAMESPACE_PREFIX: &str = "cobol-instance@1:";

/// The installed-COBOL row shape represented by a descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CobolRetentionRowKind {
    CallReplay,
    CallProtocol,
    RunState,
    Instance,
    Cancel,
}

/// Whether a validated installed-COBOL row can enter an age-based policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CobolRetentionState {
    /// Work or recovery remains live, so the row must stay in the primary store.
    Active,
    /// A terminal row predates complete owner/age metadata and stays protected.
    LegacyProtected,
    /// The row is terminal and has an explicit nonzero observation tick.
    Terminal,
}

/// A durable owner or provider row which must outlive this row.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum CobolRetentionDependency {
    Execution(String),
    RunUnit(String),
    ProviderRow { namespace: String, key: String },
}

/// Fully validated retention view of one installed-COBOL provider-state row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CobolRetentionRowDescriptor {
    pub(crate) namespace: String,
    pub(crate) key: String,
    pub(crate) row_version: u64,
    pub(crate) kind: CobolRetentionRowKind,
    pub(crate) state: CobolRetentionState,
    pub(crate) owner_execution: Option<String>,
    pub(crate) owner_run_unit: Option<String>,
    pub(crate) terminal_tick: Option<u64>,
    pub(crate) dependencies: Vec<CobolRetentionDependency>,
}

impl CobolRetentionRowDescriptor {
    pub(crate) fn terminal_tick(&self) -> Option<u64> {
        (self.state == CobolRetentionState::Terminal)
            .then_some(self.terminal_tick)
            .flatten()
    }
}

/// A malformed or unsupported row is never interpreted as retention-eligible.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CobolRetentionValidationError {
    WrongNamespace,
    InvalidIdentity,
    CorruptPayload,
    InconsistentState,
}

impl fmt::Display for CobolRetentionValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::WrongNamespace => "unexpected installed-COBOL provider-state namespace",
            Self::InvalidIdentity => "invalid installed-COBOL provider-state identity",
            Self::CorruptPayload => "corrupt installed-COBOL provider-state payload",
            Self::InconsistentState => "inconsistent installed-COBOL lifecycle state",
        })
    }
}

impl std::error::Error for CobolRetentionValidationError {}

pub(super) fn validate_row_identity(
    row: &ProviderStateRecord,
    namespace: &str,
) -> Result<(), CobolRetentionValidationError> {
    if row.namespace != namespace {
        return Err(CobolRetentionValidationError::WrongNamespace);
    }
    if row.version == 0 || row.version > i64::MAX as u64 || !valid_identity(&row.key) {
        return Err(CobolRetentionValidationError::InvalidIdentity);
    }
    Ok(())
}

pub(super) fn valid_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':' | b'/' | b'@')
        })
}

pub(super) fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(crate) fn protocol_key(run_unit: &str) -> String {
    super::replay::digest(&[b"run-protocol", run_unit.as_bytes()])
}

pub(crate) fn run_state_key(run_unit: &str, principal: &str) -> String {
    super::replay::digest(&[b"instance-owner", run_unit.as_bytes(), principal.as_bytes()])
}

pub(super) fn owner_dependencies(execution: &str, run_unit: &str) -> Vec<CobolRetentionDependency> {
    vec![
        CobolRetentionDependency::Execution(execution.into()),
        CobolRetentionDependency::RunUnit(run_unit.into()),
    ]
}

pub(super) fn provider_dependency(
    namespace: &str,
    key: impl Into<String>,
) -> CobolRetentionDependency {
    CobolRetentionDependency::ProviderRow {
        namespace: namespace.into(),
        key: key.into(),
    }
}

/// Validate and describe any provider-state namespace owned by installed COBOL.
pub(crate) fn describe_cobol_retention_row(
    row: &ProviderStateRecord,
) -> Result<CobolRetentionRowDescriptor, CobolRetentionValidationError> {
    match row.namespace.as_str() {
        CALL_REPLAY_NAMESPACE => super::replay::describe_call_replay_row(row),
        CALL_PROTOCOL_NAMESPACE => super::replay::describe_call_protocol_row(row),
        LEGACY_CALL_PROTOCOL_NAMESPACE => {
            if row.version != 1
                || !valid_digest(&row.key)
                || row.payload.as_slice() != b"installed-call@1"
            {
                return Err(CobolRetentionValidationError::InconsistentState);
            }
            Ok(CobolRetentionRowDescriptor {
                namespace: row.namespace.clone(),
                key: row.key.clone(),
                row_version: row.version,
                kind: CobolRetentionRowKind::CallProtocol,
                state: CobolRetentionState::LegacyProtected,
                owner_execution: None,
                owner_run_unit: None,
                terminal_tick: None,
                dependencies: Vec::new(),
            })
        }
        RUN_STATE_NAMESPACE => super::instance::describe_run_state_row(row),
        CANCEL_NAMESPACE => super::instance::describe_cancel_row(row),
        namespace if namespace.starts_with(INSTANCE_NAMESPACE_PREFIX) => {
            super::instance::describe_instance_row(row)
        }
        _ => Err(CobolRetentionValidationError::WrongNamespace),
    }
}

/// Enumerate exact durable owners which must remain valid while `row` is live.
pub(crate) fn cobol_retention_dependencies(
    row: &CobolRetentionRowDescriptor,
) -> &[CobolRetentionDependency] {
    &row.dependencies
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cobol::hardening::{Fixture, TestRoot, call_payload, parent};
    use mainframe_env_execution_api::{ExecutionId, IdempotencyKey, InvocationLimits, RunUnitId};
    use mainframe_env_host_api::{
        EffectRequest, HostProvider, HostRequest, ProgramName, ProgramRequest,
    };
    use mainframe_env_store::{MemoryStore, StoreLimits};
    use mainframe_env_store_api::ProviderStateStore;
    use std::sync::Arc;

    const COUNTER: &str = "IDENTIFICATION DIVISION.\nPROGRAM-ID. COUNTER.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 N PIC 9 VALUE 0.\nLINKAGE SECTION.\n01 OUT-N PIC 9.\nPROCEDURE DIVISION USING OUT-N.\nADD 1 TO N.\nMOVE N TO OUT-N.\nGOBACK.\n";

    fn tampered(row: &ProviderStateRecord, field: &str, value: &str) -> ProviderStateRecord {
        let mut row = row.clone();
        let mut payload: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
        payload[field] = serde_json::Value::String(value.into());
        row.payload = serde_json::to_vec(&payload).unwrap();
        row
    }

    #[test]
    fn active_rows_are_protected_and_completed_lifecycle_is_attributed() {
        let root = TestRoot::new();
        let fixture = Fixture::new(
            &root,
            Arc::new(MemoryStore::new(StoreLimits {
                max_provider_state: 8,
                ..Default::default()
            })),
            mainframe_env_host_api::HostProblem::NotFound,
            false,
        );
        fixture.install("COUNTER", COUNTER);
        let invocation = parent();
        assert!(
            fixture
                .call(&invocation, "COUNTER", 1, call_payload(&[vec![b'0']]))
                .outcome
                .is_ok()
        );

        let replay_rows = fixture
            .store
            .list_provider_state(CALL_REPLAY_NAMESPACE, 8)
            .unwrap();
        for corrupt in [
            tampered(&replay_rows[0], "replay_key", &"c".repeat(64)),
            tampered(&replay_rows[0], "child_execution", "redirected-child"),
            tampered(&replay_rows[0], "run_state_key", &"d".repeat(64)),
        ] {
            assert!(describe_cobol_retention_row(&corrupt).is_err());
        }
        let replay = describe_cobol_retention_row(&replay_rows[0]).unwrap();
        assert_eq!(replay.state, CobolRetentionState::Terminal);
        assert!(replay.terminal_tick().is_some());
        assert_eq!(replay.dependencies.len(), 5);
        assert_eq!(
            replay
                .dependencies
                .iter()
                .filter(|dependency| matches!(dependency, CobolRetentionDependency::Execution(_)))
                .count(),
            2
        );

        let protocol = fixture
            .store
            .list_provider_state(CALL_PROTOCOL_NAMESPACE, 8)
            .unwrap();
        assert!(
            describe_cobol_retention_row(&tampered(
                &protocol[0],
                "owner_execution",
                "redirected-owner"
            ))
            .is_err()
        );
        assert_eq!(
            describe_cobol_retention_row(&protocol[0]).unwrap().state,
            CobolRetentionState::Active
        );
        let run = fixture
            .store
            .list_provider_state(RUN_STATE_NAMESPACE, 8)
            .unwrap();
        assert!(
            describe_cobol_retention_row(&tampered(
                &run[0],
                "owner_execution",
                "redirected-run-owner"
            ))
            .is_err()
        );
        assert_eq!(
            describe_cobol_retention_row(&run[0]).unwrap().state,
            CobolRetentionState::Active
        );
        let instance_namespace = format!("{INSTANCE_NAMESPACE_PREFIX}{}", run[0].key);
        let instances = fixture
            .store
            .list_provider_state(&instance_namespace, 8)
            .unwrap();
        assert_eq!(
            describe_cobol_retention_row(&instances[0]).unwrap().state,
            CobolRetentionState::Active
        );

        let cancel = EffectRequest {
            run_unit: invocation.run_unit_id.clone(),
            sequence: 2,
            deadline_tick: invocation.deadline_tick,
            idempotency_key: Some(
                IdempotencyKey::new("cancel-for-retention", InvocationLimits::default()).unwrap(),
            ),
            request: HostRequest::Program(ProgramRequest::Cancel {
                programs: vec![ProgramName::new("COUNTER", 128).unwrap()],
            }),
        };
        assert!(fixture.router.invoke(&invocation, cancel).outcome.is_ok());
        let cancel_rows = fixture
            .store
            .list_provider_state(CANCEL_NAMESPACE, 8)
            .unwrap();
        assert!(
            describe_cobol_retention_row(&tampered(&cancel_rows[0], "cancel_key", &"e".repeat(64)))
                .is_err()
        );
        let cancel = describe_cobol_retention_row(&cancel_rows[0]).unwrap();
        assert_eq!(cancel.state, CobolRetentionState::Terminal);
        assert!(cancel.terminal_tick().is_some());
        assert!(
            cobol_retention_dependencies(&cancel)
                .iter()
                .any(|dependency| {
                    matches!(
                            dependency,
                            CobolRetentionDependency::ProviderRow { namespace, .. }
                                if namespace == CALL_PROTOCOL_NAMESPACE
                    )
                })
        );
        assert!(
            cobol_retention_dependencies(&cancel)
                .iter()
                .any(|dependency| {
                    matches!(
                        dependency,
                        CobolRetentionDependency::ProviderRow { namespace, .. }
                            if namespace == RUN_STATE_NAMESPACE
                    )
                })
        );

        fixture.router.finish_run_unit(&invocation).unwrap();
        for namespace in [RUN_STATE_NAMESPACE, CALL_PROTOCOL_NAMESPACE] {
            let rows = fixture.store.list_provider_state(namespace, 8).unwrap();
            let descriptor = describe_cobol_retention_row(&rows[0]).unwrap();
            assert_eq!(descriptor.state, CobolRetentionState::Terminal);
            assert!(descriptor.terminal_tick().is_some());
        }
        assert!(
            fixture
                .store
                .list_provider_state(&instance_namespace, 8)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn legacy_and_corrupt_rows_fail_closed() {
        let ownerless_current_protocol = ProviderStateRecord {
            namespace: CALL_PROTOCOL_NAMESPACE.into(),
            key: protocol_key("legacy-run"),
            version: 1,
            payload: b"installed-call@2".to_vec(),
        };
        assert_eq!(
            describe_cobol_retention_row(&ownerless_current_protocol)
                .unwrap()
                .state,
            CobolRetentionState::Active
        );
        let legacy_protocol = ProviderStateRecord {
            namespace: LEGACY_CALL_PROTOCOL_NAMESPACE.into(),
            key: protocol_key("legacy-run"),
            version: 1,
            payload: b"installed-call@1".to_vec(),
        };
        assert_eq!(
            describe_cobol_retention_row(&legacy_protocol)
                .unwrap()
                .state,
            CobolRetentionState::LegacyProtected
        );

        let corrupt = ProviderStateRecord {
            namespace: CALL_REPLAY_NAMESPACE.into(),
            key: "a".repeat(64),
            version: 2,
            payload: serde_json::to_vec(&serde_json::json!({
                "schema_version": 2,
                "fingerprint": "b".repeat(64),
                "child_execution": "child",
                "owner_execution": "owner",
                "owner_run_unit": "run",
                "completion_tick": 0,
                "reply": {"schema":"mainframe-env.cobol.call@1","bytes":[]}
            }))
            .unwrap(),
        };
        assert!(describe_cobol_retention_row(&corrupt).is_err());
    }

    #[test]
    fn parse_valid_protocol_owner_substitution_is_rejected_by_the_live_caller() {
        let root = TestRoot::new();
        let store = Arc::new(MemoryStore::new(Default::default()));
        let fixture = Fixture::new(
            &root,
            store.clone(),
            mainframe_env_host_api::HostProblem::NotFound,
            false,
        );
        fixture.install("COUNTER", COUNTER);
        let invocation = parent();
        let run_state = run_state_key(
            invocation.run_unit_id.as_str(),
            invocation.principal.id().as_str(),
        );
        let ended = 0_u64.to_be_bytes();
        let metadata = super::super::replay::digest(&[
            b"protocol-metadata",
            b"redirected-owner",
            invocation.run_unit_id.as_str().as_bytes(),
            invocation.principal.id().as_str().as_bytes(),
            run_state.as_bytes(),
            &ended,
        ]);
        let key = protocol_key(invocation.run_unit_id.as_str());
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: CALL_PROTOCOL_NAMESPACE.into(),
                    key,
                    version: 1,
                    payload: serde_json::to_vec(&serde_json::json!({
                        "schema_version": 2,
                        "owner_execution": "redirected-owner",
                        "owner_run_unit": invocation.run_unit_id.as_str(),
                        "owner_principal": invocation.principal.id().as_str(),
                        "run_state_key": run_state,
                        "metadata_digest": metadata,
                        "ended_tick": null
                    }))
                    .unwrap(),
                },
                None,
            )
            .unwrap();
        assert_eq!(
            fixture
                .call(&invocation, "COUNTER", 1, call_payload(&[vec![b'0']]))
                .outcome,
            Err(mainframe_env_host_api::HostProblem::UnknownOutcome)
        );
    }

    #[test]
    fn external_terminal_removal_is_seen_without_a_cobol_cache_or_restart() {
        let root = TestRoot::new();
        let store = Arc::new(MemoryStore::new(StoreLimits {
            max_provider_state: 6,
            ..Default::default()
        }));
        let fixture = Fixture::new(
            &root,
            store.clone(),
            mainframe_env_host_api::HostProblem::NotFound,
            false,
        );
        fixture.install("COUNTER", COUNTER);
        let first = parent();
        assert!(
            fixture
                .call(&first, "COUNTER", 1, call_payload(&[vec![b'0']]))
                .outcome
                .is_ok()
        );
        fixture.router.finish_run_unit(&first).unwrap();
        for namespace in [
            CALL_REPLAY_NAMESPACE,
            CALL_PROTOCOL_NAMESPACE,
            RUN_STATE_NAMESPACE,
            CANCEL_NAMESPACE,
        ] {
            for row in store.list_provider_state(namespace, 6).unwrap() {
                if describe_cobol_retention_row(&row)
                    .is_ok_and(|descriptor| descriptor.state == CobolRetentionState::Terminal)
                {
                    store
                        .delete_provider_state(&row.namespace, &row.key, row.version)
                        .unwrap();
                }
            }
        }

        let mut second = parent();
        second.execution_id =
            ExecutionId::new("second-execution", InvocationLimits::default()).unwrap();
        second.run_unit_id = RunUnitId::new("second-run", InvocationLimits::default()).unwrap();
        assert!(
            fixture
                .call(&second, "COUNTER", 1, call_payload(&[vec![b'0']]))
                .outcome
                .is_ok(),
            "ordinary store reads must reuse provider-state capacity after retention"
        );
    }
}
