//! Manager interaction regressions for the shared SSA/index/integrity route.
use super::*;

#[test]
fn indexed_ssa_virtual_field_does_not_shadow_a_child_physical_field() {
    for sqlite in [false, true] {
        let store: Arc<dyn ProviderStateStore> = if sqlite {
            Arc::new(SqliteStateStore::open("sqlite::memory:", 4_194_304, 4_096).unwrap())
        } else {
            Arc::new(MemoryStore::new(Default::default()))
        };
        let service = ImsService::open(store, Default::default()).unwrap();
        let mut metadata = indexed_catalog(true, true);
        metadata.databases[0].segments[1]
            .fields
            .push(ImsFieldMetadata {
                name: Some("BYCHILD".into()),
                offset: 2,
                length: 1,
                sequence: false,
                unique: false,
            });
        seed_index_with_catalog(&service, metadata);
        let run = "virtual-field-scope";
        call(
            &service,
            run,
            &request(run, ImsOperation::Schedule, 1, &[], b""),
        );
        let selected = nav(
            run,
            2,
            ImsOperation::GetUnique,
            2,
            &[b"ROOT    (BYCHILD EQZA)", b"CHILD   (BYCHILD EQA)"],
        );
        let result = public(service.clone(), run, selected.clone()).unwrap();
        assert_eq!(result.status, "  ");
        assert_eq!(result.segments[0].data, b"C2AZ");
        assert_eq!(public(service.clone(), run, selected).unwrap(), result);
        let position = pcb::position(&service.lock().unwrap().state.sessions[run], 2);
        assert_eq!(position.current(), position.parentage());
        assert_eq!(serde_json::to_value(position).unwrap()["parentage"], 4);
        let with_root_parentage = public(
            service.clone(),
            run,
            nav(
                run,
                3,
                ImsOperation::GetUnique,
                2,
                &[b"ROOT    *P(BYCHILD EQZA)", b"CHILD   (BYCHILD EQA)"],
            ),
        )
        .unwrap();
        assert_eq!(with_root_parentage.status, "  ");
        assert_eq!(with_root_parentage.segments[0].data, b"C2AZ");
        let position = pcb::position(&service.lock().unwrap().state.sessions[run], 2);
        assert_ne!(position.current(), position.parentage());
        assert_eq!(serde_json::to_value(position).unwrap()["parentage"], 3);
        let missing = public(
            service.clone(),
            run,
            nav(
                run,
                4,
                ImsOperation::GetUnique,
                2,
                &[b"ROOT    (BYCHILD EQZA)", b"CHILD   (BYCHILD EQZ)"],
            ),
        )
        .unwrap();
        assert_eq!(missing.status, "GE");
        assert!(missing.segments.is_empty());
    }
}
