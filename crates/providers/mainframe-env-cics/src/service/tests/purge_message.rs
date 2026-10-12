use super::*;

fn accumulate(cics: &CicsService, invocation: &Invocation, sequence: u64) {
    let accum = request(
        CicsOperation::SendControl,
        BTreeMap::from([
            ("OPTION.ACCUM".into(), cics_option()),
            ("OPTION.PAGING".into(), cics_option()),
            ("OPTION.ALARM".into(), cics_option()),
        ]),
        sequence,
    );
    cics.invoke(
        &effect(&invocation.run_unit_id, accum.clone(), sequence),
        accum,
    )
    .unwrap();
}

#[test]
fn purge_message_preserves_pages_of_already_completed_messages() {
    // CICS TS 6.x dfhp4_purgemessage / dfhp37j: only the current incomplete
    // logical message is deleted. These two SEND PAGEs complete separate messages.
    let cics = service(Arc::new(MemoryStore::new(Default::default())));
    let (invocation, session) = registered(&cics);
    for sequence in [1, 3] {
        accumulate(&cics, &invocation, sequence);
        let page = request(CicsOperation::SendPage, BTreeMap::new(), sequence + 1);
        cics.invoke(
            &effect(&invocation.run_unit_id, page.clone(), sequence + 1),
            page,
        )
        .unwrap();
    }
    accumulate(&cics, &invocation, 5);
    let before = cics.terminal_control_snapshot(&session).unwrap();
    assert_eq!(before.queued_pages, 2);
    let purge = request(CicsOperation::PurgeMessage, BTreeMap::new(), 6);
    cics.invoke(&effect(&invocation.run_unit_id, purge.clone(), 6), purge)
        .unwrap();
    let after = cics.terminal_control_snapshot(&session).unwrap();
    assert!(!after.pending_logical_message);
    assert_eq!((after.queued_pages, after.alarm_count), (2, 2));
    assert_eq!(after.last_page, before.last_page);
}

#[test]
fn purge_message_corrupt_bms_storage_fails_closed_without_fabricating_tsioerr() {
    // The provider has no deterministic BMS temporary-storage I/O injection.
    // Corrupt platform storage is infrastructure failure, not IBM TSIOERR 35.
    let store = Arc::new(MemoryStore::new(Default::default()));
    let cics = service(store.clone());
    let (invocation, session) = registered(&cics);
    accumulate(&cics, &invocation, 1);
    let mut corrupt = store
        .get_provider_state("cics-terminal-bms-v1", session.as_str())
        .unwrap()
        .unwrap();
    let version = corrupt.version;
    corrupt.version += 1;
    corrupt.payload = b"invalid-bms-record".to_vec();
    store
        .put_provider_state(corrupt.clone(), Some(version))
        .unwrap();
    let mut purge = request(CicsOperation::PurgeMessage, BTreeMap::new(), 2);
    purge.condition_policy = CicsConditionPolicy::Respond {
        response_field: "RC".into(),
        response2_field: Some("RC2".into()),
    };
    assert_eq!(
        cics.invoke(&effect(&invocation.run_unit_id, purge.clone(), 2), purge),
        Err(HostProblem::InfrastructureFailure)
    );
    assert_eq!(
        store
            .get_provider_state("cics-terminal-bms-v1", session.as_str())
            .unwrap(),
        Some(corrupt)
    );
    assert!(
        store
            .get_provider_state("cics-terminal-bms-receipt-v1", "outer-2")
            .unwrap()
            .is_none()
    );
}

#[test]
fn purge_message_authorization_denial_preserves_pending_message() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let allowed = service(store.clone());
    let (invocation, session) = registered(&allowed);
    accumulate(&allowed, &invocation, 1);
    let before = store
        .get_provider_state("cics-terminal-bms-v1", session.as_str())
        .unwrap();
    drop(allowed);
    let (authorities, _) = command_authorities(true);
    let denied = CicsService::open(authorities, store.clone(), CicsLimits::default()).unwrap();
    denied
        .register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
        .unwrap();
    let purge = request(CicsOperation::PurgeMessage, BTreeMap::new(), 2);
    assert_eq!(
        denied.invoke(&effect(&invocation.run_unit_id, purge.clone(), 2), purge),
        Err(HostProblem::Unauthorized)
    );
    assert_eq!(
        store
            .get_provider_state("cics-terminal-bms-v1", session.as_str())
            .unwrap(),
        before
    );
    assert!(
        store
            .get_provider_state("cics-terminal-bms-receipt-v1", "outer-2")
            .unwrap()
            .is_none()
    );
}
