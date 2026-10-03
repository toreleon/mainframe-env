//! Real typed CHKP/XRST over independently authored primary hierarchy literals.
use super::*;
use mainframe_env_host_api::{ImsNavigationRequest, ImsResult};

const PREFIX: &[&[u8]] = &[b"A       (AKEY    EQA1)", b"B       (BKEY    EQB11)"];
const CHILD: &[&[u8]] = &[
    b"A       (AKEY    EQA1)",
    b"B       (BKEY    EQB11)",
    b"C       (CKEY    EQC112)",
];
const MISS: &[&[u8]] = &[
    b"A       (AKEY    EQA1)",
    b"B       (BKEY    GEB11*BDATA   EQ14)",
    b"C       (CKEY    EQC113)",
];
const UU: &[&[u8]] = &[b"A       *U ", b"B       *U ", b"C        "];

fn metadata() -> ImsMetadataCatalog {
    let mut m = catalog();
    let seg = |name: &str, parent: Option<&str>, len, key: &str, width| ImsSegmentMetadata {
        name: name.into(),
        parent: parent.map(str::to_owned),
        min_length: len,
        max_length: len,
        fields: vec![ImsFieldMetadata {
            name: Some(key.into()),
            offset: 0,
            length: width,
            sequence: true,
            unique: true,
        }],
    };
    m.databases[0].segments = vec![
        seg("A", None, 3, "AKEY", 2),
        seg("B", Some("A"), 6, "BKEY", 3),
        seg("C", Some("B"), 5, "CKEY", 4),
        seg("D", Some("B"), 5, "DKEY", 4),
        seg("E", Some("A"), 4, "EKEY", 3),
    ];
    m.databases[0].segments[1].fields.push(ImsFieldMetadata {
        name: Some("BDATA".into()),
        offset: 3,
        length: 2,
        sequence: false,
        unique: false,
    });
    let ImsPcbMetadata::Database(pcb) = &mut m.psbs[0].pcbs[0] else {
        panic!()
    };
    pcb.sensitive_segments = [
        ("A", None),
        ("B", Some("A")),
        ("C", Some("B")),
        ("D", Some("B")),
        ("E", Some("A")),
    ]
    .into_iter()
    .map(|(n, p)| ImsSensitiveSegmentMetadata {
        name: n.into(),
        parent: p.map(str::to_owned),
        processing_options: None,
    })
    .collect();
    let mut second = m.psbs[0].pcbs[0].clone();
    let ImsPcbMetadata::Database(pcb) = &mut second else {
        panic!()
    };
    pcb.name = "OTHERPCB".into();
    m.psbs[0].pcbs.push(second);
    m
}

fn load(service: &ImsService, inv: &Invocation) {
    let r = |s: &str, p, data: &[u8]| ImsGenericLoadRecord {
        segment: s.into(),
        parent: p,
        data: data.to_vec(),
    };
    let image = ImsGenericLoadImage {
        database: "LOGDB".into(),
        records: vec![
            r("A", None, b"A1r"),
            r("B", Some(0), b"B1114x"),
            r("C", Some(1), b"C111a"),
            r("C", Some(1), b"C112b"),
            r("D", Some(1), b"D111e"),
            r("B", Some(0), b"B1215x"),
            r("C", Some(5), b"C121c"),
            r("B", Some(0), b"B1316x"),
            r("E", Some(0), b"E11f"),
            r("A", None, b"A2r"),
            r("B", Some(9), b"B2117x"),
            r("C", Some(10), b"C211d"),
        ],
    };
    for req in [
        database_request(ImsOperation::Schedule, 10, b""),
        database_request(ImsOperation::Load, 11, &serde_json::to_vec(&image).unwrap()),
        database_request(ImsOperation::Commit, 12, b""),
    ] {
        assert_eq!(service.execute(inv, &req).unwrap().status, "  ");
    }
}

fn nav(
    service: &Arc<ImsService>,
    inv: &Invocation,
    seq: u64,
    pcb: u16,
    op: ImsOperation,
    ssas: &[&[u8]],
) -> ImsResult {
    let mut request = database_request(op, seq, b"");
    request.pcb = pcb;
    request.segments.clear();
    let host = HostRequest::ImsNavigation(ImsNavigationRequest {
        request,
        context: ImsExecutionContext::DbBatch,
        ssas: ssas.iter().map(|s| s.to_vec()).collect(),
    });
    let result = ims_providers(service.clone(), InvocationLimits::default())[1]
        .invoke(
            inv,
            EffectRequest {
                run_unit: inv.run_unit_id.clone(),
                sequence: seq,
                deadline_tick: inv.deadline_tick,
                idempotency_key: host.mutation().map(|m| m.idempotency_key.clone()),
                request: host,
            },
        )
        .outcome
        .unwrap();
    let HostResult::Ims(result) = result else {
        panic!()
    };
    result
}

fn cursor(store: &dyn ProviderStateStore) -> serde_json::Value {
    let row = store
        .get_provider_state("ims-v1-session-index", "log-run")
        .unwrap()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
    value["value"].clone()
}

#[test]
fn primary_position_real_checkpoint_prefix_all_pcbs_and_exact_replay() {
    backends("primary-prefix", |store| {
        let service = open_catalog(store.clone(), metadata());
        let inv = invocation();
        load(&service, &inv);
        invoke_call(
            &service,
            &store,
            &inv,
            1,
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Normal,
                area_lengths: vec![3],
            },
        );
        assert_eq!(
            nav(&service, &inv, 20, 1, ImsOperation::GetUnique, PREFIX).segments[0].data,
            b"B1114x"
        );
        assert_eq!(
            nav(&service, &inv, 21, 1, ImsOperation::GetHoldNext, MISS).status,
            "GE"
        );
        assert_eq!(
            nav(&service, &inv, 22, 2, ImsOperation::GetHoldUnique, CHILD).segments[0].data,
            b"C112b"
        );
        let checkpoint = call(
            2,
            ImsRecoveryCall::SymbolicCheckpoint {
                id: "PREFIX".into(),
                user_areas: vec![b"XYZ".to_vec()],
            },
        );
        intent(&*store, &inv, &checkpoint);
        let receipt = dispatch(service.clone(), store.clone(), &inv, &checkpoint).unwrap();
        let cleared = cursor(&*store);
        assert!(cleared["position"]["current"].is_null());
        assert!(cleared["position"].get("primary_search").is_none());
        assert!(cleared["pcb_positions"].as_object().unwrap().is_empty());
        let fresh = next_execution(&inv, "primary-xrst-prefix");
        let result = invoke_call(
            &service,
            &store,
            &fresh,
            3,
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Checkpoint("PREFIX".into()),
                area_lengths: vec![3],
            },
        );
        assert_eq!(
            result,
            ImsRecoveryResult::Restarted {
                status: "  ".into(),
                checkpoint_id: Some("PREFIX".into()),
                user_areas: vec![b"XYZ".to_vec()],
                pcb_statuses: vec![(1, "  ".into()), (2, "  ".into())]
            }
        );
        let restored = cursor(&*store);
        assert_eq!(restored["position"]["current"], 2);
        assert_eq!(restored["position"]["parentage"], 2);
        assert!(restored["position"]["held"].is_null());
        assert_eq!(
            restored["position"]["primary_search"]["boundary"]["anchor_id"],
            2
        );
        assert_eq!(restored["pcb_positions"]["2"]["current"], 4);
        assert!(restored["pcb_positions"]["2"]["held"].is_null());
        assert_eq!(
            nav(&service, &fresh, 23, 1, ImsOperation::GetHoldNext, UU).segments[0].data,
            b"C111a"
        );
        let before = snapshot(&*store);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &inv, &checkpoint).unwrap(),
            receipt
        );
        assert_eq!(snapshot(&*store), before);
        let basic = next_execution(&fresh, "primary-basic");
        invoke_call(
            &service,
            &store,
            &basic,
            4,
            ImsRecoveryCall::BasicCheckpoint { id: "BASIC".into() },
        );
        let cleared = cursor(&*store);
        assert!(cleared["position"].get("primary_search").is_none());
        assert!(cleared["position"]["held"].is_null());
        assert!(cleared["pcb_positions"].as_object().unwrap().is_empty());
        eprintln!("PRIMARY-RECOVERY-PASS prefix/all-pcb/basic/exact-replay");
    });
}

#[test]
fn primary_position_deleted_child_checkpoint_xrst_real_failed_gu_gap() {
    backends("primary-deleted", |store| {
        let service = open_catalog(store.clone(), metadata());
        let inv = invocation();
        load(&service, &inv);
        invoke_call(
            &service,
            &store,
            &inv,
            1,
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Normal,
                area_lengths: vec![],
            },
        );
        assert_eq!(
            nav(&service, &inv, 20, 1, ImsOperation::GetHoldUnique, CHILD).segments[0].data,
            b"C112b"
        );
        assert_eq!(
            service
                .execute(&inv, &database_request(ImsOperation::Delete, 21, b""))
                .unwrap()
                .status,
            "  "
        );
        assert_eq!(
            cursor(&*store)["position"]["primary_search"]["boundary"]["provenance"],
            "deleted"
        );
        invoke_call(
            &service,
            &store,
            &inv,
            2,
            ImsRecoveryCall::SymbolicCheckpoint {
                id: "DELETED".into(),
                user_areas: vec![],
            },
        );
        let fresh = next_execution(&inv, "primary-xrst-deleted");
        assert_eq!(
            invoke_call(
                &service,
                &store,
                &fresh,
                3,
                ImsRecoveryCall::Restart {
                    selection: ImsRestartSelection::Checkpoint("DELETED".into()),
                    area_lengths: vec![]
                }
            ),
            ImsRecoveryResult::Restarted {
                status: "  ".into(),
                checkpoint_id: Some("DELETED".into()),
                user_areas: vec![],
                pcb_statuses: vec![(1, "GE".into())]
            }
        );
        let restored = cursor(&*store);
        assert!(restored["position"]["held"].is_null());
        assert!(restored["position"]["parentage"].is_null());
        assert_eq!(
            restored["position"]["primary_search"]["boundary"]["kind"],
            "missing"
        );
        assert_eq!(
            restored["position"]["primary_search"]["levels"][1]["key"],
            serde_json::json!([66, 49, 49])
        );
        assert_eq!(
            nav(&service, &fresh, 22, 1, ImsOperation::GetNext, &[]).segments[0].data,
            b"D111e"
        );
        eprintln!("PRIMARY-RECOVERY-PASS deleted/real-GU/missing-gap/D111e");
    });
}
