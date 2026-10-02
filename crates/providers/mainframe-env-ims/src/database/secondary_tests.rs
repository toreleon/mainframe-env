use super::*;

#[test]
fn secondary_ssa_uses_existing_pointer_cursor_and_target_fields() {
    use mainframe_env_host_api::{ImsSsaLimits, parse_ims_ssa};
    let mut definition = descriptor();
    definition.secondary_indexes[0].target_segment = Some("ROOT".into());
    for segment in &mut definition.segments {
        segment.min_length = 4;
    }
    let mut engine = DatabaseEngine::new(definition, Default::default()).unwrap();
    let a = put(&mut engine, "ROOT", None, b"A1ZZ");
    let ac = put(&mut engine, "CHILD", Some(a), b"C1ZZ");
    put(&mut engine, "GRAND", Some(ac), b"G1ZZ");
    let b = put(&mut engine, "ROOT", None, b"B2YY");
    let bc = put(&mut engine, "CHILD", Some(b), b"C2YY");
    let source = put(&mut engine, "GRAND", Some(bc), b"G2AA");
    let request = ReadRequest {
        kind: ReadKind::Unique,
        target: Some("ROOT".into()),
        path: vec![],
        hold: true,
    };
    let fields = engine.ssa_fields(Some("BYVALUE"));
    let ssa = parse_ims_ssa(
        b"ROOT    (BYVALUE EQAA&FIRST   EQY)",
        ImsSsaLimits::default(),
        &fields,
    )
    .unwrap();
    let mut position = PcbPosition::default();
    assert_eq!(
        engine
            .read_ssas(&mut position, &request, &[ssa], Some("BYVALUE"), |_| true)
            .unwrap()
            .id,
        b
    );
    assert_eq!(position.secondary.as_ref().unwrap().source, source);
    assert_eq!(position.current(), Some(b));
    assert_eq!(position.parentage(), Some(b));
    assert!(position.is_held());
    let child_request = ReadRequest {
        kind: ReadKind::NextInParent,
        target: Some("CHILD".into()),
        path: vec![
            SegmentSelector {
                segment: "ROOT".into(),
                predicates: vec![],
            },
            SegmentSelector {
                segment: "CHILD".into(),
                predicates: vec![],
            },
        ],
        hold: false,
    };
    let mismatch = parse_ims_ssa(b"ROOT    (BYVALUE EQZZ)", Default::default(), &fields).unwrap();
    let prior = position.clone();
    assert_eq!(
        engine.read_ssas(
            &mut position,
            &child_request,
            &[mismatch],
            Some("BYVALUE"),
            |_| true
        ),
        Err(EngineProblem::PathMismatch)
    );
    assert_eq!(position, prior);
    let matched = parse_ims_ssa(b"ROOT    (BYVALUE EQAA)", Default::default(), &fields).unwrap();
    assert_eq!(
        engine
            .read_ssas(
                &mut position,
                &child_request,
                &[matched],
                Some("BYVALUE"),
                |_| true
            )
            .unwrap()
            .id,
        bc
    );
    assert_eq!(position.parentage(), Some(b));
    assert!(!position.is_held());
}

#[test]
fn secondary_replace_keeps_parentage_when_index_bytes_are_unchanged() {
    for source_is_target in [true, false] {
        let mut definition = descriptor();
        definition.secondary_indexes[0].target_segment = Some("ROOT".into());
        if source_is_target {
            definition.secondary_indexes[0].source_segment = "ROOT".into();
        }
        for segment in &mut definition.segments {
            segment.min_length = 4;
        }
        let mut engine = DatabaseEngine::new(definition, EngineLimits::default()).unwrap();
        let root = put(&mut engine, "ROOT", None, b"R1AZ");
        let child = put(&mut engine, "CHILD", Some(root), b"C1XX");
        if !source_is_target {
            put(&mut engine, "GRAND", Some(child), b"G1AZ");
        }
        let mut position = PcbPosition::default();
        let read = ReadRequest {
            kind: ReadKind::Unique,
            target: Some("ROOT".into()),
            path: vec![],
            hold: true,
        };
        engine
            .read_secondary_visible("BYVALUE", &mut position, &read, |_| true)
            .unwrap();
        assert_eq!(position.parentage(), Some(root));
        engine.replace(&mut position, b"R1AZ").unwrap();
        assert_eq!(position.parentage(), Some(root), "unchanged XDFLD");
        engine.replace(&mut position, b"R1BZ").unwrap();
        assert_eq!(
            position.parentage(),
            (!source_is_target).then_some(root),
            "only the selected source's changed index bytes lose parentage"
        );
    }
}

fn descriptor() -> DatabaseDefinition {
    let segment = |name: &str, parent: Option<&str>| SegmentDefinition {
        name: name.into(),
        parent: parent.map(str::to_owned),
        min_length: 2,
        max_length: 4,
        key_field: Some("KEY".into()),
        fields: vec![
            FieldDefinition {
                name: "KEY".into(),
                offset: 0,
                length: 2,
            },
            FieldDefinition {
                name: "FIRST".into(),
                offset: 2,
                length: 1,
            },
            FieldDefinition {
                name: "LAST".into(),
                offset: 3,
                length: 1,
            },
        ],
    };
    DatabaseDefinition {
        name: "INDEXDB".into(),
        organization: DatabaseOrganization::Hidam,
        segments: vec![
            segment("ROOT", None),
            segment("CHILD", Some("ROOT")),
            segment("GRAND", Some("CHILD")),
        ],
        secondary_indexes: vec![SecondaryIndexDefinition {
            name: "BYVALUE".into(),
            source_segment: "GRAND".into(),
            field: "LAST".into(),
            additional_fields: vec!["FIRST".into()],
            target_segment: Some("CHILD".into()),
            unique: true,
        }],
    }
}

fn put(
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

#[test]
fn secondary_binary_composite_nonroot_ancestor_atomic_failures_and_restore() {
    let mut engine = DatabaseEngine::new(descriptor(), EngineLimits::default()).unwrap();
    let root = put(&mut engine, "ROOT", None, b"R1");
    let child = put(&mut engine, "CHILD", Some(root), b"C1");
    let source = put(&mut engine, "GRAND", Some(child), &[b'G', b'1', 0, 255]);
    assert_eq!(
        engine.lookup_index("BYVALUE", &[255, 0]).unwrap()[0].id,
        child
    );
    assert!(
        engine
            .lookup_index("BYVALUE", &[0, 255])
            .unwrap()
            .is_empty()
    );
    let digest = engine.state_digest();
    for (data, expected) in [
        (vec![b'G', b'2', 0], EngineProblem::InvalidData),
        (vec![b'G', b'2', 0, 255], EngineProblem::IndexConflict),
    ] {
        assert_eq!(
            engine
                .insert(InsertRequest {
                    segment: "GRAND".into(),
                    parent: Some(child),
                    data
                })
                .unwrap_err(),
            expected
        );
        assert_eq!(engine.state_digest(), digest);
    }
    let mut held = PcbPosition::default();
    held.set_current(source);
    engine
        .read(
            &mut held,
            &ReadRequest {
                kind: ReadKind::Unique,
                target: Some("GRAND".into()),
                path: vec![],
                hold: true,
            },
        )
        .unwrap();
    engine.replace(&mut held, &[b'G', b'1', 1, 254]).unwrap();
    assert!(
        engine
            .lookup_index("BYVALUE", &[255, 0])
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        engine.lookup_index("BYVALUE", &[254, 1]).unwrap()[0].id,
        child
    );
    let image = engine.image();
    let restored = DatabaseEngine::restore(image, EngineLimits::default()).unwrap();
    assert_eq!(restored.state_digest(), engine.state_digest());
    assert_eq!(
        restored.lookup_index("BYVALUE", &[254, 1]).unwrap()[0].id,
        child
    );
    assert_eq!(engine.delete(&mut held).unwrap(), 1);
    assert!(
        engine
            .lookup_index("BYVALUE", &[254, 1])
            .unwrap()
            .is_empty()
    );
}

#[test]
fn secondary_invalid_shapes_rejected_before_engine_admission() {
    let original = descriptor();
    for alter in 0..5 {
        let mut definition = original.clone();
        match alter {
            0 => definition.secondary_indexes[0].source_segment = "ROOT".into(),
            1 => definition.secondary_indexes[0].target_segment = Some("MISSING".into()),
            2 => definition.secondary_indexes[0]
                .additional_fields
                .push("LAST".into()),
            3 => definition.secondary_indexes[0].field = "MISSING".into(),
            _ => definition.secondary_indexes[0].name = "KEY".into(),
        }
        assert_eq!(
            DatabaseEngine::new(definition, EngineLimits::default()),
            Err(EngineProblem::InvalidDefinition)
        );
    }
}

#[test]
fn secondary_historical_descriptor_position_and_image_bytes_keep_identity() {
    let bytes = br#"{"name":"OLDIX","source_segment":"ROOT","field":"KEY","unique":false}"#;
    let index: SecondaryIndexDefinition = serde_json::from_slice(bytes).unwrap();
    assert_eq!(serde_json::to_vec(&index).unwrap(), bytes);
    assert_eq!(index.target_segment(), "ROOT");
    let position = br#"{"current":null,"parentage":null,"held":null,"after_end":false}"#;
    let parsed: PcbPosition = serde_json::from_slice(position).unwrap();
    assert_eq!(serde_json::to_vec(&parsed).unwrap(), position);
    let mut definition = descriptor();
    definition.secondary_indexes = vec![index];
    let mut engine = DatabaseEngine::new(definition, EngineLimits::default()).unwrap();
    put(&mut engine, "ROOT", None, b"R1");
    let bytes = serde_json::to_vec(&engine.image()).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("additional_fields"));
    let restored = DatabaseEngine::restore(
        serde_json::from_slice(&bytes).unwrap(),
        EngineLimits::default(),
    )
    .unwrap();
    assert_eq!(restored.state_digest(), engine.state_digest());
    assert_eq!(serde_json::to_vec(&restored.image()).unwrap(), bytes);
    #[derive(Deserialize)]
    struct PriorReader {
        field: String,
    }
    assert_eq!(
        serde_json::from_slice::<PriorReader>(br#"{"field":"KEY"}"#)
            .unwrap()
            .field,
        "KEY"
    );
    let extended = serde_json::to_vec(&descriptor().secondary_indexes[0]).unwrap();
    assert!(serde_json::from_slice::<PriorReader>(&extended).is_err());
}

#[test]
fn secondary_duplicate_source_pointers_keep_independent_occurrence_cursors() {
    let mut definition = descriptor();
    definition.secondary_indexes[0].target_segment = Some("ROOT".into());
    definition.secondary_indexes[0].unique = false;
    for segment in &mut definition.segments {
        segment.min_length = 4;
    }
    let mut engine = DatabaseEngine::new(definition, EngineLimits::default()).unwrap();
    let root = put(&mut engine, "ROOT", None, b"R1XX");
    let child = put(&mut engine, "CHILD", Some(root), b"C1XX");
    put(&mut engine, "GRAND", Some(child), b"G1AZ");
    put(&mut engine, "GRAND", Some(child), b"G2AZ");
    let other = put(&mut engine, "ROOT", None, b"R2XX");
    let other_child = put(&mut engine, "CHILD", Some(other), b"C2XX");
    put(&mut engine, "GRAND", Some(other_child), b"G3AZ");
    assert_eq!(
        engine
            .lookup_index("BYVALUE", b"ZA")
            .unwrap()
            .iter()
            .map(|view| view.id)
            .collect::<Vec<_>>(),
        vec![root, root, other]
    );
    let read = ReadRequest {
        kind: ReadKind::Next,
        target: Some("ROOT".into()),
        path: vec![],
        hold: false,
    };
    let mut position = PcbPosition::default();
    assert_eq!(
        engine
            .read_secondary_visible("BYVALUE", &mut position, &read, |_| true)
            .unwrap()
            .id,
        root
    );
    let first = position.clone();
    assert_eq!(
        engine
            .read_secondary_visible("BYVALUE", &mut position, &read, |_| true)
            .unwrap()
            .id,
        root
    );
    assert_ne!(position, first);
    let retained: PcbPosition =
        serde_json::from_slice(&serde_json::to_vec(&position).unwrap()).unwrap();
    engine.validate_position(&retained).unwrap();
    assert_eq!(
        engine
            .read_secondary_visible("BYVALUE", &mut position, &read, |_| true)
            .unwrap()
            .id,
        other
    );
    let parent = position.parentage();
    let child_read = ReadRequest {
        kind: ReadKind::NextInParent,
        target: Some("CHILD".into()),
        path: vec![],
        hold: true,
    };
    assert_eq!(
        engine
            .read_secondary_visible("BYVALUE", &mut position, &child_read, |_| true)
            .unwrap()
            .id,
        other_child
    );
    let current = position.current();
    assert_eq!(
        engine.read_secondary_visible("BYVALUE", &mut position, &child_read, |_| true),
        Err(EngineProblem::NotFound)
    );
    assert_eq!(position.parentage(), parent);
    assert_eq!(position.current(), current);
    assert!(!position.is_held());
    let mut corrupt = engine.image();
    corrupt.definition.secondary_indexes[0].unique = true;
    assert_eq!(
        DatabaseEngine::restore(corrupt, EngineLimits::default()),
        Err(EngineProblem::InvalidData)
    );
}
