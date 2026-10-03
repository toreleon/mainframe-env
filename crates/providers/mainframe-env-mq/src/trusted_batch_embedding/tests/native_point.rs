//! Original selected physical fixtures. Not compiled/JES/LE/SAF certification.
use super::*;
use crate::{
    MqLocalQueueUsage, MqNativeAttributes, MqNativeCharacters, MqNativeDeliverySequence,
    MqNativeQueueAttributes, MqObjectCatalog, MqObjectDefinition, MqQueueManagerDefinition,
    MqTrustedBatchContextObservation, MqTrustedBatchGmtObservation, MqTrustedBatchProducerSource,
};
use mainframe_env_host_api::mq_md_value::*;
use mainframe_env_host_api::mq_wire_options::MqWireBindings;
use std::sync::Weak;

#[derive(Default)]
struct Source {
    gmt: AtomicU64,
    batch: AtomicU64,
    live: AtomicU64,
    fail: AtomicBool,
    panic: AtomicBool,
    reentry: Mutex<Option<Weak<MqService>>>,
    hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl MqTrustedBatchProducerSource for Source {
    fn encode_structure(
        &self,
        text: &str,
        chars: MqMdCharacterEncoding,
    ) -> Result<Vec<u8>, HostProblem> {
        if chars != MqMdCharacterEncoding::AsciiCompatible {
            return Err(HostProblem::Unsupported);
        }
        Ok(text.as_bytes().to_vec())
    }
    fn check_live(&self, _: &Invocation) -> Result<(), HostProblem> {
        self.live.fetch_add(1, Ordering::SeqCst);
        if self.panic.load(Ordering::SeqCst) {
            panic!("fixture source panic");
        }
        if self.fail.load(Ordering::SeqCst) {
            return Err(HostProblem::ProviderFailure);
        }
        if let Some(hook) = self.hook.lock().unwrap().take() {
            hook();
        }
        if let Some(service) = self
            .reentry
            .lock()
            .unwrap()
            .as_ref()
            .and_then(Weak::upgrade)
        {
            assert!(matches!(
                service.require_trusted_batch_rich(),
                Err(HostProblem::Unsupported)
            ));
        }
        Ok(())
    }
    fn physical_gmt(&self, _: &Invocation) -> Result<MqTrustedBatchGmtObservation, HostProblem> {
        self.gmt.fetch_add(1, Ordering::SeqCst);
        Err(HostProblem::Unsupported)
    }
    fn batch_context(
        &self,
        _: &Invocation,
    ) -> Result<MqTrustedBatchContextObservation, HostProblem> {
        self.batch.fetch_add(1, Ordering::SeqCst);
        Err(HostProblem::Unsupported)
    }
}
static NEXT: AtomicU64 = AtomicU64::new(1);
struct Database(std::path::PathBuf);
impl Database {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "mq-native-point-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
    fn open(&self) -> Arc<dyn PlatformStore> {
        Arc::new(
            SqliteStateStore::open(
                &format!(
                    "sqlite://{}?mode=rwc",
                    self.0.join("state.sqlite").display()
                ),
                64 << 20,
                256,
            )
            .unwrap(),
        )
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        for name in ["state.sqlite", "state.sqlite-wal", "state.sqlite-shm"] {
            let path = self.0.join(name);
            if path.exists() {
                std::fs::remove_file(path).unwrap();
            }
        }
        std::fs::remove_dir(&self.0).unwrap();
    }
}
struct NativeFixture {
    f: Fixture,
    source: Arc<Source>,
    _db: Option<Database>,
}
impl std::ops::Deref for NativeFixture {
    type Target = Fixture;
    fn deref(&self) -> &Fixture {
        &self.f
    }
}
impl NativeFixture {
    fn new(sqlite: bool, version: i32, cp: bool) -> Self {
        Self::with_clock(sqlite, version, cp, None)
    }
    fn with_clock(
        sqlite: bool,
        version: i32,
        cp: bool,
        selected_clock: Option<Arc<dyn MqReplayClock>>,
    ) -> Self {
        let db = sqlite.then(Database::new);
        let store = db.as_ref().map_or_else(|| backend(false), Database::open);
        let queue = MqObjectName::new("Q").unwrap();
        let catalog = MqObjectCatalog::new(
            MqQueueManagerDefinition {
                name: MqObjectName::new("QM").unwrap(),
                default_transmission_queue: None,
            },
            vec![MqObjectDefinition::LocalQueue {
                name: queue.clone(),
                usage: MqLocalQueueUsage::Normal,
                trigger_process: None,
            }],
            Default::default(),
        )
        .unwrap()
        .with_native_attributes(MqNativeAttributes {
            coded_char_set_id: if cp { 37 } else { 819 },
            characters: if cp {
                MqNativeCharacters::OwnedCp037
            } else {
                MqNativeCharacters::Ascii819
            },
            max_msg_length: 32768,
            max_priority: 9,
            queues: vec![MqNativeQueueAttributes {
                name: queue.clone(),
                max_msg_length: 2048,
                delivery_sequence: MqNativeDeliverySequence::Fifo,
            }],
        })
        .unwrap();
        let legacy = MqService::open(store.clone(), Default::default()).unwrap();
        legacy.install_object_catalog(catalog).unwrap();
        let plan = legacy
            .plan_legacy_delivery_import(3, 5, Default::default())
            .unwrap();
        store
            .mutate_provider_states_atomic(plan.into_parts().0)
            .unwrap();
        drop(legacy);
        let service = MqService::open_selected_mqi(
            store.clone(),
            Default::default(),
            3,
            5,
            Arc::new(Saf::default()),
            Arc::new(Clock(AtomicU64::new(20))),
        )
        .unwrap();
        service.native_test_profile(
            queue,
            version,
            if cp {
                MqMdCharacterEncoding::OwnedCp037
            } else {
                MqMdCharacterEncoding::AsciiCompatible
            },
        );
        drop(service);
        // Reuse only the bounded original Invocation fixture; its temporary
        // Memory service is dropped and supplies no authority to this store.
        let parent = Fixture::new(false).parent;
        seed_execution(&*store, &parent);
        let clock = Arc::new(Clock(AtomicU64::new(20)));
        let saf = Arc::new(Saf::default());
        let mut runtime = match selected_clock {
            Some(selected_clock) => MqTrustedBatchRuntime::open(
                store.clone(),
                Default::default(),
                3,
                5,
                saf.clone(),
                selected_clock,
                descriptor(),
                Default::default(),
                Default::default(),
            )
            .unwrap(),
            None => open(store.clone(), saf.clone(), clock.clone()).unwrap(),
        };
        let source = Arc::new(Source::default());
        runtime
            .configure_producer_source(&store, source.clone())
            .unwrap();
        Self {
            f: Fixture {
                store,
                runtime,
                parent,
                clock,
                saf,
            },
            source,
            _db: db,
        }
    }
}
fn object_target(object: MqHobj) -> MqTrustedBatchPointTarget {
    MqTrustedBatchPointTarget::Object(object)
}
fn open_target() -> MqTrustedBatchPointTarget {
    MqTrustedBatchPointTarget::Open {
        lookup: lookup(),
        access: MqRouteOpenAccess::Output,
    }
}

#[test]
fn native_root_child_structure_tuple_current_unit_facts_are_read_only() {
    for sqlite in [false, true] {
        for version in [1, 2] {
            for cp in [false, true] {
                let f = NativeFixture::new(sqlite, version, cp);
                let root = f.root();
                let parent = root.frame();
                let mut child = f.child(&parent, "child");
                let (c, o) = connected(&f, &mut child);
                let rows = f.rows();
                let current = unit(&child, c);
                let calls = f.saf.calls.load(Ordering::SeqCst);
                let audits = f
                    .store
                    .audit_records(&child.original().execution_id, 0, 128)
                    .unwrap();
                let structure = child.structure_profile(MqMqiCall::Open, c).unwrap();
                assert_eq!(structure.coded_char_set_id(), if cp { 37 } else { 819 });
                assert_eq!(
                    structure.characters(),
                    if cp {
                        MqMdCharacterEncoding::OwnedCp037
                    } else {
                        MqMdCharacterEncoding::AsciiCompatible
                    }
                );
                let point = child.point_profile(&structure, open_target()).unwrap();
                assert_eq!(point.descriptor_version(), version);
                assert_eq!(point.max_message_bytes(), 2048);
                let b = point.wire_bindings();
                assert!(b.queue_defaults_are_represented(c, None, Some(&lookup())));
                assert!(!b.queue_defaults_are_represented(MqHconn::Default, None, Some(&lookup())));
                assert!(!b.queue_defaults_are_represented(c, Some(o), None));
                let mut wrong = lookup();
                if let MqRouteLookup::Queue { name, .. } = &mut wrong {
                    *name = MqRouteName::new("OTHER").unwrap();
                }
                assert!(!b.queue_defaults_are_represented(c, None, Some(&wrong)));
                assert_eq!(
                    b.admitted_unit(c),
                    Some(MqMqiUnitOfWork::Local { unit: current })
                );
                assert_eq!(b.admitted_unit(MqHconn::Unassociated), None);
                assert_eq!(b.existing_cursor(c, o), None);
                assert_eq!(b.milliseconds_to_ticks(1), None);
                child.recheck_structure_profile(&structure).unwrap();
                child.recheck_point_profile(&point).unwrap();
                let put = child.structure_profile(MqMqiCall::Put, c).unwrap();
                let p = child.point_profile(&put, object_target(o)).unwrap();
                assert!(
                    p.wire_bindings()
                        .queue_defaults_are_represented(c, Some(o), None)
                );
                assert!(child.point_profile(&put, open_target()).is_err());
                let one = child.structure_profile(MqMqiCall::PutOne, c).unwrap();
                let p = child
                    .point_profile(&one, MqTrustedBatchPointTarget::PutOne { lookup: lookup() })
                    .unwrap();
                assert!(
                    p.wire_bindings()
                        .queue_defaults_are_represented(c, None, Some(&lookup()))
                );
                assert_eq!(f.rows(), rows);
                assert_eq!(unit(&child, c), current);
                assert_eq!(f.saf.calls.load(Ordering::SeqCst), calls);
                assert_eq!(
                    f.store
                        .audit_records(&child.original().execution_id, 0, 128)
                        .unwrap(),
                    audits
                );
                assert_eq!(f.source.gmt.load(Ordering::SeqCst), 0);
                assert_eq!(f.source.batch.load(Ordering::SeqCst), 0);
            }
        }
    }
}

#[test]
fn native_known_publication_versions_close_and_unit_decisions_compare_by_call() {
    for sqlite in [false, true] {
        let f = NativeFixture::new(sqlite, 1, false);
        let root = f.root();
        let mut frame = root.frame();
        let (c, o) = connected(&f, &mut frame);
        let open = frame.structure_profile(MqMqiCall::Open, c).unwrap();
        let open_point = frame.point_profile(&open, open_target()).unwrap();
        // A normal OPEN publication advances physical dependencies but not ABI.
        let result = f.call(&mut frame, 3, object_open(c));
        assert!(matches!(output(result), MqMqiOutput::Opened { .. }));
        frame.recheck_structure_profile(&open).unwrap();
        frame.recheck_point_profile(&open_point).unwrap();
        let close = frame.structure_profile(MqMqiCall::Close, c).unwrap();
        let point = frame.point_profile(&close, object_target(o)).unwrap();
        let request = MqObjectCloseRequest::new(
            c,
            MqRouteCloseTarget::Object {
                handle: o,
                lifecycle: MqRouteCloseLifecycle::Predefined,
            },
            MqRouteCloseMode::None,
        )
        .unwrap();
        f.call(&mut frame, 4, MqMqiRequest::Close(request));
        frame.recheck_point_profile(&point).unwrap();
        assert!(
            !point
                .wire_bindings()
                .queue_defaults_are_represented(c, Some(o), None)
        );
        assert!(frame.point_profile(&close, object_target(o)).is_err());
        let structure = frame.structure_profile(MqMqiCall::PutOne, c).unwrap();
        let p = frame
            .point_profile(
                &structure,
                MqTrustedBatchPointTarget::PutOne { lookup: lookup() },
            )
            .unwrap();
        let unit = unit(&frame, c);
        f.call(
            &mut frame,
            5,
            MqMqiRequest::Commit {
                connection: c,
                unit,
            },
        );
        assert!(frame.recheck_structure_profile(&structure).is_err());
        assert_eq!(p.wire_bindings().admitted_unit(c), None);
        // Decisions need a newly observed unit, not a falsely equal old tuple.
        assert!(frame.structure_profile(MqMqiCall::PutOne, c).is_ok());
    }
}

#[test]
fn native_foreign_original_frame_stale_physical_dependencies_and_controls_refuse() {
    for sqlite in [false, true] {
        let f = NativeFixture::new(sqlite, 1, false);
        let root = f.root();
        let parent = root.frame();
        let mut child = f.child(&parent, "child");
        let (c, o) = connected(&f, &mut child);
        let structure = child.structure_profile(MqMqiCall::Put, c).unwrap();
        let point = child.point_profile(&structure, object_target(o)).unwrap();
        let rows = f.rows();
        assert!(parent.point_profile(&structure, object_target(o)).is_err());
        let other = NativeFixture::new(false, 1, false);
        let other_root = other.root();
        let mut other_frame = other_root.frame();
        let (c2, o2) = connected(&other, &mut other_frame);
        assert!(
            other_frame
                .point_profile(&structure, object_target(o2))
                .is_err()
        );
        assert!(child.structure_profile(MqMqiCall::Put, c2).is_err());
        assert!(child.point_profile(&structure, object_target(o2)).is_err());
        for c in [MqHconn::Default, MqHconn::Unassociated] {
            assert!(child.structure_profile(MqMqiCall::Open, c).is_err());
        }
        assert!(child.structure_profile(MqMqiCall::Get, c).is_err());
        let original = child.original.clone();
        child.original.deadline_tick -= 1;
        assert!(child.structure_profile(MqMqiCall::Put, c).is_err());
        child.original = original;
        assert_eq!(f.rows(), rows);
        // Independently committed dependency advance poisons the old loaded
        // authority, even if every byte other than its physical version matches.
        let row = rows
            .iter()
            .find(|r| r.namespace == "mq-selected-v1-control")
            .unwrap();
        let mut replacement = row.clone();
        replacement.version += 1;
        f.store
            .mutate_provider_states_atomic(vec![ProviderStateMutation::Put(ProviderStateWrite {
                record: replacement,
                expected_version: Some(row.version),
            })])
            .unwrap();
        let changed = f.rows();
        assert!(child.recheck_point_profile(&point).is_err());
        assert_eq!(point.wire_bindings().admitted_unit(c), None);
        assert_eq!(f.rows(), changed);
    }
}

#[test]
fn native_source_panic_reentry_cancellation_deadline_and_retired_child_are_read_only() {
    for sqlite in [false, true] {
        let f = NativeFixture::new(sqlite, 1, false);
        let root = f.root();
        let parent = root.frame();
        let mut child = f.child(&parent, "child");
        let (c, o) = connected(&f, &mut child);
        let rows = f.rows();
        *f.source.reentry.lock().unwrap() = Some(Arc::downgrade(&f.runtime.inner.service));
        let structure = child.structure_profile(MqMqiCall::Put, c).unwrap();
        let point = child.point_profile(&structure, object_target(o)).unwrap();
        f.source.fail.store(true, Ordering::SeqCst);
        assert!(child.recheck_point_profile(&point).is_err());
        f.source.fail.store(false, Ordering::SeqCst);
        f.source.panic.store(true, Ordering::SeqCst);
        assert!(matches!(
            child.recheck_point_profile(&point),
            Err(HostProblem::ProviderFailure)
        ));
        f.source.panic.store(false, Ordering::SeqCst);
        child.recheck_point_profile(&point).unwrap();
        f.clock.0.store(1000, Ordering::SeqCst);
        assert!(child.structure_profile(MqMqiCall::Put, c).is_err());
        f.clock.0.store(20, Ordering::SeqCst);
        child.return_normal().unwrap();
        assert!(child.recheck_point_profile(&point).is_err());
        assert_eq!(point.wire_bindings().admitted_unit(c), None);
        assert_eq!(f.rows(), rows);
        assert_eq!(f.source.gmt.load(Ordering::SeqCst), 0);
        assert_eq!(f.source.batch.load(Ordering::SeqCst), 0);
        let next = f.child(&parent, "next");
        let structure = next.structure_profile(MqMqiCall::Put, c).unwrap();
        let point = next.point_profile(&structure, object_target(o)).unwrap();
        next.original.cancellation_probe.as_ref().unwrap().request();
        assert!(next.recheck_point_profile(&point).is_err());
        assert_eq!(point.wire_bindings().admitted_unit(c), None);
        assert_eq!(f.rows(), rows);
    }
}

#[test]
fn native_privileged_setup_once_unique_inactive_same_store_and_unconfigured_refusal() {
    let mut f = NativeFixture::new(false, 1, false);
    assert!(matches!(
        f.f.runtime
            .configure_producer_source(&f.store.clone(), Arc::new(Source::default())),
        Err(HostProblem::Unauthorized)
    ));
    let original = f.parent.clone();
    let root = f.root();
    let mut frame = root.frame();
    let (c, _) = connected(&f, &mut frame);
    assert!(matches!(
        f.f.runtime
            .configure_producer_source(&f.store.clone(), Arc::new(Source::default())),
        Err(HostProblem::Unsupported)
    ));
    let mut runtime = open(f.store.clone(), f.saf.clone(), f.clock.clone()).unwrap();
    let other: Arc<dyn PlatformStore> = backend(false);
    assert!(matches!(
        runtime.configure_producer_source(&other, Arc::new(Source::default())),
        Err(HostProblem::Unauthorized)
    ));
    runtime
        .configure_producer_source(&f.store, Arc::new(Source::default()))
        .unwrap();
    // Opening another physical incarnation cannot lend old tokens authority.
    let r = runtime.admit_root(original).unwrap();
    assert!(r.frame().structure_profile(MqMqiCall::Put, c).is_err());
    let old = Fixture::new(false);
    let root = old.root();
    let mut frame = root.frame();
    let (c, _) = connected(&old, &mut frame);
    assert!(matches!(
        frame.structure_profile(MqMqiCall::Open, c),
        Err(HostProblem::Unsupported)
    ));
}

fn complete_message() -> MqFullMessage {
    MqFullMessage {
        descriptor: MqMdValue::V1 {
            characters: MqMdCharacterEncoding::AsciiCompatible,
            fields: MqMdFields {
                struc_id: *b"MD  ",
                report: 0,
                msg_type: 8,
                expiry: -1,
                feedback: 0,
                encoding: 273,
                coded_char_set_id: 819,
                format: [b' '; 8],
                priority: 0,
                persistence: 1,
                msg_id: [7; 24],
                correl_id: [0; 24],
                backout_count: -1,
                reply_to_q: [b' '; 48],
                reply_to_q_mgr: [b' '; 48],
                user_identifier: [b' '; 12],
                accounting_token: [0; 32],
                appl_identity_data: [b' '; 32],
                put_appl_type: 0,
                put_appl_name: [b' '; 28],
                put_date: [b' '; 8],
                put_time: [b' '; 8],
                appl_origin_data: [b' '; 4],
            },
        },
        body: vec![0, 255, 37],
        properties: vec![],
    }
}
#[test]
fn native_nocontext_source_real_original_pending_put_replay_and_backout_never_sample_time() {
    for sqlite in [false, true] {
        let f = NativeFixture::new(sqlite, 1, false);
        let root = f.root();
        let parent = root.frame();
        let mut child = f.child(&parent, "child");
        let (c, o) = connected(&f, &mut child);
        let structure = child.structure_profile(MqMqiCall::Put, c).unwrap();
        let point = child.point_profile(&structure, object_target(o)).unwrap();
        let current = unit(&child, c);
        let bit = |name: &str| {
            mainframe_env_host_api::mq_wire_options::numeric_identities()
                .iter()
                .find(|(n, _)| *n == name)
                .unwrap()
                .1
        };
        let put = mainframe_env_host_api::mq_wire_options::put_full(
            c,
            mainframe_env_host_api::mq_wire_options::MqWireFullPut {
                message: complete_message(),
                pmo_version: 1,
                options: i64::from(
                    bit("MQPMO_SYNCPOINT") | bit("MQPMO_NO_CONTEXT") | bit("MQPMO_SYNC_RESPONSE"),
                ),
            },
            point.wire_bindings(),
            Default::default(),
        )
        .unwrap();
        assert_eq!(put.unit, MqMqiUnitOfWork::Local { unit: current });
        let result = f.call(
            &mut child,
            3,
            MqMqiRequest::FullPut {
                connection: c,
                object: o,
                put,
            },
        );
        let MqMqiOutput::Produced(produced) = output(result) else {
            panic!()
        };
        assert_eq!(produced.outcome, MqDeliveryOutcome::Pending);
        assert_eq!(f.depth(), 0);
        child.recheck_point_profile(&point).unwrap();
        assert_eq!(f.source.gmt.load(Ordering::SeqCst), 0);
        assert_eq!(f.source.batch.load(Ordering::SeqCst), 0);
        f.call(
            &mut child,
            4,
            MqMqiRequest::Back {
                connection: c,
                unit: current,
            },
        );
        assert_eq!(f.depth(), 0);
        assert_eq!(point.wire_bindings().admitted_unit(c), None);
        let rows = f.rows();
        let e = effect(
            &child,
            5,
            MqMqiRequest::FullPut {
                connection: c,
                object: o,
                put: MqMqiFullPut {
                    message: complete_message(),
                    message_handle: None,
                    context: MqMqiMessageContext::Default,
                    options: MqMqiOptions::PutV1Synchronous,
                    unit: MqMqiUnitOfWork::NoSyncpoint,
                },
            },
        );
        seed(&*f.store, child.original(), &e);
        assert!(matches!(
            dispatch(&mut child, &e),
            Err(HostProblem::Unsupported)
        ));
        assert_eq!(f.rows(), rows);
        assert_eq!(f.source.gmt.load(Ordering::SeqCst), 1);
        assert_eq!(f.source.batch.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn native_late_probe_clock_execution_and_catalog_changes_never_return_facts() {
    for sqlite in [false, true] {
        for case in 0..5 {
            let f = NativeFixture::new(sqlite, 2, false);
            let root = f.root();
            let parent = root.frame();
            let mut child = f.child(&parent, "child");
            let (c, o) = connected(&f, &mut child);
            let rows = f.rows();
            let current = unit(&child, c);
            let source = f.source.clone();
            let store = f.store.clone();
            let clock = f.clock.clone();
            let probe = child.original().cancellation_probe.clone().unwrap();
            let execution = child.original().execution_id.clone();
            let catalog = rows
                .iter()
                .find(|r| r.namespace == "mq-v1-object-catalog")
                .unwrap()
                .clone();
            *source.hook.lock().unwrap() = Some(Box::new(move || match case {
                0 => probe.request(),
                1 => clock.0.store(1000, Ordering::SeqCst),
                2 => clock.0.store(19, Ordering::SeqCst),
                3 => {
                    store
                        .transition_execution(&execution, 3, ExecutionState::Failed, 20)
                        .unwrap();
                }
                _ => {
                    let mut next = catalog.clone();
                    next.version += 1;
                    store
                        .mutate_provider_states_atomic(vec![ProviderStateMutation::Put(
                            ProviderStateWrite {
                                record: next,
                                expected_version: Some(catalog.version),
                            },
                        )])
                        .unwrap();
                }
            }));
            assert!(
                child.structure_profile(MqMqiCall::Put, c).is_err(),
                "case {case}"
            );
            if case < 4 {
                assert_eq!(f.rows(), rows);
            } else {
                let mut expected = rows.clone();
                expected
                    .iter_mut()
                    .find(|r| r.namespace == "mq-v1-object-catalog")
                    .unwrap()
                    .version += 1;
                assert_eq!(f.rows(), expected);
            }
            assert_eq!(source.gmt.load(Ordering::SeqCst), 0);
            assert_eq!(source.batch.load(Ordering::SeqCst), 0);
            // Once cancellation/clock/execution has invalidated observation,
            // restore only fixture clock to inspect the still-owned live unit.
            if case == 2 {
                f.clock.0.store(20, Ordering::SeqCst);
                assert_eq!(unit(&child, c), current);
            }
            // No callback retries and no provider writes after failure.
            assert_eq!(source.live.load(Ordering::SeqCst), 1);
            let _ = o;
        }
    }
}

#[test]
fn native_unknown_postpublication_fences_escaped_profiles_without_cleanup() {
    for sqlite in [false, true] {
        let f = NativeFixture::new(sqlite, 1, false);
        let root = f.root();
        let parent = root.frame();
        let mut child = f.child(&parent, "child");
        let (c, o) = connected(&f, &mut child);
        let structure = child.structure_profile(MqMqiCall::Put, c).unwrap();
        let point = child.point_profile(&structure, object_target(o)).unwrap();
        let e = effect(
            &child,
            3,
            MqMqiRequest::FullPut {
                connection: c,
                object: o,
                put: MqMqiFullPut {
                    message: complete_message(),
                    message_handle: None,
                    context: MqMqiMessageContext::NoContext,
                    options: MqMqiOptions::PutV1Synchronous,
                    unit: MqMqiUnitOfWork::NoSyncpoint,
                },
            },
        );
        seed(&*f.store, child.original(), &e);
        f.runtime
            .inner
            .service
            .trusted_batch_test_reply_uncertainty();
        assert_eq!(dispatch(&mut child, &e), Err(HostProblem::UnknownOutcome));
        let rows = f.rows();
        let calls = f.source.live.load(Ordering::SeqCst);
        assert!(child.recheck_point_profile(&point).is_err());
        assert!(child.recheck_structure_profile(&structure).is_err());
        assert_eq!(point.wire_bindings().admitted_unit(c), None);
        assert!(
            !point
                .wire_bindings()
                .queue_defaults_are_represented(c, Some(o), None)
        );
        assert_eq!(dispatch(&mut child, &e), Err(HostProblem::Unauthorized));
        assert_eq!(f.source.live.load(Ordering::SeqCst), calls);
        drop(point);
        drop(structure);
        drop(child);
        assert_eq!(f.rows(), rows);
        assert_eq!(f.source.gmt.load(Ordering::SeqCst), 0);
        assert_eq!(f.source.batch.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn native_owned_sqlite_reopen_retains_complete_profile_not_volatile_tokens() {
    let NativeFixture { f, source, _db: db } = NativeFixture::new(true, 2, false);
    let db = db.unwrap();
    let (parent, c, o, rows) = {
        let root = f.root();
        let mut frame = root.frame();
        let (c, o) = connected(&f, &mut frame);
        let s = frame.structure_profile(MqMqiCall::Put, c).unwrap();
        let p = frame.point_profile(&s, object_target(o)).unwrap();
        assert_eq!(p.descriptor_version(), 2);
        (f.parent.clone(), c, o, f.rows())
    };
    drop(f);
    drop(source);
    let store = db.open();
    assert_eq!(store.list_provider_state_prefix("mq-", 4096).unwrap(), rows);
    let source = Arc::new(Source::default());
    let mut runtime = open(
        store.clone(),
        Arc::new(Saf::default()),
        Arc::new(Clock(AtomicU64::new(20))),
    )
    .unwrap();
    runtime
        .configure_producer_source(&store, source.clone())
        .unwrap();
    let root = runtime.admit_root(parent).unwrap();
    let mut frame = root.frame();
    let before = store.list_provider_state_prefix("mq-", 4096).unwrap();
    assert!(frame.structure_profile(MqMqiCall::Put, c).is_err());
    assert_eq!(
        store.list_provider_state_prefix("mq-", 4096).unwrap(),
        before
    );
    let connected = output({
        let e = effect(&frame, 9, connect());
        seed(&*store, frame.original(), &e);
        dispatch(&mut frame, &e).unwrap()
    });
    let MqMqiOutput::Connected(new) = connected else {
        panic!()
    };
    let structure = frame.structure_profile(MqMqiCall::Put, new).unwrap();
    assert!(frame.point_profile(&structure, object_target(o)).is_err());
    assert_eq!(source.gmt.load(Ordering::SeqCst), 0);
    drop(structure);
    drop(frame);
    drop(root);
    drop(runtime);
    drop(store);
}

#[test]
fn native_equal_physical_rows_and_reused_object_slot_cannot_lend_observation() {
    for sqlite in [false, true] {
        let f = NativeFixture::new(sqlite, 1, false);
        let root = f.root();
        let mut frame = root.frame();
        let (c, o) = connected(&f, &mut frame);
        let s = frame.structure_profile(MqMqiCall::Put, c).unwrap();
        let p = frame.point_profile(&s, object_target(o)).unwrap();
        let rows = f.rows();
        let db = sqlite.then(Database::new);
        let copy = db.as_ref().map_or_else(|| backend(false), Database::open);
        for row in &rows {
            for version in 1..=row.version {
                let mut next = row.clone();
                next.version = version;
                copy.put_provider_state(next, (version > 1).then_some(version - 1))
                    .unwrap();
            }
        }
        seed_execution(&*copy, &f.parent);
        assert_eq!(copy.list_provider_state_prefix("mq-", 4096).unwrap(), rows);
        let mut other = open(copy.clone(), f.saf.clone(), f.clock.clone()).unwrap();
        assert_eq!(
            other.configure_producer_source(&f.store, Arc::new(Source::default())),
            Err(HostProblem::Unauthorized)
        );
        other
            .configure_producer_source(&copy, Arc::new(Source::default()))
            .unwrap();
        let other_root = other.admit_root(f.parent.clone()).unwrap();
        let other_frame = other_root.frame();
        let before = copy.list_provider_state_prefix("mq-", 4096).unwrap();
        assert!(other_frame.point_profile(&s, object_target(o)).is_err());
        assert!(other_frame.recheck_point_profile(&p).is_err());
        assert!(other_frame.structure_profile(MqMqiCall::Put, c).is_err());
        assert_eq!(
            copy.list_provider_state_prefix("mq-", 4096).unwrap(),
            before
        );
        assert_eq!(f.rows(), rows);
        drop(other_frame);
        drop(other_root);
        drop(other);
        drop(copy);
        drop(db);
        // The original slot may be reused, but its old generation is never a
        // live object or point observation for the new slot occupant.
        f.call(
            &mut frame,
            3,
            MqMqiRequest::Close(
                MqObjectCloseRequest::new(
                    c,
                    MqRouteCloseTarget::Object {
                        handle: o,
                        lifecycle: MqRouteCloseLifecycle::Predefined,
                    },
                    MqRouteCloseMode::None,
                )
                .unwrap(),
            ),
        );
        let MqMqiOutput::Opened { object: new, .. } = output(f.call(&mut frame, 4, object_open(c)))
        else {
            panic!()
        };
        assert_ne!(o, new);
        assert!(frame.recheck_point_profile(&p).is_err());
        assert!(
            !p.wire_bindings()
                .queue_defaults_are_represented(c, Some(new), None)
        );
        let fresh = frame.structure_profile(MqMqiCall::Put, c).unwrap();
        assert!(frame.point_profile(&fresh, object_target(new)).is_ok());
        assert!(frame.point_profile(&fresh, object_target(o)).is_err());
    }
}

#[test]
fn native_observation_is_not_dispatch_and_preparation_abort_revokes_only_its_frame() {
    for sqlite in [false, true] {
        let f = NativeFixture::new(sqlite, 1, false);
        let root = f.root();
        let mut parent = root.frame();
        let (c, o) = connected(&f, &mut parent);
        let current = unit(&parent, c);
        let mut child = f.child(&parent, "child");
        let rows = f.rows();
        let s = child.structure_profile(MqMqiCall::Open, c).unwrap();
        let p = child
            .point_profile(
                &s,
                MqTrustedBatchPointTarget::Open {
                    lookup: lookup(),
                    access: MqRouteOpenAccess::InputShared,
                },
            )
            .unwrap();
        assert!(
            p.wire_bindings()
                .queue_defaults_are_represented(c, None, Some(&lookup()))
        );
        let callbacks = f.source.live.load(Ordering::SeqCst);
        assert!(
            !p.wire_bindings()
                .queue_defaults_are_represented(c, Some(o), None)
        );
        assert_eq!(f.source.live.load(Ordering::SeqCst), callbacks);
        // A read never marks dispatch or creates a reference requiring a durable
        // cleanup policy. The original parent's unit/objects remain live.
        child.abort_preparation().unwrap();
        assert_eq!(child.abort_preparation(), Err(HostProblem::Unauthorized));
        assert!(child.recheck_point_profile(&p).is_err());
        assert_eq!(p.wire_bindings().admitted_unit(c), None);
        assert!(
            !p.wire_bindings()
                .queue_defaults_are_represented(c, None, Some(&lookup()))
        );
        assert_eq!(unit(&parent, c), current);
        let live = parent.structure_profile(MqMqiCall::Put, c).unwrap();
        assert!(parent.point_profile(&live, object_target(o)).is_ok());
        drop(p);
        drop(s);
        drop(child);
        assert_eq!(f.rows(), rows);
    }
}

#[derive(Default)]
struct FinalClock {
    remaining: AtomicU64,
    hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl MqReplayClock for FinalClock {
    fn now_tick(&self) -> Result<u64, HostProblem> {
        if self
            .remaining
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n > 0).then(|| n - 1)
            })
            .ok()
            == Some(1)
            && let Some(hook) = self.hook.lock().unwrap().take()
        {
            hook();
        }
        Ok(20)
    }
}
impl FinalClock {
    fn arm(&self, callback: impl FnOnce() + Send + 'static) {
        *self.hook.lock().unwrap() = Some(Box::new(callback));
        self.remaining.store(2, Ordering::SeqCst);
    }
}
#[test]
fn native_final_clock_physical_dependency_mutation_refuses_facts_without_writes() {
    for sqlite in [false, true] {
        let clock = Arc::new(FinalClock::default());
        let f = NativeFixture::with_clock(sqlite, 1, false, Some(clock.clone()));
        let root = f.root();
        let mut frame = root.frame();
        let (connection, _) = connected(&f, &mut frame);
        let before = f.rows();
        let control = before
            .iter()
            .find(|r| r.namespace == "mq-selected-v1-control")
            .unwrap()
            .clone();
        let store = f.store.clone();
        clock.arm(move || {
            let mut next = control.clone();
            next.version += 1;
            store
                .mutate_provider_states_atomic(vec![ProviderStateMutation::Put(
                    ProviderStateWrite {
                        record: next,
                        expected_version: Some(control.version),
                    },
                )])
                .unwrap();
        });
        assert!(
            frame
                .structure_profile(MqMqiCall::Open, connection)
                .is_err()
        );
        let mut expected = before;
        expected
            .iter_mut()
            .find(|r| r.namespace == "mq-selected-v1-control")
            .unwrap()
            .version += 1;
        assert_eq!(f.rows(), expected);
        assert_eq!(f.source.live.load(Ordering::SeqCst), 1);
        assert_eq!(f.source.gmt.load(Ordering::SeqCst), 0);
        assert_eq!(f.source.batch.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn native_final_clock_panic_and_selected_reentry_are_contained_without_poisoning() {
    for sqlite in [false, true] {
        for panic in [false, true] {
            let clock = Arc::new(FinalClock::default());
            let f = NativeFixture::with_clock(sqlite, 1, false, Some(clock.clone()));
            let root = f.root();
            let mut frame = root.frame();
            let (connection, _) = connected(&f, &mut frame);
            let before = f.rows();
            let weak = Arc::downgrade(&f.runtime.inner.service);
            clock.arm(move || {
                if panic {
                    panic!("fixture final clock panic");
                }
                assert!(matches!(
                    weak.upgrade().unwrap().require_trusted_batch_rich(),
                    Err(HostProblem::Unsupported)
                ));
            });
            let result = frame.structure_profile(MqMqiCall::Open, connection);
            if panic {
                assert!(matches!(result, Err(HostProblem::ProviderFailure)));
            } else {
                assert!(result.is_ok());
            }
            assert_eq!(f.rows(), before);
            // The contained read did not poison the selected mutex or mark the
            // frame dispatched. A fresh bounded observation remains usable.
            assert!(frame.structure_profile(MqMqiCall::Open, connection).is_ok());
            assert_eq!(f.rows(), before);
            assert_eq!(f.source.gmt.load(Ordering::SeqCst), 0);
            assert_eq!(f.source.batch.load(Ordering::SeqCst), 0);
        }
    }
}

#[derive(Default)]
struct ZeroClock {
    inner: FinalClock,
    zero: Arc<AtomicBool>,
}
impl MqReplayClock for ZeroClock {
    fn now_tick(&self) -> Result<u64, HostProblem> {
        let tick = self.inner.now_tick()?;
        Ok(if self.zero.load(Ordering::SeqCst) {
            0
        } else {
            tick
        })
    }
}
#[test]
fn native_initial_and_final_zero_clock_refuse_without_write_or_poison() {
    for sqlite in [false, true] {
        for final_callback in [false, true] {
            let clock = Arc::new(ZeroClock::default());
            let f = NativeFixture::with_clock(sqlite, 1, false, Some(clock.clone()));
            let root = f.root();
            let mut frame = root.frame();
            let (connection, _) = connected(&f, &mut frame);
            let before = f.rows();
            if final_callback {
                let zero = clock.zero.clone();
                clock.inner.arm(move || zero.store(true, Ordering::SeqCst));
            } else {
                clock.zero.store(true, Ordering::SeqCst);
            }
            assert!(matches!(
                frame.structure_profile(MqMqiCall::Open, connection),
                Err(HostProblem::Malformed)
            ));
            assert_eq!(f.rows(), before);
            assert_eq!(
                f.source.live.load(Ordering::SeqCst),
                u64::from(final_callback)
            );
            clock.zero.store(false, Ordering::SeqCst);
            assert!(frame.structure_profile(MqMqiCall::Open, connection).is_ok());
            assert_eq!(f.rows(), before);
            assert_eq!(f.source.gmt.load(Ordering::SeqCst), 0);
            assert_eq!(f.source.batch.load(Ordering::SeqCst), 0);
        }
    }
}
