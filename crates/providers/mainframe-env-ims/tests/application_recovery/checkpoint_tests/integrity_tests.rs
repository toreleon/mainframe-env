//! XRST's internal qualified GU must obey normal PCB integrity visibility.
use super::*;

#[test]
fn restart_qualified_gu_cannot_observe_foreign_pending_undo() {
    backends("checkpoint-integrity", |store| {
        let service = open(store.clone());
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
        service
            .execute(&invocation, &get(1, 21, b"02", false))
            .unwrap();
        invoke_call(
            &service,
            &store,
            &invocation,
            2,
            ImsRecoveryCall::SymbolicCheckpoint {
                id: "READPOS".into(),
                user_areas: vec![],
            },
        );
        let mut writer = invocation.clone();
        writer.run_unit_id =
            RunUnitId::new("foreign-restart-writer", InvocationLimits::default()).unwrap();
        writer.execution_id =
            ExecutionId::new("foreign-restart-execution", InvocationLimits::default()).unwrap();
        writer.service_class = ServiceClass::Interactive;
        service
            .execute(&writer, &database_request(ImsOperation::Schedule, 101, &[]))
            .unwrap();
        service.execute(&writer, &get(1, 102, b"02", true)).unwrap();
        service
            .execute(
                &writer,
                &database_request(ImsOperation::Replace, 103, b"02Z"),
            )
            .unwrap();
        let restarted = next_execution(&invocation, "restart-visibility");
        let request = call(
            3,
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Checkpoint("READPOS".into()),
                area_lengths: vec![],
            },
        );
        intent(&*store, &restarted, &request);
        let before = snapshot(&*store);
        assert_eq!(
            dispatch(service, store.clone(), &restarted, &request),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(snapshot(&*store), before);
    });
}
