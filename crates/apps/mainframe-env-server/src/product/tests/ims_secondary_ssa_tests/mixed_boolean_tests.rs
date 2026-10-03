//! Signed selected-route guard for remaining unsupported independent classes.
use super::*;

fn reject_mixed(server: &ProductServer) {
    let run = "signed-composite";
    let before = [
        "ims-v1-generic-database",
        "ims-v1-session-index",
        "ims-v1-replay",
        "ims-v1-generic-unit-of-work",
    ]
    .map(|namespace| {
        server
            .store
            .list_provider_state(namespace, 262_144)
            .unwrap()
    });
    for selected in [1, 3] {
        for (first, second) in [('&', '#'), ('#', '*'), ('#', '|'), ('+', '#')] {
            let mut request = carddemo_request(ImsOperation::GetHoldNext, 20, vec![]);
            request.pcb = selected;
            request.segments.clear();
            let raw = if selected == 1 {
                let mut raw = b"PAUTSUM0*O(00070001LT".to_vec();
                raw.push(255);
                raw.extend(format!("{first}00070001GE").bytes());
                raw.push(0);
                raw.extend(format!("{second}00070001NE").bytes());
                raw.extend([255, b')']);
                raw
            } else {
                format!("PAUTSUM0(BYVALUE GEAZ{first}BYVALUE LEZA{second}BYVALUE NE??)")
                    .into_bytes()
            };
            let nav = ImsNavigationRequest {
                request,
                context: ImsExecutionContext::DbBatch,
                ssas: vec![raw],
            };
            assert_eq!(
                server.ims_navigation_selected(
                    "CARDDEMO-IMS",
                    &tm_invocation(run, "mixed-gap"),
                    &nav
                ),
                Err(HostProblem::Unsupported)
            );
            let after = [
                "ims-v1-generic-database",
                "ims-v1-session-index",
                "ims-v1-replay",
                "ims-v1-generic-unit-of-work",
            ]
            .map(|namespace| {
                server
                    .store
                    .list_provider_state(namespace, 262_144)
                    .unwrap()
            });
            assert_eq!(after, before);
        }
    }
    // Existing selected pointer, hold/replay and distinct source/target remain
    // live; the rejected calls did not consume the next occurrence.
    let mut request = carddemo_request(ImsOperation::GetNext, 21, vec![]);
    request.pcb = 3;
    request.segments.clear();
    let nav = ImsNavigationRequest {
        request,
        context: ImsExecutionContext::DbBatch,
        ssas: vec![b"PAUTSUM0 ".to_vec()],
    };
    let result = server
        .ims_navigation_selected("CARDDEMO-IMS", &tm_invocation(run, "after-mixed-gap"), &nav)
        .unwrap();
    assert_eq!(&result.segments[0].data[..6], b"000001");
    assert_eq!(
        server
            .ims_navigation_selected("CARDDEMO-IMS", &tm_invocation(run, "after-mixed-gap"), &nav)
            .unwrap(),
        result
    );
}

#[test]
fn mixed_boolean_signed_selected_no_flatten_memory() {
    let trust = Arc::new(test_package_trust());
    let server = ProductServer::memory_with_package_trust(config(), trust.clone()).unwrap();
    composite(&server, &trust);
    reject_mixed(&server);
}

#[test]
fn mixed_boolean_signed_selected_no_flatten_sqlite_reopen() {
    let file = std::env::temp_dir().join(format!(
        "ims-signed-mixed-{}-{}.sqlite",
        std::process::id(),
        NEXT_SECONDARY_FILE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    let mut settings = config();
    settings.store_profile = crate::StoreProfile::Sqlite;
    settings.sqlite_url = url.clone();
    let trust = Arc::new(test_package_trust());
    let open = || {
        ProductServer::open_with_package_trust(
            settings.clone(),
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap()),
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
            trust.clone(),
        )
        .unwrap()
    };
    let server = open();
    composite(&server, &trust);
    drop(server);
    let server = open();
    reject_mixed(&server);
    drop(server);
    std::fs::remove_file(file).unwrap();
}
