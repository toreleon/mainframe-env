//! Actual selected producer/store tests. Ports are test-owned provenance, NOT JES.
use super::super::producer::{ProducerBatchContext, ProducerGmt, ProducerSource};
use super::*;
use crate::object::{
    MqNativeAttributes, MqNativeCharacters, MqNativeDeliverySequence, MqNativeQueueAttributes,
};
use mainframe_env_host_api::mq_md_value::*;
use std::sync::atomic::{AtomicU8, AtomicU64};

#[path = "full_put/atomic.rs"]
mod atomic;
#[path = "full_put/authority.rs"]
mod authority;
#[path = "full_put/defaults.rs"]
mod defaults;
#[path = "full_put/failures.rs"]
mod failures;
#[path = "full_put/qualified_get.rs"]
mod qualified_get;
#[path = "full_put/storage.rs"]
mod storage;

#[derive(Default)]
struct Ports {
    encoder_calls: AtomicU64,
    encoder_mode: AtomicU8,
    encoder_hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    gmt_calls: AtomicU64,
    context_calls: AtomicU64,
    live_calls: AtomicU64,
    mode: AtomicU8,
    reentrant: Mutex<Option<std::sync::Weak<MqService>>>,
    after_capture: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl ProducerSource for Ports {
    fn encode_structure(
        &self,
        text: &str,
        chars: MqMdCharacterEncoding,
    ) -> Result<Vec<u8>, HostProblem> {
        self.encoder_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(hook) = self.encoder_hook.lock().unwrap().take() {
            hook();
        }
        match self.encoder_mode.load(Ordering::SeqCst) {
            1 => return Err(HostProblem::ProviderFailure),
            2 => panic!("QName source panic fixture"),
            3 => return Ok(vec![]),
            4 => return Err(HostProblem::Unsupported),
            _ => {}
        }
        if chars == MqMdCharacterEncoding::AsciiCompatible {
            return Ok(text.as_bytes().to_vec());
        }
        // Independent fixed fixture alphabet, NOT a product CP037 implementation.
        // Actual adapter must use the sole foundation encoding authority.
        text.bytes()
            .map(|b| {
                Ok(match b {
                    b'0'..=b'9' => 0xf0 + (b - b'0'),
                    b'A'..=b'I' => 0xc1 + (b - b'A'),
                    b'J'..=b'R' => 0xd1 + (b - b'J'),
                    b'S'..=b'Z' => 0xe2 + (b - b'S'),
                    b' ' => 0x40,
                    _ => return Err(HostProblem::Unsupported),
                })
            })
            .collect()
    }
    fn check_live(&self, _: &Invocation) -> Result<(), HostProblem> {
        self.live_calls.fetch_add(1, Ordering::SeqCst);
        if self.mode.load(Ordering::SeqCst) == 4 {
            return Err(HostProblem::ProviderFailure);
        }
        if let Some(s) = self
            .reentrant
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|w| w.upgrade())
        {
            assert!(matches!(s.lock_selected(), Err(HostProblem::Unsupported)));
        }
        Ok(())
    }
    fn physical_gmt(&self, _: &Invocation) -> Result<ProducerGmt, HostProblem> {
        self.gmt_calls.fetch_add(1, Ordering::SeqCst);
        match self.mode.load(Ordering::SeqCst) {
            1 => Err(HostProblem::ProviderFailure),
            2 => ProducerGmt::new(2026, 2, 29, 1, 2, 3, 4),
            5 => panic!("source uncertainty fixture"),
            _ => ProducerGmt::new(2024, 2, 29, 23, 59, 58, 99),
        }
    }
    fn batch_context(&self, _: &Invocation) -> Result<ProducerBatchContext, HostProblem> {
        self.context_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(hook) = self.after_capture.lock().unwrap().take() {
            hook();
        }
        match self.mode.load(Ordering::SeqCst) {
            3 => Err(HostProblem::InfrastructureFailure),
            6 => ProducerBatchContext::new("JOB".into(), None, None),
            _ => ProducerBatchContext::new("JOB".into(), Some("USER".into()), Some([0xa5; 32])),
        }
    }
}
static NEXT_DB: AtomicU64 = AtomicU64::new(1);
struct Database(std::path::PathBuf);
impl Database {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "mq-full-put-{}-{}",
            std::process::id(),
            NEXT_DB.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&dir).unwrap();
        Self(dir.canonicalize().unwrap())
    }
    fn open(&self, max_audits: usize) -> Arc<dyn PlatformStore> {
        Arc::new(
            SqliteStateStore::open(
                &format!(
                    "sqlite://{}?mode=rwc",
                    self.0.join("state.sqlite").display()
                ),
                64 << 20,
                max_audits,
            )
            .unwrap(),
        )
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        for name in ["state.sqlite", "state.sqlite-wal", "state.sqlite-shm"] {
            let p = self.0.join(name);
            if p.exists() {
                std::fs::remove_file(p).unwrap();
            }
        }
        std::fs::remove_dir(&self.0).unwrap();
    }
}
struct ProducerFixture {
    f: Fixture,
    ports: Arc<Ports>,
    db: Option<Database>,
}
impl std::ops::Deref for ProducerFixture {
    type Target = Fixture;
    fn deref(&self) -> &Fixture {
        &self.f
    }
}
fn catalog(cp: bool, max: i32) -> MqObjectCatalog {
    let q = crate::MqObjectName::new("Q").unwrap();
    MqObjectCatalog::new(
        crate::MqQueueManagerDefinition {
            name: crate::MqObjectName::new("QM").unwrap(),
            default_transmission_queue: None,
        },
        vec![crate::MqObjectDefinition::LocalQueue {
            name: q.clone(),
            usage: crate::MqLocalQueueUsage::Normal,
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
        max_priority: i32::MAX,
        queues: vec![MqNativeQueueAttributes {
            name: q,
            max_msg_length: max,
            delivery_sequence: MqNativeDeliverySequence::Fifo,
            producer_defaults: None,
        }],
    })
    .unwrap()
}
impl ProducerFixture {
    fn new(sqlite: bool, version: i32, cp: bool) -> Self {
        Self::limited(sqlite, version, cp, 256, 32768)
    }
    fn limited(sqlite: bool, version: i32, cp: bool, audits: usize, max: i32) -> Self {
        let db = sqlite.then(Database::new);
        let store: Arc<dyn PlatformStore> = db.as_ref().map_or_else(
            || {
                Arc::new(MemoryStore::new(mainframe_env_store::StoreLimits {
                    max_audits: audits,
                    ..Default::default()
                })) as Arc<dyn PlatformStore>
            },
            |d| d.open(audits),
        );
        let ports = Arc::new(Ports::default());
        let f = Fixture::with_producer_setup(
            store,
            false,
            Some((catalog(cp, max), version, ports.clone())),
        );
        Self { f, ports, db }
    }
    fn unit(&self) -> u64 {
        let s = self.service.lock_selected().unwrap();
        let rich_state::StoredAuthority::Rich(s) = &*s else {
            panic!()
        };
        s.runtime.as_ref().unwrap().connections[0].unit
    }
    fn call(&self, seq: u64, request: MqMqiRequest) -> EffectResult {
        let e = self.effect(seq, request);
        self.seed(&e);
        self.execute(&e).unwrap()
    }
    fn get(
        &self,
        c: MqHconn,
        o: MqHobj,
        seq: u64,
        version: i32,
        cp: bool,
    ) -> Option<MqFullMessage> {
        let mut md = message(version, cp, -77).descriptor;
        match &mut md {
            MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => {
                fields.msg_id = [0; 24];
                fields.correl_id = [0; 24];
            }
        }
        let r = self.call(
            seq,
            MqMqiRequest::FullGet(MqMqiFullGet {
                connection: c,
                object: o,
                descriptor: md,
                mode: MqGetMode::Remove,
                wait: MqWait::NoWait,
                truncation: MqTruncation::Reject,
                buffer_capacity: 32768,
                message_handle: None,
                options: MqMqiOptions::ContractDefault,
                unit: MqMqiUnitOfWork::NoSyncpoint,
            }),
        );
        let HostResult::MqMqi(r) = r.outcome.unwrap() else {
            panic!()
        };
        let MqMqiOutcome::ReviewedOutput {
            output: MqMqiOutput::FullGot { message, .. },
            ..
        } = r.result.outcome
        else {
            panic!()
        };
        message
    }
}
fn message(version: i32, cp: bool, counter: i32) -> MqFullMessage {
    let blank = if cp { 0x40 } else { b' ' };
    let chars = if cp {
        MqMdCharacterEncoding::OwnedCp037
    } else {
        MqMdCharacterEncoding::AsciiCompatible
    };
    let f = MqMdFields {
        struc_id: if cp {
            [0xd4, 0xc4, 0x40, 0x40]
        } else {
            *b"MD  "
        },
        report: 0,
        msg_type: 8,
        expiry: -1,
        feedback: 0,
        encoding: i32::MIN,
        coded_char_set_id: 819,
        format: [blank; 8],
        priority: 0,
        persistence: 1,
        msg_id: [0xa3; 24],
        correl_id: [0; 24],
        backout_count: counter,
        reply_to_q: [blank; 48],
        reply_to_q_mgr: [blank; 48],
        user_identifier: [0; 12],
        accounting_token: [0xff; 32],
        appl_identity_data: [0; 32],
        put_appl_type: i32::MIN,
        put_appl_name: [0; 28],
        put_date: [0; 8],
        put_time: [0; 8],
        appl_origin_data: [0; 4],
    };
    MqFullMessage {
        descriptor: if version == 1 {
            MqMdValue::V1 {
                characters: chars,
                fields: f,
            }
        } else {
            MqMdValue::V2 {
                characters: chars,
                fields: f,
                extension: MqMdV2Fields {
                    group_id: [0; 24],
                    msg_seq_number: 1,
                    offset: 0,
                    msg_flags: 0,
                    original_length: -1,
                },
            }
        },
        body: vec![0, 255, 0, 37, 64],
        properties: vec![],
    }
}
fn put(
    c: MqHconn,
    o: Option<MqHobj>,
    message: MqFullMessage,
    no_context: bool,
    unit: MqMqiUnitOfWork,
) -> MqMqiRequest {
    let p = MqMqiFullPut {
        message,
        message_handle: None,
        context: if no_context {
            MqMqiMessageContext::NoContext
        } else {
            MqMqiMessageContext::Default
        },
        options: MqMqiOptions::PutV1Synchronous,
        unit,
    };
    match o {
        Some(object) => MqMqiRequest::FullPut {
            connection: c,
            object,
            put: p,
        },
        None => MqMqiRequest::FullPutOne {
            connection: c,
            lookup: MqRouteLookup::Queue {
                name: MqRouteName::new("Q").unwrap(),
                manager: None,
                dynamic_pattern: None,
            },
            alternate_user: None,
            put: p,
        },
    }
}
fn produced(reply: &EffectResult) -> &MqMqiProduced {
    let Ok(HostResult::MqMqi(r)) = &reply.outcome else {
        panic!("{reply:?}")
    };
    let MqMqiOutcome::ReviewedOutput {
        status,
        output: MqMqiOutput::Produced(p),
    } = &r.result.outcome
    else {
        panic!("{:?}", r.result)
    };
    assert_eq!(status.completion().symbol(), "MQCC_OK");
    assert_eq!(status.reason_symbol(), "MQRC_NONE");
    p
}
#[test]
fn memory_owned_sqlite_original_full_put_put1_get_md_profiles_context_counter_and_exact_replay() {
    for sqlite in [false, true] {
        for version in [1, 2] {
            for cp in [false, true] {
                for no_context in [false, true] {
                    for one in [false, true] {
                        let f = ProducerFixture::new(sqlite, version, cp);
                        let c = f.connect();
                        let input = f.open(c);
                        let o = (!one).then_some(input);
                        let m = message(version, cp, i32::MIN);
                        let e = f.effect(
                            3,
                            put(c, o, m.clone(), no_context, MqMqiUnitOfWork::NoSyncpoint),
                        );
                        f.seed(&e);
                        let r = f.execute(&e).unwrap();
                        let p = produced(&r);
                        assert_eq!(p.descriptor.fields().backout_count, i32::MIN);
                        assert_eq!(p.backout_count, MqMqiIgnoredCounter::PreservedIgnoredInput);
                        assert_eq!(p.known_dest_count, MqMqiDestinationCount::UndefinedZos);
                        let fields = p.descriptor.fields();
                        let blank = if cp { 0x40 } else { b' ' };
                        let fixed = |prefix: &[u8], n: usize| {
                            let mut a = vec![blank; n];
                            a[..prefix.len()].copy_from_slice(prefix);
                            a
                        };
                        assert_eq!(
                            p.resolved_queue.as_slice(),
                            fixed(if cp { &[0xd8] } else { b"Q" }, 48)
                        );
                        assert_eq!(
                            p.resolved_manager.as_slice(),
                            fixed(if cp { &[0xd8, 0xd4] } else { b"QM" }, 48)
                        );
                        assert_eq!(fields.appl_identity_data, [blank; 32]);
                        assert_eq!(fields.appl_origin_data, [blank; 4]);
                        if no_context {
                            assert_eq!(fields.put_date, [blank; 8]);
                            assert_eq!(fields.put_appl_type, 0);
                            assert_eq!(fields.accounting_token, [0; 32]);
                            assert_eq!(fields.user_identifier, [blank; 12]);
                            assert_eq!(fields.put_appl_name, [blank; 28]);
                            assert_eq!(fields.put_time, [blank; 8]);
                        } else {
                            assert_eq!(
                                fields.put_date,
                                if cp {
                                    [0xf2, 0xf0, 0xf2, 0xf4, 0xf0, 0xf2, 0xf2, 0xf9]
                                } else {
                                    *b"20240229"
                                }
                            );
                            assert_eq!(
                                fields.put_time,
                                if cp {
                                    [0xf2, 0xf3, 0xf5, 0xf9, 0xf5, 0xf8, 0xf9, 0xf9]
                                } else {
                                    *b"23595899"
                                }
                            );
                            assert_eq!(fields.put_appl_type, 2);
                            assert_eq!(fields.accounting_token, [0xa5; 32]);
                            assert_eq!(
                                fields.user_identifier.as_slice(),
                                fixed(
                                    if cp {
                                        &[0xe4, 0xe2, 0xc5, 0xd9]
                                    } else {
                                        b"USER"
                                    },
                                    12
                                )
                            );
                            assert_eq!(
                                fields.put_appl_name.as_slice(),
                                fixed(if cp { &[0xd1, 0xd6, 0xc2] } else { b"JOB" }, 28)
                            );
                        }
                        assert_eq!(
                            f.ports.gmt_calls.load(Ordering::SeqCst),
                            u64::from(!no_context)
                        );
                        assert_eq!(
                            f.ports.context_calls.load(Ordering::SeqCst),
                            u64::from(!no_context)
                        );
                        let rows = f.rows();
                        let replay = f.execute(&e).unwrap();
                        assert_eq!(r, replay);
                        assert_eq!(f.rows(), rows);
                        assert_eq!(
                            f.ports.gmt_calls.load(Ordering::SeqCst),
                            u64::from(!no_context)
                        );
                        let got = f.get(c, input, 4, version, cp).unwrap();
                        let mut expected = m;
                        expected.descriptor = p.descriptor.clone();
                        match &mut expected.descriptor {
                            MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => {
                                fields.backout_count = 0
                            }
                        }
                        assert_eq!(got, expected);
                    }
                }
            }
        }
    }
}
#[test]
fn memory_owned_sqlite_local_commit_backout_and_nosyncpoint_duplicate_ids() {
    for sqlite in [false, true] {
        for commit in [false, true] {
            let f = ProducerFixture::new(sqlite, 2, false);
            let c = f.connect();
            let i = f.open(c);
            let unit = f.unit();
            let m = message(2, false, 255);
            f.call(
                3,
                put(c, None, m.clone(), true, MqMqiUnitOfWork::Local { unit }),
            );
            assert!(f.get(c, i, 4, 2, false).is_none());
            f.call(
                5,
                put(c, None, m.clone(), true, MqMqiUnitOfWork::NoSyncpoint),
            );
            f.call(
                6,
                if commit {
                    MqMqiRequest::Commit {
                        connection: c,
                        unit,
                    }
                } else {
                    MqMqiRequest::Back {
                        connection: c,
                        unit,
                    }
                },
            );
            assert!(f.get(c, i, 7, 2, false).is_some());
            assert_eq!(f.get(c, i, 8, 2, false).is_some(), commit);
            assert!(f.get(c, i, 9, 2, false).is_none());
        }
    }
}
