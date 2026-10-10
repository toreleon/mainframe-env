//! Synthetic byte/state refusal controls; no actual application credit.
use super::*;
use mainframe_env_host_api::CatalogListEntry;

const STAMP: &[u8; 26] = &[
    0xf2, 0xf0, 0xf2, 0xf6, 0x60, 0xf1, 0xf0, 0x60, 0xf1, 0xf0, 0x40, 0xf1, 0xf2, 0x7a, 0xf3, 0xf4,
    0x7a, 0xf5, 0xf6, 0x4b, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0,
];
const EPOCH: u64 = 1791635696;

fn state() -> State {
    let mut result = State::new();
    for name in inserted::NAMES {
        let core = *name == inserted::PRIMARY || *name == inserted::LOCAL_INDEX;
        let index = *name == inserted::LOCAL_INDEX;
        let mut rows = Rows {
            catalog: CatalogListEntry {
                name: DatasetName::new(*name, 128).unwrap(),
                kind: if index {
                    CatalogEntryKind::AlternateIndex
                } else {
                    CatalogEntryKind::Dataset
                },
                related: index.then(|| DatasetName::new(inserted::PRIMARY, 128).unwrap()),
                version: if core { 2 } else { 1 },
            },
            attributes: inserted::attributes(index),
            attributes_version: if core { 2 } else { 1 },
            records: if core {
                vec![inserted::BOOTSTRAP.to_vec(), inserted::PURCHASE.to_vec()]
            } else {
                vec![b"fixed unrelated".to_vec()]
            },
            identities: if core {
                vec![vec![0; 16], inserted::PURCHASE[..16].to_vec()]
            } else {
                vec![b"fixed key".to_vec()]
            },
            version: if core { 2 } else { 1 },
        };
        if *name == ACCOUNT {
            rows.attributes = DatasetAttributes {
                organization: DatasetOrganization::KeySequenced,
                record_format: RecordFormat::Fixed,
                logical_record_length: 300,
                key_offset: Some(0),
                key_length: Some(11),
                ccsid: Some(37),
            };
            rows.version = 7;
            rows.attributes_version = 7;
            rows.catalog.version = 7;
            rows.records = (1u8..50)
                .map(|number| {
                    let mut bytes = vec![0x40; 300];
                    let mut key = vec![0xf0; 11];
                    key[9] = 0xf0 + number / 10;
                    key[10] = 0xf0 + number % 10;
                    bytes[..11].copy_from_slice(&key);
                    bytes
                })
                .collect();
            rows.records.push(ACCOUNT_BEFORE.to_vec());
            rows.identities = rows
                .records
                .iter()
                .map(|bytes| bytes[..11].to_vec())
                .collect();
        } else if *name == inserted::XREF {
            rows.records = vec![inserted::SELECTED_XREF.to_vec()];
            rows.identities = vec![inserted::SELECTED_XREF[..16].to_vec()];
        }
        result.insert((*name).into(), rows);
    }
    result
}
fn paid(before: &State) -> State {
    let mut result = before.clone();
    let mut payment = PAYMENT_STATIC.to_vec();
    payment[278..304].copy_from_slice(STAMP);
    payment[304..330].copy_from_slice(STAMP);
    let primary = result.get_mut(inserted::PRIMARY).unwrap();
    primary.records.push(payment);
    primary.identities.push(KEY_TWO.to_vec());
    primary.version = 3;
    primary.attributes_version = 3;
    primary.catalog.version = 3;
    let records = primary.records.clone();
    let index = result.get_mut(inserted::LOCAL_INDEX).unwrap();
    index.records = records;
    index.records.sort_by(|left, right| {
        left[304..330]
            .cmp(&right[304..330])
            .then(left[..16].cmp(&right[..16]))
    });
    index.identities = index
        .records
        .iter()
        .map(|bytes| bytes[..16].to_vec())
        .collect();
    index.version = 3;
    index.attributes_version = 3;
    index.catalog.version = 3;
    let account = result.get_mut(ACCOUNT).unwrap();
    account.records[49] = ACCOUNT_PAID.to_vec();
    account.version += 1;
    account.attributes_version += 1;
    account.catalog.version += 1;
    result
}
fn encoded_time(literal: &[u8]) -> Vec<u8> {
    literal
        .iter()
        .map(|byte| match *byte {
            b'0'..=b'9' => 0xf0 + (*byte - b'0'),
            b'-' => 0x60,
            b' ' => 0x40,
            b':' => 0x7a,
            b'.' => 0x4b,
            _ => 0x00,
        })
        .collect()
}
fn trace() -> Vec<CicsTraceEntry> {
    [
        (CicsOperation::Read, 300),
        (CicsOperation::Read, 50),
        (CicsOperation::StartBrowse, 0),
        (CicsOperation::ReadPrev, 350),
        (CicsOperation::EndBrowse, 0),
        (CicsOperation::Asktime, 0),
        (CicsOperation::FormatTime, 0),
        (CicsOperation::Write, 0),
        (CicsOperation::Rewrite, 0),
    ]
    .into_iter()
    .map(|(operation, payload_bytes)| CicsTraceEntry {
        operation,
        outcome: "NORMAL".into(),
        response: 0,
        response2: 0,
        payload_bytes,
    })
    .collect()
}
#[test]
fn every_paid_account_and_payment_byte_including_filler_is_owned() {
    let before = state();
    let correct = paid(&before);
    assert!(compare_state(&before, &correct, EPOCH, EPOCH).is_ok());
    for offset in 0..300 {
        let mut after = correct.clone();
        after.get_mut(ACCOUNT).unwrap().records[49][offset] ^= 1;
        assert!(
            compare_state(&before, &after, EPOCH, EPOCH).is_err(),
            "account byte {offset}"
        );
    }
    for offset in 0..350 {
        let mut after = correct.clone();
        after.get_mut(inserted::PRIMARY).unwrap().records[2][offset] ^= 1;
        assert!(
            compare_state(&before, &after, EPOCH, EPOCH).is_err(),
            "payment byte {offset}"
        );
    }
    for which in 0..2 {
        for offset in 0..350 {
            let mut after = correct.clone();
            after.get_mut(inserted::PRIMARY).unwrap().records[which][offset] ^= 1;
            assert!(
                compare_state(&before, &after, EPOCH, EPOCH).is_err(),
                "old transaction {which} byte {offset}"
            );
        }
    }
}
#[test]
fn independent_utc_bounds_calendar_grammar_and_zero_microseconds_cannot_be_substituted() {
    assert_eq!(timestamp_second(STAMP, EPOCH, EPOCH).unwrap(), EPOCH);
    assert!(timestamp_second(STAMP, EPOCH + 1, EPOCH + 1).is_err());
    assert!(timestamp_second(STAMP, EPOCH - 1, EPOCH - 1).is_err());
    assert!(timestamp_second(STAMP, EPOCH + 1, EPOCH).is_err());
    assert!(timestamp_second(&STAMP[..25], EPOCH, EPOCH).is_err());
    assert!(timestamp_second(b"2026-10-10 12:34:56.000000", 0, u64::MAX).is_err());
    for literal in [
        b"2026-13-10 12:34:56.000000",
        b"2026-00-10 12:34:56.000000",
        b"2026-10-00 12:34:56.000000",
        b"2026-10-32 12:34:56.000000",
        b"2026-04-31 12:34:56.000000",
        b"2026-02-29 12:34:56.000000",
        b"2100-02-29 12:34:56.000000",
        b"2026-10-10 24:34:56.000000",
        b"2026-10-10 12:60:56.000000",
        b"2026-10-10 12:34:60.000000",
        b"2026-10-10 12:34:56.000001",
    ] {
        assert!(timestamp_second(&encoded_time(literal), 0, u64::MAX).is_err());
    }
    assert_eq!(
        timestamp_second(&encoded_time(b"1970-01-01 00:00:00.000000"), 0, 0).unwrap(),
        0
    );
    assert_eq!(
        timestamp_second(
            &encoded_time(b"2000-02-29 00:00:00.000000"),
            951_782_400,
            951_782_400
        )
        .unwrap(),
        951_782_400
    );
    assert_eq!(
        timestamp_second(
            &encoded_time(b"2024-02-29 12:34:56.000000"),
            1_709_210_096,
            1_709_210_096
        )
        .unwrap(),
        1_709_210_096
    );
    for offset in [4, 7, 10, 13, 16, 19, 20] {
        let mut wrong = STAMP.to_vec();
        wrong[offset] ^= 1;
        assert!(timestamp_second(&wrong, 0, u64::MAX).is_err());
    }
    let before = state();
    let mut after = paid(&before);
    after.get_mut(inserted::PRIMARY).unwrap().records[2][304] ^= 1;
    assert!(compare_state(&before, &after, EPOCH, EPOCH).is_err());
}
#[test]
fn wrong_raw_keys_revisions_catalog_layout_order_and_unrelated_state_refuse() {
    let before = state();
    let correct = paid(&before);
    for which in 0..12 {
        let mut after = correct.clone();
        let account = after.get_mut(ACCOUNT).unwrap();
        match which {
            0 => {
                let _ = account.records.pop();
            }
            1 => account.records.push(ACCOUNT_PAID.to_vec()),
            2 => account.records[0][20] ^= 1,
            3 => account.identities[49][0] ^= 1,
            4 => account.records.swap(0, 49),
            5 => account.version -= 1,
            6 => account.attributes_version -= 1,
            7 => account.catalog.version -= 1,
            8 => account.attributes.key_offset = Some(1),
            9 => account.catalog.kind = CatalogEntryKind::Alias,
            10 => account.catalog.related = Some(DatasetName::new(inserted::PRIMARY, 128).unwrap()),
            _ => {
                let _ = account.identities.pop();
            }
        }
        assert!(
            compare_state(&before, &after, EPOCH, EPOCH).is_err(),
            "account component {which}"
        );
    }
    for name in [inserted::PRIMARY, inserted::LOCAL_INDEX] {
        for which in 0..7 {
            let mut after = correct.clone();
            let rows = after.get_mut(name).unwrap();
            match which {
                0 => rows.records.swap(1, 2),
                1 => rows.identities[2][0] ^= 1,
                2 => rows.version = 2,
                3 => rows.attributes_version = 2,
                4 => rows.catalog.version = 2,
                5 => rows.attributes.key_length = Some(1),
                _ => {
                    let _ = rows.records.pop();
                }
            }
            assert!(compare_state(&before, &after, EPOCH, EPOCH).is_err());
        }
    }
    for which in 0..4 {
        let mut after = correct.clone();
        let rows = after.get_mut(inserted::XREF).unwrap();
        match which {
            0 => rows.records[0][0] ^= 1,
            1 => rows.identities[0][0] ^= 1,
            2 => rows.version += 1,
            _ => rows.attributes.ccsid = Some(500),
        }
        assert!(compare_state(&before, &after, EPOCH, EPOCH).is_err());
    }
    let mut after = correct.clone();
    after.remove(inserted::XREF);
    assert!(compare_state(&before, &after, EPOCH, EPOCH).is_err());
    let mut after = correct.clone();
    after.insert("EXTRA".into(), after[ACCOUNT].clone());
    assert!(compare_state(&before, &after, EPOCH, EPOCH).is_err());
    let mut wrong_before = before.clone();
    wrong_before.get_mut(ACCOUNT).unwrap().records[49][12] ^= 1;
    assert!(compare_state(&wrong_before, &correct, EPOCH, EPOCH).is_err());
}
#[test]
fn actual_time_and_mutation_effects_refuse_wrong_conditions_order_or_missing_operation() {
    let correct = trace();
    assert!(compare_trace(&correct).is_ok());
    for which in 0..8 {
        let mut actual = correct.clone();
        match which {
            0 => actual[5].operation = CicsOperation::AsktimeEib,
            1 => actual[5].outcome = "INVREQ".into(),
            2 => actual[6].response = 16,
            3 => actual[6].response2 = 1,
            4 => actual[6].payload_bytes = 1,
            5 => actual.swap(5, 6),
            6 => {
                actual.remove(6);
            }
            _ => {
                actual[8].outcome = "IOERR".into();
                actual[8].response = 17;
            }
        }
        assert!(compare_trace(&actual).is_err(), "effect component {which}");
    }
    let mut actual = correct.clone();
    actual.push(correct[5].clone());
    assert!(compare_trace(&actual).is_err());
    let mut actual = correct;
    actual[8].operation = CicsOperation::Delete;
    assert!(compare_trace(&actual).is_err());
    // A caught REWRITE error plus normal application RETURN never becomes a
    // successful mutation, rollback proof, or ASKTIME/FORMATTIME observation.
}
