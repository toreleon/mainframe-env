use super::super::super::super::pcb;
use super::*;
use mainframe_env_store_api::ProviderStateRecord;

pub(super) fn backends(label: &str, mut f: impl FnMut(Arc<dyn ProviderStateStore>, Option<&str>)) {
    eprintln!("PRIMARY-PRODUCER {label}/memory");
    f(Arc::new(MemoryStore::new(Default::default())), None);
    let path = std::env::temp_dir().join(format!(
        "ims-primary-producer-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let _cleanup = SqliteFile(Some(path.clone()));
    let url = format!("sqlite:{}?mode=rwc", path.display());
    eprintln!("PRIMARY-PRODUCER {label}/sqlite");
    f(
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap()),
        Some(&url),
    );
}

pub(super) fn position(service: &ImsService, run: &str, number: u16) -> PcbPosition {
    let state = service.lock().unwrap();
    pcb::position(&state.state.sessions[run], number)
}

pub(super) fn cursor(service: &ImsService, run: &str, number: u16) -> serde_json::Value {
    serde_json::to_value(position(service, run, number)).unwrap()
}

pub(super) fn rows(store: &dyn ProviderStateStore) -> Vec<ProviderStateRecord> {
    [
        GENERIC_DATABASE_NAMESPACE,
        GENERIC_PENDING_NAMESPACE,
        SESSION_NAMESPACE,
        REPLAY_NAMESPACE,
        SYSTEM_NAMESPACE,
        CHECKPOINT_NAMESPACE,
        STATE_NAMESPACE,
    ]
    .into_iter()
    .flat_map(|ns| store.list_provider_state(ns, 4096).unwrap())
    .collect()
}

#[test]
fn ssa_primary_producer_metadata_shape_secondary_remote_logical_fences() {
    for kind in 0..6 {
        backends(&format!("shape-{kind}"), |store, _| {
            let run = "primary-shape";
            let mut catalog = metadata();
            match kind {
                0 => catalog.databases[0].organization = ImsDatabaseOrganization::Hdam,
                1 => catalog.databases[0].segments[1].max_length = 7,
                2 => catalog.databases[0].segments[1].fields[0].unique = false,
                3 => catalog.databases[0].segments[1].fields[0].sequence = false,
                4 => {
                    let mut fourth = catalog.databases[0].segments[2].clone();
                    fourth.name = "FOURTH".into();
                    fourth.parent = Some("C".into());
                    catalog.databases[0].segments.push(fourth);
                }
                _ => {
                    catalog.databases[0]
                        .secondary_indexes
                        .push(ImsSecondaryIndexMetadata {
                            name: "BYDATA".into(),
                            source_segment: "B".into(),
                            target_segment: "A".into(),
                            source_fields: vec!["BDATA".into()],
                        });
                    let ImsPcbMetadata::Database(pcb) = &mut catalog.psbs[0].pcbs[0] else {
                        unreachable!()
                    };
                    pcb.secondary_index = Some("BYDATA".into());
                }
            }
            let service = seed_catalog(store.clone(), run, catalog);
            let stable = rows(&*store);
            assert_eq!(
                nav(&service, run, 5, ImsOperation::GetHoldNext, UU),
                Err(HostProblem::Unsupported)
            );
            assert_eq!(rows(&*store), stable);
        });
    }
    for remote in [false, true] {
        backends(&format!("logical-{remote}"), |store, _| {
            let run = "primary-logical";
            let mut catalog = metadata();
            let mut parent = catalog.databases[0].clone();
            parent.name = "PARENTDB".into();
            parent.segments.truncate(1);
            let relationship = ImsLogicalRelationshipMetadata {
                parent_database: "PARENTDB".into(),
                parent_segment: "A".into(),
                child_database: "GENDB".into(),
                child_segment: "C".into(),
                paired: false,
            };
            if remote {
                parent.logical_relationships.push(relationship);
            } else {
                catalog.databases[0]
                    .logical_relationships
                    .push(relationship);
            }
            assert_eq!(
                catalog.databases[0].logical_relationships.is_empty(),
                remote
            );
            catalog.databases.push(parent);
            let service = ImsService::open(store.clone(), Default::default()).unwrap();
            service.install_metadata(catalog).unwrap();
            let image = ImsGenericLoadImage {
                database: "GENDB".into(),
                records: vec![
                    ImsGenericLoadRecord {
                        segment: "A".into(),
                        parent: None,
                        data: b"A1r".to_vec(),
                    },
                    ImsGenericLoadRecord {
                        segment: "B".into(),
                        parent: Some(0),
                        data: b"B1114x".to_vec(),
                    },
                ],
            };
            let mut load = request(run, ImsOperation::Load, 2, &[], b"");
            load.data = serde_json::to_vec(&image).unwrap();
            for req in [
                request(run, ImsOperation::Schedule, 1, &[], b""),
                load,
                request(run, ImsOperation::Commit, 3, &[], b""),
            ] {
                let HostResult::Ims(result) = invoke(&service, run, HostRequest::Ims(req)).unwrap()
                else {
                    panic!()
                };
                assert_eq!(result.status, "  ");
            }
            found(
                &service,
                run,
                4,
                ImsOperation::GetHoldUnique,
                PREFIX,
                b"B1114x",
            );
            assert!(position(&service, run, 1).is_held());
            let stable = rows(&*store);
            assert_eq!(
                nav(&service, run, 5, ImsOperation::GetHoldNext, UU),
                Err(HostProblem::Unsupported)
            );
            assert_eq!(rows(&*store), stable);
            assert!(position(&service, run, 1).is_held());
        });
    }
}

#[test]
fn ssa_primary_producer_ac_am_conditions_and_saf_owners() {
    for condition in [0, 1] {
        backends(&format!("condition-{condition}"), |store, _| {
            let run = "primary-condition";
            let mut catalog = metadata();
            let ImsPcbMetadata::Database(pcb) = &mut catalog.psbs[0].pcbs[0] else {
                unreachable!()
            };
            if condition == 0 {
                pcb.sensitive_segments.retain(|s| s.name != "C");
            } else {
                pcb.sensitive_segments
                    .iter_mut()
                    .find(|s| s.name == "C")
                    .unwrap()
                    .processing_options = Some("I".into());
            }
            let service = seed_catalog(store.clone(), run, catalog);
            found(
                &service,
                run,
                5,
                ImsOperation::GetHoldUnique,
                PREFIX,
                b"B1114x",
            );
            let before = position(&service, run, 1);
            let database = store
                .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
                .unwrap()
                .unwrap();
            let result = nav(&service, run, 6, ImsOperation::GetHoldNext, UU).unwrap();
            assert_eq!(result.status, if condition == 0 { "AC" } else { "AM" });
            assert!(result.segments.is_empty());
            assert_eq!(position(&service, run, 1), before);
            assert_eq!(
                store
                    .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
                    .unwrap()
                    .unwrap()
                    .payload,
                database.payload
            );
            assert!(
                store
                    .get_provider_state(REPLAY_NAMESPACE, &format!("{run}-6"))
                    .unwrap()
                    .is_some()
            );
        });
    }
    backends("saf", |store, _| {
        let run = "primary-saf";
        drop(seed(store.clone(), run));
        let policy = Arc::new(Policy::default());
        let service =
            ImsService::open_authorized(store.clone(), Default::default(), policy.clone()).unwrap();
        let stable = rows(&*store);
        *policy.deny_read.lock().unwrap() = true;
        *policy.deny_update.lock().unwrap() = true;
        assert_eq!(
            nav(&service, run, 5, ImsOperation::GetHoldNext, UU),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(rows(&*store), stable);
    });
}

const ROOT_U: &[&[u8]] = &[b"A       *U ", b"B        ", b"C        "];
const CHILD_U: &[&[u8]] = &[b"A        ", b"B       *U ", b"C        "];
const ROOT_V: &[&[u8]] = &[b"A       *V ", b"B        ", b"C        "];

#[test]
fn ssa_primary_producer_stale_other_pcb_hold_keeps_existing_update_status_owner() {
    backends("stale-hold", |store, _| {
        let run = "primary-stale-hold";
        let mut catalog = metadata();
        let ImsPcbMetadata::Database(mut second) = catalog.psbs[0].pcbs[0].clone() else {
            unreachable!()
        };
        second.name = "OTHER".into();
        catalog.psbs[0].pcbs.push(ImsPcbMetadata::Database(second));
        let service = seed_catalog(store, run, catalog);
        let exact: &[&[u8]] = &[
            b"A       (AKEY    EQA1)",
            b"B       (BKEY    EQB11)",
            b"C       (CKEY    EQC111)",
        ];
        for (seq, number) in [(5, 1), (6, 2)] {
            let mut call = navigation(run, seq, ImsOperation::GetHoldUnique, exact);
            call.request.pcb = number;
            invoke(&service, run, HostRequest::ImsNavigation(call)).unwrap();
        }
        let other = position(&service, run, 2);
        let replacement = request(run, ImsOperation::Replace, 7, &["C"], b"C111z");
        let HostResult::Ims(result) = invoke(&service, run, HostRequest::Ims(replacement)).unwrap()
        else {
            panic!()
        };
        assert_eq!(result.status, "  ");
        assert_eq!(position(&service, run, 2), other);
        let mut stale = request(run, ImsOperation::Replace, 8, &["C"], b"C111q");
        stale.pcb = 2;
        let HostResult::Ims(result) = invoke(&service, run, HostRequest::Ims(stale)).unwrap()
        else {
            panic!()
        };
        assert_eq!(result.status, "DJ");
        assert_eq!(position(&service, run, 2), other);
        let mut fresh = navigation(run, 9, ImsOperation::GetHoldNext, UU);
        fresh.request.pcb = 2;
        let HostResult::Ims(result) =
            invoke(&service, run, HostRequest::ImsNavigation(fresh)).unwrap()
        else {
            panic!()
        };
        assert_eq!(result.segments[0].data, b"C112b");
        assert_eq!(
            cursor(&service, run, 2)["held"],
            serde_json::json!({"id":4,"version":1})
        );
        assert_eq!(
            cursor(&service, run, 1)["held"],
            serde_json::json!({"id":3,"version":2})
        );
    });
}

#[test]
fn ssa_primary_producer_ordinary_gnp_parent_frame_and_unique_missing_gap() {
    backends("gnp-gap", |store, _| {
        let run = "primary-gnp-gap";
        let service = seed(store.clone(), run);
        for (seq, expected) in [
            (5, b"C111a".as_slice()),
            (6, b"C112b".as_slice()),
            (7, b"D111e".as_slice()),
        ] {
            found(
                &service,
                run,
                seq,
                ImsOperation::GetHoldNextParent,
                &[],
                expected,
            );
            assert_eq!(cursor(&service, run, 1)["parentage"], 2);
        }
        for seq in [8, 9] {
            let result = nav(&service, run, seq, ImsOperation::GetHoldNextParent, &[]).unwrap();
            assert_eq!(result.status, "GE");
            assert!(result.segments.is_empty());
            assert_eq!(cursor(&service, run, 1)["parentage"], 2);
            assert!(!position(&service, run, 1).is_held());
            assert_eq!(
                cursor(&service, run, 1)["primary_search"]["feedback_path"],
                serde_json::json!([{"segment":"A","key":[65,49]},{"segment":"B","key":[66,49,49]}])
            );
        }
        assert_eq!(
            nav(&service, run, 10, ImsOperation::GetNext, UU)
                .unwrap()
                .status,
            "GE"
        );
        assert_eq!(
            nav(&service, run, 11, ImsOperation::GetNext, UU)
                .unwrap()
                .status,
            "GE"
        );
        found(&service, run, 12, ImsOperation::GetNext, &[], b"B1215x");
        assert_eq!(
            nav(&service, run, 13, ImsOperation::GetUnique, EXACT_MISS)
                .unwrap()
                .status,
            "GE"
        );
        assert_eq!(
            cursor(&service, run, 1)["primary_search"]["boundary"],
            serde_json::json!({"kind":"missing","path":[{"segment":"A","key":[65,49]},{"segment":"B","key":[66,49,49]},{"segment":"C","key":[67,49,49,51]}],"captured_revision":12,"captured_next_id":13})
        );
        found(&service, run, 14, ImsOperation::GetNext, &[], b"D111e");
    });
}

fn found(
    service: &Arc<ImsService>,
    run: &str,
    seq: u64,
    op: ImsOperation,
    ssas: &[&[u8]],
    literal: &[u8],
) {
    let result = nav(service, run, seq, op, ssas).unwrap();
    assert_eq!(result.status, "  ");
    assert_eq!(
        result
            .segments
            .iter()
            .map(|s| s.data.as_slice())
            .collect::<Vec<_>>(),
        [literal]
    );
}

#[test]
fn ssa_primary_producer_actual_b11_witness_b13_boundary_and_exact_fence() {
    backends("trace", |store, _| {
        let run = "primary-trace";
        let service = seed(store, run);
        assert_eq!(
            nav(&service, run, 5, ImsOperation::GetHoldNext, DATA_MISS)
                .unwrap()
                .status,
            "GE"
        );
        let observed = cursor(&service, run, 1);
        eprintln!("PRIMARY-TRACE {observed}");
        assert!(observed["current"].is_null());
        assert!(observed["parentage"].is_null());
        assert!(observed["held"].is_null());
        assert_eq!(observed["after_end"], false);
        let trace = &observed["primary_search"];
        assert_eq!(trace["version"], 1);
        assert_eq!(
            trace["metadata_digest"],
            serde_json::json!([
                90, 87, 81, 45, 131, 49, 221, 72, 205, 147, 98, 118, 141, 151, 61, 246, 93, 177,
                150, 130, 188, 29, 239, 130, 55, 165, 173, 183, 225, 231, 18, 48
            ])
        );
        assert_eq!(trace["observed_revision"], 12);
        assert_eq!(trace["observed_next_id"], 13);
        assert_eq!(
            trace["levels"][0],
            serde_json::json!({"id":1,"parent":null,"segment":"A","key":[65,49],"observed_version":1})
        );
        assert_eq!(
            trace["levels"][1],
            serde_json::json!({"id":2,"parent":1,"segment":"B","key":[66,49,49],"observed_version":1})
        );
        assert!(trace["levels"][2].is_null());
        assert_eq!(
            trace["boundary"],
            serde_json::json!({"kind":"after","path":[{"segment":"A","key":[65,49]},{"segment":"B","key":[66,49,51]}],"anchor_id":8,"edge":"node","provenance":"examined","captured_revision":12,"captured_next_id":13})
        );
        assert_eq!(
            trace["feedback_path"],
            serde_json::json!([{"segment":"A","key":[65,49]},{"segment":"B","key":[66,49,49]}])
        );
        found(&service, run, 6, ImsOperation::GetNext, &[], b"E11f");
        found(&service, run, 7, ImsOperation::GetUnique, PREFIX, b"B1114x");
        assert_eq!(
            nav(&service, run, 8, ImsOperation::GetNext, EXACT_MISS)
                .unwrap()
                .status,
            "GE"
        );
        assert_eq!(
            cursor(&service, run, 1)["primary_search"]["boundary"]["anchor_id"],
            4
        );
        found(&service, run, 9, ImsOperation::GetNext, &[], b"D111e");
    });
}

#[test]
fn ssa_primary_producer_gb_restart_unpositioned_and_legacy_clear() {
    backends("restart", |store, _| {
        let run = "primary-restart";
        let service = seed(store.clone(), run);
        found(
            &service,
            run,
            5,
            ImsOperation::GetUnique,
            &[
                b"A       (AKEY    EQA2)",
                b"B       (BKEY    EQB21)",
                b"C       (CKEY    EQC211)",
            ],
            b"C211d",
        );
        let result = nav(&service, run, 6, ImsOperation::GetNext, &[]).unwrap();
        assert_eq!(result.status, "GB");
        assert!(result.segments.is_empty());
        assert_eq!(
            cursor(&service, run, 1),
            serde_json::json!({"current":null,"parentage":null,"held":null,"after_end":true})
        );
        let before = rows(&*store);
        assert_eq!(
            nav(&service, run, 7, ImsOperation::GetNext, UU),
            Err(HostProblem::Unsupported)
        );
        assert_eq!(rows(&*store), before);
        found(&service, run, 8, ImsOperation::GetNext, &[], b"A1r");
        assert!(position(&service, run, 1).primary_feedback_path().is_none());
        found(&service, run, 9, ImsOperation::GetUnique, PREFIX, b"B1114x");
        let req = request(run, ImsOperation::GetNext, 10, &[], b"");
        let HostResult::Ims(result) = invoke(&service, run, HostRequest::Ims(req)).unwrap() else {
            panic!()
        };
        assert_eq!(result.segments[0].data, b"C111a");
        assert_eq!(
            cursor(&service, run, 1)["primary_search"]["levels"][2]["key"],
            serde_json::json!([67, 49, 49, 49])
        );
        found(&service, run, 11, ImsOperation::GetNext, UU, b"C112b");
        // Legacy requests have no explicit DbBatch context; this successful
        // non-Batch legacy route keeps its old owner and cancels certification.
        let inv = invocation_class(run, ServiceClass::Interactive);
        let req = request(run, ImsOperation::GetUnique, 12, &["B"], b"");
        let host = HostRequest::Ims(req);
        let result = ims_providers(service.clone(), InvocationLimits::default())
            .remove(1)
            .invoke(
                &inv,
                EffectRequest {
                    run_unit: inv.run_unit_id.clone(),
                    sequence: 12,
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
        assert_eq!(result.segments[0].data, b"B1114x");
        assert!(position(&service, run, 1).primary_feedback_path().is_none());
        let before = rows(&*store);
        assert_eq!(
            nav(&service, run, 13, ImsOperation::GetNext, UU),
            Err(HostProblem::Unsupported)
        );
        assert_eq!(rows(&*store), before);
    });
}

#[test]
fn ssa_primary_producer_five_u_v_patterns_real_gu_ghu_and_ancestor_release() {
    backends("patterns", |store, _| {
        for (i, pattern) in [UU, V, ROOT_U, CHILD_U, ROOT_V].into_iter().enumerate() {
            for hold in [false, true] {
                let run = format!("primary-pattern-{i}-{hold}");
                let service = seed(store.clone(), &run);
                found(
                    &service,
                    &run,
                    5,
                    if hold {
                        ImsOperation::GetHoldUnique
                    } else {
                        ImsOperation::GetUnique
                    },
                    PREFIX,
                    b"B1114x",
                );
                let op = if hold {
                    ImsOperation::GetHoldNext
                } else {
                    ImsOperation::GetNext
                };
                found(&service, &run, 6, op, pattern, b"C111a");
                assert_eq!(position(&service, &run, 1).is_held(), hold);
                assert_eq!(cursor(&service, &run, 1)["current"], 3);
                assert_eq!(cursor(&service, &run, 1)["parentage"], 3);
                if hold {
                    assert_eq!(
                        cursor(&service, &run, 1)["held"],
                        serde_json::json!({"id":3,"version":1})
                    );
                }
                found(&service, &run, 7, op, pattern, b"C112b");
                match i {
                    0 | 1 => {
                        for seq in [8, 9] {
                            let result = nav(&service, &run, seq, op, pattern).unwrap();
                            assert_eq!(result.status, "GE");
                            assert!(result.segments.is_empty());
                            assert!(!position(&service, &run, 1).is_held());
                        }
                        found(&service, &run, 10, ImsOperation::GetNext, &[], b"D111e");
                    }
                    2 | 4 => found(&service, &run, 8, op, pattern, b"C121c"),
                    3 => found(&service, &run, 8, op, pattern, b"C211d"),
                    _ => unreachable!(),
                }
                eprintln!("PATTERN-PASS {i}/{hold}: literal forward data/hold/parentage");
            }
        }
    });
}

#[test]
fn ssa_primary_producer_prevalidation_null_replay_and_other_pcb_isolation() {
    backends("identity", |store, _| {
        let run = "primary-identity";
        let mut catalog = metadata();
        let ImsPcbMetadata::Database(mut second) = catalog.psbs[0].pcbs[0].clone() else {
            unreachable!()
        };
        second.name = "OTHER".into();
        catalog.psbs[0].pcbs.push(ImsPcbMetadata::Database(second));
        let service = seed_catalog(store.clone(), run, catalog);
        let mut other = navigation(
            run,
            5,
            ImsOperation::GetHoldUnique,
            &[b"A       (AKEY    EQA2)"],
        );
        other.request.pcb = 2;
        invoke(&service, run, HostRequest::ImsNavigation(other)).unwrap();
        let untouched = position(&service, run, 2);
        let nulls: &[&[u8]] = &[b"A       *-U- ", b"B       *-U- ", b"C       *- "];
        let original = nav(&service, run, 6, ImsOperation::GetHoldNext, nulls).unwrap();
        assert_eq!(original.segments[0].data, b"C111a");
        assert_eq!(position(&service, run, 2), untouched);
        found(&service, run, 7, ImsOperation::GetNext, UU, b"C112b");
        let stable = rows(&*store);
        assert_eq!(
            nav(&service, run, 6, ImsOperation::GetHoldNext, nulls).unwrap(),
            original
        );
        assert_eq!(rows(&*store), stable);
        assert!(!position(&service, run, 1).is_held());
        assert_eq!(
            nav(&service, run, 6, ImsOperation::GetHoldNext, UU),
            Err(HostProblem::IdempotencyConflict)
        );
        let mut changed = navigation(run, 6, ImsOperation::GetHoldNext, nulls);
        changed.request.pcb = 2;
        assert_eq!(
            invoke(&service, run, HostRequest::ImsNavigation(changed)),
            Err(HostProblem::IdempotencyConflict)
        );
        let mut changed = navigation(run, 6, ImsOperation::GetHoldNext, nulls);
        changed.context = ImsExecutionContext::Dbctl;
        assert_eq!(
            invoke(&service, run, HostRequest::ImsNavigation(changed)),
            Err(HostProblem::IdempotencyConflict)
        );
        for (i, bad) in [
            &[b"A       *U ".as_slice(), b"B       *U ", b"C       *U "][..],
            &[b"A       *UV ".as_slice(), b"B        ", b"C        "][..],
            &[
                b"A       *U(AKEY    EQA1)".as_slice(),
                b"B       *U ",
                b"C        ",
            ][..],
            &[b"A       *U ".as_slice(), b"B       *L ", b"C        "][..],
            &[b"B       *U ".as_slice(), b"C        "][..],
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(
                nav(&service, run, 20 + i as u64, ImsOperation::GetNext, bad),
                Err(HostProblem::Unsupported)
            );
            assert_eq!(rows(&*store), stable);
        }
        assert_eq!(position(&service, run, 2), untouched);
    });
}

#[test]
fn ssa_primary_producer_capacity_real_cas_and_lost_ack() {
    use super::super::super::session_cas::SessionCasStore;
    backends("publication", |store, _| {
        let run = "primary-publication";
        let service = seed(store.clone(), run);
        let before = rows(&*store);
        let count = service.lock().unwrap().state.replay.len();
        drop(service);
        let limited = ImsService::open(
            store.clone(),
            ImsLimits {
                max_replays: count,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            nav(&limited, run, 5, ImsOperation::GetHoldNext, UU),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(rows(&*store), before);
        drop(limited);
        let raced = SessionCasStore::new(store.clone(), run);
        let service = ImsService::open(raced.clone(), Default::default()).unwrap();
        raced.arm();
        assert_eq!(
            nav(&service, run, 5, ImsOperation::GetHoldNext, UU),
            Err(HostProblem::IdempotencyConflict)
        );
        let after = rows(&*store);
        assert_eq!(after.len(), before.len());
        for (old, new) in before.iter().zip(&after) {
            assert_eq!(old.payload, new.payload);
            if old.namespace == SESSION_NAMESPACE && old.key == run {
                assert_eq!(new.version, old.version + 1);
            } else {
                assert_eq!(old, new);
            }
        }
        drop(service);
        let service = ImsService::open(raced.clone(), Default::default()).unwrap();
        raced.lose_ack();
        assert_eq!(
            nav(&service, run, 5, ImsOperation::GetHoldNext, UU),
            Err(HostProblem::UnknownOutcome)
        );
        drop(service);
        let service = ImsService::open(store.clone(), Default::default()).unwrap();
        assert_eq!(
            cursor(&service, run, 1)["held"],
            serde_json::json!({"id":3,"version":1})
        );
        let published = rows(&*store);
        found(&service, run, 5, ImsOperation::GetHoldNext, UU, b"C111a");
        assert_eq!(rows(&*store), published);
    });
}
