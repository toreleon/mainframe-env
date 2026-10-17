use super::*;
use std::cell::Cell;

#[test]
#[ignore = "explicit pinned corpus and fresh private CARDDEMO_J10_CONTROLS_EVIDENCE; controls only, run serial"]
fn handwritten_repro_control_retains_exact_operands() {
    pinned(CONTROL);
    let records = f80(&[b" REPRO INFILE(FILEIN) OUTFILE(FILEOUT)".to_vec()]).unwrap();
    let raw = text_records(&records);
    assert_eq!(validate_idcams_control(&raw), Ok(1));
    let statements = parse_idcams_control(&raw).unwrap();
    let [AmsStatement::Command(command)] = statements.as_slice() else {
        panic!("not one actual command")
    };
    assert_eq!(command.id(), "repro");
    assert_eq!(command.label(), "REPRO");
    assert_eq!(command.source(), "REPRO INFILE(FILEIN) OUTFILE(FILEOUT)");
}

#[test]
#[ignore = "explicit pinned corpus; full source refusal is preserved, not AMS semantic acceptance"]
fn full_reproct_current_comment_refusal_refuses_actual_router_admission() {
    let rows = control_rows();
    assert!(rows[..14].iter().all(|row| {
        row.starts_with(b"/*")
            && row
                .iter()
                .rposition(|byte| *byte != b' ')
                .is_some_and(|end| row[..=end].ends_with(b"*/"))
    }));
    let control = text_records(&rows);
    assert_eq!(
        validate_idcams_control(&control),
        Err(HostProblem::Unsupported)
    );
    assert_eq!(
        parse_idcams_control(&control),
        Err(HostProblem::Unsupported)
    );
    let input = ProgramInput {
        parameter: None,
        dds: vec![dd("SYSIN", &control)],
        dd_records: BTreeMap::new(),
        execution: None,
    };
    assert_eq!(route("IDCAMS", &input), Err(HostProblem::Unsupported));
}

#[test]
#[ignore = "explicit pinned full primaries and procedure; conversion proof only"]
fn pinned_reproc_jobs_bind_control_member_and_scoped_overrides() {
    let procedure = String::from_utf8(pinned("app/proc/REPROC.prc")).unwrap();
    for relative in ["app/jcl/TRANBKP.jcl", "app/jcl/TRANREPT.jcl"] {
        let job = String::from_utf8(pinned(relative)).unwrap();
        assert!(
            binding(&job, &procedure),
            "actual full offered binding differs"
        );
        let changed = job.replace(
            "CNTLLIB=AWS.M2.CARDDEMO.CNTL",
            "CNTLLIB=AWS.M2.CARDDEMO.OTHER",
        );
        assert_ne!(changed, job);
        assert!(!binding(&changed, &procedure));
        let omitted = job
            .lines()
            .filter(|line| !line.contains("CNTLLIB="))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!binding(&omitted, &procedure));
        let changed_member = procedure.replace("(REPROCT)", "(OTHER)");
        assert_ne!(changed_member, procedure);
        assert!(!binding(&job, &changed_member));
    }
}

#[test]
#[ignore = "fresh private evidence, exclusively copied source, zero target mutation on drift"]
fn same_shape_reproct_source_drift_refuses_before_mutation() {
    let raw = pinned(CONTROL);
    let mut changed = raw.clone();
    changed[3] = b'X';
    assert_eq!(raw.len(), changed.len());
    assert_eq!(
        raw.split(|byte| *byte == b'\n').count(),
        changed.split(|byte| *byte == b'\n').count()
    );
    let mut owner = Owner::claim("preflight-source-drift");
    owner.write_source("REPROCT.ctl", &changed).unwrap();
    let (store, service) = provider();
    let calls = Cell::new(0);
    assert_eq!(
        pin_then_publish(
            &owner,
            &owner.owned.join("REPROCT.ctl"),
            CONTROL_SHA,
            || {
                calls.set(calls.get() + 1);
                create_member(&service, control_rows());
            }
        ),
        Err("point-of-use source pin differs")
    );
    assert_eq!(calls.get(), 0);
    assert_eq!(
        service.invoke(DatasetRequest::Attributes {
            dataset: DatasetName::new(DATASET, 128).unwrap()
        }),
        Err(HostProblem::NotFound)
    );
    close_provider(store, service);
    owner
        .finish("source pin refused before create/write")
        .unwrap();
}

#[test]
#[ignore = "fresh private evidence and source replacement after successful preflight"]
fn postpreflight_reproct_replacement_refuses_before_write() {
    let raw = pinned(CONTROL);
    let mut owner = Owner::claim("postpreflight-source-drift");
    owner.write_source("REPROCT.ctl", &raw).unwrap();
    let source = owner.owned.join("REPROCT.ctl");
    let initial = read_file(&source, true).unwrap().1;
    assert_eq!(initial.sha256, CONTROL_SHA);
    let mut changed = raw;
    changed[3] = b'X';
    owner.replace_source("REPROCT.ctl", &changed).unwrap();
    assert_ne!(read_file(&source, true).unwrap().1.sha256, initial.sha256);
    let calls = Cell::new(0);
    assert_eq!(
        pin_then_publish(&owner, &source, CONTROL_SHA, || calls.set(calls.get() + 1)),
        Err("point-of-use source pin differs")
    );
    assert_eq!(calls.get(), 0);
    owner
        .finish("post-preflight replacement refused before write")
        .unwrap();
}

#[test]
#[ignore = "explicit pinned full ASCII F80 input; actual local provider readback only"]
fn explicit_ascii_f80_reproct_provider_member_is_complete() {
    let expected = control_rows();
    let (store, service) = provider();
    let written = create_member(&service, expected.clone());
    let observed = capture(&service);
    assert_eq!(observed.record_version, written);
    assert!(complete(&observed, &expected));
    close_provider(store, service);
}

#[test]
#[ignore = "independent complete source rows and actual provider observation; no count-only proof"]
fn reproct_complete_comparator_refuses_each_same_shape_corruption() {
    let expected = control_rows();
    let (store, service) = provider();
    create_member(&service, expected.clone());
    let actual = capture(&service);
    assert!(complete(&actual, &expected));
    for variant in 0..13 {
        let mut bad = actual.clone();
        match variant {
            0 => bad.rows = vec![bad.rows.last().unwrap().clone()],
            1 => {
                bad.rows.remove(3);
            }
            2 => bad.rows.swap(0, 1),
            3 => bad.rows[3][3] ^= 1,
            4 => bad.rows[14][79] = b'X',
            5 => bad.member = "OTHER".into(),
            6 => bad.attributes.ccsid = Some(37),
            7 => bad.identities[3] = bad.identities[2].clone(),
            8 => bad.after_version += 1,
            9 => bad.record_version += 1,
            10 => bad.more = true,
            11 => bad.names.push("OTHER".into()),
            _ => bad.after_attributes.logical_record_length = 81,
        }
        assert!(
            !complete(&bad, &expected),
            "variant {variant} escaped complete comparison"
        );
    }
    assert_eq!(
        service.invoke(DatasetRequest::Read {
            dataset: DatasetName::new(DATASET, 128).unwrap(),
            member: Some(MemberName::new("OTHER", 128).unwrap()),
            key: None,
            max_records: 4096,
            control: Default::default(),
        }),
        Err(HostProblem::NotFound)
    );
    close_provider(store, service);
}

#[test]
#[ignore = "explicit ASCII/F80 source profile; preserve current bounds and no lossy conversion"]
fn reproct_ascii_f80_construction_refuses_lossy_inputs() {
    assert_eq!(control_rows().len(), 15);
    assert!(f80(&[vec![b'A'; 81]]).is_err());
    assert!(f80(&["é".as_bytes().to_vec()]).is_err());
    assert!(f80(&[b"A\nB".to_vec()]).is_err());
    assert!(f80(&[]).is_err());
    assert!(f80(&vec![Vec::new(); 4097]).is_err());
    assert_eq!(
        f80(&[Vec::new(), vec![b'A'; 80]]).unwrap(),
        [vec![b' '; 80], vec![b'A'; 80]]
    );
}

#[test]
#[ignore = "fresh private evidence; real failure retention, exact owned disposal and KEEP refusals"]
fn failed_reproct_owner_retains_failure_and_checks_disposal() {
    let rows = control_rows();
    let control = text_records(&rows);
    let failure = validate_idcams_control(&control).unwrap_err();
    assert_eq!(failure, HostProblem::Unsupported);
    let (store, service) = provider();
    create_member(&service, rows.clone());
    let observed = capture(&service);
    assert!(complete(&observed, &rows));
    let mut owner = Owner::claim("retained-parser-failure");
    owner.write_source("REPROCT.ctl", &pinned(CONTROL)).unwrap();
    owner.retain("parser-failure", &serde_json::json!({"primary":"Unsupported","records":observed.rows.iter().enumerate().map(|(ordinal,row)|serde_json::json!({"ordinal":ordinal,"bytes":row.len(),"sha256":digest(row),"identity":observed.identities[ordinal]})).collect::<Vec<_>>(),"version":observed.record_version,"application_credit":0,"vendor_credit":0})).unwrap();
    close_provider(store, service);
    owner.finish("Unsupported").unwrap();
    assert!(
        matches!(fs::symlink_metadata(&owner.owned), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
    );
    let retained: serde_json::Value = serde_json::from_slice(
        &read_file(&owner.evidence.join("after-cleanup.json"), true)
            .unwrap()
            .0,
    )
    .unwrap();
    assert_eq!(retained["primary"], "Unsupported");
    assert_eq!(retained["issue_observations"], serde_json::json!([]));
    assert_eq!(retained["application_credit"], 0);
    assert_eq!(retained["vendor_credit"], 0);

    let mut retention_failure = Owner::claim("retention-failure-keep");
    retention_failure
        .write_source("REPROCT.ctl", &pinned(CONTROL))
        .unwrap();
    fs::write(
        retention_failure.evidence.join("before-cleanup.json"),
        b"UNKNOWN",
    )
    .unwrap();
    assert!(retention_failure.finish("Unsupported").is_err());
    assert_eq!(
        read_file(&retention_failure.owned.join("REPROCT.ctl"), true)
            .unwrap()
            .1
            .sha256,
        CONTROL_SHA
    );
    assert_eq!(
        fs::read(retention_failure.evidence.join("before-cleanup.json")).unwrap(),
        b"UNKNOWN"
    );

    let mut replaced = Owner::claim("replaced-owned-state-keep");
    replaced
        .write_source("REPROCT.ctl", &pinned(CONTROL))
        .unwrap();
    let original = replaced.evidence.join("retained-original-state");
    fs::rename(&replaced.owned, &original).unwrap();
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&replaced.owned)
        .unwrap();
    fs::write(replaced.owned.join("sentinel"), b"UNKNOWN").unwrap();
    assert!(replaced.write_source("NEXT.ctl", b"MUTANT").is_err());
    assert!(replaced.finish("Unsupported").is_err());
    assert_eq!(
        fs::read(replaced.owned.join("sentinel")).unwrap(),
        b"UNKNOWN"
    );
    assert!(
        matches!(fs::symlink_metadata(replaced.owned.join("NEXT.ctl")), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
    );
    assert_eq!(
        read_file(&original.join("REPROCT.ctl"), true)
            .unwrap()
            .1
            .sha256,
        CONTROL_SHA
    );
    // Failed/unknown namespaces remain retained for the actual root producer.
}

#[test]
#[ignore = "explicit current opaque record contract; same supervised selected control owner"]
fn iebgener_keeps_legacy_and_opaque_records_without_ccsid_inference() {
    let mut input = ProgramInput {
        parameter: None,
        dds: vec![dd("SYSUT1", b"FIRST\nSECOND\n"), dd("SYSUT2", b"")],
        dd_records: BTreeMap::new(),
        execution: None,
    };
    input.dds[1].ccsid = Some(37);
    assert_eq!(
        copy(&input).dd_outputs["SYSUT2"],
        [b"FIRST".to_vec(), b"SECOND".to_vec()]
    );
    let expected = vec![Vec::new(), b"A\nB".to_vec(), vec![0, 255, 0x40, 0x81]];
    input.dd_records.insert("SYSUT1".into(), expected.clone());
    for ccsid in [Some(37), Some(1208), None] {
        input.dds[1].ccsid = ccsid;
        let actual = copy(&input);
        assert_eq!(actual.return_code, 0);
        assert!(actual.termination.is_none());
        assert_eq!(actual.dd_outputs["SYSUT2"], expected);
    }
}

#[test]
#[ignore = "explicit Cp037 codec oracle only; no automatic JES admission or J10 seed credit"]
fn explicit_cp037_dusrsec_codec_matches_pinned_seed_and_refuses_loss() {
    let raw = pinned("app/jcl/DUSRSECJ.jcl");
    let source = String::from_utf8(raw).unwrap();
    let lines = source.lines().collect::<Vec<_>>();
    let start = lines
        .iter()
        .position(|line| *line == "//SYSUT1   DD *")
        .unwrap()
        + 1;
    let end = lines[start..]
        .iter()
        .position(|line| *line == "/*")
        .unwrap()
        + start;
    assert_eq!(end - start, 10);
    let mut native = Vec::new();
    for line in &lines[start..end] {
        assert_eq!(line.len(), 57);
        let mut record = CodePage::Cp037.encode(line, 80).unwrap();
        assert_eq!(record.len(), 57);
        assert_eq!(CodePage::Cp037.decode(&record, 228).unwrap(), *line);
        record.resize(80, 0x40);
        native.extend(record);
    }
    assert_eq!(native.len(), 800);
    assert_eq!(
        digest(&native),
        "8608d4b8a03f7587c1493282f9337ab24fdf527d0aadbdb85dc3713dccdc6fc8"
    );
    assert_eq!(
        CodePage::Cp037.encode("😀", 80),
        Err(EncodingProblem::UnmappableCharacter)
    );
    assert_eq!(
        CodePage::Cp037.encode("A", 0),
        Err(EncodingProblem::OutputLimitExceeded)
    );
    assert_eq!(
        CodePage::from_ccsid(1208),
        Err(EncodingProblem::UnsupportedCodePage(1208))
    );
    assert_eq!(
        CodePage::from_ccsid(930),
        Err(EncodingProblem::UnsupportedCodePage(930))
    );
}

#[test]
#[ignore = "preserve strict UTF8 host text and actual immutable inline source bytes"]
fn utf8_control_source_and_inline_spans_do_not_decode_native_records() {
    assert_eq!(
        parse_idcams_control(b"\xffREPRO INFILE(FILEIN) OUTFILE(FILEOUT)"),
        Err(HostProblem::Malformed)
    );
    let native = CodePage::Cp037
        .encode(" REPRO INFILE(FILEIN) OUTFILE(FILEOUT)", 80)
        .unwrap();
    assert_eq!(parse_idcams_control(&native), Err(HostProblem::Malformed));
    let source = "//J JOB\r\n//S EXEC PGM=IEBGENER\r\n//SYSUT1 DD *\r\ncontrôle\r\nTWO\n/*\n//SYSUT2 DD SYSOUT=*\n";
    let conversion = convert_jcl(
        &JclBundle {
            primary: source.into(),
            ..JclBundle::default()
        },
        JclConversionLimits::default(),
    )
    .unwrap();
    assert!(conversion.is_valid());
    let statement = conversion
        .plan()
        .unwrap()
        .statements()
        .iter()
        .find(|statement| statement.name() == Some("S.SYSUT1"))
        .unwrap();
    assert_eq!(statement.inline_data(), "contrôle\r\nTWO\n".as_bytes());
    assert!(!statement.source().origins().is_empty());
}
