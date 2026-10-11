//! Comparator adversity only; synthetic patterns never supply actual route credit.
use super::*;

fn fixture() -> (Vec<ReadData>, Vec<Vec<CellSelection>>, Vec<Vec<u8>>) {
    let mut reads = Vec::new();
    let mut slots = Vec::new();
    let mut expected = Vec::new();
    for (index, (name, _, width, _, resource)) in ORACLES.iter().enumerate() {
        let bytes = (0..*width)
            .map(|offset| (offset % 251) as u8)
            .collect::<Vec<_>>();
        let selected = [
            (name.to_string(), *width),
            (RESPONSE.into(), 4),
            (RESPONSE2.into(), 4),
        ]
        .into_iter()
        .enumerate()
        .map(|(role, (name, length))| CellSelection {
            name,
            id: (index * 3 + role) as u32,
            base: index,
            offset: role * 1000,
            length,
        })
        .collect::<Vec<_>>();
        let cells = selected
            .iter()
            .enumerate()
            .map(|(role, slot)| CellData {
                name: slot.name.clone(),
                id: slot.id,
                base: slot.base,
                offset: slot.offset,
                bytes: if role == 0 { bytes.clone() } else { vec![0; 4] },
            })
            .collect();
        reads.push(ReadData {
            sequence: index as u64 + 1,
            resource: AuditResourceDigest {
                format: AuditResourceDigestFormat::CanonicalHostResourceV1,
                value: digest(resource).unwrap(),
            },
            cells,
            eib: expected_eib(),
        });
        slots.push(selected);
        expected.push(bytes);
    }
    (reads, slots, expected)
}

#[test]
fn every_into_and_comp_response_byte_corruption_refuses() {
    let (reads, slots, expected) = fixture();
    assert!(compare_data(&reads, &slots, &expected).is_ok());
    for read in 0..3 {
        for role in 0..3 {
            for byte in 0..reads[read].cells[role].bytes.len() {
                let mut altered = reads.clone();
                altered[read].cells[role].bytes[byte] ^= 1;
                assert!(compare_data(&altered, &slots, &expected).is_err());
            }
        }
    }
}

#[test]
fn exact_typed_eib_kind_value_order_and_membership_are_required() {
    let (reads, slots, expected) = fixture();
    for read in 0..3 {
        for field in 0..6 {
            let mut altered = reads.clone();
            match &mut altered[read].eib[field].1 {
                CicsReadCopybackValue::Bytes(bytes) => bytes[0] ^= 1,
                CicsReadCopybackValue::Decimal { coefficient, .. } => *coefficient += 1,
            }
            assert!(compare_data(&altered, &slots, &expected).is_err());
            let mut altered = reads.clone();
            altered[read].eib[field].1 = CicsReadCopybackValue::Bytes(vec![0]);
            assert!(compare_data(&altered, &slots, &expected).is_err());
        }
        let mut altered = reads.clone();
        altered[read].eib[0] = ("WRONG", altered[read].eib[0].1.clone());
        assert!(compare_data(&altered, &slots, &expected).is_err());
        let mut altered = reads.clone();
        altered[read].eib.swap(0, 1);
        assert!(compare_data(&altered, &slots, &expected).is_err());
        let mut altered = reads.clone();
        altered[read].eib.pop();
        assert!(compare_data(&altered, &slots, &expected).is_err());
        let mut altered = reads.clone();
        let duplicate = altered[read].eib[0].clone();
        altered[read].eib.push(duplicate);
        assert!(compare_data(&altered, &slots, &expected).is_err());
    }
}

#[test]
fn wrong_resource_sequence_order_or_missing_extra_read_refuses() {
    let (reads, slots, expected) = fixture();
    let mut altered = reads.clone();
    altered.swap(0, 1);
    assert!(compare_data(&altered, &slots, &expected).is_err());
    let mut altered = reads.clone();
    altered[0].sequence = 0;
    assert!(compare_data(&altered, &slots, &expected).is_err());
    let mut altered = reads.clone();
    altered[1].sequence = altered[0].sequence;
    assert!(compare_data(&altered, &slots, &expected).is_err());
    let mut altered = reads.clone();
    altered.pop();
    assert!(compare_data(&altered, &slots, &expected).is_err());
    let mut altered = reads.clone();
    altered.push(reads[2].clone());
    assert!(compare_data(&altered, &slots, &expected).is_err());
    for read in 0..3 {
        let mut altered = reads.clone();
        altered[read].resource.value[0] ^= 1;
        assert!(compare_data(&altered, &slots, &expected).is_err());
        let mut altered = reads.clone();
        altered[read].resource.format = AuditResourceDigestFormat::CanonicalHostOversizedResourceV1;
        assert!(compare_data(&altered, &slots, &expected).is_err());
    }
}

#[test]
fn cell_roles_views_dimensions_and_identity_are_exact() {
    let (reads, slots, expected) = fixture();
    for read in 0..3 {
        for role in 0..3 {
            for field in 0..6 {
                let mut altered = reads.clone();
                let cell = &mut altered[read].cells[role];
                match field {
                    0 => cell.name.push('X'),
                    1 => cell.id += 1,
                    2 => cell.base += 1,
                    3 => cell.offset += 1,
                    4 => {
                        cell.bytes.pop();
                    }
                    _ => cell.bytes.push(0),
                }
                assert!(compare_data(&altered, &slots, &expected).is_err());
            }
        }
        let mut altered = reads.clone();
        altered[read].cells.swap(1, 2);
        assert!(compare_data(&altered, &slots, &expected).is_err());
        let mut altered = reads.clone();
        altered[read].cells.pop();
        assert!(compare_data(&altered, &slots, &expected).is_err());
        let mut altered = reads.clone();
        altered[read].cells.push(reads[read].cells[0].clone());
        assert!(compare_data(&altered, &slots, &expected).is_err());
    }
}

#[test]
fn supplied_enter_public_actor_context_and_completed_stage_are_required() {
    let valid = serde_json::json!({
        "schema_version":"mainframe-env.cics-terminal@1", "session":"owned-session",
        "principal":"WEBUSER", "run_unit":"owned-run", "connected":true,
        "suspended":false, "mapset":"COACTVW", "map":"CACTVWA",
        "aid":125, "transaction":"CAVW", "version":1
    });
    assert!(terminal_matches(&valid, "owned-session", "owned-run"));
    for (field, wrong) in [
        ("schema_version", serde_json::json!("wrong")),
        ("session", serde_json::json!("foreign-session")),
        ("principal", serde_json::json!("OTHER")),
        ("run_unit", serde_json::json!("foreign-run")),
        ("connected", serde_json::json!(false)),
        ("suspended", serde_json::json!(true)),
        ("mapset", serde_json::json!("OTHER")),
        ("map", serde_json::json!("OTHER")),
        ("aid", serde_json::json!(248)),
        ("transaction", serde_json::json!("CM00")),
        ("version", serde_json::json!(0)),
    ] {
        let mut altered = valid.clone();
        altered[field] = wrong;
        assert!(!terminal_matches(&altered, "owned-session", "owned-run"));
        let mut missing = valid.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(!terminal_matches(&missing, "owned-session", "owned-run"));
    }
}

#[test]
fn oracle_input_requires_exact_regular_private_width_and_frozen_hash() {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = env::temp_dir().join(format!(
        "read-copyback-oracle-control-{}-{suffix}",
        std::process::id()
    ));
    fs::create_dir(&root).unwrap();
    let path = root.join("selected.bin");
    fs::write(&path, [1, 2, 3, 4]).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let hash = format!("{:x}", Sha256::digest([1u8, 2, 3, 4]));
    assert_eq!(
        load_expected(&root, "selected.bin", 4, &hash).ok(),
        Some(vec![1, 2, 3, 4])
    );
    assert!(load_expected(&root, "selected.bin", 3, &hash).is_err());
    assert!(load_expected(&root, "selected.bin", 5, &hash).is_err());
    assert!(load_expected(&root, "selected.bin", 4, &"0".repeat(64)).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load_expected(&root, "selected.bin", 4, &hash).is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let link = root.join("link.bin");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(load_expected(&root, "link.bin", 4, &hash).is_err());
        fs::remove_file(link).unwrap();
    }
    fs::remove_file(path).unwrap();
    assert!(load_expected(&root, "selected.bin", 4, &hash).is_err());
    fs::remove_dir(root).unwrap();
}

#[test]
fn refusal_diagnostics_locate_actual_boundary_without_storage_or_context_values() {
    const POISON: &str = "DO_NOT_LOG_RAW_STORAGE";
    let (reads, slots, expected) = fixture();
    assert!(compare_data(&reads, &slots, &expected).is_ok());
    let check = |result: Result<(), CorpusProblem>, stage: &str, read, role, field| {
        let error = result.unwrap_err();
        assert_eq!(error.code, "carddemo.online.read_copyback_storage");
        assert_eq!(
            error.detail,
            format!(
                "selected READ comparison refused: stage={stage}; read={read:?}; \
                 cell_role={role:?}; eib_field={field:?}"
            )
        );
        assert!(!error.detail.contains(POISON));
        assert!(!error.detail.contains("CAVW"));
    };
    for (index, read) in reads.iter().enumerate() {
        for (role, _) in read.cells.iter().enumerate() {
            let mut altered = reads.clone();
            let cell = &mut altered[index].cells[role];
            if role == 0 {
                cell.bytes[..POISON.len()].copy_from_slice(POISON.as_bytes());
            } else {
                cell.bytes.copy_from_slice(&POISON.as_bytes()[..4]);
            }
            check(
                compare_data(&altered, &slots, &expected),
                "cell-bytes",
                Some(index),
                Some(role),
                None,
            );
            let mut altered = reads.clone();
            altered[index].cells[role].name = POISON.into();
            check(
                compare_data(&altered, &slots, &expected),
                "cell-geometry",
                Some(index),
                Some(role),
                None,
            );
        }
        for (field, _) in read.eib.iter().enumerate() {
            let mut altered = reads.clone();
            altered[index].eib[field].1 = CicsReadCopybackValue::Bytes(POISON.as_bytes().to_vec());
            check(
                compare_data(&altered, &slots, &expected),
                "eib",
                Some(index),
                None,
                Some(field),
            );
        }
        let mut altered = reads.clone();
        altered[index].eib[0].0 = POISON;
        check(
            compare_data(&altered, &slots, &expected),
            "eib",
            Some(index),
            None,
            Some(0),
        );
        let mut altered = reads.clone();
        altered[index].eib.pop();
        check(
            compare_data(&altered, &slots, &expected),
            "eib",
            Some(index),
            None,
            None,
        );
        let mut altered = reads.clone();
        altered[index].resource.value = Sha256::digest(POISON.as_bytes()).into();
        check(
            compare_data(&altered, &slots, &expected),
            "resource",
            Some(index),
            None,
            None,
        );
        let mut altered = reads.clone();
        altered[index].cells.pop();
        check(
            compare_data(&altered, &slots, &expected),
            "cell-membership",
            Some(index),
            None,
            None,
        );
        let mut altered = expected.clone();
        altered[index].pop();
        check(
            compare_data(&reads, &slots, &altered),
            "oracle-width",
            Some(index),
            None,
            None,
        );
    }
    let mut altered = reads.clone();
    altered.pop();
    check(
        compare_data(&altered, &slots, &expected),
        "read-chain",
        None,
        None,
        None,
    );
    let mut altered = reads.clone();
    altered.push(reads[2].clone());
    check(
        compare_data(&altered, &slots, &expected),
        "read-chain",
        None,
        None,
        None,
    );
    check(
        require_at(false, RefusalStage::OwnerBinding, None, None),
        "owner-binding",
        None,
        None,
        None,
    );
}
