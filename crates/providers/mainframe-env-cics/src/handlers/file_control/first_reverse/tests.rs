use super::*;
use crate::service::{CicsLimits, Invocation, InvocationLimits, SessionId};
use mainframe_env_execution_api::{BoundedPayload, CapabilityId, IdempotencyKey};
use mainframe_env_host_api::{
    CapabilityDescriptor, CicsConditionPolicy, EffectRequest, EffectResult, HostLimits,
    HostProvider, Mutation, RegistrySnapshot, ScopedHostService, SecurityDecision,
};
use mainframe_env_store::MemoryStore;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

type Replies = Arc<Mutex<VecDeque<Result<DatasetResult, HostProblem>>>>;
type Calls = Arc<Mutex<Vec<DatasetRequest>>>;

#[test]
fn task_end_browse_uses_original_actor_and_honors_saf_denial() {
    let mut f = Fixture::new();
    f.seed();
    let original = f.run.current_program.effect_invocation.clone();
    f.deny.store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        super::super::task_end::release_task(&f.service, &mut f.run),
        Err(HostProblem::Unauthorized)
    );
    f.pending(b"BB");
    assert!(f.calls.lock().unwrap().is_empty());
    f.deny.store(false, std::sync::atomic::Ordering::SeqCst);
    f.queue(Ok(empty("CURSOR-1")));
    super::super::task_end::release_task(&f.service, &mut f.run).unwrap();
    assert!(
        f.actors
            .lock()
            .unwrap()
            .iter()
            .all(|actor| actor == &original)
    );
}

#[test]
fn task_end_browse_checks_deadline_before_and_after_saf() {
    struct Clock(Mutex<VecDeque<u64>>);
    impl crate::service::CicsReplayClock for Clock {
        fn now_tick(&self) -> Result<u64, HostProblem> {
            Ok(self.0.lock().unwrap().pop_front().unwrap())
        }
    }
    for after_saf in [false, true] {
        let mut f = Fixture::new();
        f.seed();
        let deadline = f.run.current_program.effect_invocation.deadline_tick;
        assert_eq!(
            super::super::task_end::check_cleanup_deadline(&f.run, deadline),
            Err(HostProblem::TimedOut)
        );
        super::super::task_end::check_cleanup_deadline(&f.run, deadline - 1).unwrap();
        let ticks = if after_saf {
            vec![1, deadline]
        } else {
            vec![deadline]
        };
        Arc::get_mut(&mut f.service).unwrap().replay_clock =
            Some(Arc::new(Clock(Mutex::new(ticks.into()))));
        assert_eq!(
            super::super::task_end::release_task(&f.service, &mut f.run),
            Err(HostProblem::TimedOut)
        );
        assert!(f.calls.lock().unwrap().is_empty());
        f.pending(b"BB");
    }
}

#[test]
fn task_end_browse_retains_per_cursor_progress_after_refusal() {
    let mut f = Fixture::new();
    f.seed();
    f.run.browses.insert("ZZFILE".into(), "CURSOR-2".into());
    f.service
        .lock()
        .unwrap()
        .runs
        .insert(f.run.invocation.run_unit_id.clone(), f.run.clone());
    f.queue(Ok(empty("CURSOR-1")));
    f.queue(Err(HostProblem::ProviderFailure));
    assert_eq!(
        super::super::task_end::release_task(&f.service, &mut f.run),
        Err(HostProblem::ProviderFailure)
    );
    assert_eq!(f.run.browses.len(), 1);
    assert_eq!(f.run.browses["ZZFILE"], "CURSOR-2");
    assert!(f.run.initial_browse_positions.is_empty());
    assert_eq!(
        f.service.lock().unwrap().runs[&f.run.invocation.run_unit_id].browses,
        f.run.browses
    );
    f.queue(Ok(empty("CURSOR-2")));
    super::super::task_end::release_task(&f.service, &mut f.run).unwrap();
    assert!(f.run.browses.is_empty());
    let calls = f.calls.lock().unwrap();
    assert_eq!(calls.len(), 3);
    assert!(matches!(&calls[2], DatasetRequest::EndBrowse { cursor, .. } if cursor == "CURSOR-2"));
}

#[test]
fn task_end_browse_unknown_or_unbound_reply_preserves_owner() {
    for reply in [
        Err(HostProblem::UnknownOutcome),
        Ok(empty("FOREIGN")),
        Ok(full("CURSOR-1", b"AA01", b"AA", b"AA")),
        Ok(DatasetResult::Attributes {
            attributes: attributes(),
            version: 1,
        }),
    ] {
        let mut f = Fixture::new();
        f.seed();
        f.queue(reply);
        assert_eq!(
            super::super::task_end::release_task(&f.service, &mut f.run),
            Err(HostProblem::UnknownOutcome)
        );
        f.pending(b"BB");
        assert_eq!(
            f.service
                .lock()
                .unwrap()
                .task_dispatch
                .require_available_session("session"),
            Err(HostProblem::UnknownOutcome)
        );
    }
}

#[test]
fn task_end_browse_cannot_widen_grants_generations_or_cancellation() {
    for case in 0..3 {
        let mut f = Fixture::new();
        f.seed();
        let actor = &mut f.run.current_program.effect_invocation;
        match case {
            0 => {
                actor.principal = mainframe_env_execution_api::Principal::new(
                    actor.principal.id().clone(),
                    Default::default(),
                    InvocationLimits::default(),
                )
                .unwrap()
            }
            1 => {
                actor.provider_generations.insert(
                    CapabilityId::new("host.dataset.read", InvocationLimits::default()).unwrap(),
                    "stale".into(),
                );
            }
            _ => {
                let probe = mainframe_env_execution_api::CancellationProbe::new();
                probe.request();
                actor.cancellation_probe = Some(probe);
            }
        }
        let original = actor.clone();
        assert!(super::super::task_end::release_task(&f.service, &mut f.run).is_err());
        assert_eq!(f.run.current_program.effect_invocation, original);
        assert!(f.calls.lock().unwrap().is_empty());
        f.pending(b"BB");
    }
}

struct Authority {
    descriptor: CapabilityDescriptor,
    replies: Replies,
    calls: Calls,
    deny: Arc<std::sync::atomic::AtomicBool>,
    actors: Arc<Mutex<Vec<Invocation>>>,
}

impl HostProvider for Authority {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }
    fn invoke(&self, actor: &Invocation, effect: EffectRequest) -> EffectResult {
        self.actors.lock().unwrap().push(actor.clone());
        let outcome = match effect.request {
            HostRequest::Security(_) => Ok(HostResult::Security(
                if self.deny.load(std::sync::atomic::Ordering::SeqCst) {
                    SecurityDecision::Deny
                } else {
                    SecurityDecision::Allow
                },
            )),
            HostRequest::Dataset(request) => {
                self.calls.lock().unwrap().push(request.clone());
                if matches!(request, DatasetRequest::Attributes { .. }) {
                    Ok(HostResult::Dataset(DatasetResult::Attributes {
                        attributes: attributes(),
                        version: 1,
                    }))
                } else {
                    self.replies
                        .lock()
                        .unwrap()
                        .pop_front()
                        .expect("one controlled reply per delegate")
                        .map(HostResult::Dataset)
                }
            }
            _ => Err(HostProblem::Unsupported),
        };
        EffectResult {
            sequence: effect.sequence,
            outcome,
        }
    }
}

fn attributes() -> DatasetAttributes {
    DatasetAttributes {
        organization: DatasetOrganization::KeySequenced,
        record_format: RecordFormat::Fixed,
        logical_record_length: 4,
        key_offset: Some(0),
        key_length: Some(2),
        ccsid: None,
    }
}

fn literal(bytes: &[u8]) -> BoundedPayload {
    BoundedPayload::new(
        "mainframe-env.cics.literal@1",
        bytes.to_vec(),
        InvocationLimits::default(),
    )
    .unwrap()
}

fn request(operation: CicsOperation, rid: &[u8]) -> CicsRequest {
    let mut arguments = BTreeMap::from([("DATASET".into(), literal(b"REVFILE"))]);
    if operation != CicsOperation::EndBrowse {
        arguments.insert("RIDFLD".into(), literal(rid));
    }
    CicsRequest {
        operation,
        arguments,
        condition_policy: CicsConditionPolicy::NoHandle,
        mutation: None,
    }
}

fn full(cursor: &str, record: &[u8], key: &[u8], identity: &[u8]) -> DatasetResult {
    DatasetResult::Browse {
        cursor: cursor.into(),
        record: Some(record.to_vec()),
        key: Some(key.to_vec()),
        identity: Some(identity.to_vec()),
    }
}

fn empty(cursor: &str) -> DatasetResult {
    DatasetResult::Browse {
        cursor: cursor.into(),
        record: None,
        identity: None,
        key: None,
    }
}

struct Fixture {
    service: Arc<CicsService>,
    run: Run,
    replies: Replies,
    calls: Calls,
    deny: Arc<std::sync::atomic::AtomicBool>,
    actors: Arc<Mutex<Vec<Invocation>>>,
}

impl Fixture {
    fn new() -> Self {
        let replies = Arc::new(Mutex::new(VecDeque::new()));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let deny = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let actors = Arc::new(Mutex::new(Vec::new()));
        let limits = InvocationLimits::default();
        let providers = ["host.security.authorize", "host.dataset.read"]
            .into_iter()
            .map(|capability| {
                Arc::new(Authority {
                    descriptor: CapabilityDescriptor {
                        capability: CapabilityId::new(capability, limits).unwrap(),
                        provider_id: "first-reverse-guard".into(),
                        generation: "1".into(),
                        request_schema: "request@1".into(),
                        result_schema: "result@1".into(),
                        max_request_bytes: 1024 * 1024,
                        max_result_bytes: 1024 * 1024,
                        ready: true,
                    },
                    replies: replies.clone(),
                    calls: calls.clone(),
                    deny: deny.clone(),
                    actors: actors.clone(),
                }) as Arc<dyn HostProvider>
            })
            .collect();
        let host = Arc::new(ScopedHostService::new(
            Arc::new(RegistrySnapshot::new(1, providers, limits).unwrap()),
            HostLimits::default(),
        ));
        let service = CicsService::open(
            host,
            Arc::new(MemoryStore::new(Default::default())),
            CicsLimits::default(),
        )
        .unwrap();
        let actor = crate::service::tests::invocation();
        let run = handlers::task_control::new_run(actor, "session", "MENU", "MEAPPL", "MESYS");
        Self {
            service,
            run,
            replies,
            calls,
            deny,
            actors,
        }
    }
    fn seed(&mut self) {
        self.run.browses.insert("REVFILE".into(), "CURSOR-1".into());
        self.run
            .initial_browse_positions
            .insert("REVFILE".into(), ("CURSOR-1".into(), b"BB".to_vec()));
    }
    fn queue(&self, result: Result<DatasetResult, HostProblem>) {
        self.replies.lock().unwrap().push_back(result);
    }
    fn command(&mut self, request: CicsRequest) -> Result<CicsResponse, HostProblem> {
        self.service.invoke_run(&mut self.run, request, 1)
    }
    fn pending(&self, key: &[u8]) {
        assert_eq!(
            self.run.initial_browse_positions.get("REVFILE"),
            Some(&("CURSOR-1".into(), key.to_vec()))
        );
        assert_eq!(
            self.run.browses.get("REVFILE").map(String::as_str),
            Some("CURSOR-1")
        );
    }
    fn read_route(&self, operation: CicsOperation, position: bool) {
        let calls = self.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert!(
            matches!(&calls[0], DatasetRequest::Attributes { dataset } if dataset.as_str() == "REVFILE")
        );
        if position {
            assert_eq!(operation, CicsOperation::ReadPrev);
            assert!(
                matches!(&calls[1], DatasetRequest::ReadBrowsePosition { dataset, cursor, expected_key }
                if dataset.as_str() == "REVFILE" && cursor == "CURSOR-1" && expected_key == b"BB")
            );
        } else {
            assert!(
                matches!(&calls[1], DatasetRequest::ReadNext { dataset, cursor, reverse, .. }
                if dataset.as_str() == "REVFILE" && cursor == "CURSOR-1"
                    && *reverse == (operation == CicsOperation::ReadPrev))
            );
        }
    }
    fn clear_calls(&self) {
        self.calls.lock().unwrap().clear();
    }
}

#[test]
fn first_reverse_completed_readnext_record_consumes_seed() {
    let mut f = Fixture::new();
    f.seed();
    f.queue(Ok(full("CURSOR-1", b"BB02", b"BB", b"BB")));
    let response = f.command(request(CicsOperation::ReadNext, b"BB")).unwrap();
    assert_eq!(response.payload.bytes(), b"BB02");
    assert_eq!(response.outputs["RIDFLD"].bytes(), b"BB");
    assert_eq!(
        (response.condition.as_str(), response.response),
        ("NORMAL", 0)
    );
    assert!(f.run.initial_browse_positions.is_empty());
    f.read_route(CicsOperation::ReadNext, false);
    f.clear_calls();
    f.queue(Ok(full("CURSOR-1", b"AA01", b"AA", b"AA")));
    let response = f.command(request(CicsOperation::ReadPrev, b"BB")).unwrap();
    assert_eq!(response.payload.bytes(), b"AA01");
    f.read_route(CicsOperation::ReadPrev, false);
    assert!(matches!(
        f.calls.lock().unwrap().last(),
        Some(DatasetRequest::ReadNext { reverse: true, .. })
    ));
}

#[test]
fn first_reverse_ordinary_eof_consumption_follows_completed_condition_response() {
    for operation in [CicsOperation::ReadNext, CicsOperation::ReadPrev] {
        for policy in 0..5 {
            let mut f = Fixture::new();
            f.seed();
            f.queue(Ok(empty("CURSOR-1")));
            let mut req = request(operation, b"CC");
            req.condition_policy = match policy {
                0 => CicsConditionPolicy::Respond {
                    response_field: "RESP".into(),
                    response2_field: Some("RESP2".into()),
                },
                1 => CicsConditionPolicy::NoHandle,
                _ => CicsConditionPolicy::Default,
            };
            if policy == 2 {
                f.run.ignored_conditions.insert("ENDFILE".into());
            }
            if policy == 3 {
                f.run
                    .handlers
                    .insert("ENDFILE".into(), "EOF-HANDLER".into());
            }
            let result = f.command(req);
            f.read_route(operation, false);
            if policy == 4 {
                assert!(
                    matches!(result, Err(HostProblem::Condition { response: 20, ref name, .. }) if name == "ENDFILE")
                );
                f.pending(b"BB");
            } else {
                let response = result.unwrap();
                assert_eq!(
                    (response.condition.as_str(), response.response),
                    ("ENDFILE", 20)
                );
                if policy == 2 {
                    assert_eq!(response.disposition, CicsDisposition::Ignored);
                }
                if policy == 3 {
                    assert_eq!(response.target.as_deref(), Some("EOF-HANDLER"));
                }
                assert!(f.run.initial_browse_positions.is_empty());
            }
        }
    }
}

#[test]
fn first_reverse_completed_ineligible_readprev_consumes_seed() {
    for update in [false, true] {
        let mut f = Fixture::new();
        f.seed();
        f.queue(Ok(full("CURSOR-1", b"AA01", b"AA", b"AA")));
        let mut req = request(CicsOperation::ReadPrev, if update { b"BB" } else { b"CC" });
        if update {
            req.arguments.insert("OPTION.UPDATE".into(), literal(b""));
        }
        let response = f.command(req).unwrap();
        assert_eq!(response.payload.bytes(), b"AA01");
        f.read_route(CicsOperation::ReadPrev, false);
        assert!(f.run.initial_browse_positions.is_empty());
        f.clear_calls();
        f.queue(Ok(full("CURSOR-1", b"AA01", b"AA", b"AA")));
        f.command(request(CicsOperation::ReadPrev, b"BB")).unwrap();
        f.read_route(CicsOperation::ReadPrev, false);
    }
    // Explicit addressing contexts retain the old path, including explicit zero REQID.
    for name in [
        "REQID",
        "CURSOR.REQID",
        "SYSID",
        "OPTION.RBA",
        "OPTION.RRN",
        "OPTION.XRBA",
    ] {
        let mut f = Fixture::new();
        f.seed();
        let mut req = request(CicsOperation::ReadPrev, b"BB");
        req.arguments.insert(name.into(), literal(b"0"));
        assert!(
            selected_request(
                &f.run,
                &req,
                &DatasetName::new("REVFILE", 128).unwrap(),
                "CURSOR-1",
                Some(&attributes()),
                None
            )
            .unwrap()
            .is_none()
        );
    }
}

#[test]
fn first_reverse_selected_reply_requires_owned_cursor_seed_and_complete_tuple() {
    let cases = [
        (full("CURSOR-1", b"BB02", b"BB", b"BB"), None),
        (
            full("CURSOR-2", b"BB02", b"BB", b"BB"),
            Some(HostProblem::ProviderFailure),
        ),
        (
            full("CURSOR-1", b"BB02", b"CC", b"BB"),
            Some(HostProblem::ProviderFailure),
        ),
        (empty("CURSOR-1"), Some(HostProblem::ProviderFailure)),
        (
            DatasetResult::Browse {
                cursor: "CURSOR-1".into(),
                record: Some(b"BB02".to_vec()),
                identity: None,
                key: Some(b"BB".to_vec()),
            },
            Some(HostProblem::Malformed),
        ),
        (
            full("", b"BB02", b"BB", b"BB"),
            Some(HostProblem::ProviderFailure),
        ),
        (
            full(
                &"X".repeat(HostLimits::default().max_name_bytes + 1),
                b"BB02",
                b"BB",
                b"BB",
            ),
            Some(HostProblem::ProviderFailure),
        ),
    ];
    for (reply, expected) in cases {
        let mut f = Fixture::new();
        f.seed();
        f.queue(Ok(reply));
        let response = f.command(request(CicsOperation::ReadPrev, b"BB"));
        f.read_route(CicsOperation::ReadPrev, true);
        match expected {
            None => {
                let response = response.unwrap();
                assert_eq!(response.payload.bytes(), b"BB02");
                assert_eq!(response.outputs["RIDFLD"].bytes(), b"BB");
                assert!(f.run.initial_browse_positions.is_empty());
            }
            Some(HostProblem::Malformed) => {
                assert_eq!(response.unwrap().condition, "ERROR");
                f.pending(b"BB");
            }
            Some(expected) => {
                assert_eq!(response.err(), Some(expected));
                f.pending(b"BB");
            }
        }
    }
}

#[test]
fn first_reverse_refusal_or_response_construction_failure_keeps_seed() {
    for problem in [
        HostProblem::Condition {
            name: "NOTFND".into(),
            response: 13,
            response2: 0,
        },
        HostProblem::UnknownOutcome,
        HostProblem::ProviderFailure,
    ] {
        let mut f = Fixture::new();
        f.seed();
        f.queue(Err(problem.clone()));
        let response = f.command(request(CicsOperation::ReadPrev, b"BB"));
        if matches!(problem, HostProblem::Condition { .. }) {
            assert_eq!(response.unwrap().response, 13);
        } else {
            assert_eq!(response.err(), Some(problem));
        }
        f.pending(b"BB");
        f.read_route(CicsOperation::ReadPrev, true);
    }
    let mut f = Fixture::new();
    f.seed();
    f.run.current_program.effect_invocation = f
        .run
        .current_program
        .effect_invocation
        .clone()
        .with_cancellation_probe({
            let probe = mainframe_env_execution_api::CancellationProbe::new();
            probe.request();
            probe
        });
    assert_eq!(
        f.command(request(CicsOperation::ReadPrev, b"BB")).err(),
        Some(HostProblem::Cancelled)
    );
    assert!(f.calls.lock().unwrap().is_empty());
    f.pending(b"BB");
    // Production-local construction boundary: a retrieved, bound tuple is followed by an
    // explicitly controlled output refusal; no candidate exists merely because routing handles it.
    let mut f = Fixture::new();
    f.seed();
    let req = request(CicsOperation::ReadPrev, b"BB");
    let host = DatasetRequest::ReadBrowsePosition {
        dataset: DatasetName::new("REVFILE", 128).unwrap(),
        cursor: "CURSOR-1".into(),
        expected_key: b"BB".to_vec(),
    };
    let plan = BrowsePlan::new(&req, &host, "REVFILE");
    plan.validate(
        "CURSOR-1",
        &Some(b"BB02".to_vec()),
        &Some(b"BB".to_vec()),
        &Some(b"BB".to_vec()),
    )
    .unwrap();
    let mut candidate = Completion::Keep;
    let failed = finish_response(
        Err(HostProblem::ResourceExhausted),
        &plan,
        Some("CURSOR-1"),
        &mut candidate,
    );
    assert_eq!(failed.err(), Some(HostProblem::ResourceExhausted));
    let handled =
        handlers::condition_for_request(&f.service, &f.run, &req, HostProblem::ResourceExhausted);
    assert_eq!(handled.as_ref().unwrap().condition, "ERROR");
    candidate.commit(&mut f.run, &handled);
    f.pending(b"BB");
}

#[test]
fn first_reverse_completed_lengerr_preserves_length_and_consumes_seed() {
    let mut f = Fixture::new();
    f.seed();
    f.queue(Ok(full("CURSOR-1", b"BB02", b"BB", b"BB")));
    let mut req = request(CicsOperation::ReadPrev, b"BB");
    req.arguments
        .insert("LENGTH".into(), crate::service::decimal_payload(2).unwrap());
    let response = f.command(req).unwrap();
    assert_eq!(response.payload.bytes(), b"BB");
    assert_eq!(response.outputs["RIDFLD"].bytes(), b"BB");
    assert_eq!(response.outputs["LENGTH"].bytes(), b"4");
    assert_eq!(
        (response.condition.as_str(), response.response),
        ("LENGERR", 22)
    );
    assert!(f.run.initial_browse_positions.is_empty());
    f.read_route(CicsOperation::ReadPrev, true);
}

#[test]
fn first_reverse_validated_reset_replaces_seed_and_replay_does_not_redispatch() {
    let mut f = Fixture::new();
    f.seed();
    f.queue(Ok(empty("CURSOR-1")));
    let mut reset = request(CicsOperation::ResetBrowse, b"CC");
    reset.mutation = Some(Mutation {
        sequence: 2,
        idempotency_key: IdempotencyKey::new("reset-2", InvocationLimits::default()).unwrap(),
        transaction: Some("MENU".into()),
    });
    f.command(reset.clone()).unwrap();
    f.pending(b"CC");
    assert_eq!(f.calls.lock().unwrap().len(), 1);
    for reply in [
        Err(HostProblem::Condition {
            name: "NOTFND".into(),
            response: 13,
            response2: 0,
        }),
        Ok(empty("CURSOR-2")),
        Ok(full("CURSOR-1", b"BB02", b"BB", b"BB")),
    ] {
        f.clear_calls();
        let mut attempted = reset.clone();
        if reply.is_err() {
            attempted.arguments.insert("RIDFLD".into(), literal(b"AB"));
        }
        f.queue(reply);
        match f.command(attempted) {
            Ok(response) => assert_eq!(response.response, 13),
            Err(problem) => assert_eq!(problem, HostProblem::ProviderFailure),
        }
        f.pending(b"CC");
        assert_eq!(f.calls.lock().unwrap().len(), 1);
    }
    // Existing outer replay, with an actual public CICS entry and one real controlled RESET delegate.
    let replay = Fixture::new();
    let actor = replay.run.invocation.clone();
    let session = SessionId::new("session", 64).unwrap();
    replay.service.create_session(&session, 24, 80).unwrap();
    replay
        .service
        .register_run(actor.clone(), &session, "MENU", "MEAPPL", "MESYS")
        .unwrap();
    replay.queue(Ok(empty("CURSOR-1")));
    let start = request(CicsOperation::StartBrowse, b"BB");
    let effect = |req: CicsRequest, sequence| EffectRequest {
        run_unit: actor.run_unit_id.clone(),
        sequence,
        deadline_tick: actor.deadline_tick,
        idempotency_key: req
            .mutation
            .as_ref()
            .map(|mutation| mutation.idempotency_key.clone()),
        request: HostRequest::Cics(req),
    };
    replay
        .service
        .invoke(&effect(start.clone(), 1), start)
        .unwrap();
    replay.clear_calls();
    replay.queue(Ok(empty("CURSOR-1")));
    let first = replay
        .service
        .invoke(&effect(reset.clone(), 2), reset.clone())
        .unwrap();
    let second = replay
        .service
        .invoke(&effect(reset.clone(), 2), reset.clone())
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(replay.calls.lock().unwrap().len(), 1);
    let state = replay.service.lock().unwrap();
    let run = state.runs.get(&actor.run_unit_id).unwrap();
    assert_eq!(
        run.initial_browse_positions.get("REVFILE"),
        Some(&("CURSOR-1".into(), b"CC".to_vec()))
    );
    assert!(run.initial_browse_positions.len() <= run.browses.len());
    drop(state);
    // End the known owned public browse once; this is explicit teardown, not task-drop proof.
    replay.queue(Ok(empty("CURSOR-1")));
    let end = request(CicsOperation::EndBrowse, b"");
    replay.service.invoke(&effect(end.clone(), 3), end).unwrap();
    for (tuple, shared_malformed) in [
        (empty(""), false),
        (full("CURSOR-2", b"BB02", b"BB", b"BB"), false),
        (
            empty(&"X".repeat(HostLimits::default().max_name_bytes + 1)),
            false,
        ),
        (
            DatasetResult::Browse {
                cursor: "CURSOR-2".into(),
                record: Some(b"BB02".to_vec()),
                identity: None,
                key: None,
            },
            true,
        ),
    ] {
        let mut f = Fixture::new();
        f.seed();
        f.queue(Ok(tuple));
        let result = f.command(request(CicsOperation::StartBrowse, b"CC"));
        if shared_malformed {
            let response = result.unwrap();
            assert_eq!(
                (response.condition.as_str(), response.response),
                ("ERROR", 1)
            );
        } else {
            assert_eq!(result.err(), Some(HostProblem::ProviderFailure));
        }
        f.pending(b"BB");
        let calls = f.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert!(matches!(&calls[0], DatasetRequest::StartBrowse { key, .. } if key == b"CC"));
        assert!(f.replies.lock().unwrap().is_empty());
    }
    for name in ["OPTION.GENERIC", "REQID", "SYSID", "OPTION.RBA"] {
        let mut req = request(CicsOperation::StartBrowse, b"BB");
        req.arguments.insert(name.into(), literal(b"0"));
        assert!(provisional_key(&req, b"BB").is_none());
    }
    assert!(provisional_key(&request(CicsOperation::StartBrowse, b"BB"), &[255, 255]).is_none());
    assert!(
        provisional_key(
            &request(CicsOperation::StartBrowse, b"BB"),
            &vec![b'B'; HostLimits::default().max_record_bytes + 1]
        )
        .is_none()
    );
    // Same known replacement operation used by production, followed by controlled construction refusal.
    let mut f = Fixture::new();
    f.seed();
    record_start_owner(&mut f.run, "REVFILE", "CURSOR-2");
    let plan = BrowsePlan::new(
        &request(CicsOperation::StartBrowse, b"CC"),
        &DatasetRequest::StartBrowse {
            dataset: DatasetName::new("REVFILE", 128).unwrap(),
            key: b"CC".to_vec(),
            relation: mainframe_env_host_api::KeyRelation::GreaterOrEqual,
        },
        "REVFILE",
    );
    let mut candidate = Completion::Keep;
    assert_eq!(
        finish_response(
            Err(HostProblem::ResourceExhausted),
            &plan,
            Some("CURSOR-2"),
            &mut candidate
        )
        .err(),
        Some(HostProblem::ResourceExhausted)
    );
    assert!(f.run.initial_browse_positions.is_empty());
    assert_eq!(
        f.run.browses.get("REVFILE").map(String::as_str),
        Some("CURSOR-2")
    );
}

#[test]
fn first_reverse_validated_end_clears_only_matching_seed() {
    for reply in [
        Ok(empty("CURSOR-2")),
        Ok(full("CURSOR-1", b"BB02", b"BB", b"BB")),
        Err(HostProblem::UnknownOutcome),
        Err(HostProblem::ProviderFailure),
    ] {
        let mut f = Fixture::new();
        f.seed();
        f.queue(reply);
        assert!(f.command(request(CicsOperation::EndBrowse, b"")).is_err());
        f.pending(b"BB");
        assert_eq!(f.calls.lock().unwrap().len(), 1);
    }
    let mut f = Fixture::new();
    f.seed();
    f.queue(Ok(empty("CURSOR-1")));
    f.command(request(CicsOperation::EndBrowse, b"")).unwrap();
    assert!(f.run.browses.is_empty());
    assert!(f.run.initial_browse_positions.is_empty());
    assert_eq!(f.calls.lock().unwrap().len(), 1);
    f.clear_calls();
    f.queue(Ok(empty("CURSOR-2")));
    f.command(request(CicsOperation::StartBrowse, b"CC"))
        .unwrap();
    assert_eq!(
        f.run.initial_browse_positions.get("REVFILE"),
        Some(&("CURSOR-2".into(), b"CC".to_vec()))
    );
    assert_eq!(f.calls.lock().unwrap().len(), 1);
    // Known retirement remains true if subsequent local response construction refuses.
    let mut f = Fixture::new();
    f.seed();
    record_end_owner(&mut f.run, "REVFILE");
    let plan = BrowsePlan::new(
        &request(CicsOperation::EndBrowse, b""),
        &DatasetRequest::EndBrowse {
            dataset: DatasetName::new("REVFILE", 128).unwrap(),
            cursor: "CURSOR-1".into(),
        },
        "REVFILE",
    );
    let mut candidate = Completion::Keep;
    assert_eq!(
        finish_response(
            Err(HostProblem::ResourceExhausted),
            &plan,
            Some("CURSOR-1"),
            &mut candidate
        )
        .err(),
        Some(HostProblem::ResourceExhausted)
    );
    assert!(f.run.browses.is_empty());
    assert!(f.run.initial_browse_positions.is_empty());
}
