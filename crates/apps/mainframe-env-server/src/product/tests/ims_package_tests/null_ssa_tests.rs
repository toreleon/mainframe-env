use super::*;
use mainframe_env_host_api::{ImsExecutionContext, ImsNavigationRequest};

#[test]
fn signed_selected_null_ssa_slots_preserve_public_selection_and_raw_identity() {
    let trust = Arc::new(test_package_trust());
    let server = ProductServer::memory_with_package_trust(config(), trust.clone()).unwrap();
    let run = "signed-null-ssa";
    let request = ImsNavigationRequest {
        request: carddemo_request(ImsOperation::GetHoldUnique, 3, vec![]),
        context: ImsExecutionContext::DbBatch,
        ssas: vec![b"PAUTSUM0*-O--(00010006EQ000001)".to_vec()],
    };
    let invocation = tm_invocation(run, "null-ssa");
    assert_eq!(
        server.ims_navigation_selected("CARDDEMO-IMS", &invocation, &request),
        Err(HostProblem::NotFound)
    );
    let installed = server
        .install_application_package_v2(&signed_carddemo_package(&trust, 1))
        .unwrap();
    server.publish_application_generation(&installed).unwrap();
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
            &invocation,
            &carddemo_request(ImsOperation::Schedule, 1, vec![]),
        )
        .unwrap();
    let mut data = vec![b'-'; 100];
    data[..6].copy_from_slice(b"000001");
    server
        .ims_execute_selected(
            "CARDDEMO-IMS",
            &invocation,
            &carddemo_request(ImsOperation::Insert, 2, data.clone()),
        )
        .unwrap();
    let got = server
        .ims_navigation_selected("CARDDEMO-IMS", &invocation, &request)
        .unwrap();
    assert_eq!(got.status, "  ");
    assert_eq!(got.segments[0].data, data);
    assert_eq!(
        server
            .ims_navigation_selected("CARDDEMO-IMS", &invocation, &request)
            .unwrap(),
        got
    );
    let before = server
        .store
        .list_provider_state_prefix("ims-", 4096)
        .unwrap();
    let mut conflict = request.clone();
    conflict.ssas = vec![b"PAUTSUM0*O(00010006EQ000001)".to_vec()];
    assert_eq!(
        server.ims_navigation_selected("CARDDEMO-IMS", &invocation, &conflict),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(
        server
            .store
            .list_provider_state_prefix("ims-", 4096)
            .unwrap(),
        before
    );
    let mut replacement = data;
    replacement[99] = b'+';
    assert_eq!(
        server
            .ims_execute_selected(
                "CARDDEMO-IMS",
                &invocation,
                &carddemo_request(ImsOperation::Replace, 4, replacement)
            )
            .unwrap()
            .status,
        "  "
    );
    let after_replace = server
        .store
        .list_provider_state_prefix("ims-", 4096)
        .unwrap();
    assert_eq!(
        server
            .ims_navigation_selected("CARDDEMO-IMS", &invocation, &request)
            .unwrap(),
        got
    );
    assert_eq!(
        server
            .store
            .list_provider_state_prefix("ims-", 4096)
            .unwrap(),
        after_replace
    );
}
