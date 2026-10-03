use super::*;
use mainframe_env_host_api::{
    ImsGsamAccessMethod, ImsGsamControl, ImsGsamFormat, ImsGsamRecordFormat,
};

fn installed_format(
    store: Arc<dyn ProviderStateStore>,
    kind: ImsGsamRecordFormat,
) -> Arc<ImsService> {
    let service =
        ImsService::open_authorized(store, ImsLimits::default(), Arc::new(Allow)).unwrap();
    let mut c = metadata();
    let record = &mut c.databases[0].segments[0];
    record.min_length = if kind == ImsGsamRecordFormat::U {
        12
    } else {
        2
    };
    record.max_length = 16;
    c.databases[0].gsam_format = Some(ImsGsamFormat {
        version: 1,
        record_format: kind,
        access_method: ImsGsamAccessMethod::Bsam,
        block_size: 32,
        control: ImsGsamControl::None,
    });
    service.install_metadata(c.clone()).unwrap();
    service
        .publish_metadata_generation("LOGAPP", 1, PACKAGE, Some(&c))
        .unwrap();
    service
}

fn area(kind: ImsGsamRecordFormat, index: u8) -> &'static [u8] {
    match (kind, index) {
        (ImsGsamRecordFormat::V, 0) => &[0, 4, 0, 0xff],
        (ImsGsamRecordFormat::V, 1) => &[0, 6, 0x41, 0, 0xff, 0x42],
        (ImsGsamRecordFormat::V, _) => &[0, 3, 0x43],
        (_, 0) => b"0123456789AB",
        (_, 1) => b"0123456789ABCDEF",
        _ => b"0123456789ABC",
    }
}

fn add(kind: ImsGsamRecordFormat, seq: u64, index: u8) -> ImsGsamRequest {
    let data = area(kind, index);
    let mut r = db(seq, ImsOperation::Insert, 2, data);
    r.undefined_length = (kind == ImsGsamRecordFormat::U).then_some(data.len() as u32);
    r
}

fn exercise(store: Arc<dyn TestStore>, kind: ImsGsamRecordFormat) {
    let first = invocation();
    let service = installed_format(store.clone(), kind);
    schedule(&service, &first);
    normal(&service, &store, &first);
    let a = gsam(&service, &first, add(kind, 2, 0))
        .unwrap()
        .address
        .unwrap();
    let b = gsam(&service, &first, add(kind, 3, 1))
        .unwrap()
        .address
        .unwrap();
    assert_eq!(
        gsam(&service, &first, db(4, ImsOperation::GetNext, 1, b""))
            .unwrap()
            .address,
        Some(a.clone())
    );
    invoke_call(&service, &store, &first, 5, symbolic("FORMAT"));
    let json: serde_json::Value =
        serde_json::from_slice(&recovery_rows(&*store)[0].payload).unwrap();
    let positions = json["checkpoints"]["FORMAT"]["request"]["positions"]
        .as_array()
        .unwrap();
    assert_eq!(positions.len(), 2);
    assert!(
        positions
            .iter()
            .all(|p| p["gsam_format"].as_array().unwrap().len() == 32)
    );
    assert_eq!(positions[0]["gsam_format"], positions[1]["gsam_format"]);
    let later = gsam(&service, &first, add(kind, 6, 2))
        .unwrap()
        .address
        .unwrap();
    drop(service);
    let fresh =
        ImsService::open_authorized(store.clone(), ImsLimits::default(), Arc::new(Allow)).unwrap();
    let next = next_execution(&first, "format-restart");
    invoke_call(&fresh, &store, &next, 7, restart("FORMAT"));
    let found = gsam(&fresh, &next, db(8, ImsOperation::GetNext, 1, b"")).unwrap();
    assert_eq!(found.result.segments[0].data, area(kind, 1));
    assert_eq!(found.address, Some(b));
    assert_eq!(
        found.undefined_length,
        (kind == ImsGsamRecordFormat::U).then_some(16)
    );
    assert_eq!(gu(&fresh, &next, 9, later).result.status, "AJ");
    assert_eq!(
        gu(&fresh, &next, 10, a).result.segments[0].data,
        area(kind, 0)
    );
    assert_eq!(
        gsam(&fresh, &first, add(kind, 6, 2))
            .unwrap()
            .result
            .affected_segments,
        1
    );
}

#[test]
fn gsam_formats_checkpoint_restart_retains_format_lengths_and_addresses() {
    for kind in [ImsGsamRecordFormat::V, ImsGsamRecordFormat::U] {
        backends(&format!("formats-recovery-{kind:?}"), |store| {
            exercise(store, kind)
        });
    }
}

#[test]
fn gsam_formats_corrupt_image_or_stale_format_cannot_be_captured_or_reopened() {
    for kind in [ImsGsamRecordFormat::V, ImsGsamRecordFormat::U] {
        for stale_format in [false, true] {
            backends(
                &format!("formats-corrupt-{kind:?}-{stale_format}"),
                |store| {
                    let first = invocation();
                    let service = installed_format(store.clone(), kind);
                    schedule(&service, &first);
                    normal(&service, &store, &first);
                    gsam(&service, &first, add(kind, 2, 0)).unwrap();
                    let mut row = store
                        .list_provider_state("ims-v1-generic-database", 64)
                        .unwrap()
                        .remove(0);
                    let version = row.version;
                    let mut image: serde_json::Value =
                        serde_json::from_slice(&row.payload).unwrap();
                    if stale_format {
                        image["value"]["definition"]["gsam_format"]["control"] =
                            serde_json::json!("Asa");
                    } else {
                        image["value"]["records"][0]["data"] = if kind == ImsGsamRecordFormat::V {
                            serde_json::json!([0, 5, 0, 255])
                        } else {
                            serde_json::json!([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10])
                        };
                    }
                    row.version += 1;
                    row.payload = serde_json::to_vec(&image).unwrap();
                    store.put_provider_state(row, Some(version)).unwrap();
                    let request = call(3, symbolic("BADIMAGE"));
                    intent(&*store, &first, &request);
                    let before = snapshot(&*store);
                    assert_eq!(
                        dispatch(service.clone(), store.clone(), &first, &request),
                        Err(HostProblem::InfrastructureFailure)
                    );
                    assert_eq!(snapshot(&*store), before);
                    assert!(matches!(
                        ImsService::open_authorized(
                            store.clone(),
                            ImsLimits::default(),
                            Arc::new(Allow)
                        ),
                        Err(HostProblem::InfrastructureFailure)
                    ));
                    assert_eq!(snapshot(&*store), before);
                },
            );
        }
    }
}

#[test]
fn gsam_formats_checkpoint_wrong_or_absent_saved_format_rejects_without_mutation() {
    for kind in [ImsGsamRecordFormat::V, ImsGsamRecordFormat::U] {
        for absent in [false, true] {
            backends(&format!("formats-identity-{kind:?}-{absent}"), |store| {
                let first = invocation();
                let service = installed_format(store.clone(), kind);
                schedule(&service, &first);
                normal(&service, &store, &first);
                gsam(&service, &first, add(kind, 2, 0)).unwrap();
                invoke_call(&service, &store, &first, 3, symbolic("WRONG"));
                drop(service);
                // A valid, independently resealed checkpoint from a different format
                // must fail resolver compatibility, not merely its integrity checksum.
                let mut row = recovery_rows(&*store).remove(0);
                let version = row.version;
                let mut payload: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
                let mut image: mainframe_env_ims::recovery::CheckpointImage =
                    serde_json::from_value(payload["checkpoints"]["WRONG"].clone()).unwrap();
                for p in &mut image.request.positions {
                    p.gsam_format = if absent { None } else { Some([0x41; 32]) };
                }
                let image = mainframe_env_ims::recovery::CheckpointImage::seal(
                    image.sequence,
                    image.request,
                    image.committed_database_digest,
                    mainframe_env_ims::recovery::RecoveryLimits::default(),
                )
                .unwrap();
                payload["checkpoints"]["WRONG"] = serde_json::to_value(image).unwrap();
                row.version += 1;
                row.payload = serde_json::to_vec(&payload).unwrap();
                store.put_provider_state(row, Some(version)).unwrap();
                let fresh = ImsService::open_authorized(
                    store.clone(),
                    ImsLimits::default(),
                    Arc::new(Allow),
                )
                .unwrap();
                let next = next_execution(&first, "wrong-format-restart");
                let req = call(4, restart("WRONG"));
                intent(&*store, &next, &req);
                let before = snapshot(&*store);
                assert_eq!(
                    dispatch(fresh, store.clone(), &next, &req),
                    Err(HostProblem::ProviderFailure)
                );
                assert_eq!(snapshot(&*store), before);
            });
        }
    }
}

#[test]
fn gsam_formats_sqlite_real_child_process_checkpoint_restart_and_replay() {
    for kind in ["V", "U"] {
        let path = std::env::temp_dir().join(format!(
            "ims-gsam-formats-child-{kind}-{}.sqlite",
            std::process::id()
        ));
        for phase in ["seed", "restart", "replay"] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "gsam_checkpoint_tests::formats_tests::gsam_formats_process_worker",
                    "--nocapture",
                ])
                .env("IMS_GSAM_FORMATS_PATH", &path)
                .env("IMS_GSAM_FORMATS_PHASE", phase)
                .env("IMS_GSAM_FORMATS_KIND", kind)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
            println!(
                "GSAM {kind} child {phase}:\n{}",
                String::from_utf8_lossy(&output.stdout)
            );
            eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        }
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn gsam_formats_process_worker() {
    let Ok(path) = std::env::var("IMS_GSAM_FORMATS_PATH") else {
        return;
    };
    let kind = if std::env::var("IMS_GSAM_FORMATS_KIND").unwrap() == "V" {
        ImsGsamRecordFormat::V
    } else {
        ImsGsamRecordFormat::U
    };
    let store: Arc<dyn TestStore> = Arc::new(
        SqliteStateStore::open(&format!("sqlite:{path}?mode=rwc"), 64 * 1024 * 1024, 4096).unwrap(),
    );
    let first = invocation();
    let phase = std::env::var("IMS_GSAM_FORMATS_PHASE").unwrap();
    if phase == "seed" {
        let service = installed_format(store.clone(), kind);
        schedule(&service, &first);
        normal(&service, &store, &first);
        gsam(&service, &first, add(kind, 2, 0)).unwrap();
        gsam(&service, &first, add(kind, 3, 1)).unwrap();
        gsam(&service, &first, db(4, ImsOperation::GetNext, 1, b"")).unwrap();
        invoke_call(&service, &store, &first, 5, symbolic("CHILD"));
        gsam(&service, &first, add(kind, 6, 2)).unwrap();
    } else {
        let service =
            ImsService::open_authorized(store.clone(), ImsLimits::default(), Arc::new(Allow))
                .unwrap();
        let next = next_execution(&first, "formats-child-restart");
        let req = call(7, restart("CHILD"));
        if phase == "restart" {
            intent(&*store, &next, &req);
        }
        let before = snapshot(&*store);
        dispatch(service.clone(), store.clone(), &next, &req).unwrap();
        if phase == "replay" {
            assert_eq!(snapshot(&*store), before);
        }
        let got = gsam(&service, &next, db(8, ImsOperation::GetNext, 1, b"")).unwrap();
        assert_eq!(got.result.segments[0].data, area(kind, 1));
        assert_eq!(
            got.undefined_length,
            (kind == ImsGsamRecordFormat::U).then_some(16)
        );
        assert_eq!(
            gsam(&service, &next, db(9, ImsOperation::GetNext, 1, b""))
                .unwrap()
                .result
                .status,
            "GB"
        );
    }
}
