//! Corruptions of the private assurance comparisons; these emit no observations.
use super::*;
use mainframe_env_cics::CicsTraceEntry;
use mainframe_env_host_api::{HostRequest, canonical_request_digest};

fn screen(fields: &[(&str, &[u8])]) -> String {
    let mut bytes = Vec::new();
    for (name, value) in fields {
        bytes.extend_from_slice(&(name.len() as u32).to_be_bytes());
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(&(value.len() as u32).to_be_bytes());
        bytes.extend_from_slice(value);
    }
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn page(number: usize, cp037: bool) -> serde_json::Value {
    let expected = match number {
        1 => oracles::PAGE1,
        2 => oracles::PAGE2,
        _ => oracles::PAGE3,
    };
    let fields = expected
        .iter()
        .map(|(name, ascii, ebcdic)| (*name, if cp037 { *ebcdic } else { *ascii }))
        .collect::<Vec<_>>();
    serde_json::json!({"schema_version":"mainframe-env.cics-terminal@1",
        "session":"terminal-source-1","principal":"WEBUSER","mapset":"COCRDLI",
        "rows":24,"columns":80,"transaction":if number == 1 { "CM00" } else { "CCLI" },
        "aid":if number == 1 { 0x7d } else { 0xf8 },
        "map":"CCRDLIA","connected":true,"suspended":true,"version":9,
        "run_unit":"run-source-1","screen_base64":screen(&fields)})
}

#[test]
fn process_page_refuses_identity_bytes_width_order_missing_and_extra() {
    for number in 1..=3 {
        for cp037 in [false, true] {
            let valid = page(number, cp037);
            assert!(oracles::page(&valid, "terminal-source-1", number, true).is_ok());
            for (name, value) in [
                (
                    "schema_version",
                    serde_json::json!("mainframe-env.cics-terminal@2"),
                ),
                ("rows", serde_json::json!(25)),
                ("columns", serde_json::json!(79)),
                ("transaction", serde_json::json!("CC00")),
                (
                    "aid",
                    serde_json::json!(if number == 1 { 0xf8 } else { 0x7d }),
                ),
                ("session", serde_json::json!("foreign-session")),
                ("principal", serde_json::json!("WEBADM")),
                ("mapset", serde_json::json!("COACTVW")),
                ("map", serde_json::json!("CCRDLIB")),
                ("connected", serde_json::json!(false)),
                ("suspended", serde_json::json!(false)),
                ("version", serde_json::json!(0)),
                ("run_unit", serde_json::json!("")),
            ] {
                let mut corrupt = valid.clone();
                corrupt[name] = value;
                assert!(
                    oracles::page(&corrupt, "terminal-source-1", number, true).is_err(),
                    "{name}"
                );
            }
            let decoded = online_screen_fields(&valid).unwrap();
            for name in decoded.keys() {
                let mut fields = decoded.clone();
                fields.get_mut(name).unwrap()[0] ^= 1;
                let mut corrupt = valid.clone();
                corrupt["screen_base64"] = serde_json::json!(screen(
                    &fields
                        .iter()
                        .map(|(n, v)| (n.as_str(), v.as_slice()))
                        .collect::<Vec<_>>()
                ));
                assert!(
                    oracles::page(&corrupt, "terminal-source-1", number, true).is_err(),
                    "byte {name}"
                );
                let mut fields = decoded.clone();
                fields.get_mut(name).unwrap().push(0);
                corrupt["screen_base64"] = serde_json::json!(screen(
                    &fields
                        .iter()
                        .map(|(n, v)| (n.as_str(), v.as_slice()))
                        .collect::<Vec<_>>()
                ));
                assert!(
                    oracles::page(&corrupt, "terminal-source-1", number, true).is_err(),
                    "width {name}"
                );
            }
            for mode in 0..4 {
                let mut fields = decoded.clone();
                match mode {
                    0 => {
                        fields.remove("CRDNUM4");
                    }
                    1 => {
                        fields.insert("CRDNUM8".into(), fields["CRDNUM1"].clone());
                    }
                    2 => {
                        let first = fields["CRDNUM1"].clone();
                        let second = fields["CRDNUM2"].clone();
                        fields.insert("CRDNUM1".into(), second);
                        fields.insert("CRDNUM2".into(), first);
                    }
                    _ => {
                        fields.insert("PAGENO".into(), b"9  ".to_vec());
                    }
                }
                let mut corrupt = valid.clone();
                corrupt["screen_base64"] = serde_json::json!(screen(
                    &fields
                        .iter()
                        .map(|(n, v)| (n.as_str(), v.as_slice()))
                        .collect::<Vec<_>>()
                ));
                assert!(
                    oracles::page(&corrupt, "terminal-source-1", number, true).is_err(),
                    "row mode {mode}"
                );
            }
        }
    }
}

fn frame(magic: &[u8], fields: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = magic.to_vec();
    for field in fields {
        bytes.extend_from_slice(&(field.len() as u32).to_be_bytes());
        bytes.extend_from_slice(field);
    }
    bytes
}

fn context(number: usize) -> Vec<u8> {
    let slices = match number {
        1 => oracles::CONTEXT1,
        2 => oracles::CONTEXT2,
        _ => oracles::CONTEXT3,
    };
    let mut bytes = vec![0; 2000];
    for (offset, ascii, _) in slices {
        bytes[*offset..*offset + ascii.len()].copy_from_slice(ascii);
    }
    bytes
}

#[test]
fn process_continuation_refuses_wrong_page_claimant_and_corrupt_framing() {
    let fields = vec![
        b"CCLI".to_vec(),
        context(2),
        Vec::new(),
        b"source-invocation:4".to_vec(),
    ];
    let valid = frame(b"MECC1", &fields);
    assert!(evidence::continuation_payload(&valid, 2).is_ok());
    assert!(evidence::continuation_payload(&valid, 1).is_err());
    assert!(evidence::continuation_payload(&valid, 3).is_err());
    for end in 0..valid.len() {
        assert!(evidence::continuation_payload(&valid[..end], 2).is_err());
    }
    for (at, replacement) in [
        (0, b"CC00".to_vec()),
        (1, vec![0; 1999]),
        (2, b"foreign-run".to_vec()),
        (3, Vec::new()),
    ] {
        let mut corrupt = fields.clone();
        corrupt[at] = replacement;
        assert!(evidence::continuation_payload(&frame(b"MECC1", &corrupt), 2).is_err());
    }
    for (offset, ascii, _) in oracles::CONTEXT2 {
        let mut corrupt = fields.clone();
        corrupt[1][*offset + ascii.len() - 1] ^= 1;
        assert!(evidence::continuation_payload(&frame(b"MECC1", &corrupt), 2).is_err());
    }
    let mut trailing = valid.clone();
    trailing.push(0);
    assert!(evidence::continuation_payload(&trailing, 2).is_err());
    let mut overflow = valid;
    overflow[5..9].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(evidence::continuation_payload(&overflow, 2).is_err());
}

#[test]
fn process_machine_refuses_codec_binding_generations_and_transfer_corruption() {
    let artifact = ArtifactRef::new(
        format!("sha256:{}", "a".repeat(64)),
        InvocationLimits::default(),
    )
    .unwrap();
    let generations = [
        "host.security.authorize",
        "host.cics.execute",
        "host.dataset.read",
        "host.dataset.write",
        "host.db2.read",
        "host.db2.write",
        "host.ims.read",
        "host.ims.write",
        "host.mq.read",
        "host.mq.write",
        "host.program.invoke",
        "host.clock",
    ]
    .into_iter()
    .map(|name| (name.to_string(), "1".to_string()))
    .collect::<BTreeMap<_, _>>();
    let fields = vec![
        b"COCRDLIC".to_vec(),
        artifact.as_str().as_bytes().to_vec(),
        encode(&generations).unwrap(),
        vec![0],
        b"mainframe-env.reference-machine-checkpoint@12".to_vec(),
        vec![1],
        Vec::new(),
    ];
    // This is solely the envelope comparator control. The positive producer
    // requires the real machine checkpoint + actual compiled route restoration.
    let valid = frame(b"MEOM4", &fields);
    assert!(evidence::machine_payload(&valid, &artifact).is_ok());
    for end in 0..valid.len() {
        assert!(evidence::machine_payload(&valid[..end], &artifact).is_err());
    }
    for (at, replacement) in [
        (0, b"COMEN01C".to_vec()),
        (1, b"foreign-artifact".to_vec()),
        (3, Vec::new()),
        (4, b"mainframe-env.reference-machine-checkpoint@11".to_vec()),
        (5, Vec::new()),
        (6, vec![1]),
    ] {
        let mut corrupt = fields.clone();
        corrupt[at] = replacement;
        assert!(evidence::machine_payload(&frame(b"MEOM4", &corrupt), &artifact).is_err());
    }
    for mode in 0..4 {
        let mut generations = generations.clone();
        match mode {
            0 => {
                generations.remove("host.clock");
            }
            1 => {
                generations.insert("host.foreign".into(), "1".into());
            }
            2 => {
                generations.insert("host.clock".into(), "2".into());
            }
            _ => {
                generations.insert("host.clock".into(), "".into());
            }
        }
        let mut corrupt = fields.clone();
        corrupt[2] = encode(&generations).unwrap();
        assert!(evidence::machine_payload(&frame(b"MEOM4", &corrupt), &artifact).is_err());
    }
    let mut noncanonical = fields.clone();
    noncanonical[2].insert(1, b' ');
    assert!(evidence::machine_payload(&frame(b"MEOM4", &noncanonical), &artifact).is_err());
    let mut legacy = valid.clone();
    legacy[4] = b'3';
    assert!(evidence::machine_payload(&legacy, &artifact).is_err());
    let mut trailing = valid;
    trailing.push(0);
    assert!(evidence::machine_payload(&trailing, &artifact).is_err());
}

#[test]
fn process_trace_refuses_direction_order_count_condition_and_payload_corruption() {
    for suspended in [false, true] {
        let valid = evidence::expected_operations(suspended)
            .into_iter()
            .map(|operation| CicsTraceEntry {
                operation,
                outcome: "NORMAL".into(),
                response: 0,
                response2: 0,
                payload_bytes: match operation {
                    CicsOperation::ReadNext => 150,
                    CicsOperation::Return => 2000,
                    CicsOperation::SendMap => 500,
                    CicsOperation::ReceiveMap if !suspended => 376,
                    _ => 0,
                },
            })
            .collect::<Vec<_>>();
        assert!(evidence::trace_matches(&valid, suspended));
        assert!(!evidence::trace_matches(&valid, !suspended));
        for payload in [0, 375, 376, 377] {
            if payload == if suspended { 0 } else { 376 } {
                continue;
            }
            let mut wrong_receive = valid.clone();
            wrong_receive[0].payload_bytes = payload;
            assert!(!evidence::trace_matches(&wrong_receive, suspended));
        }
        for at in 0..valid.len() {
            for mode in 0..5 {
                let mut corrupt = valid.clone();
                match mode {
                    0 => {
                        corrupt[at].operation = CicsOperation::ReadPrev;
                    }
                    1 => {
                        corrupt[at].outcome = "NOTFND".into();
                    }
                    2 => {
                        corrupt[at].response = 13;
                    }
                    3 => {
                        corrupt[at].response2 = 1;
                    }
                    _ => {
                        corrupt[at].payload_bytes = 65537;
                    }
                }
                assert!(!evidence::trace_matches(&corrupt, suspended));
            }
        }
        let mut missing = valid.clone();
        missing.remove(0);
        assert!(!evidence::trace_matches(&missing, suspended));
        let mut extra = valid.clone();
        extra.push(valid[0].clone());
        assert!(!evidence::trace_matches(&extra, suspended));
        if !suspended {
            let mut reordered = valid;
            reordered.swap(1, 2);
            assert!(!evidence::trace_matches(&reordered, suspended));
        }
    }
}

#[test]
fn process_receive_fingerprint_refuses_argument_schema_policy_and_effect_identity() {
    let key = IdempotencyKey::new("source-invocation:4", InvocationLimits::default()).unwrap();
    let expected = oracles::receive_digest(4, key.clone()).unwrap();
    for mode in 0..8 {
        let mut request = oracles::receive(4, key.clone()).unwrap();
        let HostRequest::Cics(ref mut cics) = request else {
            panic!("typed CICS request required")
        };
        match mode {
            0 => {
                cics.arguments.remove("INTO");
            }
            1 => {
                cics.arguments.insert(
                    "LENGTH".into(),
                    BoundedPayload::new(
                        "mainframe-env.cics.argument@1",
                        b"2000".to_vec(),
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                );
            }
            2 => {
                cics.arguments.insert(
                    "MAP".into(),
                    BoundedPayload::new(
                        "mainframe-env.cics.storage-value@1",
                        b"CCRDLIB".to_vec(),
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                );
            }
            3 => {
                cics.arguments.insert(
                    "MAPSET".into(),
                    BoundedPayload::new(
                        "mainframe-env.cics.argument@1",
                        b"COCRDLI".to_vec(),
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                );
            }
            4 => {
                cics.condition_policy = CicsConditionPolicy::Respond {
                    response_field: "WS-RESP-CD".into(),
                    response2_field: None,
                };
            }
            5 => {
                cics.mutation.as_mut().unwrap().sequence = 5;
            }
            6 => {
                cics.mutation.as_mut().unwrap().idempotency_key =
                    IdempotencyKey::new("foreign:4", InvocationLimits::default()).unwrap();
            }
            _ => {
                cics.mutation.as_mut().unwrap().transaction = Some("CC00".into());
            }
        }
        assert_ne!(
            canonical_request_digest(&request).unwrap(),
            expected,
            "fingerprint mode {mode}"
        );
    }
}

fn receipts() -> (Marker, Receipt, Receipt) {
    let artifact = format!("sha256:{}", "a".repeat(64));
    let marker = Marker {
        root: "/private-owned-fixture".into(),
        root_device: 1,
        root_inode: 2,
        nonce: "b".repeat(64),
        parent_pid: 10,
        executable: "/qualified-binary".into(),
        executable_digest: "c".repeat(64),
        corpus_commit: CORPUS_COMMIT.into(),
        corpus_content: CORPUS_CONTENT.into(),
    };
    let first = Receipt {
        phase: "prepare".into(),
        nonce: marker.nonce.clone(),
        pid: 11,
        parent_pid: 10,
        executable_digest: marker.executable_digest.clone(),
        corpus_commit: CORPUS_COMMIT.into(),
        corpus_content: CORPUS_CONTENT.into(),
        session: "terminal-source-1".into(),
        artifact: artifact.clone(),
        owner: evidence::Owner {
            execution: "execution-source-1".into(),
            run: "run-source-1".into(),
            artifact,
            version: 6,
            attempt: 1,
            tick: 10,
            retained_digest: "d".repeat(64),
        },
        source_binding_verified: true,
        actual_suspended: true,
        page: 2,
        worker_count: 0,
        drained: true,
        disconnected: false,
    };
    let mut second = first.clone();
    second.phase = "resume".into();
    second.pid = 12;
    second.actual_suspended = false;
    second.disconnected = true;
    second.page = 3;
    second.owner.execution = "execution-source-2".into();
    second.owner.run = "run-source-2".into();
    (marker, first, second)
}

#[test]
fn process_receipts_refuse_forged_identity_stage_drain_pair_and_framing() {
    let (marker, first, second) = receipts();
    assert!(receipt_matches(&first, &marker, "prepare", 11));
    assert!(receipt_matches(&second, &marker, "resume", 12));
    assert!(receipt_pair_matches(&first, &second));
    for mode in 0..20 {
        let mut corrupt = first.clone();
        match mode {
            0 => {
                corrupt.phase = "resume".into();
            }
            1 => {
                corrupt.nonce = "e".repeat(64);
            }
            2 => {
                corrupt.pid = marker.parent_pid;
            }
            3 => {
                corrupt.parent_pid = 99;
            }
            4 => {
                corrupt.executable_digest = "e".repeat(64);
            }
            5 => {
                corrupt.corpus_commit = "foreign".into();
            }
            6 => {
                corrupt.corpus_content = "foreign".into();
            }
            7 => {
                corrupt.session.clear();
            }
            8 => {
                corrupt.owner.execution.clear();
            }
            9 => {
                corrupt.owner.run.clear();
            }
            10 => {
                corrupt.owner.artifact = "foreign".into();
            }
            11 => {
                corrupt.owner.version = 0;
            }
            12 => {
                corrupt.owner.attempt = 0;
            }
            13 => {
                corrupt.owner.tick = 0;
            }
            14 => {
                corrupt.source_binding_verified = false;
            }
            15 => {
                corrupt.actual_suspended = false;
            }
            16 => {
                corrupt.page = 1;
            }
            17 => {
                corrupt.worker_count = 1;
            }
            18 => {
                corrupt.drained = false;
            }
            _ => {
                corrupt.disconnected = true;
            }
        }
        assert!(
            !receipt_matches(&corrupt, &marker, "prepare", 11),
            "receipt mode {mode}"
        );
    }
    for mode in 0..5 {
        let mut corrupt = second.clone();
        match mode {
            0 => {
                corrupt.pid = first.pid;
            }
            1 => {
                corrupt.session = "foreign-session".into();
            }
            2 => {
                corrupt.artifact = format!("sha256:{}", "e".repeat(64));
            }
            3 => {
                corrupt.owner.execution = first.owner.execution.clone();
            }
            _ => {
                corrupt.owner.run = first.owner.run.clone();
            }
        }
        assert!(!receipt_pair_matches(&first, &corrupt), "pair mode {mode}");
    }
    let valid = encode(&first).unwrap();
    assert!(decode::<Receipt>(&valid).is_ok());
    for end in 0..valid.len() {
        assert!(decode::<Receipt>(&valid[..end]).is_err());
    }
    let mut trailing = valid.clone();
    trailing.push(0);
    assert!(decode::<Receipt>(&trailing).is_err());
    let mut duplicate = valid[..valid.len() - 1].to_vec();
    duplicate.extend_from_slice(br#","phase":"prepare"}"#);
    assert!(decode::<Receipt>(&duplicate).is_err());
    let mut unknown = valid[..valid.len() - 1].to_vec();
    unknown.extend_from_slice(br#","unknown":true}"#);
    assert!(decode::<Receipt>(&unknown).is_err());
}

#[test]
#[cfg(unix)]
fn process_private_files_and_cleanup_refuse_unowned_inputs() {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt, symlink};
    let parent = fs::canonicalize(env::temp_dir()).unwrap();
    let nonce = evidence::hash(
        format!("{}:{:?}", std::process::id(), std::time::SystemTime::now()).as_bytes(),
    );
    let root = parent.join(format!(
        "carddemo-process-controls-{}-{nonce}",
        std::process::id()
    ));
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let owned_root = protocol::root_identity(&root).unwrap();
    assert!(!protocol::is_absent(&root).unwrap());
    assert!(protocol::is_absent(&root.join("not-created")).unwrap());
    symlink(root.join("missing-target"), root.join("dangling")).unwrap();
    assert!(!protocol::is_absent(&root.join("dangling")).unwrap());
    fs::remove_file(root.join("dangling")).unwrap();
    for kind in [io::ErrorKind::PermissionDenied, io::ErrorKind::Other] {
        assert!(protocol::absence_result(Err(io::Error::new(kind, "metadata refused"))).is_err());
    }
    let (mut marker, _, _) = receipts();
    marker.root_device = owned_root.0;
    marker.root_inode = owned_root.1;
    marker.root = root.to_str().unwrap().into();
    marker.nonce = nonce;
    marker.parent_pid = std::process::id();
    let owner = encode(&marker).unwrap();
    protocol::write_private(&root.join("owner.json"), &owner, MAX_SECRET).unwrap();
    let path = root.join("bounded.json");
    assert!(protocol::write_private(&path, b"too many bytes", 1).is_err());
    assert!(!path.exists());
    protocol::write_private(&path, b"bounded", 7).unwrap();
    assert!(protocol::write_private(&path, b"replace", 7).is_err());
    assert_eq!(protocol::read_private(&path, 7).unwrap(), b"bounded");
    assert!(protocol::read_private(&path, 6).is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(protocol::read_private(&path, 7).is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    symlink(&path, root.join("alias")).unwrap();
    assert!(protocol::read_private(&root.join("alias"), 7).is_err());
    assert!(cleanup(&root, &owner, owned_root).is_err());
    assert!(root.exists() && path.exists());
    fs::remove_file(root.join("alias")).unwrap();
    assert!(cleanup(&root, b"foreign owner marker", owned_root).is_err());
    assert_eq!(protocol::read_private(&path, 7).unwrap(), b"bounded");
    // A replacement at the same canonical path with the copied marker must
    // refuse both child admission and recursive cleanup, retaining both roots.
    let retained = root.with_file_name(format!(
        "{}-retained",
        root.file_name().unwrap().to_str().unwrap()
    ));
    fs::rename(&root, &retained).unwrap();
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let replacement_identity = protocol::root_identity(&root).unwrap();
    assert_ne!(replacement_identity, owned_root);
    protocol::write_private(&root.join("owner.json"), &owner, MAX_SECRET).unwrap();
    assert!(protocol::require_root_identity(&root, owned_root).is_err());
    assert!(cleanup(&root, &owner, owned_root).is_err());
    let refused_write = root.join("refused-output.json");
    assert!(
        protocol::write_owned(
            &root,
            &refused_write,
            b"source-owned-output",
            64,
            owned_root
        )
        .is_err()
    );
    assert!(!refused_write.exists());
    assert!(open_server(&root, true, owned_root).is_err());
    assert!(!root.join("state.db").exists() && !root.join("artifacts").exists());
    assert!(root.exists() && retained.exists() && retained.join("bounded.json").exists());
    assert_eq!(
        protocol::read_private(&root.join("owner.json"), MAX_SECRET).unwrap(),
        owner
    );
    // These two exact replacement entries were created by this control. Restore
    // the original root before admitting its original checked cleanup.
    assert_eq!(
        protocol::root_identity(&root).unwrap(),
        replacement_identity
    );
    fs::remove_file(root.join("owner.json")).unwrap();
    fs::remove_dir(&root).unwrap();
    fs::rename(&retained, &root).unwrap();
    cleanup(&root, &owner, owned_root).unwrap();
    assert!(protocol::is_absent(&root).unwrap());
}
