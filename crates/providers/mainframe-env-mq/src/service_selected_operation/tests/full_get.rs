//! Actual private selected service execution with explicitly seeded full inputs.
//! Fixtures grant no PUT producer, operator, installed/public or RACF acceptance.
use super::*;
use crate::delivery::full_message::QueueProfile;
use mainframe_env_host_api::mq_md_value::*;
use std::ops::Deref;
use std::sync::atomic::{AtomicU8, AtomicU64};
#[path = "full_get/backout.rs"]
mod backout;
#[path = "full_get/copied_store.rs"]
mod copied_store;
#[path = "full_get/failures.rs"]
mod failures;
#[path = "full_get/replay.rs"]
mod replay;

static NEXT_DB: AtomicU64 = AtomicU64::new(1);
struct Database(std::path::PathBuf);
impl Database {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "mq-full-get-{}-{}",
            std::process::id(),
            NEXT_DB.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        Self(dir.canonicalize().unwrap())
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
            let p = self.0.join(name);
            if p.exists() {
                std::fs::remove_file(p).unwrap();
            }
        }
        std::fs::remove_dir(&self.0).unwrap();
    }
}

/// Mandatory typed resource-decision fixture, not a RACF implementation.
#[derive(Default)]
struct ReadSaf {
    mode: AtomicU8,
    observations: Mutex<Vec<(PrincipalId, EnterpriseResource)>>,
    hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl EnterpriseAuthorizer for ReadSaf {
    fn authorize(
        &self,
        principal: &PrincipalId,
        resource: &EnterpriseResource,
    ) -> Result<(), HostProblem> {
        self.observations
            .lock()
            .unwrap()
            .push((principal.clone(), resource.clone()));
        if let Some(hook) = self.hook.lock().unwrap().take() {
            hook();
        }
        assert_eq!(principal.as_str(), "TEST");
        match self.mode.load(Ordering::SeqCst) {
            0 => Ok(()),
            1 => Err(HostProblem::Unauthorized),
            2 => Err(HostProblem::ProviderFailure),
            _ => Err(HostProblem::InfrastructureFailure),
        }
    }
}
struct FullFixture {
    f: Fixture,
    saf: Arc<ReadSaf>,
    db: Option<Database>,
}
impl Deref for FullFixture {
    type Target = Fixture;
    fn deref(&self) -> &Fixture {
        &self.f
    }
}

fn rich(store: &dyn PlatformStore) -> rich_state::RichStoredState {
    let rich_state::StoredAuthority::Rich(state) =
        rich_state::read(store, 3, 5, Default::default()).unwrap()
    else {
        panic!("rich authority")
    };
    *state
}
impl FullFixture {
    fn new(sqlite: bool, version: i32, cp: bool, messages: Vec<MqFullMessage>) -> Self {
        let db = sqlite.then(Database::new);
        let store: Arc<dyn PlatformStore> = db.as_ref().map_or_else(
            || {
                Arc::new(MemoryStore::new(mainframe_env_store::StoreLimits {
                    max_audits: 256,
                    ..Default::default()
                })) as Arc<dyn PlatformStore>
            },
            Database::open,
        );
        let Fixture {
            store,
            service,
            inv,
            provider,
            clock,
            saf: legacy_saf,
            ..
        } = Fixture::from_store(store);
        // Bootstrap directory had NO dispatched MQ effect or persisted control.
        // Retire it before explicit pre-activation profile upgrade/input seeding.
        // This is test setup, never production operator/lifecycle permission.
        drop(service);
        let state = rich(&*store);
        let profile = QueueProfile::Complete {
            version,
            characters: if cp {
                MqMdCharacterEncoding::OwnedCp037
            } else {
                MqMdCharacterEncoding::AsciiCompatible
            },
        };
        let (changes, _) = state
            .plan_profile_upgrade(
                &BTreeMap::from([(crate::MqObjectName::new("Q").unwrap(), profile)]),
                Default::default(),
            )
            .unwrap()
            .into_parts();
        store.mutate_provider_states_atomic(changes).unwrap();
        let state = rich(&*store);
        let mut candidate = state.delivery.clone();
        for message in messages {
            candidate
                .put_full(
                    &state.catalog,
                    &crate::MqObjectName::new("Q").unwrap(),
                    message,
                    None,
                )
                .unwrap();
        }
        let (changes, _) = state
            .plan_selected_delivery(&candidate, Vec::new(), Default::default())
            .unwrap()
            .into_parts();
        store.mutate_provider_states_atomic(changes).unwrap();
        let saf = Arc::new(ReadSaf::default());
        let service = MqService::open_selected_mqi(
            store.clone(),
            MqLimits::default(),
            3,
            5,
            saf.clone(),
            clock.clone(),
        )
        .unwrap();
        let process = service.mint_selected_process(&inv).unwrap();
        let (frame, owner) = service.bind_selected_root(process, &inv).unwrap();
        Self {
            f: Fixture {
                store,
                service,
                inv,
                provider,
                clock,
                saf: legacy_saf,
                frame,
                owner,
                connection: Mutex::new(None),
            },
            saf,
            db,
        }
    }
    fn live(&self) -> Vec<u8> {
        let state = self.service.lock_selected().unwrap();
        let rich_state::StoredAuthority::Rich(s) = &*state else {
            panic!()
        };
        s.delivery.encode_live_checkpoint().unwrap()
    }
    fn audits(&self) -> Vec<AuditRecord> {
        self.store
            .audit_records(&self.inv.execution_id, 0, 256)
            .unwrap()
    }
}
fn message(version: i32, cp: bool) -> MqFullMessage {
    // Independent ordinary input fixture, NOT a native PUT/context producer.
    // Scalar initials are reviewed by the frozen raw MD declaration. Body
    // Encoding observations are unused by this unformatted/no-convert GET.
    // q097395_946–951: MQCCSI_Q_MGR must never be a successful stored GET output;
    // explicit body CCSIDs here grant no conversion or queue-default permission.
    // Arbitrary signed/property diagnostics stay in the storage tests, not here.
    let blank = if cp { 0x40 } else { b' ' };
    let characters = if cp {
        MqMdCharacterEncoding::OwnedCp037
    } else {
        MqMdCharacterEncoding::AsciiCompatible
    };
    let fields = MqMdFields {
        struc_id: if cp {
            [0xd4, 0xc4, 0x40, 0x40]
        } else {
            *b"MD  "
        },
        report: 0,
        msg_type: 8,
        expiry: -1,
        feedback: 0,
        encoding: 0,
        coded_char_set_id: if cp { 37 } else { 819 },
        format: [blank; 8],
        priority: 0,
        persistence: 1,
        msg_id: [0xa3; 24],
        correl_id: [0x19; 24],
        backout_count: 0,
        reply_to_q: [blank; 48],
        reply_to_q_mgr: [blank; 48],
        user_identifier: [blank; 12],
        accounting_token: [0; 32],
        appl_identity_data: [blank; 32],
        put_appl_type: 0,
        put_appl_name: [blank; 28],
        put_date: [blank; 8],
        put_time: [blank; 8],
        appl_origin_data: [blank; 4],
    };
    let descriptor = if version == 1 {
        MqMdValue::V1 { characters, fields }
    } else {
        MqMdValue::V2 {
            characters,
            fields,
            extension: MqMdV2Fields {
                group_id: [0; 24],
                msg_seq_number: 1,
                offset: 0,
                msg_flags: 0,
                original_length: -1,
            },
        }
    };
    MqFullMessage {
        descriptor,
        body: vec![0, 255, 37, 0, 64],
        properties: Vec::new(),
    }
}
fn descriptor(version: i32, cp: bool) -> MqMdValue {
    let mut md = message(version, cp).descriptor;
    match &mut md {
        MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => {
            fields.msg_id = [0; 24];
            fields.correl_id = [0; 24];
        }
    }
    md
}
fn request(
    c: MqHconn,
    o: MqHobj,
    version: i32,
    cp: bool,
    capacity: usize,
    unit: MqMqiUnitOfWork,
    accept: bool,
) -> MqMqiRequest {
    MqMqiRequest::FullGet(MqMqiFullGet {
        connection: c,
        object: o,
        descriptor: descriptor(version, cp),
        mode: MqGetMode::Remove,
        wait: MqWait::NoWait,
        truncation: if accept {
            MqTruncation::Accept
        } else {
            MqTruncation::Reject
        },
        buffer_capacity: capacity,
        message_handle: None,
        options: MqMqiOptions::ContractDefault,
        unit,
    })
}
fn full_output(
    reply: EffectResult,
    completion: &str,
    reason: &str,
) -> (MqGetDisposition, Option<MqFullMessage>, Option<i32>) {
    let HostResult::MqMqi(typed) = reply.outcome.unwrap() else {
        panic!()
    };
    let MqMqiOutcome::ReviewedOutput {
        status,
        output:
            MqMqiOutput::FullGot {
                disposition,
                message,
                cursor: None,
                data_length,
            },
    } = typed.result.outcome
    else {
        panic!("full reviewed output")
    };
    assert_eq!(status.completion().symbol(), completion);
    assert_eq!(status.reason_symbol(), reason);
    (disposition, message, data_length)
}

#[test]
fn memory_owned_sqlite_full_get_exact_md1_md2_profiles_prefix_datalength_status_and_replay() {
    for sqlite in [false, true] {
        for version in [1, 2] {
            for cp in [false, true] {
                for (capacity, accept) in [
                    (0, false),
                    (1, false),
                    (0, true),
                    (1, true),
                    (5, false),
                    (32, false),
                ] {
                    let original = message(version, cp);
                    let f = FullFixture::new(sqlite, version, cp, vec![original.clone()]);
                    let c = f.connect();
                    let o = f.open(c);
                    let e = f.effect(
                        3,
                        request(
                            c,
                            o,
                            version,
                            cp,
                            capacity,
                            MqMqiUnitOfWork::NoSyncpoint,
                            accept,
                        ),
                    );
                    f.seed(&e);
                    let reply = f.execute(&e).unwrap();
                    let (completion, reason) = if capacity >= 5 {
                        ("MQCC_OK", "MQRC_NONE")
                    } else if accept {
                        ("MQCC_WARNING", "MQRC_TRUNCATED_MSG_ACCEPTED")
                    } else {
                        ("MQCC_WARNING", "MQRC_TRUNCATED_MSG_FAILED")
                    };
                    let (disposition, got, length) = full_output(reply.clone(), completion, reason);
                    assert_eq!(length, Some(5));
                    let mut expected = original;
                    expected.body.truncate(capacity);
                    assert_eq!(got, Some(expected));
                    assert_eq!(f.depth(), usize::from(capacity < 5 && !accept));
                    assert_eq!(
                        disposition,
                        MqGetDisposition::Message(if capacity >= 5 {
                            MqTruncationDisposition::Complete { length: 5 }
                        } else if accept {
                            MqTruncationDisposition::AcceptedRemoved {
                                required: 5,
                                copied: capacity,
                            }
                        } else {
                            MqTruncationDisposition::RejectedRetained {
                                required: 5,
                                copied: capacity,
                            }
                        })
                    );
                    let rows = f.rows();
                    let live = f.live();
                    let audits = f.audits();
                    let replay = f.execute(&e).unwrap();
                    assert_eq!(replay, reply);
                    assert_eq!(
                        canonical_result_digest(&reply.outcome).unwrap(),
                        canonical_result_digest(&replay.outcome).unwrap()
                    );
                    assert_eq!(f.rows(), rows);
                    assert_eq!(f.live(), live);
                    assert_eq!(f.audits(), audits);
                    let receipt = rows
                        .iter()
                        .find(|r| r.namespace == receipt::NAMESPACE && r.key == "effect-3")
                        .unwrap();
                    assert_eq!(receipt.version, 1);
                    assert_eq!(
                        f.store
                            .effect(e.idempotency_key.as_ref().unwrap())
                            .unwrap()
                            .unwrap()
                            .state,
                        EffectState::Intent
                    );
                    let audit = audits.last().unwrap();
                    assert_eq!(audit.resource, canonical_audit_resource_digest(&e.request));
                    assert_eq!(audit.decision, AuditDecision::Success);
                    assert!(
                        f.saf
                            .observations
                            .lock()
                            .unwrap()
                            .iter()
                            .any(|(_, r)| r.class == EnterpriseResourceClass::MqQueue
                                && r.name.as_str() == "Q"
                                && r.intent == AccessIntent::Read)
                    );
                }
            }
        }
    }
}

#[test]
fn memory_sqlite_empty_zero_body_and_md_matching_are_lossless() {
    for sqlite in [false, true] {
        let mut first = message(2, false);
        first.body.clear();
        let mut second = message(2, false);
        let MqMdValue::V2 { fields, .. } = &mut second.descriptor else {
            panic!()
        };
        fields.msg_id = [0xb7; 24];
        fields.correl_id = [0xc3; 24];
        let f = FullFixture::new(sqlite, 2, false, vec![first.clone(), second.clone()]);
        let c = f.connect();
        let o = f.open(c);
        for (sequence, wrong_message_id) in [(3, false), (4, true)] {
            let mut mismatched = request(c, o, 2, false, 5, MqMqiUnitOfWork::NoSyncpoint, false);
            let MqMqiRequest::FullGet(g) = &mut mismatched else {
                panic!()
            };
            g.descriptor = second.descriptor.clone();
            let MqMdValue::V2 { fields, .. } = &mut g.descriptor else {
                panic!()
            };
            if wrong_message_id {
                fields.msg_id = [0xb8; 24];
            } else {
                fields.correl_id = [0xc4; 24];
            }
            assert_eq!(
                full_output(
                    f.call(sequence, mismatched),
                    "MQCC_FAILED",
                    "MQRC_NO_MSG_AVAILABLE"
                ),
                (MqGetDisposition::NoMessage, None, None)
            );
            assert_eq!(f.depth(), 2);
        }
        let mut get = request(c, o, 2, false, 5, MqMqiUnitOfWork::NoSyncpoint, false);
        let MqMqiRequest::FullGet(g) = &mut get else {
            panic!()
        };
        g.descriptor = second.descriptor.clone();
        assert_eq!(
            full_output(f.call(5, get), "MQCC_OK", "MQRC_NONE").1,
            Some(second)
        );
        assert_eq!(f.depth(), 1);
        assert_eq!(
            full_output(
                f.call(
                    6,
                    request(c, o, 2, false, 0, MqMqiUnitOfWork::NoSyncpoint, false)
                ),
                "MQCC_OK",
                "MQRC_NONE"
            )
            .1,
            Some(first)
        );
        assert_eq!(
            full_output(
                f.call(
                    7,
                    request(c, o, 2, false, 0, MqMqiUnitOfWork::NoSyncpoint, false)
                ),
                "MQCC_FAILED",
                "MQRC_NO_MSG_AVAILABLE"
            ),
            (MqGetDisposition::NoMessage, None, None)
        );
    }
}

#[test]
fn memory_sqlite_local_full_get_touches_actual_unit_backout_and_commit_preserve_payloads() {
    for sqlite in [false, true] {
        for commit in [false, true] {
            let m = message(2, true);
            let f = FullFixture::new(sqlite, 2, true, vec![m.clone()]);
            let c = f.connect();
            let o = f.open(c);
            let unit = f.unit();
            let e = f.effect(
                3,
                request(c, o, 2, true, 32, MqMqiUnitOfWork::Local { unit }, false),
            );
            f.seed(&e);
            assert_eq!(
                full_output(f.execute(&e).unwrap(), "MQCC_OK", "MQRC_NONE").1,
                Some(m)
            );
            assert_eq!(f.depth(), 0);
            {
                let state = f.service.lock_selected().unwrap();
                let rich_state::StoredAuthority::Rich(s) = &*state else {
                    panic!()
                };
                assert_eq!(s.delivery.unit_outcome(unit), MqDeliveryOutcome::Pending);
                assert_eq!(s.ownership.units[&unit].queues, vec!["Q".to_string()]);
            }
            f.call(
                4,
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
            assert_eq!(f.depth(), usize::from(!commit));
            assert_ne!(f.unit(), unit);
            let rows = f.rows();
            assert_eq!(f.execute(&e), Err(HostProblem::UnknownOutcome));
            assert_eq!(f.rows(), rows);
        }
    }
}

#[test]
fn memory_sqlite_empty_or_rejected_truncation_does_not_create_pending_get_or_queue_touch() {
    for sqlite in [false, true] {
        for empty in [false, true] {
            let f = FullFixture::new(
                sqlite,
                2,
                false,
                if empty {
                    vec![]
                } else {
                    vec![message(2, false)]
                },
            );
            let c = f.connect();
            let o = f.open(c);
            let unit = f.unit();
            f.call(
                3,
                request(c, o, 2, false, 0, MqMqiUnitOfWork::Local { unit }, false),
            );
            let state = f.service.lock_selected().unwrap();
            let rich_state::StoredAuthority::Rich(s) = &*state else {
                panic!()
            };
            assert_ne!(s.delivery.unit_outcome(unit), MqDeliveryOutcome::Pending);
            assert!(s.ownership.units[&unit].queues.is_empty());
        }
    }
}
