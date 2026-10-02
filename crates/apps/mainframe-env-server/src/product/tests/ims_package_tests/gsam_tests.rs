use super::*;
use mainframe_env_host_api::{ImsExecutionContext, ImsGsamRequest, ImsGsamSearchArgument};

#[test]
fn selected_signed_package_gsam_roundtrip_preserves_selection_and_authority() {
    let trust = Arc::new(test_package_trust());
    let server = ProductServer::memory_with_package_trust(config(), trust.clone()).unwrap();
    let run = "selected-gsam";
    let make = |op, seq, pcb, data: Vec<u8>| {
        let mut request = carddemo_request(op, seq, data);
        request.segments.clear();
        request.pcb = pcb;
        ImsGsamRequest {
            undefined_length: None,
            request,
            context: ImsExecutionContext::DbBatch,
            search: None,
            save_address: op != ImsOperation::GetUnique,
        }
    };
    let insert = make(ImsOperation::Insert, 2, 2, vec![0xff; 100]);
    assert_eq!(
        server.ims_gsam_selected("CARDDEMO-IMS", &tm_invocation(run, "missing"), &insert),
        Err(HostProblem::NotFound)
    );
    let mut package = signed_carddemo_package(&trust, 1);
    let catalog = package.sections.ims_metadata.as_mut().unwrap();
    catalog.databases[0].organization = ImsDatabaseOrganization::Gsam;
    catalog.databases[0].segments.truncate(1);
    catalog.databases[0].segments[0].fields.clear();
    let alternate = catalog.psbs[0].pcbs[1].clone();
    let ImsPcbMetadata::Database(pcb) = &mut catalog.psbs[0].pcbs[0] else {
        unreachable!()
    };
    pcb.processing_options = "G".into();
    pcb.sensitive_segments.truncate(1);
    let mut output = pcb.clone();
    output.name = "OUTPUT".into();
    output.processing_options = "L".into();
    catalog.psbs[0].pcbs = vec![
        ImsPcbMetadata::Database(pcb.clone()),
        ImsPcbMetadata::Database(output),
        alternate,
    ];
    resign_package(&mut package, &trust);
    let staged = server.install_application_package_v2(&package).unwrap();
    server.publish_application_generation(&staged).unwrap();
    server
        .bootstrap_administrator("IBMUSER", b"TESTPASS")
        .unwrap();
    for (class, name) in [("IMSPSB", "PSBPAUTB"), ("IMSDB", "DBPAUTP0")] {
        server
            .racf
            .define_profile(class, name, "IBMUSER", Some(AccessIntent::Control))
            .unwrap();
    }
    server
        .ims_execute_selected(
            "CARDDEMO-IMS",
            &tm_invocation(run, "schedule"),
            &carddemo_request(ImsOperation::Schedule, 1, vec![]),
        )
        .unwrap();
    let inserted = server
        .ims_gsam_selected("CARDDEMO-IMS", &tm_invocation(run, "insert"), &insert)
        .unwrap();
    assert_eq!(inserted.result.affected_segments, 1);
    let found = server
        .ims_gsam_selected(
            "CARDDEMO-IMS",
            &tm_invocation(run, "gn"),
            &make(ImsOperation::GetNext, 3, 1, vec![]),
        )
        .unwrap();
    assert_eq!(found.result.segments[0].data, vec![0xff; 100]);
    assert_eq!(found.address, inserted.address);
    let mut gu = make(ImsOperation::GetUnique, 4, 1, vec![]);
    gu.search = Some(ImsGsamSearchArgument::Record(found.address.unwrap()));
    assert_eq!(
        server
            .ims_gsam_selected("CARDDEMO-IMS", &tm_invocation(run, "gu"), &gu)
            .unwrap()
            .result
            .segments[0]
            .data,
        vec![0xff; 100]
    );
    assert_eq!(
        server.ims_gsam_selected("CARDDEMO-IMS", &tm_invocation(run, "replay"), &insert),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(
        server
            .ims_gsam_selected("CARDDEMO-IMS", &tm_invocation(run, "insert"), &insert)
            .unwrap(),
        inserted
    );
    let before = server
        .store
        .get_provider_state("ims-v1-generic-database", "DBPAUTP0")
        .unwrap();
    let forbidden = make(ImsOperation::Insert, 5, 1, vec![0; 100]);
    assert_eq!(
        server.ims_gsam_selected("CARDDEMO-IMS", &tm_invocation(run, "forbidden"), &forbidden),
        Err(HostProblem::Unsupported)
    );
    assert_eq!(
        server
            .store
            .get_provider_state("ims-v1-generic-database", "DBPAUTP0")
            .unwrap(),
        before
    );
}
