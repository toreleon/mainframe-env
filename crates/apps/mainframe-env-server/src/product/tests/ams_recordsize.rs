use super::*;

fn define_ksds() -> (Arc<ProductServer>, DatasetName) {
    let server = ProductServer::memory(config()).unwrap();
    server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
    server
        .ams(
            "IBMUSER",
            b"DEFINE CLUSTER (NAME(IBMUSER.RECSIZE) INDEXED KEYS(8 0) RECORDSIZE(80 120))",
        )
        .unwrap();
    (server, DatasetName::new("IBMUSER.RECSIZE", 128).unwrap())
}

#[test]
fn define_ksds_recordsize_records_the_declared_maximum() {
    let (server, dataset) = define_ksds();
    let DatasetResult::Attributes { attributes, .. } = server
        .dataset
        .invoke(DatasetRequest::Attributes { dataset })
        .unwrap()
    else {
        panic!("expected defined KSDS attributes");
    };
    assert_eq!(attributes.logical_record_length, 120);
}

#[test]
fn define_ksds_recordsize_rejects_write_above_the_declared_maximum() {
    let (server, dataset) = define_ksds();
    server
        .dataset
        .invoke(DatasetRequest::Write {
            dataset: dataset.clone(),
            member: None,
            records: vec![vec![b'A'; 120]],
            expected_version: None,
            mutation: server.mutation().unwrap(),
        })
        .unwrap();
    let result = server.dataset.invoke(DatasetRequest::Write {
        dataset,
        member: None,
        records: vec![vec![b'B'; 121]],
        expected_version: None,
        mutation: server.mutation().unwrap(),
    });
    assert!(
        matches!(
            result,
            Err(HostProblem::Condition {
                ref name,
                response: 22,
                response2: 0,
            }) if name == "LENGERR"
        ),
        "a 121-byte WRITE must exceed RECORDSIZE(80 120): {result:?}"
    );
}

#[test]
fn define_ksds_recordsize_omission_uses_idcams_4089_default() {
    let server = ProductServer::memory(config()).unwrap();
    server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
    server
        .ams(
            "IBMUSER",
            b"DEFINE CLUSTER (NAME(IBMUSER.DEFAULT) INDEXED KEYS(8 0))",
        )
        .unwrap();
    let DatasetResult::Attributes { attributes, .. } = server
        .dataset
        .invoke(DatasetRequest::Attributes {
            dataset: DatasetName::new("IBMUSER.DEFAULT", 128).unwrap(),
        })
        .unwrap()
    else {
        panic!("expected defined KSDS attributes");
    };
    assert_eq!(attributes.logical_record_length, 4089);
}

#[test]
fn define_ksds_recordsize_rejects_rewrite_above_the_declared_maximum() {
    let (server, dataset) = define_ksds();
    server
        .dataset
        .invoke(DatasetRequest::Write {
            dataset: dataset.clone(),
            member: None,
            records: vec![vec![b'A'; 120]],
            expected_version: None,
            mutation: server.mutation().unwrap(),
        })
        .unwrap();
    let result = server.dataset.invoke(DatasetRequest::RewriteRecord {
        dataset: dataset.clone(),
        key: vec![b'A'; 8],
        record: vec![b'A'; 121],
        expected_version: None,
        mutation: server.mutation().unwrap(),
    });
    assert!(
        matches!(result, Err(HostProblem::Condition { ref name, response: 22, response2: 0 }) if name == "LENGERR"),
        "overlength REWRITE: {result:?}"
    );
    let DatasetResult::Records { records, .. } = server
        .dataset
        .invoke(DatasetRequest::Read {
            dataset,
            member: None,
            key: None,
            max_records: 1,
            control: Default::default(),
        })
        .unwrap()
    else {
        panic!("expected records");
    };
    assert_eq!(records, vec![vec![b'A'; 120]]);
}

#[test]
fn define_ksds_recordsize_malformed_operand_does_not_create_a_dataset() {
    let server = ProductServer::memory(config()).unwrap();
    server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
    for operand in [
        "RECORDSIZE(80)",
        "RECORDSIZE(120 80)",
        "RECORDSIZE(0 120)",
        "RECORDSIZE(80 X)",
        "RECORDSIZE(80 120",
    ] {
        let control = format!("DEFINE CLUSTER (NAME(IBMUSER.INVALID) INDEXED KEYS(8 0)) {operand}");
        assert!(
            server.ams("IBMUSER", control.as_bytes()).is_err(),
            "{operand}"
        );
        assert_eq!(
            server.dataset.invoke(DatasetRequest::Attributes {
                dataset: DatasetName::new("IBMUSER.INVALID", 128).unwrap(),
            }),
            Err(HostProblem::NotFound)
        );
    }
}

#[test]
fn define_other_cluster_types_keep_existing_recordsize_behavior() {
    let server = ProductServer::memory(config()).unwrap();
    server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
    for (index, kind) in ["NONINDEXED", "NUMBERED", "LINEAR"].into_iter().enumerate() {
        let name = format!("IBMUSER.OTHER{index}");
        let control = format!("DEFINE CLUSTER (NAME({name}) {kind} RECORDSIZE(80 120))");
        server.ams("IBMUSER", control.as_bytes()).unwrap();
        let DatasetResult::Attributes { attributes, .. } = server
            .dataset
            .invoke(DatasetRequest::Attributes {
                dataset: DatasetName::new(name, 128).unwrap(),
            })
            .unwrap()
        else {
            panic!("expected defined cluster attributes");
        };
        assert_eq!(attributes.logical_record_length, 32760, "{kind}");
    }
}
