//! Synthetic refusal controls only; actual corpus acceptance belongs to the existing Db2 owner.
use super::*;
use mainframe_env_compiler_api::{ArtifactManifest, VersionedArtifactManifest};
use mainframe_env_execution_api::PrincipalId;

fn screen(session: &str, option: Option<u8>) -> serde_json::Value {
    let (program, transaction, mapset, map) = match option {
        Some(option) => target(option).unwrap(),
        None => ("COADM01C", "CA00", "COADM01", "COADM1A"),
    };
    let mut fields = BTreeMap::from([
        ("PGMNAME", program.as_bytes().to_vec()),
        ("TRNNAME", transaction.as_bytes().to_vec()),
    ]);
    if option.is_none() {
        fields.insert("OPTN005", LABEL5.to_vec());
        fields.insert("OPTN006", LABEL6.to_vec());
    }
    let mut bytes = Vec::new();
    for (name, value) in fields {
        bytes.extend_from_slice(&(name.len() as u32).to_be_bytes());
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(&(value.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&value);
    }
    serde_json::json!({"session":session,"principal":"WEBADM","connected":true,
        "suspended":false,"mapset":mapset,"map":map,"run_unit":"selected-run",
        "screen_base64":base64::engine::general_purpose::STANDARD.encode(bytes)})
}
fn replace_field(terminal: &mut serde_json::Value, field: &str, value: &[u8]) {
    let mut fields = online_screen_fields(terminal).unwrap();
    fields.insert(field.into(), value.to_vec());
    let mut bytes = Vec::new();
    for (name, value) in fields {
        bytes.extend_from_slice(&(name.len() as u32).to_be_bytes());
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(&(value.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&value);
    }
    terminal["screen_base64"] =
        serde_json::json!(base64::engine::general_purpose::STANDARD.encode(bytes));
}

#[test]
fn exact_full_db2_labels_reject_regular_menu_swaps_truncation_and_padding_drift() {
    assert_eq!(LABEL5.len(), 40);
    assert_eq!(LABEL6.len(), 40);
    let menu = screen("menu-session", None);
    assert!(menu_matches(&menu, "menu-session").unwrap());
    for (name, value) in [
        ("OPTN005", LABEL6),
        ("OPTN006", LABEL5),
        ("OPTN005", &LABEL5[..39]),
        ("OPTN006", b"06. Transaction Type Maintenance (Db2)X "),
        ("PGMNAME", b"COMEN01C"),
        ("TRNNAME", b"CM00"),
    ] {
        let mut wrong = menu.clone();
        replace_field(&mut wrong, name, value);
        assert!(!menu_matches(&wrong, "menu-session").unwrap(), "{name}");
    }
}

#[test]
fn wrong_menu_target_session_principal_map_and_nonboolean_surface_refuse() {
    for option in [5, 6] {
        let terminal = screen("selected-session", Some(option));
        assert!(target_matches(&terminal, "selected-session", option).unwrap());
        assert!(!target_matches(&terminal, "selected-session", 11 - option).unwrap());
        for (name, value) in [
            ("session", serde_json::json!("other-session")),
            ("principal", serde_json::json!("WEBUSER")),
            ("mapset", serde_json::json!("COADM01")),
            ("map", serde_json::json!("OTHER")),
            ("connected", serde_json::json!(false)),
            ("suspended", serde_json::json!(true)),
            ("suspended", serde_json::json!("false")),
            ("suspended", serde_json::Value::Null),
        ] {
            let mut wrong = terminal.clone();
            wrong[name] = value;
            assert!(
                !target_matches(&wrong, "selected-session", option).unwrap(),
                "{name}"
            );
        }
        for (name, value) in [("PGMNAME", b"COADM01C".as_slice()), ("TRNNAME", b"CA00")] {
            let mut wrong = terminal.clone();
            replace_field(&mut wrong, name, value);
            assert!(!target_matches(&wrong, "selected-session", option).unwrap());
        }
    }
    let mut wrong = screen("selected-session", None);
    wrong["mapset"] = serde_json::json!("COMEN01");
    assert!(!menu_matches(&wrong, "selected-session").unwrap());
}

fn owner(name: &str) -> ExecutionRecord {
    let limits = InvocationLimits::default();
    ExecutionRecord {
        execution_id: ExecutionId::new("selected-actor", limits).unwrap(),
        run_unit_id: RunUnitId::new("selected-run", limits).unwrap(),
        selector: Selector::new(format!("program:{name}"), limits).unwrap(),
        artifact: ArtifactRef::new("sha256:synthetic-selected", limits).unwrap(),
        principal: PrincipalId::new("WEBADM", limits).unwrap(),
        state: ExecutionState::Completed,
        attempt: 1,
        version: 17,
        owner_lease: None,
        lease_expiry_tick: None,
        terminal_tick: Some(100),
    }
}
fn event(owner: &ExecutionRecord, menu: bool) -> LifecycleEvent {
    LifecycleEvent {
        execution_id: owner.execution_id.clone(),
        run_unit_id: owner.run_unit_id.clone(),
        sequence: owner.version,
        attempt: owner.attempt,
        tick: 100,
        kind: if menu {
            LifecycleEventKind::HandoffCompleted
        } else {
            LifecycleEventKind::Completed { return_code: 0 }
        },
    }
}
#[test]
fn exact_selected_actor_refuses_wrong_artifact_target_principal_run_attempt_or_state() {
    let actual = owner("COTRTLIC");
    let run = actual.run_unit_id.clone();
    let artifact = actual.artifact.clone();
    assert!(actor_matches(&actual, "COTRTLIC", &artifact, &run));
    let mut cases = Vec::new();
    let mut wrong = actual.clone();
    wrong.artifact = ArtifactRef::new("sha256:foreign", InvocationLimits::default()).unwrap();
    cases.push(wrong);
    let mut wrong = actual.clone();
    wrong.selector = Selector::new("program:COTRTUPC", InvocationLimits::default()).unwrap();
    cases.push(wrong);
    let mut wrong = actual.clone();
    wrong.principal = PrincipalId::new("WEBUSER", InvocationLimits::default()).unwrap();
    cases.push(wrong);
    let mut wrong = actual.clone();
    wrong.run_unit_id = RunUnitId::new("foreign-run", InvocationLimits::default()).unwrap();
    cases.push(wrong);
    let mut wrong = actual.clone();
    wrong.attempt = 0;
    cases.push(wrong);
    let mut wrong = actual.clone();
    wrong.state = ExecutionState::Suspended;
    cases.push(wrong);
    let mut wrong = actual.clone();
    wrong.terminal_tick = None;
    cases.push(wrong);
    for wrong in cases {
        assert!(!actor_matches(&wrong, "COTRTLIC", &artifact, &run));
    }
}

#[test]
fn menu_handoff_and_target_normal_return_terminal_events_are_distinct_and_owned() {
    for (name, menu) in [("COADM01C", true), ("COTRTLIC", false), ("COTRTUPC", false)] {
        let owner = owner(name);
        let terminal = event(&owner, menu);
        assert!(event_matches(&owner, &terminal, menu));
        assert!(!event_matches(&owner, &event(&owner, !menu), menu));
        let mut cases = Vec::new();
        let mut wrong = terminal.clone();
        wrong.run_unit_id = RunUnitId::new("foreign", InvocationLimits::default()).unwrap();
        cases.push(wrong);
        let mut wrong = terminal.clone();
        wrong.execution_id = ExecutionId::new("foreign", InvocationLimits::default()).unwrap();
        cases.push(wrong);
        let mut wrong = terminal.clone();
        wrong.sequence += 1;
        cases.push(wrong);
        let mut wrong = terminal.clone();
        wrong.attempt += 1;
        cases.push(wrong);
        let mut wrong = terminal.clone();
        wrong.tick += 1;
        cases.push(wrong);
        let mut wrong = terminal.clone();
        wrong.kind = LifecycleEventKind::Completed { return_code: 8 };
        cases.push(wrong);
        for wrong in cases {
            assert!(!event_matches(&owner, &wrong, menu));
        }
    }
}

fn installed() -> Installed {
    let payload = b"synthetic selected publisher; zero application credit".to_vec();
    let definition = OnlineProgramDefinition {
        name: "COADM01C".into(),
        artifact: ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(&payload)),
            InvocationLimits::default(),
        )
        .unwrap(),
        payload,
        manifest: VersionedArtifactManifest::V3(ArtifactManifest {
            compiler_generation: "synthetic-compiler".into(),
            target: CompileTarget::new("reference").unwrap(),
            options: CompileOptions::new(BTreeMap::new()).unwrap(),
            host_interfaces: BTreeSet::from(["mainframe-env.host@1".into()]),
            ir_contract: "mainframe-env.core-mir@1".into(),
            dialect_contracts: BTreeSet::from(["mainframe-env.cobol@1".into()]),
        }),
        semantic_identity: format!("semantic-sha256:{}", "a".repeat(64)),
    };
    Installed {
        programs: BTreeMap::from([(
            "COADM01C".into(),
            state(
                "online-program",
                "COADM01C",
                definition.artifact.as_str().as_bytes().to_vec(),
            ),
        )]),
        transactions: BTreeMap::from([(
            "CA00".into(),
            state("online-transaction", "CA00", b"COADM01C".to_vec()),
        )]),
        selected_artifacts: BTreeMap::from([(
            "COADM01C".into(),
            source_artifact_binding::publisher_record(&definition).unwrap(),
        )]),
    }
}
#[test]
fn complete_artifact_bytes_metadata_and_exact_names_reject_missing_extra_and_swaps() {
    let expected = installed();
    assert!(compare_installed(&expected, &expected).is_ok());
    let mut cases = Vec::new();
    let mut wrong = expected.clone();
    wrong.programs.clear();
    cases.push(wrong);
    let mut wrong = expected.clone();
    wrong
        .programs
        .insert("EXTRA".into(), expected.programs["COADM01C"].clone());
    cases.push(wrong);
    let mut wrong = expected.clone();
    wrong.transactions.get_mut("CA00").unwrap().payload = b"COMEN01C".to_vec();
    cases.push(wrong);
    let mut wrong = expected.clone();
    wrong
        .selected_artifacts
        .get_mut("COADM01C")
        .unwrap()
        .payload
        .push(0);
    cases.push(wrong);
    let mut wrong = expected.clone();
    wrong
        .selected_artifacts
        .get_mut("COADM01C")
        .unwrap()
        .payload_digest[0] ^= 1;
    cases.push(wrong);
    let mut wrong = expected.clone();
    wrong
        .selected_artifacts
        .get_mut("COADM01C")
        .unwrap()
        .media_type = "wrong/type".into();
    cases.push(wrong);
    let mut wrong = expected.clone();
    wrong
        .selected_artifacts
        .get_mut("COADM01C")
        .unwrap()
        .executable
        .as_mut()
        .unwrap()
        .compiler_generation = "foreign".into();
    cases.push(wrong);
    let mut wrong = expected.clone();
    wrong
        .selected_artifacts
        .get_mut("COADM01C")
        .unwrap()
        .executable
        .as_mut()
        .unwrap()
        .manifest_payload_digest[0] ^= 1;
    cases.push(wrong);
    for wrong in cases {
        assert!(compare_installed(&expected, &wrong).is_err());
    }
}

#[test]
fn skipped_selection_or_unobserved_reopen_cannot_bind_application_credit() {
    for (selected, reopened) in [
        (BTreeSet::new(), false),
        (BTreeSet::from([5]), true),
        (BTreeSet::from([6]), true),
        (BTreeSet::from([5, 6]), false),
    ] {
        let proof = Proof {
            expected: installed(),
            selected,
            reopened,
        };
        let mut observations = RouteObservations::default();
        assert!(proof.bind(&mut observations).is_err());
        assert!(observations.journeys().is_empty());
        assert!(observations.issues().is_empty());
    }
}
