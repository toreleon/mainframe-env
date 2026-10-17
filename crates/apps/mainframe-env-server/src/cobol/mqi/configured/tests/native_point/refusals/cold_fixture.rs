//! Changed original-fixture inputs: real closed manager, compiled caller and reply.
//! This observes a returned packet; it cannot manufacture host/root authority.
use super::super::successor::{
    COLD, NEXT, control_is_exact_next_incarnation, original_core, router_for,
};
use super::*;
use crate::cobol::NativeRootReplyObservation;
use mainframe_env_execution_api::{AuditSubjectRecord, BoundedPayload, InvocationLimits, Selector};
use mainframe_env_host_api::{HostRequest, HostResult, ProgramName, ProgramRequest};
use mainframe_env_store::LocalArtifactStore;
#[cfg(unix)]
use mainframe_env_store::SqliteStateStore;
use mainframe_env_store_api::{
    EffectRecord, ExecutionRecord, OutboxRecord, ProviderStateIdentity, ROOT_CUSTODY_NAMESPACE,
    ROOT_DRIVER_NAMESPACE, RootCustodyCutoff, TerminalRowDependency,
};

// Source: MQNEXT's first CALL USING OUTCOME; call_codec.rs count/name/mode/data.
// These 36 bytes are frozen independently of the product reply/receipt.
const CALL_BYTES: &[u8] = b"\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x07OUTCOME\x01\x00\x00\x00\x00\x00\x00\x00\x08        ";
// Source: byte-identical MQCOLD MOVE 'DONE' TO PIC X(8), call-result count/data.
const REPLY_BYTES: &[u8] = b"\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x08DONE    ";

pub(super) enum Source {
    Memory,
    #[cfg(unix)]
    OwnedSqlite(Arc<SqliteStateStore>),
}

pub(super) fn fixture(sqlite: bool) -> (Fixture, Source) {
    if !sqlite {
        return (native(false, 1), Source::Memory);
    }
    #[cfg(unix)]
    {
        let (fixture, source) =
            Fixture::owned_native_points(include_bytes!("../../native_rich_md1.json"));
        (fixture, Source::OwnedSqlite(source))
    }
    #[cfg(not(unix))]
    {
        // Keep this required backend visible. An unsupported physical custody
        // profile cannot silently substitute Memory or waive the cold test.
        panic!("genuine held-file SQLite custody is unsupported on this platform")
    }
}

pub(super) fn install(fixture: &Fixture) {
    // All actual catalog/artifact inputs precede the original live root.
    // COLD is byte-identical to the historical connection-only COBOL body.
    fixture.install("MQCOLD", COLD);
    fixture.install("MQNEXT", NEXT);
}

type Core = (
    Vec<Option<ExecutionRecord>>,
    Vec<Option<EffectRecord>>,
    Vec<mainframe_env_execution_api::LifecycleEvent>,
    Vec<OutboxRecord>,
    Vec<AuditSubjectRecord>,
);
type Packet = (Invocation, EffectRequest, EffectResult);

pub(super) struct Proof {
    pub store: Arc<dyn PlatformStore>,
    capture: RootCustodyCutoff,
    before: Core,
    next: Invocation,
    cold_artifact: mainframe_env_execution_api::ArtifactRef,
    children: Vec<Invocation>,
    packet: Option<Packet>,
    overflow: bool,
    handoff: Vec<ProviderStateIdentity>,
    provider_epoch: u64,
}

pub(super) fn connect(
    source: Source,
    manager: &NativeMqRootManager,
    old: &Invocation,
    old_host: &Arc<ConfiguredInstalledMqHost>,
    artifacts: Arc<LocalArtifactStore>,
) -> (ExecutionOutcome, Option<EffectResult>, Proof) {
    // Cut off the actual live Open graph. No terminal/settlement is inferred.
    let custody = manager.cutoff_live_root(old).unwrap();
    let capture = custody.captured().clone();
    let before = original_core(&*old_host.store, &capture);
    let fresh = match source {
        Source::Memory => {
            let fresh = manager.prepare_successor_host(custody).unwrap();
            assert!(Arc::ptr_eq(&fresh.store, &old_host.store));
            fresh
        }
        #[cfg(unix)]
        Source::OwnedSqlite(source) => {
            let fresh = manager
                .prepare_owned_sqlite_successor(custody, source.clone())
                .unwrap();
            assert!(
                source.get_execution(&old.execution_id).is_err(),
                "actual originating SQLx pool must be closed"
            );
            assert!(!Arc::ptr_eq(&fresh.store, &old_host.store));
            fresh
        }
    };
    assert!(!Arc::ptr_eq(&fresh, old_host));
    assert!(Arc::ptr_eq(&fresh.authorizer, &old_host.authorizer));
    assert!(!Arc::ptr_eq(&fresh.control, &old_host.control));
    let store = fresh.store.clone();
    let (router, program, factory) = router_for(&fresh, store.clone(), artifacts);
    let observer = Arc::new(Mutex::new(NativeRootReplyObservation::default()));
    assert!(
        router
            .cobol
            .native_root_reply_observer
            .set(observer.clone())
            .is_ok()
    );
    let admitted = router
        .cobol
        .preflight_installed_program("MQNEXT", true)
        .unwrap();
    let cold_artifact = router
        .cobol
        .preflight_installed_program("MQCOLD", true)
        .unwrap()
        .artifact;
    // This is the actual original parentNone context, not a child rewritten to
    // look like a root. Only real native admission can create the new claim.
    let mut next = old.clone();
    next.execution_id = ExecutionId::new("cold-point-root", Default::default()).unwrap();
    next.run_unit_id = RunUnitId::new("cold-point-run", Default::default()).unwrap();
    next.idempotency_key = IdempotencyKey::new("cold-point-root-key", Default::default()).unwrap();
    next.selector = Selector::new("program:MQNEXT", Default::default()).unwrap();
    next.artifact = admitted.artifact;
    let exact = next.clone();
    let outcome = router
        .execute_native_mq_root(&fresh, &next, "MQNEXT")
        .unwrap();
    assert_eq!(next, exact);
    let observation = observer.lock().unwrap();
    let packet = observation.first.clone();
    let overflow = observation.overflow;
    let reply = packet.as_ref().map(|(_, _, returned)| returned.clone());
    let children = factory.children.lock().unwrap().clone();
    let handoff = old_host.runtime.native_custody_handoff_rows();
    let provider_epoch = store.provider_state_retention_epoch().unwrap();
    drop(program);
    (
        outcome,
        reply,
        Proof {
            store,
            capture,
            before,
            next,
            cold_artifact,
            children,
            packet,
            overflow,
            handoff,
            provider_epoch,
        },
    )
}

fn packet_matches(expected: &Invocation, actual: &Packet) -> bool {
    let (original, offer, returned) = actual;
    if expected.parent_execution_id.is_some()
        || expected.selector.as_str() != "program:MQNEXT"
        || original != expected
        || offer.run_unit != expected.run_unit_id
        || offer.sequence != 1
        || offer.deadline_tick != expected.deadline_tick
        || offer.idempotency_key.as_ref().map(IdempotencyKey::as_str)
            != Some(format!("{}:1", expected.idempotency_key.as_str()).as_str())
        || returned.sequence != 1
    {
        return false;
    }
    let HostRequest::Program(ProgramRequest::Call {
        program,
        payload,
        service: None,
    }) = &offer.request
    else {
        return false;
    };
    if program.as_str() != "MQCOLD"
        || payload.schema() != "mainframe-env.cobol.call@1"
        || payload.bytes() != CALL_BYTES
    {
        return false;
    }
    matches!(&returned.outcome, Ok(HostResult::Program(payload))
        if payload.schema() == "mainframe-env.cobol.call-result@1"
            && payload.bytes() == REPLY_BYTES)
}

fn observation_matches(expected: &Invocation, packet: Option<&Packet>, overflow: bool) -> bool {
    !overflow && packet.is_some_and(|packet| packet_matches(expected, packet))
}

impl Proof {
    pub(super) fn assert_completed(&self, outcome: &ExecutionOutcome, reply: &EffectResult) {
        let ExecutionOutcome::Completed(done) = outcome else {
            panic!("the original completion guard must pass first")
        };
        assert_eq!(done.output.bytes(), b"DONE    \n");
        assert!(
            observation_matches(&self.next, self.packet.as_ref(), self.overflow),
            "one exact actual compiled program CALL reply, without overflow"
        );
        let packet = self
            .packet
            .as_ref()
            .expect("actual private-provider returned packet");
        assert!(packet_matches(&self.next, packet), "{packet:?}");
        assert_eq!(&packet.2, reply);
        assert_eq!(self.children.len(), 1);
        let child = &self.children[0];
        assert_eq!(
            child.parent_execution_id.as_ref(),
            Some(&self.next.execution_id)
        );
        assert_eq!(child.run_unit_id, self.next.run_unit_id);
        assert_eq!(child.selector.as_str(), "program:MQCOLD");
        assert_eq!(child.artifact, self.cold_artifact);
        assert_eq!(child.principal, self.next.principal);
        assert_eq!(child.deadline_tick, self.next.deadline_tick);
        assert_eq!(child.attempt, self.next.attempt);
        assert_eq!(child.provider_generations, self.next.provider_generations);
        for actor in [&self.next, child] {
            let core = self
                .store
                .get_execution(&actor.execution_id)
                .unwrap()
                .unwrap();
            assert_eq!(core.state, ExecutionState::Completed);
            assert_eq!(core.run_unit_id, actor.run_unit_id);
            assert_eq!(core.principal, *actor.principal.id());
            assert_eq!(core.artifact, actor.artifact);
            assert_eq!(core.selector, actor.selector);
            assert_eq!(core.attempt, actor.attempt);
        }
        let row = self
            .store
            .get_provider_state(ROOT_DRIVER_NAMESPACE, self.next.execution_id.as_str())
            .unwrap()
            .unwrap();
        let root: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
        assert_eq!(root["schema"], "mainframe-env.core-root-ownership@2");
        assert_eq!(root["phase"], 2);
        assert_eq!(root["actors"].as_array().unwrap().len(), 2);
        for dependency in &self.capture.preserved.provider_dependencies {
            let row = match dependency {
                TerminalRowDependency::Exact(row) => row,
                TerminalRowDependency::Absent { namespace, key } => {
                    assert!(
                        self.store
                            .get_provider_state(namespace, key)
                            .unwrap()
                            .is_none()
                    );
                    continue;
                }
            };
            let actual = self
                .store
                .get_provider_state(&row.namespace, &row.key)
                .unwrap()
                .unwrap();
            if row.namespace == "durable-root-scope-v1" {
                if actual != *row {
                    assert_eq!(actual.version, row.version + 1);
                    assert_eq!(actual.payload, self.next.execution_id.as_str().as_bytes());
                }
            } else if let Some(index) = self
                .handoff
                .iter()
                .position(|identity| identity.namespace == row.namespace && identity.key == row.key)
            {
                assert!(actual.version > row.version);
                if index != 0 {
                    assert_eq!(actual.payload, row.payload);
                } else {
                    let retained: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
                    let advanced: serde_json::Value =
                        serde_json::from_slice(&actual.payload).unwrap();
                    assert!(control_is_exact_next_incarnation(&retained, &advanced));
                }
            } else {
                assert_eq!(actual, *row);
            }
        }
        let receipts = self
            .store
            .list_provider_state("mq-selected-v1-occurrence", 128)
            .unwrap();
        assert_eq!(receipts.len(), 2);
        assert_eq!(receipts.iter().filter(|row| {
            serde_json::from_slice::<serde_json::Value>(&row.payload).unwrap()["value"]["execution"]
                == child.execution_id.as_str()
        }).count(), 1);
        let calls = self
            .store
            .list_provider_state("cobol-call-replay@1", 128)
            .unwrap();
        assert_eq!(calls.len(), 1);
        let call: serde_json::Value = serde_json::from_slice(&calls[0].payload).unwrap();
        assert_eq!(call["schema_version"], 2);
        assert_eq!(call["child_execution"], child.execution_id.as_str());
        assert_eq!(call["owner_execution"], self.next.execution_id.as_str());
        assert_eq!(call["owner_run_unit"], self.next.run_unit_id.as_str());
        assert_eq!(
            call["reply"],
            serde_json::json!({
                "schema": "mainframe-env.cobol.call-result@1", "bytes": REPLY_BYTES
            })
        );
        self.assert_retained(
            &self
                .capture
                .preserved
                .claim
                .admission()
                .execution
                .execution_id,
        );
    }

    pub(super) fn assert_retained(&self, old: &ExecutionId) {
        assert_eq!(original_core(&*self.store, &self.capture), self.before);
        assert_eq!(
            self.store.provider_state_retention_epoch().unwrap(),
            self.provider_epoch
        );
        assert_eq!(
            self.store
                .get_provider_state(ROOT_DRIVER_NAMESPACE, old.as_str())
                .unwrap(),
            Some(self.capture.fenced.clone())
        );
        assert_eq!(
            self.store
                .get_provider_state(ROOT_CUSTODY_NAMESPACE, old.as_str())
                .unwrap()
                .unwrap()
                .version,
            2
        );
    }
}

#[test]
fn cold_reply_observer_refuses_changed_context_offer_reply_and_overflow() {
    // Controlled source-derived oracle input only; no admission/native credit.
    let mut next = crate::cobol::hardening::parent();
    next.selector = Selector::new("program:MQNEXT", Default::default()).unwrap();
    next.deadline_tick = 1000;
    let offer = EffectRequest {
        run_unit: next.run_unit_id.clone(),
        sequence: 1,
        deadline_tick: next.deadline_tick,
        idempotency_key: Some(
            IdempotencyKey::new(
                format!("{}:1", next.idempotency_key.as_str()),
                Default::default(),
            )
            .unwrap(),
        ),
        request: HostRequest::Program(ProgramRequest::Call {
            program: ProgramName::new(
                "MQCOLD",
                mainframe_env_host_api::HostLimits::default().max_name_bytes,
            )
            .unwrap(),
            payload: BoundedPayload::new(
                "mainframe-env.cobol.call@1",
                CALL_BYTES.to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
            service: None,
        }),
    };
    let reply = EffectResult {
        sequence: 1,
        outcome: Ok(HostResult::Program(
            BoundedPayload::new(
                "mainframe-env.cobol.call-result@1",
                REPLY_BYTES.to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        )),
    };
    let exact = (next.clone(), offer.clone(), reply.clone());
    assert!(packet_matches(&next, &exact));
    assert!(observation_matches(&next, Some(&exact), false));
    assert!(!observation_matches(&next, None, false));
    assert!(!observation_matches(&next, Some(&exact), true));
    for changed in [
        "request",
        "execution",
        "run",
        "parent",
        "selector",
        "artifact",
        "principal",
        "grants",
        "attempt",
        "key",
        "deadline",
        "bindings",
        "generation",
        "audit",
        "trace",
        "class",
        "priority",
        "limits",
        "cancellation-probe",
    ] {
        let mut bad = exact.clone();
        let limits = InvocationLimits::default();
        match changed {
            "request" => {
                bad.0.request_id =
                    mainframe_env_execution_api::RequestId::new("other", limits).unwrap()
            }
            "execution" => bad.0.execution_id = ExecutionId::new("other", limits).unwrap(),
            "run" => bad.0.run_unit_id = RunUnitId::new("other", limits).unwrap(),
            "parent" => bad.0.parent_execution_id = Some(next.execution_id.clone()),
            "selector" => bad.0.selector = Selector::new("program:OTHER", limits).unwrap(),
            "artifact" => {
                bad.0.artifact =
                    mainframe_env_execution_api::ArtifactRef::new("other", limits).unwrap()
            }
            "principal" => {
                bad.0.principal = mainframe_env_execution_api::Principal::new(
                    mainframe_env_execution_api::PrincipalId::new("OTHER", limits).unwrap(),
                    next.principal.grants().clone(),
                    limits,
                )
                .unwrap()
            }
            "grants" => {
                bad.0.principal = mainframe_env_execution_api::Principal::new(
                    next.principal.id().clone(),
                    Default::default(),
                    limits,
                )
                .unwrap()
            }
            "attempt" => bad.0.attempt += 1,
            "key" => bad.0.idempotency_key = IdempotencyKey::new("other", limits).unwrap(),
            "deadline" => bad.0.deadline_tick += 1,
            "bindings" => {
                bad.0.bindings.insert(
                    "other".into(),
                    BoundedPayload::new("test@1", vec![1], limits).unwrap(),
                );
            }
            "generation" => {
                bad.0.provider_generations.insert(
                    mainframe_env_execution_api::CapabilityId::new("host.program.invoke", limits)
                        .unwrap(),
                    "other".into(),
                );
            }
            "audit" => bad.0.audit_correlation = "other".into(),
            "trace" => {
                bad.0.trace_id = mainframe_env_execution_api::TraceId::new("other", limits).unwrap()
            }
            "class" => bad.0.service_class = mainframe_env_execution_api::ServiceClass::Interactive,
            "priority" => bad.0.priority += 1,
            "limits" => bad.0.limits.max_steps -= 1,
            "cancellation-probe" => {
                bad.0.cancellation_probe =
                    Some(mainframe_env_execution_api::CancellationProbe::new())
            }
            _ => unreachable!(),
        }
        assert!(!packet_matches(&next, &bad), "context {changed}");
    }
    for changed in [
        "run",
        "sequence",
        "deadline",
        "key",
        "missing-key",
        "program",
        "kind",
        "schema",
        "service",
    ] {
        let mut bad = exact.clone();
        match changed {
            "run" => bad.1.run_unit = RunUnitId::new("other", Default::default()).unwrap(),
            "sequence" => bad.1.sequence += 1,
            "deadline" => bad.1.deadline_tick += 1,
            "key" => {
                bad.1.idempotency_key =
                    Some(IdempotencyKey::new("other", Default::default()).unwrap())
            }
            "missing-key" => bad.1.idempotency_key = None,
            "program" => {
                if let HostRequest::Program(ProgramRequest::Call { program, .. }) =
                    &mut bad.1.request
                {
                    *program = ProgramName::new(
                        "OTHER",
                        mainframe_env_host_api::HostLimits::default().max_name_bytes,
                    )
                    .unwrap();
                }
            }
            "kind" => {
                bad.1.request = HostRequest::Program(ProgramRequest::Inquire {
                    program: ProgramName::new(
                        "MQCOLD",
                        mainframe_env_host_api::HostLimits::default().max_name_bytes,
                    )
                    .unwrap(),
                })
            }
            "schema" => {
                if let HostRequest::Program(ProgramRequest::Call { payload, .. }) =
                    &mut bad.1.request
                {
                    *payload =
                        BoundedPayload::new("other@1", CALL_BYTES.to_vec(), Default::default())
                            .unwrap();
                }
            }
            "service" => {
                if let HostRequest::Program(ProgramRequest::Call { service, .. }) =
                    &mut bad.1.request
                {
                    *service = Some(mainframe_env_host_api::RuntimeServiceSelector {
                        kind: mainframe_env_host_api::RuntimeServiceKind::LanguageEnvironment,
                        name: mainframe_env_host_api::RuntimeServiceName::new(
                            "CEEDAYS",
                            mainframe_env_host_api::HostLimits::default().max_name_bytes,
                        )
                        .unwrap(),
                        abi_version: 1,
                    });
                }
            }
            _ => unreachable!(),
        }
        assert!(!packet_matches(&next, &bad), "offer {changed}");
    }
    for (is_reply, bytes) in [(false, CALL_BYTES), (true, REPLY_BYTES)] {
        for offset in 0..=bytes.len() {
            let mut changed = bytes.to_vec();
            if offset == bytes.len() {
                changed.push(0);
            } else {
                changed[offset] ^= 1;
            }
            let mut bad = exact.clone();
            if is_reply {
                bad.2.outcome = Ok(HostResult::Program(
                    BoundedPayload::new(
                        "mainframe-env.cobol.call-result@1",
                        changed,
                        Default::default(),
                    )
                    .unwrap(),
                ));
            } else if let HostRequest::Program(ProgramRequest::Call { payload, .. }) =
                &mut bad.1.request
            {
                *payload =
                    BoundedPayload::new("mainframe-env.cobol.call@1", changed, Default::default())
                        .unwrap();
            }
            assert!(!packet_matches(&next, &bad), "byte {is_reply}/{offset}");
        }
    }
    for changed in ["sequence", "schema", "unknown"] {
        let mut bad = exact.clone();
        match changed {
            "sequence" => bad.2.sequence += 1,
            "schema" => {
                bad.2.outcome = Ok(HostResult::Program(
                    BoundedPayload::new("other@1", REPLY_BYTES.to_vec(), Default::default())
                        .unwrap(),
                ))
            }
            "unknown" => bad.2.outcome = Err(HostProblem::UnknownOutcome),
            _ => unreachable!(),
        }
        assert!(!packet_matches(&next, &bad), "reply {changed}");
    }
    let mut observed = NativeRootReplyObservation::default();
    observed.record(&next, offer.clone(), &reply);
    assert_eq!(observed.first.as_ref(), Some(&exact));
    assert!(!observed.overflow);
    assert!(observation_matches(
        &next,
        observed.first.as_ref(),
        observed.overflow
    ));
    let mut other = reply.clone();
    other.sequence += 1;
    observed.record(&next, offer, &other);
    assert_eq!(observed.first.as_ref(), Some(&exact));
    assert!(
        observed.overflow,
        "a second packet must not overwrite the first"
    );
    assert!(!observation_matches(
        &next,
        observed.first.as_ref(),
        observed.overflow
    ));
}
