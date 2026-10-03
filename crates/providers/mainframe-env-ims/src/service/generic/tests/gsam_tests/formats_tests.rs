use super::*;
use mainframe_env_host_api::{
    ImsGsamAccessMethod, ImsGsamControl, ImsGsamFormat, ImsGsamRecordFormat,
};
use std::sync::atomic::Ordering;
mod composition_tests;

fn format(kind: ImsGsamRecordFormat) -> ImsGsamFormat {
    ImsGsamFormat {
        version: 1,
        record_format: kind,
        access_method: ImsGsamAccessMethod::Bsam,
        block_size: 16,
        control: ImsGsamControl::None,
    }
}

fn install(
    store: Arc<dyn ProviderStateStore>,
    run: &str,
    kind: ImsGsamRecordFormat,
    limits: ImsLimits,
) -> Arc<ImsService> {
    let service = ImsService::open(store, limits).unwrap();
    let mut c = metadata();
    c.databases[0].gsam_format = Some(format(kind));
    let record = &mut c.databases[0].segments[0];
    record.min_length = if kind == ImsGsamRecordFormat::U {
        12
    } else {
        2
    };
    record.max_length = if kind == ImsGsamRecordFormat::U {
        16
    } else {
        8
    };
    service.install_metadata(c).unwrap();
    execute(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    service
}

fn insert(run: &str, seq: u64, kind: ImsGsamRecordFormat, data: &[u8]) -> ImsGsamRequest {
    let mut r = gsam(run, seq, ImsOperation::Insert, 2, data);
    r.undefined_length = (kind == ImsGsamRecordFormat::U).then_some(data.len() as u32);
    r
}

fn backends(label: &str, case: impl Fn(Arc<dyn ProviderStateStore>)) {
    case(Arc::new(MemoryStore::new(Default::default())));
    let file = std::env::temp_dir().join(format!(
        "gsam-formats-{label}-{}.sqlite",
        std::process::id()
    ));
    case(Arc::new(
        SqliteStateStore::open(
            &format!("sqlite:{}?mode=rwc", file.display()),
            64 * 1024 * 1024,
            262_144,
        )
        .unwrap(),
    ));
    std::fs::remove_file(file).unwrap();
}

fn literals(kind: ImsGsamRecordFormat) -> (&'static [u8], &'static [u8]) {
    if kind == ImsGsamRecordFormat::V {
        (&[0, 2], &[0, 8, 0, 0xff, 0x41, 0x42, 0, 0x43])
    } else {
        (b"0123456789AB", b"0123456789ABCDEF")
    }
}

#[test]
fn public_gsam_formats_boundaries_identity_eof_replay_and_reopen() {
    for kind in [ImsGsamRecordFormat::V, ImsGsamRecordFormat::U] {
        backends(&format!("roundtrip-{kind:?}"), |store| {
            let run = "format-roundtrip";
            let service = install(store.clone(), run, kind, ImsLimits::default());
            let (a, b) = literals(kind);
            let first = public(service.clone(), run, insert(run, 2, kind, a)).unwrap();
            let second = public(service.clone(), run, insert(run, 3, kind, b)).unwrap();
            let next = gsam(run, 4, ImsOperation::GetNext, 1, b"");
            let got = public(service.clone(), run, next.clone()).unwrap();
            assert_eq!(got.result.segments[0].data, a);
            assert_eq!(got.address, first.address);
            assert_eq!(
                got.undefined_length,
                (kind == ImsGsamRecordFormat::U).then_some(12)
            );
            assert_eq!(
                public(
                    service.clone(),
                    run,
                    gsam(run, 5, ImsOperation::GetNext, 3, b"")
                )
                .unwrap()
                .address,
                first.address
            );
            let direct = public(
                service.clone(),
                run,
                gu(run, 6, 1, second.address.clone().unwrap()),
            )
            .unwrap();
            assert_eq!(direct.result.segments[0].data, b);
            assert_eq!(
                direct.undefined_length,
                (kind == ImsGsamRecordFormat::U).then_some(16)
            );
            let eof = public(
                service.clone(),
                run,
                gsam(run, 7, ImsOperation::GetNext, 1, b""),
            )
            .unwrap();
            assert_eq!(eof.result.status, "GB");
            assert_eq!(eof.undefined_length, None);
            assert_eq!(
                public(
                    service.clone(),
                    run,
                    gsam(run, 8, ImsOperation::GetNext, 1, b"")
                )
                .unwrap()
                .address,
                first.address
            );
            let before = snapshot(&service);
            assert_eq!(public(service.clone(), run, next.clone()).unwrap(), got);
            assert_eq!(snapshot(&service), before);
            assert_eq!(
                public(service.clone(), run, insert(run, 2, kind, b)),
                Err(HostProblem::IdempotencyConflict)
            );
            assert_eq!(snapshot(&service), before);
            drop(service);
            let fresh = ImsService::open(store, ImsLimits::default()).unwrap();
            let before = snapshot(&fresh);
            assert_eq!(public(fresh.clone(), run, next).unwrap(), got);
            assert_eq!(snapshot(&fresh), before);
            assert_eq!(
                public(fresh, run, gu(run, 9, 1, first.address.unwrap()))
                    .unwrap()
                    .result
                    .segments[0]
                    .data,
                a
            );
        });
    }
}

#[test]
fn public_gsam_formats_undefined_requires_owned_adapter_on_historical_route() {
    backends("unowned-pcb-length", |store| {
        let run = "unowned-length";
        let service = install(store, run, ImsGsamRecordFormat::U, ImsLimits::default());
        let before = snapshot(&service);
        for request in [
            gsam(run, 2, ImsOperation::GetNext, 1, b""),
            insert(run, 3, ImsGsamRecordFormat::U, b"0123456789AB"),
        ] {
            assert_eq!(
                service.execute(&invocation(run), &request.request),
                Err(HostProblem::Unsupported)
            );
            assert_eq!(snapshot(&service), before);
        }
    });
}

#[test]
fn public_gsam_formats_malformed_areas_and_owned_lengths_never_mutate() {
    for kind in [ImsGsamRecordFormat::V, ImsGsamRecordFormat::U] {
        backends(&format!("negative-{kind:?}"), |store| {
            let run = "format-invalid";
            let service = install(store, run, kind, ImsLimits::default());
            let before = snapshot(&service);
            let bad: Vec<&[u8]> = if kind == ImsGsamRecordFormat::V {
                vec![
                    &[],
                    &[0],
                    &[0, 0],
                    &[0, 1],
                    &[0, 3],
                    &[0, 2, 1],
                    &[0, 9, 1, 2, 3, 4, 5, 6, 7],
                    &[0xff, 0xff, 1],
                ]
            } else {
                vec![&[], b"0123456789A", b"0123456789ABCDEFG"]
            };
            for (i, data) in bad.iter().enumerate() {
                assert_eq!(
                    public(service.clone(), run, insert(run, 20 + i as u64, kind, data)),
                    Err(HostProblem::Malformed)
                );
                assert_eq!(snapshot(&service), before);
            }
            for length in [None, Some(0), Some(11), Some(13), Some(u32::MAX)] {
                let mut req = insert(run, 40, kind, literals(kind).0);
                req.undefined_length = length;
                if kind == ImsGsamRecordFormat::V && length.is_none() {
                    continue;
                }
                assert_eq!(
                    public(service.clone(), run, req),
                    Err(HostProblem::Malformed)
                );
                assert_eq!(snapshot(&service), before);
            }
            let mut read = gsam(run, 41, ImsOperation::GetNext, 1, b"");
            read.undefined_length = Some(12);
            assert_eq!(
                public(service.clone(), run, read),
                Err(HostProblem::Malformed)
            );
            assert_eq!(snapshot(&service), before);
            assert_eq!(
                public(
                    service.clone(),
                    run,
                    gsam(run, 42, ImsOperation::GetUnique, 1, b"")
                )
                .unwrap()
                .result
                .status,
                "AH"
            );
            let wrong = ImsGsamAddress {
                database: "OTHER".into(),
                token: [0x41; 32],
            };
            assert_eq!(
                public(service.clone(), run, gu(run, 43, 1, wrong))
                    .unwrap()
                    .result
                    .status,
                "AJ"
            );
        });
    }
}

#[test]
fn public_gsam_formats_quota_atomic_failure_and_unknown_acknowledgment() {
    for kind in [ImsGsamRecordFormat::V, ImsGsamRecordFormat::U] {
        backends(&format!("fault-{kind:?}"), |inner| {
            let run = "format-fault";
            let store = failure_tests::Intercept::new(inner);
            let service = install(
                store.clone(),
                run,
                kind,
                ImsLimits {
                    max_roots: 1,
                    ..ImsLimits::default()
                },
            );
            let req = insert(run, 2, kind, literals(kind).0);
            let before = failure_tests::rows(&*store);
            store.mode.store(2, Ordering::SeqCst);
            assert_eq!(
                public(service.clone(), run, req.clone()),
                Err(HostProblem::ResourceExhausted)
            );
            assert_eq!(failure_tests::rows(&*store), before);
            store.mode.store(3, Ordering::SeqCst);
            assert_eq!(
                public(service, run, req.clone()),
                Err(HostProblem::UnknownOutcome)
            );
            let fresh = ImsService::open(
                store.clone(),
                ImsLimits {
                    max_roots: 1,
                    ..ImsLimits::default()
                },
            )
            .unwrap();
            let committed = public(fresh.clone(), run, req.clone()).unwrap();
            let before = failure_tests::rows(&*store);
            assert_eq!(public(fresh.clone(), run, req).unwrap(), committed);
            assert_eq!(failure_tests::rows(&*store), before);
            assert_eq!(
                public(fresh.clone(), run, insert(run, 3, kind, literals(kind).1)),
                Err(HostProblem::ResourceExhausted)
            );
            assert_eq!(failure_tests::rows(&*store), before);
            let read = gsam(run, 4, ImsOperation::GetNext, 1, b"");
            store.mode.store(3, Ordering::SeqCst);
            assert_eq!(
                public(fresh, run, read.clone()),
                Err(HostProblem::UnknownOutcome)
            );
            let fresh = ImsService::open(
                store,
                ImsLimits {
                    max_roots: 1,
                    ..ImsLimits::default()
                },
            )
            .unwrap();
            let got = public(fresh.clone(), run, read.clone()).unwrap();
            assert_eq!(got.result.segments[0].data, literals(kind).0);
            assert_eq!(
                got.undefined_length,
                (kind == ImsGsamRecordFormat::U).then_some(12)
            );
            let before = snapshot(&fresh);
            assert_eq!(public(fresh.clone(), run, read).unwrap(), got);
            assert_eq!(snapshot(&fresh), before);
        });
    }
}

#[test]
fn public_gsam_formats_actual_cas_race_has_no_loser_record() {
    for kind in [ImsGsamRecordFormat::V, ImsGsamRecordFormat::U] {
        backends(&format!("cas-{kind:?}"), |inner| {
            let store = failure_tests::Intercept::new(inner);
            let first = install(store.clone(), "format-a", kind, ImsLimits::default());
            execute(
                &first,
                "format-b",
                &request("format-b", ImsOperation::Schedule, 1, &[], b""),
            );
            let second = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
            store.mode.store(1, Ordering::SeqCst);
            let thread = std::thread::spawn(move || {
                public(
                    first,
                    "format-a",
                    insert("format-a", 2, kind, literals(kind).0),
                )
            });
            store.entered.wait();
            let winner = public(
                second.clone(),
                "format-b",
                insert("format-b", 2, kind, literals(kind).1),
            )
            .unwrap();
            store.release.wait();
            assert_eq!(
                thread.join().unwrap(),
                Err(HostProblem::IdempotencyConflict)
            );
            assert!(
                store
                    .get_provider_state(REPLAY_NAMESPACE, "format-a-2")
                    .unwrap()
                    .is_none()
            );
            execute(
                &second,
                "format-b",
                &request("format-b", ImsOperation::Commit, 3, &[], b""),
            );
            let fresh = ImsService::open(store, ImsLimits::default()).unwrap();
            assert_eq!(
                public(
                    fresh,
                    "format-a",
                    gu("format-a", 3, 1, winner.address.unwrap())
                )
                .unwrap()
                .result
                .segments[0]
                    .data,
                literals(kind).1
            );
        });
    }
}

#[test]
fn public_gsam_formats_authorization_precedes_read_write_and_replay() {
    for kind in [ImsGsamRecordFormat::V, ImsGsamRecordFormat::U] {
        backends(&format!("auth-{kind:?}"), |store| {
            let run = "format-auth";
            let seeded = install(store.clone(), run, kind, ImsLimits::default());
            let added = public(seeded, run, insert(run, 2, kind, literals(kind).0)).unwrap();
            let policy = Arc::new(Policy::default());
            let service =
                ImsService::open_authorized(store, ImsLimits::default(), policy.clone()).unwrap();
            *policy.deny_update.lock().unwrap() = true;
            let before = snapshot(&service);
            for req in [
                gu(run, 3, 1, added.address.unwrap()),
                insert(run, 4, kind, literals(kind).1),
                insert(run, 2, kind, literals(kind).0),
            ] {
                assert_eq!(
                    public(service.clone(), run, req),
                    Err(HostProblem::Unauthorized)
                );
                assert_eq!(snapshot(&service), before);
            }
            assert!(
                policy
                    .seen
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|r| r.class == EnterpriseResourceClass::ImsDatabase
                        && r.name.as_str() == "GENDB")
            );
        });
    }
}

#[test]
fn public_gsam_formats_explicit_controls_and_absent_format_authority() {
    for control in [ImsGsamControl::Asa, ImsGsamControl::Machine] {
        let run = "format-control";
        let service = ImsService::open(
            Arc::new(MemoryStore::new(Default::default())),
            ImsLimits::default(),
        )
        .unwrap();
        let mut c = metadata();
        let db = &mut c.databases[0];
        db.gsam_format = Some(ImsGsamFormat {
            control,
            ..format(ImsGsamRecordFormat::V)
        });
        db.segments[0].min_length = 3;
        db.segments[0].max_length = 8;
        service.install_metadata(c).unwrap();
        execute(
            &service,
            run,
            &request(run, ImsOperation::Schedule, 1, &[], b""),
        );
        let area = [0, 5, 0x01, 0xff, 0x00];
        public(
            service.clone(),
            run,
            insert(run, 2, ImsGsamRecordFormat::V, &area),
        )
        .unwrap();
        assert_eq!(
            public(service, run, gsam(run, 3, ImsOperation::GetNext, 1, b""))
                .unwrap()
                .result
                .segments[0]
                .data,
            area
        );
    }
    let run = "format-absent";
    let service = ImsService::open(
        Arc::new(MemoryStore::new(Default::default())),
        ImsLimits::default(),
    )
    .unwrap();
    let mut c = metadata();
    c.databases[0].segments[0].min_length = 2;
    c.databases[0].segments[0].max_length = 8;
    service.install_metadata(c).unwrap();
    execute(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    let before = snapshot(&service);
    assert_eq!(
        public(
            service.clone(),
            run,
            insert(run, 2, ImsGsamRecordFormat::V, &[0, 2])
        ),
        Err(HostProblem::Unsupported)
    );
    assert_eq!(snapshot(&service), before);
}

#[test]
fn public_gsam_formats_variable_literal_roundtrip() {
    let run = "gsam-variable";
    let service = ImsService::open(
        Arc::new(MemoryStore::new(Default::default())),
        ImsLimits::default(),
    )
    .unwrap();
    let mut catalog = metadata();
    catalog.databases[0].segments[0].min_length = 2;
    catalog.databases[0].segments[0].max_length = 8;
    catalog.databases[0].gsam_format = Some(mainframe_env_host_api::ImsGsamFormat {
        version: 1,
        record_format: mainframe_env_host_api::ImsGsamRecordFormat::V,
        access_method: mainframe_env_host_api::ImsGsamAccessMethod::Bsam,
        block_size: 16,
        control: mainframe_env_host_api::ImsGsamControl::None,
    });
    service.install_metadata(catalog).unwrap();
    execute(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    let inserted = public(
        service.clone(),
        run,
        gsam(
            run,
            2,
            ImsOperation::Insert,
            2,
            &[0, 6, 0x41, 0, 0xff, 0x42],
        ),
    )
    .unwrap();
    assert_eq!(inserted.result.status, "  ");
    let found = public(service, run, gsam(run, 3, ImsOperation::GetNext, 1, b"")).unwrap();
    assert_eq!(found.result.segments[0].data, [0, 6, 0x41, 0, 0xff, 0x42]);
    assert_eq!(found.address, inserted.address);
}
