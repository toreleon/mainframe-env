use super::*;
use crate::service::{CicsLimits, handlers};
use mainframe_env_execution_api::{
    BoundedPayload, CancellationProbe, CapabilityId, ExecutionId, Invocation, InvocationLimits,
    Principal,
};
use mainframe_env_host_api::{
    CapabilityDescriptor, CicsConditionPolicy, EffectRequest, EffectResult, HostLimits,
    HostProvider, Mutation, RegistrySnapshot, ScopedHostService, SecurityDecision, SessionId,
};
use mainframe_env_store::MemoryStore;
use std::collections::VecDeque;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

type Replies = Arc<Mutex<VecDeque<Result<DatasetResult, HostProblem>>>>;
type Calls = Arc<Mutex<Vec<(Invocation, EffectRequest)>>>;

struct Authority {
    descriptor: CapabilityDescriptor,
    replies: Replies,
    calls: Calls,
    deny: Arc<AtomicBool>,
}

impl HostProvider for Authority {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }

    fn invoke(&self, actor: &Invocation, effect: EffectRequest) -> EffectResult {
        self.calls
            .lock()
            .unwrap()
            .push((actor.clone(), effect.clone()));
        let outcome = match &effect.request {
            HostRequest::Security(_) => {
                Ok(HostResult::Security(if self.deny.load(Ordering::SeqCst) {
                    SecurityDecision::Deny
                } else {
                    SecurityDecision::Allow
                }))
            }
            HostRequest::Dataset(_) => self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("one controlled result per actual Dataset delegate")
                .map(HostResult::Dataset),
            _ => Err(HostProblem::Unsupported),
        };
        EffectResult {
            sequence: effect.sequence,
            outcome,
        }
    }
}

struct Fixture {
    service: Arc<CicsService>,
    run: Run,
    calls: Calls,
    replies: Replies,
    deny: Arc<AtomicBool>,
}

impl Fixture {
    fn new() -> Self {
        let limits = InvocationLimits::default();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let replies = Arc::new(Mutex::new(VecDeque::new()));
        let deny = Arc::new(AtomicBool::new(false));
        let providers = ["host.security.authorize", "host.dataset.read"]
            .into_iter()
            .map(|name| {
                Arc::new(Authority {
                    descriptor: CapabilityDescriptor {
                        capability: CapabilityId::new(name, limits).unwrap(),
                        provider_id: "task-retirement-guard".into(),
                        generation: "1".into(),
                        request_schema: "request@1".into(),
                        result_schema: "result@1".into(),
                        max_request_bytes: 1024 * 1024,
                        max_result_bytes: 1024 * 1024,
                        ready: true,
                    },
                    calls: calls.clone(),
                    replies: replies.clone(),
                    deny: deny.clone(),
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
        let run = handlers::new_run(actor, "retirement-session", "MENU", "ME01", "S001");
        Self {
            service,
            run,
            calls,
            replies,
            deny,
        }
    }

    fn queue(&self, reply: Result<DatasetResult, HostProblem>) {
        self.replies.lock().unwrap().push_back(reply);
    }

    fn start(&mut self, dataset: &str, cursor: &str) {
        self.queue(Ok(empty(cursor)));
        let request = CicsRequest {
            operation: CicsOperation::StartBrowse,
            arguments: BTreeMap::from([
                ("DATASET".into(), literal(dataset.as_bytes())),
                ("RIDFLD".into(), literal(b"BB")),
            ]),
            condition_policy: CicsConditionPolicy::NoHandle,
            mutation: None,
        };
        let response = self.service.invoke_run(&mut self.run, request, 1).unwrap();
        assert_eq!(response.condition, "NORMAL");
        assert_eq!(self.run.browses[dataset], cursor);
        assert!(
            self.run
                .file_updates
                .task_browses
                .contains_key(&(dataset.into(), cursor.into()))
        );
        self.calls.lock().unwrap().clear();
    }

    fn end_calls(&self) -> Vec<(Invocation, EffectRequest)> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, effect)| {
                matches!(
                    effect.request,
                    HostRequest::Dataset(DatasetRequest::EndBrowse { .. })
                )
            })
            .cloned()
            .collect()
    }

    fn end(&mut self, requested_cursor: Option<&str>) -> Result<CicsResponse, HostProblem> {
        let mut arguments = BTreeMap::from([("DATASET".into(), literal(b"DATA"))]);
        if let Some(cursor) = requested_cursor {
            arguments.insert("CURSOR".into(), literal(cursor.as_bytes()));
        }
        self.service.invoke_run(
            &mut self.run,
            CicsRequest {
                operation: CicsOperation::EndBrowse,
                arguments,
                condition_policy: CicsConditionPolicy::NoHandle,
                mutation: None,
            },
            1,
        )
    }

    fn attach(&self) -> SessionId {
        let session = SessionId::new(&self.run.session, 128).unwrap();
        self.service
            .launch_terminal(
                self.run.invocation.clone(),
                &session,
                "MENU",
                24,
                80,
                "retirement-csrf",
                1,
                1000,
            )
            .unwrap();
        self.service
            .lock()
            .unwrap()
            .runs
            .insert(self.run.invocation.run_unit_id.clone(), self.run.clone());
        self.calls.lock().unwrap().clear();
        session
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

fn empty(cursor: &str) -> DatasetResult {
    DatasetResult::Browse {
        cursor: cursor.into(),
        record: None,
        identity: None,
        key: None,
    }
}

fn io_error() -> HostProblem {
    HostProblem::Condition {
        name: "IOERR".into(),
        response: 17,
        response2: 120,
    }
}

#[test]
fn task_browse_retirement_uses_creating_actor_after_program_replacement() {
    let mut fixture = Fixture::new();
    fixture.start("DATA", "cursor-original");
    let original = fixture.run.current_program.effect_invocation.clone();
    let mut replacement = original.clone();
    replacement.execution_id =
        ExecutionId::new("replacement-program", InvocationLimits::default()).unwrap();
    replacement.deadline_tick -= 1;
    fixture.run.current_program.effect_invocation = replacement.clone();
    fixture.queue(Ok(empty("cursor-original")));
    release_task(&fixture.service, &mut fixture.run).unwrap();
    let calls = fixture.end_calls();
    assert_eq!(calls.len(), 1);
    let mut expected = original;
    expected.deadline_tick = replacement.deadline_tick;
    assert_eq!(calls[0].0, expected);
    assert_eq!(fixture.run.current_program.effect_invocation, replacement);
    assert!(fixture.run.file_updates.task_browses.is_empty());
    assert!(fixture.run.browses.is_empty());
    assert!(fixture.run.initial_browse_positions.is_empty());
}

#[test]
fn task_browse_retirement_keeps_replaced_cursors_until_confirmed_end() {
    let mut fixture = Fixture::new();
    fixture.start("DATA", "cursor-one");
    fixture.start("DATA", "cursor-two");
    assert_eq!(fixture.run.file_updates.task_browses.len(), 2);
    fixture.queue(Ok(empty("cursor-one")));
    fixture.queue(Ok(empty("cursor-two")));
    release_task(&fixture.service, &mut fixture.run).unwrap();
    let calls = fixture.end_calls();
    assert_eq!(calls.len(), 2);
    for (i, expected) in ["cursor-one", "cursor-two"].into_iter().enumerate() {
        assert!(
            matches!(&calls[i].1.request, HostRequest::Dataset(DatasetRequest::EndBrowse { dataset, cursor })
            if dataset.as_str() == "DATA" && cursor == expected)
        );
    }
    assert!(fixture.run.file_updates.task_browses.is_empty());
}

#[test]
fn task_browse_retirement_retains_terminal_progress_before_retrying_later_cursor() {
    let mut fixture = Fixture::new();
    fixture.start("AAAA", "cursor-a");
    fixture.start("BBBB", "cursor-b");
    let session = fixture.attach();
    fixture.queue(Ok(empty("cursor-a")));
    fixture.queue(Err(io_error()));
    let principal = fixture.run.invocation.principal.id();
    assert_eq!(
        fixture
            .service
            .complete_terminal_run(&session, principal, 2),
        Err(io_error())
    );
    let saved = fixture.service.lock().unwrap().runs[&fixture.run.invocation.run_unit_id].clone();
    assert!(!saved.browses.contains_key("AAAA"));
    assert_eq!(saved.browses["BBBB"], "cursor-b");
    assert_eq!(saved.file_updates.task_browses.len(), 1);
    assert!(saved.host_sequence > fixture.run.host_sequence);
    fixture.queue(Ok(empty("cursor-b")));
    fixture
        .service
        .complete_terminal_run(&session, principal, 3)
        .unwrap();
    let calls = fixture.end_calls();
    assert_eq!(calls.len(), 3);
    for (i, expected) in ["cursor-a", "cursor-b", "cursor-b"].into_iter().enumerate() {
        assert!(
            matches!(&calls[i].1.request, HostRequest::Dataset(DatasetRequest::EndBrowse { cursor, .. }) if cursor == expected)
        );
    }
    assert!(
        calls
            .windows(2)
            .all(|pair| pair[0].1.sequence < pair[1].1.sequence)
    );
    assert!(
        !fixture
            .service
            .lock()
            .unwrap()
            .runs
            .contains_key(&fixture.run.invocation.run_unit_id)
    );
}

#[test]
fn task_browse_retirement_unknown_terminal_result_holds_owner_without_redispatch() {
    let mut fixture = Fixture::new();
    fixture.start("DATA", "cursor");
    let session = fixture.attach();
    fixture.queue(Err(HostProblem::UnknownOutcome));
    for tick in [2, 3] {
        assert_eq!(
            fixture.service.abort_terminal_run(
                &session,
                fixture.run.invocation.principal.id(),
                tick
            ),
            Err(HostProblem::UnknownOutcome)
        );
    }
    assert_eq!(fixture.end_calls().len(), 1);
    let saved = fixture.service.lock().unwrap().runs[&fixture.run.invocation.run_unit_id].clone();
    assert_eq!(saved.browses["DATA"], "cursor");
    assert!(saved.file_updates.task_browses[&("DATA".into(), "cursor".into())].retirement_unknown);
}

#[test]
fn task_browse_retirement_unbound_results_cannot_become_confirmed_end() {
    for reply in [
        empty("different-cursor"),
        DatasetResult::Browse {
            cursor: "cursor".into(),
            record: Some(b"BB02".to_vec()),
            identity: Some(b"BB".to_vec()),
            key: Some(b"BB".to_vec()),
        },
    ] {
        let mut fixture = Fixture::new();
        fixture.start("DATA", "cursor");
        fixture.queue(Ok(reply));
        assert_eq!(
            release_task(&fixture.service, &mut fixture.run),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(
            release_task(&fixture.service, &mut fixture.run),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(fixture.end_calls().len(), 1);
        assert_eq!(fixture.run.browses["DATA"], "cursor");
        assert_eq!(fixture.run.file_updates.task_browses.len(), 1);
    }
}

#[test]
fn task_browse_retirement_does_not_redispatch_uncertain_ordinary_end() {
    for reply in [
        Err(HostProblem::UnknownOutcome),
        Err(HostProblem::ProviderFailure),
        Err(HostProblem::InfrastructureFailure),
        Err(HostProblem::Malformed),
        Ok(empty("wrong-cursor")),
        Ok(DatasetResult::Browse {
            cursor: "cursor".into(),
            record: Some(b"BB02".to_vec()),
            identity: Some(b"BB".to_vec()),
            key: Some(b"BB".to_vec()),
        }),
    ] {
        let mut fixture = Fixture::new();
        fixture.start("DATA", "cursor");
        fixture.queue(reply);
        assert!(fixture.end(None).is_err());
        assert_eq!(fixture.end_calls().len(), 1);
        assert_eq!(fixture.end(None), Err(HostProblem::UnknownOutcome));
        assert_eq!(
            release_task(&fixture.service, &mut fixture.run),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(fixture.end_calls().len(), 1);
        assert_eq!(fixture.run.browses["DATA"], "cursor");
    }
}

#[test]
fn task_browse_retirement_keeps_ordinary_end_pre_dispatch_refusals_retryable() {
    for generation in [true, false] {
        let mut fixture = Fixture::new();
        fixture.start("DATA", "cursor");
        if generation {
            fixture
                .run
                .current_program
                .effect_invocation
                .provider_generations
                .insert(
                    CapabilityId::new("host.dataset.read", InvocationLimits::default()).unwrap(),
                    "stale-generation".into(),
                );
            assert_eq!(fixture.end(None), Err(HostProblem::ProviderFailure));
            fixture
                .run
                .current_program
                .effect_invocation
                .provider_generations
                .clear();
        } else {
            let response = fixture.end(Some("not-owned")).unwrap();
            assert_eq!(
                (response.condition.as_str(), response.response),
                ("INVREQ", 16)
            );
        }
        assert!(fixture.end_calls().is_empty());
        assert!(
            !fixture.run.file_updates.task_browses[&("DATA".into(), "cursor".into())]
                .retirement_unknown
        );
        fixture.queue(Ok(empty("cursor")));
        fixture.end(None).unwrap();
        release_task(&fixture.service, &mut fixture.run).unwrap();
        assert_eq!(fixture.end_calls().len(), 1);
        assert!(fixture.run.browses.is_empty());
        assert!(fixture.run.file_updates.task_browses.is_empty());
    }
}

#[test]
fn task_browse_retirement_requires_known_owner_and_ordinary_saf_grant_generation_controls() {
    for case in 0..4 {
        let mut fixture = Fixture::new();
        fixture.start("DATA", "cursor");
        let expected = match case {
            0 => {
                fixture.run.file_updates.task_browses.clear();
                HostProblem::UnknownOutcome
            }
            1 => {
                fixture.deny.store(true, Ordering::SeqCst);
                HostProblem::Unauthorized
            }
            2 => {
                fixture.run.invocation.principal = Principal::new(
                    fixture.run.invocation.principal.id().clone(),
                    Default::default(),
                    InvocationLimits::default(),
                )
                .unwrap();
                HostProblem::Unauthorized
            }
            _ => {
                let capability =
                    CapabilityId::new("host.dataset.read", InvocationLimits::default()).unwrap();
                fixture
                    .run
                    .invocation
                    .provider_generations
                    .insert(capability.clone(), "different-generation".into());
                fixture
                    .run
                    .file_updates
                    .task_browses
                    .get_mut(&("DATA".into(), "cursor".into()))
                    .unwrap()
                    .actor
                    .provider_generations
                    .insert(capability, "different-generation".into());
                HostProblem::ProviderFailure
            }
        };
        let result = release_task(&fixture.service, &mut fixture.run);
        assert_eq!(result, Err(expected));
        assert!(fixture.end_calls().is_empty());
        assert_eq!(fixture.run.browses["DATA"], "cursor");
        if case != 0 {
            assert!(
                !fixture.run.file_updates.task_browses[&("DATA".into(), "cursor".into())]
                    .retirement_unknown
            );
        }
    }
}

#[test]
fn task_browse_retirement_preserves_live_cancellation_and_narrow_deadline() {
    for cancelled in [true, false] {
        let mut fixture = Fixture::new();
        let probe = CancellationProbe::new();
        fixture.run.invocation.cancellation_probe = Some(probe.clone());
        fixture
            .run
            .current_program
            .effect_invocation
            .cancellation_probe = Some(probe.clone());
        fixture.start("DATA", "cursor");
        let expected = if cancelled {
            probe.request();
            HostProblem::Cancelled
        } else {
            fixture.run.current_program.effect_invocation.deadline_tick = 0;
            HostProblem::TimedOut
        };
        assert_eq!(
            release_task(&fixture.service, &mut fixture.run),
            Err(expected)
        );
        assert!(fixture.end_calls().is_empty());
        assert_eq!(fixture.run.browses["DATA"], "cursor");
        assert_eq!(fixture.run.file_updates.task_browses.len(), 1);
    }
}

#[test]
fn task_browse_retirement_does_not_end_lower_return_or_handled_abend() {
    for handled_abend in [true, false] {
        let mut fixture = Fixture::new();
        fixture.start("DATA", "cursor");
        fixture.run.current_program.logical_level = 2;
        let (operation, arguments) = if handled_abend {
            fixture.run.abend_handler = Some(handlers::AbendExit::Label("RECOVER".into()));
            (
                CicsOperation::Abend,
                BTreeMap::from([("ABCODE".into(), literal(b"TEST"))]),
            )
        } else {
            (CicsOperation::Return, BTreeMap::new())
        };
        let request = CicsRequest {
            operation,
            arguments,
            condition_policy: CicsConditionPolicy::Default,
            mutation: Some(Mutation {
                sequence: 8,
                idempotency_key: mainframe_env_execution_api::IdempotencyKey::new(
                    "lower-control",
                    InvocationLimits::default(),
                )
                .unwrap(),
                transaction: Some("MENU".into()),
            }),
        };
        let response = fixture
            .service
            .invoke_run(&mut fixture.run, request, 1)
            .unwrap();
        assert_eq!(
            response.disposition,
            if handled_abend {
                CicsDisposition::Handler
            } else {
                CicsDisposition::Returned
            }
        );
        assert!(fixture.end_calls().is_empty());
        assert_eq!(fixture.run.browses["DATA"], "cursor");
        assert_eq!(fixture.run.file_updates.task_browses.len(), 1);
    }
}
