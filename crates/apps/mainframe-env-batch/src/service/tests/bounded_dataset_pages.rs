//! Owning-provider and transport-integrity regressions. All UNRUN at authoring.
use super::*;
use crate::dd_hydration::{complete_dataset_records, refuse_implicit_repro_prefix};

pub(super) fn test_provider_page(
    state: &Mutex<BTreeMap<String, Vec<Vec<u8>>>>,
    dataset: &DatasetName,
    _member: Option<&MemberName>,
    start: Option<u64>,
    reverse: bool,
    max_records: u32,
) -> Result<DatasetResult, HostProblem> {
    // Keep the old fake provider's flattened member-fixture convention.
    // Real member semantics are exercised separately below.
    if reverse {
        return Err(HostProblem::Unsupported);
    }
    let state = state
        .lock()
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let records = state.get(dataset.as_str()).ok_or(HostProblem::NotFound)?;
    let start = usize::try_from(start.unwrap_or(0)).map_err(|_| HostProblem::ResourceExhausted)?;
    if !records.is_empty() && start >= records.len() {
        return Err(HostProblem::Condition {
            name: "NOTFND".into(),
            response: 13,
            response2: 0,
        });
    }
    let selected = records
        .iter()
        .enumerate()
        .skip(start)
        .take(max_records as usize);
    let pairs = selected
        .map(|(i, r)| (r.clone(), (i as u64).to_be_bytes().to_vec()))
        .collect::<Vec<_>>();
    Ok(DatasetResult::Records {
        records: pairs.iter().map(|(r, _)| r.clone()).collect(),
        identities: pairs.into_iter().map(|(_, i)| i).collect(),
        version: 1,
    })
}

fn fixture_attributes() -> DatasetAttributes {
    DatasetAttributes {
        organization: DatasetOrganization::Sequential,
        record_format: RecordFormat::Variable,
        logical_record_length: 256,
        key_offset: None,
        key_length: None,
        ccsid: Some(37),
    }
}
fn fixture_records(count: usize) -> Vec<Vec<u8>> {
    (0..count)
        .map(|i| format!("R{i:07}").into_bytes())
        .collect()
}
fn page_response(
    request: DatasetRequest,
    records: &[Vec<u8>],
) -> Result<DatasetResult, HostProblem> {
    match request {
        DatasetRequest::Attributes { .. } => Ok(DatasetResult::Attributes {
            attributes: fixture_attributes(),
            version: 1,
        }),
        DatasetRequest::ReadSequential {
            start,
            reverse,
            max_records,
            ..
        } => {
            assert!(!reverse);
            assert_eq!(max_records, 4096);
            let start = start.unwrap() as usize;
            if !records.is_empty() && start >= records.len() {
                return Err(HostProblem::Condition {
                    name: "NOTFND".into(),
                    response: 13,
                    response2: 0,
                });
            }
            let pairs = records
                .iter()
                .enumerate()
                .skip(start)
                .take(max_records as usize)
                .collect::<Vec<_>>();
            Ok(DatasetResult::Records {
                records: pairs.iter().map(|(_, r)| (*r).clone()).collect(),
                identities: pairs
                    .iter()
                    .map(|(i, _)| (*i as u64).to_be_bytes().to_vec())
                    .collect(),
                version: 1,
            })
        }
        _ => panic!("unexpected transport request"),
    }
}

#[test]
fn complete_dataset_pages_preserve_full_bytes_at_exact_page_and_provider_boundaries() {
    let dataset = DatasetName::new("IBMUSER.INPUT", 128).unwrap();
    for count in [0, 1, 4095, 4096, 4097, 5000, 65536] {
        let expected = fixture_records(count);
        let mut requests = Vec::new();
        let actual = complete_dataset_records(
            &dataset,
            None,
            true,
            64 * 1024 * 1024,
            || Ok(()),
            |request| {
                requests.push(request.clone());
                page_response(request, &expected)
            },
        )
        .unwrap()
        .into_records()
        .unwrap();
        assert_eq!(actual, expected, "input{count}");
        assert!(matches!(
            requests.first(),
            Some(DatasetRequest::Attributes { .. })
        ));
        assert!(matches!(
            requests.last(),
            Some(DatasetRequest::Attributes { .. })
        ));
        if count > 0 && count % 4096 == 0 {
            assert!(requests.iter().any(|request|matches!(request,DatasetRequest::ReadSequential {start:Some(n),..} if *n==count as u64)));
        }
    }
    let too_many = fixture_records(65537);
    assert_eq!(
        complete_dataset_records(
            &dataset,
            None,
            true,
            64 * 1024 * 1024,
            || Ok(()),
            |request| page_response(request, &too_many)
        ),
        Err(HostProblem::ResourceExhausted)
    );
}

#[test]
fn complete_dataset_pages_refuse_changed_versions_geometry_and_duplicate_identities() {
    let dataset = DatasetName::new("IBMUSER.INPUT", 128).unwrap();
    let expected = fixture_records(5000);
    for corruption in [
        "later-version",
        "final-version",
        "final-layout",
        "identity",
        "duplicate",
        "oversized-page",
    ] {
        let mut attrs = 0;
        let result = complete_dataset_records(
            &dataset,
            None,
            true,
            64 * 1024 * 1024,
            || Ok(()),
            |request| {
                let later =
                    matches!(&request,DatasetRequest::ReadSequential{start:Some(n),..} if *n>0);
                let is_attrs = matches!(&request, DatasetRequest::Attributes { .. });
                let mut value = page_response(request, &expected)?;
                if is_attrs {
                    attrs += 1;
                }
                match &mut value {
                    DatasetResult::Records { version, .. }
                        if corruption == "later-version" && later =>
                    {
                        *version = 2
                    }
                    DatasetResult::Attributes { version, .. }
                        if corruption == "final-version" && attrs == 2 =>
                    {
                        *version = 2
                    }
                    DatasetResult::Attributes { attributes, .. }
                        if corruption == "final-layout" && attrs == 2 =>
                    {
                        attributes.ccsid = Some(1047)
                    }
                    DatasetResult::Records { identities, .. } if corruption == "identity" => {
                        identities[0] = vec![0]
                    }
                    DatasetResult::Records { identities, .. }
                        if corruption == "duplicate" && later =>
                    {
                        identities[0] = 0u64.to_be_bytes().to_vec()
                    }
                    DatasetResult::Records {
                        records,
                        identities,
                        ..
                    } if corruption == "oversized-page" && !later => {
                        records.push(b"EXTRA".to_vec());
                        identities.push(4096u64.to_be_bytes().to_vec());
                    }
                    _ => {}
                };
                Ok(value)
            },
        );
        assert!(
            matches!(
                result,
                Err(HostProblem::IdempotencyConflict | HostProblem::ProviderFailure)
            ),
            "corruption{corruption}"
        );
    }
}

#[test]
fn complete_dataset_pages_keep_eof_errors_cancellation_and_storage_refusal_strict() {
    let dataset = DatasetName::new("IBMUSER.INPUT", 128).unwrap();
    let expected = fixture_records(4096);
    for problem in [
        HostProblem::NotFound,
        HostProblem::UnknownOutcome,
        HostProblem::Condition {
            name: "NOTFND".into(),
            response: 13,
            response2: 1,
        },
    ] {
        let wanted = problem.clone();
        assert_eq!(
            complete_dataset_records(
                &dataset,
                None,
                true,
                64 * 1024 * 1024,
                || Ok(()),
                |request| {
                    if matches!(&request,DatasetRequest::ReadSequential{start:Some(n),..} if *n>0) {
                        Err(problem.clone())
                    } else {
                        page_response(request, &expected)
                    }
                }
            ),
            Err(wanted)
        );
    }
    assert_eq!(
        complete_dataset_records(
            &dataset,
            None,
            true,
            64 * 1024 * 1024,
            || Err(HostProblem::UnknownOutcome),
            |_| panic!("effect after cancellation")
        ),
        Err(HostProblem::UnknownOutcome)
    );
    assert_eq!(
        complete_dataset_records(
            &dataset,
            None,
            true,
            1,
            || Ok(()),
            |request| page_response(request, &expected)
        ),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(
        complete_dataset_records(
            &dataset,
            None,
            false,
            64 * 1024 * 1024,
            || Ok(()),
            |request| page_response(request, &expected)
        ),
        Err(HostProblem::ResourceExhausted)
    );
}

#[test]
fn implicit_repro_capacity_refuses_hidden_tail_and_keeps_explicit_selection() {
    assert_eq!(
        refuse_implicit_repro_prefix(5000, 0, 4096, false),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(
        refuse_implicit_repro_prefix(4097, 0, 4096, false),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(refuse_implicit_repro_prefix(4096, 0, 4096, false), Ok(()));
    assert_eq!(
        refuse_implicit_repro_prefix(5000, 1000, 4096, false),
        Ok(())
    );
    assert_eq!(refuse_implicit_repro_prefix(5000, 0, 4096, true), Ok(()));
    assert_eq!(refuse_implicit_repro_prefix(5000, 0, 0, true), Ok(()));
}

#[test]
fn real_provider_hydration_offers_all5000_records_under_the_owned_dd_lock() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let dataset = DatasetService::open(store.clone(), DatasetLimits::default()).unwrap();
    let expected = fixture_records(5000);
    seed_real_dataset(&dataset, "IBMUSER.INPUT", expected.clone(), 80000);
    let captured = Arc::new(Mutex::new(Vec::new()));
    let program = Arc::new(ProgramInputCapture {
        inner: builtins(),
        captured: captured.clone(),
    });
    let mut providers = dataset_providers(dataset, InvocationLimits::default());
    providers.extend(spool_test_providers(store.clone()));
    let service = BatchService::open(
        host_with(program, providers),
        store,
        Default::default(),
        Default::default(),
    )
    .unwrap();
    let invocation = invocation();
    service.submit(&invocation,&JclBundle {primary:"//PAGES JOB CLASS=A\n//S EXEC PGM=IEFBR14\n//INPUT DD DSN=IBMUSER.INPUT,DISP=SHR\n".into(),..Default::default()},
        &IdempotencyKey::new("real-paged-input",InvocationLimits::default()).unwrap(),false).unwrap();
    let completed = service.run_next(&invocation, false).unwrap().unwrap();
    assert_eq!(completed.state, JobState::Completed);
    assert_eq!(completed.return_code, Some(0));
    let values = captured.lock().unwrap();
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].dd_records["INPUT"], expected);
}

#[test]
fn real_provider_repro_without_count_refuses5000_before_any_output_write() {
    let (service, dataset) = service_with_real_datasets();
    let expected = fixture_records(5000);
    seed_real_dataset(&dataset, "IBMUSER.INPUT", expected.clone(), 81000);
    seed_real_dataset(
        &dataset,
        "IBMUSER.TARGET",
        vec![b"UNCHANGED".to_vec()],
        81002,
    );
    let invocation = invocation();
    service.submit(&invocation,&JclBundle {primary:"//BIGCOPY JOB CLASS=A\n//S EXEC PGM=IDCAMS\n//INPUT DD DSN=IBMUSER.INPUT,DISP=SHR\n//OUTPUT DD DSN=IBMUSER.TARGET,DISP=OLD\n//SYSIN DD *\n REPRO INFILE(INPUT) OUTFILE(OUTPUT)\n/*\n".into(),..Default::default()},
        &IdempotencyKey::new("no-silent-repro-prefix",InvocationLimits::default()).unwrap(),false).unwrap();
    let completed = service.run_next(&invocation, false).unwrap().unwrap();
    assert_eq!(completed.return_code, Some(16));
    let DatasetResult::Records { records, .. } = dataset
        .invoke(DatasetRequest::Read {
            dataset: DatasetName::new("IBMUSER.TARGET", 128).unwrap(),
            member: None,
            key: None,
            max_records: 8,
            control: Default::default(),
        })
        .unwrap()
    else {
        panic!("wrong result")
    };
    assert_eq!(records, vec![b"UNCHANGED".to_vec()]);
    let DatasetResult::Records { records, .. } = dataset
        .invoke(DatasetRequest::Read {
            dataset: DatasetName::new("IBMUSER.INPUT", 128).unwrap(),
            member: None,
            key: None,
            max_records: 5001,
            control: Default::default(),
        })
        .unwrap()
    else {
        panic!("wrong result")
    };
    assert_eq!(records, expected);
}

#[test]
fn real_provider_member_concatenation_keeps_first_found_library_member() {
    let (service, dataset) = service_with_real_datasets();
    let selected = MemberName::new("SELECTED", 8).unwrap();
    for (i, name) in ["IBMUSER.LIBONE", "IBMUSER.LIBTWO", "IBMUSER.LIBTHR"]
        .into_iter()
        .enumerate()
    {
        let dataset_name = DatasetName::new(name, 128).unwrap();
        let mut attributes = fixture_attributes();
        attributes.organization = DatasetOrganization::Partitioned;
        let DatasetResult::Created { version } = dataset
            .invoke(DatasetRequest::Create {
                dataset: dataset_name.clone(),
                attributes,
                mutation: dataset_test_mutation(82000 + i as u64 * 2),
            })
            .unwrap()
        else {
            panic!("wrong library create result")
        };
        dataset
            .invoke(DatasetRequest::Write {
                dataset: dataset_name,
                member: Some(if i == 0 {
                    MemberName::new("OTHER", 8).unwrap()
                } else {
                    selected.clone()
                }),
                records: if i == 1 {
                    vec![b"FIRST".to_vec(), b"MEMBER".to_vec()]
                } else {
                    vec![b"UNSELECTED".to_vec()]
                },
                expected_version: Some(version),
                mutation: dataset_test_mutation(82001 + i as u64 * 2),
            })
            .unwrap();
    }
    seed_real_dataset(&dataset, "IBMUSER.TARGET", vec![b"STALE".to_vec()], 82006);
    let invocation = invocation();
    service.submit(&invocation,&JclBundle{
        primary:"//MEMCOPY JOB CLASS=A\n//S EXEC PGM=IEBGENER\n//SYSUT1 DD DSN=IBMUSER.LIBONE(SELECTED),DISP=SHR\n// DD DSN=IBMUSER.LIBTWO(SELECTED),DISP=SHR\n// DD DSN=IBMUSER.LIBTHR(SELECTED),DISP=SHR\n//SYSUT2 DD DSN=IBMUSER.TARGET,DISP=OLD\n".into(),..Default::default()},
        &IdempotencyKey::new("paged-member-concat",InvocationLimits::default()).unwrap(),false).unwrap();
    let completed = service.run_next(&invocation, false).unwrap().unwrap();
    assert_eq!(completed.state, JobState::Completed);
    assert_eq!(completed.return_code, Some(0));
    let DatasetResult::Records { records, .. } = dataset
        .invoke(DatasetRequest::Read {
            dataset: DatasetName::new("IBMUSER.TARGET", 128).unwrap(),
            member: None,
            key: None,
            max_records: 8,
            control: Default::default(),
        })
        .unwrap()
    else {
        panic!("wrong target read result")
    };
    assert_eq!(records, vec![b"FIRST".to_vec(), b"MEMBER".to_vec()]);
    for name in [
        "IBMUSER.LIBONE",
        "IBMUSER.LIBTWO",
        "IBMUSER.LIBTHR",
        "IBMUSER.TARGET",
    ] {
        assert!(matches!(dataset.invoke(DatasetRequest::ListLocks{
            dataset:DatasetName::new(name,128).unwrap(),now_tick:1,max_items:8,
        }),Ok(DatasetResult::Locks{locks}) if locks.is_empty()));
    }
}

#[test]
fn confirmed_missing_member_is_distinct_from_missing_or_changed_dataset() {
    use crate::dd_hydration::CompleteDatasetRead;
    let dataset = DatasetName::new("IBMUSER.LIB", 128).unwrap();
    let member = MemberName::new("SELECTED", 8).unwrap();
    for scenario in [
        "member-missing",
        "initial-dataset-missing",
        "later-dataset-missing",
        "version-changed",
        "layout-changed",
        "different-page-error",
        "not-a-library",
    ] {
        let mut attributes_reads = 0;
        let result = complete_dataset_records(
            &dataset,
            Some(&member),
            true,
            64 * 1024 * 1024,
            || Ok(()),
            |request| match request {
                DatasetRequest::Attributes { .. } => {
                    attributes_reads += 1;
                    if scenario == "initial-dataset-missing"
                        || scenario == "later-dataset-missing" && attributes_reads == 2
                    {
                        return Err(HostProblem::NotFound);
                    }
                    let mut attributes = fixture_attributes();
                    if scenario != "not-a-library" {
                        attributes.organization = DatasetOrganization::Partitioned;
                    }
                    if scenario == "layout-changed" && attributes_reads == 2 {
                        attributes.ccsid = Some(1047);
                    }
                    Ok(DatasetResult::Attributes {
                        attributes,
                        version: if scenario == "version-changed" && attributes_reads == 2 {
                            2
                        } else {
                            1
                        },
                    })
                }
                DatasetRequest::ReadSequential { start, .. } => {
                    assert_eq!(start, Some(0));
                    if scenario == "different-page-error" {
                        Err(HostProblem::UnknownOutcome)
                    } else {
                        Err(HostProblem::NotFound)
                    }
                }
                _ => panic!("unexpected request"),
            },
        );
        match scenario {
            "member-missing" => {
                assert_eq!(result, Ok(CompleteDatasetRead::MissingMember));
                assert_eq!(attributes_reads, 2);
            }
            "version-changed" | "layout-changed" => {
                assert_eq!(result, Err(HostProblem::IdempotencyConflict))
            }
            "different-page-error" => assert_eq!(result, Err(HostProblem::UnknownOutcome)),
            _ => assert_eq!(result, Err(HostProblem::NotFound)),
        }
    }
    assert_eq!(
        CompleteDatasetRead::MissingMember.into_records(),
        Err(HostProblem::NotFound)
    );
}
