use super::*;
use crate::service::DatasetService;
use mainframe_env_execution_api::{IdempotencyKey, InvocationLimits};
use mainframe_env_host_api::{DatasetAttributes, DatasetRequest, Mutation, RecordFormat};
use mainframe_env_store::MemoryStore;
use std::sync::Arc;

fn mutation(sequence: u64) -> Mutation {
    Mutation {
        sequence,
        idempotency_key: IdempotencyKey::new(
            format!("position-{sequence}"),
            InvocationLimits::default(),
        )
        .unwrap(),
        transaction: None,
    }
}

fn name(value: &str) -> DatasetName {
    DatasetName::new(value, 128).unwrap()
}

fn fixture(
    rows: &[&[u8]],
    organization: DatasetOrganization,
) -> (Arc<DatasetService>, DatasetName) {
    let service = DatasetService::open(
        Arc::new(MemoryStore::new(Default::default())),
        DatasetLimits::default(),
    )
    .unwrap();
    let dataset = name("REVFILE");
    service
        .invoke(DatasetRequest::Create {
            dataset: dataset.clone(),
            attributes: DatasetAttributes {
                organization,
                record_format: RecordFormat::Fixed,
                logical_record_length: 4,
                key_offset: (organization == DatasetOrganization::KeySequenced).then_some(0),
                key_length: (organization == DatasetOrganization::KeySequenced).then_some(2),
                ccsid: None,
            },
            mutation: mutation(1),
        })
        .unwrap();
    service
        .invoke(DatasetRequest::Write {
            dataset: dataset.clone(),
            member: None,
            records: rows.iter().map(|row| row.to_vec()).collect(),
            expected_version: None,
            mutation: mutation(2),
        })
        .unwrap();
    (service, dataset)
}

fn start(service: &DatasetService, dataset: &DatasetName, key: &[u8]) -> String {
    match service
        .invoke(DatasetRequest::StartBrowse {
            dataset: dataset.clone(),
            key: key.to_vec(),
            relation: KeyRelation::GreaterOrEqual,
        })
        .unwrap()
    {
        DatasetResult::Browse {
            cursor,
            record: None,
            identity: None,
            key: None,
        } => cursor,
        result => panic!("unexpected start result: {result:?}"),
    }
}

fn positioned(
    service: &DatasetService,
    dataset: &DatasetName,
    cursor: &str,
    key: &[u8],
) -> Result<DatasetResult, HostProblem> {
    service.invoke(DatasetRequest::ReadBrowsePosition {
        dataset: dataset.clone(),
        cursor: cursor.into(),
        expected_key: key.to_vec(),
    })
}

fn literal(cursor: &str, record: &[u8], key: &[u8], identity: &[u8]) -> DatasetResult {
    DatasetResult::Browse {
        cursor: cursor.into(),
        record: Some(record.to_vec()),
        identity: Some(identity.to_vec()),
        key: Some(key.to_vec()),
    }
}

fn snapshot(service: &DatasetService, cursor: &str) -> (usize, isize, Vec<BrowseIdentity>) {
    let state = service.state.lock().unwrap();
    let active = state.cursors.get(cursor).unwrap();
    (state.cursors.len(), active.index, active.identities.clone())
}

fn reverse(service: &DatasetService, dataset: &DatasetName, cursor: &str) -> DatasetResult {
    service
        .invoke(DatasetRequest::ReadNext {
            dataset: dataset.clone(),
            cursor: cursor.into(),
            reverse: true,
            control: Default::default(),
        })
        .unwrap()
}

#[test]
fn read_browse_position_observes_anchor_without_advancing_gap() {
    let (service, dataset) = fixture(
        &[b"AA01", b"BB02", b"CC03"],
        DatasetOrganization::KeySequenced,
    );
    let cursor = start(&service, &dataset, b"BB");
    let before = snapshot(&service, &cursor);
    assert_eq!(before.0, 1);
    assert_eq!(before.1, 1);
    for _ in 0..2 {
        assert_eq!(
            positioned(&service, &dataset, &cursor, b"BB"),
            Ok(literal(&cursor, b"BB02", b"BB", b"BB"))
        );
        assert_eq!(snapshot(&service, &cursor), before);
    }
    assert_eq!(
        reverse(&service, &dataset, &cursor),
        literal(&cursor, b"AA01", b"AA", b"AA")
    );
}

#[test]
fn read_browse_position_refuses_wrong_owner_and_key_width_without_changes() {
    let (service, dataset) = fixture(
        &[b"AA01", b"BB02", b"CC03"],
        DatasetOrganization::KeySequenced,
    );
    let cursor = start(&service, &dataset, b"BB");
    let before = snapshot(&service, &cursor);
    for (owner, id) in [
        (dataset.clone(), "MISSING"),
        (name("OTHER"), cursor.as_str()),
    ] {
        assert!(
            matches!(positioned(&service, &owner, id, b"BB"), Err(HostProblem::Condition { response: 16, ref name, .. }) if name == "INVREQ")
        );
        assert_eq!(snapshot(&service, &cursor), before);
    }
    for key in [&b"B"[..], &b"BBB"[..]] {
        assert_eq!(
            positioned(&service, &dataset, &cursor, key),
            Err(HostProblem::Malformed)
        );
        assert_eq!(snapshot(&service, &cursor), before);
    }
    assert_eq!(
        positioned(&service, &dataset, &cursor, b"BB"),
        Ok(literal(&cursor, b"BB02", b"BB", b"BB"))
    );
    let (sequential, unkeyed) = fixture(&[b"AA01", b"BB02"], DatasetOrganization::Sequential);
    let unkeyed_cursor = start(&sequential, &unkeyed, b"");
    let unkeyed_before = snapshot(&sequential, &unkeyed_cursor);
    assert_eq!(
        positioned(&sequential, &unkeyed, &unkeyed_cursor, b"BB"),
        Err(HostProblem::Unsupported)
    );
    assert_eq!(snapshot(&sequential, &unkeyed_cursor), unkeyed_before);
}

#[test]
fn read_browse_position_mismatch_or_deleted_anchor_does_not_reposition() {
    let (service, dataset) = fixture(
        &[b"AA01", b"BB02", b"CC03"],
        DatasetOrganization::KeySequenced,
    );
    let cursor = start(&service, &dataset, b"AB");
    let before = snapshot(&service, &cursor);
    assert!(
        matches!(positioned(&service, &dataset, &cursor, b"AB"), Err(HostProblem::Condition { response: 13, ref name, .. }) if name == "NOTFND")
    );
    assert_eq!(snapshot(&service, &cursor), before);
    assert_eq!(
        positioned(&service, &dataset, &cursor, b"BB"),
        Ok(literal(&cursor, b"BB02", b"BB", b"BB"))
    );
    service
        .invoke(DatasetRequest::DeleteRecord {
            dataset: dataset.clone(),
            key: b"BB".to_vec(),
            expected_version: None,
            mutation: mutation(3),
        })
        .unwrap();
    assert!(
        matches!(positioned(&service, &dataset, &cursor, b"BB"), Err(HostProblem::Condition { response: 13, ref name, .. }) if name == "NOTFND")
    );
    assert_eq!(snapshot(&service, &cursor), before);
}

#[test]
fn read_browse_position_retains_duplicate_base_identity_after_insert() {
    let (service, dataset) = fixture(
        &[b"AA01", b"BB02", b"CC02", b"DD03"],
        DatasetOrganization::KeySequenced,
    );
    let index = name("REVINDEX");
    service
        .invoke(DatasetRequest::DefineAlternateIndex {
            base: dataset.clone(),
            index: index.clone(),
            key_offset: 2,
            key_length: 2,
            allow_duplicates: true,
            upgrade: true,
            mutation: mutation(3),
        })
        .unwrap();
    service
        .invoke(DatasetRequest::BuildAlternateIndex {
            base: dataset.clone(),
            index: index.clone(),
            mutation: mutation(4),
        })
        .unwrap();
    let cursor = start(&service, &index, b"02");
    let before = snapshot(&service, &cursor);
    service
        .invoke(DatasetRequest::Write {
            dataset: dataset.clone(),
            member: None,
            records: vec![b"AB02".to_vec()],
            expected_version: None,
            mutation: mutation(5),
        })
        .unwrap();
    let rows = service
        .invoke(DatasetRequest::Read {
            dataset,
            member: None,
            key: None,
            max_records: 10,
            control: Default::default(),
        })
        .unwrap();
    match rows {
        DatasetResult::Records {
            records,
            identities,
            ..
        } => {
            assert_eq!(
                records,
                [
                    b"AA01".to_vec(),
                    b"AB02".to_vec(),
                    b"BB02".to_vec(),
                    b"CC02".to_vec(),
                    b"DD03".to_vec()
                ]
            );
            assert_eq!(
                identities,
                [
                    b"AA".to_vec(),
                    b"AB".to_vec(),
                    b"BB".to_vec(),
                    b"CC".to_vec(),
                    b"DD".to_vec()
                ]
            );
        }
        other => panic!("unexpected full fixture reply: {other:?}"),
    }
    assert_eq!(snapshot(&service, &cursor), before);
    assert_eq!(
        positioned(&service, &index, &cursor, b"02"),
        Ok(literal(&cursor, b"BB02", b"02", b"BB"))
    );
    assert_eq!(snapshot(&service, &cursor), before);
    assert_eq!(
        reverse(&service, &index, &cursor),
        literal(&cursor, b"AA01", b"01", b"AA")
    );
}

#[test]
fn read_browse_position_resolves_live_body_by_retained_identity() {
    let (service, dataset) = fixture(
        &[b"AA01", b"BB02", b"CC03"],
        DatasetOrganization::KeySequenced,
    );
    let cursor = start(&service, &dataset, b"BB");
    let before = snapshot(&service, &cursor);
    service
        .invoke(DatasetRequest::RewriteRecord {
            dataset: dataset.clone(),
            key: b"BB".to_vec(),
            record: b"BB99".to_vec(),
            expected_version: None,
            mutation: mutation(3),
        })
        .unwrap();
    assert_eq!(
        positioned(&service, &dataset, &cursor, b"BB"),
        Ok(literal(&cursor, b"BB99", b"BB", b"BB"))
    );
    assert_eq!(snapshot(&service, &cursor), before);
    assert_eq!(
        reverse(&service, &dataset, &cursor),
        literal(&cursor, b"AA01", b"AA", b"AA")
    );
}
