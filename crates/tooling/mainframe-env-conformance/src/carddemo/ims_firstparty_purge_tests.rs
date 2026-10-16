//! Preparation/comparator controls only. No synthetic test value earns J18 credit.
use super::*;
use mainframe_env_batch::{JclBundle, JclLimits, parse_jcl};
use mainframe_env_host_api::{ImsOperation, ImsRequest, ImsResult, ImsSegment};

fn corpus() -> PathBuf {
    PathBuf::from(env::var_os("CARDDEMO_CORPUS_DIR").expect("pinned public corpus required"))
}

#[test]
#[ignore = "requires the independently retained pinned CardDemo public source; zero journey credit"]
fn original_cbpaup0c_compiles_without_replacing_its_jcl_or_source() {
    let prepared = prepare(&corpus()).unwrap();
    assert_eq!(prepared.program.name, "CBPAUP0C");
    assert!(!prepared.program.payload.is_empty());
    assert_eq!(prepared.bundle_id.len(), 64);
    let mut plan = parse_jcl(
        &JclBundle {
            primary: prepared.jcl.clone(),
            ..Default::default()
        },
        JclLimits::default(),
    )
    .unwrap();
    compare_original_plan(&prepared, &plan).unwrap();
    let original = plan.clone();
    for parameter in [
        "DLI,CBPAUP0C,PSBPAUTB",
        "BMP,OTHER,PSBPAUTB",
        "BMP,CBPAUP0C,OTHER",
    ] {
        plan = original.clone();
        plan.steps[0].parameter = Some(parameter.into());
        assert!(compare_original_plan(&prepared, &plan).is_err());
    }
    plan = original.clone();
    plan.steps[0]
        .dds
        .iter_mut()
        .find(|dd| dd.name == "SYSIN")
        .unwrap()
        .inline_data = b"05,00001,00001,Y\n".to_vec();
    assert!(compare_original_plan(&prepared, &plan).is_err());
    plan = original.clone();
    plan.steps[0].dds[0].dataset = Some("FOREIGN.LOADLIB".into());
    assert!(compare_original_plan(&prepared, &plan).is_err());
    plan = original.clone();
    plan.restart_step = Some("STEP01".into());
    assert!(compare_original_plan(&prepared, &plan).is_err());
    plan = original;
    plan.source.push('\n');
    assert!(compare_original_plan(&prepared, &plan).is_err());
}

#[test]
#[ignore = "requires the independently retained pinned CardDemo public source; zero journey credit"]
fn missing_changed_or_surplus_source_refuses_before_compilation() {
    let corpus = corpus();
    let files = PINS
        .iter()
        .map(|(path, _)| {
            (
                (*path).to_string(),
                read_corpus_file(&corpus, &corpus.join(path)).unwrap(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    validate_sources(&files).unwrap();
    for (path, _) in PINS {
        let mut changed = files.clone();
        changed.get_mut(path).unwrap()[0] ^= 1;
        assert!(validate_sources(&changed).is_err());
        let mut missing = files.clone();
        missing.remove(path);
        assert!(validate_sources(&missing).is_err());
    }
    let mut surplus = files;
    surplus.insert("foreign.cbl".into(), b"GOBACK.".to_vec());
    assert!(validate_sources(&surplus).is_err());
}

#[test]
#[ignore = "requires the independently retained pinned CardDemo public source; zero journey credit"]
fn selected_database_pcb1_cannot_be_relabelled_as_bmp_pcb2() {
    let catalog = ims_packages::metadata(&corpus()).unwrap();
    let mut psb = catalog
        .psbs
        .into_iter()
        .find(|psb| psb.name == "PSBPAUTB")
        .unwrap();
    assert_eq!(psb.pcbs.len(), 1);
    assert!(require_bmp_declarations(&psb).is_err());
    psb.pcbs.push(psb.pcbs[0].clone());
    // Two database PCBs still do not allocate an I/O PCB or establish BMP.
    assert!(require_bmp_declarations(&psb).is_err());
    psb.name = "FOREIGN".into();
    assert!(require_bmp_declarations(&psb).is_err());
}

#[test]
fn mixed_reference_preserves_whole_retained_summary_and_neighbor_bytes() {
    let expected = scenario::mixed().unwrap();
    assert_eq!(expected.current_yyddd, 26_289);
    assert_eq!(expected.before.len(), 3);
    assert_eq!(expected.before[0].data[..6], [0, 0, 0, 0, 5, 0x0c]);
    assert_eq!(expected.before[0].data[50..54], [0, 2, 0, 2]);
    assert_eq!(expected.before[0].data[54..60], [0, 0, 0, 0x12, 0x50, 0x0c]);
    assert_eq!(expected.remaining[0].data, expected.before[0].data);
    assert_eq!(expected.remaining[1], expected.before[2]);
    assert_eq!(
        expected.remaining[0].children,
        expected.before[0].children[..2]
    );
    expected.compare_hierarchy(&expected.remaining).unwrap();
    for root in 0..expected.remaining.len() {
        for offset in 0..expected.remaining[root].data.len() {
            let mut wrong = expected.remaining.clone();
            wrong[root].data[offset] ^= 1;
            assert!(expected.compare_hierarchy(&wrong).is_err());
        }
        for child in 0..expected.remaining[root].children.len() {
            for offset in 0..expected.remaining[root].children[child].len() {
                let mut wrong = expected.remaining.clone();
                wrong[root].children[child][offset] ^= 1;
                assert!(expected.compare_hierarchy(&wrong).is_err());
            }
        }
    }
    let mut wrong = expected.remaining.clone();
    wrong.reverse();
    assert!(expected.compare_hierarchy(&wrong).is_err());
    let mut wrong = expected.remaining.clone();
    wrong[0].children.reverse();
    assert!(expected.compare_hierarchy(&wrong).is_err());
    assert!(expected.compare_hierarchy(&[]).is_err());
}

// These isolated synthetic frames exercise the comparator, not a proposed full
// CBPAUP0C execution schedule or a successful hold/delete/recovery claim.
fn synthetic_reference(expected: &scenario::Scenario) -> Vec<scenario::Call> {
    let root = &expected.before[0];
    vec![
        scenario::Call {
            operation: ImsOperation::GetNext,
            pcb: 2,
            segments: vec!["PAUTSUM0".into()],
            data: Vec::new(),
            status: "  ".into(),
            returned: vec![ImsSegment {
                name: "PAUTSUM0".into(),
                parent_key: None,
                data: root.data.clone(),
            }],
            checkpoint: None,
            affected_segments: 0,
        },
        scenario::Call {
            operation: ImsOperation::GetNextParent,
            pcb: 2,
            segments: vec!["PAUTDTL1".into()],
            data: Vec::new(),
            status: "  ".into(),
            returned: vec![ImsSegment {
                name: "PAUTDTL1".into(),
                parent_key: Some(root.data[..6].to_vec()),
                data: root.children[2].clone(),
            }],
            checkpoint: None,
            affected_segments: 0,
        },
        scenario::Call {
            operation: ImsOperation::Delete,
            pcb: 2,
            segments: vec!["PAUTDTL1".into()],
            data: root.children[2].clone(),
            status: "  ".into(),
            returned: Vec::new(),
            checkpoint: None,
            affected_segments: 1,
        },
        scenario::Call {
            operation: ImsOperation::Checkpoint,
            pcb: 1,
            segments: Vec::new(),
            data: Vec::new(),
            status: "  ".into(),
            returned: Vec::new(),
            checkpoint: Some("RMAD0000".into()),
            affected_segments: 0,
        },
    ]
}
fn synthetic_calls(reference: &[scenario::Call]) -> Vec<(ImsRequest, ImsResult)> {
    reference
        .iter()
        .enumerate()
        .map(|(index, call)| {
            let names = call.segments.iter().map(String::as_str).collect::<Vec<_>>();
            let mut request = ims_request(
                call.operation,
                index as u64 + 1,
                None,
                &names,
                call.data.clone(),
                Vec::new(),
                call.checkpoint.as_deref(),
            )
            .unwrap();
            request.pcb = call.pcb;
            request.max_segments = 1;
            let result = ImsResult {
                status: call.status.clone(),
                segments: call.returned.clone(),
                checkpoint_id: call.checkpoint.clone(),
                affected_segments: call.affected_segments,
                system: None,
            };
            (request, result)
        })
        .collect()
}
fn compare(
    expected: &scenario::Scenario,
    reference: &[scenario::Call],
    actual: &[(ImsRequest, ImsResult)],
) -> Result<(), CorpusProblem> {
    expected.compare_calls(
        reference,
        &actual
            .iter()
            .map(|(request, result)| (request, result))
            .collect::<Vec<_>>(),
    )
}

#[test]
fn source_call_order_pcb_full_data_status_and_literal_checkpoint_are_required() {
    let expected = scenario::mixed().unwrap();
    let reference = synthetic_reference(&expected);
    let actual = synthetic_calls(&reference);
    compare(&expected, &reference, &actual).unwrap();
    assert!(compare(&expected, &[], &[]).is_err());
    assert!(compare(&expected, &reference, &actual[..actual.len() - 1]).is_err());
    let mut wrong = actual.clone();
    wrong.push(actual.last().unwrap().clone());
    assert!(compare(&expected, &reference, &wrong).is_err());
    let mut wrong = actual.clone();
    wrong.swap(0, 1);
    assert!(compare(&expected, &reference, &wrong).is_err());
    for index in 0..actual.len() {
        let mut wrong = actual.clone();
        wrong[index].0.pcb = if wrong[index].0.pcb == 1 { 2 } else { 1 };
        assert!(compare(&expected, &reference, &wrong).is_err());
        let mut wrong = actual.clone();
        wrong[index].1.status = "GE".into();
        if actual[index].1.status != "GE" {
            assert!(compare(&expected, &reference, &wrong).is_err());
        }
        let mut wrong = actual.clone();
        wrong[index].0.mutation = None;
        assert!(compare(&expected, &reference, &wrong).is_err());
        let mut wrong = actual.clone();
        wrong[index].1.affected_segments += 1;
        assert!(compare(&expected, &reference, &wrong).is_err());
        let mut wrong = actual.clone();
        wrong[index].0.psb = Some("PSBPAUTB".into());
        assert!(compare(&expected, &reference, &wrong).is_err());
        let mut wrong = actual.clone();
        wrong[index].0.max_segments = 2;
        assert!(compare(&expected, &reference, &wrong).is_err());
        for offset in 0..actual[index].0.data.len() {
            let mut wrong = actual.clone();
            wrong[index].0.data[offset] ^= 1;
            assert!(compare(&expected, &reference, &wrong).is_err());
        }
        for segment in 0..actual[index].1.segments.len() {
            for offset in 0..actual[index].1.segments[segment].data.len() {
                let mut wrong = actual.clone();
                wrong[index].1.segments[segment].data[offset] ^= 1;
                assert!(compare(&expected, &reference, &wrong).is_err());
            }
            let mut wrong = actual.clone();
            wrong[index].1.segments[segment].name = "FOREIGN".into();
            assert!(compare(&expected, &reference, &wrong).is_err());
            let mut wrong = actual.clone();
            wrong[index].1.segments[segment].parent_key = Some(b"FOREIGN".to_vec());
            assert!(compare(&expected, &reference, &wrong).is_err());
        }
    }
    let checkpoints = actual
        .iter()
        .enumerate()
        .filter(|(_, (r, _))| r.operation == ImsOperation::Checkpoint)
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    assert_eq!(checkpoints.len(), 1); // one synthetic frame, no golden frequency
    for index in checkpoints {
        assert_eq!(actual[index].0.checkpoint_id.as_deref(), Some("RMAD0000"));
        let mut wrong = actual.clone();
        wrong[index].0.checkpoint_id = Some("RMAD0001".into());
        assert!(compare(&expected, &reference, &wrong).is_err());
    }
    let mut foreign_reference = reference.clone();
    foreign_reference[0].segments = vec!["FOREIGN".into()];
    let foreign_calls = synthetic_calls(&foreign_reference);
    assert!(compare(&expected, &foreign_reference, &foreign_calls).is_err());
    foreign_reference = reference.clone();
    foreign_reference[0].operation = ImsOperation::GetHoldNext;
    assert!(
        compare(
            &expected,
            &foreign_reference,
            &synthetic_calls(&foreign_reference)
        )
        .is_err()
    );
    foreign_reference = reference.clone();
    foreign_reference[2].data.pop();
    assert!(
        compare(
            &expected,
            &foreign_reference,
            &synthetic_calls(&foreign_reference)
        )
        .is_err()
    );
}

#[test]
fn manual_credit_rewrite_and_checkpoint_reopen_do_not_fill_original_requirements() {
    let expected = scenario::mixed().unwrap();
    let mut rewritten = expected.remaining.clone();
    rewritten[0].data[50..54].copy_from_slice(&[0, 1, 0, 1]);
    // These are the local working-storage subtractions. The original body has
    // no REPL, so performing them from the wrapper would be a foreign write.
    assert!(expected.compare_hierarchy(&rewritten).is_err());
    assert_eq!(
        expected.source_contract_gaps(),
        ["available-credit adjustment", "restart from checkpoint"]
    );
    assert_eq!(
        LABELS,
        [
            "BMP scheduling",
            "GN/GNP/DLET",
            "CHKP",
            "available-credit adjustment",
            "restart from checkpoint",
            "exact remaining hierarchy"
        ]
    );
    let reference = synthetic_reference(&expected);
    let actual = synthetic_calls(&reference);
    compare(&expected, &reference, &actual).unwrap();
    // A copy of the same hierarchy after a reopen still supplies neither a
    // first-party persistent write nor application/process restart proof.
    expected
        .compare_hierarchy(&expected.remaining.clone())
        .unwrap();
    assert_eq!(expected.source_contract_gaps().len(), 2);
}
