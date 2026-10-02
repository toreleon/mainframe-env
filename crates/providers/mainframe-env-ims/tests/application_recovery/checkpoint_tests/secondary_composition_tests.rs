//! Actual secondary images compose with the one retained local backout owner.
use super::*;

fn point(token: [u8; 4]) -> ImsRecoveryCall {
    ImsRecoveryCall::Sets {
        token: Some(token),
        user_data: Some(b"point".to_vec()),
    }
}

fn rols(token: [u8; 4]) -> ImsRecoveryCall {
    ImsRecoveryCall::Rols {
        token: Some(token),
        area_length: Some(5),
    }
}

fn marker(store: &dyn ProviderStateStore) -> serde_json::Value {
    let row = store
        .get_provider_state("ims-v1-session-index", "log-run")
        .unwrap()
        .unwrap();
    let row: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
    row["value"]["recovery"].clone()
}

#[test]
fn selected_secondary_checkpoint_epoch_named_and_terminal_backout_never_revive_witnesses() {
    for boundary in ["named", "rolb", "roll", "generic"] {
        for change in ["data", "key", "delete"] {
            backends(&format!("secondary-compose-{boundary}-{change}"), |store| {
                let service = open_catalog(store.clone(), indexed_catalog());
                let first = invocation();
                setup(&service, &store, &first);
                invoke_call(&service, &store, &first, 2, point(*b"OLD1"));
                let before = marker(&*store);
                nav(
                    &service,
                    &first,
                    2,
                    ImsOperation::GetUnique,
                    &[b"ROOT    (BYCOMP  EQ\0\xff)"],
                );
                invoke_call(&service, &store, &first, 3, symbolic());
                let after = marker(&*store);
                assert_eq!(after["uow_incarnation"], before["uow_incarnation"]);
                assert_eq!(
                    after["uow_epoch"].as_u64(),
                    before["uow_epoch"].as_u64().map(|n| n + 1)
                );
                assert!(matches!(
                    invoke_call(&service, &store, &first, 4, rols(*b"OLD1")),
                    ImsRecoveryResult::BackedOut { status, .. } if status == "RA"
                ));
                invoke_call(&service, &store, &first, 5, point(*b"NEW1"));
                nav(
                    &service,
                    &first,
                    1,
                    ImsOperation::GetHoldUnique,
                    if change == "data" {
                        &[b"ROOT    (KEY     EQ02)"]
                    } else {
                        &[b"ROOT    (KEY     EQ02)", b"CHILD   (CKEY    EQB1)"]
                    },
                );
                let update = database_request(
                    if change == "delete" {
                        ImsOperation::Delete
                    } else {
                        ImsOperation::Replace
                    },
                    40,
                    match change {
                        "data" => b"02X",
                        "key" => b"B1Z1",
                        _ => b"",
                    },
                );
                assert_eq!(service.execute(&first, &update).unwrap().status, "  ");
                if boundary == "generic" {
                    assert_eq!(
                        service
                            .execute(&first, &database_request(ImsOperation::Rollback, 41, b""))
                            .unwrap()
                            .status,
                        "  "
                    );
                } else {
                    let result = invoke_call(
                        &service,
                        &store,
                        &first,
                        6,
                        match boundary {
                            "named" => rols(*b"NEW1"),
                            "rolb" => ImsRecoveryCall::Rolb,
                            _ => ImsRecoveryCall::Roll,
                        },
                    );
                    if boundary == "roll" {
                        assert_eq!(
                            result,
                            ImsRecoveryResult::Abended {
                                code: "U0778".into()
                            }
                        );
                        let r = call(7, restart());
                        intent(&*store, &first, &r);
                        assert_eq!(
                            dispatch(service.clone(), store.clone(), &first, &r),
                            Err(HostProblem::Unsupported)
                        );
                        let rows = recovery_rows(&*store);
                        let recovery: serde_json::Value =
                            serde_json::from_slice(&rows[0].payload).unwrap();
                        let saved = &recovery["checkpoints"]["SECPOS"]["request"]["positions"][1]["secondary"]
                            ["source"];
                        let row = store
                            .get_provider_state("ims-v1-generic-database", "LOGDB")
                            .unwrap()
                            .unwrap();
                        let image: serde_json::Value =
                            serde_json::from_slice(&row.payload).unwrap();
                        let source = image["value"]["records"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|r| r["id"] == saved["id"])
                            .unwrap();
                        if change == "data" {
                            assert_eq!(source["secondary_checkpoint_identity"], saved["identity"]);
                        } else {
                            assert!(source["secondary_checkpoint_identity"].is_null());
                        }
                        assert_eq!(
                            service.execute(
                                &first,
                                &database_request(ImsOperation::Schedule, 43, b"")
                            ),
                            Err(HostProblem::Unsupported)
                        );
                        return;
                    } else {
                        assert!(
                            matches!(result, ImsRecoveryResult::BackedOut { status, .. } if status == "  ")
                        );
                    }
                    if boundary == "named" {
                        // Reconciliation must remain a witnessed image for the later full backout.
                        invoke_call(&service, &store, &first, 7, ImsRecoveryCall::Rolb);
                    }
                }
                if boundary != "roll" {
                    assert_eq!(
                        nav(
                            &service,
                            &first,
                            2,
                            ImsOperation::GetNextParent,
                            &[b"CHILD    "]
                        )
                        .status,
                        "GP"
                    );
                }
                // Rescheduling changes the authoritative incarnation. Old named tokens cannot cross it.
                service
                    .execute(&first, &database_request(ImsOperation::Terminate, 42, b""))
                    .unwrap();
                let next = next_execution(&first, "secondary-composition-restart");
                service
                    .execute(&next, &database_request(ImsOperation::Schedule, 43, b""))
                    .unwrap();
                assert_ne!(marker(&*store)["uow_incarnation"], after["uow_incarnation"]);
                assert!(matches!(
                    invoke_call(&service, &store, &next, 8, rols(*b"NEW1")),
                    ImsRecoveryResult::BackedOut { status, .. } if status == "RA"
                ));
                let result = invoke_call(&service, &store, &next, 9, restart());
                assert!(
                    matches!(
                        result, ImsRecoveryResult::Restarted { pcb_statuses, .. }
                        if pcb_statuses.contains(&(2, if change == "data" { "  " } else { "GE" }.into()))
                    ),
                    "boundary {boundary}, change {change}"
                );
                assert_eq!(
                    nav(
                        &service,
                        &next,
                        1,
                        ImsOperation::GetUnique,
                        &[b"ROOT    (KEY     EQ02)"]
                    )
                    .segments[0]
                        .data,
                    b"02B"
                );
            });
        }
    }
}

#[test]
fn one_checkpoint_restores_primary_secondary_and_formatted_gsam_positions() {
    use mainframe_env_host_api::{
        ImsGsamAccessMethod, ImsGsamControl, ImsGsamFormat, ImsGsamRecordFormat, ImsGsamRequest,
        ImsGsamSearchArgument,
    };

    for kind in [ImsGsamRecordFormat::V, ImsGsamRecordFormat::U] {
        backends(&format!("joint-secondary-gsam-{kind:?}"), |store| {
            let mut metadata = indexed_catalog();
            let mut file = catalog().databases.remove(0);
            file.name = "FILEDB".into();
            file.organization = ImsDatabaseOrganization::Gsam;
            file.segments[0].fields.clear();
            file.segments[0].min_length = if kind == ImsGsamRecordFormat::U {
                12
            } else {
                2
            };
            file.segments[0].max_length = 16;
            file.gsam_format = Some(ImsGsamFormat {
                version: 1,
                record_format: kind,
                access_method: ImsGsamAccessMethod::Bsam,
                block_size: 32,
                control: ImsGsamControl::None,
            });
            metadata.databases.push(file);
            let ImsPcbMetadata::Database(mut input) = catalog().psbs.remove(0).pcbs.remove(0)
            else {
                unreachable!()
            };
            input.name = "FILEIN".into();
            input.database = "FILEDB".into();
            input.processing_options = "G".into();
            let mut output = input.clone();
            output.name = "FILEOUT".into();
            output.processing_options = "L".into();
            metadata.psbs[0].pcbs.extend([
                ImsPcbMetadata::Database(input),
                ImsPcbMetadata::Database(output),
            ]);
            let service = open_catalog(store.clone(), metadata);
            let first = invocation();
            setup(&service, &store, &first);
            let file_call = |seq, op, pcb, data: &[u8]| {
                let mut request = database_request(op, seq, data);
                request.pcb = pcb;
                request.segments.clear();
                ImsGsamRequest {
                    undefined_length: (kind == ImsGsamRecordFormat::U
                        && op == ImsOperation::Insert)
                        .then_some(data.len() as u32),
                    request,
                    context: ImsExecutionContext::DbBatch,
                    save_address: true,
                    search: None,
                }
            };
            let (first_area, second_area): (&[u8], &[u8]) = if kind == ImsGsamRecordFormat::U {
                (b"0123456789AB", b"0123456789ABCDEF")
            } else {
                (&[0, 4, 0, 255], &[0, 5, 65, 0, 255])
            };
            service
                .execute_gsam(&first, &file_call(501, ImsOperation::Insert, 5, first_area))
                .unwrap();
            let second_address = service
                .execute_gsam(
                    &first,
                    &file_call(502, ImsOperation::Insert, 5, second_area),
                )
                .unwrap()
                .address
                .unwrap();
            service
                .execute_gsam(&first, &file_call(503, ImsOperation::GetNext, 4, b""))
                .unwrap();
            nav(
                &service,
                &first,
                1,
                ImsOperation::GetUnique,
                &[b"ROOT    (KEY     EQ01)"],
            );
            nav(
                &service,
                &first,
                2,
                ImsOperation::GetUnique,
                &[b"ROOT    (BYCOMP  EQ\0\xff)"],
            );
            nav(
                &service,
                &first,
                3,
                ImsOperation::GetUnique,
                &[b"ROOT    (BYCOMP  EQ\xff\0)"],
            );
            invoke_call(&service, &store, &first, 20, symbolic());
            let row: serde_json::Value =
                serde_json::from_slice(&recovery_rows(&*store)[0].payload).unwrap();
            let positions = row["checkpoints"]["SECPOS"]["request"]["positions"]
                .as_array()
                .unwrap();
            assert_eq!(positions.len(), 5);
            assert!(positions[0]["gsam"].is_null() && positions[0]["secondary"].is_null());
            assert!(positions[1]["secondary"].is_object() && positions[1]["gsam_format"].is_null());
            assert!(positions[2]["secondary"].is_object());
            assert!(positions[3]["gsam"].is_object() && positions[3]["secondary"].is_null());
            assert_eq!(positions[3]["gsam_format"], positions[4]["gsam_format"]);
            let suffix = service
                .execute_gsam(&first, &file_call(504, ImsOperation::Insert, 5, first_area))
                .unwrap()
                .address
                .unwrap();
            drop(service);
            let fresh =
                ImsService::open_authorized(store.clone(), Default::default(), Arc::new(Allow))
                    .unwrap();
            let next = next_execution(&first, "joint-restart");
            assert!(
                matches!(invoke_call(&fresh, &store, &next, 21, restart()), ImsRecoveryResult::Restarted { pcb_statuses, .. } if pcb_statuses == (1..=5).map(|n| (n, "  ".into())).collect::<Vec<_>>())
            );
            assert_eq!(
                nav(&fresh, &next, 1, ImsOperation::GetNext, &[b"ROOT     "]).segments[0].data,
                b"02B"
            );
            assert_eq!(
                nav(
                    &fresh,
                    &next,
                    2,
                    ImsOperation::GetNextParent,
                    &[b"CHILD    "]
                )
                .segments[0]
                    .data,
                b"B1\0\xff"
            );
            assert_eq!(
                nav(
                    &fresh,
                    &next,
                    3,
                    ImsOperation::GetNextParent,
                    &[b"CHILD    "]
                )
                .segments[0]
                    .data,
                b"A1\xff\0"
            );
            let found = fresh
                .execute_gsam(&next, &file_call(505, ImsOperation::GetNext, 4, b""))
                .unwrap();
            assert_eq!(found.address, Some(second_address));
            assert_eq!(found.result.segments[0].data, second_area);
            assert_eq!(
                found.undefined_length,
                (kind == ImsGsamRecordFormat::U).then_some(second_area.len() as u32)
            );
            let mut stale = file_call(506, ImsOperation::GetUnique, 4, b"");
            stale.save_address = false;
            stale.search = Some(ImsGsamSearchArgument::Record(suffix));
            assert_eq!(
                fresh.execute_gsam(&next, &stale).unwrap().result.status,
                "AJ"
            );
        });
    }
}
