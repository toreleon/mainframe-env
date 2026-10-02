//! A secondary cursor must not become a primary-order checkpoint position.
use super::*;

#[test]
fn symbolic_checkpoint_rejects_unimplemented_selected_secondary_resume_without_mutation() {
    backends("checkpoint-secondary", |store| {
        let mut metadata = catalog();
        metadata.databases[0].secondary_indexes = vec![ImsSecondaryIndexMetadata {
            name: "BYKEY".into(),
            source_segment: "ROOT".into(),
            target_segment: "ROOT".into(),
            source_fields: vec!["KEY".into()],
        }];
        let ImsPcbMetadata::Database(mut indexed) = metadata.psbs[0].pcbs[0].clone() else {
            panic!()
        };
        indexed.name = "INDEXPCB".into();
        indexed.secondary_index = Some("BYKEY".into());
        metadata.psbs[0]
            .pcbs
            .push(ImsPcbMetadata::Database(indexed));
        let service = open_catalog(store.clone(), metadata);
        let invocation = invocation();
        seed(&service, &invocation);
        service
            .execute(
                &invocation,
                &database_request(ImsOperation::Commit, 20, &[]),
            )
            .unwrap();
        invoke_call(
            &service,
            &store,
            &invocation,
            1,
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Normal,
                area_lengths: vec![],
            },
        );
        let mut read = database_request(ImsOperation::GetNext, 21, &[]);
        read.pcb = 2;
        assert_eq!(
            service.execute(&invocation, &read).unwrap().segments[0].data,
            b"01A"
        );
        let request = call(
            2,
            ImsRecoveryCall::SymbolicCheckpoint {
                id: "INDEXPOS".into(),
                user_areas: vec![],
            },
        );
        intent(&*store, &invocation, &request);
        let before = snapshot(&*store);
        assert_eq!(
            dispatch(service, store.clone(), &invocation, &request),
            Err(HostProblem::Unsupported)
        );
        assert_eq!(snapshot(&*store), before);
    });
}
