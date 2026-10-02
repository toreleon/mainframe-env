//! Executable contract gap: a real signed scheduled TM run is not DB-only recovery.
//! These negative witnesses grant no actual TM application-backout acceptance.
use super::*;
use mainframe_env_store_api::{ProviderStateRecord, WorkState};

fn ims_rows(store: &dyn PlatformStore) -> Vec<ProviderStateRecord> {
    store.list_provider_state_prefix("ims", 4096).unwrap()
}

fn actor(base: &Invocation, key: &str) -> Invocation {
    let mut value = base.clone();
    value.idempotency_key = IdempotencyKey::new(key, InvocationLimits::default()).unwrap();
    value
}

fn exercise(store: Arc<dyn PlatformStore>, config: ServerConfig) {
    let trust = Arc::new(test_package_trust());
    let server = ProductServer::open_with_package_trust(
        config,
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        trust.clone(),
    )
    .unwrap();
    let mut package = signed_tm_package(&trust, 1, "BACK");
    package.sections.ims_metadata.as_mut().unwrap().psbs[0]
        .pcbs
        .push(ImsPcbMetadata::AlternateTerminal(ImsTerminalPcbMetadata {
            name: "EXP".into(),
            destination: Some("TERM2".into()),
            modifiable: false,
            express: true,
            same_terminal: false,
            response_mode: false,
        }));
    package.sections.ims_tm.as_mut().unwrap().transactions[0]
        .alternate_pcbs
        .push(TmAlternatePcbDefinition {
            name: "EXP".into(),
            destination: TmDestination::Fixed("TERM2".into()),
            express: true,
        });
    resign_package(&mut package, &trust);
    let installed = server.install_application_package_v2(&package).unwrap();
    server.publish_application_generation(&installed).unwrap();
    permit_tm(&server, &["BACK"]);
    server
        .racf
        .define_profile("IMSDB", "AUTHDB", "IBMUSER", Some(AccessIntent::Control))
        .unwrap();
    let mut invocation = tm_invocation("tm-backout-gap-run", "tm-backout-gap");
    invocation.service_class = ServiceClass::Batch;
    invocation.deadline_tick = server.jes_clock.now_tick().unwrap() + 60_000;
    let ids = InvocationLimits::default();
    invocation.principal = Principal::new(
        PrincipalId::new("IBMUSER", ids).unwrap(),
        [CapabilityId::new("host.ims.write", ids).unwrap()]
            .into_iter()
            .collect(),
        ids,
    )
    .unwrap();
    let admitted = server
        .ims_tm_enqueue(
            "SIGNED-IMS-APPLICATION",
            &actor(&invocation, "tm-gap-enqueue"),
            tm_message("tm-gap-input", "BACK", None),
        )
        .unwrap();
    let work = server
        .ims_tm_claim(
            "SIGNED-IMS-APPLICATION",
            "BACK",
            "tm-gap-worker",
            server.jes_clock.now_tick().unwrap(),
            10_000,
        )
        .unwrap()
        .unwrap();
    assert_eq!(work.work_id, admitted.work_id);
    assert_eq!(work.state, WorkState::Claimed);
    assert!(work.lease_epoch > 0);
    server
        .ims_tm_start(
            "SIGNED-IMS-APPLICATION",
            &actor(&invocation, "tm-gap-start"),
            &work,
        )
        .unwrap();
    let input = server
        .ims_tm_call(
            "SIGNED-IMS-APPLICATION",
            &actor(&invocation, "tm-gap-gu"),
            TmCall::GetUnique,
        )
        .unwrap();
    assert_eq!(input.segment, Some(b"first".to_vec()));
    assert_eq!(input.status, TmPcbStatus::SUCCESS);
    server
        .ims_execute_selected(
            "SIGNED-IMS-APPLICATION",
            &invocation,
            &recovery_db(ImsOperation::Schedule, 100, "tm-gap-db"),
        )
        .unwrap();
    let mut insert = recovery_db(ImsOperation::Insert, 101, "tm-gap-db");
    insert.segments = vec!["ROOT".into()];
    insert.data = b"00000001ROOTDATA".to_vec();
    assert_eq!(
        server
            .ims_execute_selected("SIGNED-IMS-APPLICATION", &invocation, &insert)
            .unwrap()
            .status,
        "  "
    );
    for (key, pcb, bytes) in [
        ("tm-gap-ordinary", TmPcb::Io, b"ordinary".to_vec()),
        (
            "tm-gap-exp-purged",
            TmPcb::Alternate("EXP".into()),
            b"sent".to_vec(),
        ),
    ] {
        server
            .ims_tm_call(
                "SIGNED-IMS-APPLICATION",
                &actor(&invocation, key),
                TmCall::Insert {
                    pcb,
                    segment: bytes,
                },
            )
            .unwrap();
    }
    let purged = server
        .ims_tm_call(
            "SIGNED-IMS-APPLICATION",
            &actor(&invocation, "tm-gap-purg"),
            TmCall::Purge {
                pcb: TmPcb::Alternate("EXP".into()),
            },
        )
        .unwrap();
    assert_eq!(purged.output_message_ids.len(), 1);
    server
        .ims_tm_call(
            "SIGNED-IMS-APPLICATION",
            &actor(&invocation, "tm-gap-exp-buffer"),
            TmCall::Insert {
                pcb: TmPcb::Alternate("EXP".into()),
                segment: b"unsent".to_vec(),
            },
        )
        .unwrap();
    let output = server.ims_tm.outbound("TERM2", 16).unwrap();
    assert_eq!(output.len(), 1);
    assert_eq!(output[0].segments, vec![b"sent".to_vec()]);
    let before = ims_rows(&*store);
    assert!(
        before
            .iter()
            .any(|r| r.namespace == "ims-tm-v1-conversation")
    );
    assert!(
        before
            .iter()
            .any(|r| r.namespace == "ims-v1-generic-unit-of-work")
    );
    let mut run = invocation.clone();
    run.execution_id = ExecutionId::new("tm-gap-recovery", ids).unwrap();
    let request = recovery_request(
        &installed.identity,
        1,
        "tm-gap-recovery",
        ImsRecoveryCall::Rolb,
    );
    let results = run_recovery_machine(
        &server,
        store.clone(),
        &run,
        vec![HostRequest::ImsRecovery(request)],
    );
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].outcome, Err(HostProblem::Unsupported));
    assert_eq!(ims_rows(&*store), before);
    assert_eq!(
        store.get_work(&admitted.work_id).unwrap(),
        Some(work.clone())
    );
    // Actual source context must also reject, rather than admitting TM by
    // weakening the already accepted DB-batch request validator.
    for (sequence, call) in [
        (
            2,
            ImsRecoveryCall::Sets {
                token: Some(*b"TMPT"),
                user_data: Some(vec![]),
            },
        ),
        (
            3,
            ImsRecoveryCall::Setu {
                token: Some(*b"TMPT"),
                user_data: Some(vec![]),
            },
        ),
        (
            4,
            ImsRecoveryCall::Rols {
                token: Some(*b"TMPT"),
                area_length: Some(0),
            },
        ),
        (
            5,
            ImsRecoveryCall::Rols {
                token: None,
                area_length: None,
            },
        ),
        (6, ImsRecoveryCall::Roll),
        (7, ImsRecoveryCall::Rolb),
    ] {
        let mut negative = invocation.clone();
        let key = format!("tm-gap-negative-{sequence}");
        negative.execution_id = ExecutionId::new(&key, ids).unwrap();
        let mut request = recovery_request(&installed.identity, sequence, &key, call);
        if sequence == 7 {
            request.context = ImsExecutionContext::DbDc;
        }
        let results = run_recovery_machine(
            &server,
            store.clone(),
            &negative,
            vec![HostRequest::ImsRecovery(request)],
        );
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].outcome, Err(HostProblem::Unsupported));
        assert_eq!(ims_rows(&*store), before);
        assert_eq!(
            store.get_work(&admitted.work_id).unwrap(),
            Some(work.clone())
        );
    }
    for (label, call, expected) in [
        (
            "malformed",
            ImsRecoveryCall::Sets {
                token: Some(*b"BAD!"),
                user_data: None,
            },
            HostProblem::Malformed,
        ),
        (
            "quota",
            ImsRecoveryCall::Sets {
                token: Some(*b"FULL"),
                user_data: Some(vec![0; 32 * 1024 + 1]),
            },
            HostProblem::ResourceExhausted,
        ),
        ("denied", ImsRecoveryCall::Rolb, HostProblem::Unauthorized),
    ] {
        let mut negative = invocation.clone();
        let key = format!("tm-gap-{label}");
        negative.execution_id = ExecutionId::new(&key, ids).unwrap();
        if label == "denied" {
            negative.principal = Principal::new(
                PrincipalId::new("STRANGER", ids).unwrap(),
                [CapabilityId::new("host.ims.write", ids).unwrap()]
                    .into_iter()
                    .collect(),
                ids,
            )
            .unwrap();
        }
        let request = recovery_request(&installed.identity, 1, &key, call);
        let results = run_recovery_machine(
            &server,
            store.clone(),
            &negative,
            vec![HostRequest::ImsRecovery(request)],
        );
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].outcome, Err(expected));
        assert_eq!(ims_rows(&*store), before);
        assert_eq!(
            store.get_work(&admitted.work_id).unwrap(),
            Some(work.clone())
        );
    }
    let request = recovery_request(
        &installed.identity,
        1,
        "tm-gap-recovery",
        ImsRecoveryCall::Rolb,
    );
    let effect = store
        .effect(&request.mutation.idempotency_key)
        .unwrap()
        .unwrap();
    assert_eq!(effect.state, EffectState::Failed);
    assert_eq!(
        effect.request_digest,
        mainframe_env_host_api::canonical_request_digest(&HostRequest::ImsRecovery(
            request.clone()
        ))
        .unwrap()
    );
    // A later real input read cannot manufacture a recovery publication receipt.
    assert_eq!(
        server
            .ims_tm_call(
                "SIGNED-IMS-APPLICATION",
                &actor(&invocation, "tm-gap-gn"),
                TmCall::GetNext
            )
            .unwrap()
            .segment,
        Some(b"second".to_vec())
    );
    let later = ims_rows(&*store);
    assert_eq!(
        server
            .ims
            .observe_application_recovery(&run, &request)
            .unwrap(),
        None
    );
    assert_eq!(ims_rows(&*store), later);
    assert_eq!(store.get_work(&admitted.work_id).unwrap(), Some(work));
}

#[test]
fn signed_tm_backout_gap_memory() {
    exercise(Arc::new(MemoryStore::new(Default::default())), config());
}

#[test]
fn signed_tm_backout_gap_sqlite() {
    let dir = std::env::temp_dir().join(format!("ims-tm-backout-gap-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let url = format!("sqlite:{}?mode=rwc", dir.join("state.db").display());
    let mut config = config();
    config.store_profile = crate::StoreProfile::Sqlite;
    config.sqlite_url = url.clone();
    exercise(
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).unwrap()),
        config,
    );
    let reopened = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).unwrap();
    assert_eq!(
        reopened
            .list_provider_state("ims-tm-v1-session", 16)
            .unwrap()
            .len(),
        1
    );
    assert!(
        reopened
            .list_provider_state("ims-recovery-v1-session", 16)
            .unwrap()
            .is_empty()
    );
    drop(reopened);
    std::fs::remove_dir_all(dir).unwrap();
}

// A separate output-identity witness; the negative backout tests above stay exact.
fn signed_express_purg_groups(
    store: Arc<dyn PlatformStore>,
    config: ServerConfig,
    second_group: bool,
) {
    let trust = Arc::new(test_package_trust());
    let server = ProductServer::open_with_package_trust(
        config,
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        trust.clone(),
    )
    .unwrap();
    let mut package = signed_tm_package(&trust, 1, "BACK");
    package.sections.ims_metadata.as_mut().unwrap().psbs[0]
        .pcbs
        .push(ImsPcbMetadata::AlternateTerminal(ImsTerminalPcbMetadata {
            name: "EXP".into(),
            destination: Some("TERM2".into()),
            modifiable: false,
            express: true,
            same_terminal: false,
            response_mode: false,
        }));
    package.sections.ims_tm.as_mut().unwrap().transactions[0]
        .alternate_pcbs
        .push(TmAlternatePcbDefinition {
            name: "EXP".into(),
            destination: TmDestination::Fixed("TERM2".into()),
            express: true,
        });
    resign_package(&mut package, &trust);
    let installed = server.install_application_package_v2(&package).unwrap();
    server.publish_application_generation(&installed).unwrap();
    permit_tm(&server, &["BACK"]);
    let deadline = server.jes_clock.now_tick().unwrap() + 60_000;
    let invoke = |key: &str| {
        let mut invocation = tm_invocation("signed-express-group-run", key);
        invocation.deadline_tick = deadline;
        invocation
    };
    let admitted = server
        .ims_tm_enqueue(
            "SIGNED-IMS-APPLICATION",
            &invoke("signed-express-enqueue"),
            tm_message("signed-express-input", "BACK", None),
        )
        .unwrap();
    let now = server.jes_clock.now_tick().unwrap();
    let work = server
        .ims_tm_claim(
            "SIGNED-IMS-APPLICATION",
            "BACK",
            "signed-express-worker",
            now,
            10_000,
        )
        .unwrap()
        .unwrap();
    assert_eq!(work.work_id, admitted.work_id);
    assert_eq!(work.state, WorkState::Claimed);
    assert!(work.lease_id.is_some());
    assert!(work.lease_epoch > 0);
    assert!(work.lease_expiry_tick.unwrap() > now);
    server
        .ims_tm_start(
            "SIGNED-IMS-APPLICATION",
            &invoke("signed-express-start"),
            &work,
        )
        .unwrap();
    let input = server
        .ims_tm_call(
            "SIGNED-IMS-APPLICATION",
            &invoke("signed-express-gu"),
            TmCall::GetUnique,
        )
        .unwrap();
    assert_eq!(input.status, TmPcbStatus::SUCCESS);
    assert_eq!(input.segment, Some(b"first".to_vec()));
    for (key, segment) in [
        ("signed-express-first-one", b"first-one".to_vec()),
        ("signed-express-first-two", b"first-two".to_vec()),
    ] {
        assert_eq!(
            server
                .ims_tm_call(
                    "SIGNED-IMS-APPLICATION",
                    &invoke(key),
                    TmCall::Insert {
                        pcb: TmPcb::Alternate("EXP".into()),
                        segment,
                    },
                )
                .unwrap()
                .status,
            TmPcbStatus::SUCCESS
        );
    }
    assert!(server.ims_tm.outbound("TERM2", 16).unwrap().is_empty());
    let purge = TmCall::Purge {
        pcb: TmPcb::Alternate("EXP".into()),
    };
    let first_invocation = invoke("signed-express-first-purg");
    let first = server
        .ims_tm_call("SIGNED-IMS-APPLICATION", &first_invocation, purge.clone())
        .unwrap();
    assert_eq!(first.status, TmPcbStatus::SUCCESS);
    assert!(!first.replayed);
    assert_eq!(first.output_message_ids.len(), 1);
    let first_groups = server.ims_tm.outbound("TERM2", 16).unwrap();
    assert_eq!(first_groups.len(), 1);
    assert_eq!(first_groups[0].message_id, first.output_message_ids[0]);
    assert_eq!(first_groups[0].destination, "TERM2");
    assert!(first_groups[0].express);
    assert_eq!(
        first_groups[0].segments,
        vec![b"first-one".to_vec(), b"first-two".to_vec()]
    );
    let first_rows = store.list_provider_state("ims-tm-v1-outbound", 16).unwrap();
    assert_eq!(first_rows.len(), 1);
    let replay = server
        .ims_tm_call("SIGNED-IMS-APPLICATION", &first_invocation, purge.clone())
        .unwrap();
    assert_eq!(replay.status, TmPcbStatus::SUCCESS);
    assert!(replay.replayed);
    assert_eq!(replay.output_message_ids, first.output_message_ids);
    assert_eq!(server.ims_tm.outbound("TERM2", 16).unwrap(), first_groups);
    assert_eq!(
        store.list_provider_state("ims-tm-v1-outbound", 16).unwrap(),
        first_rows
    );
    assert_eq!(store.get_work(&work.work_id).unwrap(), Some(work));
    eprintln!(
        "SIGNED EXPRESS CONTROL: signed install/publication; live enqueue/claim/start/GU; first literal group and exact PURG replay passed"
    );
    if second_group {
        for (key, segment) in [
            ("signed-express-second-one", b"second-one".to_vec()),
            ("signed-express-second-two", b"second-two".to_vec()),
        ] {
            assert_eq!(
                server
                    .ims_tm_call(
                        "SIGNED-IMS-APPLICATION",
                        &invoke(key),
                        TmCall::Insert {
                            pcb: TmPcb::Alternate("EXP".into()),
                            segment,
                        },
                    )
                    .unwrap()
                    .status,
                TmPcbStatus::SUCCESS
            );
        }
        let second_invocation = invoke("signed-express-second-purg");
        assert_ne!(
            second_invocation.execution_id,
            first_invocation.execution_id
        );
        assert_ne!(
            second_invocation.idempotency_key,
            first_invocation.idempotency_key
        );
        let second = server.ims_tm_call("SIGNED-IMS-APPLICATION", &second_invocation, purge);
        let actual_groups = server.ims_tm.outbound("TERM2", 16).unwrap();
        let actual_rows = store.list_provider_state("ims-tm-v1-outbound", 16).unwrap();
        eprintln!(
            "SIGNED EXPRESS FAIL-FIRST: second={second:?}; stored_groups={actual_groups:?}; outbound_rows={actual_rows:?}"
        );
        assert_eq!(
            actual_rows.iter().find(|row| row.key == first_rows[0].key),
            Some(&first_rows[0]),
            "the completed first group must remain immutable"
        );
        assert_eq!(
            second.as_ref().map(|result| result.status),
            Ok(TmPcbStatus::SUCCESS)
        );
        let second = second.unwrap();
        assert_eq!(second.output_message_ids.len(), 1);
        assert_ne!(second.output_message_ids[0], first.output_message_ids[0]);
        assert_eq!(actual_groups.len(), 2);
        assert_eq!(
            actual_groups[0].segments,
            vec![b"first-one".to_vec(), b"first-two".to_vec()]
        );
        assert_eq!(
            actual_groups[1].segments,
            vec![b"second-one".to_vec(), b"second-two".to_vec()]
        );
        assert_eq!(actual_groups[1].message_id, second.output_message_ids[0]);
        assert!(actual_groups[1].sequence > actual_groups[0].sequence);
        assert_eq!(actual_rows.len(), 2);
    }
}

struct SignedExpressPurgSqliteDirectory(std::path::PathBuf);

impl Drop for SignedExpressPurgSqliteDirectory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn signed_express_purg_sqlite(second_group: bool) {
    let directory = SignedExpressPurgSqliteDirectory(std::env::temp_dir().join(format!(
        "ims-signed-express-purg-{}-{second_group}",
        std::process::id()
    )));
    std::fs::create_dir(&directory.0).unwrap();
    let url = format!("sqlite:{}?mode=rwc", directory.0.join("state.db").display());
    let mut config = config();
    config.store_profile = crate::StoreProfile::Sqlite;
    config.sqlite_url = url.clone();
    signed_express_purg_groups(
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap()),
        config,
        second_group,
    );
}

#[test]
fn signed_express_purg_output_identity_failfirst_memory() {
    signed_express_purg_groups(
        Arc::new(MemoryStore::new(Default::default())),
        config(),
        true,
    );
}

#[test]
fn signed_express_purg_output_identity_failfirst_sqlite() {
    signed_express_purg_sqlite(true);
}

#[test]
fn signed_express_purg_first_group_replay_control_memory() {
    signed_express_purg_groups(
        Arc::new(MemoryStore::new(Default::default())),
        config(),
        false,
    );
}

#[test]
fn signed_express_purg_first_group_replay_control_sqlite() {
    signed_express_purg_sqlite(false);
}
