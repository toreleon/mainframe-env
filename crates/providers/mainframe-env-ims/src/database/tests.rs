use super::*;

fn shisam_fixed_definition(organization: DatabaseOrganization) -> DatabaseDefinition {
    let mut value = definition(organization);
    value.segments.truncate(1);
    value.segments[0].max_length = 3;
    value.secondary_indexes.clear();
    value
}

#[test]
fn shisam_fixed_layout_engine_rejects_variable_root() {
    let good = shisam_fixed_definition(DatabaseOrganization::Shisam);
    DatabaseEngine::new(good.clone(), EngineLimits::default()).unwrap();
    let mut invalid = good;
    invalid.segments[0].max_length = 4;
    let actual = DatabaseEngine::new(invalid, EngineLimits::default());
    eprintln!("SHISAM_ENGINE_VARIABLE_ACTUAL={actual:?}");
    if let Ok(mut accepted) = actual.clone() {
        insert(&mut accepted, "ROOT", None, b"A1X");
        insert(&mut accepted, "ROOT", None, b"B2YZ");
        let bytes = serde_json::to_vec(&accepted.image()).unwrap();
        let restored = DatabaseEngine::restore(
            serde_json::from_slice(&bytes).unwrap(),
            EngineLimits::default(),
        )
        .unwrap();
        eprintln!(
            "SHISAM_ENGINE_LEGACY_IMAGE={}",
            String::from_utf8(bytes.clone()).unwrap()
        );
        eprintln!(
            "SHISAM_ENGINE_LEGACY_RESTORED_EXACT={}",
            serde_json::to_vec(&restored.image()).unwrap() == bytes
        );
    }
    assert_eq!(actual, Err(EngineProblem::InvalidDefinition));
}

#[test]
fn shisam_fixed_layout_engine_fixed_images_and_other_organization_controls() {
    for organization in [
        DatabaseOrganization::Shisam,
        DatabaseOrganization::Hisam,
        DatabaseOrganization::Shsam,
        DatabaseOrganization::Hidam,
    ] {
        let mut engine = DatabaseEngine::new(
            shisam_fixed_definition(organization),
            EngineLimits::default(),
        )
        .unwrap();
        insert(&mut engine, "ROOT", None, b"A1X");
        insert(&mut engine, "ROOT", None, b"B2Y");
        let bytes = serde_json::to_vec(&engine.image()).unwrap();
        if let Some(directory) = std::env::var_os("SHISAM_CAPTURE_GOOD_DIR") {
            let path =
                std::path::PathBuf::from(directory).join(format!("good-{organization:?}.json"));
            std::fs::write(path, &bytes).unwrap();
        }
        let restored = DatabaseEngine::restore(
            serde_json::from_slice(&bytes).unwrap(),
            EngineLimits::default(),
        )
        .unwrap();
        assert_eq!(serde_json::to_vec(&restored.image()).unwrap(), bytes);
        let mut position = PcbPosition::default();
        assert_eq!(
            read(
                &restored,
                &mut position,
                ReadKind::Unique,
                Some("ROOT"),
                vec![equal("ROOT", "ROOTKEY", b"A1")],
                true
            )
            .unwrap()
            .data,
            b"A1X"
        );
        assert!(position.is_held());
        assert_eq!(
            read(
                &restored,
                &mut position,
                ReadKind::Next,
                None,
                vec![],
                false
            )
            .unwrap()
            .data,
            b"B2Y"
        );
    }
    for organization in [DatabaseOrganization::Hisam, DatabaseOrganization::Hidam] {
        let mut ranged = shisam_fixed_definition(organization);
        ranged.segments[0].max_length = 4;
        DatabaseEngine::new(ranged, EngineLimits::default()).unwrap();
    }
    let mut shsam = shisam_fixed_definition(DatabaseOrganization::Shsam);
    shsam.segments[0].max_length = 4;
    assert_eq!(
        DatabaseEngine::new(shsam, EngineLimits::default()),
        Err(EngineProblem::InvalidDefinition)
    );
}

#[test]
#[ignore = "requires exact externally retained old/new engine images"]
fn shisam_fixed_layout_engine_historical_reader() {
    let directory = std::path::PathBuf::from(std::env::var_os("SHISAM_IMAGE_DIR").unwrap());
    for organization in [
        DatabaseOrganization::Shisam,
        DatabaseOrganization::Hisam,
        DatabaseOrganization::Shsam,
        DatabaseOrganization::Hidam,
    ] {
        let path = directory.join(format!("good-{organization:?}.json"));
        let bytes = std::fs::read(&path).unwrap();
        let engine = DatabaseEngine::restore(
            serde_json::from_slice(&bytes).unwrap(),
            EngineLimits::default(),
        )
        .unwrap();
        assert_eq!(serde_json::to_vec(&engine.image()).unwrap(), bytes);
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
    let path = std::path::PathBuf::from(std::env::var_os("SHISAM_INVALID_IMAGE").unwrap());
    let bytes = std::fs::read(&path).unwrap();
    let result = DatabaseEngine::restore(
        serde_json::from_slice(&bytes).unwrap(),
        EngineLimits::default(),
    );
    match std::env::var("SHISAM_READER_EXPECTATION").unwrap().as_str() {
        "old" => assert_eq!(serde_json::to_vec(&result.unwrap().image()).unwrap(), bytes),
        "new" => assert_eq!(result, Err(EngineProblem::InvalidDefinition)),
        other => panic!("Invalid independently selected reader expectation: {other}"),
    }
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    eprintln!("SHISAM_ENGINE_HISTORICAL_READER_PASS");
}

#[test]
fn hisam_nonunique_private_last_preserves_historical_insert_and_image() {
    let mut descriptor = definition(DatabaseOrganization::Hisam);
    descriptor.segments.truncate(2);
    descriptor.secondary_indexes.clear();
    for segment in &mut descriptor.segments {
        segment.max_length = 3;
    }
    let descriptor_bytes = serde_json::to_vec(&descriptor).unwrap();
    let mut engine = DatabaseEngine::new(descriptor, EngineLimits::default()).unwrap();
    let root = insert(&mut engine, "ROOT", None, b"A1X");
    let first = insert(&mut engine, "CHILD", Some(root), b"C1A");
    let later = insert(&mut engine, "CHILD", Some(root), b"C2Z");
    let duplicate = InsertRequest {
        segment: "CHILD".into(),
        parent: Some(root),
        data: b"C1B".to_vec(),
    };
    let before = engine.state_digest();
    assert_eq!(
        engine.insert(duplicate.clone()),
        Err(EngineProblem::Duplicate)
    );
    assert_eq!(
        engine.insert_loaded(duplicate.clone()),
        Err(EngineProblem::Duplicate)
    );
    assert_eq!(engine.state_digest(), before);
    let inserted = engine
        .insert_nonunique_dependent_last(duplicate, false)
        .unwrap();
    assert_ne!(inserted.id, first);
    let mut position = PcbPosition::default();
    position.set_current(root);
    engine
        .position_after_nonunique_dependent_insert(&mut position, inserted.id)
        .unwrap();
    assert_eq!(position.current(), Some(inserted.id));
    assert_eq!(position.parentage(), Some(root));
    assert!(!position.is_held());
    assert_eq!(
        engine
            .export_records()
            .iter()
            .map(|r| (r.id, r.data.as_slice()))
            .collect::<Vec<_>>(),
        vec![
            (root, &b"A1X"[..]),
            (first, &b"C1A"[..]),
            (later, &b"C2Z"[..]),
            (inserted.id, &b"C1B"[..])
        ]
    );
    position.set_current(root);
    for bytes in [&b"C1A"[..], &b"C1B"[..], &b"C2Z"[..]] {
        assert_eq!(
            read(
                &engine,
                &mut position,
                ReadKind::NextInParent,
                Some("CHILD"),
                vec![],
                false
            )
            .unwrap()
            .data,
            bytes
        );
    }
    assert_eq!(
        serde_json::to_vec(engine.definition()).unwrap(),
        descriptor_bytes
    );
    let bytes = serde_json::to_vec(&engine.image()).unwrap();
    let mut restored = DatabaseEngine::restore(
        serde_json::from_slice(&bytes).unwrap(),
        EngineLimits::default(),
    )
    .unwrap();
    assert_eq!(serde_json::to_vec(&restored.image()).unwrap(), bytes);
    assert_eq!(
        restored.insert(InsertRequest {
            segment: "CHILD".into(),
            parent: Some(root),
            data: b"C1Q".to_vec()
        }),
        Err(EngineProblem::Duplicate)
    );
    let utility = restored
        .insert_nonunique_dependent_last(
            InsertRequest {
                segment: "CHILD".into(),
                parent: Some(root),
                data: b"C1Q".to_vec(),
            },
            true,
        )
        .unwrap();
    assert!(utility.id > inserted.id);
}

fn field(name: &str, offset: usize, length: usize) -> FieldDefinition {
    FieldDefinition {
        name: name.into(),
        offset,
        length,
    }
}

#[test]
fn restored_images_reject_orphaned_children_and_impossible_versions() {
    let mut engine = DatabaseEngine::new(
        definition(DatabaseOrganization::Hidam),
        EngineLimits::default(),
    )
    .unwrap();
    let root = insert(&mut engine, "ROOT", None, b"R1A");
    insert(&mut engine, "CHILD", Some(root), b"C1B");
    assert_eq!(
        DatabaseEngine::restore(engine.image(), EngineLimits::default())
            .unwrap()
            .state_digest(),
        engine.state_digest()
    );

    let mut orphan = serde_json::to_value(engine.image()).unwrap();
    orphan["records"][0]["children"] = serde_json::json!([999]);
    let orphan: DatabaseEngineImage = serde_json::from_value(orphan).unwrap();
    assert_eq!(
        DatabaseEngine::restore(orphan, EngineLimits::default()),
        Err(EngineProblem::InvalidData)
    );

    let mut impossible = serde_json::to_value(engine.image()).unwrap();
    impossible["records"][0]["version"] = serde_json::json!(0);
    let impossible: DatabaseEngineImage = serde_json::from_value(impossible).unwrap();
    assert_eq!(
        DatabaseEngine::restore(impossible, EngineLimits::default()),
        Err(EngineProblem::InvalidData)
    );
}

fn segment(name: &str, parent: Option<&str>, key: &str) -> SegmentDefinition {
    SegmentDefinition {
        name: name.into(),
        parent: parent.map(str::to_owned),
        min_length: 3,
        max_length: 5,
        key_field: Some(key.into()),
        fields: vec![field(key, 0, 2), field("KIND", 2, 1)],
    }
}

fn definition(organization: DatabaseOrganization) -> DatabaseDefinition {
    DatabaseDefinition {
        gsam_format: None,
        name: "TESTDB".into(),
        organization,
        segments: vec![
            segment("ROOT", None, "ROOTKEY"),
            segment("CHILD", Some("ROOT"), "CHILDKEY"),
            segment("GRAND", Some("CHILD"), "GRANDKEY"),
        ],
        secondary_indexes: vec![
            SecondaryIndexDefinition {
                name: "ROOT-BY-KIND".into(),
                source_segment: "ROOT".into(),
                field: "KIND".into(),
                additional_fields: vec![],
                target_segment: None,
                unique: true,
            },
            SecondaryIndexDefinition {
                name: "CHILD-BY-KIND".into(),
                source_segment: "CHILD".into(),
                field: "KIND".into(),
                additional_fields: vec![],
                target_segment: None,
                unique: false,
            },
        ],
    }
}

fn insert(
    engine: &mut DatabaseEngine,
    segment: &str,
    parent: Option<RecordId>,
    data: &[u8],
) -> RecordId {
    engine
        .insert(InsertRequest {
            segment: segment.into(),
            parent,
            data: data.to_vec(),
        })
        .unwrap()
        .id
}

fn equal(segment: &str, field: &str, value: &[u8]) -> SegmentSelector {
    SegmentSelector {
        segment: segment.into(),
        predicates: vec![FieldPredicate {
            field: field.into(),
            relation: Relation::Equal,
            value: value.to_vec(),
        }],
    }
}

fn read(
    engine: &DatabaseEngine,
    position: &mut PcbPosition,
    kind: ReadKind,
    target: Option<&str>,
    path: Vec<SegmentSelector>,
    hold: bool,
) -> Result<RecordView, EngineProblem> {
    engine.read(
        position,
        &ReadRequest {
            kind,
            target: target.map(str::to_owned),
            path,
            hold,
        },
    )
}

#[test]
fn metadata_validation_is_bounded_and_organization_aware() {
    let engine = DatabaseEngine::new(
        definition(DatabaseOrganization::Hidam),
        EngineLimits::default(),
    )
    .unwrap();
    assert_eq!(engine.record_count(), 0);

    let mut broken = definition(DatabaseOrganization::Shisam);
    assert_eq!(
        DatabaseEngine::new(broken.clone(), EngineLimits::default()),
        Err(EngineProblem::InvalidDefinition)
    );
    broken.organization = DatabaseOrganization::Gsam;
    broken.segments.truncate(1);
    assert_eq!(
        DatabaseEngine::new(broken, EngineLimits::default()),
        Err(EngineProblem::InvalidDefinition)
    );

    let mut variable = definition(DatabaseOrganization::Hidam);
    variable.segments[0].min_length = 4;
    variable.segments[0].max_length = 3;
    assert_eq!(
        DatabaseEngine::new(variable, EngineLimits::default()),
        Err(EngineProblem::InvalidDefinition)
    );
}

#[test]
fn gu_gn_gnp_preserve_hierarchy_order_parentage_and_failed_position() {
    let mut engine = DatabaseEngine::new(
        definition(DatabaseOrganization::Hidam),
        EngineLimits::default(),
    )
    .unwrap();
    let _r2 = insert(&mut engine, "ROOT", None, b"R2B");
    let r1 = insert(&mut engine, "ROOT", None, b"R1A");
    let _c2 = insert(&mut engine, "CHILD", Some(r1), b"C2B");
    let c1 = insert(&mut engine, "CHILD", Some(r1), b"C1A");
    let _g1 = insert(&mut engine, "GRAND", Some(c1), b"G1A");

    let mut sequential = PcbPosition::default();
    assert_eq!(
        read(
            &engine,
            &mut sequential,
            ReadKind::Next,
            None,
            vec![],
            false
        )
        .unwrap()
        .data,
        b"R1A"
    );
    assert_eq!(
        read(
            &engine,
            &mut sequential,
            ReadKind::Next,
            None,
            vec![],
            false
        )
        .unwrap()
        .data,
        b"C1A"
    );
    assert_eq!(
        read(
            &engine,
            &mut sequential,
            ReadKind::Next,
            None,
            vec![],
            false
        )
        .unwrap()
        .data,
        b"G1A"
    );

    let mut position = PcbPosition::default();
    let root = read(
        &engine,
        &mut position,
        ReadKind::Unique,
        Some("ROOT"),
        vec![equal("ROOT", "ROOTKEY", b"R1")],
        false,
    )
    .unwrap();
    assert_eq!(position.parentage(), Some(root.id));
    assert_eq!(
        read(
            &engine,
            &mut position,
            ReadKind::NextInParent,
            Some("CHILD"),
            vec![],
            false,
        )
        .unwrap()
        .data,
        b"C1A"
    );
    assert_eq!(position.parentage(), Some(root.id));
    assert_eq!(
        read(
            &engine,
            &mut position,
            ReadKind::NextInParent,
            Some("CHILD"),
            vec![],
            false,
        )
        .unwrap()
        .data,
        b"C2B"
    );
    let retained = position.clone();
    assert_eq!(
        read(
            &engine,
            &mut position,
            ReadKind::NextInParent,
            Some("CHILD"),
            vec![equal("ROOT", "ROOTKEY", b"R2")],
            false,
        ),
        Err(EngineProblem::PathMismatch)
    );
    assert_eq!(position, retained);

    assert_eq!(
        read(
            &engine,
            &mut position,
            ReadKind::Unique,
            Some("ROOT"),
            vec![equal("ROOT", "ROOTKEY", b"NO")],
            false,
        ),
        Err(EngineProblem::NotFound)
    );
    assert_eq!(position.current(), retained.current());
    assert_eq!(position.parentage(), None);
}

#[test]
fn hold_guards_atomic_replace_delete_and_secondary_index_maintenance() {
    let mut engine = DatabaseEngine::new(
        definition(DatabaseOrganization::Hidam),
        EngineLimits::default(),
    )
    .unwrap();
    let r1 = insert(&mut engine, "ROOT", None, b"R1A");
    let _r2 = insert(&mut engine, "ROOT", None, b"R2B");
    let c1 = insert(&mut engine, "CHILD", Some(r1), b"C1A");
    let _g1 = insert(&mut engine, "GRAND", Some(c1), b"G1A");
    let mut position = PcbPosition::default();

    read(
        &engine,
        &mut position,
        ReadKind::Unique,
        Some("ROOT"),
        vec![equal("ROOT", "ROOTKEY", b"R1")],
        false,
    )
    .unwrap();
    let before = engine.state_digest();
    assert_eq!(
        engine.replace(&mut position, b"R1Z"),
        Err(EngineProblem::HoldRequired)
    );
    assert_eq!(engine.state_digest(), before);

    read(
        &engine,
        &mut position,
        ReadKind::Unique,
        Some("ROOT"),
        vec![equal("ROOT", "ROOTKEY", b"R1")],
        true,
    )
    .unwrap();
    assert_eq!(
        engine.replace(&mut position, b"XXZ"),
        Err(EngineProblem::KeyChange)
    );
    assert_eq!(engine.state_digest(), before);
    engine.replace(&mut position, b"R1Z").unwrap();
    assert!(
        engine
            .lookup_index("ROOT-BY-KIND", b"A")
            .unwrap()
            .is_empty()
    );
    assert_eq!(engine.lookup_index("ROOT-BY-KIND", b"Z").unwrap()[0].id, r1);
    assert!(position.is_held());
    engine.replace(&mut position, b"R1Y").unwrap();
    assert_eq!(engine.lookup_index("ROOT-BY-KIND", b"Y").unwrap()[0].id, r1);
    assert!(position.is_held());

    read(
        &engine,
        &mut position,
        ReadKind::Unique,
        Some("CHILD"),
        vec![
            equal("ROOT", "ROOTKEY", b"R1"),
            equal("CHILD", "CHILDKEY", b"C1"),
        ],
        true,
    )
    .unwrap();
    assert_eq!(engine.delete(&mut position).unwrap(), 2);
    assert!(
        engine
            .lookup_index("CHILD-BY-KIND", b"A")
            .unwrap()
            .is_empty()
    );
    assert_eq!(engine.record_count(), 2);
}

#[test]
fn rejected_duplicate_index_and_length_changes_leave_state_unchanged() {
    let mut engine = DatabaseEngine::new(
        definition(DatabaseOrganization::Hdam),
        EngineLimits::default(),
    )
    .unwrap();
    let root = insert(&mut engine, "ROOT", None, b"R1A");
    insert(&mut engine, "CHILD", Some(root), b"C1A");

    let before = engine.state_digest();
    assert_eq!(
        engine.insert(InsertRequest {
            segment: "ROOT".into(),
            parent: None,
            data: b"R1Z".to_vec(),
        }),
        Err(EngineProblem::Duplicate)
    );
    assert_eq!(engine.state_digest(), before);
    assert_eq!(
        engine.insert(InsertRequest {
            segment: "ROOT".into(),
            parent: None,
            data: b"R2A".to_vec(),
        }),
        Err(EngineProblem::IndexConflict)
    );
    assert_eq!(engine.state_digest(), before);
    assert_eq!(
        engine.insert(InsertRequest {
            segment: "CHILD".into(),
            parent: Some(root),
            data: b"X".to_vec(),
        }),
        Err(EngineProblem::InvalidData)
    );
    assert_eq!(engine.state_digest(), before);
}

#[test]
fn deterministic_retry_from_the_same_snapshot_has_the_same_result_and_digest() {
    let mut left = DatabaseEngine::new(
        definition(DatabaseOrganization::Hidam),
        EngineLimits::default(),
    )
    .unwrap();
    let mut right = left.clone();
    let request = InsertRequest {
        segment: "ROOT".into(),
        parent: None,
        data: b"R1A".to_vec(),
    };
    assert_eq!(left.insert(request.clone()), right.insert(request.clone()));
    assert_eq!(left.state_digest(), right.state_digest());

    let accepted = left.state_digest();
    assert_eq!(left.insert(request), Err(EngineProblem::Duplicate));
    assert_eq!(left.state_digest(), accepted);
}

#[test]
fn record_version_fences_a_stale_hold_without_mutation() {
    let mut engine = DatabaseEngine::new(
        definition(DatabaseOrganization::Hidam),
        EngineLimits::default(),
    )
    .unwrap();
    insert(&mut engine, "ROOT", None, b"R1A");
    let held_root = || ReadRequest {
        kind: ReadKind::Unique,
        target: Some("ROOT".into()),
        path: vec![equal("ROOT", "ROOTKEY", b"R1")],
        hold: true,
    };
    let mut left = PcbPosition::default();
    let mut right = PcbPosition::default();
    engine.read(&mut left, &held_root()).unwrap();
    engine.read(&mut right, &held_root()).unwrap();
    engine.replace(&mut left, b"R1Z").unwrap();

    let accepted = engine.state_digest();
    assert_eq!(
        engine.replace(&mut right, b"R1Y"),
        Err(EngineProblem::StaleHold)
    );
    assert_eq!(engine.state_digest(), accepted);
}

#[test]
fn gsam_is_bounded_ordered_and_append_only() {
    let definition = DatabaseDefinition {
        gsam_format: None,
        name: "GSAMDB".into(),
        organization: DatabaseOrganization::Gsam,
        segments: vec![SegmentDefinition {
            name: "RECORD".into(),
            parent: None,
            min_length: 1,
            max_length: 5,
            key_field: None,
            fields: vec![],
        }],
        secondary_indexes: vec![],
    };
    let mut engine = DatabaseEngine::new(definition, EngineLimits::default()).unwrap();
    insert(&mut engine, "RECORD", None, b"one");
    insert(&mut engine, "RECORD", None, b"two");
    let mut position = PcbPosition::default();
    assert_eq!(
        read(&engine, &mut position, ReadKind::Next, None, vec![], false)
            .unwrap()
            .data,
        b"one"
    );
    assert_eq!(
        read(&engine, &mut position, ReadKind::Next, None, vec![], false)
            .unwrap()
            .data,
        b"two"
    );
    assert_eq!(
        read(&engine, &mut position, ReadKind::Next, None, vec![], false),
        Err(EngineProblem::EndOfDatabase)
    );
    assert_eq!(
        read(&engine, &mut position, ReadKind::Next, None, vec![], false)
            .unwrap()
            .data,
        b"one"
    );
    position.held = Some(HeldRecord {
        id: RecordId(1),
        version: 1,
    });
    let before_unsupported = position.clone();
    assert_eq!(
        read(
            &engine,
            &mut position,
            ReadKind::Unique,
            Some("RECORD"),
            vec![],
            true,
        ),
        Err(EngineProblem::Unsupported)
    );
    assert_eq!(position, before_unsupported);
}

#[test]
fn all_pinned_organizations_have_bounded_engine_or_index_only_behavior() {
    use DatabaseOrganization::*;
    let organizations = [
        Dedb, Gsam, Hdam, Hidam, Hisam, Hsam, Index, Msdb, Phdam, Phidam, Psindex, Shisam, Shsam,
    ];
    for organization in organizations {
        let mut descriptor = definition(organization);
        descriptor.secondary_indexes.clear();
        if matches!(organization, Gsam | Hsam | Shsam) {
            descriptor.segments[0].key_field = None;
        }
        if matches!(organization, Gsam | Index | Msdb | Psindex | Shisam | Shsam) {
            descriptor.segments.truncate(1);
        }
        if matches!(organization, Shsam | Shisam) {
            descriptor.segments[0].max_length = descriptor.segments[0].min_length;
        }
        let mut engine = DatabaseEngine::new(descriptor, EngineLimits::default()).unwrap();
        let before = engine.state_digest();
        let first = engine.insert(InsertRequest {
            segment: "ROOT".into(),
            parent: None,
            data: b"R2A".to_vec(),
        });
        if matches!(organization, Index | Psindex) {
            assert_eq!(first, Err(EngineProblem::Unsupported));
            assert_eq!(engine.state_digest(), before);
            engine
                .insert_loaded(InsertRequest {
                    segment: "ROOT".into(),
                    parent: None,
                    data: b"R2A".to_vec(),
                })
                .unwrap();
        } else {
            first.unwrap();
        }
        engine
            .insert_loaded(InsertRequest {
                segment: "ROOT".into(),
                parent: None,
                data: b"R1B".to_vec(),
            })
            .unwrap();
        let expected = if keyed_root_order(organization) {
            b"R1B"
        } else {
            b"R2A"
        };
        assert_eq!(
            engine.ordered_records()[0].data,
            expected,
            "{organization:?}"
        );
        assert_eq!(
            DatabaseEngine::restore(engine.image(), EngineLimits::default()).unwrap(),
            engine
        );
    }
}
