use super::*;

fn create_sequential(dataset: &DatasetService, name: &DatasetName) {
    assert_eq!(
        dataset.invoke(DatasetRequest::Create {
            dataset: name.clone(),
            attributes: attrs(DatasetOrganization::Sequential),
            mutation: mutation(1),
        }),
        Ok(DatasetResult::Created { version: 1 })
    );
}

fn append(name: &DatasetName, version: u64, records: Vec<Vec<u8>>) -> DatasetRequest {
    DatasetRequest::Append {
        dataset: name.clone(),
        member: None,
        records,
        expected_version: Some(version),
        mutation: mutation(version + 1),
    }
}

fn persisted(store: &dyn ProviderStateStore, name: &DatasetName) -> (u64, Vec<u8>) {
    let row = store
        .get_provider_state("dataset", name.as_str())
        .unwrap()
        .unwrap();
    (row.version, row.payload)
}

fn assert_records(
    dataset: &DatasetService,
    name: &DatasetName,
    version: u64,
    expected: &[Vec<u8>],
) {
    assert!(matches!(
        dataset.invoke(DatasetRequest::Read {
            dataset: name.clone(),
            member: None,
            key: None,
            max_records: u32::try_from(expected.len().max(1)).unwrap(),
            control: Default::default(),
        }),
        Ok(DatasetResult::Records { records, version: actual_version, .. })
            if actual_version == version && records == expected
    ));
}

#[test]
fn memory_append_quota_and_replay_are_exact() {
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    let dataset = DatasetService::open(
        store.clone(),
        DatasetLimits {
            max_records: 2,
            max_record_bytes: 4,
            max_total_bytes: 8,
            ..Default::default()
        },
    )
    .unwrap();
    let name = DatasetName::new("APPEND.QUOTA", 44).unwrap();
    let mut definition = mainframe_env_host_api::DatasetDefinition::compatibility(attrs(
        DatasetOrganization::Sequential,
    ));
    definition.dcb.buffer_count = 2;
    definition.dcb.buffer_size = Some(4);
    assert_eq!(
        dataset.invoke(DatasetRequest::Define {
            dataset: name.clone(),
            definition: Box::new(definition),
            mutation: mutation(1),
        }),
        Ok(DatasetResult::Created { version: 1 })
    );
    let records = vec![b"AA11".to_vec(), b"BB22".to_vec()];
    let request = append(&name, 1, records.clone());
    assert_eq!(
        dataset.invoke(request.clone()),
        Ok(DatasetResult::Mutated { version: 2 })
    );
    let committed = persisted(&*store, &name);
    assert_eq!(
        dataset.invoke(request),
        Ok(DatasetResult::Mutated { version: 2 })
    );
    assert_eq!(persisted(&*store, &name), committed);

    assert_eq!(
        dataset.invoke(append(&name, 2, vec![b"CC33".to_vec()])),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(persisted(&*store, &name), committed);
    assert_records(&dataset, &name, 2, &records);

    let mut stale = append(&name, 1, vec![b"DD44".to_vec()]);
    if let DatasetRequest::Append {
        mutation: value, ..
    } = &mut stale
    {
        *value = mutation(4);
    }
    assert_eq!(dataset.invoke(stale), Err(HostProblem::IdempotencyConflict));
    assert_eq!(persisted(&*store, &name), committed);
    assert_records(&dataset, &name, 2, &records);
}

#[test]
fn sqlite_append_physical_reopen_keeps_records_versions_and_replay() {
    let directory = std::env::temp_dir().join(format!(
        "mainframe-env-dataset-append-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("dataset.db");
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let name = DatasetName::new("APPEND.SQLITE", 44).unwrap();
    let records = (0..512_u32)
        .map(|value| value.to_be_bytes().to_vec())
        .collect::<Vec<_>>();
    let last = append(&name, 512, vec![records[511].clone()]);
    let committed;
    {
        let store: Arc<dyn ProviderStateStore> =
            Arc::new(SqliteStateStore::open(&url, 2 * 1024 * 1024, 65536).unwrap());
        let dataset = service(store.clone());
        create_sequential(&dataset, &name);
        for (index, record) in records.iter().enumerate() {
            let version = index as u64 + 1;
            assert_eq!(
                dataset.invoke(append(&name, version, vec![record.clone()])),
                Ok(DatasetResult::Mutated {
                    version: version + 1
                })
            );
        }
        assert_records(&dataset, &name, 513, &records);
        committed = persisted(&*store, &name);
        assert_eq!(committed.0, 513);
        assert!(committed.1.starts_with(b"MEDS6"));
    }
    // Both the service and its physical SQLite store have been dropped.
    {
        let store: Arc<dyn ProviderStateStore> =
            Arc::new(SqliteStateStore::open(&url, 2 * 1024 * 1024, 65536).unwrap());
        let dataset = service(store.clone());
        assert_eq!(persisted(&*store, &name), committed);
        assert_records(&dataset, &name, 513, &records);
        assert_eq!(
            dataset.invoke(last),
            Ok(DatasetResult::Mutated { version: 513 })
        );
        assert_eq!(persisted(&*store, &name), committed);
        assert_records(&dataset, &name, 513, &records);
    }
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(directory).unwrap();
}

#[test]
fn append_atomic_failure_and_lost_ack_preserve_state() {
    let store = Arc::new(FailAtomicOnceStore::new());
    let dataset = service(store.clone());
    let name = DatasetName::new("APPEND.ATOMIC", 44).unwrap();
    create_sequential(&dataset, &name);
    assert_eq!(
        dataset.invoke(append(&name, 1, vec![b"AA11".to_vec()])),
        Ok(DatasetResult::Mutated { version: 2 })
    );
    let committed = persisted(&*store, &name);
    let request = append(&name, 2, vec![b"BB22".to_vec()]);
    store.arm();
    assert_eq!(
        dataset.invoke(request.clone()),
        Err(HostProblem::UnknownOutcome)
    );
    assert_eq!(persisted(&*store, &name), committed);
    assert_records(&dataset, &name, 2, &[b"AA11".to_vec()]);
    assert_eq!(
        dataset.invoke(request.clone()),
        Ok(DatasetResult::Mutated { version: 3 })
    );
    let retried = persisted(&*store, &name);
    assert_eq!(
        dataset.invoke(request),
        Ok(DatasetResult::Mutated { version: 3 })
    );
    assert_eq!(persisted(&*store, &name), retried);

    let request = append(&name, 3, vec![b"CC33".to_vec()]);
    store.arm_after_commit();
    // Read-back resolves a commit whose acknowledgement was lost.
    assert_eq!(
        dataset.invoke(request.clone()),
        Ok(DatasetResult::Mutated { version: 4 })
    );
    let committed = persisted(&*store, &name);
    assert_eq!(
        dataset.invoke(request),
        Ok(DatasetResult::Mutated { version: 4 })
    );
    assert_eq!(persisted(&*store, &name), committed);
    let records = [b"AA11".to_vec(), b"BB22".to_vec(), b"CC33".to_vec()];
    assert_records(&dataset, &name, 4, &records);
    drop(dataset);
    let reopened = service(store.clone());
    assert_eq!(persisted(&*store, &name), committed);
    assert_records(&reopened, &name, 4, &records);
}
