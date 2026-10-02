use super::*;
use mainframe_env_host_api::mq_status::MqCompletion;

#[path = "property/capacity.rs"]
mod capacity;
#[path = "property/failures.rs"]
mod failures;
#[path = "property/restart.rs"]
mod restart_properties;

#[derive(Default)]
struct Policy {
    mode: std::sync::atomic::AtomicU64,
    observed: Mutex<Vec<(PrincipalId, EnterpriseResource)>>,
    hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl EnterpriseAuthorizer for Policy {
    fn authorize(
        &self,
        principal: &PrincipalId,
        resource: &EnterpriseResource,
    ) -> Result<(), HostProblem> {
        self.observed
            .lock()
            .unwrap()
            .push((principal.clone(), resource.clone()));
        if let Some(hook) = self.hook.lock().unwrap().take() {
            hook();
        }
        if principal.as_str() != "TEST"
            || resource.class != EnterpriseResourceClass::MqUnitOfWork
            || resource.name.as_str() != "CURRENT"
            || !matches!(
                resource.intent,
                AccessIntent::Execute | AccessIntent::Read | AccessIntent::Update
            )
        {
            return Err(HostProblem::Unauthorized);
        }
        match self.mode.load(Ordering::SeqCst) {
            0 => Ok(()),
            1 => Err(HostProblem::Unauthorized),
            _ => Err(HostProblem::InfrastructureFailure),
        }
    }
}
fn install_policy(f: &mut Fixture) -> Arc<Policy> {
    let policy = Arc::new(Policy::default());
    Arc::get_mut(&mut f.service)
        .expect("exclusive test service")
        .authorizer = Some(policy.clone());
    policy
}
fn opts(call: MqMqiCall) -> MqPropertyOptions {
    MqPropertyOptions::checked(call, 1, 0).unwrap()
}
fn named(name: &str) -> MqPropertyName {
    MqPropertyName::checked(name.into(), MqMessageLimits::default()).unwrap()
}
fn create(c: MqHconn) -> MqMqiRequest {
    MqMqiRequest::Property(MqPropertyRequest::Create {
        connection: c,
        options: opts(MqMqiCall::CreateMessageHandle),
    })
}
fn set(c: MqHconn, h: MqHmsg, name: &str, kind: MqPropertyType, bytes: Vec<u8>) -> MqMqiRequest {
    MqMqiRequest::Property(MqPropertyRequest::Set {
        connection: c,
        handle: h,
        options: opts(MqMqiCall::SetProperty),
        name: named(name),
        descriptor: MqPropertyDescriptor::source_default(),
        value: MqPropertyData {
            kind,
            encoding: 785,
            ccsid: 1208,
            bytes,
        },
    })
}
fn inquire(c: MqHconn, h: MqHmsg, name: &str, n: usize, v: usize) -> MqMqiRequest {
    MqMqiRequest::Property(MqPropertyRequest::Inquire {
        connection: c,
        handle: h,
        options: opts(MqMqiCall::InquireProperty),
        name: named(name),
        requested_type: 0,
        name_capacity: n,
        value_capacity: v,
    })
}
fn delete(c: MqHconn, h: MqHmsg, name: &str) -> MqMqiRequest {
    MqMqiRequest::Property(MqPropertyRequest::Delete {
        connection: c,
        handle: h,
        options: opts(MqMqiCall::DeleteProperty),
        name: named(name),
    })
}
fn retire(c: MqHconn, h: MqHmsg) -> MqMqiRequest {
    MqMqiRequest::Property(MqPropertyRequest::DeleteHandle {
        connection: c,
        handle: h,
        options: opts(MqMqiCall::DeleteMessageHandle),
    })
}
fn hmsg(reply: EffectResult) -> MqHmsg {
    let MqMqiOutput::MessageHandle(v) = output(reply) else {
        panic!("created actual handle")
    };
    assert!(!v.is_historical());
    v
}
fn value(reply: EffectResult) -> MqPropertyInquiryObservation {
    let MqMqiOutput::PropertyObservation(MqPropertyObservation::Inquired(v)) = output(reply) else {
        panic!("defined inquiry")
    };
    v
}
fn pair(reply: &EffectResult) -> (MqCompletion, i32) {
    let Ok(HostResult::MqMqi(v)) = &reply.outcome else {
        panic!("typed actual result")
    };
    let MqMqiOutcome::ReviewedOutput { status, .. } = &v.result.outcome else {
        panic!("reviewed actual output")
    };
    (status.completion(), status.reason_decimal())
}
fn snapshot(f: &Fixture) -> String {
    let mut guard = f.service.lock_selected().unwrap();
    let rich_state::StoredAuthority::Rich(state) = &mut *guard else {
        panic!()
    };
    let access = state
        .runtime
        .as_mut()
        .unwrap()
        .handles
        .message_handles_mut();
    format!("{:?}", &*access)
}
fn peek(f: &Fixture, request: MqMqiRequest) -> MqMqiOutput {
    let mut guard = f.service.lock_selected().unwrap();
    let rich_state::StoredAuthority::Rich(state) = &mut *guard else {
        panic!()
    };
    let MqMqiRequest::Property(request) = request else {
        panic!()
    };
    let mut access = state
        .runtime
        .as_mut()
        .unwrap()
        .handles
        .message_handles_mut();
    let stage = access
        .stage_property(f.owner, &request, MqMqiLimits::default())
        .unwrap();
    stage.output.clone()
}

#[test]
fn memory_owned_sqlite_original_five_call_flow_uses_one_registry_publication_and_replay() {
    for sqlite in [false, true] {
        let db = super::restart::Database::new();
        let mut f = if sqlite {
            Fixture::from_store(db.open())
        } else {
            Fixture::new(false)
        };
        let policy = install_policy(&mut f);
        let c = f.connect();
        let unit = f.unit();
        let before_queue = f
            .store
            .get_provider_state(CATALOG_NAMESPACE, CATALOG_KEY)
            .unwrap();
        let before_delivery = f
            .rows()
            .into_iter()
            .filter(|r| r.namespace == "mq-delivery-live-v1-queue")
            .collect::<Vec<_>>();
        let h = hmsg(f.call(2, create(c)));
        let binary = vec![0, 255, 128, 1];
        let set_reply = f.call(
            3,
            set(
                c,
                h,
                "invoice.id",
                MqPropertyType::ByteString,
                binary.clone(),
            ),
        );
        assert_eq!(pair(&set_reply), (MqCompletion::Ok, 0));
        assert_eq!(
            output(set_reply),
            MqMqiOutput::PropertyObservation(MqPropertyObservation::Set(
                MqPropertyDescriptor::source_default()
            ))
        );
        let observation = value(f.call(4, inquire(c, h, "invoice.id", 64, 64)));
        assert_eq!(
            observation,
            MqPropertyInquiryObservation {
                descriptor: MqPropertyDescriptor::source_default(),
                kind: MqPropertyType::ByteString,
                returned_encoding: 785,
                returned_ccsid: None,
                returned_name: b"invoice.id".to_vec(),
                name_length: 10,
                name_ccsid: 1208,
                data_length: 4,
                copied_value: binary
            }
        );
        let deleted = f.call(5, delete(c, h, "invoice.id"));
        assert_eq!(pair(&deleted), (MqCompletion::Ok, 0));
        assert_eq!(
            output(deleted),
            MqMqiOutput::PropertyObservation(MqPropertyObservation::PropertyDeleted)
        );
        let retired = f.call(6, retire(c, h));
        assert_eq!(pair(&retired), (MqCompletion::Ok, 0));
        assert_eq!(
            output(retired),
            MqMqiOutput::PropertyObservation(MqPropertyObservation::HandleDeleted)
        );
        assert_eq!(f.unit(), unit);
        let after_queue = f
            .store
            .get_provider_state(CATALOG_NAMESPACE, CATALOG_KEY)
            .unwrap()
            .unwrap();
        let before_queue = before_queue.unwrap();
        assert_eq!(after_queue.payload, before_queue.payload);
        assert_eq!(after_queue.version, before_queue.version + 5);
        assert_eq!(
            f.rows()
                .into_iter()
                .filter(|r| r.namespace == "mq-delivery-live-v1-queue")
                .collect::<Vec<_>>(),
            before_delivery
        );
        let rows = f.rows();
        let e = f.effect(7, inquire(c, h, "invoice.id", 64, 64));
        f.seed(&e);
        let n = policy.observed.lock().unwrap().len();
        assert!(f.execute(&e).is_err());
        assert_eq!(policy.observed.lock().unwrap().len(), n);
        assert_eq!(f.rows(), rows);
        let audits = f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap();
        assert_eq!(audits.len(), 6);
        assert!(audits.iter().all(|a| a.principal == *f.inv.principal.id()
            && a.attempt == 1
            && a.invocation_key == f.inv.idempotency_key));
        for sequence in 1..=6 {
            assert_eq!(
                f.store
                    .effect(
                        &IdempotencyKey::new(
                            format!("effect-{sequence}"),
                            InvocationLimits::default()
                        )
                        .unwrap()
                    )
                    .unwrap()
                    .unwrap()
                    .state,
                EffectState::Intent
            );
        }
        assert!(
            policy
                .observed
                .lock()
                .unwrap()
                .iter()
                .any(|(_, r)| r.intent == AccessIntent::Read)
        );
        assert!(
            policy
                .observed
                .lock()
                .unwrap()
                .iter()
                .any(|(_, r)| r.intent == AccessIntent::Update)
        );
    }
}

#[test]
fn memory_owned_sqlite_partial_outputs_preserve_required_lengths_prefixes_status_and_undefined_ccsid()
 {
    for sqlite in [false, true] {
        let db = super::restart::Database::new();
        let f = if sqlite {
            Fixture::from_store(db.open())
        } else {
            Fixture::new(false)
        };
        let c = f.connect();
        let h = hmsg(f.call(2, create(c)));
        f.call(
            3,
            set(
                c,
                h,
                "a.value",
                MqPropertyType::Int32,
                (-123i32).to_be_bytes().to_vec(),
            ),
        );
        let short = f.call(4, inquire(c, h, "a.value", 7, 2));
        assert_eq!(pair(&short), (MqCompletion::Failed, 2469));
        let v = value(short);
        assert_eq!(v.data_length, 4);
        assert_eq!(v.copied_value, vec![255, 255]);
        assert_eq!(v.returned_ccsid, None);
        assert_eq!(v.name_length, 7);
        let short = f.call(5, inquire(c, h, "a.value", 2, 4));
        assert_eq!(pair(&short), (MqCompletion::Failed, 2465));
        let v = value(short);
        assert_eq!(v.returned_name, b"a.");
        assert_eq!(v.name_length, 7);
        assert_eq!(v.copied_value, (-123i32).to_be_bytes());
        f.call(
            6,
            set(c, h, "a.text", MqPropertyType::String, b"x\0  ".to_vec()),
        );
        let v = value(f.call(7, inquire(c, h, "a.text", 64, 64)));
        assert_eq!(v.returned_ccsid, Some(1208));
        assert_eq!(v.copied_value, b"x\0  ");
        let rows = f.rows();
        let state = snapshot(&f);
        let e = f.effect(8, inquire(c, h, "a.value", 0, 0));
        f.seed(&e);
        assert_eq!(f.execute(&e), Err(HostProblem::Unsupported));
        assert_eq!(f.rows(), rows);
        assert_eq!(snapshot(&f), state);
        let no = f.call(9, inquire(c, h, "absent", 64, 64));
        assert_eq!(pair(&no), (MqCompletion::Failed, 2471));
        let no = f.call(10, delete(c, h, "absent"));
        assert_eq!(pair(&no), (MqCompletion::Warning, 2471));
    }
}

#[test]
fn associated_complete_descriptor_initializers_field_replacement_and_reset_are_exact() {
    let f = Fixture::new(false);
    let c = f.connect();
    let h = hmsg(f.call(2, create(c)));
    let expected: [(&str, i32); 10] = [
        ("Report", 0),
        ("MsgType", 8),
        ("Expiry", -1),
        ("Feedback", 0),
        ("Encoding", 785),
        ("CodedCharSetId", 0),
        ("Priority", -1),
        ("Persistence", 2),
        ("BackoutCount", 0),
        ("PutApplType", 0),
    ];
    let mut sequence = 3;
    for (field, default) in expected {
        let name = format!("Root.MQMD.{field}");
        let v = value(f.call(sequence, inquire(c, h, &name, 128, 128)));
        sequence += 1;
        assert_eq!(v.kind, MqPropertyType::Int32);
        assert_eq!(v.copied_value, default.to_be_bytes());
        assert_eq!(v.descriptor.copy_options, 0);
    }
    for (field, width, byte, kind) in [
        ("MsgId", 24, 0, MqPropertyType::ByteString),
        ("AccountingToken", 32, 0, MqPropertyType::ByteString),
        ("ReplyToQ", 48, b' ', MqPropertyType::String),
        ("Format", 8, b' ', MqPropertyType::String),
    ] {
        let name = format!("Root.MQMD.{field}");
        let v = value(f.call(sequence, inquire(c, h, &name, 128, 128)));
        sequence += 1;
        assert_eq!(v.copied_value, vec![byte; width]);
        assert_eq!(v.kind, kind);
        let bytes = if kind == MqPropertyType::String {
            vec![b'A'; width]
        } else {
            (0..width).map(|i| (i * 137) as u8).collect()
        };
        let reply = f.call(sequence, set(c, h, &name, kind, bytes.clone()));
        sequence += 1;
        let MqMqiOutput::PropertyObservation(MqPropertyObservation::Set(pd)) = output(reply) else {
            panic!()
        };
        assert_eq!(pd.copy_options, 0);
        assert_eq!(
            value(f.call(sequence, inquire(c, h, &name, 128, 128))).copied_value,
            bytes
        );
        sequence += 1;
        f.call(sequence, delete(c, h, &name));
        sequence += 1;
        assert_eq!(
            value(f.call(sequence, inquire(c, h, &name, 128, 128))).copied_value,
            vec![byte; width]
        );
        sequence += 1;
    }
}

#[test]
fn one_original_concurrent_create_publishes_and_allocates_once() {
    let f = Arc::new(Fixture::new(false));
    let c = f.connect();
    let e = f.effect(2, create(c));
    f.seed(&e);
    let replies = std::thread::scope(|s| {
        let a = s.spawn(|| f.execute(&e));
        let b = s.spawn(|| f.execute(&e));
        (a.join().unwrap().unwrap(), b.join().unwrap().unwrap())
    });
    assert_eq!(replies.0, replies.1);
    hmsg(replies.0);
    let mut guard = f.service.lock_selected().unwrap();
    let rich_state::StoredAuthority::Rich(state) = &mut *guard else {
        panic!()
    };
    assert_eq!(
        state
            .runtime
            .as_mut()
            .unwrap()
            .handles
            .handles_mut()
            .active_handles(),
        2
    );
    assert_eq!(
        f.store
            .audit_records(&f.inv.execution_id, 0, 128)
            .unwrap()
            .len(),
        2
    );
}
