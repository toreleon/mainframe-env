use super::*;
use mainframe_env_host_api::DatasetResult;

#[test]
fn single_record_reply_with_more_restores_varying_length_and_status() {
    let mut machine = varying_file_machine();
    machine
        .implicit
        .insert("STATUS-X".into(), CobolValue::Bytes(b"99".to_vec()));
    machine.files.get_mut("VBFILE").unwrap().file_status = Some("STATUS-X".into());
    let Step::Effect(effect) = machine.dataset_effect("read", &["VBFILE".into()]).unwrap() else {
        panic!("expected dataset read");
    };
    assert!(matches!(
        effect.request,
        HostRequest::Dataset(DatasetRequest::Read { max_records: 1, .. })
    ));
    machine
        .resume_host(EffectResult {
            sequence: effect.sequence,
            outcome: Ok(HostResult::Dataset(DatasetResult::Records {
                records: vec![vec![b'A'; 12]],
                identities: vec![b"1".to_vec()],
                version: 1,
                more: true,
            })),
        })
        .unwrap();
    assert_eq!(machine.read("STATUS-X").unwrap(), b"00");
    assert_eq!(machine.read("WS-RECD-LEN").unwrap(), b"0012");
    assert_eq!(&machine.read("VBR-REC").unwrap()[..12], &[b'A'; 12]);
}

#[test]
fn sort_read_rejects_incomplete_whole_input_before_consuming_records() {
    let mut machine = varying_file_machine();
    let Step::Effect(effect) = machine
        .effect(
            HostRequest::Dataset(DatasetRequest::Read {
                dataset: DatasetName::new("SORT.INPUT", 44).unwrap(),
                member: None,
                key: None,
                max_records: 1,
                control: Default::default(),
            }),
            PendingKind::SortRead,
        )
        .unwrap()
    else {
        panic!("expected sort read")
    };
    assert_eq!(
        machine.resume_host(EffectResult {
            sequence: effect.sequence,
            outcome: Ok(HostResult::Dataset(DatasetResult::Records {
                records: vec![b"FIRST".to_vec()],
                identities: vec![Vec::new()],
                version: 1,
                more: true,
            })),
        }),
        Err(MachineProblem::Host(HostProblem::Condition {
            name: "DATASET_READ_TRUNCATED".into(),
            response: 16,
            response2: 0,
        }))
    );
    assert!(machine.sort_workspaces.is_empty());
}
